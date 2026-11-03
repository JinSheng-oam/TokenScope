use chrono::DateTime;
use serde_json::Value;
use tokenscope_core_domain::{CanonicalRecord, TokenBreakdown};

use super::model::{ClaudeError, Result};

#[derive(Clone)]
pub(super) struct Sample {
    pub(super) record: CanonicalRecord,
    // Higher observed totals and output counts represent a more complete
    // streaming snapshot; never add two snapshots of the same response.
    pub(super) score: (u64, u64),
}

pub(super) fn number(usage: &Value, key: &'static str, required: bool) -> Result<u64> {
    match usage.get(key) {
        Some(v) if v.is_null() && !required => Ok(0),
        None if !required => Ok(0),
        Some(v) => v.as_u64().ok_or(ClaudeError::Invalid(
            "usage field not a nonnegative integer",
        )),
        None => Err(ClaudeError::Invalid(key)),
    }
}

/// A top-level zero cache-write with nonzero nested writes can be a placeholder
/// row. Ignore it only if another valid row with the same response identity is
/// present; otherwise FAIL CLOSED instead of guessing the total.
pub(super) fn parse_usage(usage: &Value) -> Result<Option<TokenBreakdown>> {
    let input = number(usage, "input_tokens", true)?;
    let output = number(usage, "output_tokens", true)?;
    let read = number(usage, "cache_read_input_tokens", false)?;
    let write = number(usage, "cache_creation_input_tokens", false)?;
    if let Some(nested) = usage.get("cache_creation").filter(|v| !v.is_null()) {
        let a = number(nested, "ephemeral_5m_input_tokens", false)?;
        let b = number(nested, "ephemeral_1h_input_tokens", false)?;
        let sum = a
            .checked_add(b)
            .ok_or(ClaudeError::Limit("cache counters overflow"))?;
        if sum != write {
            return Ok(None);
        }
    }
    let reasoning = usage
        .get("output_tokens_details")
        .and_then(|v| v.get("thinking_tokens"))
        .filter(|v| !v.is_null())
        .map(|v| v.as_u64().ok_or(ClaudeError::Invalid("reasoning counter")))
        .transpose()?;
    let result = TokenBreakdown {
        input_uncached_tokens: input,
        cache_read_tokens: read,
        cache_write_tokens: write,
        output_tokens: output,
        unclassified_tokens: 0,
        reasoning_tokens: reasoning,
    };
    result.validate()?;
    if result.total()? > i64::MAX as u64 {
        return Err(ClaudeError::Limit("SQLite token integer"));
    }
    Ok(Some(result))
}
pub(super) fn parse_time(v: &Value) -> Result<i64> {
    let ts = v
        .get("timestamp")
        .and_then(Value::as_str)
        .ok_or(ClaudeError::Invalid("missing timestamp"))?;
    DateTime::parse_from_rfc3339(ts)
        .map(|t| t.timestamp_millis())
        .map_err(|_| ClaudeError::Invalid("invalid timestamp"))
}
