use super::super::{
    models::{tool_display_name, RemoteDeviceUsage, RemoteUsageBreakdown, RemoteUsageTotals},
    sync::{read_remote_totals, REMOTE_TOTAL_COLUMNS},
    LocalStore,
};
use crate::error::{pos_u64, Result, StoreError};
use rusqlite::{params_from_iter, types::Value};
use std::collections::BTreeMap;

pub(super) struct RemoteDashboard {
    pub(super) total: RemoteUsageTotals,
    pub(super) devices: Vec<RemoteDeviceUsage>,
    pub(super) tools: Vec<RemoteUsageBreakdown>,
    pub(super) models: Vec<RemoteUsageBreakdown>,
    pub(super) projects: Vec<RemoteUsageBreakdown>,
}

#[derive(Default)]
struct Group {
    usage: RemoteUsageTotals,
    untimed: u64,
}

type Groups = BTreeMap<(String, Option<String>), Group>;

fn add_usage(target: &mut RemoteUsageTotals, source: &RemoteUsageTotals) -> Result<()> {
    macro_rules! add {
        ($($field:ident),+ $(,)?) => {$({
            target.$field = target.$field.checked_add(source.$field)
                .ok_or(StoreError::IntegerOutOfRange)?;
        })+};
    }
    add!(
        total_tokens,
        input_tokens,
        cache_read_tokens,
        cache_write_tokens,
        output_tokens,
        unclassified_tokens,
        reasoning_tokens,
        reasoning_known_records,
        record_count,
        priced_records,
        unpriced_records,
        exact_records,
        aggregate_records,
        estimated_records,
        unknown_precision_records,
    );
    if let Some(cost) = source.estimated_cost_microusd {
        target.estimated_cost_microusd = Some(
            target
                .estimated_cost_microusd
                .unwrap_or_default()
                .checked_add(cost)
                .ok_or(StoreError::IntegerOutOfRange)?,
        );
    }
    Ok(())
}

fn add_group(
    groups: &mut Groups,
    owner: &str,
    key: Option<String>,
    usage: &RemoteUsageTotals,
    untimed: u64,
) -> Result<()> {
    let group = groups.entry((owner.to_owned(), key)).or_default();
    add_usage(&mut group.usage, usage)?;
    group.untimed = group
        .untimed
        .checked_add(untimed)
        .ok_or(StoreError::IntegerOutOfRange)?;
    Ok(())
}

impl LocalStore {
    /// Aggregate each remote dimension from one SQL scan. Keep raw nullable
    /// keys separate until projection, just as SQLite GROUP BY does.
    pub(super) fn remote_dashboard(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<RemoteDashboard> {
        let mut sql = format!(
            "SELECT owner_device_id,json_extract(payload_json,'$.tool_id'),\
             json_extract(payload_json,'$.model_id'),json_extract(payload_json,'$.project_id'),\
             {REMOTE_TOTAL_COLUMNS},SUM(json_extract(payload_json,'$.event_at_ms') IS NULL)\
             FROM remote_usage_cache WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local'"
        );
        let mut args = Vec::<Value>::new();
        if let Some(from) = from_ms {
            sql.push_str(" AND json_extract(payload_json,'$.event_at_ms')>=?");
            args.push(from.into());
        }
        if let Some(to) = to_ms {
            sql.push_str(" AND json_extract(payload_json,'$.event_at_ms')<?");
            args.push(to.into());
        }
        sql.push_str(" GROUP BY 1,2,3,4");
        let mut statement = self.conn.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(args))?;
        let mut total = RemoteUsageTotals::default();
        let mut devices = BTreeMap::<String, RemoteUsageTotals>::new();
        let mut tools = Groups::new();
        let mut models = Groups::new();
        let mut projects = Groups::new();
        while let Some(row) = rows.next()? {
            let owner: String = row.get(0)?;
            let tool: Option<String> = row.get(1)?;
            let model: Option<String> = row.get(2)?;
            let project: Option<String> = row.get(3)?;
            let usage = read_remote_totals(row, 4)?;
            let untimed = pos_u64(row.get(18)?)?;
            add_usage(&mut total, &usage)?;
            add_usage(devices.entry(owner.clone()).or_default(), &usage)?;
            add_group(&mut tools, &owner, tool, &usage, untimed)?;
            add_group(&mut models, &owner, model, &usage, untimed)?;
            add_group(&mut projects, &owner, project, &usage, untimed)?;
        }
        let devices = devices
            .into_iter()
            .map(|(owner_device_id, usage)| RemoteDeviceUsage {
                owner_device_id,
                total_tokens: usage.total_tokens,
                record_count: usage.record_count,
                estimated_cost_microusd: usage.estimated_cost_microusd,
                usage,
            })
            .collect();
        Ok(RemoteDashboard {
            total,
            devices,
            tools: self.remote_groups(tools, "tool_id")?,
            models: self.remote_groups(models, "model_id")?,
            projects: self.remote_groups(projects, "project_id")?,
        })
    }

    fn remote_groups(
        &self,
        groups: Groups,
        dimension: &'static str,
    ) -> Result<Vec<RemoteUsageBreakdown>> {
        if groups.len() > 1000 {
            return Err(StoreError::InvalidInput(
                "remote breakdown cardinality exceeds 1000",
            ));
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by_key(|b| std::cmp::Reverse(b.1.usage.total_tokens));
        groups
            .into_iter()
            .map(|((owner_device_id, raw_key), group)| {
                let key = raw_key.unwrap_or_else(|| match dimension {
                    "tool_id" => "unknown-tool".into(),
                    "model_id" => "unknown-model".into(),
                    _ => "no-project".into(),
                });
                let label = match dimension {
                    "project_id" => self
                        .project_display_name(&key)?
                        .unwrap_or_else(|| "项目名称未提供".into()),
                    "tool_id" => tool_display_name(&key),
                    "model_id" if key == "unknown-model" => "来源未提供模型".into(),
                    _ => key.clone(),
                };
                Ok(RemoteUsageBreakdown {
                    owner_device_id,
                    key,
                    label,
                    total_tokens: group.usage.total_tokens,
                    record_count: group.usage.record_count,
                    estimated_cost_microusd: group.usage.estimated_cost_microusd,
                    exact_records: group.usage.exact_records,
                    aggregate_records: group.usage.aggregate_records,
                    estimated_records: group.usage.estimated_records,
                    unknown_precision_records: group.usage.unknown_precision_records,
                    untimed_records: group.untimed,
                })
            })
            .collect()
    }
}
