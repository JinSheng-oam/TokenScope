//! OpenAI-compatible AI assistant module for fetching models, URL auto-completion,
//! and generating structured Chinese summaries & usage guides for Agent skills.

use crate::agent_customizations::{resolve, CustomizationId};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

pub(crate) const SUMMARY_FILE: &str = ".tokenscope-ai-summary.json";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub(crate) struct AiConfig {
    pub(crate) api_url: String,
    pub(crate) api_key: String,
    pub(crate) model: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct SkillSummary {
    pub(crate) skill_name: String,
    #[serde(default)]
    pub(crate) brief: String,
    pub(crate) overview: String,
    #[serde(default)]
    pub(crate) scenarios: String,
    pub(crate) usage_guide: String,
    pub(crate) raw_response: String,
    pub(crate) model: String,
    pub(crate) updated_at_ms: i64,
}

fn shared_http_client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn normalize_api_url(input: &str) -> String {
    let mut s = input.trim();
    if s.is_empty() {
        return String::new();
    }
    s = s
        .strip_suffix("/chat/completions")
        .unwrap_or(s)
        .trim_end_matches('/');
    s = s.strip_suffix("/models").unwrap_or(s).trim_end_matches('/');

    let lower = s.to_ascii_lowercase();

    if lower == "openai" || lower == "api.openai.com" || lower == "https://api.openai.com" {
        return "https://api.openai.com/v1".into();
    }
    if lower == "deepseek" || lower == "api.deepseek.com" || lower == "https://api.deepseek.com" {
        return "https://api.deepseek.com/v1".into();
    }
    if lower == "siliconflow"
        || lower == "api.siliconflow.cn"
        || lower == "https://api.siliconflow.cn"
    {
        return "https://api.siliconflow.cn/v1".into();
    }
    if lower == "dashscope"
        || lower == "dashscope.aliyuncs.com"
        || lower == "https://dashscope.aliyuncs.com"
    {
        return "https://dashscope.aliyuncs.com/compatible-mode/v1".into();
    }
    if lower == "openrouter"
        || lower == "openrouter.ai"
        || lower == "https://openrouter.ai"
        || lower == "https://openrouter.ai/api"
    {
        return "https://openrouter.ai/api/v1".into();
    }
    if lower == "groq" || lower == "api.groq.com" || lower == "https://api.groq.com" {
        return "https://api.groq.com/openai/v1".into();
    }
    if lower == "moonshot" || lower == "api.moonshot.cn" || lower == "https://api.moonshot.cn" {
        return "https://api.moonshot.cn/v1".into();
    }
    if lower == "zhipu" || lower == "open.bigmodel.cn" || lower == "https://open.bigmodel.cn" {
        return "https://open.bigmodel.cn/api/paas/v4".into();
    }
    if lower == "ollama" || lower == "localhost:11434" || lower == "http://localhost:11434" {
        return "http://localhost:11434/v1".into();
    }
    if lower == "lmstudio" || lower == "localhost:1234" || lower == "http://localhost:1234" {
        return "http://localhost:1234/v1".into();
    }

    let with_scheme = if !s.starts_with("http://") && !s.starts_with("https://") {
        if s.starts_with("localhost") || s.starts_with("127.0.0.1") {
            format!("http://{s}")
        } else {
            format!("https://{s}")
        }
    } else {
        s.to_string()
    };

    let trimmed = with_scheme.trim_end_matches('/');
    if let Ok(parsed) = Url::parse(trimmed) {
        let path = parsed.path().trim_end_matches('/');
        if path.is_empty() {
            return format!("{trimmed}/v1");
        }
    }
    trimmed.to_string()
}

fn ai_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录：{e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
    Ok(dir.join("ai-config.json"))
}

pub(crate) fn read_ai_config(app: &AppHandle) -> Result<AiConfig, String> {
    let path = ai_config_path(app)?;
    if !path.is_file() {
        return Ok(AiConfig::default());
    }
    let data = fs::read(&path).map_err(|e| format!("读取 AI 配置失败：{e}"))?;
    serde_json::from_slice(&data).map_err(|e| format!("解析 AI 配置失败：{e}"))
}

pub(crate) fn write_ai_config(app: &AppHandle, mut config: AiConfig) -> Result<(), String> {
    config.api_url = normalize_api_url(&config.api_url);
    let path = ai_config_path(app)?;
    let data = serde_json::to_vec_pretty(&config).map_err(|e| format!("序列化配置失败：{e}"))?;
    fs::write(&path, data).map_err(|e| format!("保存 AI 配置失败：{e}"))
}

pub(crate) async fn fetch_models(api_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let base = normalize_api_url(api_url);
    if base.is_empty() {
        return Err("请输入 API 地址".into());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))?;

    let endpoint = format!("{base}/models");
    let mut req = client.get(&endpoint);
    if !api_key.trim().is_empty() {
        req = req.bearer_auth(api_key.trim());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("连接 API 失败（{}）：{e}", endpoint))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("拉取模型失败 (HTTP {status})：{body}"));
    }

    let val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析模型列表 JSON 失败：{e}"))?;

    let mut models = Vec::new();
    if let Some(data) = val.get("data").and_then(|v| v.as_array()) {
        for item in data {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            }
        }
    } else if let Some(arr) = val.as_array() {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            } else if let Some(s) = item.as_str() {
                models.push(s.to_string());
            }
        }
    } else if let Some(arr) = val.get("models").and_then(|v| v.as_array()) {
        for item in arr {
            if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                models.push(id.to_string());
            } else if let Some(s) = item.as_str() {
                models.push(s.to_string());
            }
        }
    }

    if models.is_empty() {
        return Err("接口未返回任何可用模型标识".into());
    }

    models.sort();
    models.dedup();
    Ok(models)
}

