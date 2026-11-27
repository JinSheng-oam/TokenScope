use reqwest::blocking::{Client, Response};
use reqwest::header::{ETAG, IF_NONE_MATCH};
use reqwest::redirect::Policy;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokenscope_local_ipc::{PriceCatalogStatus, PRICE_CATALOG_STATUS_FILENAME};
use tokenscope_local_store::{LocalStore, ModelPriceEntry};

const CATALOG_URL: &str = "https://models.dev/api.json";
const CATALOG_VERSION_PREFIX: &str = "models.dev:base-v4:";
const BUILTIN_FALLBACK_VERSION: &str = "builtin:fallback-v1";
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
const SUCCESS_REFRESH_MS: i64 = 24 * 60 * 60 * 1000;
const RETRY_REFRESH_MS: i64 = 60 * 60 * 1000;
const EMPTY_RETRY_REFRESH_MS: i64 = 2 * 60 * 1000;
static MANUAL_PRICES_APPLIED: Mutex<Vec<ModelPriceEntry>> = Mutex::new(Vec::new());
static LEGACY_REFRESH_ATTEMPTED: AtomicBool = AtomicBool::new(false);

enum CatalogResponse {
    NotModified(Option<String>),
    Updated {
        entries: Vec<ModelPriceEntry>,
        version: String,
        etag: Option<String>,
    },
}

fn write_status_file(db_path: &Path, status: &PriceCatalogStatus) {
    let Some(dir) = db_path.parent() else {
        return;
    };
    let destination = dir.join(PRICE_CATALOG_STATUS_FILENAME);
    let temporary = dir.join(format!("{PRICE_CATALOG_STATUS_FILENAME}.tmp"));
    let _ = fs::remove_file(&temporary);
    let Ok(body) = serde_json::to_vec(status) else {
        return;
    };
    let mut options = OpenOptions::new();
    options.create(true).write(true).truncate(true);
    let write_res = (|| -> std::io::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&body)?;
        file.sync_all()?;
        fs::rename(&temporary, &destination)
    })();
    if write_res.is_err() {
        let _ = fs::remove_file(&temporary);
    }
}

pub(super) fn refresh_if_due(store: &mut LocalStore, db_path: &Path) {
    let now = chrono::Utc::now().timestamp_millis();
    apply_manual_prices(store, now);
    let state = match store.model_price_catalog_state() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("TokenScope price catalog status unavailable: {error}");
            return;
        }
    };
    let has_catalog = state.active_version.is_some();
    let retry_window_ms = if has_catalog {
        RETRY_REFRESH_MS
    } else {
        EMPTY_RETRY_REFRESH_MS
    };
    let success_is_fresh = state
        .last_success_ms
        .is_some_and(|saved| now.saturating_sub(saved) < SUCCESS_REFRESH_MS);
    let attempt_is_recent = state
        .last_attempt_ms
        .is_some_and(|saved| now.saturating_sub(saved) < retry_window_ms);
    let current_format = state
        .active_version
        .as_deref()
        .is_some_and(|version| version.starts_with(CATALOG_VERSION_PREFIX));
    // A saved attempt from the old format cannot tell us whether this version tried.
    let first_upgrade_attempt = has_catalog
        && !current_format
        && !LEGACY_REFRESH_ATTEMPTED.swap(true, Ordering::Relaxed);
    if !first_upgrade_attempt && ((current_format && success_is_fresh) || attempt_is_recent) {
        return;
    }
    let _ = run_refresh(store, db_path, now, state.etag.as_deref(), current_format, state.last_success_ms);
}

pub(super) fn force_refresh(
    store: &mut LocalStore,
    db_path: &Path,
) -> Result<(Option<i64>, usize, usize), String> {
    let now = chrono::Utc::now().timestamp_millis();
    apply_manual_prices(store, now);
    let state = store
        .model_price_catalog_state()
        .map_err(|error| format!("读取价格目录状态失败：{error}"))?;
    let current_format = state
        .active_version
        .as_deref()
        .is_some_and(|version| version.starts_with(CATALOG_VERSION_PREFIX));
    // Force refresh bypasses ETag when no catalog is installed so we always download fresh data.
    let etag = if current_format {
        state.etag.as_deref()
    } else {
        None
    };
    run_refresh(store, db_path, now, etag, current_format, state.last_success_ms)
}

