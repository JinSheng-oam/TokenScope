use super::{
    codec::{inspect, rewrite},
    locate::{file_revision, legacy_codex_identity, locate_in},
    storage::{create_copy, move_to_backup, stamp},
    Edit,
};
use std::{
    fs::{self, OpenOptions},
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_core_domain::session_key;

#[test]
fn antigravity_history_only_exposes_user_and_assistant_text() {
    let transcript = br#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>hello</USER_REQUEST>\n<ADDITIONAL_METADATA>private metadata</ADDITIONAL_METADATA>"}
{"source":"MODEL","type":"GENERIC","content":"tool result"}
{"source":"MODEL","type":"PLANNER_RESPONSE","content":"answer","thinking":"private reasoning"}
"#;
    let history = inspect("antigravity", transcript, 0, false).unwrap();
    assert!(!history.writable);
    assert_eq!(history.total, 2);
    assert_eq!(history.messages[0].text, "hello");
    assert_eq!(history.messages[1].text, "answer");
}

#[test]
fn codex_locator_groups_continuations_and_resolves_legacy_keys() {
    let home = std::env::temp_dir().join(format!(
        "tokenscope-locator-{}-{}",
        std::process::id(),
        stamp().unwrap()
    ));
    let dir = home.join("sessions");
    fs::create_dir_all(&dir).unwrap();
    let original = dir.join("rollout-01-thread.jsonl");
    let continued = dir.join("rollout-02-thread_continuation.jsonl");
    let header = r#"{"type":"session_meta","payload":{"id":"thread"}}"#;
    fs::write(&original, header).unwrap();
    fs::write(&continued, header).unwrap();
    let key = session_key("codex", b"thread").unwrap();
    assert_eq!(
        locate_in("codex", &key, &home, &["sessions"]).unwrap(),
        (vec![original.clone(), continued.clone()], None)
    );
    let alias = legacy_codex_identity("thread", &continued).unwrap();
    let key = session_key("codex", alias.as_bytes()).unwrap();
    assert_eq!(
        locate_in("codex", &key, &home, &["sessions"]).unwrap(),
        (vec![original.clone(), continued.clone()], Some(1))
    );
    assert_ne!(
        file_revision(&original, "same"),
        file_revision(&continued, "same")
    );
    fs::remove_file(original).unwrap();
    fs::remove_file(continued).unwrap();
    fs::remove_dir(dir).unwrap();
    fs::remove_dir(home).unwrap();
}

#[test]
fn codex_updates_display_copy_without_rewriting_usage() {
    let bytes = br#"{"type":"event_msg","payload":{"type":"task_started"}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"before"}]}}
{"type":"event_msg","payload":{"type":"agent_message","message":"before"}}
{"type":"event_msg", "payload":{"type":"token_count","total":123}}
{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"before"}}
"#;
    let history = inspect("codex", bytes, 0, true).unwrap();
    assert!(history.writable);
    let edit = Edit {
        line: history.messages[0].line,
        pointer: history.messages[0].pointer.clone(),
        text: Some("改后\n第二行".into()),
        delete: false,
    };
    let output = rewrite("codex", bytes, &edit).unwrap();
    let text = std::str::from_utf8(&output).unwrap();
    assert_eq!(text.matches("改后").count(), 3);
    assert!(text.contains(r#"{"type":"event_msg", "payload":{"type":"token_count","total":123}}"#));
    let first = bytes.split_inclusive(|&b| b == b'\n').next().unwrap();
    assert!(!inspect("codex", first, 0, true).unwrap().writable);
    let deleted = rewrite(
        "codex",
        bytes,
        &Edit {
            text: None,
            delete: true,
            ..edit
        },
    )
    .unwrap();
    let deleted_text = std::str::from_utf8(&deleted).unwrap();
    assert!(!deleted_text.contains("before"));
    assert!(deleted_text.contains(r#""total":123"#));
    assert_eq!(inspect("codex", &deleted, 0, true).unwrap().total, 0);
}

#[test]
fn only_conversation_text_is_editable_in_claude_and_deepseek() {
    for (tool, bytes) in [
        ("claude", br#"{"type":"assistant","message":{"content":[{"type":"text","text":"before"},{"type":"tool_use","text":"keep"}]}}"#.as_slice()),
        ("deepseek-harness", br#"{"type":"user/message","data":{"message":{"content":"before","source":{"kind":"user"}}}}"#.as_slice()),
    ] {
        let history = inspect(tool, bytes, 0, true).unwrap();
        assert_eq!(history.total, 1);
        let edit = Edit { line: 0, pointer: history.messages[0].pointer.clone(), text: Some("after".into()), delete: false };
        let rewritten = rewrite(tool, bytes, &edit).unwrap();
        assert_eq!(inspect(tool, &rewritten, 0, true).unwrap().messages[0].text, "after");
        assert!(rewrite(tool, bytes, &Edit { pointer: "/type".into(), ..edit }).is_err());
    }
}

#[test]
fn replacement_preserves_a_backup_while_the_source_is_open() {
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-history-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&dir).unwrap();
    let source = dir.join("source.jsonl");
    let temporary = dir.join("replacement.tmp");
    let backup = dir.join("source.bak");
    fs::write(&source, b"original").unwrap();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0x1 | 0x4);
    }
    let open = options.open(&source).unwrap();
    create_copy(&backup, b"original", &open).unwrap();
    create_copy(&temporary, b"edited", &open).unwrap();
    crate::agent_customizations::replace_file(&temporary, &source, true).unwrap();
    assert_eq!(fs::read(&backup).unwrap(), b"original");
    assert_eq!(fs::read(&source).unwrap(), b"edited");
    drop(open);
    let removed = dir.join("removed.bak");
    let open = options.open(&source).unwrap();
    move_to_backup(&source, &removed).unwrap();
    assert!(!source.exists());
    assert_eq!(fs::read(&removed).unwrap(), b"edited");
    drop(open);
    fs::remove_file(removed).unwrap();
    fs::remove_file(backup).unwrap();
    fs::remove_dir(dir).unwrap();
}
