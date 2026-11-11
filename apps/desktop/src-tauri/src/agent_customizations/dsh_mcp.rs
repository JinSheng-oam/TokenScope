use super::mcp::parse_json;
use serde_json::{json, Map, Value};
use yaml_rust2::{Yaml, YamlLoader};

const START: &str = "# tokenscope:mcp:start";
const END: &str = "# tokenscope:mcp:end";
const CLIENT: &str = "@deepseek-ai/dsh-mcp-client";
const PREFIX: &str = "tokenscope-mcp-";

fn sequence(raw: &str) -> Result<Vec<Yaml>, String> {
    if raw.trim_start_matches('\u{feff}').lines().all(|line| {
        let line = line.trim();
        line.is_empty() || line.starts_with('#')
    }) {
        return Ok(Vec::new());
    }
    let mut docs = YamlLoader::load_from_str(raw.trim_start_matches('\u{feff}'))
        .map_err(|_| "DeepSeek 配置档案不是有效的 YAML")?;
    match docs.pop() {
        Some(Yaml::Array(rows)) if docs.is_empty() => Ok(rows),
        _ => Err("DeepSeek 配置档案必须包含一个 YAML 补丁列表".into()),
    }
}

fn parts(raw: &str) -> Result<(&str, Option<&str>, &str), String> {
    let mut start = None;
    let mut end = None;
    let mut offset = 0;
    for line in raw.split_inclusive('\n') {
        match line.trim_end_matches(['\r', '\n']) {
            START if start.is_none() && end.is_none() => {
                start = Some((offset, offset + line.len()))
            }
            END if start.is_some() && end.is_none() => end = Some((offset, offset + line.len())),
            START | END => return Err("DeepSeek MCP 管理区标记重复或顺序错误".into()),
            _ => {}
        }
        offset += line.len();
    }
    match (start, end) {
        (None, None) => Ok((raw, None, "")),
        (Some((begin, body)), Some((finish, tail))) => {
            Ok((&raw[..begin], Some(&raw[body..finish]), &raw[tail..]))
        }
        _ => Err("DeepSeek MCP 管理区标记不完整".into()),
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn read_managed(raw: &str) -> Result<Map<String, Value>, String> {
    let mut result = Map::new();
    if let Some(body) = parts(raw)?.1 {
        let rows = parse_json(
            body.trim()
                .strip_prefix("- insert: ")
                .ok_or("无法识别 DeepSeek MCP 管理区")?,
        )?;
        for row in rows.as_array().ok_or("DeepSeek MCP 管理区不是列表")? {
            let mut config = row["config"]
                .as_object()
                .cloned()
                .ok_or("DeepSeek MCP 条目缺少配置")?;
            let name = config
                .remove("serverName")
                .and_then(|v| v.as_str().map(str::to_owned))
                .filter(|name| valid_name(name))
                .ok_or("DeepSeek MCP 条目名称无效")?;
            if row["name"] != CLIENT
                || row["id"] != format!("{PREFIX}{name}")
                || result.contains_key(&name)
            {
                return Err("DeepSeek MCP 管理区条目无法识别".into());
            }
            if row["disabled"] == true {
                config.insert("enabled".into(), Value::Bool(false));
            }
            result.insert(name, Value::Object(config));
        }
    }
    Ok(result)
}

pub(super) fn draft(raw: &str) -> Result<String, String> {
    sequence(raw)?;
    serde_json::to_string_pretty(&json!({"mcpServers": read_managed(raw)?}))
        .map_err(|_| "无法显示 DeepSeek MCP 配置".into())
}

fn has_conflict(rows: &[Yaml], name: &str) -> bool {
    let id = format!("{PREFIX}{name}");
    let mut stack: Vec<_> = rows.iter().collect();
    while let Some(node) = stack.pop() {
        if node["id"].as_str() == Some(&id)
            || (node["name"].as_str() == Some(CLIENT)
                && node["config"]["serverName"].as_str() == Some(name))
            || (node["id"].as_str() == Some("mcp")
                && !node["config"]["servers"][name].is_badvalue())
        {
            return true;
        }
        if let Some(children) = node.as_vec() {
            stack.extend(children);
        } else if let Some(fields) = node.as_hash() {
            stack.extend(fields.values());
        }
    }
    false
}

pub(super) fn merge(raw: &str, home: &str, edited: &str) -> Result<String, String> {
    let (before, _, after) = parts(raw)?;
    let mut outside = format!("{before}{after}");
    let existing = sequence(&outside)?;
    let home = sequence(home)?;
    let mut document = parse_json(edited)?;
    let document = document.as_object_mut().ok_or("配置必须是 JSON 对象")?;
    let servers = document
        .remove("mcpServers")
        .ok_or("请保留 mcpServers 对象")?;
    if !document.is_empty() {
        return Err("只能编辑 mcpServers 对象".into());
    }
    let servers = servers.as_object().ok_or("mcpServers 必须是对象")?;
    let mut rows = Vec::new();
    for (name, server) in servers {
        if !valid_name(name) {
            return Err("服务器名称仅允许 1–32 个英文字母、数字、下划线或连字符".into());
        }
        if has_conflict(&existing, name) || has_conflict(&home, name) {
            return Err(format!("服务器 {name} 已在手写补丁中配置，请使用其他名称"));
        }
        let mut config = server
            .as_object()
            .cloned()
            .ok_or("每个服务器必须是 JSON 对象")?;
        let enabled = match config.remove("enabled") {
            None => true,
            Some(Value::Bool(value)) => value,
            _ => return Err("enabled 必须是布尔值".into()),
        };
        let required = match config.get("transport").and_then(Value::as_str) {
            Some("stdio") => "command",
            Some("streamable-http") => "url",
            _ => return Err("transport 必须是 stdio 或 streamable-http".into()),
        };
        if config
            .get(required)
            .and_then(Value::as_str)
            .is_none_or(|s| s.trim().is_empty())
        {
            return Err(format!("服务器 {name} 缺少 {required}"));
        }
        config.insert("serverName".into(), Value::String(name.clone()));
        let mut row = json!({"id": format!("{PREFIX}{name}"), "name": CLIENT, "config": config});
        if !enabled {
            row["disabled"] = Value::Bool(true);
        }
        rows.push(row);
    }
    if rows.is_empty() {
        if existing.is_empty()
            && !outside
                .trim()
                .lines()
                .any(|l| l.trim_start().starts_with("[]"))
        {
            outside.push_str("\n[]\n");
        }
        sequence(&outside)?;
        return Ok(outside);
    }

    // Keep existing YAML bytes (including comments and !!js tags). The only
    // exception is an empty flow list, which cannot have block rows appended.
    let nl = if raw.contains("\r\n") { "\r\n" } else { "\n" };
    if existing.is_empty()
        && !outside.trim_start_matches('\u{feff}').lines().all(|l| {
            let l = l.trim();
            l.is_empty() || l.starts_with('#')
        })
    {
        let trimmed = outside.trim_start_matches('\u{feff}').trim();
        if trimmed != "[]" {
            return Err("空的 DeepSeek 档案格式无法安全追加，请先在原应用中保存".into());
        }
        outside = if outside.starts_with('\u{feff}') {
            "\u{feff}".into()
        } else {
            String::new()
        };
    }
    if !outside.is_empty() && !outside.ends_with('\n') {
        outside.push_str(nl);
    }
    // Use a single JSON flow value on the YAML insert row. It is data only,
    // never evaluated as YAML code or sent to a running Harness process.
    let body = serde_json::to_string(&rows).map_err(|_| "无法生成 DeepSeek MCP 补丁")?;
    outside.push_str(&format!("{START}{nl}- insert: {body}{nl}{END}{nl}"));
    let parsed = sequence(&outside)?;
    if parsed.len() != existing.len() + 1 || parsed[..existing.len()] != existing {
        return Err("无法在保留原补丁结构的前提下写入 MCP".into());
    }
    read_managed(&outside)?;
    Ok(outside)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_entries_roundtrip_without_rewriting_manual_yaml() {
        let original = "# user preferences\r\n- id: other\r\n  config:\r\n    value: !!js process.platform\r\n";
        let input =
            r#"{"mcpServers":{"demo":{"transport":"stdio","command":"demo","enabled":false}}}"#;
        let next = merge(original, "", input).unwrap();
        assert!(next.starts_with(original));
        assert!(next.contains("\"disabled\":true"));
        assert_eq!(
            parse_json(&draft(&next).unwrap()).unwrap(),
            parse_json(input).unwrap()
        );
        assert_eq!(merge(&next, "", r#"{"mcpServers":{}}"#).unwrap(), original);
        let empty = merge("[]\n", "", input).unwrap();
        assert_eq!(sequence(&empty).unwrap().len(), 1);
        assert!(merge(
            original,
            "- insert:\n  - name: '@deepseek-ai/dsh-mcp-client'\n    config: {serverName: demo}\n",
            input
        )
        .is_err());
    }
}
