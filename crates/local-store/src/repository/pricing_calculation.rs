use super::{PricingPreferences, RATE_SCALE};
use crate::error::Result;
use crate::repository::models::ModelPriceEntry;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use tokenscope_core_domain::{CanonicalRecord, Precision, UsageScope};

const ANTIGRAVITY_REFERENCE_MODEL: &str = "gemini-3-flash-preview";

pub(in crate::repository) struct PriceBookCache<'a> {
    conn: &'a Connection,
    active_version: Option<String>,
    preferences: PricingPreferences,
    rates: HashMap<(String, String, String), Option<ModelPriceEntry>>,
}

impl<'a> PriceBookCache<'a> {
    pub(in crate::repository) fn new(conn: &'a Connection) -> Result<Self> {
        let active_version = conn.query_row(
            "SELECT active_version FROM model_price_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        let preferences = conn.path().filter(|path| !path.is_empty()).map_or_else(
            PricingPreferences::default,
            |path| {
                PricingPreferences::read(std::path::Path::new(path)).unwrap_or_else(|_| {
                    PricingPreferences {
                        keyword_matching: false,
                        manual_entries: Vec::new(),
                    }
                })
            },
        );
        let cache = Self {
            conn,
            active_version,
            preferences,
            rates: HashMap::new(),
        };
        Ok(cache)
    }

    pub(in crate::repository) fn has_rates(&self) -> bool {
        self.active_version.is_some() || !self.preferences.manual_entries.is_empty()
    }

    fn rate_for(
        &mut self,
        version: &str,
        provider: &str,
        model_id: &str,
    ) -> Result<Option<ModelPriceEntry>> {
        let key = (version.to_owned(), provider.to_owned(), model_id.to_owned());
        if let Some(rate) = self.rates.get(&key) {
            return Ok(rate.clone());
        }
        let rate = self
            .conn
            .query_row(
                "SELECT provider_id,model_id,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,reasoning_rate_nanos FROM model_price_rates WHERE price_version=?1 AND provider_id=?2 AND model_id=?3",
                params![version, provider, model_id],
                |row| {
                    Ok(ModelPriceEntry {
                        provider_id: row.get(0)?,
                        model_id: row.get(1)?,
                        input_rate_nanos: row.get(2)?,
                        output_rate_nanos: row.get(3)?,
                        cache_read_rate_nanos: row.get(4)?,
                        cache_write_rate_nanos: row.get(5)?,
                        reasoning_rate_nanos: row.get(6)?,
                    })
                },
            )
            .optional()?;
        self.rates.insert(key, rate.clone());
        Ok(rate)
    }

    pub(in crate::repository) fn price_record(
        &mut self,
        record: &mut CanonicalRecord,
    ) -> Result<bool> {
        self.price_record_with_previous(record, None)
    }

    pub(in crate::repository) fn price_record_with_previous(
        &mut self,
        record: &mut CanonicalRecord,
        previous: Option<&CanonicalRecord>,
    ) -> Result<bool> {
        if record.is_deleted || record.estimated_cost_microusd.is_some() {
            return Ok(false);
        }
        if let Some(previous) = previous {
            if let Some(cost) = previous.estimated_cost_microusd {
                if previous.model_id == record.model_id
                    && previous.usage == record.usage
                    && previous.scope == record.scope
                {
                    record.estimated_cost_microusd = Some(cost);
                    record.price_version.clone_from(&previous.price_version);
                    return Ok(false);
                }
            }
        }
        let version = previous
            .and_then(|record| record.price_version.clone())
            .filter(|version| !version.starts_with("manual:"))
            .or_else(|| self.active_version.clone());
        let Some((cost, version)) = self.estimate(version.as_deref(), record)? else {
            return Ok(false);
        };
        record.estimated_cost_microusd = Some(cost);
        record.price_version = Some(version);
        Ok(true)
    }

