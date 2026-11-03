use crate::{models::NativeUsage, AntigravityError, Result, MAX_PROTO_BYTES};
use tokenscope_core_domain::{
    session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};

fn varint(b: &[u8], pos: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    for i in 0..10 {
        let byte = *b
            .get(*pos)
            .ok_or(AntigravityError::Invalid("truncated protobuf"))?;
        *pos += 1;
        if i == 9 && byte > 1 {
            return Err(AntigravityError::Invalid("protobuf integer overflow"));
        }
        value |= u64::from(byte & 0x7f) << (i * 7);
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(AntigravityError::Invalid("protobuf varint too long"))
}
pub(super) fn proto_field(b: &[u8], requested: u64) -> Result<Option<&[u8]>> {
    let mut pos = 0;
    while pos < b.len() {
        let tag = varint(b, &mut pos)?;
        let field = tag >> 3;
        if field == 0 {
            return Err(AntigravityError::Invalid("protobuf field zero"));
        }
        match tag & 7 {
            0 => {
                varint(b, &mut pos)?;
            }
            1 => {
                pos = pos
                    .checked_add(8)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
            }
            2 => {
                let n = usize::try_from(varint(b, &mut pos)?)
                    .map_err(|_| AntigravityError::Invalid("protobuf length"))?;
                let end = pos
                    .checked_add(n)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
                let slice = b
                    .get(pos..end)
                    .ok_or(AntigravityError::Invalid("protobuf truncated field"))?;
                if field == requested {
                    return Ok(Some(slice));
                }
                pos = end;
            }
            5 => {
                pos = pos
                    .checked_add(4)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
            }
            _ => return Err(AntigravityError::Invalid("unsupported protobuf wire type")),
        }
        if pos > b.len() {
            return Err(AntigravityError::Invalid("protobuf bounds"));
        }
    }
    Ok(None)
}

/// `gen_metadata.field_1.field_19` holds a model identifier in known
/// Antigravity versions. Unknown layouts are reported, not interpreted as 0.
pub fn extract_model_name(data: &[u8]) -> Result<Option<String>> {
    if data.len() > MAX_PROTO_BYTES {
        return Err(AntigravityError::Invalid("metadata BLOB too large"));
    }
    let Some(nested) = proto_field(data, 1)? else {
        return Ok(None);
    };
    let Some(bytes) = proto_field(nested, 19)? else {
        return Ok(None);
    };
    let name =
        std::str::from_utf8(bytes).map_err(|_| AntigravityError::Invalid("model encoding"))?;
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b':' | b'/'))
    {
        return Err(AntigravityError::Invalid("model name is unsafe"));
    }
    Ok(Some(name.to_string()))
}

pub(super) fn proto_varint_field(data: &[u8], requested: u64) -> Result<Option<u64>> {
    let mut pos = 0;
    while pos < data.len() {
        let tag = varint(data, &mut pos)?;
        let field = tag >> 3;
        if field == 0 {
            return Err(AntigravityError::Invalid("protobuf field zero"));
        }
        match tag & 7 {
            0 => {
                let value = varint(data, &mut pos)?;
                if field == requested {
                    return Ok(Some(value));
                }
            }
            1 => {
                pos = pos
                    .checked_add(8)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?
            }
            2 => {
                let len = usize::try_from(varint(data, &mut pos)?)
                    .map_err(|_| AntigravityError::Invalid("protobuf length"))?;
                pos = pos
                    .checked_add(len)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?;
            }
            5 => {
                pos = pos
                    .checked_add(4)
                    .ok_or(AntigravityError::Invalid("protobuf bounds"))?
            }
            _ => return Err(AntigravityError::Invalid("unsupported protobuf wire type")),
        }
        if pos > data.len() {
            return Err(AntigravityError::Invalid("protobuf bounds"));
        }
    }
    Ok(None)
}

