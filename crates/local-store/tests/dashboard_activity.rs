use tokenscope_core_domain::{session_key, CanonicalRecord, Precision, TokenBreakdown, UsageScope};
use tokenscope_local_store::{
    AntigravityActivity, DashboardQuery, LocalIdentity, LocalStore, SourceCheckpoint,
};

fn record(
    record_id: char,
    tool_id: &str,
    session_id: String,
    model_id: Option<&str>,
    precision: Precision,
    event_at_ms: Option<i64>,
) -> CanonicalRecord {
    CanonicalRecord {
        record_id: record_id.to_string().repeat(64),
        owner_device_id: "device-a".into(),
        tool_id: tool_id.into(),
        scope: UsageScope::DeviceLocal,
        precision,
        account_id: None,
        model_id: model_id.map(str::to_owned),
        project_id: None,
        session_id: Some(session_id),
        event_at_ms,
        usage: TokenBreakdown {
            input_uncached_tokens: 100,
            output_tokens: 50,
            ..TokenBreakdown::default()
        },
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    }
}

fn checkpoint(tool: &str, key: String, fingerprint: &str, observed: i64) -> SourceCheckpoint {
    SourceCheckpoint {
        tool_id: tool.into(),
        source_key: key,
        source_fingerprint: fingerprint.into(),
        read_offset: 0,
        parser_version: 2,
        last_scan_ms: observed,
        last_complete_scan_ms: Some(observed),
        last_status: "complete".into(),
    }
}

fn store() -> LocalStore {
    let mut store = LocalStore::open_in_memory().unwrap();
    store
        .init_identity(&LocalIdentity {
            device_id: "device-a".into(),
            display_name: "Test PC".into(),
            time_zone: "UTC".into(),
        })
        .unwrap();
    store
}

#[test]
fn codex_model_calls_only_count_per_response_records_and_sessions_keep_precision() {
    let mut store = store();
    let session = session_key("codex", b"session-a").unwrap();
    let checkpoint = checkpoint("codex", session.clone(), r#"{"mode":"modern"}"#, 2_000);
    store
        .apply_scan(
            None,
            &checkpoint,
            &[
                record(
                    'a',
                    "codex",
                    session.clone(),
                    Some("gpt-5"),
                    Precision::Exact,
                    Some(1_500),
                ),
                record(
                    'b',
                    "codex",
                    session.clone(),
                    Some("gpt-5"),
                    Precision::Aggregate,
                    Some(1_600),
                ),
                record('c', "codex", session, None, Precision::Unknown, Some(1_700)),
            ],
            2_000,
            false,
        )
        .unwrap();

    let snapshot = store
        .dashboard_snapshot(DashboardQuery {
            range: "all",
            from_ms: None,
            to_ms: None,
            daily_ranges: &[],
            generated_at_ms: 2_000,
            time_zone: "UTC".into(),
            time_zone_valid: true,
            session_offset: 0,
        })
        .unwrap();
    let gpt_activity = snapshot
        .model_activity
        .iter()
        .find(|row| row.tool_id == "codex" && row.model_id == "gpt-5")
        .unwrap();
    assert_eq!(gpt_activity.response_count, 2);
    let unknown_activity = snapshot
        .model_activity
        .iter()
        .find(|row| row.tool_id == "codex" && row.model_id == "unknown-model")
        .unwrap();
    assert_eq!(unknown_activity.response_count, 1);

    let model_usage = snapshot
        .by_model
        .iter()
        .find(|row| row.key == "gpt-5")
        .unwrap();
    assert_eq!(model_usage.record_count, 2);
    assert_eq!(model_usage.exact_records, 1);
    assert_eq!(model_usage.aggregate_records, 1);
    assert_eq!(model_usage.estimated_records, 0);

    let session = &snapshot.session_usage.sessions[0];
    assert_eq!(session.known_response_count, Some(2));
    assert_eq!(session.unknown_response_count, Some(1));
    assert_eq!(session.usage_record_count, 3);
    assert_eq!(session.exact_records, 1);
    assert_eq!(session.aggregate_records, 1);
    assert_eq!(session.unknown_precision_records, 1);
}

#[test]
fn antigravity_session_keeps_untimed_tokens_and_separates_source_activity() {
    let mut store = store();
    let source_key = "e".repeat(64);
    let session = session_key("antigravity", b"conversation-a").unwrap();
    let checkpoint = checkpoint(
        "antigravity",
        source_key.clone(),
        "sha256:verified-source-snapshot",
        2_000,
    );
    let activity = AntigravityActivity {
        known_turns: 2,
        unknown_turns: 1,
        models: vec![("gemini-2.5-pro".into(), 2)],
    };
    store
        .reconcile_source_with_activity(
            None,
            &checkpoint,
            [record(
                'f',
                "antigravity",
                session,
                Some("gemini-2.5-pro"),
                Precision::Aggregate,
                None,
            )],
            2_000,
            Some(&activity),
        )
        .unwrap();

    let all = store
        .dashboard_snapshot(DashboardQuery {
            range: "all",
            from_ms: None,
            to_ms: None,
            daily_ranges: &[],
            generated_at_ms: 2_500,
            time_zone: "UTC".into(),
            time_zone_valid: true,
            session_offset: 0,
        })
        .unwrap();
    let session = &all.session_usage.sessions[0];
    assert_eq!(session.session_id, source_key);
    assert_eq!(session.total_tokens, 150);
    assert_eq!(session.untimed_records, 1);
    assert_eq!(session.known_response_count, Some(2));
    assert_eq!(session.unknown_response_count, Some(1));
    assert_eq!(session.activity_observed_at_ms, Some(2_000));
    assert_eq!(all.model_activity[0].response_count, 2);

    let selected = store
        .dashboard_snapshot(DashboardQuery {
            range: "7d",
            from_ms: Some(1_000),
            to_ms: Some(3_000),
            daily_ranges: &[],
            generated_at_ms: 2_500,
            time_zone: "UTC".into(),
            time_zone_valid: true,
            session_offset: 0,
        })
        .unwrap();
    // A conversation whose only records fall outside the range is not listed;
    // otherwise every historical conversation shows up with a zero total and
    // an empty model split.
    assert!(selected.session_usage.sessions.is_empty());
}