fn run_refresh(
    store: &mut LocalStore,
    db_path: &Path,
    now: i64,
    etag: Option<&str>,
    current_format: bool,
    previous_success_ms: Option<i64>,
) -> Result<(Option<i64>, usize, usize), String> {
    if let Err(error) = store.mark_model_price_attempt(now) {
        let message = format!("记录价格目录刷新尝试失败：{error}");
        eprintln!("TokenScope price catalog refresh deferred: {error}");
        return Err(message);
    }

    match fetch_catalog(etag, current_format) {
        Ok(CatalogResponse::NotModified(new_etag)) => {
            if let Err(error) = store.mark_model_price_not_modified(now, new_etag.as_deref()) {
                let message = format!("更新价格目录状态失败：{error}");
                eprintln!("TokenScope price catalog refresh failed: {error}");
                write_status_file(
                    db_path,
                    &PriceCatalogStatus {
                        last_attempt_ms: Some(now),
                        last_success_ms: previous_success_ms,
                        last_error: Some(message.clone()),
                    },
                );
                return Err(message);
            }
            let model_count = store
                .active_model_price_catalog()
                .map(|(_, entries)| entries.len())
                .unwrap_or(0);
            write_status_file(
                db_path,
                &PriceCatalogStatus {
                    last_attempt_ms: Some(now),
                    last_success_ms: Some(now),
                    last_error: None,
                },
            );
            Ok((Some(now), model_count, 0))
        }
        Ok(CatalogResponse::Updated {
            entries,
            version,
            etag,
        }) => match store.install_model_price_catalog(&version, &entries, etag.as_deref(), now) {
            Ok(report) => {
                println!(
                    "model price catalog version={} models={} historical_records_priced={}",
                    report.price_version, report.model_count, report.backfilled_records
                );
                write_status_file(
                    db_path,
                    &PriceCatalogStatus {
                        last_attempt_ms: Some(now),
                        last_success_ms: Some(now),
                        last_error: None,
                    },
                );
                Ok((Some(now), report.model_count, report.backfilled_records))
            }
            Err(error) => {
                let message = format!("写入价格目录失败：{error}");
                eprintln!("TokenScope price catalog install failed: {error}");
                write_status_file(
                    db_path,
                    &PriceCatalogStatus {
                        last_attempt_ms: Some(now),
                        last_success_ms: previous_success_ms,
                        last_error: Some(message.clone()),
                    },
                );
                Err(message)
            }
        },
        Err(error) => {
            let mut message = format!("下载价格目录失败（models.dev）：{error}");
            eprintln!("TokenScope public model prices unavailable: {error}");
            let has_any_catalog = store
                .model_price_catalog_state()
                .ok()
                .and_then(|s| s.active_version)
                .is_some();
            if !has_any_catalog {
                let fallback = builtin_fallback_entries();
                if let Ok(report) = store.install_model_price_catalog(
                    BUILTIN_FALLBACK_VERSION,
                    &fallback,
                    None,
                    now,
                ) {
                    println!(
                        "model price catalog fallback installed version={} models={} historical_records_priced={}",
                        report.price_version, report.model_count, report.backfilled_records
                    );
                    message = format!(
                        "已启用内置离线兜底报价（{} 个主流模型），联网后可刷新完整目录：{error}",
                        report.model_count
                    );
                }
            }
            write_status_file(
                db_path,
                &PriceCatalogStatus {
                    last_attempt_ms: Some(now),
                    last_success_ms: previous_success_ms,
                    last_error: Some(message.clone()),
                },
            );
            Err(message)
        }
    }
}

fn apply_manual_prices(store: &mut LocalStore, now: i64) {
    let Ok(preferences) = store.pricing_preferences() else {
        return;
    };
    let Ok(mut applied) = MANUAL_PRICES_APPLIED.lock() else {
        return;
    };
    if *applied == preferences.manual_entries {
        return;
    }
    if !preferences.manual_entries.is_empty() {
        if let Err(error) = store.backfill_unpriced_model_costs(now) {
            eprintln!("TokenScope manual price backfill failed: {error}");
            return;
        }
    }
    *applied = preferences.manual_entries;
}

