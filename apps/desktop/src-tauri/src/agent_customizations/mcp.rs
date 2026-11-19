use super::{display_path, is_link, read_path, save_path, Root};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use toml_edit::{Array, DocumentMut, Item, Table, Value as TomlValue};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Toml,
    Json,
    Deepseek,
}

struct Target {
    provider: String,
    path: PathBuf,
    format: Format,
    overlay: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServerEntry {
    pub(crate) name: String,
    pub(crate) transport: String,
    pub(crate) command: Option<String>,
    #[serde(default)]
    pub(crate) args: Vec<String>,
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) env: BTreeMap<String, String>,
    #[serde(default)]
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) cwd: Option<String>,
    #[serde(default = "default_enabled")]
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) has_extra_fields: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Serialize)]
pub(crate) struct McpView {
    path: String,
    format: &'static str,
    draft: String,
    revision: String,
    servers: Vec<McpServerEntry>,
}

impl Target {
    fn for_provider(provider: &str, profile: Option<&str>) -> Result<Self, String> {
        use super::paths::{configured, home};
        let home = home()?;
        let mut overlay = None;
        let (path, format) = match provider {
            "codex" => (
                configured("CODEX_HOME", home.join(".codex")).join("config.toml"),
                Format::Toml,
            ),
            "claude" => (
                configured("CLAUDE_CONFIG_DIR", home).join(".claude.json"),
                Format::Json,
            ),
            "antigravity" => (
                configured("TOKENSCOPE_GEMINI_HOME", home.join(".gemini"))
                    .join("config/mcp_config.json"),
                Format::Json,
            ),
            "pi" => (
                configured("PI_CODING_AGENT_DIR", home.join(".pi/agent")).join("mcp.json"),
                Format::Json,
            ),
            "deepseek" => {
                let profile = match profile {
                    Some(value @ ("desktop" | "web")) => value,
                    _ => return Err("请选择 DeepSeek Desktop 或 Web 配置档案".into()),
                };
                let dsh = configured("DSH_HOME", home.join(".dsh"));
                let directory = dsh.join("profiles").join(profile);
                if !directory.join("package.json").is_file() {
                    return Err("尚未初始化该 DeepSeek 配置档案，请先在原应用中打开一次".into());
                }
                overlay = Some(dsh.join("cordis.patch.yml"));
                (directory.join("cordis.patch.yml"), Format::Deepseek)
            }
            _ => return Err("不支持此 Agent 的 MCP 配置".into()),
        };
        if !path.is_absolute()
            || path.ancestors().any(is_link)
            || overlay.as_ref().is_some_and(|p| p.ancestors().any(is_link))
        {
            return Err("MCP 配置路径必须是本机绝对路径，且不能包含符号链接".into());
        }
        Ok(Self {
            provider: provider.to_owned(),
            path,
            format,
            overlay,
        })
    }

    fn root(&self) -> Result<Root, String> {
        Ok(Root {
            id: "mcp-config",
            path: self.path.parent().ok_or("MCP 配置目录无效")?.to_path_buf(),
            kind: "config",
            names: &[],
        })
    }

    fn view(&self, raw: Option<&str>) -> Result<McpView, String> {
        let (format, draft) = match self.format {
            Format::Deepseek => ("JSON", super::dsh_mcp::draft(raw.unwrap_or_default())?),
            Format::Toml => {
                let mut isolated = DocumentMut::new();
                if let Some(section) = parse_toml(raw.unwrap_or_default())?.remove("mcp_servers") {
                    if section.as_table_like().is_none() {
                        return Err("原配置的 mcp_servers 必须是表".into());
                    }
                    isolated.insert("mcp_servers", section);
                }
                let draft = if isolated.is_empty() {
                    "[mcp_servers]\n".into()
                } else {
                    isolated.to_string()
                };
                ("TOML", draft)
            }
            Format::Json => {
                let mut existing = parse_json(raw.unwrap_or("{}"))?;
                let servers = existing
                    .as_object_mut()
                    .ok_or("原配置必须是 JSON 对象")?
                    .remove("mcpServers")
                    .unwrap_or_else(|| serde_json::json!({}));
                if !servers.is_object() {
                    return Err("原配置的 mcpServers 必须是对象".into());
                }
                (
                    "JSON",
                    serde_json::to_string_pretty(&serde_json::json!({"mcpServers": servers}))
                        .map_err(|_| "无法显示 MCP 配置")?,
                )
            }
        };
        let servers = extract_servers(self.format, &draft)?;
        Ok(McpView {
            path: display_path(&self.path),
            format,
            draft,
            revision: revision(&self.path, raw),
            servers,
        })
    }
}

