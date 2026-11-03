mod common;
use common::{cp, rec, setup};
use tokenscope_core_domain::CanonicalRecord;
use tokenscope_local_store::StoreError;

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
