use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokenscope_core_domain::session_key;

const MAX_FILE: usize = 128 * 1024 * 1024;
const MAX_LINE: usize = 4 * 1024 * 1024;
const MAX_TEXT: usize = 256 * 1024;
const PAGE_SIZE: usize = 30;
static LAST_SAVE: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct History {
    revision: String,
    messages: Vec<Message>,
    total: usize,
    writable: bool,
    part_count: usize,
    part_index: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    line: usize,
    pointer: String,
    role: &'static str,
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Edit {
    line: usize,
    pointer: String,
    text: Option<String>,
    #[serde(default)]
    delete: bool,
}

fn io(error: std::io::Error) -> String {
    format!("会话文件操作失败：{error}")
}

fn locate(tool: &str, id: &str) -> Result<(Vec<PathBuf>, Option<usize>), String> {
    let (env, fallback, roots): (_, _, &[_]) = match tool {
        "codex" => ("CODEX_HOME", ".codex", &["sessions", "archived_sessions"]),
        "claude" => ("CLAUDE_CONFIG_DIR", ".claude", &["projects"]),
        "deepseek-harness" => ("DSH_HOME", ".dsh", &["sessions"]),
        _ => return Err("此 Agent 暂不支持历史会话编辑".into()),
    };
    let home = crate::session_activity::tool_home(env, fallback).ok_or("未找到 Agent 目录")?;
    locate_in(tool, id, &home, roots)
}

fn locate_in(
    tool: &str,
    id: &str,
    home: &Path,
    roots: &[&str],
) -> Result<(Vec<PathBuf>, Option<usize>), String> {
    let mut candidates = Vec::new();
    for name in roots {
        let root = home.join(name);
        if !root.is_dir() || crate::agent_customizations::is_link(&root) {
            continue;
        }
        let mut pending = vec![root.clone()];
        let mut count = 0;
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir).map_err(io)? {
                let entry = entry.map_err(io)?;
                count += 1;
                if count > 100_000 {
                    return Err("Agent 目录过大，无法安全定位会话".into());
                }
                let path = entry.path();
                let ty = entry.file_type().map_err(io)?;
                if crate::agent_customizations::is_link(&path) {
                    continue;
                }
                if ty.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !ty.is_file() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                    continue;
                };
                let Some(stem) = name.strip_suffix(".jsonl").or_else(|| {
                    (tool == "codex")
                        .then(|| name.strip_suffix(".jsonl.zst"))
                        .flatten()
                }) else {
                    continue;
                };
                let native = if tool == "claude" {
                    path.strip_prefix(&root)
                        .ok()
                        .and_then(Path::to_str)
                        .map(str::to_owned)
                } else if tool == "codex" {
                    if name.ends_with(".zst") {
                        stem.rsplit_once('T')
                            .and_then(|(_, tail)| tail.get(9..))
                            .and_then(|ids| ids.split('_').next())
                            .map(str::to_owned)
                    } else {
                        header_id(&path, tool)
                    }
                } else {
                    header_id(&path, tool)
                };
                if let Some(native) = native {
                    candidates.push((native, path));
                }
            }
        }
    }
    let mut preferred = None;
    let native = candidates
        .iter()
        .find(|(native, _)| matches_session(tool, native, id))
        .map(|(native, _)| native.clone())
        .or_else(|| {
            if tool != "codex" {
                return None;
            }
            // Older collectors exposed a path-derived key for each continuation.
            candidates.iter().find_map(|(native, path)| {
                let legacy = legacy_codex_identity(native, path)?;
                if !matches_session(tool, &legacy, id) {
                    return None;
                }
                preferred = Some(path.clone());
                Some(native.clone())
            })
        });
    let mut matches: Vec<_> = candidates
        .into_iter()
        .filter_map(|(candidate, path)| (Some(&candidate) == native.as_ref()).then_some(path))
        .collect();
    if matches.is_empty() {
        return Err("未找到对应的本地会话文件；文件可能已移走或不在当前 Agent 目录中。".into());
    }
    if tool != "codex" && matches.len() > 1 {
        return Err("此会话对应多个源文件，暂不支持改写".into());
    }
    matches.sort_by(|a, b| a.file_name().cmp(&b.file_name()).then_with(|| a.cmp(b)));
    let index = preferred.and_then(|path| matches.iter().position(|p| *p == path));
    Ok((matches, index))
}

fn matches_session(tool: &str, native: &str, id: &str) -> bool {
    session_key(tool, native.as_bytes()).ok().as_deref() == Some(id)
}

