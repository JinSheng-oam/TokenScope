use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_codex::collect_codex_precise;

const SESSION: &str = "019a9b8c-b777-4d55-9222-121212121212";

fn fixture() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("tokenscope-zstd-{}-{stamp}", std::process::id()));
    let folder = root.join("sessions/2026/11/02");
    fs::create_dir_all(&folder).unwrap();
    let body = format!(
        concat!(
            "{{\"timestamp\":\"2026-11-01T23:59:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"{session}\",\"originator\":\"Codex CLI\"}}}}\n",
            "{{\"timestamp\":\"2026-11-01T23:59:01Z\",\"type\":\"turn_context\",\"payload\":{{\"model\":\"gpt-6-sol\"}}}}\n",
            "{{\"timestamp\":\"2026-11-01T23:59:02Z\",\"type\":\"token_usage_record\",\"payload\":{{\"response_id\":\"resp-1\",\"usage\":{{\"input_tokens\":100,\"cached_input_tokens\":30,\"cache_write_input_tokens\":10,\"output_tokens\":20,\"reasoning_output_tokens\":2,\"total_tokens\":120}}}}}}\n"
        ),
        session = SESSION
    );
    let file = folder.join("rollout-sample.jsonl.zst");
    fs::write(&file, zstd::encode_all(body.as_bytes(), 3).unwrap()).unwrap();
    root
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

#[test]
fn zstd_rollout_is_collected_and_stays_idempotent() {
    let root = fixture();
    let mut db = store();
    let first = collect_codex_precise(&root, &mut db, "dev1").unwrap();
    assert_eq!((first.files, first.inserted), (1, 1));
    let rows = db.pending_outbox(10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].envelope.record.usage.total().unwrap(), 120);
    assert_eq!(
        rows[0].envelope.record.model_id.as_deref(),
        Some("gpt-6-sol")
    );

    let second = collect_codex_precise(&root, &mut db, "dev1").unwrap();
    assert_eq!((second.files, second.inserted), (1, 0));
    fs::remove_dir_all(&root).ok();
}
