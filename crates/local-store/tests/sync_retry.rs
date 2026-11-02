use tokenscope_core_domain::{
    stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{LocalIdentity, LocalStore, SourceCheckpoint};

fn setup() -> LocalStore {
    let mut db = LocalStore::open_in_memory().unwrap();
    db.init_identity(&LocalIdentity {
        device_id: "dev-test-001".into(),
        display_name: "Test PC".into(),
        time_zone: "UTC".into(),
    })
    .unwrap();
    db
}
fn checkpoint(at: i64) -> SourceCheckpoint {
    SourceCheckpoint {
        tool_id: "codex".into(),
        source_key: "test-source".into(),
        source_fingerprint: "file:test".into(),
        read_offset: 100,
        parser_version: 1,
        last_scan_ms: at,
        last_complete_scan_ms: Some(at),
        last_status: "complete".into(),
    }
}
fn record() -> CanonicalRecord {
    CanonicalRecord {
        record_id: stable_record_id("codex", b"source", b"event-1").unwrap(),
        owner_device_id: "dev-test-001".into(),
        tool_id: "codex".into(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Exact,
        account_id: None,
        model_id: Some("gpt-6".into()),
        project_id: None,
        session_id: None,
        event_at_ms: Some(1_000),
        usage: TokenBreakdown {
            input_uncached_tokens: 10,
            output_tokens: 5,
            reasoning_tokens: Some(2),
            ..Default::default()
        },
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    }
}

#[test]
fn retry_backoff_is_durable_and_due_query_respects_schedule() {
    let mut db = setup();
    let rec = record();
    db.apply_scan(
        None,
        &checkpoint(100),
        std::slice::from_ref(&rec),
        100,
        false,
    )
    .unwrap();
    let revision = db
        .get_record(&rec.record_id)
        .unwrap()
        .unwrap()
        .record_revision;
    assert_eq!(db.pending_outbox_due(200, 10_000).unwrap().len(), 1);
    assert_eq!(
        db.schedule_outbox_retry(&rec.record_id, revision, 10_000, "transport")
            .unwrap(),
        Some(15_000)
    );
    assert!(db.pending_outbox_due(200, 14_999).unwrap().is_empty());
    assert_eq!(db.pending_outbox_due(200, 15_000).unwrap().len(), 1);
    assert_eq!(
        db.schedule_outbox_retry(&rec.record_id, revision, 15_000, "transport")
            .unwrap(),
        Some(25_000)
    );
}

#[test]
fn superseded_revision_is_quarantined_until_a_new_revision_arrives() {
    let mut db = setup();
    let mut rec = record();
    db.apply_scan(
        None,
        &checkpoint(100),
        std::slice::from_ref(&rec),
        100,
        false,
    )
    .unwrap();
    let revision1 = db
        .get_record(&rec.record_id)
        .unwrap()
        .unwrap()
        .record_revision;
    assert!(db
        .block_outbox_revision(&rec.record_id, revision1, 200, "hub_superseded")
        .unwrap());
    assert!(db.pending_outbox_due(200, 1_000_000).unwrap().is_empty());
    assert_eq!(db.sync_outbox_status(1_000_000).unwrap().blocked, 1);
    rec.usage.input_uncached_tokens = 20;
    db.apply_scan(
        Some(&checkpoint(100)),
        &checkpoint(300),
        std::slice::from_ref(&rec),
        300,
        false,
    )
    .unwrap();
    let revision2 = db
        .get_record(&rec.record_id)
        .unwrap()
        .unwrap()
        .record_revision;
    assert_eq!(revision2, revision1 + 1);
    assert_eq!(db.pending_outbox_due(200, 1_000_000).unwrap().len(), 1);
    assert_eq!(db.sync_outbox_status(1_000_000).unwrap().blocked, 0);
}

#[test]
fn sync_state_records_attempt_and_success() {
    let mut db = setup();
    db.update_sync_state(100, "deferred", Some("transport"))
        .unwrap();
    let s = db.sync_outbox_status(100).unwrap();
    assert_eq!(s.last_status, "deferred");
    assert_eq!(s.last_attempt_ms, Some(100));
    db.update_sync_state(200, "success", None).unwrap();
    let s = db.sync_outbox_status(200).unwrap();
    assert_eq!(s.last_success_ms, Some(200));
    assert_eq!(s.last_error, None);
}
