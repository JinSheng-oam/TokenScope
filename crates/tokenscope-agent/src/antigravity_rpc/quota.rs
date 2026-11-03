use chrono::DateTime;
use serde_json::Value;
use tokenscope_local_store::AntigravityQuota;

const LOCAL_SOURCE_ID: &str = "local_antigravity";

pub(super) fn fraction(value: &Value) -> Option<u16> {
    let v = value.as_f64()?;
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        return None;
    }
    Some((v * 10_000.0).round() as u16)
}

fn remaining_fraction(bucket: &Value) -> Option<u16> {
    if bucket.get("disabled").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    bucket
        .get("remainingFraction")
        .and_then(fraction)
        .or_else(|| {
            bucket
                .pointer("/remaining/remainingFraction")
                .and_then(fraction)
        })
        .or_else(|| {
            if bucket.pointer("/remaining/case").and_then(Value::as_str)
                == Some("remainingFraction")
            {
                bucket.pointer("/remaining/value").and_then(fraction)
            } else {
                None
            }
        })
}

fn parse_time(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(if n > 20_000_000_000 {
            n
        } else {
            n.saturating_mul(1000)
        })
        .filter(|n| *n >= 0);
    }
    let s = value.as_str()?;
    let parsed = DateTime::parse_from_rfc3339(s).ok()?;
    let ms = parsed.timestamp_millis();
    (ms >= 0).then_some(ms)
}

fn pool_id(display: &str) -> String {
    let lower = display.to_ascii_lowercase();
    if lower.contains("argon") {
        return "gemini_4_argon".into();
    }
    if lower.contains("claude") || lower.contains("gpt") {
        return "claude_gpt".into();
    }
    if lower.contains("gemini") {
        return "gemini".into();
    }
    // Preserve unknown upstream group identity instead of collapsing every
    // future provider into a single `other` bucket.
    let mut slug = String::new();
    for ch in lower.chars().take(128) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('_') {
            slug.push('_');
        }
        if slug.len() >= 64 {
            break;
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "other".into()
    } else {
        slug.to_string()
    }
}

fn window_id(bucket: &Value) -> Option<&'static str> {
    let values = ["window", "bucketId", "displayName"];
    for key in values {
        let raw = bucket
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .replace('_', "-");
        if raw.contains("weekly") {
            return Some("weekly");
        }
        if raw.contains("daily") {
            return Some("daily");
        }
        if raw.contains("session")
            || raw.contains("5h")
            || raw.contains("5-hour")
            || raw.contains("five hour")
            || raw.contains("five-hour")
        {
            return Some("five_hour");
        }
    }
    None
}

pub(super) fn parse_grouped_quota(
    payload: &Value,
    source_id: &str,
    observed_at_ms: i64,
) -> Vec<AntigravityQuota> {
    let root = payload
        .get("response")
        .or_else(|| payload.get("summary"))
        .unwrap_or(payload);
    let Some(groups) = root.get("groups").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut rows = std::collections::BTreeMap::<(String, String), AntigravityQuota>::new();
    for group in groups.iter().take(32) {
        let name = group
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("other");
        let pool = pool_id(name);
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
            continue;
        };
        for bucket in buckets.iter().take(16) {
            if rows.len() >= 200 {
                break;
            }
            let Some(window) = window_id(bucket) else {
                continue;
            };
            let disabled = bucket.get("disabled").and_then(Value::as_bool) == Some(true);
            let remaining = remaining_fraction(bucket);
            // Missing/invalid percentages must not erase a previously known
            // value. Explicitly disabled pools are retained as unknown.
            if !disabled && remaining.is_none() {
                continue;
            }
            let row = AntigravityQuota {
                source_id: source_id.to_string(),
                pool_id: pool.clone(),
                window_id: window.to_string(),
                remaining_basis_points: remaining,
                reset_at_ms: bucket.get("resetTime").and_then(parse_time),
                observed_at_ms,
            };
            let key = (row.pool_id.clone(), row.window_id.clone());
            match rows.get(&key) {
                // If upstream splits one pool across multiple groups, preserve
                // the lowest remaining quota (conservative); tie-break by the
                // earliest known reset time.
                Some(old) => {
                    let old_remaining = old.remaining_basis_points.unwrap_or(u16::MAX);
                    let new_remaining = row.remaining_basis_points.unwrap_or(u16::MAX);
                    let old_reset = old.reset_at_ms.unwrap_or(i64::MAX);
                    let new_reset = row.reset_at_ms.unwrap_or(i64::MAX);
                    if new_remaining < old_remaining
                        || (new_remaining == old_remaining && new_reset < old_reset)
                    {
                        rows.insert(key, row);
                    }
                }
                None => {
                    rows.insert(key, row);
                }
            }
        }
        if rows.len() >= 200 {
            break;
        }
    }
    rows.into_values().collect()
}

pub(super) fn parse_legacy_quota(
    payload: &Value,
    source_id: &str,
    observed_at_ms: i64,
) -> Vec<AntigravityQuota> {
    const BLACKLIST: &[&str] = &[
        "MODEL_CHAT_20706",
        "MODEL_CHAT_23310",
        "MODEL_GOOGLE_GEMINI_2_5_FLASH",
        "MODEL_GOOGLE_GEMINI_2_5_FLASH_THINKING",
        "MODEL_GOOGLE_GEMINI_2_5_FLASH_LITE",
        "MODEL_GOOGLE_GEMINI_2_5_PRO",
        "MODEL_PLACEHOLDER_M19",
        "MODEL_PLACEHOLDER_M9",
        "MODEL_PLACEHOLDER_M12",
    ];
    let configs = payload
        .pointer("/userStatus/cascadeModelConfigData/clientModelConfigs")
        .or_else(|| {
            payload.pointer("/response/userStatus/cascadeModelConfigData/clientModelConfigs")
        })
        .or_else(|| payload.pointer("/response/clientModelConfigs"))
        .or_else(|| payload.get("clientModelConfigs"))
        .and_then(Value::as_array);
    let Some(configs) = configs else {
        return Vec::new();
    };
    let mut rows = std::collections::BTreeMap::<String, AntigravityQuota>::new();
    for cfg in configs.iter().take(512) {
        let model_id = cfg
            .pointer("/modelOrAlias/model")
            .and_then(Value::as_str)
            .unwrap_or("");
        if model_id.is_empty() || BLACKLIST.contains(&model_id) {
            continue;
        }
        let label = cfg.get("label").and_then(Value::as_str).unwrap_or(model_id);
        let Some(quota) = cfg.get("quotaInfo") else {
            continue;
        };
        let Some(remaining) = quota.get("remainingFraction").and_then(fraction) else {
            continue;
        };
        let pool = pool_id(&format!("{label} {model_id}"));
        let row = AntigravityQuota {
            source_id: source_id.to_string(),
            pool_id: pool.clone(),
            window_id: "other".into(),
            remaining_basis_points: Some(remaining),
            reset_at_ms: quota.get("resetTime").and_then(parse_time),
            observed_at_ms,
        };
        match rows.get(&pool) {
            Some(old) if old.remaining_basis_points.unwrap_or(u16::MAX) <= remaining => {}
            _ => {
                rows.insert(pool, row);
            }
        }
    }
    rows.into_values().collect()
}

// Quota snapshots are device-local and not synchronized. We intentionally do
// not persist a Google email or a reversible hash of it; one local Antigravity
// account is represented by one latest-snapshot namespace.
pub(super) fn local_source_id() -> &'static str {
    LOCAL_SOURCE_ID
}