fn fetch_catalog(
    etag: Option<&str>,
    has_saved_catalog: bool,
) -> Result<CatalogResponse, Box<dyn std::error::Error>> {
    let redirect_policy = Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 {
            attempt.error("too many redirects")
        } else if attempt.url().scheme() != "https" {
            attempt.error("refusing non-HTTPS redirect")
        } else {
            attempt.follow()
        }
    });
    let client = Client::builder()
        .redirect(redirect_policy)
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut request = client.get(CATALOG_URL);
    if let Some(etag) = etag {
        if has_saved_catalog && !etag.is_empty() && etag.len() <= 512 {
            request = request.header(IF_NONE_MATCH, etag);
        }
    }
    let response = request.send()?;
    let status = response.status();
    let response_etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .filter(|value| {
            !value.is_empty() && value.len() <= 512 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned);
    if status == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(CatalogResponse::NotModified(response_etag));
    }
    if !status.is_success() {
        return Err(format!("HTTP status {status}").into());
    }
    let entries = parse_catalog_response(response)?;
    let provider_count = entries
        .iter()
        .map(|entry| entry.provider_id.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len();
    if entries.len() < 25 || provider_count < 3 {
        return Err("price catalog was incomplete or unexpectedly small".into());
    }
    let version = price_version(&entries)?;
    Ok(CatalogResponse::Updated {
        entries,
        version,
        etag: response_etag,
    })
}

fn parse_catalog_response(
    mut response: Response,
) -> Result<Vec<ModelPriceEntry>, Box<dyn std::error::Error>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err("price catalog response exceeds size limit".into());
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("price catalog response exceeds size limit".into());
    }
    let root: Value = serde_json::from_slice(&bytes)?;
    parse_catalog(&root).map_err(Into::into)
}

fn parse_catalog(root: &Value) -> Result<Vec<ModelPriceEntry>, String> {
    let providers = root
        .as_object()
        .ok_or_else(|| "price catalog root must be an object".to_owned())?;
    let mut entries = BTreeMap::<(String, String), ModelPriceEntry>::new();
    for (provider_key, provider_value) in providers {
        let Some(provider) = provider_value.as_object() else {
            continue;
        };
        let provider_id = provider
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or(provider_key);
        let Some(models) = provider.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (model_key, model_value) in models {
            let Some(model) = model_value.as_object() else {
                continue;
            };
            let Some(cost) = model.get("cost").and_then(Value::as_object) else {
                continue;
            };
            // Import published base rates. Usage records do not reliably carry the billing tier.
            let model_id = model.get("id").and_then(Value::as_str).unwrap_or(model_key);
            let reference = provider_id == "google"
                && matches!(
                    model_id,
                    "gemini-3-flash-preview"
                        | "gemini-3.6-flash"
                        | "gemini-3.7-flash"
                        | "gemini-3.8-flash"
                );
            if has_unsupported_pricing(cost, reference) {
                continue;
            }
            let (Ok(input_rate_nanos), Ok(output_rate_nanos), Ok(cache_read_rate_nanos), Ok(cache_write_rate_nanos), Ok(reasoning_rate_nanos)) = (
                parse_rate(cost.get("input")),
                parse_rate(cost.get("output")),
                parse_rate(cost.get("cache_read")),
                parse_rate(cost.get("cache_write")),
                parse_rate(cost.get("reasoning")),
            ) else {
                continue;
            };
            let entry = ModelPriceEntry {
                provider_id: provider_id.to_owned(),
                model_id: model_id.to_owned(),
                input_rate_nanos,
                output_rate_nanos,
                cache_read_rate_nanos,
                cache_write_rate_nanos,
                reasoning_rate_nanos,
            };
            if entry_has_any_rate(&entry) {
                let _ = insert_entry(&mut entries, entry.clone());
                if model_key != model_id {
                    let _ = insert_entry(
                        &mut entries,
                        ModelPriceEntry {
                            model_id: model_key.clone(),
                            ..entry
                        },
                    );
                }
            }
        }
    }
    if entries.is_empty() {
        return Err("price catalog contains no usable model rates".to_owned());
    }
    Ok(entries.into_values().collect())
}

