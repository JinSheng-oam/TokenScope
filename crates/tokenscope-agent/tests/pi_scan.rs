use std::{fs, process::Command};
use tokenscope_core_domain::session_key;
use tokenscope_local_store::LocalStore;

#[test]
fn pi_scan_counts_billed_entries_without_recounting_a_fork() {
    let root = std::env::temp_dir().join(format!("tokenscope-pi-{}", uuid::Uuid::new_v4()));
    let source = root.join("sessions/project");
    fs::create_dir_all(&source).unwrap();
    let parent = source.join("parent.jsonl");
    let fork = source.join("fork.jsonl");
    let db = root.join("test.db");
    let copied = r#"{"type":"message","id":"a","timestamp":"2026-01-01T00:00:01Z","message":{"role":"assistant","model":"example-a","usage":{"input":10,"output":5,"cacheRead":80,"cacheWrite":5,"totalTokens":100}}}"#;
    let summary = r#"{"type":"compaction","id":"b","timestamp":"2026-01-01T00:00:02Z","tokensBefore":10000,"usage":{"input":12,"output":8,"totalTokens":20}}"#;
    let parent_header =
        r#"{"type":"session","version":3,"id":"parent","timestamp":"2026-01-01T00:00:00Z"}"#;
    fs::write(&parent, format!("{parent_header}\n{copied}\n{summary}\n")).unwrap();
    let fork_header = r#"{"type":"session","version":3,"id":"fork","timestamp":"2026-01-01T00:00:03Z","parentSession":"parent.jsonl"}"#;
    let cache = r#"{"type":"usage","id":"c","timestamp":"2026-01-01T00:00:04Z","model":"example-b","usage":{"cacheRead":50,"totalTokens":50}}"#;
    let tool = r#"{"type":"message","id":"d","timestamp":"2026-01-01T00:00:05Z","message":{"role":"toolResult","usage":{"input":4,"output":6,"totalTokens":10}}}"#;
    let fork_content = format!("{fork_header}\n{copied}\n{summary}\n{cache}\n{tool}\n");
    fs::write(&fork, &fork_content).unwrap();

    let run = || {
        let output = Command::new(env!("CARGO_BIN_EXE_tokenscope-agent"))
            .arg("--scan-pi")
            .arg("--pi-sessions")
            .arg(root.join("sessions"))
            .arg("--db")
            .arg(&db)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    assert!(run().contains("inserted=4"));
    assert!(run().contains("cached=2"));
    let check = |native: &str, count, total| {
        let store = LocalStore::open_readonly(&db).unwrap();
        let key = session_key("pi", native.as_bytes()).unwrap();
        let records = store.records_for_source("pi", &key, 10).unwrap();
        assert_eq!(records.len(), count);
        assert_eq!(
            records
                .iter()
                .map(|r| r.record.usage.total().unwrap())
                .sum::<u64>(),
            total
        );
    };
    check("parent", 2, 120);
    check("fork", 2, 60);
    fs::write(&fork, format!("{fork_content}{{")).unwrap();
    assert!(run().contains("skipped=1"));
    check("fork", 2, 60);
    fs::remove_dir_all(root).unwrap();
}
