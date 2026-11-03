use std::path::PathBuf;
use tokenscope_core_domain::{
    stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{LocalIdentity, LocalStore, SourceCheckpoint, StoreError};

fn identity() -> LocalIdentity {
    LocalIdentity {
        device_id: "dev-test-001".into(),
        display_name: "Dev".into(),
        time_zone: "Asia/Shanghai".into(),
    }
}
fn cp(key: &str, scan: i64) -> SourceCheckpoint {
    SourceCheckpoint {
        tool_id: "codex".into(),
        source_key: key.into(),
        source_fingerprint: "file:abc".into(),
        read_offset: 100,
        parser_version: 1,
        last_scan_ms: scan,
        last_complete_scan_ms: Some(scan),
        last_status: "complete".into(),
    }
}
fn rec(native: &str, tokens: u64) -> CanonicalRecord {
    CanonicalRecord {
        record_id: stable_record_id("codex", b"source-canonical", native.as_bytes()).unwrap(),
        owner_device_id: "dev-test-001".into(),
        tool_id: "codex".into(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Exact,
        account_id: None,
        model_id: Some("gpt-6".into()),
        project_id: None,
        session_id: None,
        event_at_ms: Some(1000),
        usage: TokenBreakdown {
            input_uncached_tokens: tokens,
            output_tokens: 2,
            reasoning_tokens: Some(1),
            ..Default::default()
        },
        estimated_cost_microusd: Some(125),
        price_version: Some("v1".into()),
        is_deleted: false,
    }
}
fn setup() -> LocalStore {
    let mut db = LocalStore::open_in_memory().unwrap();
    db.init_identity(&identity()).unwrap();
    db
}
fn unique_db() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tokenscope-test-{}-{nanos}.sqlite",
        std::process::id()
    ))
}

#[test]
fn migration_and_one_time_identity() {
    let mut db = setup();
    assert_eq!(db.schema_version().unwrap(), 6);
    db.init_identity(&identity()).unwrap();
    assert_eq!(db.identity().unwrap(), Some(identity()));
    let mut changed = identity();
    changed.device_id = "dev-foreign".into();
    assert!(matches!(
        db.init_identity(&changed),
        Err(StoreError::IdentityMismatch)
    ));
    assert_eq!(db.identity().unwrap(), Some(identity()));
}

#[test]
fn insert_replay_and_revision_replaces_instead_of_accumulating() {
    let mut db = setup();
    let a = rec("message-1", 100);
    let first = cp("src-a", 100);
    let report = db
        .apply_scan(None, &first, std::slice::from_ref(&a), 100, false)
        .unwrap();
    assert_eq!(
        (report.inserted, report.revised, report.unchanged),
        (1, 0, 0)
    );
    assert_eq!(
        db.get_record(&a.record_id)
            .unwrap()
            .unwrap()
            .record_revision,
        1
    );
    let second = cp("src-a", 101);
    let report = db
        .apply_scan(Some(&first), &second, std::slice::from_ref(&a), 101, false)
        .unwrap();
    assert_eq!(
        (report.inserted, report.revised, report.unchanged),
        (0, 0, 1)
    );
    let mut amended = a.clone();
    amended.usage.input_uncached_tokens = 30;
    let third = cp("src-a", 102);
    assert_eq!(
        db.apply_scan(
            Some(&second),
            &third,
            std::slice::from_ref(&amended),
            102,
            false
        )
        .unwrap()
        .revised,
        1
    );
    let envelope = db.get_record(&a.record_id).unwrap().unwrap();
    assert_eq!(envelope.record_revision, 2);
    assert_eq!(envelope.record.usage.input_uncached_tokens, 30);
    assert_eq!(
        db.pending_outbox(200).unwrap()[0].envelope.record_revision,
        2
    );
}

#[test]
fn checkpoint_cas_rejects_stale_scan_entirely() {
    let mut db = setup();
    let a = rec("one", 1);
    let first = cp("src-a", 100);
    db.apply_scan(None, &first, std::slice::from_ref(&a), 100, false)
        .unwrap();
    let second = cp("src-a", 101);
    let b = rec("two", 2);
    assert!(matches!(
        db.apply_scan(None, &second, std::slice::from_ref(&b), 101, false),
        Err(StoreError::CheckpointConflict)
    ));
    assert!(db.get_record(&b.record_id).unwrap().is_none());
    assert_eq!(db.checkpoint("codex", "src-a").unwrap(), Some(first));
}

