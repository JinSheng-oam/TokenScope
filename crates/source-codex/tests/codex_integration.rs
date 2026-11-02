use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_codex::{collect_codex, discover_codex_files};

const SESSION: &str = "019a9b8c-b777-4d55-9222-121212121212";

struct Fixture {
    home: PathBuf,
    file: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let home =
            std::env::temp_dir().join(format!("tokenscope-codex-{}-{stamp}", std::process::id()));
        let dir = home.join("sessions/2026/11/02");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("rollout-test.jsonl");
        fs::write(&file,format!("{{\"timestamp\":\"2026-11-02T00:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{SESSION}\",\"originator\":\"Codex CLI\",\"cwd\":\"/secret/work\"}}}}\n")).unwrap();
        Self { home, file }
    }
    fn append(&self, text: &str) {
        OpenOptions::new()
            .append(true)
            .open(&self.file)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap()
    }
    fn add(&self, input: u64, cached: u64, output: u64) {
        self.append(&packet(input, cached, output))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
fn packet(input: u64, cached: u64, output: u64) -> String {
    format!("{{\"timestamp\":\"2026-11-02T01:02:03Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"token_count\",\"info\":{{\"total_token_usage\":{{\"input_tokens\":{input},\"cached_input_tokens\":{cached},\"output_tokens\":{output},\"reasoning_output_tokens\":1,\"total_tokens\":{}}},\"last_token_usage\":{{\"input_tokens\":999999}}}}}}}}\n",input+output)
}
fn db() -> LocalStore {
    let mut s = LocalStore::open_in_memory().unwrap();
    s.init_identity(&LocalIdentity {
        device_id: "dev-01".into(),
        display_name: "Test".into(),
        time_zone: "UTC".into(),
    })
    .unwrap();
    s
}
fn record(db: &LocalStore) -> tokenscope_core_domain::RevisionedRecord {
    db.pending_outbox(200)
        .unwrap()
        .first()
        .unwrap()
        .envelope
        .clone()
}

#[test]
fn cumulative_snapshot_replaces_instead_of_summing() {
    let f = Fixture::new();
    let mut s = db();
    f.add(100, 30, 20);
    f.add(150, 40, 30);
    let first = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!(
        (first.scanned_files, first.inserted, first.revised),
        (1, 1, 0)
    );
    let r = record(&s);
    assert_eq!(
        (
            r.record.usage.input_uncached_tokens,
            r.record.usage.cache_read_tokens,
            r.record.usage.output_tokens
        ),
        (110, 40, 30)
    );
    assert_eq!(r.record.usage.total().unwrap(), 180);
    assert_eq!(r.record.model_id, None); // cumulative multi-model cannot be attributed safely
    let second = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!(
        (second.inserted, second.revised, second.unchanged),
        (0, 0, 1)
    );
    assert_eq!(record(&s).record_revision, 1);
}
#[test]
fn append_incremental_cursor_revises_single_record() {
    let f = Fixture::new();
    let mut s = db();
    f.add(100, 20, 20);
    collect_codex(&f.home, &mut s, "dev-01").unwrap();
    let cp = s
        .checkpoint("codex", &record(&s).record.session_id.clone().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(cp.last_status, "complete");
    f.add(180, 50, 45);
    let rep = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!((rep.inserted, rep.revised, rep.parsed_lines), (0, 1, 1));
    assert_eq!(record(&s).record_revision, 2);
    assert_eq!(record(&s).record.usage.total().unwrap(), 225);
    assert!(
        s.checkpoint("codex", &cp.source_key)
            .unwrap()
            .unwrap()
            .read_offset
            > cp.read_offset
    );
}
#[test]
fn rate_limit_only_repeated_last_usage_does_not_double_count() {
    let f = Fixture::new();
    let mut s = db();
    f.add(100, 20, 20);
    collect_codex(&f.home, &mut s, "dev-01").unwrap();
    f.append(&packet(100, 20, 20));
    let rep = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!((rep.revised, rep.unchanged), (0, 1));
    assert_eq!(record(&s).record.usage.total().unwrap(), 120);
}
#[test]
fn incomplete_last_line_is_retried_after_append() {
    let f = Fixture::new();
    let mut s = db();
    let line = packet(100, 20, 20);
    let split = line.len() / 2;
    f.append(&line[..split]);
    let rep = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!(rep.pending_lines, 1);
    assert!(s.pending_outbox(200).unwrap().is_empty());
    f.append(&line[split..]);
    let next = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!((next.inserted, next.pending_lines), (1, 0));
    assert_eq!(record(&s).record.usage.total().unwrap(), 120);
}
#[test]
fn malformed_complete_line_does_not_advance_checkpoint_or_publish() {
    let f = Fixture::new();
    let mut s = db();
    f.add(100, 20, 20);
    collect_codex(&f.home, &mut s, "dev-01").unwrap();
    let old = record(&s);
    let cp = s
        .checkpoint("codex", old.record.session_id.as_deref().unwrap())
        .unwrap()
        .unwrap();
    f.add(200, 30, 40);
    f.append("{bad-json}\n");
    assert!(collect_codex(&f.home, &mut s, "dev-01").is_err());
    assert_eq!(record(&s).record_revision, 1);
    assert_eq!(s.checkpoint("codex", &cp.source_key).unwrap(), Some(cp));
}
#[test]
fn counter_regression_fails_closed() {
    let f = Fixture::new();
    let mut s = db();
    f.add(150, 40, 20);
    collect_codex(&f.home, &mut s, "dev-01").unwrap();
    f.add(100, 30, 10);
    assert!(collect_codex(&f.home, &mut s, "dev-01").is_err());
    assert_eq!(record(&s).record.usage.total().unwrap(), 170);
}
#[test]
fn rewrite_is_reconciled_by_revision_not_added_as_new_session() {
    let f = Fixture::new();
    let mut s = db();
    f.add(150, 40, 20);
    collect_codex(&f.home, &mut s, "dev-01").unwrap();
    let header = fs::read_to_string(&f.file)
        .unwrap()
        .lines()
        .next()
        .unwrap()
        .to_owned();
    fs::write(&f.file, format!("{header}\n{}", packet(100, 20, 10))).unwrap();
    let rep = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!(rep.revised, 1);
    assert_eq!(record(&s).record.usage.total().unwrap(), 110);
    assert_eq!(record(&s).record_revision, 2);
}
#[test]
fn session_move_to_archive_keeps_identity_and_revision() {
    let f = Fixture::new();
    let mut s = db();
    f.add(100, 20, 20);
    collect_codex(&f.home, &mut s, "dev-01").unwrap();
    let archived = f.home.join("archived_sessions");
    fs::create_dir_all(&archived).unwrap();
    fs::rename(&f.file, archived.join("rollout-test.jsonl")).unwrap();
    let rep = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!((rep.inserted, rep.revised, rep.unchanged), (0, 0, 1));
    assert_eq!(record(&s).record_revision, 1);
}
#[test]
fn malformed_counters_rejected_without_data() {
    let f = Fixture::new();
    let mut s = db();
    f.add(10, 20, 5);
    assert!(collect_codex(&f.home, &mut s, "dev-01").is_err());
    assert!(s.pending_outbox(200).unwrap().is_empty());
}
#[test]
fn only_session_files_are_discovered() {
    let f = Fixture::new();
    fs::write(f.home.join("not-an-event.json"), "private").unwrap();
    assert_eq!(discover_codex_files(&f.home).unwrap(), vec![f.file.clone()]);
}
#[test]
fn missing_root_is_valid_empty_scan() {
    let f = Fixture::new();
    let mut s = db();
    let rep = collect_codex(
        Path::new("/unlikely-tokenscope-nonexistent-codex"),
        &mut s,
        "dev-01",
    )
    .unwrap();
    assert_eq!(rep.scanned_files, 0);
    assert!(s.pending_outbox(200).unwrap().is_empty());
    drop(f);
}
#[test]
fn extra_long_line_is_bounded() {
    let f = Fixture::new();
    let mut s = db();
    f.append(&format!(
        "{{\"type\":\"response_item\",\"payload\":\"{}\"}}\n",
        "x".repeat(4 * 1024 * 1024)
    ));
    assert!(collect_codex(&f.home, &mut s, "dev-01").is_err());
    assert!(s.pending_outbox(200).unwrap().is_empty());
}

#[test]
fn incomplete_new_session_header_waits_without_blocking_other_sessions() {
    let f = Fixture::new();
    let mut s = db();
    f.add(100, 20, 20);
    let dir = f.home.join("sessions/2026/11/02");
    fs::write(
        dir.join("rollout-in-progress.jsonl"),
        "{\"type\":\"session_meta\"",
    )
    .unwrap();
    let r = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!(r.scanned_files, 1);
    assert_eq!(r.pending_lines, 1);
    assert_eq!(record(&s).record.usage.total().unwrap(), 120);
}

#[test]
fn token_count_without_usage_info_is_ignored() {
    let f = Fixture::new();
    let mut s = db();
    f.add(100, 20, 20);
    f.append("{\"timestamp\":\"2026-11-02T01:03:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":null,\"rate_limits\":{}}}\n");
    collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!(record(&s).record.usage.total().unwrap(), 120);
}

#[test]
fn no_codex_token_count_cannot_be_invented() {
    let f = Fixture::new();
    let mut s = db();
    f.append("{\"timestamp\":\"2026-11-02T01:00:00Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\"}}\n");
    let result = collect_codex(&f.home, &mut s, "dev-01").unwrap();
    assert_eq!(result.scanned_files, 1);
    assert_eq!(result.inserted, 0);
    assert!(s.pending_outbox(200).unwrap().is_empty());
}