pub(crate) fn read_cached_summary_in_dir(dir: &Path) -> Option<SkillSummary> {
    let summary_path = dir.join(SUMMARY_FILE);
    if !summary_path.is_file() {
        return None;
    }
    let data = fs::read(&summary_path).ok()?;
    serde_json::from_slice(&data).ok()
}

pub(crate) fn get_cached_summary(id: &CustomizationId) -> Result<Option<SkillSummary>, String> {
    let (_, path) = resolve(id)?;
    let dir = path.parent().ok_or("无效的 skill 路径")?;
    let summary_path = dir.join(SUMMARY_FILE);
    if !summary_path.is_file() {
        return Ok(None);
    }
    let data = fs::read(&summary_path).map_err(|e| format!("读取摘要缓存失败：{e}"))?;
    serde_json::from_slice(&data)
        .map(Some)
        .map_err(|e| format!("解析摘要缓存失败：{e}"))
}

fn clean_brief_line(raw: &str) -> String {
    let first_line = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    first_line
        .trim_start_matches(['-', '*', '•', '>', ' '])
        .replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .trim()
        .trim_matches(['"', '“', '”'])
        .to_string()
}

pub(crate) fn parse_summary_sections(raw: &str) -> (String, String, String, String) {
    let mut current: Option<&str> = None;
    let mut brief_lines = Vec::new();
    let mut overview_lines = Vec::new();
    let mut scenario_lines = Vec::new();
    let mut usage_lines = Vec::new();

    let normalized = raw.replace("\r\n", "\n");
    for line in normalized.lines() {
        let trimmed = line.trim();
        let heading_text = trimmed.trim_start_matches('#').trim();
        let is_heading = trimmed.starts_with('#') && !heading_text.is_empty();
        if is_heading {
            if heading_text.contains("一句话简介") || heading_text.contains("核心简介") {
                current = Some("brief");
                continue;
            }
            if heading_text.contains("功能定位") {
                current = Some("overview");
                continue;
            }
            if heading_text.contains("适用场景") || heading_text.contains("触发时机") {
                current = Some("scenarios");
                continue;
            }
            if heading_text.contains("使用方式") || heading_text.contains("调用指南") {
                current = Some("usage");
                continue;
            }
        }
        match current {
            Some("brief") => brief_lines.push(line),
            Some("overview") => overview_lines.push(line),
            Some("scenarios") => scenario_lines.push(line),
            Some("usage") => usage_lines.push(line),
            None => overview_lines.push(line),
            _ => {}
        }
    }

    let brief = clean_brief_line(&brief_lines.join("\n"));
    let overview = overview_lines.join("\n").trim().to_string();
    let scenarios = scenario_lines.join("\n").trim().to_string();
    let usage_guide = usage_lines.join("\n").trim().to_string();

    if overview.is_empty() && usage_guide.is_empty() && scenarios.is_empty() && brief.is_empty() {
        (String::new(), raw.trim().to_string(), String::new(), String::new())
    } else {
        (brief, overview, scenarios, usage_guide)
    }
}