fn clean_opt(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn normalize_transport(
    raw: Option<&str>,
    has_url: bool,
    has_command: bool,
    format: Format,
) -> String {
    match raw.map(str::trim).unwrap_or_default() {
        "stdio" => "stdio".into(),
        "sse" => "sse".into(),
        "streamable-http" | "streamableHttp" => "streamable-http".into(),
        "http" => {
            if format == Format::Deepseek {
                "streamable-http".into()
            } else {
                "http".into()
            }
        }
        _ if has_url && !has_command => {
            if format == Format::Deepseek {
                "streamable-http".into()
            } else {
                "http".into()
            }
        }
        _ => "stdio".into(),
    }
}

fn extract_servers(format: Format, draft: &str) -> Result<Vec<McpServerEntry>, String> {
    let mut result = Vec::new();
    match format {
        Format::Toml => {
            let doc = parse_toml(draft)?;
            let Some(table) = doc.get("mcp_servers").and_then(Item::as_table_like) else {
                return Ok(result);
            };
            const KNOWN: &[&str] = &[
                "command",
                "args",
                "url",
                "cwd",
                "env",
                "http_headers",
                "headers",
                "enabled",
                "disabled",
                "transport",
                "type",
            ];
            for (name, item) in table.iter() {
                let Some(server) = item.as_table_like() else {
                    continue;
                };
                let command = clean_opt(server.get("command").and_then(Item::as_str));
                let args = server
                    .get("args")
                    .and_then(Item::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(TomlValue::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let url = clean_opt(server.get("url").and_then(Item::as_str));
                let cwd = clean_opt(server.get("cwd").and_then(Item::as_str));
                let env = server
                    .get("env")
                    .and_then(Item::as_table_like)
                    .map(|tbl| {
                        tbl.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.to_owned(), s.to_owned())))
                            .collect()
                    })
                    .unwrap_or_default();
                let headers = server
                    .get("http_headers")
                    .or_else(|| server.get("headers"))
                    .and_then(Item::as_table_like)
                    .map(|tbl| {
                        tbl.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.to_owned(), s.to_owned())))
                            .collect()
                    })
                    .unwrap_or_default();
                let enabled = if let Some(v) = server.get("enabled").and_then(Item::as_bool) {
                    v
                } else if let Some(disabled) = server.get("disabled").and_then(Item::as_bool) {
                    !disabled
                } else {
                    true
                };
                let transport = normalize_transport(
                    server
                        .get("transport")
                        .or_else(|| server.get("type"))
                        .and_then(Item::as_str),
                    url.is_some(),
                    command.is_some(),
                    format,
                );
                let has_extra_fields = server.iter().any(|(k, _)| !KNOWN.contains(&k));
                result.push(McpServerEntry {
                    name: name.to_owned(),
                    transport,
                    command,
                    args,
                    url,
                    env,
                    headers,
                    cwd,
                    enabled,
                    has_extra_fields,
                });
            }
        }
        Format::Json | Format::Deepseek => {
            let doc = parse_json(draft)?;
            let Some(servers) = doc.get("mcpServers").and_then(Value::as_object) else {
                return Ok(result);
            };
            const KNOWN: &[&str] = &[
                "command",
                "args",
                "url",
                "serverUrl",
                "cwd",
                "env",
                "headers",
                "enabled",
                "disabled",
                "transport",
                "type",
            ];
            for (name, val) in servers {
                let Some(obj) = val.as_object() else {
                    continue;
                };
                let command = clean_opt(obj.get("command").and_then(Value::as_str));
                let args = obj
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                let url = clean_opt(
                    obj.get("url")
                        .or_else(|| obj.get("serverUrl"))
                        .and_then(Value::as_str),
                );
                let cwd = clean_opt(obj.get("cwd").and_then(Value::as_str));
                let env = obj
                    .get("env")
                    .and_then(Value::as_object)
                    .map(|map| {
                        map.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                            .collect()
                    })
                    .unwrap_or_default();
                let headers = obj
                    .get("headers")
                    .and_then(Value::as_object)
                    .map(|map| {
                        map.iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                            .collect()
                    })
                    .unwrap_or_default();
                let enabled = if let Some(v) = obj.get("enabled").and_then(Value::as_bool) {
                    v
                } else if let Some(disabled) = obj.get("disabled").and_then(Value::as_bool) {
                    !disabled
                } else {
                    true
                };
                let transport = normalize_transport(
                    obj.get("transport")
                        .or_else(|| obj.get("type"))
                        .and_then(Value::as_str),
                    url.is_some(),
                    command.is_some(),
                    format,
                );
                let has_extra_fields = obj.keys().any(|k| !KNOWN.contains(&k.as_str()));
                result.push(McpServerEntry {
                    name: name.clone(),
                    transport,
                    command,
                    args,
                    url,
                    env,
                    headers,
                    cwd,
                    enabled,
                    has_extra_fields,
                });
            }
        }
    }
    Ok(result)
}

