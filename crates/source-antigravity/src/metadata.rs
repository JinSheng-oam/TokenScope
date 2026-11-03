use crate::{models::ProjectMetadata, now_ms, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{collections::HashMap, path::Path};
use tokenscope_core_domain::{project_key, stable_record_id};
use tokenscope_local_store::LocalStore;

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

pub(super) fn project_index(gemini_home: &Path) -> HashMap<(String, String), ProjectMetadata> {
    let mut projects = HashMap::new();
    for prefix in [
        "antigravity",
        "antigravity-ide",
        "antigravity-backup",
        "antigravity-cli",
    ] {
        let path = gemini_home.join(prefix).join("conversation_summaries.db");
        if !path.is_file() {
            continue;
        }
        let Ok(conn) = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) else {
            continue;
        };
        if conn
            .busy_timeout(std::time::Duration::from_secs(2))
            .is_err()
        {
            continue;
        }
        let exists = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversation_summaries')",
            [],
            |row| row.get::<_, bool>(0),
        );
        if !matches!(exists, Ok(true)) {
            continue;
        }
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

/// Antigravity keeps conversation titles in a separate summaries database.
/// They are imported as display-only labels keyed by the same conversation
/// identity the usage rows use.
pub fn import_titles(gemini_home: &Path, store: &mut LocalStore) -> Result<usize> {
    let mut imported = 0usize;
    for prefix in [
        "antigravity",
        "antigravity-ide",
        "antigravity-backup",
        "antigravity-cli",
    ] {
        let path = gemini_home.join(prefix).join("conversation_summaries.db");
        if !path.is_file() {
            continue;
        }
        let conn = match Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(conn) => conn,
            Err(_) => continue,
        };
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='conversation_summaries')",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            continue;
        }
        let mut stmt =
            conn.prepare("SELECT conversation_id,title FROM conversation_summaries LIMIT 5000")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            if id.is_empty() || id.len() > 200 {
                continue;
            }
            let key = stable_record_id("antigravity", b"conversation", id.as_bytes())?;
            if title.trim().is_empty() {
                store.clear_session_name(&key, "antigravity")?;
                continue;
            }
            let name = title.split_whitespace().collect::<Vec<_>>().join(" ");
            if name.is_empty() || name.chars().count() > 200 {
                store.clear_session_name(&key, "antigravity")?;
                continue;
            }
            let name: String = name.chars().take(80).collect();
            if store
                .upsert_session_name(&key, "antigravity", &name, now_ms()?)
                .is_ok()
            {
                imported += 1;
            }
        }
    }
    Ok(imported)
}
