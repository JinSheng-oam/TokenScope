use std::{
    fs,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_local_store::LocalStore;

#[test]
fn standalone_deepseek_scan_collects_sessions() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("tokenscope-dsh-cli-{}-{nonce}", std::process::id()));
    let source = root.join("harness");
    fs::create_dir_all(source.join("sessions/project/session")).unwrap();
    fs::write(
        source.join("sessions/project/session/session.jsonl"),
        "{\"type\":\"session\",\"id\":\"cli-test\",\"version\":4}\n",
    )
    .unwrap();
    let db = root.join("test.db");
    let output = Command::new(env!("CARGO_BIN_EXE_tokenscope-agent"))
        .arg("--scan-deepseek-harness")
        .arg("--db")
        .arg(&db)
        .arg("--deepseek-harness-home")
        .arg(&source)
        .output()
        .unwrap();
    assert!(output.status.success());
    let store = LocalStore::open_readonly(&db).unwrap();
    assert!(store
        .source_checkpoints()
        .unwrap()
        .iter()
        .any(|checkpoint| checkpoint.tool_id == "deepseek-harness"
            && checkpoint.last_status == "complete"));
    drop(store);
    assert_eq!(root.parent(), Some(std::env::temp_dir().as_path()));
    fs::remove_dir_all(root).unwrap();
}