fn has_unsupported_pricing(cost: &serde_json::Map<String, Value>, text_reference: bool) -> bool {
    cost.keys().any(|key| {
        // The reference is explicitly a text rate; its separate audio rate does not apply.
        if text_reference && key == "input_audio" {
            return false;
        }
        !matches!(
            key.as_str(),
            "input"
                | "output"
                | "cache_read"
                | "cache_write"
                | "reasoning"
                | "tiers"
                | "context_over_200k"
        )
    })
}

fn entry_has_any_rate(entry: &ModelPriceEntry) -> bool {
    entry.input_rate_nanos.is_some()
        || entry.output_rate_nanos.is_some()
        || entry.cache_read_rate_nanos.is_some()
        || entry.cache_write_rate_nanos.is_some()
        || entry.reasoning_rate_nanos.is_some()
}

fn insert_entry(
    entries: &mut BTreeMap<(String, String), ModelPriceEntry>,
    entry: ModelPriceEntry,
) -> Result<(), String> {
    let key = (entry.provider_id.clone(), entry.model_id.clone());
    if entries.get(&key).is_some_and(|previous| previous != &entry) {
        return Err(format!(
            "conflicting public prices for {}/{}",
            entry.provider_id, entry.model_id
        ));
    }
    entries.insert(key, entry);
    Ok(())
}

fn parse_rate(value: Option<&Value>) -> Result<Option<i64>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let raw = match value {
        Value::Number(number) => number.to_string(),
        Value::String(string) => string.clone(),
        _ => return Err("model price rate must be a decimal number".to_owned()),
    };
    decimal_to_nanos(&raw)
        .map(Some)
        .ok_or_else(|| "model price rate is negative, malformed, or out of range".to_owned())
}