pub(crate) async fn analyze_skill(
    app: AppHandle,
    id: CustomizationId,
) -> Result<SkillSummary, String> {
    let cfg = read_ai_config(&app)?;
    let base = normalize_api_url(&cfg.api_url);
    if base.is_empty() {
        return Err("未配置 AI API 地址，请在右上角【AI 助手配置】中配置".into());
    }
    if cfg.model.trim().is_empty() {
        return Err("未配置 AI 模型，请在右上角【AI 助手配置】中选择模型".into());
    }

    let (_, path) = resolve(&id)?;
    let dir = path.parent().ok_or("无效的 skill 路径")?.to_path_buf();
    let content = if path.is_file() {
        fs::read_to_string(&path).map_err(|e| format!("读取 SKILL.md 失败：{e}"))?
    } else {
        return Err("找不到 SKILL.md 文件".into());
    };

    let truncated_content: String = content.chars().take(8000).collect();
    let client = shared_http_client()?;

    let system_prompt = "你是一位精通各类 AI 编码助手（如 Claude Code, Codex, Antigravity, Pi Agent, DeepSeek Harness 等）的资深架构师与技能分析专家。用户会提供一份 Agent Skill 的定义文件（SKILL.md）。请仔细阅读并进行专业分析，输出结构化的中文摘要。\n\n请严格按照以下四部分输出：\n\n### ✨ 一句话简介\n用 18~42 字的一句精炼中文，直接概括这个 Skill 是干什么的（不要使用 Markdown 粗体、列表符号或多余前缀，适合展示在技能卡片上）。\n\n### 💡 功能定位\n用精炼的 2~3 句话明确总结这个 Skill 的核心能力、解决了什么痛点、以及能力边界。\n\n### 🎯 适用场景\n用 2~4 条简洁要点说明它在哪些开发任务或问题场景下应该被触发或主动调用。\n\n### 🚀 使用方式与调用指南\n说明如何向 AI Agent 下达调用指令，并附 1~2 条简短实用的调用 Prompt 示例。";

    let messages = serde_json::json!([
        { "role": "system", "content": system_prompt },
        { "role": "user", "content": format!("请分析以下 Skill (名称: {}):\n\n```markdown\n{}\n```", id.name, truncated_content) }
    ]);

    let body = serde_json::json!({
        "model": cfg.model.trim(),
        "messages": messages,
        "temperature": 0.3
    });

    let endpoint = format!("{base}/chat/completions");
    let mut req = client.post(&endpoint).json(&body);
    if !cfg.api_key.trim().is_empty() {
        req = req.bearer_auth(cfg.api_key.trim());
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("请求 AI 接口失败：{e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("AI 接口返回错误 (HTTP {status})：{err_text}"));
    }

    let result_json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("解析 AI 响应失败：{e}"))?;
    let assistant_content = result_json
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "AI 响应中未包含有效文本内容".to_string())?
        .trim();

    let (brief, overview, scenarios, usage_guide) = parse_summary_sections(assistant_content);

    let summary = SkillSummary {
        skill_name: id.name.clone(),
        brief,
        overview,
        scenarios,
        usage_guide,
        raw_response: assistant_content.to_string(),
        model: cfg.model.clone(),
        updated_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    };

    if let Ok(data) = serde_json::to_vec_pretty(&summary) {
        let _ = fs::write(dir.join(SUMMARY_FILE), data);
    }

    Ok(summary)
}

#[tauri::command]
pub(crate) async fn get_ai_config(app: AppHandle) -> Result<AiConfig, String> {
    read_ai_config(&app)
}

#[tauri::command]
pub(crate) async fn save_ai_config(app: AppHandle, config: AiConfig) -> Result<(), String> {
    write_ai_config(&app, config)
}

#[tauri::command]
pub(crate) async fn fetch_ai_models(
    api_url: String,
    api_key: String,
) -> Result<Vec<String>, String> {
    fetch_models(&api_url, &api_key).await
}

#[tauri::command]
pub(crate) async fn get_skill_summary(id: CustomizationId) -> Result<Option<SkillSummary>, String> {
    get_cached_summary(&id)
}

#[tauri::command]
pub(crate) async fn generate_skill_summary(
    app: AppHandle,
    id: CustomizationId,
) -> Result<SkillSummary, String> {
    analyze_skill(app, id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_various_api_urls() {
        assert_eq!(
            normalize_api_url("api.openai.com"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_api_url("https://api.openai.com"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_api_url("api.deepseek.com"),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            normalize_api_url("https://dashscope.aliyuncs.com"),
            "https://dashscope.aliyuncs.com/compatible-mode/v1"
        );
        assert_eq!(
            normalize_api_url("https://api.deepseek.com/v1/chat/completions"),
            "https://api.deepseek.com/v1"
        );
        assert_eq!(
            normalize_api_url("localhost:11434"),
            "http://localhost:11434/v1"
        );
        assert_eq!(
            normalize_api_url("https://my-proxy.com/api"),
            "https://my-proxy.com/api"
        );
    }

    #[test]
    fn parses_four_section_summary_and_legacy_json() {
        let raw = "### ✨ 一句话简介\n**用深模块视角**评审和重构代码接口边界。\n\n### 💡 功能定位\n建立统一架构术语。\n\n### 🎯 适用场景\n- 接口评审\n- 重构拆界\n\n### 🚀 使用方式与调用指南\n```text\n用深模块原则重构\n```";
        let (brief, overview, scenarios, usage) = parse_summary_sections(raw);
        assert_eq!(brief, "用深模块视角评审和重构代码接口边界。");
        assert_eq!(overview, "建立统一架构术语。");
        assert!(scenarios.contains("接口评审"));
        assert!(usage.contains("用深模块原则重构"));

        let legacy = r#"{"skill_name":"tdd","overview":"测试驱动开发","usage_guide":"先写测试","raw_response":"raw","model":"gpt-4o","updated_at_ms":123}"#;
        let parsed: SkillSummary = serde_json::from_str(legacy).expect("legacy json parses");
        assert_eq!(parsed.brief, "");
        assert_eq!(parsed.scenarios, "");
        assert_eq!(parsed.overview, "测试驱动开发");
    }
}