#[test]
fn old_ack_cannot_remove_new_revision() {
    let mut db = setup();
    let first = cp("src-a", 10);
    let mut a = rec("one", 100);
    db.apply_scan(None, &first, std::slice::from_ref(&a), 10, false)
        .unwrap();
    let in_flight = db.pending_outbox(200).unwrap();
    assert_eq!(in_flight[0].envelope.record_revision, 1);
    a.usage.input_uncached_tokens = 200;
    db.apply_scan(
        Some(&first),
        &cp("src-a", 11),
        std::slice::from_ref(&a),
        11,
        false,
    )
    .unwrap();
    assert!(!db.ack_outbox_if_revision(&a.record_id, 1).unwrap());
    assert_eq!(db.pending_outbox(200).unwrap().len(), 1);
    assert!(db.ack_outbox_if_revision(&a.record_id, 2).unwrap());
    assert!(db.pending_outbox(200).unwrap().is_empty());
    assert!(!db.ack_outbox_if_revision(&a.record_id, 2).unwrap());
}

#[test]
fn missing_source_record_never_implicitly_deletes_archive() {
    let mut db = setup();
    let a = rec("one", 1);
    let first = cp("src-a", 10);
    db.apply_scan(None, &first, std::slice::from_ref(&a), 10, false)
        .unwrap();
    db.apply_scan(Some(&first), &cp("src-a", 11), &[], 11, false)
        .unwrap();
    assert!(
        !db.get_record(&a.record_id)
            .unwrap()
            .unwrap()
            .record
            .is_deleted
    );
}

#[test]
fn source_conflict_rejected_without_revisions() {
    let mut db = setup();
    let a = rec("one", 1);
    let first = cp("src-a", 10);
    db.apply_scan(None, &first, std::slice::from_ref(&a), 10, false)
        .unwrap();
    let source_b = cp("src-b", 11);
    db.apply_scan(None, &source_b, std::slice::from_ref(&a), 11, false)
        .unwrap();
    let mut conflict = a.clone();
    conflict.usage.input_uncached_tokens = 3;
    let err = db
        .apply_scan(Some(&source_b), &cp("src-b", 12), &[conflict], 12, false)
        .unwrap_err();
    assert!(matches!(err, StoreError::SourceArbitrationConflict));
    assert_eq!(
        db.get_record(&a.record_id)
            .unwrap()
            .unwrap()
            .record_revision,
        1
    );
}

#[test]
fn identity_boundaries_and_duplicates_are_rejected() {
    let mut db = setup();
    let a = rec("one", 1);
    let first = cp("src-a", 10);
    assert!(matches!(
        db.apply_scan(None, &first, &[a.clone(), a.clone()], 10, false),
        Err(StoreError::DuplicateBatchRecord)
    ));
    assert!(db.checkpoint("codex", "src-a").unwrap().is_none());
    let mut foreign = a.clone();
    foreign.owner_device_id = "other-device".into();
    assert!(matches!(
        db.apply_scan(None, &first, &[foreign], 10, false),
        Err(StoreError::IdentityMismatch)
    ));
    assert!(db.get_record(&a.record_id).unwrap().is_none());
}

#[test]
fn tombstones_must_be_explicit_and_remain_syncable() {
    let mut db = setup();
    let mut a = rec("one", 1);
    let first = cp("src-a", 10);
    db.apply_scan(None, &first, std::slice::from_ref(&a), 10, false)
        .unwrap();
    a.is_deleted = true;
    a.usage = TokenBreakdown::default();
    a.estimated_cost_microusd = None;
    let second = cp("src-a", 11);
    assert!(matches!(
        db.apply_scan(Some(&first), &second, std::slice::from_ref(&a), 11, false),
        Err(StoreError::InvalidInput(_))
    ));
    assert_eq!(
        db.apply_scan(Some(&first), &second, std::slice::from_ref(&a), 11, true)
            .unwrap()
            .revised,
        1
    );
    assert!(
        db.get_record(&a.record_id)
            .unwrap()
            .unwrap()
            .record
            .is_deleted
    );
}

