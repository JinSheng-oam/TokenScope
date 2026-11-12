use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tokenscope_core_domain::{aggregate_device_local, session_key, CanonicalRecord};
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_claude::{collect_claude, discover_claude_files};

struct Fixture {
    root: PathBuf,
    file: PathBuf,
    _temp_dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let temp_dir = tempfile::Builder::new()
            .prefix("tokenscope-m006-")
            .tempdir()
            .unwrap();
        let root = temp_dir.path().to_path_buf();
        let dir = root.join("projects/sample-project");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("session.jsonl");
        fs::write(&file, "").unwrap();
        Self {
            root,
            file,
            _temp_dir: temp_dir,
        }
    }
    fn append(&self, text: &str) {
        OpenOptions::new()
            .append(true)
            .open(&self.file)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }
    fn replace(&self, text: &str) {
        fs::write(&self.file, text).unwrap();
    }
    fn row(&self, id: &str, time: &str, input: u64, read: u64, write: u64, output: u64) {
        self.append(&row(id, time, input, read, write, output));
    }
    fn key(&self) -> String {
        let relative = self
            .file
            .strip_prefix(self.root.join("projects"))
            .unwrap()
            .to_str()
            .unwrap();
        session_key("claude", relative.as_bytes()).unwrap()
    }
}
fn store() -> LocalStore {
    let mut db = LocalStore::open_in_memory().unwrap();
    db.init_identity(&LocalIdentity {
        device_id: "dev1".into(),
        display_name: "Test".into(),
        time_zone: "UTC".into(),
    })
    .unwrap();
    db
}
fn row(id: &str, time: &str, input: u64, read: u64, write: u64, output: u64) -> String {
    json!({"type":"assistant","requestId":format!("req-{id}"),"timestamp":time,
        "message":{"id":id,"model":"claude-model-a", "usage": {
            "input_tokens":input,"cache_read_input_tokens":read,
            "cache_creation_input_tokens":write,"output_tokens":output,
            "cache_creation":{"ephemeral_5m_input_tokens":write,"ephemeral_1h_input_tokens":0}
        },"content":[{"type":"text","text":"PRIVATE_SAMPLE_DO_NOT_PERSIST"}]}}
    )
    .to_string()
        + "\n"
}
fn active(db: &LocalStore) -> Vec<CanonicalRecord> {
    db.pending_outbox(200)
        .unwrap()
        .into_iter()
        .filter(|e| !e.envelope.record.is_deleted)
        .map(|e| e.envelope.record)
        .collect()
}
#[test]
fn empty_root_returns_no_files() {
    let f = Fixture::new();
    assert_eq!(
        discover_claude_files(&f.root.join("missing"))
            .unwrap()
            .len(),
        0
    );
}
#[test]
fn parses_cache_into_disjoint_buckets() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-01T23:59:00Z", 2, 100, 20, 7);
    let r = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(r.inserted, 1);
    let rows = active(&db);
    let v = &rows[0].usage;
    assert_eq!(
        (
            v.input_uncached_tokens,
            v.cache_read_tokens,
            v.cache_write_tokens,
            v.output_tokens
        ),
        (2, 100, 20, 7)
    );
    assert_eq!(v.total().unwrap(), 129);
    assert!(rows[0].project_id.as_ref().unwrap().starts_with("prj_"));
}
#[test]
fn repeated_streaming_lines_are_one_response() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-01T23:59:00Z", 2, 100, 20, 1);
    f.row("msg-1", "2026-11-01T23:59:02Z", 2, 100, 20, 17);
    f.row("msg-1", "2026-11-01T23:59:03Z", 2, 100, 20, 17);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(active(&db).len(), 1);
    assert_eq!(active(&db)[0].usage.output_tokens, 17);
}
#[test]
fn zero_output_placeholder_does_not_override_final() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-01T23:59:00Z", 2, 100, 20, 40);
    f.row("msg-1", "2026-11-01T23:59:10Z", 0, 0, 0, 0);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(active(&db)[0].usage.output_tokens, 40);
}
#[test]
fn duplicated_responses_keep_distinct_utc_times() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-01T23:59:59Z", 2, 0, 0, 4);
    f.row("msg-2", "2026-11-02T00:00:01Z", 3, 0, 0, 5);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    let mut times: Vec<_> = active(&db).iter().filter_map(|r| r.event_at_ms).collect();
    times.sort();
    assert_eq!(times.len(), 2);
    assert!(times[0] < times[1]);
}
#[test]
fn unchanged_files_do_not_enqueue_again() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    let next = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(
        (next.unchanged_files, next.inserted, next.revised),
        (1, 0, 0)
    );
}
#[test]
fn growing_response_revises_same_record_not_add_another() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    f.row("msg-1", "2026-11-02T00:00:01Z", 2, 0, 0, 12);
    let r = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(r.revised, 1);
    assert_eq!(active(&db).len(), 1);
    assert_eq!(active(&db)[0].usage.output_tokens, 12);
}
#[test]
fn rewritten_source_tombstones_missing_response() {
    let f = Fixture::new();
    let mut db = store();
    let first = row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    f.append(&first);
    f.row("msg-2", "2026-11-02T00:00:01Z", 3, 0, 0, 5);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    let old_ids: Vec<String> = active(&db).iter().map(|r| r.record_id.clone()).collect();
    f.replace(&first);
    let result = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(result.deleted, 1);
    let all = db.records_for_source("claude", &f.key(), 2000).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().filter(|e| e.record.is_deleted).count(), 1);
    assert!(old_ids
        .iter()
        .all(|id| db.get_record(id).unwrap().is_some()));
}
#[test]
fn incomplete_tail_preserves_checkpoint_and_data() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    let key = f.key();
    let old = db.checkpoint("claude", &key).unwrap().unwrap();
    f.append("{\"type\":\"assistant\",\"message\":");
    let pending = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(pending.pending_files, 1);
    assert_eq!(db.checkpoint("claude", &key).unwrap().unwrap(), old);
    assert_eq!(active(&db).len(), 1);
}
#[test]
fn malformed_complete_json_preserves_checkpoint() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    let old = db.checkpoint("claude", &f.key()).unwrap().unwrap();
    f.append("{malformed}\n");
    assert!(collect_claude(&f.root, &mut db, "dev1").is_err());
    assert_eq!(db.checkpoint("claude", &f.key()).unwrap().unwrap(), old);
}
#[test]
fn cache_mismatch_requires_an_authoritative_sibling() {
    let f = Fixture::new();
    let mut db = store();
    let mut bad: serde_json::Value =
        serde_json::from_str(&row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 10, 4)).unwrap();
    bad["message"]["usage"]["cache_creation_input_tokens"] = json!(0);
    f.append(&(bad.to_string() + "\n"));
    assert!(collect_claude(&f.root, &mut db, "dev1").is_err());
    f.row("msg-1", "2026-11-02T00:00:01Z", 2, 0, 10, 4);
    assert_eq!(
        collect_claude(&f.root, &mut db, "dev1").unwrap().inserted,
        1
    );
}
#[test]
fn usage_on_different_projects_is_separated() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-same", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    let other = f.root.join("projects/another-project/session.jsonl");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, row("msg-same", "2026-11-02T00:00:00Z", 2, 0, 0, 4)).unwrap();
    let r = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(r.inserted, 2);
    let rows = active(&db);
    assert_ne!(rows[0].project_id, rows[1].project_id);
    assert_ne!(rows[0].record_id, rows[1].record_id);
}
#[test]
fn subagent_files_are_tracked_separately() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    let sub = f
        .root
        .join("projects/sample-project/session/subagents/agent-a.jsonl");
    fs::create_dir_all(sub.parent().unwrap()).unwrap();
    fs::write(&sub, row("msg-2", "2026-11-02T00:00:02Z", 2, 0, 0, 4)).unwrap();
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(active(&db).len(), 2);
}
#[test]
fn aggregate_count_does_not_double_add_cache() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 10, 20, 30, 40);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(
        aggregate_device_local(active(&db).iter())
            .unwrap()
            .usage
            .total()
            .unwrap(),
        100
    );
}
#[test]
fn no_prompt_text_is_retained_in_sqlite() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 4);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    let snapshot = f.root.join("snapshot.db");
    db.backup_to(&snapshot).unwrap();
    let bytes = fs::read(snapshot).unwrap();
    assert!(!bytes
        .windows(b"PRIVATE_SAMPLE_DO_NOT_PERSIST".len())
        .any(|w| w == b"PRIVATE_SAMPLE_DO_NOT_PERSIST"));
}

