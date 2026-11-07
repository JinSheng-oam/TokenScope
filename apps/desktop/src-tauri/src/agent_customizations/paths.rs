use super::{CustomizationId, Root};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn home() -> Result<PathBuf, String> {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .ok_or_else(|| "无法确定用户目录".into())
}
fn configured(var: &str, fallback: PathBuf) -> PathBuf {
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or(fallback)
}
pub(crate) fn roots(provider: &str) -> Result<Vec<Root>, String> {
    let h = home()?;
    let shared = configured("DSH_AGENTS_HOME", h.join(".agents"));
    let codex = configured("CODEX_HOME", h.join(".codex"));
    let claude = configured("CLAUDE_CONFIG_DIR", h.join(".claude"));
    let gemini = h.join(".gemini");
    let dsh = configured("DSH_HOME", h.join(".dsh"));
    let skill = |id, path: PathBuf| Root {
        id,
        path,
        kind: "skill",
        names: &[],
    };
    let instructions = |id, path: PathBuf, names| Root {
        id,
        path,
        kind: "instruction",
        names,
    };
    Ok(match provider {
        "codex" => vec![
            skill("shared-skills", h.join(".agents/skills")),
            skill("codex-skills", codex.join("skills")),
            instructions(
                "codex-instructions",
                codex,
                &["AGENTS.md", "AGENTS.override.md"],
            ),
        ],
        "claude" => vec![
            skill("claude-skills", claude.join("skills")),
            instructions("claude-instructions", claude, &["CLAUDE.md"]),
        ],
        "antigravity" => vec![
            skill("antigravity-skills", gemini.join("config/skills")),
            instructions(
                "gemini-instructions",
                gemini.clone(),
                &["AGENTS.md", "GEMINI.md"],
            ),
            instructions(
                "gemini-config-instructions",
                gemini.join("config"),
                &["AGENTS.md", "GEMINI.md"],
            ),
        ],
        "deepseek" => vec![
            skill("dsh-skills", dsh.join("skills")),
            skill("dsh-shared-skills", shared.join("skills")),
            instructions("dsh-instructions", dsh, &["AGENTS.md"]),
            instructions("dsh-shared-instructions", shared, &["AGENTS.md"]),
        ],
        _ => return Err("不支持的工具".into()),
    })
}
pub(crate) fn valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != ".system"
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
pub(crate) fn valid_instruction_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 128 {
        return false;
    }
    if !trimmed.to_ascii_lowercase().ends_with(".md") {
        return false;
    }
    let stem = &trimmed[..trimmed.len() - 3];
    if stem.is_empty() || stem == "." || stem == ".." {
        return false;
    }
    !stem.contains('/')
        && !stem.contains('\\')
        && !stem.contains(':')
        && stem
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
}
pub(crate) fn resolve(id: &CustomizationId) -> Result<(Root, PathBuf), String> {
    let root = roots(&id.provider)?
        .into_iter()
        .find(|r| r.id == id.root)
        .ok_or("不支持的文件位置")?;
    let path = if root.kind == "skill" {
        if !valid_skill_name(&id.name) {
            return Err("无效的 skill 名称".into());
        }
        let active = root.path.join(&id.name).join("SKILL.md");
        let disabled = root.path.join(&id.name).join("SKILL.md.disabled");
        if disabled.is_file() && !active.is_file() {
            disabled
        } else {
            active
        }
    } else {
        if !root.names.contains(&id.name.as_str()) && !valid_instruction_name(&id.name) {
            return Err("不支持的指令文件".into());
        }
        root.path.join(&id.name)
    };
    // Follow an explicitly listed skill/instruction link to its actual target.
    // Replacing the link itself would silently break skill-manager deployments.
    Ok((root, canonical_target(&path)?))
}
pub(super) fn canonical_target(path: &Path) -> Result<PathBuf, String> {
    let path = if path.exists() {
        fs::canonicalize(path).map_err(|e| format!("解析文件位置失败：{e}"))?
    } else if path.parent().is_some_and(Path::exists) {
        let parent = fs::canonicalize(path.parent().ok_or("无效路径")?)
            .map_err(|e| format!("解析文件位置失败：{e}"))?;
        parent.join(path.file_name().ok_or("无效路径")?)
    } else {
        path.to_path_buf()
    };
    Ok(path)
}
pub(crate) fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}
pub(super) fn check_target(root: &Root, path: &Path) -> Result<(), String> {
    if (root.kind == "skill" && is_link(path.parent().ok_or("无效路径")?)) || is_link(path) {
        return Err("此文件或 skill 目录是符号链接，请在原位置管理".into());
    }
    Ok(())
}
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    }
}
