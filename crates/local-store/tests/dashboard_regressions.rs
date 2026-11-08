mod common;
use common::{cp, rec, setup};
use tokenscope_core_domain::{
    project_key, Precision, RevisionedRecord, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{
    CodexQuota, DashboardQuery, DashboardSnapshot, LocalStore, RemoteUsageChange,
};

fn snapshot(db: &LocalStore, days: &[(String, i64, i64)]) -> DashboardSnapshot {
    db.dashboard_snapshot(DashboardQuery {
        range: "all",
        from_ms: None,
        to_ms: None,
        daily_ranges: days,
        generated_at_ms: 200_000_000,
        time_zone: "UTC".into(),
        time_zone_valid: true,
        session_offset: 0,
    })
    .unwrap()
}

#[test]
fn devices_and_calendar_keep_real_components_precision_and_day_boundaries() {
    let mut db = setup();
    let boundary = 23 * 60 * 60 * 1000; // A short DST day followed by a 25-hour day.
    let days = vec![
        ("2026-03-08".into(), 0, boundary),
        ("2026-03-09".into(), boundary, 48 * 60 * 60 * 1000),
    ];
    let mut local = rec("local", 50);
    local.event_at_ms = Some(boundary - 1);
    db.apply_scan(None, &cp("local", 1), &[local], 1, false)
        .unwrap();
    let mut a = rec("a", 100);
    a.owner_device_id = "remote-a".into();
    a.event_at_ms = Some(boundary - 1);
    a.precision = Precision::Estimated;
    let mut b = rec("b", 0);
    b.owner_device_id = "remote-b".into();
    b.event_at_ms = Some(boundary);
    b.usage.output_tokens = 900;
    b.precision = Precision::Aggregate;
    b.estimated_cost_microusd = None;
    b.price_version = None;
    let mut untimed = rec("untimed", 7);
    untimed.owner_device_id = "remote-b".into();
    untimed.event_at_ms = None;
    db.apply_remote_changes(
        &[a, b, untimed]
            .into_iter()
            .enumerate()
            .map(|(i, r)| RemoteUsageChange {
                change_seq: i as u64 + 1,
                owner_device_id: r.owner_device_id.clone(),
                envelope: RevisionedRecord::from_store(r, 1).unwrap(),
            })
            .collect::<Vec<_>>(),
        3,
    )
    .unwrap();
    let s = snapshot(&db, &days);
    assert_eq!(
        s.daily.iter().map(|d| d.total_tokens).collect::<Vec<_>>(),
        vec![52, 0]
    );
    assert_eq!(s.remote_daily.len(), 2);
    assert_eq!(
        (
            s.remote_daily[0].usage.date.as_str(),
            s.remote_daily[0].usage.total_tokens
        ),
        ("2026-03-08", 102)
    );
    assert_eq!(
        (
            s.remote_daily[1].usage.date.as_str(),
            s.remote_daily[1].usage.total_tokens
        ),
        ("2026-03-09", 900)
    );
    let a = &s.remote_by_device[0].usage;
    assert_eq!(
        (
            a.input_tokens,
            a.output_tokens,
            a.estimated_records,
            a.exact_records
        ),
        (100, 2, 1, 0)
    );
    let b = &s.remote_by_device[1].usage;
    assert_eq!(
        (
            b.input_tokens,
            b.output_tokens,
            b.aggregate_records,
            b.priced_records
        ),
        (7, 902, 1, 1)
    );
    assert_eq!(s.remote.reasoning_tokens, 3);
    assert_eq!(
        s.remote_by_tool
            .iter()
            .map(|r| r.estimated_records)
            .sum::<u64>(),
        1
    );
    assert_eq!(
        s.remote_by_tool
            .iter()
            .map(|r| r.untimed_records)
            .sum::<u64>(),
        1
    );
}

#[test]
fn all_projects_reach_the_ui_before_name_merging() {
    let mut db = setup();
    let records = (0..30)
        .map(|i| {
            let mut r = rec(&format!("project-{i}"), i + 1);
            let key = project_key("codex", format!("workspace-{i}").as_bytes()).unwrap();
            db.upsert_project_name(&key, &format!("project-{}", i % 5), 1)
                .unwrap();
            r.project_id = Some(key);
            r
        })
        .collect::<Vec<_>>();
    db.apply_scan(None, &cp("projects", 1), &records, 1, false)
        .unwrap();
    let s = snapshot(&db, &[]);
    assert_eq!(s.by_project.len(), 30);
    assert_eq!(
        s.by_project.iter().map(|p| p.total_tokens).sum::<u64>(),
        s.local.total_tokens
    );
}

#[test]
fn grouped_remote_dashboard_matches_independent_queries_for_each_range() {
    let mut db = setup();
    let mut records = Vec::new();
    for i in 0..24 {
        let mut r = rec(&format!("remote-{i}"), i as u64 * 3);
        r.owner_device_id = format!("remote-{}", i % 3);
        r.model_id = (i % 4 != 0).then(|| format!("model-{}", i % 4));
        r.project_id = (i % 3 == 0)
            .then(|| project_key("codex", format!("project-{}", i % 2).as_bytes()).unwrap());
        r.event_at_ms = (i % 5 != 0).then_some(1000 + i * 100);
        if i % 3 == 0 {
            r.estimated_cost_microusd = None;
            r.price_version = None;
        }
        if i % 4 == 0 {
            r.precision = Precision::Aggregate;
        }
        if i == 22 {
            r.scope = UsageScope::Unknown;
        }
        if i == 23 {
            r.is_deleted = true;
            r.usage = TokenBreakdown::default();
            r.estimated_cost_microusd = None;
        }
        records.push(r);
    }
    let changes: Vec<_> = records
        .into_iter()
        .enumerate()
        .map(|(index, record)| RemoteUsageChange {
            change_seq: index as u64 + 1,
            owner_device_id: record.owner_device_id.clone(),
            envelope: RevisionedRecord::from_store(record, 1).unwrap(),
        })
        .collect();
    db.apply_remote_changes(&changes, changes.len() as u64)
        .unwrap();

    for (start, end) in [
        (None, None),
        (Some(1600), Some(2600)),
        (Some(2000), None),
        (None, Some(2000)),
    ] {
        let result = db
            .dashboard_snapshot(DashboardQuery {
                range: "all",
                from_ms: start,
                to_ms: end,
                daily_ranges: &[],
                generated_at_ms: 10_000,
                time_zone: "UTC".into(),
                time_zone_valid: true,
                session_offset: 0,
            })
            .unwrap();
        macro_rules! assert_json_eq {
            ($actual:expr, $expected:expr) => {
                assert_eq!(
                    serde_json::to_value($actual).unwrap(),
                    serde_json::to_value($expected).unwrap()
                );
            };
        }
        assert_json_eq!(
            result.remote,
            db.remote_usage_totals(None, start, end).unwrap()
        );
        assert_json_eq!(
            result.remote_by_device,
            db.remote_usage_by_device(start, end).unwrap()
        );
        assert_json_eq!(
            result.remote_by_tool,
            db.remote_usage_breakdown("tool_id", start, end).unwrap()
        );
        assert_json_eq!(
            result.remote_by_model,
            db.remote_usage_breakdown("model_id", start, end).unwrap()
        );
        assert_json_eq!(
            result.remote_by_project,
            db.remote_usage_breakdown("project_id", start, end).unwrap()
        );
    }
}

#[test]
fn earliest_event_ignores_missing_times_across_local_and_remote_sources() {
    let mut db = setup();
    assert_eq!(
        db.earliest_dashboard_event_ms("dev-test-001").unwrap(),
        None
    );
    let mut local = rec("earliest-local", 10);
    local.event_at_ms = None;
    db.apply_scan(None, &cp("earliest", 1), &[local], 1, false)
        .unwrap();
    let mut remote = rec("earliest-remote", 5);
    remote.owner_device_id = "remote-a".into();
    remote.event_at_ms = Some(2_000);
    db.apply_remote_changes(
        &[RemoteUsageChange {
            change_seq: 1,
            owner_device_id: remote.owner_device_id.clone(),
            envelope: RevisionedRecord::from_store(remote, 1).unwrap(),
        }],
        1,
    )
    .unwrap();
    assert_eq!(
        db.earliest_dashboard_event_ms("dev-test-001").unwrap(),
        Some(2_000)
    );
    let mut local = rec("earliest-local", 10);
    local.event_at_ms = Some(1_000);
    db.apply_scan(
        Some(&cp("earliest", 1)),
        &cp("earliest", 2),
        &[local],
        2,
        false,
    )
    .unwrap();
    assert_eq!(
        db.earliest_dashboard_event_ms("dev-test-001").unwrap(),
        Some(1_000)
    );
}

#[test]
fn checkpoints_and_quota_only_dirty_observations_while_edits_and_deletions_dirty_usage() {
    let mut db = setup();
    let checkpoint = cp("source", 1);
    let record = rec("event", 10);
    db.apply_scan(None, &checkpoint, std::slice::from_ref(&record), 1, false)
        .unwrap();
    let first = db.dashboard_revisions().unwrap();
    let next = cp("source", 2);
    db.apply_scan(
        Some(&checkpoint),
        &next,
        std::slice::from_ref(&record),
        2,
        false,
    )
    .unwrap();
    let scanned = db.dashboard_revisions().unwrap();
    assert_eq!(first.0, scanned.0);
    assert!(scanned.1 > first.1);
    db.upsert_codex_quotas(
        &[CodexQuota {
            source_id: "local".into(),
            pool_id: "codex".into(),
            window_duration_minutes: 300,
            remaining_basis_points: Some(5000),
            reset_at_ms: None,
            observed_at_ms: 3,
        }],
        3,
    )
    .unwrap();
    let quota = db.dashboard_revisions().unwrap();
    assert_eq!(quota.0, first.0);
    assert!(quota.1 > scanned.1);
    assert_eq!(db.dashboard_observations(4).unwrap().codex_quota.len(), 1);
    db.upsert_session_name("session", "codex", "First", 4)
        .unwrap();
    let named = db.dashboard_revisions().unwrap().0;
    db.upsert_session_name("session", "codex", "First", 5)
        .unwrap();
    assert_eq!(db.dashboard_revisions().unwrap().0, named);
    db.upsert_session_name("session", "codex", "Renamed", 6)
        .unwrap();
    assert!(db.dashboard_revisions().unwrap().0 > named);
    let before_delete = db.dashboard_revisions().unwrap().0;
    db.reconcile_complete_source(Some(&next), &cp("source", 7), [], 7)
        .unwrap();
    assert!(db.dashboard_revisions().unwrap().0 > before_delete);
    assert_eq!(snapshot(&db, &[]).local.total_tokens, 0);
}