fn legacy_codex_identity(native: &str, path: &Path) -> Option<String> {
    let canonical = path.canonicalize().ok()?;
    let digest = format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    );
    Some(format!("{native}#{}", &digest[..8]))
}

fn antigravity_transcripts(home: &Path, id: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for product in [
        "antigravity",
        "antigravity-cli",
        "antigravity-ide",
        "antigravity-backup",
    ] {
        let root = home.join(product).join("brain");
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(e)),
        };
        let canonical_root = root.canonicalize().map_err(io)?;
        for (index, entry) in entries.enumerate() {
            if index >= 20_000 {
                return Err("Antigravity 会话目录过大".into());
            }
            let entry = entry.map_err(io)?;
            if !entry.file_type().map_err(io)?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(native) = name.to_str() else {
                continue;
            };
            if tokenscope_core_domain::stable_record_id(
                "antigravity",
                b"conversation",
                native.as_bytes(),
            )
            .ok()
            .as_deref()
                != Some(id)
            {
                continue;
            }
            let transcript = entry
                .path()
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            let canonical = match transcript.canonicalize() {
                Ok(path) => path,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io(e)),
            };
            if canonical.starts_with(&canonical_root)
                && canonical.is_file()
                && !paths.contains(&canonical)
            {
                paths.push(canonical);
            }
        }
    }
    if paths.is_empty() {
        return Err(
            "未找到这个 Antigravity 会话的本地正文日志；旧格式或未生成日志的会话暂不可查看。"
                .into(),
        );
    }
    Ok(paths)
}

fn antigravity_message(line: usize, row: &Value) -> Option<Message> {
    let source = row.get("source")?.as_str()?;
    let kind = row.get("type")?.as_str()?;
    let mut text = row.get("content")?.as_str()?;
    let role = match (source, kind) {
        ("USER_EXPLICIT", "USER_INPUT") => {
            if let Some(wrapped) = text.trim_start().strip_prefix("<USER_REQUEST>") {
                let request = wrapped
                    .split_once("<ADDITIONAL_METADATA>")
                    .map_or(wrapped, |(before, _)| before);
                text = request.trim_end().strip_suffix("</USER_REQUEST>")?.trim();
            }
            "user"
        }
        ("MODEL", "PLANNER_RESPONSE") => "assistant",
        _ => return None,
    };
    if text.trim().is_empty() || text.len() > MAX_TEXT {
        return None;
    }
    Some(Message {
        line,
        pointer: "/content".into(),
        role,
        text: text.to_owned(),
    })
}

fn part_path(paths: &[PathBuf], part: usize) -> Result<&Path, String> {
    let path = paths.get(part).ok_or("会话分段已变化，请重新打开")?;
    if path.extension().is_some_and(|ext| ext == "zst") {
        return Err("此分段已压缩归档，暂不支持编辑".into());
    }
    Ok(path)
}

fn file_revision(path: &Path, content_revision: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(content_revision.as_bytes());
    hash.update(path.as_os_str().as_encoded_bytes());
    format!("{:x}", hash.finalize())
}

fn header_id(path: &Path, tool: &str) -> Option<String> {
    let mut reader = BufReader::new(File::open(path).ok()?.take(MAX_LINE as u64 + 1));
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).ok()?;
    let value: Value = serde_json::from_slice(&line).ok()?;
    match (tool, value.get("type")?.as_str()?) {
        ("codex", "session_meta") => value.pointer("/payload/id")?.as_str().map(str::to_owned),
        ("deepseek-harness" | "pi", "session") => value.get("id")?.as_str().map(str::to_owned),
        _ => None,
    }
}

