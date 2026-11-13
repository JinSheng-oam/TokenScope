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
    let mut tokenless = blob("gemini-3-flash-a");
    tokenless.extend(scalar(2, 8));
    conn.execute(
        "INSERT INTO steps(idx,step_type,metadata) VALUES(8,15,?1)",
        params![field(1, &timestamp)],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(8,?1,0)",
        params![tokenless],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO gen_metadata(idx,data,size) VALUES(9,?1,0)",
        params![blob("gemini-3-flash-a")],
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
    let snapshot = store
        .dashboard_snapshot(tokenscope_local_store::DashboardQuery {
            range: "custom",
            from_ms: Some(1_800_000_000_250),
            to_ms: Some(1_800_000_000_251),
            daily_ranges: &[],
            generated_at_ms: 1_800_000_000_251,
            time_zone: "UTC".into(),
            time_zone_valid: true,
            session_offset: 0,
        })
        .unwrap();
    assert_eq!(snapshot.model_activity[0].response_count, 2);
    assert_eq!(snapshot.local.total_tokens, 3272);
    assert_eq!(store.antigravity_activity_summary().unwrap().known_turns, 3);
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
fn title_import_supports_nulls_preview_transcript_and_subagent_fallbacks() {
    let f = Fixture::new();
    f.write("antigravity", "explicit-id", &[Some("gemini-pro")]);
    f.write("antigravity", "preview-id", &[Some("gemini-pro")]);
    f.write("antigravity", "missing-summary-id", &[Some("gemini-pro")]);
    f.write("antigravity", "subagent-id", &[Some("gemini-pro")]);
    f.write("antigravity", "cleared-id", &[Some("gemini-pro")]);

    let summary_db = f.root.join("antigravity").join("conversation_summaries.db");
    let conn = Connection::open(&summary_db).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries(
            conversation_id TEXT PRIMARY KEY,
            title TEXT,
            preview TEXT,
            parent_conversation_id TEXT,
            agent_name TEXT,
            last_modified_time TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            "explicit-id",
            "  Explicit   Title  ",
            "ignored preview",
            "",
            "",
            "2026-11-13T10:00:00Z"
        ],
    )
    .unwrap();
    let long_preview = format!("Preview fallback {}", "a".repeat(220));
    conn.execute(
        "INSERT INTO conversation_summaries VALUES(?1,NULL,?2,NULL,NULL,?3)",
        params!["preview-id", long_preview, "2026-11-13T10:01:00Z"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES(?1,'','','missing-summary-id','research',?2)",
        params!["subagent-id", "2026-11-13T10:02:00Z"],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES(?1,'','',NULL,NULL,?2)",
        params!["cleared-id", "2026-11-13T09:00:00Z"],
    )
    .unwrap();
    drop(conn);

    let root_log = f
        .root
        .join("antigravity")
        .join("brain")
        .join("missing-summary-id")
        .join(".system_generated")
        .join("logs");
    std::fs::create_dir_all(&root_log).unwrap();
    std::fs::write(
        root_log.join("transcript.jsonl"),
        concat!(
            r#"{"source":"SYSTEM","type":"SYSTEM_MESSAGE","content":"ignore system"}"#,
            "\n",
            r#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\nFix   unnamed   sessions\n</USER_REQUEST>\n<ADDITIONAL_METADATA>noise</ADDITIONAL_METADATA>"}"#,
            "\n"
        ),
    )
    .unwrap();

    let sub_log = f
        .root
        .join("antigravity")
        .join("brain")
        .join("subagent-id")
        .join(".system_generated")
        .join("logs");
    std::fs::create_dir_all(&sub_log).unwrap();
    std::fs::write(
        sub_log.join("transcript.jsonl"),
        concat!(
            r#"{"source":"SYSTEM","type":"SYSTEM_MESSAGE","content":"subagent prompt"}"#,
            "\n"
        ),
    )
    .unwrap();

    let mut s = store();
    let cleared_key =
        tokenscope_core_domain::stable_record_id("antigravity", b"conversation", b"cleared-id")
            .unwrap();
    s.upsert_session_name(&cleared_key, "antigravity", "Old Stale Title", 1)
        .unwrap();

    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();

    let page = s
        .session_analysis_page(
            "test-device",
            None,
            None,
            0,
            20,
            None,
            Some("antigravity"),
            None,
            None,
        )
        .unwrap();
    let by_id: std::collections::HashMap<String, Option<String>> = page
        .page
        .sessions
        .into_iter()
        .map(|row| (row.session_id, row.display_name))
        .collect();

    let key_for = |id: &str| {
        tokenscope_core_domain::stable_record_id("antigravity", b"conversation", id.as_bytes())
            .unwrap()
    };

    assert_eq!(
        by_id.get(&key_for("explicit-id")).cloned().flatten(),
        Some("Explicit Title".into())
    );
    let preview_name = by_id
        .get(&key_for("preview-id"))
        .cloned()
        .flatten()
        .unwrap();
    assert!(preview_name.starts_with("Preview fallback "));
    assert_eq!(preview_name.chars().count(), 80);
    assert_eq!(
        by_id.get(&key_for("missing-summary-id")).cloned().flatten(),
        Some("Fix unnamed sessions".into())
    );
    assert_eq!(
        by_id.get(&key_for("subagent-id")).cloned().flatten(),
        Some("[research] Fix unnamed sessions".into())
    );
    assert_eq!(by_id.get(&key_for("cleared-id")).cloned().flatten(), None);
}

#[test]
fn import_titles_supports_minimal_schema_and_ignores_unsafe_ids() {
    let f = Fixture::new();
    f.write("antigravity", "minimal-id", &[Some("gemini-pro")]);
    let summary_db = f.root.join("antigravity").join("conversation_summaries.db");
    let conn = Connection::open(&summary_db).unwrap();
    conn.execute_batch(
        "CREATE TABLE conversation_summaries(conversation_id TEXT PRIMARY KEY, title TEXT);",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES('minimal-id', 'Minimal Schema Title')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO conversation_summaries VALUES('../escape', '')",
        [],
    )
    .unwrap();
    drop(conn);

    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    let imported = tokenscope_source_antigravity::import_titles(&f.root, &mut s).unwrap();
    assert_eq!(imported, 1);

    let page = s
        .session_analysis_page(
            "test-device",
            None,
            None,
            0,
            20,
            None,
            Some("antigravity"),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        page.page.sessions[0].display_name.as_deref(),
        Some("Minimal Schema Title")
    );
}

#[test]
fn side_question_wrapper_extracts_actual_question() {
    let f = Fixture::new();
    f.write("antigravity", "side-q", &[Some("gemini-pro")]);
    let root_log = f
        .root
        .join("antigravity")
        .join("brain")
        .join("side-q")
        .join(".system_generated")
        .join("logs");
    std::fs::create_dir_all(&root_log).unwrap();
    std::fs::write(
        root_log.join("transcript.jsonl"),
        r#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>\nThis is a side question from the user. Your tools are disabled. Here is the question: How does cache rate work?\n</USER_REQUEST>\n<ADDITIONAL_METADATA>meta</ADDITIONAL_METADATA>"}"#,
    )
    .unwrap();

    let mut s = store();
    collect_antigravity(&f.root, &mut s, "test-device", true).unwrap();
    let page = s
        .session_analysis_page(
            "test-device",
            None,
            None,
            0,
            20,
            None,
            Some("antigravity"),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        page.page.sessions[0].display_name.as_deref(),
        Some("How does cache rate work?")
    );
}
