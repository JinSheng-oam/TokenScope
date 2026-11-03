use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_core_domain::{aggregate_device_local, CanonicalRecord};
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_codex::{
    collect_codex, collect_codex_precise, collect_codex_precise_tolerant,
};
const SESSION: &str = "019a9b8c-b777-4d55-9222-121212121212";

struct Fx {
    root: PathBuf,
    file: PathBuf,
}
impl Fx {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("tokenscope-m005-{}-{stamp}", std::process::id()));
        let folder = root.join("sessions/2026/11/02");
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("rollout-sample.jsonl");
        fs::write(&file,format!("{{\"timestamp\":\"2026-11-01T23:59:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{SESSION}\",\"originator\":\"Codex CLI\"}}}}\n")).unwrap();
        Self { root, file }
    }
    fn append(&self, s: &str) {
        OpenOptions::new()
            .append(true)
            .open(&self.file)
            .unwrap()
            .write_all(s.as_bytes())
            .unwrap()
    }
    fn replace_body(&self, body: &str) {
        let header = fs::read_to_string(&self.file)
            .unwrap()
            .lines()
            .next()
            .unwrap()
            .to_owned();
        fs::write(&self.file, format!("{header}\n{body}")).unwrap();
    }
    fn model(&self, model: &str) {
        self.append(&format!("{{\"timestamp\":\"2026-11-01T23:59:01Z\",\"type\":\"turn_context\",\"payload\":{{\"model\":\"{model}\"}}}}\n"));
    }
    fn response(&self, id: &str, time: &str, input: u64, cache: u64, write: u64, output: u64) {
        self.append(&response(id, time, input, cache, write, output));
    }
    fn total(&self, input: u64, cache: u64, output: u64) {
        self.append(&total(input, cache, output));
    }
}
impl Drop for Fx {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
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
fn response(id: &str, time: &str, i: u64, c: u64, w: u64, o: u64) -> String {
    format!("{{\"timestamp\":\"{time}\",\"type\":\"token_usage_record\",\"payload\":{{\"response_id\":\"{id}\",\"usage\":{{\"input_tokens\":{i},\"cached_input_tokens\":{c},\"cache_write_input_tokens\":{w},\"output_tokens\":{o},\"reasoning_output_tokens\":2,\"total_tokens\":{}}}}}}}\n",i+o)
}
fn total(i: u64, c: u64, o: u64) -> String {
    format!("{{\"timestamp\":\"2026-11-02T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":{i},\"cached_input_tokens\":{c},\"output_tokens\":{o},\"reasoning_output_tokens\":2,\"total_tokens\":{}}}}},\"rate_limits\":{{}}}}}}\n",i+o)
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
fn modern_usage_does_not_double_count_matching_total_snapshot() {
    let f = Fx::new();
    let mut db = store();
    f.model("gpt-6-sol");
    f.response("resp-1", "2026-11-01T23:59:59Z", 100, 30, 10, 20);
    f.total(100, 30, 20);
    let result = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!((result.inserted, result.lines), (1, 4));
    let rows = active(&db);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.model_id.as_deref(), Some("gpt-6-sol"));
    assert_eq!(
        (
            r.usage.input_uncached_tokens,
            r.usage.cache_read_tokens,
            r.usage.cache_write_tokens,
            r.usage.output_tokens
        ),
        (60, 30, 10, 20)
    );
    assert_eq!(r.usage.total().unwrap(), 120);
}
#[test]
fn modern_models_dates_are_independent() {
    let f = Fx::new();
    let mut db = store();
    f.model("model-a");
    f.response("r1", "2026-11-01T23:59:58Z", 100, 0, 0, 10);
    f.model("model-b");
    f.response("r2", "2026-11-02T00:00:05Z", 200, 20, 0, 30);
    collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    let rows = active(&db);
    assert_eq!(rows.len(), 2);
    let first = rows
        .iter()
        .find(|r| r.model_id.as_deref() == Some("model-a"))
        .unwrap();
    let second = rows
        .iter()
        .find(|r| r.model_id.as_deref() == Some("model-b"))
        .unwrap();
    assert!(first.event_at_ms.unwrap() < second.event_at_ms.unwrap());
    assert_eq!(first.usage.total().unwrap(), 110);
    assert_eq!(second.usage.total().unwrap(), 230);
    assert_eq!(
        aggregate_device_local(rows.iter())
            .unwrap()
            .usage
            .total()
            .unwrap(),
        340
    );
}
#[test]
fn legacy_uses_cumulative_deltas_not_last_usage() {
    let f = Fx::new();
    let mut db = store();
    f.model("model-a");
    f.total(100, 30, 20);
    f.total(100, 30, 20);
    f.total(150, 50, 27);
    let rep = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(rep.inserted, 2);
    let rows = active(&db);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        aggregate_device_local(rows.iter())
            .unwrap()
            .usage
            .total()
            .unwrap(),
        177
    );
}
#[test]
fn modern_append_is_incremental_and_idempotent() {
    let f = Fx::new();
    let mut db = store();
    f.response("r1", "2026-11-02T01:00:00Z", 100, 20, 0, 10);
    collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    let again = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!((again.lines, again.inserted, again.revised), (0, 0, 0));
    f.response("r2", "2026-11-02T01:10:00Z", 60, 10, 0, 20);
    let updated = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(
        (updated.lines, updated.inserted, updated.reconciled),
        (1, 1, 0)
    );
    assert_eq!(active(&db).len(), 2);
}
#[test]
fn parser_upgrade_tombstones_old_session_aggregate() {
    let f = Fx::new();
    let mut db = store();
    f.total(100, 20, 10);
    f.total(180, 30, 30);
    collect_codex(&f.root, &mut db, "dev1").unwrap();
    let before = active(&db).into_iter().next().unwrap();
    let result = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(result.inserted, 2);
    assert_eq!(result.revised, 1);
    assert!(
        db.get_record(&before.record_id)
            .unwrap()
            .unwrap()
            .record
            .is_deleted
    );
    let rows = active(&db);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        aggregate_device_local(rows.iter())
            .unwrap()
            .usage
            .total()
            .unwrap(),
        210
    );
}
#[test]
fn rewritten_modern_file_tombstones_removed_responses() {
    let f = Fx::new();
    let mut db = store();
    f.response("r1", "2026-11-02T01:00:00Z", 10, 0, 0, 3);
    f.response("r2", "2026-11-02T01:01:00Z", 20, 0, 0, 4);
    collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    let before = active(&db);
    let old_id = before
        .iter()
        .find(|r| r.usage.total().unwrap() == 24)
        .unwrap()
        .record_id
        .clone();
    f.replace_body(&response("r1", "2026-11-02T01:00:00Z", 10, 0, 0, 3));
    let res = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(res.revised, 1);
    assert!(db.get_record(&old_id).unwrap().unwrap().record.is_deleted);
}
#[test]
fn unfinished_response_line_is_deferred() {
    let f = Fx::new();
    let mut db = store();
    let line = response("r1", "2026-11-02T01:00:00Z", 12, 2, 0, 3);
    let cut = line.len() / 2;
    f.append(&line[..cut]);
    let a = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(a.pending, 1);
    assert!(active(&db).is_empty());
    f.append(&line[cut..]);
    let b = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(b.inserted, 1);
}
#[test]
fn mixed_legacy_prefix_falls_back_to_cumulative_projection() {
    let f = Fx::new();
    let mut db = store();
    f.total(100, 0, 10);
    f.response("r2", "2026-11-02T01:00:00Z", 20, 0, 0, 3);
    let res = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(res.inserted, 2);
    assert_eq!(
        aggregate_device_local(active(&db).iter())
            .unwrap()
            .usage
            .total()
            .unwrap(),
        133
    );
}
#[test]
fn inconsistent_cache_write_fails_closed() {
    let f = Fx::new();
    let mut db = store();
    f.response("r1", "2026-11-02T01:00:00Z", 10, 9, 4, 3);
    assert!(collect_codex_precise(&f.root, &mut db, "dev1").is_err());
    assert!(db.pending_outbox(200).unwrap().is_empty());
}

