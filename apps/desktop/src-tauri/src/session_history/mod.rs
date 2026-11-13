mod codec;
mod locate;
mod storage;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::{fs, fs::File};

pub(super) const MAX_FILE: usize = 128 * 1024 * 1024;
pub(super) const MAX_LINE: usize = 4 * 1024 * 1024;
pub(super) const MAX_TEXT: usize = 256 * 1024;
pub(super) const PAGE_SIZE: usize = 30;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct History {
    pub(super) revision: String,
    pub(super) messages: Vec<Message>,
    pub(super) total: usize,
    pub(super) writable: bool,
    pub(super) part_count: usize,
    pub(super) part_index: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Message {
    pub(super) line: usize,
    pub(super) pointer: String,
    pub(super) role: &'static str,
    pub(super) text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Edit {
    pub(super) line: usize,
    pub(super) pointer: String,
    pub(super) text: Option<String>,
    #[serde(default)]
    pub(super) delete: bool,
}

pub(super) fn io(error: std::io::Error) -> String {
    format!("会话文件操作失败：{error}")
}

#[tauri::command]
pub(crate) async fn save_session_message(
    tool_id: String,
    session_id: String,
    expected_revision: String,
    edit: Edit,
    part: Option<usize>,
    offset: usize,
) -> Result<History, String> {
    tauri::async_runtime::spawn_blocking(move || {
        storage::mutate(
            &tool_id,
            &session_id,
            &expected_revision,
            Some(part.unwrap_or_default()),
            |path, original, bytes, count, last| {
                let updated = codec::rewrite(&tool_id, bytes, &edit)?;
                let mut history = codec::inspect(&tool_id, &updated, offset, true)?;
                if offset > 0 && history.messages.is_empty() {
                    history =
                        codec::inspect(&tool_id, &updated, offset.saturating_sub(PAGE_SIZE), true)?;
                }
                history.part_count = count;
                history.part_index = part.unwrap_or_default();
                history.revision = locate::file_revision(path, &history.revision);
                if updated == bytes {
                    return Ok(history);
                }
                let stamp = storage::stamp()?;
                let backup = storage::backup_path(path, stamp)?;
                let temporary = path.with_file_name(format!(".tokenscope-history-{stamp}.tmp"));
                storage::create_copy(&temporary, &updated, original)?;
                let result = (|| {
                    storage::create_copy(&backup, bytes, original)?;
                    if locate::part_path(
                        &locate::locate(&tool_id, &session_id)?.0,
                        part.unwrap_or_default(),
                    )? != path
                        || storage::read_bytes(&File::open(path).map_err(io)?)? != bytes
                    {
                        return Err("会话已变化，未写回".into());
                    }
                    crate::agent_customizations::replace_file(&temporary, path, true)
                        .map_err(io)?;
                    *last = Some((path.to_owned(), history.revision.clone()));
                    Ok(history)
                })();
                let _ = fs::remove_file(temporary);
                result
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn remove_source_session(
    tool_id: String,
    session_id: String,
    expected_revision: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        storage::mutate(
            &tool_id,
            &session_id,
            &expected_revision,
            None,
            |path, _, bytes, _, last| {
                let backup = storage::backup_path(path, storage::stamp()?)?;
                if locate::locate(&tool_id, &session_id)?.0.as_slice() != [path.to_owned()]
                    || storage::read_bytes(&File::open(path).map_err(io)?)? != bytes
                {
                    return Err("会话已变化，未删除".into());
                }
                storage::move_to_backup(path, &backup)?;
                *last = None;
                Ok(crate::agent_customizations::display_path(&backup))
            },
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn get_session_history(
    tool_id: String,
    session_id: String,
    part: Option<usize>,
    offset: usize,
) -> Result<History, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (paths, preferred) = if tool_id == "antigravity" {
            let home = crate::session_activity::tool_home("TOKENSCOPE_GEMINI_HOME", ".gemini")
                .ok_or("未找到 Antigravity 目录")?;
            (locate::antigravity_transcripts(&home, &session_id)?, None)
        } else if tool_id == "pi" {
            let root = crate::session_activity::pi_sessions_root().ok_or("未找到 Pi Agent 目录")?;
            locate::locate_in("pi", &session_id, &root, &[""])?
        } else {
            locate::locate(&tool_id, &session_id)?
        };
        let index = part.or(preferred).unwrap_or_else(|| {
            paths
                .iter()
                .position(|p| locate::part_path(std::slice::from_ref(p), 0).is_ok())
                .unwrap_or_default()
        });
        let path = locate::part_path(&paths, index)?;
        let file = File::open(path).map_err(io)?;
        let bytes = storage::read_bytes(&file)?;
        let mut history = codec::inspect(
            &tool_id,
            &bytes,
            offset,
            !matches!(tool_id.as_str(), "antigravity" | "pi"),
        )?;
        history.part_count = paths.len();
        history.part_index = index;
        history.revision = locate::file_revision(path, &history.revision);
        if history.writable {
            history.writable =
                storage::is_writable_for_cached_revision(path, &file, &history.revision)?;
        }
        Ok(history)
    })
    .await
    .map_err(|e| e.to_string())?
}
