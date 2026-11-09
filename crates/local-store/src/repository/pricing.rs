use super::models::{ModelPriceCatalogState, ModelPriceEntry, ModelPriceInstallReport};
use super::records::{enqueue, read_record, write_record};
use super::LocalStore;
use crate::error::{Result, StoreError};
use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};
use tokenscope_core_domain::payload_hash;

const RATE_SCALE: i128 = 1_000_000_000;
const BACKFILL_BATCH_SIZE: i64 = 256;

impl LocalStore {
    pub fn model_price_catalog_state(&self) -> Result<ModelPriceCatalogState> {
        self.conn
            .query_row(
                "SELECT active_version,etag,last_attempt_ms,last_success_ms FROM model_price_state WHERE singleton=1",
                [],
                |row| {
                    Ok(ModelPriceCatalogState {
                        active_version: row.get(0)?,
                        etag: row.get(1)?,
                        last_attempt_ms: row.get(2)?,
                        last_success_ms: row.get(3)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    /// Read the immutable rates used by the active catalog.
    pub fn active_model_price_catalog(&self) -> Result<(Option<i64>, Vec<ModelPriceEntry>)> {
        let state = self.model_price_catalog_state()?;
        let entries = match state.active_version {
            Some(version) => load_entries(&self.conn, &version)?,
            None => Vec::new(),
        };
        Ok((state.last_success_ms, entries))
    }

    pub fn pricing_preferences(&self) -> Result<PricingPreferences> {
        self.conn
            .path()
            .filter(|path| !path.is_empty())
            .map_or_else(
                || Ok(PricingPreferences::default()),
                |path| PricingPreferences::read(std::path::Path::new(path)),
            )
    }

    pub fn backfill_unpriced_model_costs(&mut self, observed_at_ms: i64) -> Result<usize> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let count = backfill_unpriced_records(&tx, observed_at_ms)?;
        tx.commit()?;
        Ok(count)
    }

    pub fn mark_model_price_attempt(&mut self, attempted_at_ms: i64) -> Result<()> {
        if attempted_at_ms < 0 {
            return Err(StoreError::InvalidInput("model price attempt time"));
        }
        self.conn.execute(
            "UPDATE model_price_state SET last_attempt_ms=?1 WHERE singleton=1",
            [attempted_at_ms],
        )?;
        Ok(())
    }

    pub fn mark_model_price_not_modified(
        &mut self,
        attempted_at_ms: i64,
        etag: Option<&str>,
    ) -> Result<()> {
        validate_catalog_metadata(attempted_at_ms, etag)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active: Option<String> = tx.query_row(
            "SELECT active_version FROM model_price_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if active.is_none() {
            return Err(StoreError::DatabaseInconsistent(
                "price server returned 304 without a saved catalog",
            ));
        }
        tx.execute(
            "UPDATE model_price_state SET etag=COALESCE(?1,etag),last_attempt_ms=?2,last_success_ms=?2 WHERE singleton=1",
            params![etag, attempted_at_ms],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Installs an immutable public price snapshot and reprices only local,
    /// previously unpriced usage in the same transaction as its outbox updates.
    pub fn install_model_price_catalog(
        &mut self,
        price_version: &str,
        entries: &[ModelPriceEntry],
        etag: Option<&str>,
        fetched_at_ms: i64,
    ) -> Result<ModelPriceInstallReport> {
        validate_catalog(price_version, entries, fetched_at_ms, etag)?;
        let mut entries = entries.to_vec();
        entries.sort_by(|a, b| (&a.provider_id, &a.model_id).cmp(&(&b.provider_id, &b.model_id)));
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;

        let existing_count: Option<i64> = tx
            .query_row(
                "SELECT model_count FROM model_price_catalogs WHERE price_version=?1",
                [price_version],
                |row| row.get(0),
            )
            .optional()?;
        match existing_count {
            Some(count) => {
                let stored = load_entries(&tx, price_version)?;
                if usize::try_from(count).ok() != Some(entries.len()) || stored != entries {
                    return Err(StoreError::DatabaseInconsistent(
                        "price version does not match its saved snapshot",
                    ));
                }
            }
            None => {
                tx.execute(
                    "INSERT INTO model_price_catalogs(price_version,fetched_at_ms,model_count) VALUES(?1,?2,?3)",
                    params![price_version, fetched_at_ms, i64::try_from(entries.len()).map_err(|_| StoreError::IntegerOutOfRange)?],
                )?;
                {
                    let mut insert = tx.prepare(
                        "INSERT INTO model_price_rates(price_version,provider_id,model_id,input_rate_nanos,output_rate_nanos,cache_read_rate_nanos,cache_write_rate_nanos,reasoning_rate_nanos) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    )?;
                    for entry in &entries {
                        insert.execute(params![
                            price_version,
                            entry.provider_id,
                            entry.model_id,
                            entry.input_rate_nanos,
                            entry.output_rate_nanos,
                            entry.cache_read_rate_nanos,
                            entry.cache_write_rate_nanos,
                            entry.reasoning_rate_nanos
                        ])?;
                    }
                }
            }
        }
        tx.execute(
            "UPDATE model_price_state SET active_version=?1,etag=?2,last_attempt_ms=?3,last_success_ms=?3 WHERE singleton=1",
            params![price_version, etag, fetched_at_ms],
        )?;
        let backfilled_records = backfill_unpriced_records(&tx, fetched_at_ms)?;
        tx.commit()?;
        Ok(ModelPriceInstallReport {
            price_version: price_version.to_owned(),
            model_count: entries.len(),
            backfilled_records,
        })
    }
}

fn backfill_unpriced_records(tx: &Transaction<'_>, observed_at_ms: i64) -> Result<usize> {
    let owner: Option<String> = tx
        .query_row(
            "SELECT device_id FROM local_identity WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let Some(owner) = owner else {
        return Ok(0);
    };
    let mut books = PriceBookCache::new(tx)?;
    let mut last_id = String::new();
    let mut changed = 0usize;
    loop {
        let ids = {
            let mut statement = tx.prepare(
                "SELECT record_id FROM usage_records WHERE owner_device_id=?1 AND scope='device_local' AND is_deleted=0 AND estimated_cost_microusd IS NULL AND (unclassified_tokens=0 OR tool_id='antigravity') AND tool_id IN ('codex','claude','antigravity','deepseek-harness','pi') AND record_id>?2 ORDER BY record_id LIMIT ?3",
            )?;
            let rows = statement
                .query_map(params![owner, last_id, BACKFILL_BATCH_SIZE], |row| {
                    row.get::<_, String>(0)
                })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if ids.is_empty() {
            break;
        }
        for record_id in &ids {
            let mut envelope = read_record(tx, record_id)?
                .ok_or(StoreError::DatabaseInconsistent("price candidate missing"))?;
            if !books.price_record(&mut envelope.record)? {
                continue;
            }
            let revision = i64::try_from(envelope.record_revision)
                .map_err(|_| StoreError::RevisionOverflow)?
                .checked_add(1)
                .ok_or(StoreError::RevisionOverflow)?;
            let hash = payload_hash(&envelope.record)?;
            write_record(tx, &envelope.record, revision, &hash, observed_at_ms)?;
            enqueue(tx, record_id, revision, observed_at_ms)?;
            changed += 1;
        }
        last_id = ids.last().cloned().unwrap_or_default();
    }
    Ok(changed)
}

#[path = "pricing_calculation.rs"]
mod calculation;
#[path = "pricing_catalog.rs"]
mod pricing_catalog;
#[path = "pricing_preferences.rs"]
mod pricing_preferences;
pub use pricing_preferences::PricingPreferences;
#[cfg(test)]
#[path = "pricing_tests.rs"]
mod pricing_tests;
pub(super) use calculation::PriceBookCache;
use pricing_catalog::{load_entries, validate_catalog, validate_catalog_metadata};

#[path = "pricing_remote.rs"]
mod pricing_remote;