fn validate_entry(format: Format, entry: &mut McpServerEntry) -> Result<(), String> {
    entry.name = entry.name.trim().to_owned();
    if entry.name.is_empty()
        || entry.name.len() > 128
        || entry
            .name
            .chars()
            .any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("服务器名称不能为空，且不能包含空白或控制字符".into());
    }
    if format == Format::Deepseek
        && (entry.name.len() > 32
            || !entry
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
    {
        return Err("DeepSeek 服务器名称仅允许 1–32 个英文字母、数字、下划线或连字符".into());
    }

    let transport = entry.transport.trim();
    entry.transport = match transport {
        "stdio" => "stdio".into(),
        "http" => {
            if format == Format::Deepseek {
                "streamable-http".into()
            } else {
                "http".into()
            }
        }
        "streamable-http" => {
            if format == Format::Deepseek {
                "streamable-http".into()
            } else {
                "http".into()
            }
        }
        "sse" => {
            if format == Format::Deepseek {
                return Err("DeepSeek 仅支持 stdio 或 streamable-http 传输方式".into());
            }
            "sse".into()
        }
        _ => return Err("不支持的传输方式，请选择 stdio 或 HTTP".into()),
    };

    entry.command = clean_opt(entry.command.as_deref());
    entry.url = clean_opt(entry.url.as_deref());
    entry.cwd = clean_opt(entry.cwd.as_deref());
    entry.args = entry
        .args
        .iter()
        .map(|arg| arg.trim().to_owned())
        .filter(|arg| !arg.is_empty())
        .collect();

    if entry.transport == "stdio" {
        let Some(cmd) = entry.command.as_deref() else {
            return Err("本地进程模式必须填写启动命令".into());
        };
        if cmd.chars().any(char::is_control) {
            return Err("启动命令不能包含控制字符".into());
        }
        if entry
            .args
            .iter()
            .any(|arg| arg.chars().any(char::is_control))
        {
            return Err("命令参数不能包含控制字符".into());
        }
        if entry
            .cwd
            .as_deref()
            .is_some_and(|cwd| cwd.chars().any(char::is_control))
        {
            return Err("工作目录不能包含控制字符".into());
        }
    } else {
        let Some(url) = entry.url.as_deref() else {
            return Err("远端服务模式必须填写服务地址（URL）".into());
        };
        if (!url.starts_with("http://") && !url.starts_with("https://"))
            || url.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("服务地址必须是以 http:// 或 https:// 开头的有效 URL".into());
        }
    }

    let mut clean_env = BTreeMap::new();
    for (k, v) in &entry.env {
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        if key.contains('=') || key.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(format!("环境变量名无效：{key}"));
        }
        if v.chars().any(char::is_control) {
            return Err(format!("环境变量 {key} 的值不能包含控制字符"));
        }
        clean_env.insert(key.to_owned(), v.clone());
    }
    entry.env = clean_env;

    let mut clean_headers = BTreeMap::new();
    for (k, v) in &entry.headers {
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        if key.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(format!("请求头名称无效：{key}"));
        }
        if v.chars().any(char::is_control) {
            return Err(format!("请求头 {key} 的值不能包含控制字符"));
        }
        clean_headers.insert(key.to_owned(), v.clone());
    }
    entry.headers = clean_headers;
    Ok(())
}

