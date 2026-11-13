use crate::{models::ProjectMetadata, now_ms, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::Path,
};
use tokenscope_core_domain::{project_key, stable_record_id};
use tokenscope_local_store::LocalStore;

const SUMMARY_PREFIXES: [&str; 4] = [
    "antigravity",
    "antigravity-ide",
    "antigravity-backup",
    "antigravity-cli",
];
const MAX_SUMMARY_ROWS: usize = 5000;
const MAX_TRANSCRIPT_LINES: usize = 64;
const MAX_TRANSCRIPT_BYTES: usize = 2 * 1024 * 1024;
const MAX_TRANSCRIPT_LINE_BYTES: usize = 512 * 1024;
const MAX_TITLE_CHARS: usize = 80;

pub(super) fn workspace_project_id(raw: Option<&str>) -> Option<String> {
    let value: Value = serde_json::from_str(raw?).ok()?;
    let mut uris = value
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|uri| !uri.is_empty() && uri.len() <= 4096 && !uri.chars().any(char::is_control))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    uris.sort();
    uris.dedup();
    if uris.is_empty() {
        return None;
    }
    let mut identity = Vec::new();
    for uri in uris {
        let bytes = uri.as_bytes();
        identity.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
        identity.extend_from_slice(bytes);
    }
    project_key("antigravity", &identity).ok()
}

fn safe_project_name(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
    {
        return None;
    }
    Some(name.to_owned())
}

fn decode_uri_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            decoded.push((hex(high)? << 4) | hex(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn uri_project_name(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 4096 || raw.chars().any(char::is_control) {
        return None;
    }
    let uri = raw.split(['?', '#']).next()?;
    let path = if let Some(path) = uri.strip_prefix("file://") {
        path
    } else if let Some((_, rest)) = uri.split_once("://") {
        rest.split_once('/')?.1
    } else {
        uri
    };
    let component = path
        .rsplit(['/', '\\'])
        .find(|component| !component.is_empty())?;
    safe_project_name(&decode_uri_component(component)?)
}

fn workspace_project_name(raw: Option<&str>) -> Option<String> {
    let value: Value = serde_json::from_str(raw?).ok()?;
    let mut names = value
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(uri_project_name)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    let joined = names.join(" + ");
    let joined: String = joined.chars().take(120).collect();
    safe_project_name(&joined)
}

fn is_valid_conversation_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control)
}

fn is_safe_path_segment(segment: &str) -> bool {
    is_valid_conversation_id(segment)
        && segment != "."
        && segment != ".."
        && !segment.contains('/')
        && !segment.contains('\\')
}

fn open_summaries_db(gemini_home: &Path, prefix: &str) -> Option<Connection> {
    let prefix_dir = gemini_home.join(prefix);
    let prefix_meta = fs::symlink_metadata(&prefix_dir).ok()?;
    if prefix_meta.file_type().is_symlink() || !prefix_meta.is_dir() {
        return None;
    }
    let path = prefix_dir.join("conversation_summaries.db");
    let meta = fs::symlink_metadata(&path).ok()?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
    conn.busy_timeout(std::time::Duration::from_secs(2)).ok()?;
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversation_summaries')",
        [],
        |row| row.get::<_, bool>(0),
    );
    matches!(exists, Ok(true)).then_some(conn)
}

fn summary_columns(conn: &Connection) -> HashSet<String> {
    let mut cols = HashSet::new();
    let Ok(mut stmt) = conn.prepare("PRAGMA table_info(conversation_summaries)") else {
        return cols;
    };
    let Ok(mut rows) = stmt.query([]) else {
        return cols;
    };
    while let Ok(Some(row)) = rows.next() {
        if let Ok(name) = row.get::<_, String>(1) {
            cols.insert(name);
        }
    }
    cols
}

pub(super) fn project_index(gemini_home: &Path) -> HashMap<(String, String), ProjectMetadata> {
    let mut projects = HashMap::new();
    for prefix in SUMMARY_PREFIXES {
        let Some(conn) = open_summaries_db(gemini_home, prefix) else {
            continue;
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT conversation_id,project_id,workspace_uris FROM conversation_summaries LIMIT 5000",
        ) else {
            continue;
        };
        let Ok(mut rows) = stmt.query([]) else {
            continue;
        };
        loop {
            let row = match rows.next() {
                Ok(Some(row)) => row,
                Ok(None) | Err(_) => break,
            };
            let id = match row.get::<_, String>(0) {
                Ok(id)
                    if !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control) =>
                {
                    id
                }
                _ => continue,
            };
            let project = row.get::<_, Option<String>>(1).ok().flatten();
            let workspace_uris = row.get::<_, Option<String>>(2).ok().flatten();
            let project_id = project
                .as_deref()
                .filter(|value| !value.trim().is_empty() && value.len() <= 4096)
                .and_then(|value| project_key("antigravity", value.trim().as_bytes()).ok())
                .or_else(|| workspace_project_id(workspace_uris.as_deref()));
            if let Some(project_id) = project_id {
                projects.insert(
                    (prefix.to_owned(), id),
                    ProjectMetadata {
                        project_id,
                        display_name: workspace_project_name(workspace_uris.as_deref()),
                    },
                );
            }
        }
    }
    projects
}

