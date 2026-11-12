use super::*;
use crate::repository::models::{LocalIdentity, SourceCheckpoint};
use tokenscope_core_domain::{CanonicalRecord, Precision, TokenBreakdown, UsageScope};

fn store() -> LocalStore {
    let mut store = LocalStore::open_in_memory().unwrap();
    store
        .init_identity(&LocalIdentity {
            device_id: "pricing-device".into(),
            display_name: "Test device".into(),
            time_zone: "UTC".into(),
        })
        .unwrap();
    store
}

fn entry(model_id: &str, input: i64, output: i64) -> ModelPriceEntry {
    ModelPriceEntry {
        provider_id: "openai".into(),
        model_id: model_id.into(),
        input_rate_nanos: Some(input),
        output_rate_nanos: Some(output),
        cache_read_rate_nanos: Some(500_000_000),
        cache_write_rate_nanos: Some(2_500_000_000),
        reasoning_rate_nanos: Some(16_000_000_000),
    }
}

fn record(id: char, model: &str) -> CanonicalRecord {
    CanonicalRecord {
        record_id: id.to_string().repeat(64),
        owner_device_id: "pricing-device".into(),
        tool_id: "codex".into(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Exact,
        account_id: None,
        model_id: Some(model.into()),
        project_id: None,
        session_id: None,
        event_at_ms: Some(1_000),
        usage: TokenBreakdown {
            input_uncached_tokens: 10,
            cache_read_tokens: 20,
            cache_write_tokens: 3,
            output_tokens: 100,
            unclassified_tokens: 0,
            reasoning_tokens: Some(25),
        },
        estimated_cost_microusd: None,
        price_version: None,
        is_deleted: false,
    }
}

fn checkpoint(source: &str, at: i64) -> SourceCheckpoint {
    SourceCheckpoint {
        tool_id: "codex".into(),
        source_key: source.into(),
        source_fingerprint: format!("fingerprint-{at}"),
        read_offset: 0,
        parser_version: 1,
        last_scan_ms: at,
        last_complete_scan_ms: Some(at),
        last_status: "complete".into(),
    }
}

fn install(store: &mut LocalStore, version_digit: char, entries: &[ModelPriceEntry]) {
    store
        .install_model_price_catalog(
            &format!("models.dev:{}", version_digit.to_string().repeat(64)),
            entries,
            None,
            1_000,
        )
        .unwrap();
}

#[test]
fn prices_exact_classified_dimensions_and_reasoning_once() {
    let mut store = store();
    install(
        &mut store,
        'a',
        &[entry("gpt-5.4", 2_000_000_000, 8_000_000_000)],
    );
    let input = record('a', "gpt-5.4");
    store
        .apply_scan(
            None,
            &checkpoint("source", 1_000),
            std::slice::from_ref(&input),
            1_000,
            false,
        )
        .unwrap();
    let saved = store.get_record(&input.record_id).unwrap().unwrap();
    let expected_version = format!("models.dev:{}", "a".repeat(64));
    assert_eq!(saved.record.estimated_cost_microusd, Some(1_038));
    assert_eq!(
        saved.record.price_version.as_deref(),
        Some(expected_version.as_str())
    );
}

#[test]
fn pricing_requires_exact_model_and_classified_tokens() {
    let mut store = store();
    install(
        &mut store,
        'b',
        &[entry("gpt-5.4", 2_000_000_000, 8_000_000_000)],
    );
    let unmatched = record('b', "gpt-5.4-preview");
    let mut unclassified = record('c', "gpt-5.4");
    unclassified.precision = Precision::Aggregate;
    unclassified.usage.unclassified_tokens = 4;
    store
        .apply_scan(
            None,
            &checkpoint("source", 1_000),
            &[unmatched.clone(), unclassified.clone()],
            1_000,
            false,
        )
        .unwrap();
    assert_eq!(
        store
            .get_record(&unmatched.record_id)
            .unwrap()
            .unwrap()
            .record
            .estimated_cost_microusd,
        None
    );
    assert_eq!(
        store
            .get_record(&unclassified.record_id)
            .unwrap()
            .unwrap()
            .record
            .estimated_cost_microusd,
        None
    );
}

#[test]
fn catalog_install_backfills_and_enqueues_a_new_revision_atomically() {
    let mut store = store();
    let usage = record('d', "gpt-5.4");
    store
        .apply_scan(
            None,
            &checkpoint("source", 1_000),
            std::slice::from_ref(&usage),
            1_000,
            false,
        )
        .unwrap();
    assert_eq!(
        store
            .get_record(&usage.record_id)
            .unwrap()
            .unwrap()
            .record
            .estimated_cost_microusd,
        None
    );
    let report = store
        .install_model_price_catalog(
            &format!("models.dev:{}", "e".repeat(64)),
            &[entry("gpt-5.4", 2_000_000_000, 8_000_000_000)],
            Some("\"v1\""),
            2_000,
        )
        .unwrap();
    let saved = store.get_record(&usage.record_id).unwrap().unwrap();
    let outbox_revision: i64 = store
        .conn
        .query_row(
            "SELECT record_revision FROM sync_outbox WHERE record_id=?1",
            [&usage.record_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(report.backfilled_records, 1);
    assert_eq!(saved.record_revision, 2);
    assert_eq!(saved.record.estimated_cost_microusd, Some(1_038));
    assert_eq!(outbox_revision, 2);
}

#[test]
fn already_priced_records_keep_the_original_snapshot_version() {
    let mut store = store();
    install(
        &mut store,
        'f',
        &[entry("gpt-5.4", 2_000_000_000, 8_000_000_000)],
    );
    let usage = record('f', "gpt-5.4");
    let first = checkpoint("source", 1_000);
    store
        .apply_scan(None, &first, std::slice::from_ref(&usage), 1_000, false)
        .unwrap();
    install(
        &mut store,
        '1',
        &[entry("gpt-5.4", 5_000_000_000, 20_000_000_000)],
    );
    store
        .apply_scan(
            Some(&first),
            &checkpoint("source", 2_000),
            std::slice::from_ref(&usage),
            2_000,
            false,
        )
        .unwrap();
    let saved = store.get_record(&usage.record_id).unwrap().unwrap();
    let expected_version = format!("models.dev:{}", "f".repeat(64));
    assert_eq!(saved.record.estimated_cost_microusd, Some(1_038));
    assert_eq!(
        saved.record.price_version.as_deref(),
        Some(expected_version.as_str())
    );
}

#[test]
fn antigravity_backfill_uses_reference_without_reclassifying_tokens() {
    let mut store = store();
    let mut usage = record('f', "internal-model-alias");
    usage.tool_id = "antigravity".into();
    usage.precision = Precision::Aggregate;
    usage.usage = TokenBreakdown {
        unclassified_tokens: 1_000_000,
        cache_read_tokens: 1_000_000,
        ..TokenBreakdown::default()
    };
    let mut source = checkpoint("antigravity", 1_000);
    source.tool_id = "antigravity".into();
    store
        .apply_scan(None, &source, &[usage.clone()], 1_000, false)
        .unwrap();

    let mut reference = entry("gemini-3-flash-preview", 500_000_000, 3_000_000_000);
    reference.provider_id = "google".into();
    reference.cache_read_rate_nanos = Some(50_000_000);
    install(&mut store, 'f', &[reference]);

    let saved = store.get_record(&usage.record_id).unwrap().unwrap().record;
    assert_eq!(saved.estimated_cost_microusd, Some(550_000));
    assert_eq!(saved.usage, usage.usage);
    assert_eq!(saved.precision, Precision::Aggregate);
}

#[test]
fn named_channel_uses_its_own_quote_without_falling_back_to_the_manufacturer() {
    let mut db = store();
    let native = entry("gpt-5.4", 2_000_000_000, 8_000_000_000);
    let mut channel = entry("gpt-5.4", 1_000_000_000, 2_000_000_000);
    channel.provider_id = "opencode-go".into();
    install(&mut db, 'a', &[native, channel]);
    let quoted = record('a', "opencode-go/gpt-5.4");
    let unavailable = record('b', "xiaomi/gpt-5.4");
    db.apply_scan(
        None,
        &checkpoint("source", 1_000),
        &[quoted.clone(), unavailable.clone()],
        1_000,
        false,
    )
    .unwrap();
    let cost = |id| {
        db.get_record(id)
            .unwrap()
            .unwrap()
            .record
            .estimated_cost_microusd
    };
    assert_eq!(cost(&quoted.record_id), Some(578));
    assert_eq!(cost(&unavailable.record_id), None);
}

#[test]
fn manual_price_and_exact_match_preference_apply_to_backfill() {
    let dir = std::env::temp_dir().join(format!(
        "tokenscope-pricing-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("test.db");
    let mut db = LocalStore::open(&path).unwrap();
    db.init_identity(&LocalIdentity {
        device_id: "pricing-device".into(),
        display_name: "Test device".into(),
        time_zone: "UTC".into(),
    })
    .unwrap();
    let alias = record('a', "proxy/openai/gpt-5.4");
    let exact = record('b', "gpt-5.4");
    let mut preferences = PricingPreferences {
        keyword_matching: false,
        manual_entries: Vec::new(),
    };
    preferences.write(&path).unwrap();
    install(
        &mut db,
        'a',
        &[entry("gpt-5.4", 2_000_000_000, 8_000_000_000)],
    );
    db.apply_scan(
        None,
        &checkpoint("source", 1_000),
        &[alias.clone(), exact.clone()],
        1_000,
        false,
    )
    .unwrap();
    assert!(db
        .get_record(&alias.record_id)
        .unwrap()
        .unwrap()
        .record
        .estimated_cost_microusd
        .is_none());
    let original = db
        .get_record(&exact.record_id)
        .unwrap()
        .unwrap()
        .record
        .estimated_cost_microusd;
    assert!(original.is_some());

    preferences
        .manual_entries
        .push(entry("proxy/openai/gpt-5.4", 1_000_000_000, 2_000_000_000));
    preferences.write(&path).unwrap();
    assert_eq!(PricingPreferences::read(&path).unwrap(), preferences);
    assert_eq!(db.backfill_unpriced_model_costs(2_000).unwrap(), 1);
    let saved = db.get_record(&alias.record_id).unwrap().unwrap().record;
    assert!(saved.estimated_cost_microusd.is_some());
    assert!(saved.price_version.unwrap().starts_with("manual:"));
    assert_eq!(
        db.get_record(&exact.record_id)
            .unwrap()
            .unwrap()
            .record
            .estimated_cost_microusd,
        original
    );
    drop(db);
    for name in ["test.db", "test.pricing.json", "test.db-wal", "test.db-shm"] {
        let file = dir.join(name);
        if file.exists() {
            std::fs::remove_file(file).unwrap();
        }
    }
    std::fs::remove_dir(dir).unwrap();
}