fn upsert_server_in_draft(
    format: Format,
    provider: &str,
    draft: &str,
    original_name: Option<&str>,
    mut entry: McpServerEntry,
) -> Result<String, String> {
    validate_entry(format, &mut entry)?;
    let original_name = original_name.map(str::trim).filter(|s| !s.is_empty());

    match format {
        Format::Toml => {
            let mut doc = parse_toml(draft)?;
            if doc.get("mcp_servers").is_none() {
                doc.insert("mcp_servers", Item::Table(Table::new()));
            }
            let servers = doc
                .get_mut("mcp_servers")
                .and_then(Item::as_table_mut)
                .ok_or("mcp_servers 必须是表")?;
            let is_rename = original_name.is_some_and(|old| old != entry.name);
            if (original_name.is_none() || is_rename) && servers.contains_key(&entry.name) {
                return Err(format!("已存在同名 MCP 服务器：{}", entry.name));
            }
            let mut table = original_name
                .and_then(|old| servers.remove(old))
                .and_then(|item| item.into_table().ok())
                .unwrap_or_default();

            if entry.transport == "stdio" {
                table.remove("url");
                table.remove("http_headers");
                table.remove("headers");
                table.remove("transport");
                table.remove("type");
                if let Some(cmd) = entry.command.as_deref() {
                    table.insert("command", toml_edit::value(cmd));
                }
                if entry.args.is_empty() {
                    table.remove("args");
                } else {
                    let mut arr = Array::new();
                    for arg in &entry.args {
                        arr.push(arg.as_str());
                    }
                    table.insert("args", Item::Value(TomlValue::Array(arr)));
                }
                if let Some(cwd) = entry.cwd.as_deref() {
                    table.insert("cwd", toml_edit::value(cwd));
                } else {
                    table.remove("cwd");
                }
                if entry.env.is_empty() {
                    table.remove("env");
                } else {
                    let mut env_tbl = Table::new();
                    for (k, v) in &entry.env {
                        env_tbl.insert(k, toml_edit::value(v.as_str()));
                    }
                    table.insert("env", Item::Table(env_tbl));
                }
            } else {
                table.remove("command");
                table.remove("args");
                table.remove("cwd");
                table.remove("env");
                if let Some(url) = entry.url.as_deref() {
                    table.insert("url", toml_edit::value(url));
                }
                if entry.transport == "sse" {
                    table.insert("type", toml_edit::value("sse"));
                } else {
                    table.remove("type");
                    table.remove("transport");
                }
                if entry.headers.is_empty() {
                    table.remove("http_headers");
                    table.remove("headers");
                } else {
                    table.remove("headers");
                    let mut hdr_tbl = Table::new();
                    for (k, v) in &entry.headers {
                        hdr_tbl.insert(k, toml_edit::value(v.as_str()));
                    }
                    table.insert("http_headers", Item::Table(hdr_tbl));
                }
            }

            table.remove("disabled");
            if !entry.enabled {
                table.insert("enabled", toml_edit::value(false));
            } else if table.contains_key("enabled") {
                table.insert("enabled", toml_edit::value(true));
            }

            servers.insert(&entry.name, Item::Table(table));
            Ok(doc.to_string())
        }
        Format::Json | Format::Deepseek => {
            let mut doc = parse_json(draft)?;
            let root = doc.as_object_mut().ok_or("配置必须是 JSON 对象")?;
            if !root.contains_key("mcpServers") {
                root.insert("mcpServers".into(), Value::Object(Map::new()));
            }
            let servers = root
                .get_mut("mcpServers")
                .and_then(Value::as_object_mut)
                .ok_or("mcpServers 必须是对象")?;
            let is_rename = original_name.is_some_and(|old| old != entry.name);
            if (original_name.is_none() || is_rename) && servers.contains_key(&entry.name) {
                return Err(format!("已存在同名 MCP 服务器：{}", entry.name));
            }
            let mut obj = original_name
                .and_then(|old| servers.remove(old))
                .and_then(|val| match val {
                    Value::Object(map) => Some(map),
                    _ => None,
                })
                .unwrap_or_default();

            let used_server_url = obj.contains_key("serverUrl") && !obj.contains_key("url");

            if entry.transport == "stdio" {
                obj.remove("url");
                obj.remove("serverUrl");
                obj.remove("headers");
                if format == Format::Deepseek {
                    obj.remove("type");
                    obj.insert("transport".into(), Value::String("stdio".into()));
                } else if provider == "claude" || obj.contains_key("type") {
                    obj.remove("transport");
                    obj.insert("type".into(), Value::String("stdio".into()));
                } else {
                    obj.remove("transport");
                    obj.remove("type");
                }
                if let Some(cmd) = entry.command.as_deref() {
                    obj.insert("command".into(), Value::String(cmd.to_owned()));
                }
                if entry.args.is_empty() {
                    obj.remove("args");
                } else {
                    obj.insert(
                        "args".into(),
                        Value::Array(entry.args.iter().cloned().map(Value::String).collect()),
                    );
                }
                if let Some(cwd) = entry.cwd.as_deref() {
                    obj.insert("cwd".into(), Value::String(cwd.to_owned()));
                } else {
                    obj.remove("cwd");
                }
                if entry.env.is_empty() {
                    obj.remove("env");
                } else {
                    obj.insert(
                        "env".into(),
                        Value::Object(
                            entry
                                .env
                                .iter()
                                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                                .collect(),
                        ),
                    );
                }
            } else {
                obj.remove("command");
                obj.remove("args");
                obj.remove("cwd");
                obj.remove("env");
                if format == Format::Deepseek {
                    obj.remove("type");
                    obj.remove("serverUrl");
                    obj.insert("transport".into(), Value::String("streamable-http".into()));
                    if let Some(url) = entry.url.as_deref() {
                        obj.insert("url".into(), Value::String(url.to_owned()));
                    }
                } else {
                    obj.remove("transport");
                    if provider == "claude" || entry.transport == "sse" || obj.contains_key("type")
                    {
                        obj.insert("type".into(), Value::String(entry.transport.clone()));
                    }
                    if let Some(url) = entry.url.as_deref() {
                        if used_server_url && provider == "antigravity" {
                            obj.remove("url");
                            obj.insert("serverUrl".into(), Value::String(url.to_owned()));
                        } else {
                            obj.remove("serverUrl");
                            obj.insert("url".into(), Value::String(url.to_owned()));
                        }
                    }
                }
                if entry.headers.is_empty() {
                    obj.remove("headers");
                } else {
                    obj.insert(
                        "headers".into(),
                        Value::Object(
                            entry
                                .headers
                                .iter()
                                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                                .collect(),
                        ),
                    );
                }
            }

            if format == Format::Deepseek {
                obj.remove("disabled");
                if !entry.enabled {
                    obj.insert("enabled".into(), Value::Bool(false));
                } else {
                    obj.remove("enabled");
                }
            } else if obj.contains_key("enabled") {
                obj.remove("disabled");
                obj.insert("enabled".into(), Value::Bool(entry.enabled));
            } else {
                obj.remove("enabled");
                if !entry.enabled {
                    obj.insert("disabled".into(), Value::Bool(true));
                } else {
                    obj.remove("disabled");
                }
            }

            servers.insert(entry.name, Value::Object(obj));
            serde_json::to_string_pretty(&doc).map_err(|_| "无法编码 MCP 配置".into())
        }
    }
}

