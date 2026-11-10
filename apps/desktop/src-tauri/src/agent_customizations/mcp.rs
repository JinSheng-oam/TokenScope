use super::{display_path, is_link, read_path, save_path, Root};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use toml_edit::DocumentMut;

#[derive(Clone, Copy)]
enum Format {
    Toml,
    Json,
}

struct Target {
    path: PathBuf,
    format: Format,
}

#[derive(Serialize)]
pub(crate) struct McpView {
    path: String,
    format: &'static str,
    draft: String,
    revision: String,
}

impl Target {
    fn for_provider(provider: &str) -> Result<Self, String> {
        use super::paths::{configured, home};
        let home = home()?;
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
            _ => return Err("此 Agent 请在其原生设置中管理 MCP".into()),
        };
        if !path.is_absolute() || path.ancestors().any(is_link) {
            return Err("MCP 配置路径必须是本机绝对路径，且不能包含符号链接".into());
        }
        Ok(Self { path, format })
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
        Ok(McpView {
            path: display_path(&self.path),
            format,
            draft,
            revision: revision(&self.path, raw),
        })
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

fn parse_json(text: &str) -> Result<Value, String> {
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
pub(crate) async fn read_mcp_config(provider: String) -> Result<McpView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = Target::for_provider(&provider)?;
        let raw = read_path(&target.root()?, &target.path)?;
        target.view(raw.as_deref())
    })
    .await
    .map_err(|_| "读取 MCP 配置失败")?
}

#[tauri::command]
pub(crate) async fn save_mcp_config(
    provider: String,
    expected: String,
    draft: String,
) -> Result<McpView, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let target = Target::for_provider(&provider)?;
        let root = target.root()?;
        let raw = read_path(&root, &target.path)?;
        if expected != revision(&target.path, raw.as_deref()) {
            return Err("配置已在外部修改，请重新读取后再保存".into());
        }
        let next = merge(target.format, raw.as_deref(), &draft)?;
        let view = target.view(Some(&next))?;
        save_path(&root, &target.path, raw, next)?;
        Ok(view)
    })
    .await
    .map_err(|_| "保存 MCP 配置失败")?
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
}
