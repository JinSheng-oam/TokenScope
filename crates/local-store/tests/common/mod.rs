#![allow(dead_code)]

use std::path::PathBuf;
use tokenscope_core_domain::{
    stable_record_id, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{LocalIdentity, LocalStore, SourceCheckpoint};

pub fn identity() -> LocalIdentity {
    LocalIdentity {
        device_id: "dev-test-001".into(),
        display_name: "Dev".into(),
        time_zone: "Asia/Shanghai".into(),
    }
}
pub fn cp(key: &str, scan: i64) -> SourceCheckpoint {
    SourceCheckpoint {
        tool_id: "codex".into(),
        source_key: key.into(),
        source_fingerprint: "file:abc".into(),
        read_offset: 100,
        parser_version: 1,
        last_scan_ms: scan,
        last_complete_scan_ms: Some(scan),
        last_status: "complete".into(),
    }
}
pub fn rec(native: &str, tokens: u64) -> CanonicalRecord {
    CanonicalRecord {
        record_id: stable_record_id("codex", b"source-canonical", native.as_bytes()).unwrap(),
        owner_device_id: "dev-test-001".into(),
        tool_id: "codex".into(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Exact,
        account_id: None,
        model_id: Some("gpt-6".into()),
        project_id: None,
        session_id: None,
        event_at_ms: Some(1000),
        usage: TokenBreakdown {
            input_uncached_tokens: tokens,
            output_tokens: 2,
            reasoning_tokens: Some(1),
            ..Default::default()
        },
        estimated_cost_microusd: Some(125),
        price_version: Some("v1".into()),
        is_deleted: false,
    }
}
pub fn setup() -> LocalStore {
    let mut db = LocalStore::open_in_memory().unwrap();
    db.init_identity(&identity()).unwrap();
    db
}
pub fn unique_db() -> PathBuf {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tokenscope-test-{}-{nanos}.sqlite",
        std::process::id()
    ))
}