#[test]
fn missing_request_id_does_not_duplicate_message_id() {
    let f = Fixture::new();
    let mut db = store();
    let first = row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 3);
    let mut later: serde_json::Value =
        serde_json::from_str(&row("msg-1", "2026-11-02T00:00:01Z", 2, 0, 0, 10)).unwrap();
    later.as_object_mut().unwrap().remove("requestId");
    f.append(&first);
    f.append(&(later.to_string() + "\n"));
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(active(&db).len(), 1);
    assert_eq!(active(&db)[0].usage.output_tokens, 10);
}
#[test]
fn differing_streaming_snapshots_are_conservatively_estimated() {
    let f = Fixture::new();
    let mut db = store();
    f.row("msg-1", "2026-11-02T00:00:00Z", 2, 0, 0, 1);
    f.row("msg-1", "2026-11-02T00:00:01Z", 2, 0, 0, 30);
    collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(
        active(&db)[0].precision,
        tokenscope_core_domain::Precision::Estimated
    );
}
#[test]
fn invalid_negative_counter_cannot_commit() {
    let f = Fixture::new();
    let mut db = store();
    f.append(r#"{"type":"assistant","timestamp":"2026-11-02T00:00:00Z","message":{"id":"msg","usage":{"input_tokens":-1,"output_tokens":2}}}"#);
    f.append("\n");
    assert!(collect_claude(&f.root, &mut db, "dev1").is_err());
    assert!(db.checkpoint("claude", &f.key()).unwrap().is_none());
}

#[test]
fn large_3100_distinct_responses_reconcile_and_keep_history() {
    let f = Fixture::new();
    let mut db = store();
    let mut data = String::new();
    for i in 0..3100 {
        data.push_str(&row(
            &format!("msg-{i}"),
            "2026-11-02T00:00:00Z",
            2,
            0,
            0,
            3,
        ));
    }
    f.replace(&data);
    let first = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(first.inserted, 3100);
    let second = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(second.unchanged_files, 1);
    let only_first = row("msg-0", "2026-11-02T00:00:00Z", 2, 0, 0, 3);
    f.replace(&only_first);
    let revised = collect_claude(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(revised.deleted, 3099);
    let pending = db.pending_outbox(200).unwrap();
    assert_eq!(pending.len(), 200);
    assert!(pending.iter().any(|entry| entry.envelope.record.is_deleted));
}
