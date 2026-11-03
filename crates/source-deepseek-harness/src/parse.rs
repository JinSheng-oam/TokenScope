use crate::{
    inventory::{bounded_line, clean_title, json_line, json_time, open_log_reader, parse_header},
    model::{IndexedRecord, ModelRoute, ParsedSession, ReplacementSlot, UsageFold},
    DeepSeekHarnessError, Result, MAX_EVENT_ROWS_PER_SESSION, MAX_RECORDS_PER_SESSION, TOOL_ID,
};
use serde_json::Value;
use std::{io::BufReader, path::Path};
use tokenscope_core_domain::{
    stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};

struct RecordContext<'a> {
    session_key_value: &'a str,
    device_id: &'a str,
    project_id: Option<&'a str>,
}

struct SettlementSample {
    model_id: Option<String>,
    event_at_ms: Option<i64>,
    usage: TokenBreakdown,
    turn: u64,
    step: u64,
    sequence: u64,
    legacy_chunk: bool,
}

struct IndependentUsageSample {
    model_id: Option<String>,
    event_at_ms: Option<i64>,
    usage: TokenBreakdown,
    sequence: u64,
}

pub(super) fn user_message_title(data: &Value) -> Option<String> {
    let message = data.get("message").unwrap_or(data);
    if message
        .get("source")
        .and_then(|source| source.get("kind"))
        .and_then(Value::as_str)
        != Some("user")
    {
        return None;
    }
    let content = message.get("content")?;
    if let Some(text) = content.as_str() {
        return clean_title(text);
    }
    content
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .find_map(|block| {
            block
                .get("text")
                .and_then(Value::as_str)
                .and_then(clean_title)
        })
}

fn route_from_config(value: &Value) -> Option<ModelRoute> {
    let provider = value
        .get("provider")
        .and_then(Value::as_str)
        .filter(|provider| {
            !provider.trim().is_empty()
                && provider.len() <= 128
                && !provider.chars().any(char::is_control)
        })?;
    let model = value.get("model").and_then(Value::as_str).filter(|model| {
        !model.trim().is_empty() && model.len() <= 256 && !model.chars().any(char::is_control)
    })?;
    Some(ModelRoute {
        provider: provider.to_owned(),
        model: model.to_owned(),
    })
}

fn route_from_message(data: &Value) -> Option<ModelRoute> {
    let message = data.get("message")?;
    let source = message.get("source")?;
    route_from_config(source)
}

fn route_from_event(event_type: &str, data: &Value) -> Option<ModelRoute> {
    match event_type {
        "request/context" => route_from_config(data),
        "request/header" => data
            .get("header")
            .and_then(|header| header.get("config"))
            .and_then(route_from_config),
        _ => None,
    }
}

fn parse_counter(usage: &Value, name: &str, required: bool) -> Result<u64> {
    match usage.get(name) {
        Some(value) if value.is_null() && !required => Ok(0),
        Some(value) => value
            .as_u64()
            .ok_or(DeepSeekHarnessError::Invalid("invalid token counter")),
        None if required => Err(DeepSeekHarnessError::Invalid("missing token counter")),
        None => Ok(0),
    }
}

fn parse_usage(usage: &Value) -> Result<TokenBreakdown> {
    let result = TokenBreakdown {
        input_uncached_tokens: parse_counter(usage, "inputTokens", true)?,
        cache_read_tokens: parse_counter(usage, "cacheReadTokens", false)?,
        cache_write_tokens: parse_counter(usage, "cacheWriteTokens", false)?,
        output_tokens: parse_counter(usage, "outputTokens", true)?,
        unclassified_tokens: 0,
        reasoning_tokens: usage
            .get("reasoningTokens")
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_u64()
                    .ok_or(DeepSeekHarnessError::Invalid("invalid reasoning counter"))
            })
            .transpose()?,
    };
    result.validate()?;
    if result.total()? > i64::MAX as u64 {
        return Err(DeepSeekHarnessError::Limit(
            "tokens exceed SQLite integer range",
        ));
    }
    Ok(result)
}

fn last_stream_usage(data: &Value) -> Result<Option<(TokenBreakdown, Option<i64>)>> {
    let Some(stream) = data.get("stream").and_then(Value::as_array) else {
        return Ok(None);
    };
    for record in stream.iter().rev() {
        let chunk = if record.get("type").and_then(Value::as_str) == Some("chunk") {
            record.get("chunk")
        } else if record.get("type").and_then(Value::as_str) == Some("usage") {
            Some(record)
        } else {
            None
        };
        let Some(chunk) =
            chunk.filter(|chunk| chunk.get("type").and_then(Value::as_str) == Some("usage"))
        else {
            continue;
        };
        let usage = chunk
            .get("usage")
            .ok_or(DeepSeekHarnessError::Invalid("usage chunk lacks counters"))?;
        return Ok(Some((parse_usage(usage)?, json_time(record.get("time")))));
    }
    Ok(None)
}