fn read_bytes(file: &File) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(io)?.len() > MAX_FILE as u64 {
        return Err("会话超过 128 MiB，暂不支持编辑".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > MAX_FILE {
        return Err("会话超过 128 MiB，暂不支持编辑".into());
    }
    Ok(bytes)
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn messages(tool: &str, line: usize, row: &Value) -> Vec<Message> {
    if tool == "antigravity" {
        return antigravity_message(line, row).into_iter().collect();
    }
    let kind = row.get("type").and_then(Value::as_str).unwrap_or_default();
    let (role, base, expected) = match (tool, kind) {
        ("codex", "response_item")
            if row.pointer("/payload/type").and_then(Value::as_str) == Some("message") =>
        {
            match row.pointer("/payload/role").and_then(Value::as_str) {
                Some("user") => ("user", "/payload/content", "input_text"),
                Some("assistant") => ("assistant", "/payload/content", "output_text"),
                _ => return Vec::new(),
            }
        }
        ("pi", "message") => match row.pointer("/message/role").and_then(Value::as_str) {
            Some("user") => ("user", "/message/content", "text"),
            Some("assistant") => ("assistant", "/message/content", "text"),
            _ => return Vec::new(),
        },
        ("claude", "user") => ("user", "/message/content", "text"),
        ("claude", "assistant") => ("assistant", "/message/content", "text"),
        ("deepseek-harness", "user/message" | "assistant/message") => {
            let base = if row.pointer("/data/message").is_some() {
                "/data/message/content"
            } else {
                "/data/content"
            };
            (
                if kind == "user/message" {
                    "user"
                } else {
                    "assistant"
                },
                base,
                "text",
            )
        }
        _ => return Vec::new(),
    };
    if row.get("isMeta").and_then(Value::as_bool) == Some(true) {
        return Vec::new();
    }
    let text_block = |pointer: String, text: &str| {
        // Generated environment context is sometimes recorded with role=user.
        (!text.trim().is_empty()
            && text.len() <= MAX_TEXT
            && !text.trim_start().starts_with("<environment_context>")
            && !text.trim_start().starts_with("<user_instructions>"))
        .then(|| Message {
            line,
            pointer,
            role,
            text: text.to_owned(),
        })
    };
    match row.pointer(base) {
        Some(Value::String(s)) => text_block(base.to_owned(), s).into_iter().collect(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                (b.get("type")?.as_str()? == expected).then_some(())?;
                text_block(format!("{base}/{i}/text"), b.get("text")?.as_str()?)
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn parse(line: &[u8]) -> Result<Option<Value>, String> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(None);
    }
    if line.len() > MAX_LINE {
        return Err("会话包含超过 4 MiB 的记录，暂不支持编辑".into());
    }
    serde_json::from_slice(line)
        .map(Some)
        .map_err(|_| "会话日志不完整或格式未知，请关闭原会话后重试".into())
}

fn old_enough(file: &File) -> bool {
    file.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|elapsed| elapsed >= Duration::from_secs(30))
}

fn inspect(tool: &str, bytes: &[u8], offset: usize, writable: bool) -> Result<History, String> {
    let mut page = Vec::new();
    let mut total = 0;
    let mut open_turn = false;
    for (i, line) in bytes.split_inclusive(|b| *b == b'\n').enumerate() {
        let Some(row) = parse(line)? else { continue };
        if tool == "codex" && row.get("type").and_then(Value::as_str) == Some("event_msg") {
            match row.pointer("/payload/type").and_then(Value::as_str) {
                Some("task_started" | "turn_started") => open_turn = true,
                Some("task_complete" | "task_aborted" | "turn_aborted" | "turn_completed") => {
                    open_turn = false
                }
                _ => {}
            }
        }
        for message in messages(tool, i, &row) {
            if total >= offset && page.len() < PAGE_SIZE {
                page.push(message);
            }
            total += 1;
        }
    }
    Ok(History {
        revision: revision(bytes),
        messages: page,
        total,
        writable: writable && !open_turn,
        part_count: 1,
        part_index: 0,
    })
}

fn codex_event(row: &Value) -> Option<&str> {
    (row.get("type")?.as_str()? == "event_msg").then_some(())?;
    row.pointer("/payload/type")?.as_str()
}

fn rewrite(tool: &str, bytes: &[u8], edit: &Edit) -> Result<Vec<u8>, String> {
    let text = match (edit.delete, edit.text.as_deref()) {
        (true, None) => "",
        (false, Some(text)) if !text.trim().is_empty() && text.len() <= MAX_TEXT => text,
        _ => return Err("正文不能为空，也不能超过 256 KiB".into()),
    };
    let lines: Vec<_> = bytes.split_inclusive(|b| *b == b'\n').collect();
    let Some(mut row) = lines
        .get(edit.line)
        .map(|line| parse(line))
        .transpose()?
        .flatten()
    else {
        return Err("找不到要修改的消息".into());
    };
    let original = messages(tool, edit.line, &row)
        .into_iter()
        .find(|m| m.pointer == edit.pointer)
        .ok_or("此字段不是可编辑的聊天正文")?;
    if original.text == text {
        return Ok(bytes.to_vec());
    }
    // Preserve the record and its links, attachments and accounting fields.
    *row.pointer_mut(&edit.pointer).ok_or("找不到正文")? = Value::String(text.to_owned());
    let mut replacements = std::collections::BTreeMap::from([(edit.line, row)]);
    if tool == "codex" {
        // The Codex UI can render event copies instead of the model input/output.
        // Match only within the containing turn, and refuse ambiguous copies.
        let mut start = 0;
        let mut end = lines.len();
        for (i, line) in lines.iter().enumerate() {
            if let Some(row) = parse(line)? {
                if matches!(codex_event(&row), Some("task_started" | "turn_started")) {
                    if i > edit.line {
                        end = i;
                        break;
                    }
                    start = i;
                }
            }
        }
        let mut equivalents = 0;
        for (i, line) in lines.iter().enumerate().take(end).skip(start) {
            let Some(mut row) = parse(line)? else {
                continue;
            };
            equivalents += messages(tool, i, &row)
                .into_iter()
                .filter(|m| m.role == original.role && m.text == original.text)
                .count();
            let key = match (original.role, codex_event(&row)) {
                ("user", Some("user_message")) | ("assistant", Some("agent_message")) => {
                    "/payload/message"
                }
                ("assistant", Some("task_complete" | "turn_completed")) => {
                    "/payload/last_agent_message"
                }
                _ => continue,
            };
            if let Some(Value::String(copy)) = row.pointer_mut(key) {
                if *copy == original.text {
                    *copy = text.to_owned();
                    replacements.insert(i, row);
                }
            }
        }
        if equivalents > 1 && replacements.len() > 1 {
            return Err("同一轮有重复文本，无法安全定位 Codex 的显示副本".into());
        }
    }
    let mut output = Vec::with_capacity(bytes.len());
    for (i, line) in lines.into_iter().enumerate() {
        if let Some(row) = replacements.remove(&i) {
            let start = output.len();
            serde_json::to_writer(&mut output, &row).map_err(|e| e.to_string())?;
            if line.ends_with(b"\r\n") {
                output.extend_from_slice(b"\r\n");
            } else if line.ends_with(b"\n") {
                output.push(b'\n');
            }
            if output.len() - start > MAX_LINE {
                return Err("修改后单条日志超过 4 MiB".into());
            }
        } else {
            output.extend_from_slice(line);
        }
    }
    if output.len() > MAX_FILE {
        return Err("修改后会话超过 128 MiB".into());
    }
    Ok(output)
}

fn same_revision(
    path: &Path,
    file: &File,
    expected: &str,
    last: &Option<(PathBuf, String)>,
) -> bool {
    old_enough(file)
        || last
            .as_ref()
            .is_some_and(|(p, r)| p == path && r == expected)
}

fn create_copy(path: &Path, bytes: &[u8], original: &File) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut target = options.open(path).map_err(io)?;
    let result = target
        .set_permissions(original.metadata().map_err(io)?.permissions())
        .and_then(|_| target.write_all(bytes))
        .and_then(|_| target.sync_all());
    drop(target);
    if let Err(error) = result {
        let _ = fs::remove_file(path);
        return Err(io(error));
    }
    Ok(())
}

fn mutate<T>(
    tool_id: &str,
    session_id: &str,
    expected_revision: &str,
    part: Option<usize>,
    apply: impl FnOnce(&Path, &File, &[u8], usize, &mut Option<(PathBuf, String)>) -> Result<T, String>,
) -> Result<T, String> {
    let mut last = LAST_SAVE.lock().map_err(|_| "会话保存锁不可用")?;
    let (paths, _) = locate(tool_id, session_id)?;
    if part.is_none() && paths.len() != 1 {
        return Err("此会话有多个分段，暂不支持删除整个会话".into());
    }
    let path = part_path(&paths, part.unwrap_or_default())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Allow readers and atomic replacement; reject an open writer.
        options.share_mode(0x1 | 0x4);
    }
    let original = options
        .open(path)
        .map_err(|e| format!("无法锁定会话文件，请退出原会话后重试：{e}"))?;
    let bytes = read_bytes(&original)?;
    if file_revision(path, &revision(&bytes)) != expected_revision {
        return Err("会话已在原 Agent 中变化，请重新打开后再编辑".into());
    }
    if !inspect(
        tool_id,
        &bytes,
        0,
        same_revision(path, &original, expected_revision, &last),
    )?
    .writable
    {
        return Err("会话仍在运行或刚更新，请关闭原会话并等待 30 秒后重试".into());
    }
    apply(path, &original, &bytes, paths.len(), &mut last)
}

