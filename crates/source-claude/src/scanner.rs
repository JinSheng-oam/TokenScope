use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::BufReader;
use std::path::Path;
use tokenscope_core_domain::{stable_record_id, CanonicalRecord, Precision, UsageScope};
use tokenscope_local_store::SourceCheckpoint;

use super::metadata::{bounded_line, clean_title, file_identity, fingerprint, modified_ns};
use super::model::{ClaudeError, ClaudeScan, Result};
use super::usage::{parse_time, parse_usage, Sample};
use super::PARSER_VERSION;

const MAX_EVENTS: usize = 50_000; // bounded fallback; streaming parse is a later optimization

/// A fresh full scan is intentionally used when file metadata changes. This
/// avoids needing to persist a large streaming-response ID state. It is
/// bounded to 50,000 distinct responses/file, with O(file size) read cost.
pub fn scan_claude_file(
    path: &Path,
    config_dir: &Path,
    device_id: &str,
    previous: Option<&SourceCheckpoint>,
    observed_at_ms: i64,
) -> Result<Option<ClaudeScan>> {
    let (source_key, session_id, project_id) = file_identity(path, config_dir)?;
    if let Some(cp) = previous {
        if cp.source_key != source_key || cp.tool_id != "claude" {
            return Err(ClaudeError::Invalid("checkpoint source mismatch"));
        }
    }
    let before = fs::symlink_metadata(path)?;
    if !before.file_type().is_file() {
        return Err(ClaudeError::Invalid("transcript must be a regular file"));
    }
    let fp = fingerprint(path, &before)?;
    if previous.is_some_and(|p| {
        p.parser_version == PARSER_VERSION
            && p.source_fingerprint == fp
            && p.last_status == "complete"
    }) {
        return Ok(None);
    }

    let mut stream = BufReader::new(File::open(path)?);
    let mut samples: HashMap<String, Sample> = HashMap::new();
    let mut inconsistent: HashSet<String> = HashSet::new();
    let mut parsed = 0_usize;
    let mut custom_title: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    while let Some((line, complete)) = bounded_line(&mut stream)? {
        if !complete {
            return Err(ClaudeError::Pending);
        }
        parsed += 1;
        let v: Value = serde_json::from_slice(&line)?;
        let kind = v.get("type").and_then(Value::as_str);
        if custom_title.is_none() && kind == Some("custom-title") {
            custom_title = v
                .get("customTitle")
                .and_then(Value::as_str)
                .and_then(clean_title);
        }
        if first_prompt.is_none() && kind == Some("user") {
            first_prompt = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| match c {
                    Value::String(s) => Some(s.as_str()),
                    Value::Array(items) => items
                        .iter()
                        .find_map(|i| i.get("text").and_then(Value::as_str)),
                    _ => None,
                })
                .and_then(clean_title);
        }
        if kind != Some("assistant") {
            continue;
        }
        let Some(message) = v.get("message") else {
            continue;
        };
        let Some(u) = message.get("usage").filter(|u| !u.is_null()) else {
            continue;
        };
        let msg_id = message
            .get("id")
            .and_then(Value::as_str)
            .ok_or(ClaudeError::Invalid(
                "assistant with usage lacks message.id",
            ))?;
        if msg_id.is_empty() || msg_id.len() > 512 || msg_id.chars().any(char::is_control) {
            return Err(ClaudeError::Invalid("response identity"));
        }
        // Anthropic message.id is the logical API response identity. requestId
        // can be absent on some rows of that same response.
        let native = msg_id.to_owned();
        let at = parse_time(&v)?;
        let usage = match parse_usage(u)? {
            Some(value) => value,
            None => {
                if !samples.contains_key(&native) {
                    inconsistent.insert(native);
                }
                continue;
            }
        };
        inconsistent.remove(&native);
        let score = (usage.total()?, usage.output_tokens);
        let model = message
            .get("model")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control))
            .map(str::to_owned);
        let id = stable_record_id("claude", source_key.as_bytes(), native.as_bytes())?;
        let record = CanonicalRecord {
            record_id: id,
            owner_device_id: device_id.to_owned(),
            tool_id: "claude".into(),
            scope: UsageScope::DeviceLocal,
            precision: Precision::Exact,
            account_id: None,
            model_id: model,
            project_id: Some(project_id.clone()),
            session_id: Some(session_id.clone()),
            event_at_ms: Some(at),
            usage,
            estimated_cost_microusd: None,
            price_version: None,
            is_deleted: false,
        }
        .normalized()?;
        match samples.get_mut(&native) {
            Some(existing) => {
                let ambiguous = existing.record.precision == Precision::Estimated
                    || existing.record.usage != record.usage;
                if score > existing.score
                    || (score == existing.score
                        && (record.model_id.is_some() || existing.record.model_id.is_none()))
                {
                    let first_time = existing.record.event_at_ms;
                    let prior_model = existing.record.model_id.clone();
                    existing.record = record;
                    if ambiguous {
                        existing.record.precision = Precision::Estimated;
                    }
                    if existing.record.model_id.is_none() {
                        existing.record.model_id = prior_model;
                    }
                    existing.record.event_at_ms = first_time.map(|x| x.min(at));
                    existing.score = score;
                } else if ambiguous {
                    existing.record.precision = Precision::Estimated;
                }
            }
            None => {
                if samples.len() >= MAX_EVENTS {
                    return Err(ClaudeError::Limit("responses per file > 50000"));
                }
                samples.insert(native, Sample { record, score });
            }
        }
    }
    if !inconsistent.is_empty() {
        return Err(ClaudeError::Invalid("unresolved cache usage mismatch"));
    }
    let after = fs::symlink_metadata(path)?;
    if !after.file_type().is_file()
        || after.len() != before.len()
        || modified_ns(&after)? != modified_ns(&before)?
        || fingerprint(path, &after)? != fp
    {
        return Err(ClaudeError::Pending);
    }
    let next = SourceCheckpoint {
        tool_id: "claude".into(),
        source_key,
        source_fingerprint: fp,
        read_offset: after.len(),
        parser_version: PARSER_VERSION,
        last_scan_ms: previous.map_or(observed_at_ms, |p| {
            observed_at_ms.max(p.last_scan_ms.saturating_add(1))
        }),
        last_complete_scan_ms: None,
        last_status: "complete".into(),
    };
    let mut next = next;
    next.last_complete_scan_ms = Some(next.last_scan_ms);
    let mut records: Vec<_> = samples.into_values().map(|s| s.record).collect();
    records.sort_by(|a, b| a.record_id.cmp(&b.record_id));
    Ok(Some(ClaudeScan {
        next,
        records,
        parsed_lines: parsed,
        title: custom_title.or(first_prompt),
    }))
}
