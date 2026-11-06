use notify::{event::ModifyKind, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_core_domain::{session_key, stable_record_id};

const ACTIVE_TURN_RETENTION_MS: u64 = 20 * 60 * 1000;
const FRESH_FILE_ACTIVE_MS: u64 = 90 * 1000;
const MAX_CODEX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CODEX_INDEX_LINE_BYTES: usize = 1024 * 1024;
const MAX_CODEX_INDEX_ROWS: usize = 100_000;
static ACTIVE_TURNS: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
static OTHER_ACTIVITY: OnceLock<Mutex<HashMap<String, SessionActivity>>> = OnceLock::new();
static OTHER_WATCHER: OnceLock<Mutex<Option<RecommendedWatcher>>> = OnceLock::new();
static CODEX_THREAD_TITLES: OnceLock<Mutex<CodexThreadTitleCache>> = OnceLock::new();

#[derive(Default)]
struct CodexThreadTitleCache {
    path: Option<PathBuf>,
    file_len: u64,
    modified: Option<SystemTime>,
    names: Arc<HashMap<String, String>>,
}

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

fn active_roots() -> Vec<(std::path::PathBuf, &'static str, bool)> {
    let mut roots = Vec::new();
    if let Some(home) = tool_home("CLAUDE_CONFIG_DIR", ".claude") {
        let path = home.join("projects");
        if path.is_dir() {
            roots.push((path, "claude", true));
        }
    }
    if let Some(home) = tool_home("DSH_HOME", ".dsh") {
        let path = home.join("sessions");
        if path.is_dir() {
            roots.push((path, "deepseek-harness", true));
        }
    }
    if let Some(home) = tool_home("TOKENSCOPE_GEMINI_HOME", ".gemini") {
        for product in [
            "antigravity",
            "antigravity-ide",
            "antigravity-backup",
            "antigravity-cli",
        ] {
            let summary_db = home.join(product).join("conversation_summaries.db");
            if summary_db.is_file() {
                if let Some(path) = summary_db.parent() {
                    roots.push((path.to_path_buf(), "antigravity-summary", false));
                }
            }
        }
    }
    roots
}

fn dsh_session_id(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut line = Vec::new();
    reader.take(65_536).read_until(b'\n', &mut line).ok()?;
    let header: serde_json::Value = serde_json::from_slice(&line).ok()?;
    if header.get("type")?.as_str()? != "session" {
        return None;
    }
    let id = header.get("id")?.as_str()?;
    if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
        return None;
    }
    session_key("deepseek-harness", id.as_bytes()).ok()
}

fn activity_from_path(
    path: &Path,
    roots: &[(std::path::PathBuf, &'static str, bool)],
    now: u64,
) -> Option<SessionActivity> {
    let mut source_path = path.to_path_buf();
    if let Some(raw) = path.file_name().and_then(|name| name.to_str()) {
        if let Some(database) = raw.strip_suffix("-wal") {
            source_path.set_file_name(database);
        }
    }
    let (root, tool_id, _) = roots
        .iter()
        .find(|(root, tool, _)| *tool != "antigravity-summary" && source_path.starts_with(root))?;
    let metadata = fs::symlink_metadata(&source_path).ok()?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return None;
    }
    let session_id = match *tool_id {
        "claude" => {
            if source_path.extension().is_none_or(|e| e != "jsonl") {
                return None;
            }
            let relative = source_path.strip_prefix(root).ok()?.to_str()?;
            session_key("claude", relative.as_bytes()).ok()?
        }
        "deepseek-harness" => {
            if source_path.extension().is_none_or(|e| e != "jsonl") {
                return None;
            }
            dsh_session_id(&source_path)?
        }
        _ => return None,
    };
    Some(SessionActivity {
        tool_id,
        session_id,
        state: "recent",
        observed_at_ms: now,
        display_name: None,
        model_id: None,
    })
}

fn read_antigravity_active(db: &Path, now: u64) -> Option<Vec<SessionActivity>> {
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(std::time::Duration::from_millis(100))
        .ok()?;
    let mut stmt = conn
        .prepare(
            "SELECT conversation_id,title FROM conversation_summaries \
         WHERE not_fully_idle=1 AND killed=0 AND COALESCE(status,'')<>'CASCADE_RUN_STATUS_IDLE' \
         ORDER BY last_modified_time DESC LIMIT 32",
        )
        .ok()?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })
        .ok()?;
    Some(
        rows.flatten()
            .filter_map(|(id, title)| {
                if id.is_empty() || id.len() > 200 || id.chars().any(char::is_control) {
                    return None;
                }
                let session_id =
                    stable_record_id("antigravity", b"conversation", id.as_bytes()).ok()?;
                Some(SessionActivity {
                    tool_id: "antigravity",
                    session_id,
                    state: "generating",
                    observed_at_ms: now,
                    display_name: title.map(|name| name.chars().take(200).collect()),
                    model_id: None,
                })
            })
            .collect(),
    )
}