fn backup_path(path: &Path, stamp: u128) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("会话文件名无效")?;
    Ok(path.with_file_name(format!("{name}.tokenscope-{stamp}.bak")))
}

fn stamp() -> Result<u128, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos())
}

fn move_to_backup(path: &Path, backup: &Path) -> Result<(), String> {
    if backup.try_exists().map_err(io)? {
        return Err("备份文件已存在，请重试".into());
    }
    fs::rename(path, backup).map_err(io)
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
        mutate(
            &tool_id,
            &session_id,
            &expected_revision,
            Some(part.unwrap_or_default()),
            |path, original, bytes, count, last| {
                let updated = rewrite(&tool_id, bytes, &edit)?;
                let mut history = inspect(&tool_id, &updated, offset, true)?;
                if offset > 0 && history.messages.is_empty() {
                    history = inspect(&tool_id, &updated, offset.saturating_sub(PAGE_SIZE), true)?;
                }
                history.part_count = count;
                history.part_index = part.unwrap_or_default();
                history.revision = file_revision(path, &history.revision);
                if updated == bytes {
                    return Ok(history);
                }
                let stamp = stamp()?;
                let backup = backup_path(path, stamp)?;
                let temporary = path.with_file_name(format!(".tokenscope-history-{stamp}.tmp"));
                create_copy(&temporary, &updated, original)?;
                let result = (|| {
                    create_copy(&backup, bytes, original)?;
                    if part_path(&locate(&tool_id, &session_id)?.0, part.unwrap_or_default())?
                        != path
                        || read_bytes(&File::open(path).map_err(io)?)? != bytes
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
        mutate(
            &tool_id,
            &session_id,
            &expected_revision,
            None,
            |path, _, bytes, _, last| {
                let backup = backup_path(path, stamp()?)?;
                if locate(&tool_id, &session_id)?.0.as_slice() != [path.to_owned()]
                    || read_bytes(&File::open(path).map_err(io)?)? != bytes
                {
                    return Err("会话已变化，未删除".into());
                }
                move_to_backup(path, &backup)?;
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
            (antigravity_transcripts(&home, &session_id)?, None)
        } else if tool_id == "pi" {
            let root = crate::session_activity::pi_sessions_root().ok_or("未找到 Pi Agent 目录")?;
            locate_in("pi", &session_id, &root, &[""])?
        } else {
            locate(&tool_id, &session_id)?
        };
        let index = part.or(preferred).unwrap_or_else(|| {
            paths
                .iter()
                .position(|p| part_path(std::slice::from_ref(p), 0).is_ok())
                .unwrap_or_default()
        });
        let path = part_path(&paths, index)?;
        let file = File::open(path).map_err(io)?;
        let bytes = read_bytes(&file)?;
        let mut history = inspect(
            &tool_id,
            &bytes,
            offset,
            !matches!(tool_id.as_str(), "antigravity" | "pi"),
        )?;
        history.part_count = paths.len();
        history.part_index = index;
        history.revision = file_revision(path, &history.revision);
        if history.writable {
            let last = LAST_SAVE.lock().map_err(|_| "会话保存锁不可用")?;
            history.writable = same_revision(path, &file, &history.revision, &last);
        }
        Ok(history)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn antigravity_history_only_exposes_user_and_assistant_text() {
        let transcript = br#"{"source":"USER_EXPLICIT","type":"USER_INPUT","content":"<USER_REQUEST>hello</USER_REQUEST>\n<ADDITIONAL_METADATA>private metadata</ADDITIONAL_METADATA>"}
{"source":"MODEL","type":"GENERIC","content":"tool result"}
{"source":"MODEL","type":"PLANNER_RESPONSE","content":"answer","thinking":"private reasoning"}
"#;
        let history = inspect("antigravity", transcript, 0, false).unwrap();
        assert!(!history.writable);
        assert_eq!(history.total, 2);
        assert_eq!(history.messages[0].text, "hello");
        assert_eq!(history.messages[1].text, "answer");
    }

    #[test]
    fn codex_locator_groups_continuations_and_resolves_legacy_keys() {
        let home = std::env::temp_dir().join(format!(
            "tokenscope-locator-{}-{}",
            std::process::id(),
            stamp().unwrap()
        ));
        let dir = home.join("sessions");
        fs::create_dir_all(&dir).unwrap();
        let original = dir.join("rollout-01-thread.jsonl");
        let continued = dir.join("rollout-02-thread_continuation.jsonl");
        let header = r#"{"type":"session_meta","payload":{"id":"thread"}}"#;
        fs::write(&original, header).unwrap();
        fs::write(&continued, header).unwrap();
        let key = session_key("codex", b"thread").unwrap();
        assert_eq!(
            locate_in("codex", &key, &home, &["sessions"]).unwrap(),
            (vec![original.clone(), continued.clone()], None)
        );
        let alias = legacy_codex_identity("thread", &continued).unwrap();
        let key = session_key("codex", alias.as_bytes()).unwrap();
        assert_eq!(
            locate_in("codex", &key, &home, &["sessions"]).unwrap(),
            (vec![original.clone(), continued.clone()], Some(1))
        );
        assert_ne!(
            file_revision(&original, "same"),
            file_revision(&continued, "same")
        );
        fs::remove_file(original).unwrap();
        fs::remove_file(continued).unwrap();
        fs::remove_dir(dir).unwrap();
        fs::remove_dir(home).unwrap();
    }

    #[test]
    fn codex_updates_display_copy_without_rewriting_usage() {
        let bytes = br#"{"type":"event_msg","payload":{"type":"task_started"}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"before"}]}}
{"type":"event_msg","payload":{"type":"agent_message","message":"before"}}
{"type":"event_msg", "payload":{"type":"token_count","total":123}}
{"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"before"}}
"#;
        let history = inspect("codex", bytes, 0, true).unwrap();
        assert!(history.writable);
        let edit = Edit {
            line: history.messages[0].line,
            pointer: history.messages[0].pointer.clone(),
            text: Some("改后\n第二行".into()),
            delete: false,
        };
        let output = rewrite("codex", bytes, &edit).unwrap();
        let text = std::str::from_utf8(&output).unwrap();
        assert_eq!(text.matches("改后").count(), 3);
        assert!(
            text.contains(r#"{"type":"event_msg", "payload":{"type":"token_count","total":123}}"#)
        );
        let first = bytes.split_inclusive(|&b| b == b'\n').next().unwrap();
        assert!(!inspect("codex", first, 0, true).unwrap().writable);
        let deleted = rewrite(
            "codex",
            bytes,
            &Edit {
                text: None,
                delete: true,
                ..edit
            },
        )
        .unwrap();
        let deleted_text = std::str::from_utf8(&deleted).unwrap();
        assert!(!deleted_text.contains("before"));
        assert!(deleted_text.contains(r#""total":123"#));
        assert_eq!(inspect("codex", &deleted, 0, true).unwrap().total, 0);
    }

    #[test]
    fn only_conversation_text_is_editable_in_claude_and_deepseek() {
        for (tool, bytes) in [
            ("claude", br#"{"type":"assistant","message":{"content":[{"type":"text","text":"before"},{"type":"tool_use","text":"keep"}]}}"#.as_slice()),
            ("deepseek-harness", br#"{"type":"user/message","data":{"message":{"content":"before","source":{"kind":"user"}}}}"#.as_slice()),
        ] {
            let history = inspect(tool, bytes, 0, true).unwrap();
            assert_eq!(history.total, 1);
            let edit = Edit { line: 0, pointer: history.messages[0].pointer.clone(), text: Some("after".into()), delete: false };
            let rewritten = rewrite(tool, bytes, &edit).unwrap();
            assert_eq!(inspect(tool, &rewritten, 0, true).unwrap().messages[0].text, "after");
            assert!(rewrite(tool, bytes, &Edit { pointer: "/type".into(), ..edit }).is_err());
        }
    }

    #[test]
    fn replacement_preserves_a_backup_while_the_source_is_open() {
        let dir = std::env::temp_dir().join(format!(
            "tokenscope-history-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&dir).unwrap();
        let source = dir.join("source.jsonl");
        let temporary = dir.join("replacement.tmp");
        let backup = dir.join("source.bak");
        fs::write(&source, b"original").unwrap();
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(0x1 | 0x4);
        }
        let open = options.open(&source).unwrap();
        create_copy(&backup, b"original", &open).unwrap();
        create_copy(&temporary, b"edited", &open).unwrap();
        crate::agent_customizations::replace_file(&temporary, &source, true).unwrap();
        assert_eq!(fs::read(&backup).unwrap(), b"original");
        assert_eq!(fs::read(&source).unwrap(), b"edited");
        drop(open);
        let removed = dir.join("removed.bak");
        let open = options.open(&source).unwrap();
        move_to_backup(&source, &removed).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&removed).unwrap(), b"edited");
        drop(open);
        fs::remove_file(removed).unwrap();
        fs::remove_file(backup).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