    pub(in crate::repository) fn estimate_current(
        &mut self,
        record: &CanonicalRecord,
    ) -> Result<Option<i64>> {
        let version = self.active_version.clone();
        Ok(self
            .estimate(version.as_deref(), record)?
            .map(|(cost, _)| cost))
    }

    fn estimate(
        &mut self,
        version: Option<&str>,
        record: &CanonicalRecord,
    ) -> Result<Option<(i64, String)>> {
        if record.is_deleted
            || record.scope != UsageScope::DeviceLocal
            || record.precision == Precision::Unknown
            || (record.usage.unclassified_tokens != 0 && record.tool_id != "antigravity")
        {
            return Ok(None);
        }
        let keywords = self.preferences.keyword_matching;
        if let Some(id) = record.model_id.as_deref() {
            let candidates = model_candidates(&record.tool_id, id, keywords);
            let mut exact_manual = self
                .preferences
                .manual_entries
                .iter()
                .filter(|entry| entry.model_id == id);
            let unique_exact = exact_manual
                .next()
                .filter(|_| exact_manual.next().is_none());
            let manual = candidates
                .iter()
                .find_map(|(provider, model)| {
                    self.preferences
                        .manual_entries
                        .iter()
                        .find(|entry| entry.provider_id == *provider && entry.model_id == *model)
                })
                .or(unique_exact);
            if let Some(rate) = manual {
                let mut effective = rate.clone();
                effective.cache_read_rate_nanos = effective
                    .cache_read_rate_nanos
                    .or(effective.input_rate_nanos);
                effective.cache_write_rate_nanos = effective
                    .cache_write_rate_nanos
                    .or(effective.input_rate_nanos);
                let encoded = serde_json::to_vec(rate)
                    .map_err(|_| crate::error::StoreError::InvalidInput("manual price"))?;
                let revision = format!("manual:{:x}", Sha256::digest(encoded));
                return Ok(estimate_cost(record, &effective).map(|cost| (cost, revision)));
            }
            if let Some(version) = version {
                for (provider, model) in candidates {
                    if let Some(cost) = self
                        .rate_for(version, provider, model)?
                        .and_then(|rate| estimate_cost(record, &rate))
                    {
                        return Ok(Some((cost, version.to_owned())));
                    }
                }
            }
        }
        if let Some(version) = version.filter(|_| {
            keywords && record.tool_id == "antigravity" && record.usage.unclassified_tokens > 0
        }) {
            return Ok(self
                .rate_for(version, "google", ANTIGRAVITY_REFERENCE_MODEL)?
                .and_then(|rate| estimate_cost(record, &rate))
                .map(|cost| (cost, version.to_owned())));
        }
        Ok(None)
    }
}
fn estimate_cost(record: &CanonicalRecord, entry: &ModelPriceEntry) -> Option<i64> {
    if record.scope != UsageScope::DeviceLocal || record.precision == Precision::Unknown {
        return None;
    }
    if record.usage.input_uncached_tokens == 0
        && record.usage.cache_read_tokens == 0
        && record.usage.cache_write_tokens == 0
        && record.usage.output_tokens == 0
        && record.usage.unclassified_tokens == 0
    {
        return None;
    }

    let mut numerator = 0_i128;
    add_rate(
        &mut numerator,
        record.usage.unclassified_tokens,
        entry.input_rate_nanos,
    )?;
    add_rate(
        &mut numerator,
        record.usage.input_uncached_tokens,
        entry.input_rate_nanos,
    )?;
    add_rate(
        &mut numerator,
        record.usage.cache_read_tokens,
        entry.cache_read_rate_nanos,
    )?;
    add_rate(
        &mut numerator,
        record.usage.cache_write_tokens,
        entry.cache_write_rate_nanos,
    )?;
    if let Some(reasoning_rate) = entry
        .reasoning_rate_nanos
        .filter(|rate| Some(*rate) != entry.output_rate_nanos)
    {
        if record.usage.output_tokens > 0 {
            let reasoning = record.usage.reasoning_tokens?;
            let non_reasoning = record.usage.output_tokens.checked_sub(reasoning)?;
            add_rate(&mut numerator, non_reasoning, entry.output_rate_nanos)?;
            add_rate(&mut numerator, reasoning, Some(reasoning_rate))?;
        }
    } else {
        add_rate(
            &mut numerator,
            record.usage.output_tokens,
            entry.output_rate_nanos,
        )?;
    }
    let rounded = numerator
        .checked_add(RATE_SCALE / 2)?
        .checked_div(RATE_SCALE)?;
    i64::try_from(rounded).ok()
}

