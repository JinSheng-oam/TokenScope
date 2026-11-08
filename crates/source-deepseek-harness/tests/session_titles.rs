use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_core_domain::session_key;
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_deepseek_harness::{collect_deepseek_harness, PARSER_VERSION};

static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "tokenscope-dsh-title-{}-{nonce}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("sessions/project/session")).unwrap();
        Self(root)
    }

    fn write_log(&self, extra: &str) {
        let log = format!(
            "{}{}{}{}{}{}",
            "{\"type\":\"session\",\"id\":\"test-session\",\"version\":4}\n",
            "{\"type\":\"user/message\",\"data\":{\"source\":{\"kind\":\"user\"},\"content\":\"第一句对话\"}}\n",
            "{\"type\":\"session/title\",\"data\":{\"title\":\"临时标题\",\"source\":{\"kind\":\"fallback\"}}}\n",
            "{\"type\":\"session/title\",\"data\":{\"title\":\"真实会话标题\",\"source\":{\"kind\":\"provider\"}}}\n",
            "{\"type\":\"user/message\",\"data\":{\"source\":{\"kind\":\"user\"},\"content\":\"最后一句对话\"}}\n",
            extra
        );
        fs::write(self.0.join("sessions/project/session/session.jsonl"), log).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.0.parent() == Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn store() -> LocalStore {
    let mut store = LocalStore::open_in_memory().unwrap();
    store
        .init_identity(&LocalIdentity {
            device_id: "test-device".into(),
            display_name: "Test".into(),
            time_zone: "UTC".into(),
        })
        .unwrap();
    store
}

fn session_title(store: &LocalStore) -> Option<String> {
    store
        .session_analysis_page("test-device", None, None, 0, 50, None, None, None, None)
        .unwrap()
        .page
        .sessions[0]
        .display_name
        .clone()
}

const USAGE: &str = "{\"type\":\"assistant/message\",\"time\":1000,\"data\":{\"turn\":1,\"step\":1,\"usage\":{\"inputTokens\":10,\"outputTokens\":5}}}\n";

#[test]
fn source_title_overrides_dialogue_and_updates_after_rename() {
    let fixture = Fixture::new();
    fixture.write_log(USAGE);
    let mut store = store();
    collect_deepseek_harness(&fixture.0, &mut store, "test-device").unwrap();
    assert_eq!(session_title(&store).as_deref(), Some("真实会话标题"));
    assert_eq!(
        collect_deepseek_harness(&fixture.0, &mut store, "test-device")
            .unwrap()
            .unchanged,
        1
    );

    fixture.write_log(&format!("{USAGE}{}{}",
        "{\"type\":\"session/title\",\"data\":{\"title\":\"新的会话标题\",\"source\":{\"kind\":\"user\"}}}\n",
        "{\"type\":\"session/title\",\"data\":{\"title\":\"  \"}}\n"));
    collect_deepseek_harness(&fixture.0, &mut store, "test-device").unwrap();
    assert_eq!(session_title(&store).as_deref(), Some("新的会话标题"));
}

#[test]
fn old_checkpoint_reparses_without_usage_revision() {
    let fixture = Fixture::new();
    fixture.write_log(USAGE);
    let mut store = store();
    collect_deepseek_harness(&fixture.0, &mut store, "test-device").unwrap();
    let key = session_key("deepseek-harness", b"test-session").unwrap();
    let current = store.checkpoint("deepseek-harness", &key).unwrap().unwrap();
    let records = store
        .records_for_source("deepseek-harness", &key, 100)
        .unwrap()
        .into_iter()
        .map(|row| row.record)
        .collect::<Vec<_>>();
    let mut legacy = current.clone();
    legacy.parser_version = PARSER_VERSION - 1;
    legacy.last_scan_ms += 1;
    legacy.last_complete_scan_ms = Some(legacy.last_scan_ms);
    store
        .reconcile_complete_source(Some(&current), &legacy, records, legacy.last_scan_ms)
        .unwrap();
    store
        .upsert_session_name(
            &key,
            "deepseek-harness",
            "错误的旧标题",
            legacy.last_scan_ms,
        )
        .unwrap();

    let report = collect_deepseek_harness(&fixture.0, &mut store, "test-device").unwrap();
    assert_eq!(
        (report.unchanged, report.inserted, report.revised),
        (0, 0, 0)
    );
    assert_eq!(session_title(&store).as_deref(), Some("真实会话标题"));
    assert_eq!(
        store
            .checkpoint("deepseek-harness", &key)
            .unwrap()
            .unwrap()
            .parser_version,
        PARSER_VERSION
    );
}

#[test]
fn sessions_without_native_title_use_first_user_message() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("sessions/project/session/session.jsonl"), format!(
        "{}{}{}{}",
        "{\"type\":\"session\",\"id\":\"test-session\",\"version\":4}\n",
        "{\"type\":\"user/message\",\"data\":{\"source\":{\"kind\":\"user\"},\"content\":\"第一句\"}}\n",
        "{\"type\":\"user/message\",\"data\":{\"source\":{\"kind\":\"user\"},\"content\":\"最后一句\"}}\n",
        USAGE)).unwrap();
    let mut store = store();
    collect_deepseek_harness(&fixture.0, &mut store, "test-device").unwrap();
    assert_eq!(session_title(&store).as_deref(), Some("第一句"));
}