fn delete_server_in_draft(format: Format, draft: &str, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("缺少要删除的服务器名称".into());
    }
    match format {
        Format::Toml => {
            let mut doc = parse_toml(draft)?;
            if let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_mut) {
                servers.remove(name);
            }
            Ok(doc.to_string())
        }
        Format::Json | Format::Deepseek => {
            let mut doc = parse_json(draft)?;
            if let Some(servers) = doc.get_mut("mcpServers").and_then(Value::as_object_mut) {
                servers.remove(name);
            }
            serde_json::to_string_pretty(&doc).map_err(|_| "无法编码 MCP 配置".into())
        }
    }
}

fn revision(path: &Path, raw: Option<&str>) -> String {
    let mut hash = Sha256::new();
    hash.update(path.as_os_str().as_encoded_bytes());
    hash.update([0, u8::from(raw.is_some())]);
    hash.update(raw.unwrap_or_default());
    format!("{:x}", hash.finalize())
}

fn parse_toml(text: &str) -> Result<DocumentMut, String> {
    text.trim_start_matches('\u{feff}')
        .parse()
        .map_err(|_| "TOML 格式无效；请检查原文件或编辑内容".into())
}

pub(super) fn parse_json(text: &str) -> Result<Value, String> {
    serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|error| {
        format!(
            "JSON 格式无效（第 {} 行，第 {} 列）；请检查原文件或编辑内容",
            error.line(),
            error.column()
        )
    })
}