fn add_rate(total: &mut i128, tokens: u64, rate: Option<i64>) -> Option<()> {
    if tokens == 0 {
        return Some(());
    }
    let rate = rate?;
    let component = i128::from(tokens).checked_mul(i128::from(rate))?;
    *total = total.checked_add(component)?;
    Some(())
}

fn model_candidates<'a>(
    tool_id: &str,
    model_id: &'a str,
    keywords: bool,
) -> Vec<(&'static str, &'a str)> {
    if !keywords {
        if let Some((provider, model)) = model_id.split_once('/') {
            let exact_provider = match provider {
                "openai" => "openai",
                "anthropic" => "anthropic",
                "google" => "google",
                "deepseek" => "deepseek",
                "xiaomi" => "xiaomi",
                "opencode-go" => "opencode-go",
                _ => return Vec::new(),
            };
            return vec![(exact_provider, model)];
        }
    }
    let leaf = model_id.rsplit('/').next().unwrap_or_default();
    let model = leaf
        .strip_prefix("deepseek-")
        .filter(|id| keywords && id.starts_with("deepseek-"))
        .unwrap_or(leaf);
    let explicit =
        model_id
            .rsplit_once('/')
            .and_then(|(path, _)| match path.rsplit('/').next()? {
                "openai" | "codex" => Some("openai"),
                "anthropic" => Some("anthropic"),
                "google" | "antigravity" => Some("google"),
                "deepseek" | "deepseek-official" | "deepseek-account" => Some("deepseek"),
                "xiaomi" => Some("xiaomi"),
                "opencode-go" => Some("opencode-go"),
                _ => None,
            });
    let native = [
        ("gpt-", "openai"),
        ("claude-", "anthropic"),
        ("deepseek-", "deepseek"),
        ("gemini-", "google"),
        ("mimo-", "xiaomi"),
    ]
    .into_iter()
    .find_map(|(prefix, provider)| model.starts_with(prefix).then_some(provider));
    // A named channel has its own rates. If those are missing, a manufacturer's
    // price would be a different quote for the same model, not a fallback.
    let Some(provider) = explicit.or(native).or_else(|| provider_for_tool(tool_id)) else {
        return Vec::new();
    };
    let mut candidates = vec![(provider, model)];
    if !keywords {
        return candidates;
    }
    let alias = match (provider, model) {
        ("deepseek", "deepseek-flash" | "deepseek-v4.1-flash") => Some("deepseek-v4-flash"),
        ("deepseek", "deepseek-pro" | "deepseek-v4.1-pro") => Some("deepseek-v4-pro"),
        ("anthropic", model) => ["-minimal", "-low", "-medium", "-high", "-xhigh", "-max"]
            .iter()
            .find_map(|suffix| model.strip_suffix(suffix)),
        ("google", "gemini-3.8-flash-n" | "gemini-3.8-flash-tiered") => Some("gemini-3.8-flash"),
        _ => None,
    };
    if let Some(alias) = alias {
        candidates.push((provider, alias));
    }
    candidates
}

fn provider_for_tool(tool_id: &str) -> Option<&'static str> {
    match tool_id {
        "codex" => Some("openai"),
        "claude" => Some("anthropic"),
        "antigravity" => Some("google"),
        "deepseek-harness" => Some("deepseek"),
        _ => None,
    }
}
