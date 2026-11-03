use rusqlite::{params, Connection};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_core_domain::{aggregate_device_local, stable_record_id, Precision};
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_antigravity::{collect_antigravity, discover_databases, extract_model_name};

struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("tokenscope-ag-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    fn file(&self, dir: &str, stem: &str) -> PathBuf {
        let dir = self.root.join(dir).join("conversations");
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{stem}.db"))
    }
    fn write(&self, dir: &str, stem: &str, models: &[Option<&str>]) -> PathBuf {
        let path = self.file(dir, stem);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS gen_metadata(data BLOB NOT NULL); DELETE FROM gen_metadata;").unwrap();
        for model in models {
            let b = model.map(blob).unwrap_or_else(|| vec![0x08, 0x01]);
            conn.execute("INSERT INTO gen_metadata(data) VALUES(?1)", params![b])
                .unwrap();
        }
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn varint(mut n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out
}
fn field(num: usize, bytes: &[u8]) -> Vec<u8> {
    let mut out = varint(num * 8 + 2);
    out.extend(varint(bytes.len()));
    out.extend(bytes);
    out
}
fn blob(model: &str) -> Vec<u8> {
    field(1, &field(19, model.as_bytes()))
}
fn scalar(num: usize, value: usize) -> Vec<u8> {
    let mut out = varint(num * 8);
    out.extend(varint(value));
    out
}
fn usage_blob(model: &str, response_id: &str, input: usize) -> Vec<u8> {
    let mut usage = Vec::new();
    usage.extend(scalar(1, 1132)); // uncertain fixed prompt contribution
    usage.extend(scalar(2, input));
    usage.extend(scalar(3, 340)); // text output + reasoning
    usage.extend(scalar(5, 1200)); // cache read
    usage.extend(scalar(9, 300)); // text output
    usage.extend(scalar(10, 40)); // reasoning is an output subset
    usage.extend(field(11, response_id.as_bytes()));
    let mut chat_model = field(4, &usage);
    chat_model.extend(field(19, model.as_bytes()));
    let mut generation = field(1, &chat_model);
    generation.extend(scalar(2, 7)); // steps.idx link
    generation
}
fn store() -> LocalStore {
    let mut s = LocalStore::open_in_memory().unwrap();
    s.init_identity(&LocalIdentity {
        device_id: "test-device".into(),
        display_name: "Test".into(),
        time_zone: "UTC".into(),
    })
    .unwrap();
    s
}
fn active(s: &LocalStore) -> Vec<tokenscope_core_domain::CanonicalRecord> {
    s.pending_outbox(200)
        .unwrap()
        .into_iter()
        .map(|r| r.envelope.record)
        .filter(|r| !r.is_deleted)
        .collect()
}
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
            params![vec![0u8; 300_000]],
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