fn merge(format: Format, raw: Option<&str>, draft: &str) -> Result<String, String> {
    if draft.len() > 1024 * 1024 {
        return Err("MCP 配置超过 1 MiB".into());
    }
    match format {
        Format::Deepseek => Err("缺少 DeepSeek 配置档案".into()),
        Format::Toml => {
            let mut edited = parse_toml(draft)?;
            let servers = edited
                .remove("mcp_servers")
                .ok_or("请保留 mcp_servers 表")?;
            if !edited.is_empty()
                || servers.as_table_like().is_none_or(|table| {
                    table
                        .iter()
                        .any(|(_, server)| server.as_table_like().is_none())
                })
            {
                return Err("只能编辑 mcp_servers 表，每个服务必须是独立的表".into());
            }
            let mut existing = parse_toml(raw.unwrap_or_default())?;
            existing.insert("mcp_servers", servers);
            Ok(existing.to_string())
        }
        Format::Json => {
            let mut edited = parse_json(draft)?;
            let edited = edited.as_object_mut().ok_or("配置必须是 JSON 对象")?;
            let servers = edited
                .remove("mcpServers")
                .ok_or("请保留 mcpServers 对象")?;
            if !edited.is_empty()
                || servers
                    .as_object()
                    .is_none_or(|map| map.values().any(|server| !server.is_object()))
            {
                return Err("只能编辑 mcpServers，每个服务必须是 JSON 对象".into());
            }
            let mut existing = parse_json(raw.unwrap_or("{}"))?;
            existing
                .as_object_mut()
                .ok_or("原配置必须是 JSON 对象")?
                .insert("mcpServers".into(), servers);
            serde_json::to_string_pretty(&existing)
                .map(|text| text + "\n")
                .map_err(|_| "无法编码 MCP 配置".into())
        }
    }
}

