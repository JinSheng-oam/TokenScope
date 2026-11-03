mod common;

use common::*;
use rusqlite::{params, Connection};
use tokenscope_core_domain::Precision;
use tokenscope_source_antigravity::{collect_antigravity, discover_databases, extract_model_name};

#[test]
fn protobuf_extracts_model_only() {
    assert_eq!(
        extract_model_name(&blob("gemini-3-flash-a"))
            .unwrap()
            .unwrap(),
        "gemini-3-flash-a"
    );
    assert_eq!(extract_model_name(&[0x08, 0x01]).unwrap(), None);
}

#[test]
fn truncated_protobuf_errors() {
    assert!(extract_model_name(&[0x0a, 0xff]).is_err());
    assert!(extract_model_name(&[0x0a, 0x02, 0x9a]).is_err());
}

#[test]
fn protobuf_rejects_bad_model_name() {
    assert!(extract_model_name(&blob("secret private prompt!")).is_err());
}

#[test]
fn native_generation_usage_is_attributed_and_uses_real_step_time() {
    let f = Fixture::new();
    let path = f.file("antigravity", "native");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE gen_metadata(idx INTEGER,data BLOB,size INTEGER); CREATE TABLE steps(idx INTEGER,step_type INTEGER,metadata BLOB);").unwrap();
    let mut timestamp = scalar(1, 1_800_000_000);
    timestamp.extend(scalar(2, 250_000_000));
    conn.execute(
        "INSERT INTO steps(idx,step_type,metadata) VALUES(7,15,?1)",
        params![field(1, &timestamp)],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(7,?1,0)",
        params![usage_blob("gemini-3-flash-a", "response-native", 600)],
    )
    .unwrap();
    drop(conn);

    let mut store = store();
    let report = collect_antigravity(&f.root, &mut store, "test-device", false).unwrap();
    assert_eq!(
        (
            report.measured_records,
            report.estimated_records,
            report.inserted
        ),
        (1, 0, 1)
    );
    let records = active(&store);
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.model_id.as_deref(), Some("gemini-3-flash-a"));
    assert_eq!(record.precision, Precision::Aggregate);
    assert_eq!(record.usage.input_uncached_tokens, 600);
    assert_eq!(record.usage.cache_read_tokens, 1200);
    assert_eq!(record.usage.output_tokens, 340);
    assert_eq!(record.usage.reasoning_tokens, Some(40));
    assert_eq!(record.usage.unclassified_tokens, 1132);
    assert_eq!(record.usage.total().unwrap(), 3272);
    assert_eq!(record.event_at_ms, Some(1_800_000_000_250));
}

#[test]
fn duplicate_response_ids_are_deduplicated_and_conflicts_fail_closed() {
    let f = Fixture::new();
    let path = f.file("antigravity", "duplicate");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE gen_metadata(idx INTEGER,data BLOB,size INTEGER);")
        .unwrap();
    let data = usage_blob("gemini-3-flash-a", "response-same", 600);
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(1,?1,0)",
        params![&data],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(2,?1,0)",
        params![&data],
    )
    .unwrap();
    drop(conn);
    let mut store = store();
    let report = collect_antigravity(&f.root, &mut store, "test-device", false).unwrap();
    assert_eq!(report.model_turns, 1);
    assert_eq!(active(&store).len(), 1);

    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(3,?1,0)",
        params![usage_blob("gemini-3-flash-a", "response-same", 999)],
    )
    .unwrap();
    drop(conn);
    assert!(collect_antigravity(&f.root, &mut store, "test-device", false).is_err());
    assert_eq!(active(&store).len(), 1);
}

#[test]
fn empty_root_does_not_modify_db() {
    let f = Fixture::new();
    let mut s = store();
    let report = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(report.source_files, 0);
    assert!(active(&s).is_empty());
}

#[test]
fn discovered_db_does_not_follow_symlink() {
    let f = Fixture::new();
    f.write("antigravity", "a", &[Some("gemini-pro")]);
    assert_eq!(discover_databases(&f.root).unwrap().len(), 1);
}

#[test]
fn activity_only_is_default_and_never_persists_fake_tokens() {
    let f = Fixture::new();
    f.write(
        "antigravity",
        "s1",
        &[Some("gemini-pro"), Some("gemini-pro")],
    );
    let mut s = store();
    let r = collect_antigravity(&f.root, &mut s, "test-device", false).unwrap();
    assert_eq!(r.model_turns, 2);
    assert_eq!(r.estimated_records, 0);
    assert!(active(&s).is_empty());
}

#[test]
fn opt_in_creates_only_estimated_rows_without_fake_dates_or_cost() {
    let f = Fixture::new();
    f.write(
        "antigravity",
        "s1",
        &[Some("gemini-3-flash-a"), Some("gemini-3-flash-a")],
    );
    let mut s = store();
    let r = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(r.inserted, 1);
    let records = active(&s);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].precision, Precision::Estimated);
    assert_eq!(records[0].usage.unclassified_tokens, 8000);
    assert_eq!(records[0].usage.output_tokens, 1000);
    assert_eq!(records[0].event_at_ms, None);
    assert_eq!(records[0].estimated_cost_microusd, None);
}

#[test]
fn repeated_scans_are_idempotent() {
    let f = Fixture::new();
    f.write("antigravity", "s1", &[Some("claude-sonnet-4")]);
    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    let again = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!((again.inserted, again.revised, again.unchanged), (0, 0, 1));
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn same_session_in_ide_and_cli_is_counted_once() {
    let f = Fixture::new();
    f.write("antigravity", "same", &[Some("gemini-pro")]);
    f.write("antigravity-cli", "same", &[Some("gemini-pro")]);
    let mut s = store();
    let report = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(
        (
            report.source_files,
            report.conversations,
            report.model_turns
        ),
        (2, 1, 1)
    );
    assert_eq!(active(&s).len(), 1);
}

#[test]
fn larger_snapshot_preferred_over_older_cli_copy() {
    let f = Fixture::new();
    f.write(
        "antigravity",
        "same",
        &[Some("gemini-pro"), Some("gemini-pro")],
    );
    f.write("antigravity-cli", "same", &[Some("gemini-pro")]);
    let mut s = store();
    let report = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(report.model_turns, 2);
    assert_eq!(active(&s)[0].usage.unclassified_tokens, 16000);
}

#[test]
fn divergent_duplicate_sources_are_not_summed() {
    let f = Fixture::new();
    f.write("antigravity", "same", &[Some("gemini-pro")]);
    f.write("antigravity-cli", "same", &[Some("claude-opus")]);
    let mut s = store();
    let r = collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    assert_eq!(r.ambiguous_conversations, 1);
    assert!(active(&s).is_empty());
}
