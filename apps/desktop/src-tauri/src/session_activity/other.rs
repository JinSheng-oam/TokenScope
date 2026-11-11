use super::{tool_home, SessionActivity};
use notify::{event::ModifyKind, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::Path,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_core_domain::{session_key, stable_record_id};

static OTHER_ACTIVITY: OnceLock<Mutex<HashMap<String, SessionActivity>>> = OnceLock::new();
static OTHER_WATCHER: OnceLock<Mutex<Option<RecommendedWatcher>>> = OnceLock::new();

fn active_roots() -> Vec<(std::path::PathBuf, &'static str, bool)> {
    let mut roots = Vec::new();
    if let Some(home) = tool_home("CLAUDE_CONFIG_DIR", ".claude") {
        let path = home.join("projects");
        if path.is_dir() {
            roots.push((path, "claude", true));
        }
    }
    if let Some(path) = super::pi_sessions_root() {
        if path.is_dir() {
            roots.push((path, "pi", true));
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

fn jsonl_session_id(tool_id: &str, path: &Path) -> Option<String> {
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
    session_key(tool_id, id.as_bytes()).ok()
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
        "deepseek-harness" | "pi" => {
            if source_path.extension().is_none_or(|e| e != "jsonl") {
                return None;
            }
            jsonl_session_id(tool_id, &source_path)?
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
        output_speed: None,
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
                    output_speed: None,
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

pub(super) fn other_activities(now: u64) -> Vec<SessionActivity> {
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