#[test]
fn sql_failure_rolls_back_record_outbox_and_checkpoint() {
    let path = unique_db();
    let a = rec("one", 1);
    {
        let mut db = LocalStore::open(&path).unwrap();
        db.init_identity(&identity()).unwrap();
        // Inject a real SQLite trigger simulating an outbox insert failure.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TRIGGER abort_sync BEFORE INSERT ON sync_outbox BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        assert!(db
            .apply_scan(None, &cp("src-a", 10), std::slice::from_ref(&a), 10, false)
            .is_err());
        assert!(db.get_record(&a.record_id).unwrap().is_none());
        assert!(db.pending_outbox(200).unwrap().is_empty());
        assert!(db.checkpoint("codex", "src-a").unwrap().is_none());
    }
    {
        let db = LocalStore::open(&path).unwrap();
        assert!(db.get_record(&a.record_id).unwrap().is_none());
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}

#[test]
fn restart_recovers_checkpoint_revision_and_outbox() {
    let path = unique_db();
    let a = rec("one", 100);
    {
        let mut db = LocalStore::open(&path).unwrap();
        db.init_identity(&identity()).unwrap();
        db.apply_scan(None, &cp("src-a", 10), std::slice::from_ref(&a), 10, false)
            .unwrap();
    }
    {
        let mut db = LocalStore::open(&path).unwrap();
        assert_eq!(
            db.checkpoint("codex", "src-a").unwrap(),
            Some(cp("src-a", 10))
        );
        assert_eq!(
            db.get_record(&a.record_id)
                .unwrap()
                .unwrap()
                .record_revision,
            1
        );
        assert_eq!(db.pending_outbox(200).unwrap().len(), 1);
        assert!(db.ack_outbox_if_revision(&a.record_id, 1).unwrap());
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}-wal", path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", path.display()));
}

#[test]
fn large_unsigned_usage_fails_before_sql_write() {
    let mut db = setup();
    let mut a = rec("one", 1);
    a.usage.input_uncached_tokens = i64::MAX as u64 + 1;
    assert!(matches!(
        db.apply_scan(None, &cp("src-a", 1), &[a], 1, false),
        Err(StoreError::IntegerOutOfRange)
    ));
    assert!(db.pending_outbox(200).unwrap().is_empty());
}

#[test]
fn degraded_scan_cannot_publish_data() {
    let mut db = setup();
    let mut status = cp("src-a", 10);
    status.last_status = "degraded".into();
    status.last_complete_scan_ms = None;
    assert!(db
        .apply_scan(None, &status, &[rec("one", 1)], 10, false)
        .is_err());
    assert!(db.checkpoint("codex", "src-a").unwrap().is_none());
}

#[test]
fn retry_markers_are_cas_bound_to_current_revision() {
    let mut db = setup();
    let a = rec("one", 1);
    db.apply_scan(None, &cp("src-a", 10), std::slice::from_ref(&a), 10, false)
        .unwrap();
    assert!(!db.note_outbox_failure(&a.record_id, 2, "timeout").unwrap());
    assert!(db.note_outbox_failure(&a.record_id, 1, "timeout").unwrap());
    assert_eq!(db.pending_outbox(200).unwrap()[0].attempts, 1);
    assert!(db.note_outbox_failure(&a.record_id, 1, "timeout").unwrap());
    assert_eq!(db.pending_outbox(200).unwrap()[0].attempts, 2);
}

#[test]
fn consistent_online_backup_can_be_reopened() {
    let path = unique_db();
    let dest = unique_db();
    let a = rec("one", 33);
    {
        let mut db = LocalStore::open(&path).unwrap();
        db.init_identity(&identity()).unwrap();
        db.apply_scan(None, &cp("src-a", 10), std::slice::from_ref(&a), 10, false)
            .unwrap();
        db.quick_integrity_check().unwrap();
        db.backup_to(&dest).unwrap();
        assert!(db.backup_to(&dest).is_err());
    }
    {
        let backup = LocalStore::open(&dest).unwrap();
        assert_eq!(backup.identity().unwrap(), Some(identity()));
        assert_eq!(
            backup
                .get_record(&a.record_id)
                .unwrap()
                .unwrap()
                .record_revision,
            1
        );
        assert_eq!(backup.pending_outbox(200).unwrap().len(), 1);
    }
    for file in [&path, &dest] {
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(format!("{}-wal", file.display()));
        let _ = std::fs::remove_file(format!("{}-shm", file.display()));
    }
}

#[test]
fn failed_scan_cannot_advance_last_good_cursor() {
    let mut db = setup();
    let first = cp("src-a", 10);
    db.apply_scan(None, &first, &[], 10, false).unwrap();
    let mut failed = first.clone();
    failed.last_scan_ms = 11;
    failed.last_status = "degraded".into();
    failed.read_offset = 1024;
    assert!(matches!(
        db.apply_scan(Some(&first), &failed, &[], 11, false),
        Err(StoreError::InvalidInput(_))
    ));
    assert_eq!(db.checkpoint("codex", "src-a").unwrap(), Some(first));
}

