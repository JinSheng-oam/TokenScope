use super::super::models::{
    tool_display_name, ModelActivityBreakdown, ToolUsageOverview, UsageBreakdown, UsageOverview,
    UsageTotals,
};
use super::super::{LocalStore, PROJECT_LABEL_TOOL_ID};
use crate::error::{pos_u64, Result, StoreError};
use rusqlite::params;
use std::collections::HashMap;
impl LocalStore {
    pub fn earliest_dashboard_event_ms(&self, owner_device_id: &str) -> Result<Option<i64>> {
        let (local, remote): (Option<i64>, Option<i64>) = self.conn.query_row(
            "SELECT
                (SELECT MIN(event_at_ms) FROM usage_records
                 WHERE owner_device_id=?1 AND scope='device_local' AND is_deleted=0),
                (SELECT MIN(json_extract(payload_json,'$.event_at_ms')) FROM remote_usage_cache
                 WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local')",
            [owner_device_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(local.into_iter().chain(remote).min())
    }

    /// Aggregates local `device_local`, `unknown`, and `account_global` totals
    /// in a single table scan instead of three separate range queries.
    pub(super) fn usage_totals_by_scope(
        &self,
        owner_device_id: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<(UsageTotals, UsageTotals, UsageTotals)> {
        let mut stmt = self.conn.prepare(
            "SELECT scope, \
             COALESCE(SUM(input_uncached_tokens + cache_read_tokens + cache_write_tokens + output_tokens + unclassified_tokens),0), \
             COALESCE(SUM(input_uncached_tokens),0), COALESCE(SUM(cache_read_tokens),0), \
             COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(output_tokens),0), \
             COALESCE(SUM(unclassified_tokens),0), COALESCE(SUM(COALESCE(reasoning_tokens,0)),0), \
             COALESCE(SUM(CASE WHEN reasoning_tokens IS NOT NULL THEN 1 ELSE 0 END),0), \
             COUNT(*), COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN estimated_cost_microusd ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NULL THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='exact' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='aggregate' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='estimated' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='unknown' THEN 1 ELSE 0 END),0) \
             FROM usage_records WHERE owner_device_id=?1 \
             AND scope IN ('device_local','unknown','account_global') AND is_deleted=0 \
             AND (?2 IS NULL OR event_at_ms >= ?2) AND (?3 IS NULL OR event_at_ms < ?3) \
             GROUP BY scope",
        )?;
        let mut local = UsageTotals::default();
        let mut unassigned = UsageTotals::default();
        let mut account_global = UsageTotals::default();
        let mut rows = stmt.query(params![owner_device_id, from_ms, to_ms])?;
        while let Some(r) = rows.next()? {
            let scope: String = r.get(0)?;
            let priced = r.get::<_, i64>(11)?;
            let cost = r.get::<_, i64>(10)?;
            let totals = UsageTotals {
                total_tokens: pos_u64(r.get(1)?)?,
                input_tokens: pos_u64(r.get(2)?)?,
                cache_read_tokens: pos_u64(r.get(3)?)?,
                cache_write_tokens: pos_u64(r.get(4)?)?,
                output_tokens: pos_u64(r.get(5)?)?,
                unclassified_tokens: pos_u64(r.get(6)?)?,
                reasoning_tokens: pos_u64(r.get(7)?)?,
                reasoning_known_records: pos_u64(r.get(8)?)?,
                record_count: pos_u64(r.get(9)?)?,
                estimated_cost_microusd: if priced > 0 { Some(cost) } else { None },
                priced_records: pos_u64(priced)?,
                unpriced_records: pos_u64(r.get(12)?)?,
                exact_records: pos_u64(r.get(13)?)?,
                aggregate_records: pos_u64(r.get(14)?)?,
                estimated_records: pos_u64(r.get(15)?)?,
                unknown_precision_records: pos_u64(r.get(16)?)?,
            };
            match scope.as_str() {
                "device_local" => local = totals,
                "unknown" => unassigned = totals,
                "account_global" => account_global = totals,
                _ => {}
            }
        }
        Ok((local, unassigned, account_global))
    }

    /// Bounded aggregate used by the diagnostics panel; never builds the full
    /// dashboard projection or scans per-record history in application memory.
    pub fn usage_overview(&self) -> Result<UsageOverview> {
        let mut stmt = self.conn.prepare(
            "SELECT tool_id, \
             COALESCE(SUM(input_uncached_tokens + cache_read_tokens + cache_write_tokens + output_tokens + unclassified_tokens),0), \
             COUNT(*), MAX(event_at_ms) \
             FROM usage_records WHERE scope='device_local' AND is_deleted=0 \
             AND owner_device_id=(SELECT device_id FROM local_identity WHERE singleton=1) \
             GROUP BY tool_id ORDER BY 2 DESC,1",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i64>>(3)?,
            ))
        })?;
        let mut overview = UsageOverview::default();
        for row in rows {
            let (tool_id, total, count, last_event_at_ms) = row?;
            let total = pos_u64(total)?;
            let count = pos_u64(count)?;
            overview.total_tokens = overview.total_tokens.saturating_add(total);
            overview.record_count = overview.record_count.saturating_add(count);
            if let (Some(current), Some(next)) = (overview.last_event_at_ms, last_event_at_ms) {
                overview.last_event_at_ms = Some(current.max(next));
            } else if overview.last_event_at_ms.is_none() {
                overview.last_event_at_ms = last_event_at_ms;
            }
            overview.tools.push(ToolUsageOverview {
                tool_id,
                total_tokens: total,
                record_count: count,
                last_event_at_ms,
            });
        }
        Ok(overview)
    }

    pub(super) fn usage_breakdown(
        &self,
        column: &'static str,
        owner_device_id: &str,
        scope: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<UsageBreakdown>> {
        // `column` is a compile-time allowlist supplied only by dashboard_snapshot.
        if !["tool_id", "model_id", "project_id"].contains(&column) {
            return Err(StoreError::InvalidInput("dashboard breakdown column"));
        }
        let sql = format!(
            "SELECT {column}, COUNT(*), \
             COALESCE(SUM(input_uncached_tokens + cache_read_tokens + cache_write_tokens + output_tokens + unclassified_tokens),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN estimated_cost_microusd ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN estimated_cost_microusd IS NOT NULL THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='exact' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='aggregate' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='estimated' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN precision='unknown' THEN 1 ELSE 0 END),0), \
             COALESCE(SUM(CASE WHEN event_at_ms IS NULL THEN 1 ELSE 0 END),0) \
             FROM usage_records WHERE owner_device_id=?1 AND scope=?2 AND is_deleted=0 \
             AND (?3 IS NULL OR event_at_ms >= ?3) AND (?4 IS NULL OR event_at_ms < ?4) \
             GROUP BY {column} ORDER BY 3 DESC,{column}"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![owner_device_id, scope, from_ms, to_ms], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
            ))
        })?;
        let project_names = if column == "project_id" {
            Some(self.project_display_names_map()?)
        } else {
            None
        };
        let mut result = Vec::new();
        for row in rows {
            let (
                raw_key,
                count,
                total,
                cost,
                priced,
                exact,
                aggregate,
                estimated,
                unknown,
                untimed,
            ) = row?;
            let (key, label) = match (column, raw_key) {
                ("tool_id", Some(value)) => (value.clone(), tool_display_name(&value)),
                ("model_id", Some(value)) => (value.clone(), value),
                ("project_id", Some(value)) => {
                    let label = project_names
                        .as_ref()
                        .and_then(|names| names.get(&value).cloned())
                        .unwrap_or_else(|| "项目名称未提供".into());
                    (value, label)
                }
                ("tool_id", None) => ("unknown-tool".into(), "未知工具".into()),
                ("model_id", None) => ("unknown-model".into(), "来源未提供模型".into()),
                ("project_id", None) => ("no-project".into(), "未关联项目".into()),
                _ => return Err(StoreError::InvalidInput("dashboard breakdown column")),
            };
            result.push(UsageBreakdown {
                key,
                label,
                total_tokens: pos_u64(total)?,
                record_count: pos_u64(count)?,
                estimated_cost_microusd: if priced > 0 { Some(cost) } else { None },
                exact_records: pos_u64(exact)?,
                aggregate_records: pos_u64(aggregate)?,
                estimated_records: pos_u64(estimated)?,
                unknown_precision_records: pos_u64(unknown)?,
                untimed_records: pos_u64(untimed)?,
            });
        }
        Ok(result)
    }

    pub(super) fn model_activity_breakdown(
        &self,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<Vec<ModelActivityBreakdown>> {
        let mut activity = Vec::new();
        let mut stmt = self.conn.prepare(
            "SELECT COALESCE(u.model_id,'unknown-model'),COUNT(DISTINCT u.record_id) \
             FROM usage_records u \
             JOIN record_sources rs ON rs.record_id=u.record_id AND rs.tool_id='codex' \
             JOIN source_checkpoints cp ON cp.tool_id=rs.tool_id AND cp.source_key=rs.source_key \
             WHERE u.tool_id='codex' AND u.scope='device_local' AND u.is_deleted=0 \
               AND cp.source_fingerprint LIKE '%\"mode\":\"modern\"%' \
               AND (?1 IS NULL OR u.event_at_ms >= ?1) AND (?2 IS NULL OR u.event_at_ms < ?2) \
             GROUP BY u.model_id ORDER BY 2 DESC,1",
        )?;
        let rows = stmt.query_map(params![from_ms, to_ms], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (model_id, response_count) = row?;
            activity.push(ModelActivityBreakdown {
                tool_id: "codex".into(),
                model_id,
                response_count: pos_u64(response_count)?,
                time_scope: "selected_range".into(),
            });
        }
        // The source snapshot has counts but no event times. On bounded ranges,
        // only actual responses with a recorded event time can be attributed.
        let all_time = from_ms.is_none() && to_ms.is_none();
        let sql = if all_time {
            "SELECT model_id,SUM(response_count) FROM antigravity_model_activity
             GROUP BY model_id ORDER BY 2 DESC,1"
        } else {
            "SELECT model_id,SUM(response_count) FROM (
               SELECT COALESCE(model_id,'unknown-model') AS model_id,COUNT(*) AS response_count
               FROM usage_records
               WHERE tool_id='antigravity' AND scope='device_local' AND is_deleted=0
                 AND precision IN ('exact','aggregate') AND event_at_ms IS NOT NULL
                 AND (?1 IS NULL OR event_at_ms >= ?1) AND (?2 IS NULL OR event_at_ms < ?2)
               GROUP BY model_id
               UNION ALL
               SELECT model_id,response_count FROM antigravity_dated_unmeasured_activity
               WHERE (?1 IS NULL OR event_at_ms >= ?1) AND (?2 IS NULL OR event_at_ms < ?2)
             ) GROUP BY model_id ORDER BY 2 DESC,1"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let bounds: &[&dyn rusqlite::ToSql] = if all_time { &[] } else { &[&from_ms, &to_ms] };
        let rows = stmt.query_map(bounds, |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for row in rows {
            let (model_id, response_count) = row?;
            activity.push(ModelActivityBreakdown {
                tool_id: "antigravity".into(),
                model_id,
                response_count: pos_u64(response_count)?,
                time_scope: if all_time {
                    "all_time"
                } else {
                    "selected_range"
                }
                .into(),
            });
        }
        activity.sort_by(|a, b| {
            b.response_count
                .cmp(&a.response_count)
                .then_with(|| a.tool_id.cmp(&b.tool_id))
                .then_with(|| a.model_id.cmp(&b.model_id))
        });
        Ok(activity)
    }
    pub(in crate::repository) fn project_display_names_map(
        &self,
    ) -> Result<HashMap<String, String>> {
        let mut stmt = self.conn.prepare(
            "SELECT substr(session_id, 9), name FROM session_names \
             WHERE session_id >= 'project:' AND session_id < 'project;' AND tool_id=?1",
        )?;
        let rows = stmt.query_map([PROJECT_LABEL_TOOL_ID], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
    }
}