#[tauri::command]
pub(crate) async fn read_mcp_config(
    provider: String,
    profile: Option<String>,
) -> Result<McpView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = Target::for_provider(&provider, profile.as_deref())?;
        let raw = read_path(&target.root()?, &target.path)?;
        target.view(raw.as_deref())
    })
    .await
    .map_err(|_| "读取 MCP 配置失败")?
}

#[tauri::command]
pub(crate) async fn save_mcp_config(
    provider: String,
    profile: Option<String>,
    expected: String,
    draft: String,
) -> Result<McpView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = Target::for_provider(&provider, profile.as_deref())?;
        let root = target.root()?;
        let raw = read_path(&root, &target.path)?;
        if expected != revision(&target.path, raw.as_deref()) {
            return Err("配置已在外部修改，请重新读取后再保存".into());
        }
        let next = if let Some(overlay) = &target.overlay {
            if draft.len() > 1024 * 1024 {
                return Err("MCP 配置超过 1 MiB".into());
            }
            let home = read_path(&root, overlay)?;
            super::dsh_mcp::merge(
                raw.as_deref().unwrap_or_default(),
                home.as_deref().unwrap_or_default(),
                &draft,
            )?
        } else {
            merge(target.format, raw.as_deref(), &draft)?
        };
        let view = target.view(Some(&next))?;
        save_path(&root, &target.path, raw, next)?;
        Ok(view)
    })
    .await
    .map_err(|_| "保存 MCP 配置失败")?
}

fn apply_draft(
    target: &Target,
    root: &Root,
    raw: Option<&str>,
    draft: &str,
) -> Result<String, String> {
    if let Some(overlay) = &target.overlay {
        if draft.len() > 1024 * 1024 {
            return Err("MCP 配置超过 1 MiB".into());
        }
        let home = read_path(root, overlay)?;
        super::dsh_mcp::merge(
            raw.unwrap_or_default(),
            home.as_deref().unwrap_or_default(),
            draft,
        )
    } else {
        merge(target.format, raw, draft)
    }
}

#[tauri::command]
pub(crate) async fn upsert_mcp_server(
    provider: String,
    profile: Option<String>,
    expected: String,
    original_name: Option<String>,
    entry: McpServerEntry,
) -> Result<McpView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = Target::for_provider(&provider, profile.as_deref())?;
        let root = target.root()?;
        let raw = read_path(&root, &target.path)?;
        if expected != revision(&target.path, raw.as_deref()) {
            return Err("配置已在外部修改，请重新读取后再保存".into());
        }
        let current = target.view(raw.as_deref())?;
        let updated_draft = upsert_server_in_draft(
            target.format,
            &target.provider,
            &current.draft,
            original_name.as_deref(),
            entry,
        )?;
        let next = apply_draft(&target, &root, raw.as_deref(), &updated_draft)?;
        let view = target.view(Some(&next))?;
        save_path(&root, &target.path, raw, next)?;
        Ok(view)
    })
    .await
    .map_err(|_| "保存 MCP 服务器失败")?
}

