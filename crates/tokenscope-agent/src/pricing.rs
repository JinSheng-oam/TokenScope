use reqwest::blocking::{Client, Response};
use reqwest::header::{ETAG, IF_NONE_MATCH};
use reqwest::redirect::Policy;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokenscope_local_store::{LocalStore, ModelPriceEntry};

const CATALOG_URL: &str = "https://models.dev/api.json";
const CATALOG_VERSION_PREFIX: &str = "models.dev:base-v4:";
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
const SUCCESS_REFRESH_MS: i64 = 24 * 60 * 60 * 1000;
const RETRY_REFRESH_MS: i64 = 60 * 60 * 1000;
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

pub(super) fn refresh_if_due(store: &mut LocalStore) {
    let now = chrono::Utc::now().timestamp_millis();
    apply_manual_prices(store, now);
    let state = match store.model_price_catalog_state() {
        Ok(state) => state,
        Err(error) => {
            eprintln!("TokenScope price catalog status unavailable: {error}");
            return;
        }
    };
    let success_is_fresh = state
        .last_success_ms
        .is_some_and(|saved| now.saturating_sub(saved) < SUCCESS_REFRESH_MS);
    let attempt_is_recent = state
        .last_attempt_ms
        .is_some_and(|saved| now.saturating_sub(saved) < RETRY_REFRESH_MS);
    let current_format = state
        .active_version
        .as_deref()
        .is_some_and(|version| version.starts_with(CATALOG_VERSION_PREFIX));
    // A saved attempt from the old format cannot tell us whether this version tried.
    let first_upgrade_attempt = state.active_version.is_some()
        && !current_format
        && !LEGACY_REFRESH_ATTEMPTED.swap(true, Ordering::Relaxed);
    if !first_upgrade_attempt && ((current_format && success_is_fresh) || attempt_is_recent) {
        return;
    }
    if let Err(error) = store.mark_model_price_attempt(now) {
        eprintln!("TokenScope price catalog refresh deferred: {error}");
        return;
    }

    match fetch_catalog(state.etag.as_deref(), current_format) {
        Ok(CatalogResponse::NotModified(etag)) => {
            if let Err(error) = store.mark_model_price_not_modified(now, etag.as_deref()) {
                eprintln!("TokenScope price catalog refresh failed: {error}");
            }
        }
        Ok(CatalogResponse::Updated {
            entries,
            version,
            etag,
        }) => match store.install_model_price_catalog(&version, &entries, etag.as_deref(), now) {
            Ok(report) => println!(
                "model price catalog version={} models={} historical_records_priced={}",
                report.price_version, report.model_count, report.backfilled_records
            ),
            Err(error) => eprintln!("TokenScope price catalog install failed: {error}"),
        },
        Err(error) => eprintln!("TokenScope public model prices unavailable: {error}"),
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
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
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
            let entry = ModelPriceEntry {
                provider_id: provider_id.to_owned(),
                model_id: model_id.to_owned(),
                input_rate_nanos: parse_rate(cost.get("input"))?,
                output_rate_nanos: parse_rate(cost.get("output"))?,
                cache_read_rate_nanos: parse_rate(cost.get("cache_read"))?,
                cache_write_rate_nanos: parse_rate(cost.get("cache_write"))?,
                reasoning_rate_nanos: parse_rate(cost.get("reasoning"))?,
            };
            if entry_has_any_rate(&entry) {
                insert_entry(&mut entries, entry.clone())?;
                if model_key != model_id {
                    insert_entry(
                        &mut entries,
                        ModelPriceEntry {
                            model_id: model_key.clone(),
                            ..entry
                        },
                    )?;
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
