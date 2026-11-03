mod activity;
mod dashboard;
mod filters;
mod sessions;
mod usage;

pub(super) const SESSION_LINKS_CTE: &str = r#"
WITH session_links AS (
    SELECT u.tool_id,u.session_id,u.model_id,u.project_id
    FROM usage_records u
    WHERE u.owner_device_id=?1 AND u.scope='device_local' AND u.is_deleted=0
        AND u.session_id IS NOT NULL AND u.tool_id<>'antigravity'
        AND (?2 IS NULL OR u.event_at_ms >= ?2) AND (?3 IS NULL OR u.event_at_ms < ?3)
    UNION ALL
    SELECT 'antigravity',rs.source_key,u.model_id,u.project_id
    FROM record_sources rs JOIN usage_records u ON u.record_id=rs.record_id
    WHERE rs.tool_id='antigravity' AND u.owner_device_id=?1
        AND u.scope='device_local' AND u.is_deleted=0
        AND (?2 IS NULL OR u.event_at_ms >= ?2) AND (?3 IS NULL OR u.event_at_ms < ?3)
)
"#;
#[cfg(test)]
mod dashboard_tests {
    use crate::repository::{DashboardQuery, LocalIdentity, LocalStore, SourceCheckpoint};
    use tokenscope_core_domain::{CanonicalRecord, Precision, TokenBreakdown, UsageScope};

    fn record(id: char, scope: UsageScope, event_at_ms: Option<i64>) -> CanonicalRecord {
        CanonicalRecord {
            record_id: id.to_string().repeat(64),
            owner_device_id: "device-a".into(),
            tool_id: "codex".into(),
            scope,
            precision: Precision::Aggregate,
            account_id: if scope == UsageScope::AccountGlobal {
                Some(format!("acct_{}", "a".repeat(64)))
            } else {
                None
            },
            model_id: Some("gpt-5".into()),
            project_id: Some(format!("prj_{}", "b".repeat(64))),
            session_id: None,
            event_at_ms,
            usage: TokenBreakdown {
                input_uncached_tokens: 100,
                cache_read_tokens: 10,
                cache_write_tokens: 5,
                output_tokens: 50,
                unclassified_tokens: 0,
                reasoning_tokens: Some(20),
            },
            estimated_cost_microusd: Some(1250),
            price_version: Some("test-price-v1".into()),
            is_deleted: false,
        }
    }

    #[test]
    fn dashboard_keeps_scope_and_reasoning_semantics_separate() {
        let mut store = LocalStore::open_in_memory().unwrap();
        store
            .init_identity(&LocalIdentity {
                device_id: "device-a".into(),
                display_name: "Office PC".into(),
                time_zone: "Asia/Shanghai".into(),
            })
            .unwrap();
        let checkpoint = SourceCheckpoint {
            tool_id: "codex".into(),
            source_key: "test-source".into(),
            source_fingerprint: "fingerprint-v1".into(),
            read_offset: 100,
            parser_version: 1,
            last_scan_ms: 2_000,
            last_complete_scan_ms: Some(2_000),
            last_status: "complete".into(),
        };
        store
            .apply_scan(
                None,
                &checkpoint,
                &[
                    record('a', UsageScope::DeviceLocal, Some(1_500)),
                    record('b', UsageScope::Unknown, Some(1_500)),
                    record('c', UsageScope::AccountGlobal, Some(1_500)),
                ],
                2_000,
                false,
            )
            .unwrap();

        let snapshot = store
            .dashboard_snapshot(DashboardQuery {
                range: "7d",
                from_ms: Some(1_000),
                to_ms: Some(3_000),
                daily_ranges: &[("2026-11-02".into(), 1_000, 3_000)],
                generated_at_ms: 2_000,
                time_zone: "Asia/Shanghai".into(),
                time_zone_valid: true,
                session_offset: 0,
            })
            .unwrap();
        assert_eq!(snapshot.local.total_tokens, 165);
        assert_eq!(snapshot.local.reasoning_tokens, 20);
        assert_eq!(snapshot.local.record_count, 1);
        assert_eq!(snapshot.local.estimated_cost_microusd, Some(1250));
        assert_eq!(snapshot.unassigned.total_tokens, 165);
        assert_eq!(snapshot.account_global.total_tokens, 165);
        assert_eq!(snapshot.by_tool.len(), 1);
        assert_eq!(snapshot.by_model[0].label, "gpt-5");
        assert_eq!(snapshot.daily[0].total_tokens, 165);
    }

    #[test]
    fn dashboard_excludes_deleted_and_untimed_rows_from_calendar_buckets() {
        let mut store = LocalStore::open_in_memory().unwrap();
        store
            .init_identity(&LocalIdentity {
                device_id: "device-a".into(),
                display_name: "Office PC".into(),
                time_zone: "UTC".into(),
            })
            .unwrap();
        let checkpoint = SourceCheckpoint {
            tool_id: "codex".into(),
            source_key: "test-source".into(),
            source_fingerprint: "fingerprint-v1".into(),
            read_offset: 100,
            parser_version: 1,
            last_scan_ms: 2_000,
            last_complete_scan_ms: Some(2_000),
            last_status: "complete".into(),
        };
        store
            .apply_scan(
                None,
                &checkpoint,
                &[
                    record('a', UsageScope::DeviceLocal, Some(1_500)),
                    record('b', UsageScope::DeviceLocal, None),
                ],
                2_000,
                false,
            )
            .unwrap();
        let snapshot = store
            .dashboard_snapshot(DashboardQuery {
                range: "all",
                from_ms: None,
                to_ms: None,
                daily_ranges: &[("1970-01-01".into(), 1_000, 3_000)],
                generated_at_ms: 2_000,
                time_zone: "UTC".into(),
                time_zone_valid: true,
                session_offset: 0,
            })
            .unwrap();
        assert_eq!(snapshot.local.total_tokens, 330);
        assert_eq!(snapshot.daily[0].total_tokens, 165);
    }
}
