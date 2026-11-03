use crate::{CodexError, Counters, Result};
use serde_json::Value;
use std::path::Path;
use tokenscope_core_domain::{
    project_key, session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown,
    UsageScope,
};

pub(super) fn clean_title(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut depth = 0usize;
    for ch in raw.chars() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    // Injected context (page state, agent instructions, environment blobs)
    // must never become a session label.
    if trimmed.is_empty()
        || trimmed.starts_with('{')
        || trimmed.starts_with('#')
        || trimmed.starts_with('[')
        || trimmed.contains("AGENTS.md instructions")
        || trimmed.starts_with("The following is the Codex agent history")
        // Legacy environment blobs: "D:\\path powershell 2026-04-02 Asia/Shanghai".
        || (trimmed.chars().nth(1) == Some(':') && trimmed.contains("Asia/"))
    {
        return None;
    }
    let text: String = trimmed.chars().take(80).collect();
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text)
}

pub(super) fn codex_project_metadata(cwd: &str) -> Option<(String, String)> {
    let path = Path::new(cwd);
    if cwd.len() > 4096 || !path.is_absolute() || cwd.chars().any(char::is_control) {
        return None;
    }
    let mut root = path;
    for ancestor in path.ancestors().take(64) {
        if ancestor.join(".git").exists() {
            root = ancestor;
            break;
        }
    }
    let identity = root.to_string_lossy().trim().replace('\\', "/");
    let identity = if cfg!(windows) {
        identity.to_lowercase()
    } else {
        identity
    };
    let project_id = project_key("codex", identity.as_bytes()).ok()?;
    let name = root.file_name().and_then(|value| value.to_str())?.trim();
    if name.is_empty()
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
    {
        return None;
    }
    Some((project_id, name.to_owned()))
}

pub(super) fn usage(v: &Value) -> Result<TokenBreakdown> {
    let number = |name: &str, required: bool| -> Result<u64> {
        match v.get(name) {
            None if !required => Ok(0),
            Some(x) => x
                .as_u64()
                .ok_or(CodexError::Invalid("invalid usage counter")),
            None => Err(CodexError::Invalid("missing usage counter")),
        }
    };
    let input = number("input_tokens", true)?;
    let cache_read = number("cached_input_tokens", false)?;
    let cache_write = number("cache_write_input_tokens", false)?;
    let output = number("output_tokens", true)?;
    let sum = input
        .checked_add(output)
        .ok_or(CodexError::Invalid("usage overflow"))?;
    let total = match v.get("total_tokens") {
        Some(x) => x
            .as_u64()
            .ok_or(CodexError::Invalid("invalid total_tokens"))?,
        None => sum,
    };
    if total < sum
        || total > i64::MAX as u64
        || cache_read
            .checked_add(cache_write)
            .is_none_or(|c| c > input)
    {
        return Err(CodexError::Invalid("usage buckets inconsistent"));
    }
    let reasoning = v
        .get("reasoning_output_tokens")
        .map(|x| x.as_u64().ok_or(CodexError::Invalid("invalid reasoning")))
        .transpose()?;
    if reasoning.is_some_and(|r| r > output) {
        return Err(CodexError::Invalid("reasoning exceeds output"));
    }
    Ok(TokenBreakdown {
        input_uncached_tokens: input - cache_read - cache_write,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        output_tokens: output,
        unclassified_tokens: total - sum,
        reasoning_tokens: reasoning,
    })
}

pub(super) fn legacy_difference(
    current: Counters,
    previous: Option<Counters>,
) -> Result<TokenBreakdown> {
    let p = previous.unwrap_or_default();
    if !current.monotonic_from(&p) {
        return Err(CodexError::CounterRegression);
    }
    let input = current.input - p.input;
    let cached = current.cached - p.cached;
    let output = current.output - p.output;
    let total = current.total - p.total;
    if cached > input
        || total
            < input
                .checked_add(output)
                .ok_or(CodexError::Invalid("delta overflow"))?
    {
        return Err(CodexError::Invalid("cumulative delta inconsistent"));
    }
    let reasoning = match (current.reasoning, previous.and_then(|x| x.reasoning)) {
        (Some(n), Some(old)) => Some(n.checked_sub(old).ok_or(CodexError::CounterRegression)?),
        (Some(n), None) if previous.is_none() => Some(n),
        _ => None,
    };
    if reasoning.is_some_and(|r| r > output) {
        return Err(CodexError::Invalid("delta reasoning"));
    }
    Ok(TokenBreakdown {
        input_uncached_tokens: input - cached,
        cache_read_tokens: cached,
        cache_write_tokens: 0,
        output_tokens: output,
        unclassified_tokens: total - input - output,
        reasoning_tokens: reasoning,
    })
}

pub(super) fn record(
    session: &str,
    device: &str,
    native_key: &[u8],
    model: Option<String>,
    project_id: Option<String>,
    at: i64,
    tokens: TokenBreakdown,
) -> Result<CanonicalRecord> {
    let rec = CanonicalRecord {
        record_id: stable_record_id("codex", session.as_bytes(), native_key)?,
        owner_device_id: device.to_owned(),
        tool_id: "codex".into(),
        scope: UsageScope::DeviceLocal,
        precision: if tokens.unclassified_tokens == 0 {
            Precision::Exact
        } else {
            Precision::Aggregate
        },
        account_id: None,
        model_id: model,
        project_id,
        session_id: Some(session_key("codex", session.as_bytes())?),
        event_at_ms: Some(at),
        usage: tokens,
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    };
    Ok(rec.normalized()?)
}