pub(super) fn extract_usage(data: &[u8]) -> Result<Option<NativeUsage>> {
    let Some(chat_model) = proto_field(data, 1)? else {
        return Ok(None);
    };
    let Some(usage) = proto_field(chat_model, 4)? else {
        return Ok(None);
    };
    let field = |number| proto_varint_field(usage, number).map(|value| value.unwrap_or(0));
    let parsed = NativeUsage {
        fixed_prompt_tokens: field(1)?,
        input_tokens: field(2)?,
        cache_read_tokens: field(5)?,
        output_tokens: field(9)?,
        reasoning_tokens: field(10)?,
    };
    let output_total = parsed
        .output_tokens
        .checked_add(parsed.reasoning_tokens)
        .ok_or(AntigravityError::Invalid("output token count overflow"))?;
    if proto_varint_field(usage, 3)?.is_some_and(|stored| stored != output_total) {
        return Err(AntigravityError::Invalid(
            "inconsistent output token counters",
        ));
    }
    if [
        parsed.fixed_prompt_tokens,
        parsed.input_tokens,
        parsed.cache_read_tokens,
        parsed.output_tokens,
        parsed.reasoning_tokens,
    ]
    .iter()
    .all(|count| *count == 0)
    {
        return Ok(None);
    }
    for count in [
        parsed.fixed_prompt_tokens,
        parsed.input_tokens,
        parsed.cache_read_tokens,
        output_total,
    ] {
        if count > i64::MAX as u64 {
            return Err(AntigravityError::Invalid(
                "token count outside storage range",
            ));
        }
    }
    Ok(Some(parsed))
}

pub(super) fn extract_response_id(data: &[u8]) -> Result<Option<String>> {
    let Some(chat_model) = proto_field(data, 1)? else {
        return Ok(None);
    };
    let Some(usage) = proto_field(chat_model, 4)? else {
        return Ok(None);
    };
    let Some(bytes) = proto_field(usage, 11)? else {
        return Ok(None);
    };
    let value = std::str::from_utf8(bytes)
        .map_err(|_| AntigravityError::Invalid("response ID encoding"))?;
    if value.trim().is_empty() {
        return Ok(None);
    }
    if value.len() > 512 || value.chars().any(char::is_control) {
        return Err(AntigravityError::Invalid("response ID bounds"));
    }
    Ok(Some(value.to_string()))
}

pub(super) fn make_record(
    stem: &str,
    device: &str,
    native_id: &str,
    model: Option<&str>,
    project_id: Option<&str>,
    usage: NativeUsage,
    event_at_ms: Option<i64>,
) -> Result<CanonicalRecord> {
    let source = stem.as_bytes();
    let session = session_key("antigravity", source)?;
    let mut record = CanonicalRecord {
        record_id: stable_record_id("antigravity", source, native_id.as_bytes())?,
        owner_device_id: device.to_string(),
        tool_id: "antigravity".into(),
        scope: UsageScope::DeviceLocal,
        precision: if usage.fixed_prompt_tokens == 0 {
            Precision::Exact
        } else {
            Precision::Aggregate
        },
        account_id: None,
        model_id: model.map(str::to_string),
        project_id: project_id.map(str::to_owned),
        session_id: Some(session),
        event_at_ms,
        usage: TokenBreakdown {
            input_uncached_tokens: usage.input_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            output_tokens: usage
                .output_tokens
                .checked_add(usage.reasoning_tokens)
                .ok_or(AntigravityError::Invalid("output token count overflow"))?,
            unclassified_tokens: usage.fixed_prompt_tokens,
            reasoning_tokens: Some(usage.reasoning_tokens),
            ..TokenBreakdown::default()
        },
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    };
    record = record.normalized()?;
    Ok(record)
}

pub(super) fn protobuf_timestamp_ms(data: &[u8]) -> Option<i64> {
    let seconds = i64::try_from(proto_varint_field(data, 1).ok()??).ok()?;
    let nanos = proto_varint_field(data, 2).ok()?.unwrap_or(0);
    if !(0..=253_402_300_799).contains(&seconds) || nanos >= 1_000_000_000 {
        return None;
    }
    seconds
        .checked_mul(1000)?
        .checked_add(i64::try_from(nanos / 1_000_000).ok()?)
}
