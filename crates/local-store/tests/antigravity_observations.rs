use tokenscope_local_store::{
    AntigravityActivity, AntigravityQuota, LocalIdentity, LocalStore, SourceCheckpoint,
};

fn db() -> LocalStore {
    let mut s = LocalStore::open_in_memory().unwrap();
    s.init_identity(&LocalIdentity {
        device_id: "pc1".into(),
        display_name: "Office".into(),
        time_zone: "UTC".into(),
    })
    .unwrap();
    s
}
fn cp(fingerprint: &str, at: i64) -> SourceCheckpoint {
    SourceCheckpoint {
        tool_id: "antigravity".into(),
        source_key: "opaque-source".into(),
        source_fingerprint: fingerprint.into(),
        read_offset: 0,
        parser_version: 2,
        last_scan_ms: at,
        last_complete_scan_ms: Some(at),
        last_status: "complete".into(),
    }
}
fn activity() -> AntigravityActivity {
    AntigravityActivity {
        known_turns: 3,
        unknown_turns: 1,
        models: vec![("gemini-pro".into(), 2), ("claude-sonnet".into(), 1)],
    }
}
fn quota(at: i64, remaining: Option<u16>) -> AntigravityQuota {
    AntigravityQuota {
        source_id: "local-account-1".into(),
        pool_id: "gemini_pro".into(),
        window_id: "five_hour".into(),
        remaining_basis_points: remaining,
        reset_at_ms: None,
        observed_at_ms: at,
    }
}
#[test]
fn schema_migration_current_version() {
    assert_eq!(db().schema_version().unwrap(), 7);
}
#[test]
fn activity_and_usage_are_separate() {
    let mut s = db();
    s.reconcile_source_with_activity(None, &cp("fp1", 100), Vec::new(), 100, Some(&activity()))
        .unwrap();
    let a = s.antigravity_activity_summary().unwrap();
    assert_eq!((a.conversations, a.known_turns, a.unknown_turns), (1, 3, 1));
    assert_eq!(a.by_model.len(), 2);
    assert!(s.pending_outbox(50).unwrap().is_empty());
}
#[test]
fn activity_changes_replace_counts_not_add() {
    let mut s = db();
    let initial = cp("fp1", 100);
    s.reconcile_source_with_activity(None, &initial, Vec::new(), 100, Some(&activity()))
        .unwrap();
    let next = cp("fp2", 200);
    let replaced = AntigravityActivity {
        known_turns: 1,
        unknown_turns: 0,
        models: vec![("gemini-pro".into(), 1)],
    };
    s.reconcile_source_with_activity(Some(&initial), &next, Vec::new(), 200, Some(&replaced))
        .unwrap();
    let a = s.antigravity_activity_summary().unwrap();
    assert_eq!(
        (
            a.conversations,
            a.known_turns,
            a.unknown_turns,
            a.by_model.len()
        ),
        (1, 1, 0, 1)
    );
}

