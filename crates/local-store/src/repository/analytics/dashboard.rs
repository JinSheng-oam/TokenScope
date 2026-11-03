use super::super::models::{DailyUsage, DashboardQuery, DashboardSnapshot, SourceHealth};
use super::super::LocalStore;
use crate::error::{Result, StoreError};
use crate::schema;
use std::time::Instant;
impl LocalStore {
    /// Read-only dashboard aggregate. Daily ranges are calculated by the Agent
    /// with the device IANA timezone, so DST boundaries are not assumed to be 24h.
    pub fn dashboard_snapshot(&self, query: DashboardQuery<'_>) -> Result<DashboardSnapshot> {
        let DashboardQuery {
            range,
            from_ms,
            to_ms,
            daily_ranges,
            generated_at_ms,
            time_zone,
            time_zone_valid,
            session_offset,
        } = query;
        if !["today", "7d", "30d", "all", "custom"].contains(&range)
            || generated_at_ms < 0
            || to_ms.is_some_and(|v| v < 0)
            || from_ms.is_some_and(|v| v < 0)
            || from_ms.zip(to_ms).is_some_and(|(a, b)| a >= b)
            || daily_ranges.len() > 3_660
        {
            return Err(StoreError::InvalidInput("dashboard query"));
        }
        let identity = self.identity()?;
        let owner_device_id = identity
            .as_ref()
            .map(|v| v.device_id.as_str())
            .unwrap_or("");
        let daily_started = Instant::now();
        let mut daily = Vec::with_capacity(daily_ranges.len());
        for (date, start, end) in daily_ranges {
            if date.len() != 10 || *start < 0 || *end <= *start {
                return Err(StoreError::InvalidInput("dashboard daily range"));
            }
            let totals =
                self.usage_totals(owner_device_id, "device_local", Some(*start), Some(*end))?;
            daily.push(DailyUsage {
                date: date.clone(),
                total_tokens: totals.total_tokens,
                input_tokens: totals.input_tokens,
                output_tokens: totals.output_tokens,
            });
        }
        if std::env::var_os("TOKENSCOPE_DASHBOARD_PROFILE").is_some() {
            eprintln!("daily: {:?}", daily_started.elapsed());
        }
        let mut health = std::collections::BTreeMap::<String, SourceHealth>::new();
        let mut stmt = self.conn.prepare(
            "SELECT tool_id,last_scan_ms,last_complete_scan_ms,last_status FROM source_checkpoints ORDER BY tool_id,last_scan_ms"
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?;
        for row in rows {
            let (tool, last_scan, last_complete, status) = row?;
            let entry = health.entry(tool.clone()).or_insert(SourceHealth {
                tool_id: tool,
                last_scan_ms: None,
                last_complete_scan_ms: None,
                status: "unknown".into(),
                source_count: 0,
            });
            entry.source_count += 1;
            if entry.last_scan_ms.is_none_or(|old| last_scan >= old) {
                entry.last_scan_ms = Some(last_scan);
                entry.status = status;
            }
            if let Some(value) = last_complete {
                if entry.last_complete_scan_ms.is_none_or(|old| value > old) {
                    entry.last_complete_scan_ms = Some(value);
                }
            }
        }
        let profile = std::env::var_os("TOKENSCOPE_DASHBOARD_PROFILE").is_some();
        macro_rules! timed {
            ($label:literal, $expr:expr) => {{
                let started = Instant::now();
                let value = $expr;
                if profile {
                    eprintln!("dashboard {}: {:?}", $label, started.elapsed());
                }
                value
            }};
        }
        Ok(DashboardSnapshot {
            schema_version: schema::VERSION,
            generated_at_ms,
            range: range.into(),
            range_start_ms: from_ms,
            range_end_ms: to_ms,
            time_zone,
            time_zone_valid,
            identity: identity.clone(),
            local: timed!(
                "local",
                self.usage_totals(owner_device_id, "device_local", from_ms, to_ms)?
            ),
            remote: timed!("remote", self.remote_usage_totals(None, from_ms, to_ms)?),
            remote_by_device: timed!(
                "remote_by_device",
                self.remote_usage_by_device(from_ms, to_ms)?
            ),
            remote_by_tool: timed!(
                "remote_by_tool",
                self.remote_usage_breakdown("tool_id", from_ms, to_ms)?
            ),
            remote_by_model: timed!(
                "remote_by_model",
                self.remote_usage_breakdown("model_id", from_ms, to_ms)?
            ),
            remote_by_project: timed!(
                "remote_by_project",
                self.remote_usage_breakdown("project_id", from_ms, to_ms)?
            ),
            unassigned: timed!(
                "unassigned",
                self.usage_totals(owner_device_id, "unknown", from_ms, to_ms)?
            ),
            account_global: timed!(
                "account_global",
                self.usage_totals(owner_device_id, "account_global", from_ms, to_ms)?
            ),
            by_tool: timed!(
                "by_tool",
                self.usage_breakdown("tool_id", owner_device_id, "device_local", from_ms, to_ms)?
            ),
            by_model: timed!(
                "by_model",
                self.usage_breakdown("model_id", owner_device_id, "device_local", from_ms, to_ms)?
            ),
            model_activity: timed!(
                "model_activity",
                self.model_activity_breakdown(from_ms, to_ms)?
            ),
            by_project: timed!(
                "by_project",
                self.usage_breakdown(
                    "project_id",
                    owner_device_id,
                    "device_local",
                    from_ms,
                    to_ms
                )?
            ),
            session_usage: timed!(
                "session_usage",
                self.session_usage_page(owner_device_id, from_ms, to_ms, session_offset, 50)?
            ),
            daily,
            source_health: health.into_values().collect(),
            antigravity_quota: timed!("antigravity_quota", self.antigravity_quota_snapshots(200)?),
            codex_quota: timed!("codex_quota", self.codex_quota_snapshots(32)?),
            antigravity_activity: timed!(
                "antigravity_activity",
                self.antigravity_activity_summary()?
            ),
            sync_status: timed!("sync_status", self.sync_outbox_status(generated_at_ms)?),
        })
    }
}