fn ensure_other_watchers() {
    let Some(mut lock) = OTHER_WATCHER.get_or_init(|| Mutex::new(None)).lock().ok() else {
        return;
    };
    if lock.is_some() {
        return;
    }
    let roots = active_roots()
        .into_iter()
        .filter_map(|(path, tool, recursive)| {
            path.canonicalize().ok().map(|path| (path, tool, recursive))
        })
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return;
    }
    let watcher_roots = roots.clone();
    let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else {
            return;
        };
        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_))
        ) {
            return;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default();
        let states = OTHER_ACTIVITY.get_or_init(|| Mutex::new(HashMap::new()));
        if let Ok(mut states) = states.lock() {
            states.retain(|_, activity| now.saturating_sub(activity.observed_at_ms) <= 120_000);
            for path in event.paths {
                let summary_root = watcher_roots.iter().find_map(|(root, tool, _)| {
                    let name = path.file_name()?.to_str()?;
                    (*tool == "antigravity-summary"
                        && root == path.parent()?
                        && (name == "conversation_summaries.db"
                            || name == "conversation_summaries.db-wal"))
                        .then_some(root)
                });
                if let Some(root) = summary_root {
                    let db = root.join("conversation_summaries.db");
                    if let Some(active) = read_antigravity_active(&db, now) {
                        states.retain(|_, activity| activity.tool_id != "antigravity");
                        for activity in active {
                            states.insert(activity.session_id.clone(), activity);
                        }
                    }
                } else if let Some(activity) = activity_from_path(&path, &watcher_roots, now) {
                    states.insert(activity.session_id.clone(), activity);
                }
            }
        }
    });
    let Ok(mut watcher) = watcher else {
        return;
    };
    let mut watched = 0usize;
    for (path, tool, recursive) in &roots {
        let mode = if *recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        if watcher.watch(path, mode).is_ok() {
            watched += 1;
        }
        if *tool == "antigravity-summary" {
            if let Some(active) = read_antigravity_active(
                &path.join("conversation_summaries.db"),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or_default(),
            ) {
                if let Ok(mut states) = OTHER_ACTIVITY
                    .get_or_init(|| Mutex::new(HashMap::new()))
                    .lock()
                {
                    for activity in active {
                        states.insert(activity.session_id.clone(), activity);
                    }
                }
            }
        }
    }
    if watched > 0 {
        *lock = Some(watcher);
    }
}

fn codex_thread_titles(home: &Path) -> Arc<HashMap<String, String>> {
    let path = home.join("session_index.jsonl");
    let before = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        _ => return Arc::new(HashMap::new()),
    };
    if before.len() > MAX_CODEX_INDEX_BYTES {
        return Arc::new(HashMap::new());
    }
    let modified = before.modified().ok();
    let cache = CODEX_THREAD_TITLES.get_or_init(|| Mutex::new(CodexThreadTitleCache::default()));
    let Ok(mut cache) = cache.lock() else {
        return Arc::new(HashMap::new());
    };
    if cache.path.as_deref() == Some(path.as_path())
        && cache.file_len == before.len()
        && cache.modified == modified
    {
        return Arc::clone(&cache.names);
    }
    let fallback = if cache.path.as_deref() == Some(path.as_path()) {
        Arc::clone(&cache.names)
    } else {
        Arc::new(HashMap::new())
    };

    let file = match File::open(&path) {
        Ok(file) => file,
        Err(_) => return fallback,
    };
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut names = HashMap::new();
    let mut rows = 0usize;
    loop {
        line.clear();
        let bytes = match reader.read_until(b'\n', &mut line) {
            Ok(bytes) => bytes,
            Err(_) => return fallback,
        };
        if bytes == 0 {
            break;
        }
        rows = rows.saturating_add(1);
        if rows > MAX_CODEX_INDEX_ROWS || line.len() > MAX_CODEX_INDEX_LINE_BYTES {
            return fallback;
        }
        let has_newline = line.last() == Some(&b'\n');
        let value: serde_json::Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) if !has_newline => break,
            Err(_) => continue,
        };
        let Some(id) = value.get("id").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !(8..=128).contains(&id.len())
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            continue;
        }
        let Some(raw_name) = value.get("thread_name").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let normalized = raw_name.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() || normalized.chars().any(char::is_control) {
            continue;
        }
        names.insert(id.to_owned(), normalized.chars().take(200).collect());
    }

    let after = match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        _ => return fallback,
    };
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return fallback;
    }
    cache.path = Some(path);
    cache.file_len = after.len();
    cache.modified = after.modified().ok();
    cache.names = Arc::new(names);
    Arc::clone(&cache.names)
}