#[derive(Default)]
struct SummaryCandidate {
    in_summaries: bool,
    title: Option<String>,
    preview: Option<String>,
    parent_id: Option<String>,
    agent_name: Option<String>,
}

fn clean_summary_text(raw: &str, reject_markup_prefix: bool) -> Option<String> {
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() || joined.chars().any(char::is_control) {
        return None;
    }
    if reject_markup_prefix && matches!(joined.chars().next(), Some('<' | '{')) {
        return None;
    }
    Some(joined.chars().take(MAX_TITLE_CHARS).collect())
}

fn clean_agent_label(raw: &str) -> Option<String> {
    let joined = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if joined.is_empty() || joined.chars().any(char::is_control) {
        return None;
    }
    Some(joined.chars().take(32).collect())
}

fn clean_user_request_text(raw: &str) -> Option<String> {
    let without_meta = raw.split("<ADDITIONAL_METADATA>").next().unwrap_or(raw);
    let body = if let Some((_, after_open)) = without_meta.split_once("<USER_REQUEST>") {
        after_open
            .split("</USER_REQUEST>")
            .next()
            .unwrap_or(after_open)
    } else {
        without_meta
    };
    let body = if body
        .trim_start()
        .starts_with("This is a side question from the user.")
    {
        body.split_once("Here is the question:")
            .map(|(_, question)| question)
            .unwrap_or(body)
    } else {
        body
    };
    clean_summary_text(body, true)
}

fn safe_non_symlink_path(root: &Path, segments: &[&str]) -> Option<std::path::PathBuf> {
    let mut current = root.to_path_buf();
    let last_idx = segments.len().checked_sub(1)?;
    for (idx, segment) in segments.iter().enumerate() {
        if !is_safe_path_segment(segment) {
            return None;
        }
        current.push(segment);
        let meta = fs::symlink_metadata(&current).ok()?;
        if meta.file_type().is_symlink() {
            return None;
        }
        let is_last = idx == last_idx;
        if (is_last && !meta.is_file()) || (!is_last && !meta.is_dir()) {
            return None;
        }
    }
    Some(current)
}

fn read_transcript_title(gemini_home: &Path, id: &str) -> Option<String> {
    if !is_safe_path_segment(id) {
        return None;
    }
    for prefix in SUMMARY_PREFIXES {
        let Some(path) = safe_non_symlink_path(
            gemini_home,
            &[
                prefix,
                "brain",
                id,
                ".system_generated",
                "logs",
                "transcript.jsonl",
            ],
        ) else {
            continue;
        };
        let Ok(file) = File::open(&path) else {
            continue;
        };
        let mut reader = BufReader::new(file.take(MAX_TRANSCRIPT_BYTES as u64));
        let mut buf = Vec::new();
        for _ in 0..MAX_TRANSCRIPT_LINES {
            buf.clear();
            let Ok(read) = reader.read_until(b'\n', &mut buf) else {
                break;
            };
            if read == 0 {
                break;
            }
            if buf.len() > MAX_TRANSCRIPT_LINE_BYTES {
                continue;
            }
            let Ok(value) = serde_json::from_slice::<Value>(&buf) else {
                continue;
            };
            if value.get("type").and_then(Value::as_str) != Some("USER_INPUT") {
                continue;
            }
            if value.get("source").and_then(Value::as_str) == Some("SYSTEM") {
                continue;
            }
            if let Some(content) = value.get("content").and_then(Value::as_str) {
                if let Some(title) = clean_user_request_text(content) {
                    return Some(title);
                }
            }
        }
    }
    None
}

fn collect_brain_conversation_ids(
    gemini_home: &Path,
    candidates: &mut BTreeMap<String, SummaryCandidate>,
) {
    for prefix in SUMMARY_PREFIXES {
        let Some(brain_dir) = safe_non_symlink_path(gemini_home, &[prefix, "brain"]) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(&brain_dir) else {
            continue;
        };
        for entry in entries.flatten().take(MAX_SUMMARY_ROWS) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            if let Some(id) = entry.file_name().to_str() {
                if is_safe_path_segment(id) {
                    candidates.entry(id.to_owned()).or_default();
                }
            }
        }
    }
}

fn resolve_subagent_title(
    cand: &SummaryCandidate,
    resolved: &HashMap<String, String>,
) -> Option<String> {
    let parent_title = cand
        .parent_id
        .as_deref()
        .and_then(|parent_id| resolved.get(parent_id))
        .map(String::as_str);
    let agent = cand.agent_name.as_deref();
    match (agent, parent_title) {
        (Some(agent), Some(parent)) => clean_summary_text(&format!("[{agent}] {parent}"), false),
        (None, Some(parent)) => clean_summary_text(&format!("[子代理] {parent}"), false),
        (Some(agent), None) => clean_summary_text(&format!("子代理 ({agent})"), false),
        (None, None) if cand.parent_id.is_some() => Some("[子代理]".to_owned()),
        (None, None) => None,
    }
}

