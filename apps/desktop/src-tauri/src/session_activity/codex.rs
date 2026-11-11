use super::{is_generating, remembered_active, OutputSpeed, SessionActivity};
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

#[derive(Default)]
struct TurnObservation {
    active: Option<bool>,
    output_speed: Option<OutputSpeed>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Usage {
    input: u64,
    output: u64,
}

impl Usage {
    fn read(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            input: value.get("input_tokens")?.as_u64()?,
            output: value.get("output_tokens")?.as_u64()?,
        })
    }
}

struct MeasuredTurn {
    id: Option<String>,
    invalid: bool,
    output_tokens: Option<u64>,
}

fn completed_speed(row: &serde_json::Value, turn: MeasuredTurn) -> Option<OutputSpeed> {
    let payload = &row["payload"];
    if turn.invalid || turn.id.as_deref()? != payload["turn_id"].as_str()? {
        return None;
    }
    let output_tokens = turn.output_tokens.filter(|tokens| *tokens > 0)?;
    let generation_ms = payload["duration_ms"]
        .as_u64()?
        .checked_sub(payload["time_to_first_token_ms"].as_u64()?)?;
    if generation_ms == 0 {
        return None;
    }
    Some(OutputSpeed {
        output_tokens,
        generation_ms,
        completed_at_ms: chrono::DateTime::parse_from_rfc3339(row["timestamp"].as_str()?)
            .ok()?
            .timestamp_millis(),
    })
}

fn observe_turns(tail: &[u8]) -> TurnObservation {
    // Own the turn ID because each JSON row is dropped before the next one.
    let mut turn: Option<MeasuredTurn> = None;
    let mut observation = TurnObservation::default();
    let mut previous_usage = None;
    let mut last_terminal_end = None;
    let mut offset = 0usize;
    for line in tail.split(|b| *b == b'\n') {
        let line_end = offset.saturating_add(line.len());
        offset = line_end.saturating_add(1);
        let Ok(row) = serde_json::from_slice::<serde_json::Value>(line) else {
            if !line.iter().all(u8::is_ascii_whitespace) {
                if let Some(turn) = turn.as_mut() {
                    turn.invalid = true;
                }
            }
            continue;
        };
        let payload = &row["payload"];
        match (row["type"].as_str(), payload["type"].as_str()) {
            (Some("event_msg"), Some("task_started" | "turn_started")) => {
                observation.active = Some(true);
                last_terminal_end = None;
                turn = Some(MeasuredTurn {
                    id: payload["turn_id"].as_str().map(str::to_owned),
                    invalid: false,
                    output_tokens: None,
                });
            }
            (
                Some("event_msg"),
                Some("task_complete" | "turn_completed" | "task_aborted" | "turn_aborted"),
            ) => {
                observation.active = Some(false);
                last_terminal_end = Some(line_end);
                observation.output_speed = turn.take().and_then(|turn| {
                    if matches!(
                        payload["type"].as_str(),
                        Some("task_complete" | "turn_completed")
                    ) {
                        completed_speed(&row, turn)
                    } else {
                        None
                    }
                });
            }
            (Some("event_msg"), Some("token_count")) => {
                let info = &payload["info"];
                let Some(total) = Usage::read(&info["total_token_usage"]) else {
                    continue;
                };
                if previous_usage == Some(total) {
                    continue;
                }
                if let Some(turn) = turn.as_mut() {
                    // A second distinct usage event implies multiple requests.
                    if turn.output_tokens.is_some() {
                        turn.invalid = true;
                    } else {
                        let delta = previous_usage.map_or(Some(total.output), |previous: Usage| {
                            total.output.checked_sub(previous.output)
                        });
                        let last = Usage::read(&info["last_token_usage"]);
                        let matches_request = last.is_some_and(|last| {
                            delta == Some(last.output)
                                && previous_usage.map_or(total.input == last.input, |previous| {
                                    total.input.checked_sub(previous.input) == Some(last.input)
                                })
                        });
                        turn.invalid |= !matches_request;
                        turn.output_tokens = delta;
                    }
                } else {
                    // Unpaired usage after completion makes the last sample ambiguous.
                    observation.output_speed = None;
                }
                previous_usage = Some(total);
            }
            (Some("response_item"), Some(kind)) if !matches!(kind, "message" | "reasoning") => {
                if let Some(turn) = turn.as_mut() {
                    turn.invalid = true;
                }
            }
            (Some("event_msg"), Some("item_started" | "item_completed" | "context_compacted")) => {
                if let Some(turn) = turn.as_mut() {
                    turn.invalid |= !matches!(
                        payload["item"]["type"].as_str(),
                        Some("AgentMessage" | "UserMessage" | "Reasoning" | "Plan")
                    );
                }
            }
            _ => {}
        }
    }
    if observation.active == Some(false)
        && last_terminal_end.is_some_and(|end| tail.len().saturating_sub(end) > 8_192)
    {
        observation.active = None;
    }
    observation
}

#[cfg(test)]
pub(super) fn turn_active(tail: &[u8]) -> Option<bool> {
    observe_turns(tail).active
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
        let observation = observe_turns(&tail);
        let generating = is_generating(&session_id, observation.active, modified, now);
        result.push(SessionActivity {
            tool_id: "codex",
            output_speed: observation.output_speed,
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

#[cfg(test)]
mod output_speed_tests {
    use super::observe_turns;

    const BASE: &str = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":300,"output_tokens":500}}}}"#;
    const START: &str = r#"{"type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#;
    const USAGE: &str = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"output_tokens":620},"last_token_usage":{"input_tokens":700,"output_tokens":120,"reasoning_output_tokens":20}}}}"#;
    const END: &str = r#"{"timestamp":"2026-11-11T12:00:00Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t1","duration_ms":4100,"time_to_first_token_ms":1100}}"#;

    #[test]
    fn measures_one_request_and_deduplicates_repeated_usage() {
        let log = [BASE, START, USAGE, USAGE, END].join("\n");
        let speed = observe_turns(log.as_bytes()).output_speed.unwrap();
        assert_eq!((speed.output_tokens, speed.generation_ms), (120, 3000));
    }

    #[test]
    fn ambiguous_or_incomplete_turns_have_no_speed() {
        let tool = r#"{"type":"response_item","payload":{"type":"custom_tool_call"}}"#;
        let second = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1900,"output_tokens":650},"last_token_usage":{"input_tokens":900,"output_tokens":30}}}}"#;
        let invalid_duration = END.replace("4100", "1100");
        let different_turn = END.replace("t1", "t2");
        for rows in [
            vec![BASE, START, USAGE],
            vec![START, USAGE, END],
            vec![BASE, START, tool, USAGE, END],
            vec![BASE, START, USAGE, second, END],
            vec![BASE, START, USAGE, END, second],
            vec![BASE, START, USAGE, &invalid_duration],
            vec![BASE, START, USAGE, &different_turn],
        ] {
            assert!(observe_turns(rows.join("\n").as_bytes())
                .output_speed
                .is_none());
        }
    }
}
