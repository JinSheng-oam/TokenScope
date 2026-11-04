mod common;

use common::*;
use rusqlite::{params, Connection};
use std::fs;
use tokenscope_core_domain::{aggregate_device_local, stable_record_id, Precision};
use tokenscope_source_antigravity::{collect_antigravity, collect_antigravity_tolerant};

#[test]
fn append_same_model_revises_one_record() {
    let f = Fixture::new();
    f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    f.write(
        "antigravity",
        "s1",
        &[Some("gemini-pro"), Some("gemini-pro")],
    );
    let r = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(r.revised, 1);
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn model_removed_creates_tombstone_not_lingering_estimate() {
    let f = Fixture::new();
    f.write(
        "antigravity",
        "s1",
        &[Some("gemini-pro"), Some("claude-opus")],
    );
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let r = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(r.tombstoned, 1);
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn switching_estimation_off_tombstones_estimates() {
    let f = Fixture::new();
    f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    let r = collect_antigravity(&f.root, &mut s, "test-device", false).unwrap();
    assert_eq!(r.tombstoned, 1);
    assert!(active(&s).is_empty());
}

#[test]
fn incomplete_metadata_does_not_advance_checkpoint_or_tombstone_usage() {
    let f = Fixture::new();
    let path = f.file("antigravity", "s1");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE gen_metadata(data BLOB NOT NULL);")
        .unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(data) VALUES(?1)",
        params![usage_blob("gemini-3-flash-a", "response-native", 600)],
    )
    .unwrap();
    drop(conn);

    let mut store = store();
    collect_antigravity(&f.root, &mut store, "test-device", false).unwrap();
    let source_key = stable_record_id("antigravity", b"conversation", b"s1").unwrap();
    let checkpoint_before = store
        .checkpoint("antigravity", &source_key)
        .unwrap()
        .unwrap();
    assert_eq!(active(&store).len(), 1);

    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(data) VALUES(?1)",
        params![vec![0_u8; 8 * 1024 * 1024 + 1]],
    )
    .unwrap();
    drop(conn);

    let report = collect_antigravity_tolerant(&f.root, &mut store, "test-device", false).unwrap();
    assert_eq!(report.skipped_sources, 1);
    assert_eq!(report.skipped_rows, 1);
    assert_eq!(report.tombstoned, 0);
    assert_eq!(active(&store).len(), 1);
    assert_eq!(
        store
            .checkpoint("antigravity", &source_key)
            .unwrap()
            .unwrap(),
        checkpoint_before
    );
}

#[test]
fn unknown_model_prevents_incomplete_estimate_and_preserves_history() {
    let f = Fixture::new();
    f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    f.write("antigravity", "s1", &[Some("gemini-pro"), None]);
    let r = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(r.unknown_model_turns, 1);
    assert_eq!(r.revised, 0);
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn tokenless_snapshot_cannot_tombstone_previous_native_usage_or_advance_checkpoint() {
    let f = Fixture::new();
    let path = f.file("antigravity", "s1");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE gen_metadata(idx INTEGER,data BLOB,size INTEGER);")
        .unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(1,?1,0)",
        params![usage_blob("gemini-3-flash-a", "response-native", 600)],
    )
    .unwrap();
    drop(conn);

    let mut store = store();
    collect_antigravity(&f.root, &mut store, "test-device", false).unwrap();
    let source_key = stable_record_id("antigravity", b"conversation", b"s1").unwrap();
    let checkpoint_before = store
        .checkpoint("antigravity", &source_key)
        .unwrap()
        .unwrap();
    assert_eq!(active(&store).len(), 1);

    let conn = Connection::open(&path).unwrap();
    conn.execute("DELETE FROM gen_metadata", []).unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(2,?1,0)",
        params![blob("gemini-3-flash-a")],
    )
    .unwrap();
    drop(conn);

    let report = collect_antigravity(&f.root, &mut store, "test-device", false).unwrap();
    assert_eq!(report.tombstoned, 0);
    assert_eq!(active(&store).len(), 1);
    assert_eq!(
        store
            .checkpoint("antigravity", &source_key)
            .unwrap()
            .unwrap(),
        checkpoint_before
    );
}

#[test]
fn malformed_schema_never_advances_checkpoint_or_deletes_old_data() {
    let f = Fixture::new();
    let path = f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    let conn = Connection::open(path).unwrap();
    conn.execute_batch("DROP TABLE gen_metadata").unwrap();
    assert!(collect_antigravity(&f.root, &mut s, "test-device", true).is_err());
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn aggregate_total_retains_estimated_precision_in_records() {
    let f = Fixture::new();
    f.write("antigravity", "a", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    let rows = active(&s);
    assert_eq!(
        aggregate_device_local(rows.iter())
            .unwrap()
            .usage
            .total()
            .unwrap(),
        8800
    );
    assert!(rows.iter().all(|r| r.precision == Precision::Estimated));
}

#[test]
fn raw_metadata_does_not_leak_to_storage() {
    let f = Fixture::new();
    let path = f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(data) VALUES(?1)",
        params![vec![0x0a_u8, 0x00_u8]],
    )
    .unwrap();
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", false).unwrap();
    assert!(active(&s).is_empty());
}

#[test]
fn too_large_blob_does_not_replace_last_good_record() {
    let f = Fixture::new();
    let path = f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO gen_metadata(data) VALUES(?1)",
            params![vec![0u8; 8 * 1024 * 1024 + 1]],
        )
        .unwrap();
    let report = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(report.skipped_rows, 1);
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn missing_source_does_not_delete_last_known_usage() {
    let f = Fixture::new();
    let path = f.write("antigravity", "s1", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    fs::remove_file(path).unwrap();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn default_activity_is_persisted_without_token_rows() {
    let f = Fixture::new();
    f.write(
        "antigravity",
        "persist",
        &[Some("gemini-pro"), Some("claude-sonnet"), None],
    );
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", false).unwrap();
    let summary = s.antigravity_activity_summary().unwrap();
    assert_eq!(
        (
            summary.conversations,
            summary.known_turns,
            summary.unknown_turns
        ),
        (1, 2, 1)
    );
    assert_eq!(summary.by_model.len(), 2);
    assert!(active(&s).is_empty());
}

#[test]
fn default_activity_scan_is_idempotent() {
    let f = Fixture::new();
    f.write("antigravity", "persist", &[Some("gemini-pro")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", false).unwrap();
    collect_antigravity(&f.root, &mut s, "test-device", false).unwrap();
    assert_eq!(s.antigravity_activity_summary().unwrap().known_turns, 1);
}
