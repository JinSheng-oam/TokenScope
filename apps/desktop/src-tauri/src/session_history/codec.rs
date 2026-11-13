use super::{storage::revision, Edit, History, Message, MAX_FILE, MAX_LINE, MAX_TEXT, PAGE_SIZE};
use serde_json::Value;

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

pub(super) fn inspect(
    tool: &str,
    bytes: &[u8],
    offset: usize,
    writable: bool,
) -> Result<History, String> {
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

pub(super) fn rewrite(tool: &str, bytes: &[u8], edit: &Edit) -> Result<Vec<u8>, String> {
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