/// Antigravity keeps conversation titles in a separate summaries database,
/// with fallbacks to preview text, local transcript requests, and parent
/// subagent titles.
pub fn import_titles(gemini_home: &Path, store: &mut LocalStore) -> Result<usize> {
    import_titles_for_conversations(gemini_home, store, &[])
}

pub(super) fn import_titles_for_conversations(
    gemini_home: &Path,
    store: &mut LocalStore,
    extra_conversation_ids: &[&str],
) -> Result<usize> {
    let mut candidates: BTreeMap<String, SummaryCandidate> = BTreeMap::new();
    for prefix in SUMMARY_PREFIXES {
        let Some(conn) = open_summaries_db(gemini_home, prefix) else {
            continue;
        };
        let cols = summary_columns(&conn);
        if !cols.contains("conversation_id") {
            continue;
        }
        let select_col = |name: &'static str| if cols.contains(name) { name } else { "NULL" };
        let order_by = if cols.contains("last_modified_time") {
            " ORDER BY last_modified_time DESC"
        } else {
            ""
        };
        let sql = format!(
            "SELECT conversation_id, {}, {}, {}, {} FROM conversation_summaries{} LIMIT {}",
            select_col("title"),
            select_col("preview"),
            select_col("parent_conversation_id"),
            select_col("agent_name"),
            order_by,
            MAX_SUMMARY_ROWS
        );
        let Ok(mut stmt) = conn.prepare(&sql) else {
            continue;
        };
        let Ok(mut rows) = stmt.query([]) else {
            continue;
        };
        loop {
            let row = match rows.next() {
                Ok(Some(row)) => row,
                Ok(None) | Err(_) => break,
            };
            let Ok(Some(id)) = row.get::<_, Option<String>>(0) else {
                continue;
            };
            if !is_valid_conversation_id(&id) {
                continue;
            }
            let entry = candidates.entry(id).or_default();
            entry.in_summaries = true;
            if entry.title.is_none() {
                if let Some(raw) = row.get::<_, Option<String>>(1).ok().flatten() {
                    entry.title = clean_summary_text(&raw, false);
                }
            }
            if entry.preview.is_none() {
                if let Some(raw) = row.get::<_, Option<String>>(2).ok().flatten() {
                    entry.preview = clean_summary_text(&raw, false);
                }
            }
            if entry.parent_id.is_none() {
                if let Some(parent_id) = row.get::<_, Option<String>>(3).ok().flatten() {
                    if is_valid_conversation_id(&parent_id) {
                        entry.parent_id = Some(parent_id);
                    }
                }
            }
            if entry.agent_name.is_none() {
                if let Some(agent_name) = row.get::<_, Option<String>>(4).ok().flatten() {
                    entry.agent_name = clean_agent_label(&agent_name);
                }
            }
        }
    }

    for id in extra_conversation_ids {
        if is_valid_conversation_id(id) {
            candidates.entry((*id).to_owned()).or_default();
        }
    }
    collect_brain_conversation_ids(gemini_home, &mut candidates);

    let mut resolved: HashMap<String, String> = HashMap::new();
    for (id, cand) in &candidates {
        if let Some(title) = cand
            .title
            .clone()
            .or_else(|| cand.preview.clone())
            .or_else(|| read_transcript_title(gemini_home, id))
        {
            resolved.insert(id.clone(), title);
        }
    }

    // Resolve subagents whose title/preview are empty via their parent conversation
    // title (with bounded hops in case of nested subagents or cycles).
    for _ in 0..5 {
        let mut progressed = false;
        let pending: Vec<(String, String)> = candidates
            .iter()
            .filter(|(id, cand)| {
                !resolved.contains_key(*id)
                    && (cand.parent_id.is_some() || cand.agent_name.is_some())
            })
            .filter_map(|(id, cand)| {
                resolve_subagent_title(cand, &resolved).map(|title| (id.clone(), title))
            })
            .collect();
        for (id, title) in pending {
            if let std::collections::hash_map::Entry::Vacant(entry) = resolved.entry(id) {
                entry.insert(title);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    let mut imported = 0usize;
    for (id, cand) in &candidates {
        let key = stable_record_id("antigravity", b"conversation", id.as_bytes())?;
        if let Some(name) = resolved.get(id) {
            if store
                .upsert_session_name(&key, "antigravity", name, now_ms()?)
                .is_ok()
            {
                imported += 1;
            }
        } else if cand.in_summaries {
            store.clear_session_name(&key, "antigravity")?;
        }
    }
    Ok(imported)
}
