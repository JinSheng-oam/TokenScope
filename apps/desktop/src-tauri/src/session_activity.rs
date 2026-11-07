use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

mod codex;
mod other;
use codex::read_codex_activity;
use other::other_activities;

const ACTIVE_TURN_RETENTION_MS: u64 = 20 * 60 * 1000;
const FRESH_FILE_ACTIVE_MS: u64 = 90 * 1000;
static ACTIVE_TURNS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
fn is_generating(session_id: &str, explicit: Option<bool>, modified_ms: u64, now_ms: u64) -> bool {
    let states = ACTIVE_TURNS.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut states) = states.lock() else {
        return explicit.unwrap_or(false)
            || now_ms.saturating_sub(modified_ms) <= FRESH_FILE_ACTIVE_MS;
    };
    states.retain(|_, last_activity| {
        now_ms.saturating_sub(*last_activity) <= ACTIVE_TURN_RETENTION_MS
    });
    if let Some(active) = explicit {
        if active {
            states.insert(session_id.to_owned(), modified_ms);
        } else {
            states.remove(session_id);
        }
        return active;
    }
    let fresh = now_ms.saturating_sub(modified_ms) <= FRESH_FILE_ACTIVE_MS;
    if fresh {
        states.insert(session_id.to_owned(), modified_ms);
        return true;
    }
    let Some(last_activity) = states.get(session_id).copied() else {
        return false;
    };
    if modified_ms > last_activity && now_ms.saturating_sub(modified_ms) <= ACTIVE_TURN_RETENTION_MS
    {
        states.insert(session_id.to_owned(), modified_ms);
        return true;
    }
    now_ms.saturating_sub(last_activity) <= ACTIVE_TURN_RETENTION_MS
}

fn remembered_active(session_id: &str, now_ms: u64) -> bool {
    ACTIVE_TURNS
        .get()
        .and_then(|states| states.lock().ok())
        .and_then(|states| states.get(session_id).copied())
        .is_some_and(|last_activity| {
            now_ms.saturating_sub(last_activity) <= ACTIVE_TURN_RETENTION_MS
        })
}

#[derive(Clone, Serialize)]
pub(crate) struct SessionActivity {
    tool_id: &'static str,
    session_id: String,
    state: &'static str,
    observed_at_ms: u64,
    display_name: Option<String>,
    model_id: Option<String>,
}

fn tool_home(env_name: &str, default_dir: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(env_name)
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|path| std::path::PathBuf::from(path).join(default_dir))
        })
}

fn read_activity() -> Result<Vec<SessionActivity>, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_millis() as u64;
    let mut activities = other_activities(now);
    if let Ok(mut codex) = read_codex_activity(now) {
        activities.append(&mut codex);
    }
    Ok(activities)
}

#[cfg(test)]
mod tests {
    use super::{codex::turn_active, is_generating};

    #[test]
    fn latest_turn_event_controls_status() {
        let start = br#"{"type":"event_msg","payload":{"type":"task_started"}}"#;
        let complete = br#"{"type":"event_msg","payload":{"type":"task_complete"}}"#;
        assert_eq!(turn_active(start), Some(true));
        assert_eq!(
            turn_active(&[start.as_slice(), b"\n", complete.as_slice()].concat()),
            Some(false)
        );
        assert_eq!(
            turn_active(&[complete.as_slice(), b"\n", start.as_slice()].concat()),
            Some(true)
        );
        assert_eq!(turn_active(b"truncated\n{\"type\":\"other\"}"), None);
    }

    #[test]
    fn completed_turn_after_large_response_stays_completed() {
        let response =
            serde_json::json!({"type":"response_item","payload":{"text":"x".repeat(10_000)}});
        let tail = format!(
            "{}\n{{\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\"}}}}\n",
            response
        );
        assert_eq!(turn_active(tail.as_bytes()), Some(false));
        assert!(!is_generating(
            "completed-large-response",
            turn_active(tail.as_bytes()),
            1_000_000,
            1_000_000
        ));
    }

    #[test]
    fn recent_log_updates_hold_active_state_until_completion() {
        let key = "activity-state-retention-test";
        let now = 2_000_000_000_000;
        assert!(is_generating(key, None, now - 1_000, now));
        assert!(is_generating(key, None, now - 1_000, now + 5 * 60_000));
        assert!(!is_generating(
            key,
            Some(false),
            now + 5 * 60_000,
            now + 5 * 60_000
        ));
        assert!(!is_generating(
            key,
            None,
            now - 30 * 60_000,
            now + 30 * 60_000
        ));
    }
}

#[tauri::command]
pub(crate) async fn recent_session_activity() -> Result<Vec<SessionActivity>, String> {
    tauri::async_runtime::spawn_blocking(read_activity)
        .await
        .map_err(|e| e.to_string())?
}
