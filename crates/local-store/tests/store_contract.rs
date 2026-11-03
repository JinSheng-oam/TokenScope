mod common;
use common::{cp, identity, rec, setup, unique_db};
use tokenscope_core_domain::TokenBreakdown;
use tokenscope_local_store::{LocalStore, StoreError};

#[test]
fn migration_and_one_time_identity() {
    let mut db = setup();
    assert_eq!(db.schema_version().unwrap(), 7);
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