#[tauri::command]
pub(crate) async fn delete_mcp_server(
    provider: String,
    profile: Option<String>,
    expected: String,
    name: String,
) -> Result<McpView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = Target::for_provider(&provider, profile.as_deref())?;
        let root = target.root()?;
        let raw = read_path(&root, &target.path)?;
        if expected != revision(&target.path, raw.as_deref()) {
            return Err("配置已在外部修改，请重新读取后再保存".into());
        }
        let current = target.view(raw.as_deref())?;
        let updated_draft = delete_server_in_draft(target.format, &current.draft, &name)?;
        let next = apply_draft(&target, &root, raw.as_deref(), &updated_draft)?;
        let view = target.view(Some(&next))?;
        save_path(&root, &target.path, raw, next)?;
        Ok(view)
    })
    .await
    .map_err(|_| "删除 MCP 服务器失败")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_only_the_mcp_section() {
        let raw = "# preferences\nmodel = \"test\"\n\n[mcp_servers.old]\ncommand = \"old\"\n\n[other]\nkey = true\n";
        let next = merge(
            Format::Toml,
            Some(raw),
            "[mcp_servers.new]\ncommand = \"new\"\n",
        )
        .unwrap();
        assert!(next.contains("# preferences\nmodel = \"test\""));
        let doc = parse_toml(&next).unwrap();
        assert!(doc["other"]["key"].as_bool().unwrap());
        assert!(doc["mcp_servers"].get("old").is_none());
        assert_eq!(doc["mcp_servers"]["new"]["command"].as_str(), Some("new"));
        let next = merge(
            Format::Json,
            Some(r#"{"account":{"id":"test"},"mcpServers":{"old":{}}}"#),
            r#"{"mcpServers":{"new":{"url":"https://example.invalid/mcp"}}}"#,
        )
        .unwrap();
        assert_eq!(parse_json(&next).unwrap()["account"]["id"], "test");
        assert!(merge(Format::Json, None, r#"{"account":{},"mcpServers":{}}"#).is_err());
    }

    #[test]
    fn structured_upsert_extract_and_delete_preserves_extra_fields() {
        let toml_draft = "[mcp_servers.fs]\ncommand = \"npx\"\nargs = [\"-y\", \"server-fs\"]\nstartup_timeout_sec = 30\n";
        let servers = extract_servers(Format::Toml, toml_draft).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "fs");
        assert!(servers[0].has_extra_fields);

        let mut updated = servers[0].clone();
        updated.args.push("/tmp".into());
        updated.env.insert("API_KEY".into(), "secret".into());
        updated.enabled = false;
        let next_toml =
            upsert_server_in_draft(Format::Toml, "codex", toml_draft, Some("fs"), updated).unwrap();
        let doc = parse_toml(&next_toml).unwrap();
        assert_eq!(
            doc["mcp_servers"]["fs"]["startup_timeout_sec"].as_integer(),
            Some(30)
        );
        assert_eq!(doc["mcp_servers"]["fs"]["enabled"].as_bool(), Some(false));
        assert_eq!(
            doc["mcp_servers"]["fs"]["env"]["API_KEY"].as_str(),
            Some("secret")
        );

        // Duplicate name rejected when adding new
        let dup = McpServerEntry {
            name: "fs".into(),
            transport: "stdio".into(),
            command: Some("node".into()),
            args: vec![],
            url: None,
            env: BTreeMap::new(),
            headers: BTreeMap::new(),
            cwd: None,
            enabled: true,
            has_extra_fields: false,
        };
        assert!(upsert_server_in_draft(Format::Toml, "codex", &next_toml, None, dup).is_err());

        // Remote HTTP server in JSON + DeepSeek round-trip
        let remote = McpServerEntry {
            name: "docs".into(),
            transport: "http".into(),
            command: None,
            args: vec![],
            url: Some("https://example.invalid/mcp".into()),
            env: BTreeMap::new(),
            headers: BTreeMap::from([("Authorization".into(), "Bearer tok".into())]),
            cwd: None,
            enabled: true,
            has_extra_fields: false,
        };
        let dsh_draft = upsert_server_in_draft(
            Format::Deepseek,
            "deepseek",
            r#"{"mcpServers":{}}"#,
            None,
            remote.clone(),
        )
        .unwrap();
        let dsh_servers = extract_servers(Format::Deepseek, &dsh_draft).unwrap();
        assert_eq!(dsh_servers.len(), 1);
        assert_eq!(dsh_servers[0].transport, "streamable-http");
        assert_eq!(
            dsh_servers[0].url.as_deref(),
            Some("https://example.invalid/mcp")
        );

        let after_delete = delete_server_in_draft(Format::Deepseek, &dsh_draft, "docs").unwrap();
        assert!(extract_servers(Format::Deepseek, &after_delete)
            .unwrap()
            .is_empty());
    }
}
