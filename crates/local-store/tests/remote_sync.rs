mod common;
use common::{rec, setup};
#[test]
fn remote_pull_applies_revision_and_advances_cursor_atomically() {
    use tokenscope_core_domain::RevisionedRecord;
    use tokenscope_local_store::RemoteUsageChange;
    let mut db = setup();
    let mut remote = rec("remote-event", 40);
    remote.owner_device_id = "remote-pc-001".into();
    let first = RevisionedRecord::from_store(remote.clone(), 1).unwrap();
    let n = db
        .apply_remote_changes(
            &[RemoteUsageChange {
                change_seq: 1,
                owner_device_id: "remote-pc-001".into(),
                envelope: first,
            }],
            1,
        )
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(db.pull_cursor().unwrap(), 1);
    let totals = db
        .remote_usage_totals(Some("remote-pc-001"), None, None)
        .unwrap();
    assert_eq!(totals.total_tokens, 42);
    assert_eq!(totals.record_count, 1);
    let by_device = db.remote_usage_by_device(None, None).unwrap();
    assert_eq!(by_device.len(), 1);
    assert_eq!(by_device[0].owner_device_id, "remote-pc-001");
    remote.usage.input_uncached_tokens = 80;
    let second = RevisionedRecord::from_store(remote.clone(), 2).unwrap();
    assert_eq!(
        db.apply_remote_changes(
            &[RemoteUsageChange {
                change_seq: 2,
                owner_device_id: "remote-pc-001".into(),
                envelope: second
            }],
            2
        )
        .unwrap(),
        1
    );
    assert_eq!(
        db.remote_usage_totals(None, None, None)
            .unwrap()
            .total_tokens,
        82
    );
    remote.is_deleted = true;
    remote.usage = Default::default();
    remote.estimated_cost_microusd = None;
    let tombstone = RevisionedRecord::from_store(remote, 3).unwrap();
    db.apply_remote_changes(
        &[RemoteUsageChange {
            change_seq: 3,
            owner_device_id: "remote-pc-001".into(),
            envelope: tombstone,
        }],
        3,
    )
    .unwrap();
    assert_eq!(
        db.remote_usage_totals(None, None, None)
            .unwrap()
            .total_tokens,
        0
    );
}

#[test]
fn remote_breakdowns_keep_device_tool_model_and_project_dimensions() {
    use tokenscope_core_domain::RevisionedRecord;
    use tokenscope_local_store::RemoteUsageChange;
    let mut db = setup();
    let mut a = rec("remote-a", 10);
    a.owner_device_id = "remote-pc-001".into();
    let prj = tokenscope_core_domain::project_key("codex", b"alpha").unwrap();
    a.tool_id = "codex".into();
    a.model_id = Some("gpt-6".into());
    a.project_id = Some(prj);
    let env = RevisionedRecord::from_store(a, 1).unwrap();
    db.apply_remote_changes(
        &[RemoteUsageChange {
            change_seq: 1,
            owner_device_id: "remote-pc-001".into(),
            envelope: env,
        }],
        1,
    )
    .unwrap();
    let tools = db.remote_usage_breakdown("tool_id", None, None).unwrap();
    let models = db.remote_usage_breakdown("model_id", None, None).unwrap();
    let projects = db.remote_usage_breakdown("project_id", None, None).unwrap();
    assert_eq!(
        (
            tools.len(),
            tools[0].owner_device_id.as_str(),
            tools[0].key.as_str(),
            tools[0].total_tokens
        ),
        (1, "remote-pc-001", "codex", 12)
    );
    assert_eq!(models[0].key, "gpt-6");
    assert_eq!(projects[0].owner_device_id, "remote-pc-001");
    assert!(db
        .remote_usage_breakdown("payload_json", None, None)
        .is_err());
}

#[test]
fn invalid_remote_page_rolls_back_cache_and_cursor() {
    use tokenscope_core_domain::RevisionedRecord;
    use tokenscope_local_store::RemoteUsageChange;
    let mut db = setup();
    let mut remote = rec("remote-event", 5);
    remote.owner_device_id = "remote-pc-001".into();
    let valid = RevisionedRecord::from_store(remote.clone(), 1).unwrap();
    let mut invalid = RevisionedRecord::from_store(remote, 2).unwrap();
    invalid.payload_hash = "a".repeat(64);
    let page = [
        RemoteUsageChange {
            change_seq: 1,
            owner_device_id: "remote-pc-001".into(),
            envelope: valid,
        },
        RemoteUsageChange {
            change_seq: 2,
            owner_device_id: "remote-pc-001".into(),
            envelope: invalid,
        },
    ];
    assert!(db.apply_remote_changes(&page, 2).is_err());
    assert_eq!(db.pull_cursor().unwrap(), 0);
    assert_eq!(
        db.remote_usage_totals(None, None, None)
            .unwrap()
            .record_count,
        0
    );
}