fn other_activities(now: u64) -> Vec<SessionActivity> {
    ensure_other_watchers();
    // File events are a fast path; quiet generations still need a heartbeat.
    static LAST_CHECK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let last = LAST_CHECK.load(std::sync::atomic::Ordering::Relaxed);
    if now.saturating_sub(last) >= 30_000
        && LAST_CHECK
            .compare_exchange(
                last,
                now,
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_ok()
    {
        let mut active = Vec::new();
        let mut succeeded = false;
        for (root, tool, _) in active_roots() {
            if tool == "antigravity-summary" {
                if let Some(rows) =
                    read_antigravity_active(&root.join("conversation_summaries.db"), now)
                {
                    active.extend(rows);
                    succeeded = true;
                }
            }
        }
        if succeeded {
            if let Ok(mut states) = OTHER_ACTIVITY
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
            {
                states.retain(|_, a| a.tool_id != "antigravity");
                for a in active {
                    states.insert(a.session_id.clone(), a);
                }
            }
        }
    }
    OTHER_ACTIVITY
        .get()
        .and_then(|states| states.lock().ok())
        .map(|mut states| {
            states.retain(|_, activity| now.saturating_sub(activity.observed_at_ms) <= 120_000);
            states.values().cloned().collect()
        })
        .unwrap_or_default()
}

fn turn_active(tail: &[u8]) -> Option<bool> {
    let mut turn = None;
    let mut last_terminal_end = None;
    let mut offset = 0usize;
    for line in tail.split(|b| *b == b'\n') {
        let line_end = offset.saturating_add(line.len());
        offset = line_end.saturating_add(1);
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) {
            if value.get("type").and_then(|v| v.as_str()) != Some("event_msg") {
                continue;
            }
            match value
                .get("payload")
                .and_then(|v| v.get("type"))
                .and_then(|v| v.as_str())
            {
                Some("task_started") => {
                    turn = Some(true);
                    last_terminal_end = None;
                }
                Some("task_complete" | "task_aborted" | "turn_aborted") => {
                    turn = Some(false);
                    last_terminal_end = Some(line_end);
                }
                _ => {}
            }
        }
    }
    if turn == Some(false) && tail.len().saturating_sub(last_terminal_end?) > 8_192 {
        return None;
    }
    turn
}

fn read_codex_activity(now: u64) -> Result<Vec<SessionActivity>, String> {
    let Some(home) = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|p| std::path::PathBuf::from(p).join(".codex"))
        })
    else {
        return Ok(Vec::new());
    };
    let root = match home.join("sessions").canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(Vec::new()),
    };
    let db = home.join("state_5.sqlite");
    if !db.is_file() {
        return Ok(Vec::new());
    }
    let conn = match Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(error) => return Err(error.to_string()),
    };
    conn.busy_timeout(std::time::Duration::from_millis(250))
        .map_err(|e| e.to_string())?;
    let thread_titles = codex_thread_titles(&home);
    let mut stmt = conn.prepare("SELECT id,rollout_path,title,model FROM threads WHERE archived=0 ORDER BY updated_at DESC LIMIT 128").map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for (index, row) in rows.flatten().enumerate() {
        let (id, raw_path, display_name, model_id) = row;
        let Ok(session_id) = session_key("codex", id.as_bytes()) else {
            continue;
        };
        let path = match Path::new(&raw_path).canonicalize() {
            Ok(p) if p.starts_with(&root) && p.extension().is_some_and(|v| v == "jsonl") => p,
            _ => continue,
        };
        let metadata = match fs::metadata(&path) {
            Ok(m) if m.is_file() => m,
            _ => continue,
        };
        let modified = match metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        {
            Some(t) => t.as_millis() as u64,
            None => continue,
        };
        let age = now.saturating_sub(modified);
        let remembered = remembered_active(&session_id, now);
        let initial_recent_candidate = index < 4 && age <= 5 * 60_000;
        if modified > now.saturating_add(5_000)
            || (age > 90_000 && !remembered && !initial_recent_candidate)
        {
            continue;
        }
        let mut file = match fs::File::open(&path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let tail_limit = if remembered || age <= 90_000 {
            65_536
        } else {
            2 * 1024 * 1024
        };
        let offset = metadata.len().saturating_sub(tail_limit);
        if file.seek(SeekFrom::Start(offset)).is_err() {
            continue;
        }
        let mut tail = Vec::new();
        if file.take(tail_limit).read_to_end(&mut tail).is_err() {
            continue;
        }
        let generating = is_generating(&session_id, turn_active(&tail), modified, now);
        result.push(SessionActivity {
            tool_id: "codex",
            session_id,
            state: if generating { "generating" } else { "recent" },
            observed_at_ms: modified,
            display_name: thread_titles
                .get(&id)
                .cloned()
                .or(display_name)
                .map(|name| name.chars().take(200).collect()),
            model_id: model_id.map(|name| name.chars().take(128).collect()),
        });
    }
    Ok(result)
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
    use super::{is_generating, turn_active};

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