fn settlement_usage(
    event_type: &str,
    data: &Value,
) -> Result<Option<(TokenBreakdown, Option<i64>)>> {
    if event_type == "assistant/message" {
        if let Some(usage) = data.get("usage").filter(|usage| !usage.is_null()) {
            return Ok(Some((parse_usage(usage)?, None)));
        }
    }
    last_stream_usage(data)
}

fn event_sequence(value: &Value, fallback: u64) -> u64 {
    value
        .get("seq")
        .or_else(|| value.get("seq0"))
        .and_then(Value::as_u64)
        .unwrap_or(fallback)
}

fn make_record(
    device_id: &str,
    session_key_value: &str,
    project_id: Option<&str>,
    model_id: Option<String>,
    event_at_ms: Option<i64>,
    usage: TokenBreakdown,
    native_identity: &[u8],
) -> Result<CanonicalRecord> {
    Ok(CanonicalRecord {
        record_id: stable_record_id(TOOL_ID, session_key_value.as_bytes(), native_identity)?,
        owner_device_id: device_id.to_owned(),
        tool_id: TOOL_ID.to_owned(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Exact,
        account_id: None,
        model_id,
        project_id: project_id.map(str::to_owned),
        session_id: Some(session_key_value.to_owned()),
        event_at_ms,
        usage,
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    }
    .normalized()?)
}

impl UsageFold {
    fn push_settlement(
        &mut self,
        context: &RecordContext<'_>,
        sample: SettlementSample,
    ) -> Result<()> {
        let SettlementSample {
            model_id,
            event_at_ms,
            usage,
            turn,
            step,
            sequence,
            legacy_chunk,
        } = sample;
        let (record_index, source_seq, native_identity) = match self.last_slot {
            Some(slot) if slot.turn == turn && slot.step == step => (
                slot.record_index,
                slot.source_seq,
                format!("assistant-settlement:{}:{}", turn, slot.source_seq).into_bytes(),
            ),
            _ => (
                self.records.len(),
                sequence,
                format!("assistant-settlement:{turn}:{sequence}").into_bytes(),
            ),
        };
        let record = make_record(
            context.device_id,
            context.session_key_value,
            context.project_id,
            model_id,
            event_at_ms,
            usage,
            &native_identity,
        )?;
        if record_index == self.records.len() {
            if self.records.len() >= MAX_RECORDS_PER_SESSION {
                return Err(DeepSeekHarnessError::Limit("usage rows per session"));
            }
            self.records.push(IndexedRecord { source_seq, record });
        } else {
            self.records[record_index].record = record;
        }
        self.last_slot = Some(ReplacementSlot {
            turn,
            step,
            record_index,
            source_seq,
            legacy_chunk,
        });
        Ok(())
    }

    fn update_last_metadata(
        &mut self,
        turn: u64,
        step: u64,
        model_id: Option<String>,
        event_at_ms: Option<i64>,
    ) {
        let Some(slot) = self
            .last_slot
            .filter(|slot| slot.turn == turn && slot.step == step)
        else {
            return;
        };
        let record = &mut self.records[slot.record_index].record;
        if model_id.is_some() {
            record.model_id = model_id;
        }
        if event_at_ms.is_some() {
            record.event_at_ms = event_at_ms;
        }
    }

    fn push_independent(
        &mut self,
        context: &RecordContext<'_>,
        sample: IndependentUsageSample,
    ) -> Result<()> {
        let IndependentUsageSample {
            model_id,
            event_at_ms,
            usage,
            sequence,
        } = sample;
        if self.records.len() >= MAX_RECORDS_PER_SESSION {
            return Err(DeepSeekHarnessError::Limit("usage rows per session"));
        }
        let native_identity = format!("independent-call:{sequence}");
        let record = make_record(
            context.device_id,
            context.session_key_value,
            context.project_id,
            model_id,
            event_at_ms,
            usage,
            native_identity.as_bytes(),
        )?;
        self.records.push(IndexedRecord {
            source_seq: sequence,
            record,
        });
        Ok(())
    }
}

pub(super) fn parse_session(
    path: &Path,
    compressed: bool,
    expected_id: &str,
    session_key_value: &str,
    device_id: &str,
    project_id: Option<&str>,
) -> Result<ParsedSession> {
    let mut reader = BufReader::new(open_log_reader(path, compressed)?);
    let record_context = RecordContext {
        session_key_value,
        device_id,
        project_id,
    };
    let mut line_number = 0usize;
    let mut event_rows = 0usize;
    let mut fallback_sequence = 0u64;
    let mut header = None;
    let mut title = None;
    let mut route: Option<ModelRoute> = None;
    let mut fold = UsageFold::default();
    let mut own_start_sequence = 0u64;
    let mut seed_boundary_seen = false;
    let mut format_version = 0u64;

    while let Some((line, complete)) = bounded_line(&mut reader)? {
        if !complete {
            return Err(DeepSeekHarnessError::Pending);
        }
        let value = json_line(line)?;
        if line_number == 0 {
            let parsed_header = parse_header(&value)?;
            if parsed_header.id != expected_id {
                return Err(DeepSeekHarnessError::Invalid("session identity changed"));
            }
            format_version = parsed_header.version;
            if let Some(seed_length) = parsed_header.seed_length {
                own_start_sequence = seed_length;
                seed_boundary_seen = true;
            }
            if !parsed_header.is_seeded {
                seed_boundary_seen = true;
            }
            header = Some(parsed_header);
            line_number += 1;
            continue;
        }
        line_number = line_number.saturating_add(1);
        event_rows = event_rows.saturating_add(1);
        if event_rows > MAX_EVENT_ROWS_PER_SESSION {
            return Err(DeepSeekHarnessError::Limit("event rows per session"));
        }
        let sequence = event_sequence(&value, fallback_sequence);
        fallback_sequence = sequence.saturating_add(1);
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        let data = value.get("data").unwrap_or(&Value::Null);
        let event_time = json_time(value.get("time"))
            .or_else(|| header.as_ref().and_then(|header| header.created_at_ms));

        if let Some(next_route) = route_from_event(event_type, data) {
            route = Some(next_route);
        }
        if event_type == "session/end-seed"
            && data.get("inherited").and_then(Value::as_bool) == Some(true)
        {
            own_start_sequence = sequence;
            seed_boundary_seen = true;
            fold.last_slot = None;
            continue;
        }
        if title.is_none() && event_type == "user/message" {
            title = user_message_title(data);
        }
        if event_type == "llm/retry-started" || event_type == "llm/retry" {
            let turn = data.get("turn").and_then(Value::as_u64);
            let step = data.get("step").and_then(Value::as_u64);
            if fold
                .last_slot
                .is_some_and(|slot| Some(slot.turn) == turn && Some(slot.step) == step)
            {
                fold.last_slot = None;
            }
            continue;
        }

        if sequence < own_start_sequence {
            continue;
        }
        match event_type {
            "assistant/chunk" if format_version < 2 => {
                let Some(chunk) = data
                    .get("chunk")
                    .filter(|chunk| chunk.get("type").and_then(Value::as_str) == Some("usage"))
                else {
                    continue;
                };
                let Some(usage_value) = chunk.get("usage") else {
                    return Err(DeepSeekHarnessError::Invalid("usage chunk lacks counters"));
                };
                let turn = data
                    .get("turn")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant chunk turn"))?;
                let step = data
                    .get("step")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant chunk step"))?;
                let model_id = route.clone().and_then(|route| route.model_id());
                fold.push_settlement(
                    &record_context,
                    SettlementSample {
                        model_id,
                        event_at_ms: event_time,
                        usage: parse_usage(usage_value)?,
                        turn,
                        step,
                        sequence,
                        legacy_chunk: true,
                    },
                )?;
            }
            "assistant/message" | "assistant/attempt" => {
                let turn = data
                    .get("turn")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant settlement turn"))?;
                let step = data
                    .get("step")
                    .and_then(Value::as_u64)
                    .ok_or(DeepSeekHarnessError::Invalid("assistant settlement step"))?;
                let message_route = (event_type == "assistant/message")
                    .then(|| route_from_message(data))
                    .flatten();
                let model_id = message_route
                    .or_else(|| route.clone())
                    .and_then(|route| route.model_id());
                if event_type == "assistant/message"
                    && format_version < 2
                    && fold.last_slot.is_some_and(|slot| {
                        slot.turn == turn && slot.step == step && slot.legacy_chunk
                    })
                {
                    // Legacy logs store raw usage chunks; message.usage is a
                    // fallback for steps that produced no usage chunk.
                    fold.update_last_metadata(turn, step, model_id, event_time);
                    continue;
                }
                let Some((usage, stream_time)) = settlement_usage(event_type, data)? else {
                    continue;
                };
                fold.push_settlement(
                    &record_context,
                    SettlementSample {
                        model_id,
                        event_at_ms: stream_time.or(event_time),
                        usage,
                        turn,
                        step,
                        sequence,
                        legacy_chunk: false,
                    },
                )?;
            }
            "compaction/summary" => {
                let Some(usage_value) = data.get("usage").filter(|value| !value.is_null()) else {
                    continue;
                };
                let usage = parse_usage(usage_value)?;
                let model_id = route_from_config(data).and_then(|route| route.model_id());
                fold.push_independent(
                    &record_context,
                    IndependentUsageSample {
                        model_id,
                        event_at_ms: event_time,
                        usage,
                        sequence,
                    },
                )?;
                // A compaction summary is an independent provider call. The
                // following assistant settlement must not replace a sample
                // from an earlier request in the same turn and step.
                fold.last_slot = None;
            }
            _ => {}
        }
    }

    let header = header.ok_or(DeepSeekHarnessError::Invalid("empty session log"))?;
    if header.is_seeded && !seed_boundary_seen {
        return Err(DeepSeekHarnessError::Invalid("fork seed boundary missing"));
    }
    let mut records: Vec<_> = fold
        .records
        .into_iter()
        .filter(|record| record.source_seq >= own_start_sequence)
        .map(|record| record.record)
        .collect();
    records.sort_by(|left, right| left.record_id.cmp(&right.record_id));
    let model_calls = records.len();
    Ok(ParsedSession {
        title,
        records,
        model_calls,
    })
}