#[test]
fn tolerant_collection_skips_broken_source_and_keeps_others() {
    let f = Fx::new();
    let mut db = store();
    f.response("r1", "2026-11-02T01:00:00Z", 10, 0, 0, 3);
    let broken = f.root.join("sessions/2026/11/02/rollout-broken.jsonl");
    fs::write(
        &broken,
        format!(
            "{{\"timestamp\":\"2026-11-01T23:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"019a9b8c-b777-4d55-9222-333333333333\",\"originator\":\"Codex CLI\"}}}}\n{}",
            response("m1", "2026-11-02T02:00:00Z", 10, 9, 4, 3)
        ),
    )
    .unwrap();

    let res = collect_codex_precise_tolerant(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(res.unsupported, 1);
    assert_eq!(res.inserted, 1);
    assert_eq!(active(&db).len(), 1);
}
#[test]
fn duplicate_response_identity_fails_closed() {
    let f = Fx::new();
    let mut db = store();
    f.response("same", "2026-11-02T01:00:00Z", 10, 0, 0, 3);
    f.response("same", "2026-11-02T01:10:00Z", 10, 0, 0, 3);
    assert!(collect_codex_precise(&f.root, &mut db, "dev1").is_err());
}
#[test]
fn counter_regression_does_not_publish() {
    let f = Fx::new();
    let mut db = store();
    f.total(100, 20, 10);
    f.total(90, 10, 7);
    assert!(collect_codex_precise(&f.root, &mut db, "dev1").is_err());
    assert!(db.pending_outbox(200).unwrap().is_empty());
}

#[test]
fn partial_rewrite_keeps_last_known_good_history() {
    let f = Fx::new();
    let mut db = store();
    f.response("r1", "2026-11-02T01:00:00Z", 100, 0, 0, 3);
    collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    let old = active(&db).into_iter().next().unwrap();
    let next = response("r2", "2026-11-02T01:10:00Z", 100, 0, 0, 3);
    let split = next.len() / 2;
    f.replace_body(&next[..split]);
    let result = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!(result.pending, 1);
    assert!(
        !db.get_record(&old.record_id)
            .unwrap()
            .unwrap()
            .record
            .is_deleted
    );
    f.append(&next[split..]);
    let complete = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!((complete.inserted, complete.revised), (1, 1));
    assert!(
        db.get_record(&old.record_id)
            .unwrap()
            .unwrap()
            .record
            .is_deleted
    );
}

#[test]
fn unprovable_legacy_to_modern_append_retains_old_projection() {
    let f = Fx::new();
    let mut db = store();
    f.total(100, 10, 20);
    collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    let old = active(&db).into_iter().next().unwrap();
    f.response("r2", "2026-11-02T01:10:00Z", 50, 0, 0, 5);
    collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert!(
        !db.get_record(&old.record_id)
            .unwrap()
            .unwrap()
            .record
            .is_deleted
    );
    assert_eq!(db.pending_outbox(200).unwrap().len(), 2);
}

#[test]
fn modern_full_log_with_3100_responses_is_not_truncated() {
    let f = Fx::new();
    let mut db = store();
    let mut data = String::new();
    for i in 0..3100 {
        data.push_str(&response(
            &format!("resp-{i}"),
            "2026-11-02T01:00:00Z",
            10,
            0,
            0,
            3,
        ));
    }
    f.append(&data);
    let result = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!((result.inserted, result.reconciled), (3100, 1));
    let next = collect_codex_precise(&f.root, &mut db, "dev1").unwrap();
    assert_eq!((next.inserted, next.revised), (0, 0));
}
