use super::{is_generating, remembered_active, SessionActivity};
use rusqlite::{Connection, OpenFlags};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_core_domain::session_key;

const MAX_CODEX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CODEX_INDEX_LINE_BYTES: usize = 1024 * 1024;
const MAX_CODEX_INDEX_ROWS: usize = 100_000;

static CODEX_THREAD_TITLES: OnceLock<Mutex<CodexThreadTitleCache>> = OnceLock::new();

#[derive(Default)]
struct CodexThreadTitleCache {
    path: Option<PathBuf>,
    file_len: u64,
    modified: Option<SystemTime>,
    names: Arc<HashMap<String, String>>,
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

pub(super) fn turn_active(tail: &[u8]) -> Option<bool> {
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

pub(super) fn read_codex_activity(now: u64) -> Result<Vec<SessionActivity>, String> {
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