#[test]
fn webdav_device_batch_apply_and_cursor_tracking() {
    use tokenscope_core_domain::RevisionedRecord;
    let mut db = setup();
    assert_eq!(db.webdav_cursor("remote-mac-01").unwrap(), 0);

    let mut remote = rec("remote-msg-1", 100);
    remote.owner_device_id = "remote-mac-01".into();
    let env1 = RevisionedRecord::from_store(remote.clone(), 1).unwrap();

    let applied = db
        .apply_remote_device_records("remote-mac-01", 1, &[env1], 1000)
        .unwrap();
    assert_eq!(applied, 1);
    assert_eq!(db.webdav_cursor("remote-mac-01").unwrap(), 1);

    // Re-applying same or older batch seq is a no-op
    let reapply = db
        .apply_remote_device_records("remote-mac-01", 1, &[], 2000)
        .unwrap();
    assert_eq!(reapply, 0);

    // Apply batch 2 with an updated record
    remote.usage.output_tokens = 50;
    let env2 = RevisionedRecord::from_store(remote, 2).unwrap();
    let applied2 = db
        .apply_remote_device_records("remote-mac-01", 2, &[env2], 3000)
        .unwrap();
    assert_eq!(applied2, 1);
    assert_eq!(db.webdav_cursor("remote-mac-01").unwrap(), 2);

    let totals = db
        .remote_usage_totals(Some("remote-mac-01"), None, None)
        .unwrap();
    assert_eq!(totals.record_count, 1);
    assert_eq!(totals.output_tokens, 50);

    let cursors = db.all_webdav_cursors().unwrap();
    assert_eq!(cursors.len(), 1);
    assert_eq!(cursors[0], ("remote-mac-01".to_string(), 2));
}

#[test]
fn remote_models_estimate_keyword_aliases_without_rewriting_peer_costs() {
    use tokenscope_core_domain::RevisionedRecord;
    use tokenscope_local_store::{ModelPriceEntry, RemoteUsageChange};

    let mut db = setup();
    db.install_model_price_catalog(
        &format!("models.dev:base-v4:{}", "a".repeat(64)),
        &[ModelPriceEntry {
            provider_id: "deepseek".into(),
            model_id: "deepseek-v4-flash".into(),
            input_rate_nanos: Some(1_000_000_000),
            output_rate_nanos: Some(2_000_000_000),
            cache_read_rate_nanos: None,
            cache_write_rate_nanos: None,
            reasoning_rate_nanos: None,
        }],
        None,
        1_000,
    )
    .unwrap();

    let mut raw = rec("remote-alias", 10);
    raw.owner_device_id = "remote-pc-001".into();
    raw.estimated_cost_microusd = None;
    raw.price_version = None;
    raw.model_id = Some("magpie/command/deepseek/deepseek-v4.1-flash".into());
    let mut priced = rec("remote-alias-priced", 10);
    priced.owner_device_id.clone_from(&raw.owner_device_id);
    priced.model_id.clone_from(&raw.model_id);
    priced.estimated_cost_microusd = Some(20);
    db.apply_remote_changes(
        &[raw, priced]
            .into_iter()
            .enumerate()
            .map(|(i, record)| RemoteUsageChange {
                change_seq: (i + 1) as u64,
                owner_device_id: record.owner_device_id.clone(),
                envelope: RevisionedRecord::from_store(record, 1).unwrap(),
            })
            .collect::<Vec<_>>(),
        2,
    )
    .unwrap();

    let snapshot = db
        .dashboard_snapshot(tokenscope_local_store::DashboardQuery {
            range: "all",
            from_ms: None,
            to_ms: None,
            daily_ranges: &[],
            generated_at_ms: 1_000,
            time_zone: "UTC".into(),
            time_zone_valid: true,
            session_offset: 0,
        })
        .unwrap();
    assert_eq!(snapshot.remote.estimated_cost_microusd, Some(34));
    assert_eq!(snapshot.remote.priced_records, 2);
    assert_eq!(snapshot.remote.unpriced_records, 0);
    assert_eq!(
        snapshot.remote_by_device[0].usage.estimated_cost_microusd,
        Some(34)
    );
    assert_eq!(
        snapshot.remote_by_device[0].estimated_cost_microusd,
        Some(34)
    );
    for rows in [
        &snapshot.remote_by_model,
        &snapshot.remote_by_tool,
        &snapshot.remote_by_project,
    ] {
        assert_eq!(rows[0].estimated_cost_microusd, Some(34));
        assert_eq!(rows[0].record_count, 2);
    }
    assert_eq!(
        db.remote_usage_totals(None, None, None)
            .unwrap()
            .estimated_cost_microusd,
        Some(20)
    );
}