#[test]
fn missing_antigravity_source_keeps_activity_checkpoint_and_usage() {
    use std::collections::HashSet;
    use tokenscope_core_domain::{
        stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
    };

    let mut s = db();
    let initial = cp("fp1", 100);
    let record = CanonicalRecord {
        record_id: stable_record_id("antigravity", b"opaque-source", b"response-1").unwrap(),
        owner_device_id: "pc1".into(),
        tool_id: "antigravity".into(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Estimated,
        account_id: None,
        model_id: Some("gemini-pro".into()),
        project_id: None,
        session_id: None,
        event_at_ms: None,
        usage: TokenBreakdown {
            unclassified_tokens: 8_000,
            ..Default::default()
        },
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    };
    s.reconcile_source_with_activity(None, &initial, [record], 100, Some(&activity()))
        .unwrap();

    for observed_at_ms in [40_100, 70_100] {
        assert_eq!(
            s.reconcile_missing_sources("antigravity", &HashSet::new(), observed_at_ms)
                .unwrap(),
            0
        );
    }

    assert_eq!(
        s.checkpoint("antigravity", "opaque-source").unwrap(),
        Some(initial)
    );
    let pending = s.pending_outbox(10).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(!pending[0].envelope.record.is_deleted);
    assert_eq!(pending[0].envelope.record.usage.unclassified_tokens, 8_000);
    assert_eq!(s.antigravity_activity_summary().unwrap().known_turns, 3);
}
#[test]
fn invalid_activity_does_not_commit_checkpoint() {
    let mut s = db();
    let bad = AntigravityActivity {
        known_turns: 100,
        unknown_turns: 0,
        models: vec![("gemini-pro".into(), 1)],
    };
    assert!(s
        .reconcile_source_with_activity(None, &cp("fp", 100), Vec::new(), 100, Some(&bad))
        .is_err());
    assert!(s
        .checkpoint("antigravity", "opaque-source")
        .unwrap()
        .is_none());
}
#[test]
fn stale_checkpoint_does_not_overwrite_activity() {
    let mut s = db();
    let initial = cp("fp1", 100);
    s.reconcile_source_with_activity(None, &initial, Vec::new(), 100, Some(&activity()))
        .unwrap();
    assert!(s
        .reconcile_source_with_activity(None, &cp("fp2", 200), Vec::new(), 200, Some(&activity()))
        .is_err());
    assert_eq!(s.antigravity_activity_summary().unwrap().known_turns, 3);
}
#[test]
fn quota_is_separate_from_outbox_and_usage() {
    let mut s = db();
    assert_eq!(
        s.upsert_antigravity_quotas(&[quota(100, Some(0))], 120)
            .unwrap(),
        1
    );
    assert!(s.pending_outbox(10).unwrap().is_empty());
    assert!(s
        .antigravity_activity_summary()
        .unwrap()
        .by_model
        .is_empty());
    assert_eq!(
        s.antigravity_quota_snapshots(10).unwrap()[0].remaining_basis_points,
        Some(0)
    );
}
#[test]
fn stale_quota_cannot_override_newer_snapshot() {
    let mut s = db();
    assert_eq!(
        s.upsert_antigravity_quotas(&[quota(200, Some(9000))], 300)
            .unwrap(),
        1
    );
    assert_eq!(
        s.upsert_antigravity_quotas(&[quota(100, Some(100))], 300)
            .unwrap(),
        0
    );
    assert_eq!(
        s.antigravity_quota_snapshots(10).unwrap()[0].remaining_basis_points,
        Some(9000)
    );
}
#[test]
fn quota_unknown_is_distinct_from_zero() {
    let mut s = db();
    s.upsert_antigravity_quotas(&[quota(100, None)], 120)
        .unwrap();
    assert_eq!(
        s.antigravity_quota_snapshots(10).unwrap()[0].remaining_basis_points,
        None
    );
    s.upsert_antigravity_quotas(&[quota(150, Some(0))], 200)
        .unwrap();
    assert_eq!(
        s.antigravity_quota_snapshots(10).unwrap()[0].remaining_basis_points,
        Some(0)
    );
}
#[test]
fn invalid_quota_batch_is_atomic() {
    let mut s = db();
    let mut bad = quota(100, Some(5000));
    bad.pool_id = "has spaces".into();
    assert!(s
        .upsert_antigravity_quotas(&[quota(100, Some(5000)), bad], 120)
        .is_err());
    assert!(s.antigravity_quota_snapshots(10).unwrap().is_empty());
}
#[test]
fn duplicated_quota_key_is_rejected() {
    let mut s = db();
    assert!(s
        .upsert_antigravity_quotas(&[quota(100, None), quota(101, Some(0))], 120)
        .is_err());
    assert!(s.antigravity_quota_snapshots(10).unwrap().is_empty());
}
#[test]
fn quota_future_timestamp_rejected() {
    let mut s = db();
    assert!(s
        .upsert_antigravity_quotas(&[quota(200000, Some(2000))], 100)
        .is_err());
}

#[test]
fn existing_v1_identity_survives_upgrade() {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path =
        std::env::temp_dir().join(format!("tokenscope-m009-{}-{stamp}.db", std::process::id()));
    {
        // Set up genuine v1 schema and identity
        let con = rusqlite::Connection::open(&path).unwrap();
        con.execute_batch(tokenscope_local_store::schema::MIGRATION_001)
            .unwrap();
        con.execute(
            "INSERT INTO schema_migrations(version,applied_at_ms) VALUES (1,1000)",
            [],
        )
        .unwrap();
        con.execute("INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES (1,'v1-owner','Before','UTC')", []).unwrap();
        con.execute_batch("PRAGMA user_version=1;").unwrap();
    }
    {
        let s = LocalStore::open(&path).unwrap();
        assert_eq!(s.schema_version().unwrap(), 7);
        assert_eq!(s.identity().unwrap().unwrap().device_id, "v1-owner");
        assert_eq!(s.antigravity_activity_summary().unwrap().conversations, 0);
    }
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(path.with_extension("db-wal"));
    let _ = fs::remove_file(path.with_extension("db-shm"));
}
