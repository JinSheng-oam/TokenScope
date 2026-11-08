use super::{LocalStore, PriceBookCache};
use crate::error::{Result, StoreError};
use crate::{DashboardSnapshot, RemoteUsageTotals};
use rusqlite::{params_from_iter, types::Value};
use std::collections::HashMap;
use tokenscope_core_domain::CanonicalRecord;

#[derive(Default)]
struct Estimate {
    cost: Option<i64>,
    count: u64,
}

fn add_cost(current: &mut Option<i64>, cost: i64) -> Result<()> {
    *current = Some(
        current
            .unwrap_or_default()
            .checked_add(cost)
            .ok_or(StoreError::IntegerOutOfRange)?,
    );
    Ok(())
}

impl Estimate {
    fn add(&mut self, cost: i64) -> Result<()> {
        add_cost(&mut self.cost, cost)?;
        self.count += 1;
        Ok(())
    }

    fn apply(&self, totals: &mut RemoteUsageTotals) -> Result<()> {
        if let Some(cost) = self.cost {
            add_cost(&mut totals.estimated_cost_microusd, cost)?;
            totals.priced_records += self.count;
            totals.unpriced_records -= self.count;
        }
        Ok(())
    }
}

type BreakdownCosts = HashMap<(String, String), Option<i64>>;

impl LocalStore {
    /// Fill display-only estimates for peers that have not priced their records yet.
    pub(in crate::repository) fn estimate_remote_dashboard_costs(
        &self,
        snapshot: &mut DashboardSnapshot,
        start_ms: Option<i64>,
        end_ms: Option<i64>,
    ) -> Result<()> {
        if snapshot.remote.unpriced_records == 0 {
            return Ok(());
        }
        let mut total = Estimate::default();
        let mut devices = HashMap::<String, Estimate>::new();
        let mut tools = BreakdownCosts::new();
        let mut models = BreakdownCosts::new();
        let mut projects = BreakdownCosts::new();
        let mut prices = PriceBookCache::new(&self.conn)?;
        if !prices.has_rates() {
            return Ok(());
        }
        let mut sql = String::from(
            "SELECT payload_json FROM remote_usage_cache
            WHERE is_deleted=0 AND json_extract(payload_json, '$.scope')='device_local'
            AND json_extract(payload_json, '$.estimated_cost_microusd') IS NULL",
        );
        let mut args = Vec::<Value>::new();
        if let Some(start) = start_ms {
            sql.push_str(" AND json_extract(payload_json, '$.event_at_ms')>=?");
            args.push(start.into());
        }
        if let Some(end) = end_ms {
            sql.push_str(" AND json_extract(payload_json, '$.event_at_ms')<?");
            args.push(end.into());
        }
        let mut query = self.conn.prepare(&sql)?;
        let mut rows = query.query(params_from_iter(args))?;
        while let Some(row) = rows.next()? {
            let payload: String = row.get(0)?;
            let Ok(record) = serde_json::from_str::<CanonicalRecord>(&payload) else {
                continue;
            };
            if let Some(cost) = prices.estimate_current(&record)? {
                total.add(cost)?;
                devices
                    .entry(record.owner_device_id.clone())
                    .or_default()
                    .add(cost)?;
                let owner = record.owner_device_id;
                for (map, key) in [
                    (&mut tools, record.tool_id),
                    (
                        &mut models,
                        record.model_id.unwrap_or_else(|| "unknown-model".into()),
                    ),
                    (
                        &mut projects,
                        record.project_id.unwrap_or_else(|| "no-project".into()),
                    ),
                ] {
                    add_cost(map.entry((owner.clone(), key)).or_default(), cost)?;
                }
            }
        }
        total.apply(&mut snapshot.remote)?;
        for device in &mut snapshot.remote_by_device {
            if let Some(estimate) = devices.get(&device.owner_device_id) {
                estimate.apply(&mut device.usage)?;
                device.estimated_cost_microusd = device.usage.estimated_cost_microusd;
            }
        }
        for (rows, mut costs) in [
            (&mut snapshot.remote_by_tool, tools),
            (&mut snapshot.remote_by_model, models),
            (&mut snapshot.remote_by_project, projects),
        ] {
            for row in rows {
                if let Some(Some(cost)) =
                    costs.remove(&(row.owner_device_id.clone(), row.key.clone()))
                {
                    add_cost(&mut row.estimated_cost_microusd, cost)?;
                }
            }
        }
        Ok(())
    }
}