#[test]
fn excessive_batch_rejected_before_write() {
    let mut db = setup();
    let a = rec("one", 1);
    let batch = vec![a; 2001];
    assert!(matches!(
        db.apply_scan(None, &cp("src-a", 10), &batch, 10, false),
        Err(StoreError::InvalidInput(_))
    ));
    assert!(db.pending_outbox(200).unwrap().is_empty());
}

#[test]
fn full_reconcile_supports_over_3000_and_preserves_outbox_identity() {
    let mut db = setup();
    let initial = cp("big-source", 900);
    let initial_rows: Vec<_> = (0..3205)
        .map(|i| rec(&format!("resp-{i}"), i + 1))
        .collect();
    let report = db
        .reconcile_complete_source(None, &initial, initial_rows.clone(), 900)
        .unwrap();
    assert_eq!(
        (report.scanned, report.applied.inserted, report.tombstoned),
        (3205, 3205, 0)
    );
    assert_eq!(
        db.checkpoint("codex", "big-source").unwrap(),
        Some(initial.clone())
    );
    let revised = cp("big-source", 901);
    let report = db
        .reconcile_complete_source(Some(&initial), &revised, initial_rows[..3100].to_vec(), 901)
        .unwrap();
    assert_eq!((report.applied.unchanged, report.tombstoned), (3100, 105));
    let missing = db
        .get_record(&initial_rows[3150].record_id)
        .unwrap()
        .unwrap();
    assert_eq!(missing.record_revision, 2);
    assert!(missing.record.is_deleted);
    assert!(!db
        .ack_outbox_if_revision(&missing.record.record_id, 1)
        .unwrap());
    assert_eq!(db.checkpoint("codex", "big-source").unwrap(), Some(revised));
}

#[test]
fn full_reconcile_duplicate_rolls_back_events_and_checkpoint() {
    let mut db = setup();
    let baseline = cp("big-source", 20);
    let original = rec("existing", 50);
    db.reconcile_complete_source(None, &baseline, vec![original.clone()], 20)
        .unwrap();
    let attempted = cp("big-source", 21);
    let mut changed = original.clone();
    changed.usage.input_uncached_tokens = 999;
    let duplicate = rec("dup", 5);
    let output = db.reconcile_complete_source(
        Some(&baseline),
        &attempted,
        vec![changed, duplicate.clone(), duplicate.clone()],
        21,
    );
    assert!(matches!(output, Err(StoreError::DuplicateBatchRecord)));
    assert!(db.get_record(&duplicate.record_id).unwrap().is_none());
    assert_eq!(
        db.get_record(&original.record_id)
            .unwrap()
            .unwrap()
            .record_revision,
        1
    );
    assert_eq!(
        db.checkpoint("codex", "big-source").unwrap(),
        Some(baseline)
    );
}

#[test]
fn no_full_reconciliation_for_partial_checkpoint() {
    let mut db = setup();
    let mut partial = cp("partial", 10);
    partial.last_status = "partial".into();
    partial.last_complete_scan_ms = None;
    assert!(db
        .reconcile_complete_source(None, &partial, vec![rec("one", 1)], 10)
        .is_err());
    assert!(db.checkpoint("codex", "partial").unwrap().is_none());
}

#[test]
fn full_reconcile_empty_snapshot_tombstones_only_if_complete() {
    let mut db = setup();
    let initial = cp("complete", 10);
    let first = rec("a", 2);
    db.reconcile_complete_source(None, &initial, vec![first.clone()], 10)
        .unwrap();
    let next = cp("complete", 11);
    let rep = db
        .reconcile_complete_source(Some(&initial), &next, Vec::<CanonicalRecord>::new(), 11)
        .unwrap();
    assert_eq!(rep.tombstoned, 1);
    assert!(
        db.get_record(&first.record_id)
            .unwrap()
            .unwrap()
            .record
            .is_deleted
    );
}

