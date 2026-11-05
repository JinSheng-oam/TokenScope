use super::super::models::{
    DailyUsage, DashboardObservations, DashboardQuery, DashboardSnapshot, RemoteDailyUsage,
    SourceHealth,
};
use super::super::LocalStore;
use crate::error::{pos_u64, Result, StoreError};
use crate::schema;
use rusqlite::params;
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
        let transaction = self.conn.unchecked_transaction()?;
        let data_revision = self.dashboard_revisions()?.0;
        let identity = self.identity()?;
        let owner_device_id = identity
            .as_ref()
            .map(|v| v.device_id.as_str())
            .unwrap_or("");
        let daily_started = Instant::now();
        let (daily, remote_daily) = self.daily_usage(owner_device_id, daily_ranges)?;
        if std::env::var_os("TOKENSCOPE_DASHBOARD_PROFILE").is_some() {
            eprintln!("daily: {:?}", daily_started.elapsed());
        }
        let source_health = self.source_health()?;
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
        let snapshot = DashboardSnapshot {
            data_revision,
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
            remote_daily,
            source_health,
            antigravity_quota: timed!("antigravity_quota", self.antigravity_quota_snapshots(200)?),
            codex_quota: timed!("codex_quota", self.codex_quota_snapshots(32)?),
            antigravity_activity: timed!(
                "antigravity_activity",
                self.antigravity_activity_summary()?
            ),
            sync_status: timed!("sync_status", self.sync_outbox_status(generated_at_ms)?),
        };
        transaction.commit()?;
        Ok(snapshot)
    }
    /// Constant-size polling, independent of the number of usage records.
    pub fn dashboard_revisions(&self) -> Result<(u64, u64)> {
        let (full, observations): (i64, i64) = self.conn.query_row(
            "SELECT full_revision,observation_revision FROM dashboard_revisions WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((pos_u64(full)?, pos_u64(observations)?))
    }

    pub fn dashboard_observations(&self, now: i64) -> Result<DashboardObservations> {
        let transaction = self.conn.unchecked_transaction()?;
        let observations = DashboardObservations {
            observed_at_ms: now,
            source_health: self.source_health()?,
            codex_quota: self.codex_quota_snapshots(32)?,
            antigravity_quota: self.antigravity_quota_snapshots(200)?,
            sync_status: self.sync_outbox_status(now)?,
        };
        transaction.commit()?;
        Ok(observations)
    }

    fn source_health(&self) -> Result<Vec<SourceHealth>> {
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
        Ok(health.into_values().collect())
    }

    /// The Agent supplies actual timezone boundaries (including DST days).
    /// Two batched range joins replace one full aggregate call per calendar day.
    fn daily_usage(
        &self,
        owner: &str,
        ranges: &[(String, i64, i64)],
    ) -> Result<(Vec<DailyUsage>, Vec<RemoteDailyUsage>)> {
        for (date, start, end) in ranges {
            if date.len() != 10 || *start < 0 || end <= start {
                return Err(StoreError::InvalidInput("dashboard daily range"));
            }
        }
        let ranges_json = serde_json::to_string(ranges)
            .map_err(|_| StoreError::InvalidInput("dashboard daily ranges"))?;
        let mut stmt = self.conn.prepare("SELECT json_extract(d.value,'$[0]'),
            COALESCE(SUM(u.input_uncached_tokens+u.cache_read_tokens+u.cache_write_tokens+u.output_tokens+u.unclassified_tokens),0),
            COALESCE(SUM(u.input_uncached_tokens),0),COALESCE(SUM(u.output_tokens),0)
            FROM json_each(?1) d LEFT JOIN usage_records u
            ON u.owner_device_id=?2 AND u.scope='device_local' AND u.is_deleted=0
            AND u.event_at_ms>=json_extract(d.value,'$[1]') AND u.event_at_ms<json_extract(d.value,'$[2]')
            GROUP BY d.key ORDER BY CAST(d.key AS INTEGER)")?;
        let rows = stmt.query_map(params![ranges_json, owner], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut local = Vec::with_capacity(ranges.len());
        for row in rows {
            let (date, total, input, output) = row?;
            local.push(DailyUsage {
                date,
                total_tokens: pos_u64(total)?,
                input_tokens: pos_u64(input)?,
                output_tokens: pos_u64(output)?,
            });
        }
        let mut stmt = self.conn.prepare("SELECT r.owner_device_id,json_extract(d.value,'$[0]'),
            SUM(json_extract(r.payload_json,'$.usage.input_uncached_tokens')+json_extract(r.payload_json,'$.usage.cache_read_tokens')+
                json_extract(r.payload_json,'$.usage.cache_write_tokens')+json_extract(r.payload_json,'$.usage.output_tokens')+
                json_extract(r.payload_json,'$.usage.unclassified_tokens')),
            SUM(json_extract(r.payload_json,'$.usage.input_uncached_tokens')),SUM(json_extract(r.payload_json,'$.usage.output_tokens'))
            FROM json_each(?1) d JOIN remote_usage_cache r
            ON r.is_deleted=0 AND json_extract(r.payload_json,'$.scope')='device_local'
            AND json_extract(r.payload_json,'$.event_at_ms')>=json_extract(d.value,'$[1]')
            AND json_extract(r.payload_json,'$.event_at_ms')<json_extract(d.value,'$[2]')
            GROUP BY r.owner_device_id,d.key ORDER BY r.owner_device_id,CAST(d.key AS INTEGER)")?;
        let rows = stmt.query_map([ranges_json], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut remote = Vec::new();
        for row in rows {
            let (owner_device_id, date, total, input, output) = row?;
            remote.push(RemoteDailyUsage {
                owner_device_id,
                usage: DailyUsage {
                    date,
                    total_tokens: pos_u64(total)?,
                    input_tokens: pos_u64(input)?,
                    output_tokens: pos_u64(output)?,
                },
            });
        }
        Ok((local, remote))
    }
}
