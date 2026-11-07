use crate::error::{Result, StoreError};
use crate::repository::models::ModelPriceEntry;
use rusqlite::Connection;
use std::collections::HashSet;

pub(super) fn validate_catalog_metadata(at: i64, etag: Option<&str>) -> Result<()> {
    if at < 0 {
        return Err(StoreError::InvalidInput("model price catalog time"));
    }
    if etag.is_some_and(|value| {
        value.is_empty() || value.len() > 512 || value.chars().any(char::is_control)
    }) {
        return Err(StoreError::InvalidInput("model price catalog etag"));
    }
    Ok(())
}

pub(super) fn validate_catalog(
    version: &str,
    entries: &[ModelPriceEntry],
    fetched_at_ms: i64,
    etag: Option<&str>,
) -> Result<()> {
    validate_catalog_metadata(fetched_at_ms, etag)?;
    if version.len() != 75
        || !version.starts_with("models.dev:")
        || !version[11..].bytes().all(|byte| byte.is_ascii_hexdigit())
        || entries.is_empty()
        || entries.len() > 50_000
    {
        return Err(StoreError::InvalidInput("model price catalog"));
    }
    let mut identities = HashSet::with_capacity(entries.len());
    for entry in entries {
        let valid_provider = !entry.provider_id.is_empty()
            && entry.provider_id.len() <= 128
            && entry.provider_id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            });
        let valid_model = !entry.model_id.trim().is_empty()
            && entry.model_id.len() <= 256
            && !entry.model_id.chars().any(char::is_control);
        let rates = [
            entry.input_rate_nanos,
            entry.output_rate_nanos,
            entry.cache_read_rate_nanos,
            entry.cache_write_rate_nanos,
            entry.reasoning_rate_nanos,
        ];
        if !valid_provider
            || !valid_model
            || rates.iter().flatten().any(|rate| *rate < 0)
            || rates.iter().all(Option::is_none)
            || !identities.insert((entry.provider_id.as_str(), entry.model_id.as_str()))
        {
            return Err(StoreError::InvalidInput("model price entry"));
        }
    }
    Ok(())
}

pub(super) fn load_entries(conn: &Connection, version: &str) -> Result<Vec<ModelPriceEntry>> {
    let mut statement = conn.prepare(
        "SELECT provider_id,model_id,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,reasoning_rate_nanos FROM model_price_rates WHERE price_version=?1 ORDER BY provider_id,model_id",
    )?;
    let rows = statement.query_map([version], |row| {
        Ok(ModelPriceEntry {
            provider_id: row.get(0)?,
            model_id: row.get(1)?,
            input_rate_nanos: row.get(2)?,
            output_rate_nanos: row.get(3)?,
            cache_read_rate_nanos: row.get(4)?,
            cache_write_rate_nanos: row.get(5)?,
            reasoning_rate_nanos: row.get(6)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}
