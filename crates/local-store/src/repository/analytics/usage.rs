use super::super::models::{
    tool_display_name, ModelActivityBreakdown, ToolUsageOverview, UsageBreakdown, UsageOverview,
    UsageTotals,
};
use super::super::{LocalStore, PROJECT_LABEL_TOOL_ID};
use crate::error::{pos_u64, Result, StoreError};
use rusqlite::{params, OptionalExtension};
impl LocalStore {
    pub fn earliest_dashboard_event_ms(&self, owner_device_id: &str) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT MIN(event_at_ms) FROM (
                    SELECT event_at_ms FROM usage_records WHERE owner_device_id=?1 AND scope='device_local' AND is_deleted=0
                    UNION ALL SELECT json_extract(payload_json,'$.event_at_ms') FROM remote_usage_cache
                    WHERE is_deleted=0 AND json_extract(payload_json,'$.scope')='device_local'
                 )",
                [owner_device_id],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    pub(super) fn usage_totals(
        &self,
        owner_device_id: &str,
        scope: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<UsageTotals> {
        let row = self.conn.query_row(
            "SELECT \
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
             FROM usage_records WHERE owner_device_id=?1 AND scope=?2 AND is_deleted=0 \
             AND (?3 IS NULL OR event_at_ms >= ?3) AND (?4 IS NULL OR event_at_ms < ?4)",
            params![owner_device_id, scope, from_ms, to_ms],
            |r| Ok((
                r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,
                r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,i64>(7)?,
                r.get::<_,i64>(8)?,r.get::<_,i64>(9)?,r.get::<_,i64>(10)?,r.get::<_,i64>(11)?,
                r.get::<_,i64>(12)?,r.get::<_,i64>(13)?,r.get::<_,i64>(14)?,r.get::<_,i64>(15)?,
            )),
        )?;
        let (
            total,
            input,
            cache_read,
            cache_write,
            output,
            unclassified,
            reasoning,
            reasoning_known,
            count,
            cost,
            priced,
            unpriced,
            exact,
            aggregate,
            estimated,
            unknown,
        ) = row;
        Ok(UsageTotals {
            total_tokens: pos_u64(total)?,
            input_tokens: pos_u64(input)?,
            cache_read_tokens: pos_u64(cache_read)?,
            cache_write_tokens: pos_u64(cache_write)?,
            output_tokens: pos_u64(output)?,
            unclassified_tokens: pos_u64(unclassified)?,
            reasoning_tokens: pos_u64(reasoning)?,
            reasoning_known_records: pos_u64(reasoning_known)?,
            record_count: pos_u64(count)?,
            estimated_cost_microusd: if priced > 0 { Some(cost) } else { None },
            priced_records: pos_u64(priced)?,
            unpriced_records: pos_u64(unpriced)?,
            exact_records: pos_u64(exact)?,
            aggregate_records: pos_u64(aggregate)?,
            estimated_records: pos_u64(estimated)?,
            unknown_precision_records: pos_u64(unknown)?,
        })
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
                    let label = self
                        .project_display_name(&value)?
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
    pub(in crate::repository) fn project_display_name(
        &self,
        project_id: &str,
    ) -> Result<Option<String>> {
        let cache_key = format!("project:{project_id}");
        Ok(self
            .conn
            .query_row(
                "SELECT name FROM session_names WHERE session_id=?1 AND tool_id=?2",
                params![cache_key, PROJECT_LABEL_TOOL_ID],
                |row| row.get(0),
            )
            .optional()?)
    }
}