#[test]
fn remote_pull_applies_revision_and_advances_cursor_atomically() {
    use tokenscope_core_domain::RevisionedRecord;
    use tokenscope_local_store::RemoteUsageChange;
    let mut db = setup();
    let mut remote = rec("remote-event", 40);
    remote.owner_device_id = "remote-pc-001".into();
    let first = RevisionedRecord::from_store(remote.clone(), 1).unwrap();
    let n = db
        .apply_remote_changes(
            &[RemoteUsageChange {
                change_seq: 1,
                owner_device_id: "remote-pc-001".into(),
                envelope: first,
            }],
            1,
        )
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(db.pull_cursor().unwrap(), 1);
    let totals = db
        .remote_usage_totals(Some("remote-pc-001"), None, None)
        .unwrap();
    assert_eq!(totals.total_tokens, 42);
    assert_eq!(totals.record_count, 1);
    let by_device = db.remote_usage_by_device(None, None).unwrap();
    assert_eq!(by_device.len(), 1);
    assert_eq!(by_device[0].owner_device_id, "remote-pc-001");
    remote.usage.input_uncached_tokens = 80;
    let second = RevisionedRecord::from_store(remote.clone(), 2).unwrap();
    assert_eq!(
        db.apply_remote_changes(
            &[RemoteUsageChange {
                change_seq: 2,
                owner_device_id: "remote-pc-001".into(),
                envelope: second
            }],
            2
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.remote_usage_totals(None, None, None)
            .unwrap()
            .total_tokens,
        82
    );
    remote.is_deleted = true;
    remote.usage = Default::default();
    remote.estimated_cost_microusd = None;
    let tombstone = RevisionedRecord::from_store(remote, 3).unwrap();
    db.apply_remote_changes(
        &[RemoteUsageChange {
            change_seq: 3,
            owner_device_id: "remote-pc-001".into(),
            envelope: tombstone,
        }],
        3,
    )
    .unwrap();
    assert_eq!(
        db.remote_usage_totals(None, None, None)
            .unwrap()
            .total_tokens,
        0
    );
}

#[test]
fn remote_breakdowns_keep_device_tool_model_and_project_dimensions() {
    use tokenscope_core_domain::RevisionedRecord;
    use tokenscope_local_store::RemoteUsageChange;
    let mut db = setup();
    let mut a = rec("remote-a", 10);
    a.owner_device_id = "remote-pc-001".into();
    let prj = tokenscope_core_domain::project_key("codex", b"alpha").unwrap();
    a.tool_id = "codex".into();
    a.model_id = Some("gpt-6".into());
    a.project_id = Some(prj);
    let env = RevisionedRecord::from_store(a, 1).unwrap();
    db.apply_remote_changes(
        &[RemoteUsageChange {
            change_seq: 1,
            owner_device_id: "remote-pc-001".into(),
            envelope: env,
        }],
        1,
    )
    .unwrap();
    let tools = db.remote_usage_breakdown("tool_id", None, None).unwrap();
    let models = db.remote_usage_breakdown("model_id", None, None).unwrap();
    let projects = db.remote_usage_breakdown("project_id", None, None).unwrap();
    assert_eq!(
        (
            tools.len(),
            tools[0].owner_device_id.as_str(),
            tools[0].key.as_str(),
            tools[0].total_tokens
        ),
        (1, "remote-pc-001", "codex", 12)
    );
    assert_eq!(models[0].key, "gpt-6");
    assert_eq!(projects[0].owner_device_id, "remote-pc-001");
    assert!(db
        .remote_usage_breakdown("payload_json", None, None)
        .is_err());
}

#[test]
fn invalid_remote_page_rolls_back_cache_and_cursor() {
    use tokenscope_core_domain::RevisionedRecord;
    use tokenscope_local_store::RemoteUsageChange;
    let mut db = setup();
    let mut remote = rec("remote-event", 5);
    remote.owner_device_id = "remote-pc-001".into();
    let valid = RevisionedRecord::from_store(remote.clone(), 1).unwrap();
    let mut invalid = RevisionedRecord::from_store(remote, 2).unwrap();
    invalid.payload_hash = "a".repeat(64);
    let page = [
        RemoteUsageChange {
            change_seq: 1,
            owner_device_id: "remote-pc-001".into(),
            envelope: valid,
        },
        RemoteUsageChange {
            change_seq: 2,
            owner_device_id: "remote-pc-001".into(),
            envelope: invalid,
        },
    ];
    assert!(db.apply_remote_changes(&page, 2).is_err());
    assert_eq!(db.pull_cursor().unwrap(), 0);
    assert_eq!(
        db.remote_usage_totals(None, None, None)
            .unwrap()
            .record_count,
        0
    );
}
