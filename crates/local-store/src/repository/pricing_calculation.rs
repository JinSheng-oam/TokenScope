use super::RATE_SCALE;
use crate::error::Result;
use crate::repository::models::ModelPriceEntry;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use tokenscope_core_domain::{CanonicalRecord, Precision, UsageScope};

pub(in crate::repository) struct PriceBookCache<'a> {
    conn: &'a Connection,
    active_version: Option<String>,
    rates: HashMap<(String, String, String), Option<ModelPriceEntry>>,
}

impl<'a> PriceBookCache<'a> {
    pub(in crate::repository) fn new(conn: &'a Connection) -> Result<Self> {
        let active_version = conn.query_row(
            "SELECT active_version FROM model_price_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        let cache = Self {
            conn,
            active_version,
            rates: HashMap::new(),
        };
        Ok(cache)
    }

    pub(super) fn for_version(conn: &'a Connection, version: &str) -> Result<Self> {
        let cache = Self {
            conn,
            active_version: Some(version.to_owned()),
            rates: HashMap::new(),
        };
        Ok(cache)
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
            .or_else(|| self.active_version.clone());
        let Some(version) = version else {
            return Ok(false);
        };
        if record.scope != UsageScope::DeviceLocal
            || record.precision == Precision::Unknown
            || record.usage.unclassified_tokens != 0
        {
            return Ok(false);
        }
        let Some(provider) = provider_for_tool(&record.tool_id) else {
            return Ok(false);
        };
        let Some(model_id) = record.model_id.as_deref() else {
            return Ok(false);
        };
        let model_id = model_id
            .strip_prefix(&format!("{provider}/"))
            .unwrap_or(model_id);
        let Some(rate) = self.rate_for(&version, provider, model_id)? else {
            return Ok(false);
        };
        let Some(cost) = estimate_cost(record, &rate) else {
            return Ok(false);
        };
        record.estimated_cost_microusd = Some(cost);
        record.price_version = Some(version);
        Ok(true)
    }
}

fn estimate_cost(record: &CanonicalRecord, entry: &ModelPriceEntry) -> Option<i64> {
    if record.scope != UsageScope::DeviceLocal
        || record.precision == Precision::Unknown
        || record.usage.unclassified_tokens != 0
    {
        return None;
    }
    if record.usage.input_uncached_tokens == 0
        && record.usage.cache_read_tokens == 0
        && record.usage.cache_write_tokens == 0
        && record.usage.output_tokens == 0
    {
        return None;
    }

    let mut numerator = 0_i128;
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
    if let Some(reasoning_rate) = entry.reasoning_rate_nanos {
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

fn provider_for_tool(tool_id: &str) -> Option<&'static str> {
    match tool_id {
        "codex" => Some("openai"),
        "claude" => Some("anthropic"),
        "antigravity" => Some("google"),
        "deepseek-harness" => Some("deepseek"),
        _ => None,
    }
}