fn decimal_to_nanos(raw: &str) -> Option<i64> {
    if raw.is_empty() || raw.starts_with('-') {
        return None;
    }
    let (mantissa, exponent) = match raw.find(['e', 'E']) {
        Some(index) => (&raw[..index], raw[index + 1..].parse::<i32>().ok()?),
        None => (raw, 0),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some(parts) => parts,
        None => (mantissa, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let value = digits.parse::<i128>().ok()?;
    if value == 0 {
        return Some(0);
    }
    let power = exponent
        .checked_sub(i32::try_from(fraction.len()).ok()?)?
        .checked_add(9)?;
    let scaled = if power >= 0 {
        value.checked_mul(10_i128.checked_pow(power as u32)?)?
    } else {
        let places = power.unsigned_abs();
        if places > 38 {
            0
        } else {
            let divisor = 10_i128.checked_pow(places)?;
            value.checked_add(divisor / 2)?.checked_div(divisor)?
        }
    };
    i64::try_from(scaled).ok()
}

fn price_version(entries: &[ModelPriceEntry]) -> Result<String, serde_json::Error> {
    let encoded = serde_json::to_vec(entries)?;
    let digest = Sha256::digest(encoded);
    Ok(format!("{CATALOG_VERSION_PREFIX}{digest:x}"))
}

fn rate_entry(
    provider_id: &str,
    model_id: &str,
    input: &str,
    output: &str,
    cache_read: Option<&str>,
    cache_write: Option<&str>,
) -> ModelPriceEntry {
    ModelPriceEntry {
        provider_id: provider_id.to_owned(),
        model_id: model_id.to_owned(),
        input_rate_nanos: decimal_to_nanos(input),
        output_rate_nanos: decimal_to_nanos(output),
        cache_read_rate_nanos: cache_read.and_then(decimal_to_nanos),
        cache_write_rate_nanos: cache_write.and_then(decimal_to_nanos),
        reasoning_rate_nanos: None,
    }
}

/// Built-in offline fallback rates (USD per 1M tokens) for core models so users in
/// air-gapped or proxy-restricted networks still get baseline cost estimates until
/// the live models.dev catalog can be fetched.
fn builtin_fallback_entries() -> Vec<ModelPriceEntry> {
    vec![
        // OpenAI
        rate_entry("openai", "gpt-5.4", "2.5", "15", Some("0.25"), None),
        rate_entry("openai", "gpt-5.3-codex", "1.75", "14", Some("0.175"), None),
        rate_entry("openai", "gpt-5.2", "1.75", "14", Some("0.175"), None),
        rate_entry("openai", "gpt-5", "1.25", "10", Some("0.125"), None),
        rate_entry("openai", "gpt-5-mini", "0.25", "2", Some("0.025"), None),
        rate_entry("openai", "gpt-4.1", "2", "8", Some("0.5"), None),
        rate_entry("openai", "gpt-4.1-mini", "0.4", "1.6", Some("0.1"), None),
        rate_entry("openai", "gpt-4o", "2.5", "10", Some("1.25"), None),
        rate_entry("openai", "o3", "2", "8", Some("0.5"), None),
        rate_entry("openai", "o4-mini", "1.1", "4.4", Some("0.275"), None),
        // Anthropic
        rate_entry("anthropic", "claude-opus-4-6", "15", "75", Some("1.5"), Some("18.75")),
        rate_entry("anthropic", "claude-sonnet-4-6", "3", "15", Some("0.3"), Some("3.75")),
        rate_entry("anthropic", "claude-sonnet-4-5", "3", "15", Some("0.3"), Some("3.75")),
        rate_entry("anthropic", "claude-haiku-4-5", "1", "5", Some("0.1"), Some("1.25")),
        rate_entry("anthropic", "claude-3-7-sonnet", "3", "15", Some("0.3"), Some("3.75")),
        rate_entry("anthropic", "claude-3-5-sonnet", "3", "15", Some("0.3"), Some("3.75")),
        // Google
        rate_entry("google", "gemini-3-flash-preview", "0.5", "3", Some("0.05"), None),
        rate_entry("google", "gemini-2.5-pro", "1.25", "10", Some("0.125"), None),
        rate_entry("google", "gemini-2.5-flash", "0.3", "2.5", Some("0.03"), None),
        rate_entry("google", "gemini-2.0-flash", "0.1", "0.4", Some("0.025"), None),
        // DeepSeek
        rate_entry("deepseek", "deepseek-chat", "0.27", "1.1", Some("0.07"), None),
        rate_entry("deepseek", "deepseek-reasoner", "0.55", "2.19", Some("0.14"), None),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decimal_rates_are_scaled_without_floating_point_arithmetic() {
        assert_eq!(decimal_to_nanos("1.75"), Some(1_750_000_000));
        assert_eq!(decimal_to_nanos("0.000000001"), Some(1));
        assert_eq!(decimal_to_nanos("4e-10"), Some(0));
        assert_eq!(decimal_to_nanos("1.25e2"), Some(125_000_000_000));
        assert_eq!(decimal_to_nanos("-1"), None);
        assert_eq!(decimal_to_nanos("1.2.3"), None);
    }

    #[test]
    fn parser_keeps_base_rates_for_tiered_models_and_skips_unsupported_rates() {
        let catalog = json!({
            "openai": {"id":"openai","models": {
                "gpt-5.4": {"id":"gpt-5.4","cost":{"input":1.25,"output":10,"cache_read":0.125}},
                "tiered": {"id":"tiered","cost":{"input":1,"output":2,"tiers":[{"input":3}]}},
                "audio": {"id":"audio","cost":{"input":1,"output":2,"input_audio":3}}
            }},
            "google": {"id":"google","models": {
                "gemini-3": {"id":"gemini-3","cost":{"input":0,"output":0}}
            }}
        });
        let entries = parse_catalog(&catalog).unwrap();
        assert_eq!(entries.len(), 3);
        let openai = entries
            .iter()
            .find(|entry| entry.provider_id == "openai")
            .unwrap();
        let google = entries
            .iter()
            .find(|entry| entry.provider_id == "google")
            .unwrap();
        assert_eq!(openai.input_rate_nanos, Some(1_250_000_000));
        assert_eq!(google.input_rate_nanos, Some(0));
        let tiered = entries
            .iter()
            .find(|entry| entry.model_id == "tiered")
            .unwrap();
        assert_eq!(tiered.input_rate_nanos, Some(1_000_000_000));
        assert!(!entries.iter().any(|entry| entry.model_id == "audio"));
        assert_ne!(
            price_version(&entries).unwrap(),
            price_version(&entries[..1]).unwrap()
        );
    }
}
