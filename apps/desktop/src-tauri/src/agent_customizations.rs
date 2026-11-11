//! Explicitly requested local editing of coding-agent instructions and skills.
//! Content never enters the collector, database, logs, or synchronization.
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

mod dsh_mcp;
mod file_io;
pub(crate) mod mcp;
mod paths;
use file_io::{read, save};
pub(crate) use file_io::{read_path, replace_file, save_path};
use paths::check_target;
use paths::valid_instruction_name;
pub(crate) use paths::{display_path, is_link, resolve, roots, valid_skill_name};

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct CustomizationId {
    pub(crate) provider: String,
    pub(crate) root: String,
    pub(crate) name: String,
}

#[derive(Serialize)]
pub(crate) struct CustomizationEntry {
    id: CustomizationId,
    path: String,
    kind: &'static str,
    exists: bool,
    editable: bool,
    builtin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    builtin_description: Option<&'static str>,
    linked: bool,
    enabled: bool,
    #[serde(rename = "instructionEntry")]
    instruction_entry: bool,
}

pub(crate) struct Root {
    pub(crate) id: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) kind: &'static str,
    pub(crate) names: &'static [&'static str],
}

impl Root {
    pub(crate) fn is_builtin(&self) -> bool {
        matches!(
            self.id,
            "codex-system-skills"
                | "antigravity-app-builtin"
                | "antigravity-cli-builtin"
                | "dsh-office-builtin"
        )
    }

    pub(crate) fn require_writable(&self) -> Result<(), String> {
        if self.is_builtin() {
            return Err("Agent 自带 Skill 由 Agent 管理，只能查看".into());
        }
        Ok(())
    }
}

fn entry(
    id: CustomizationId,
    root: &Root,
    path: PathBuf,
    linked: bool,
    enabled: bool,
) -> CustomizationEntry {
    let editable = check_target(root, &path).is_ok();
    let instruction_entry = root.kind == "instruction" && root.names.contains(&id.name.as_str());
    CustomizationEntry {
        id,
        path: display_path(&path),
        kind: root.kind,
        exists: path.is_file(),
        editable: editable && !root.is_builtin(),
        builtin: root.is_builtin(),
        builtin_description: None,
        linked,
        enabled,
        instruction_entry,
    }
}
fn list(provider: &str) -> Result<Vec<CustomizationEntry>, String> {
    let mut result = Vec::new();
    for root in roots(provider)? {
        if root.kind == "instruction" {
            let mut seen = std::collections::HashSet::new();
            for name in root.names {
                seen.insert((*name).to_owned());
                let id = CustomizationId {
                    provider: provider.into(),
                    root: root.id.into(),
                    name: (*name).into(),
                };
                let path = resolve(&id)
                    .map(|(_, path)| path)
                    .unwrap_or_else(|_| root.path.join(name));
                let linked = is_link(&root.path.join(name));
                result.push(entry(id, &root, path, linked, true));
            }
            if let Ok(entries) = fs::read_dir(&root.path) {
                for item in entries.flatten() {
                    let file_name = item.file_name().to_string_lossy().to_string();
                    if valid_instruction_name(&file_name)
                        && !seen.contains(&file_name)
                        && item.path().is_file()
                    {
                        seen.insert(file_name.clone());
                        let id = CustomizationId {
                            provider: provider.into(),
                            root: root.id.into(),
                            name: file_name,
                        };
                        let linked = is_link(&item.path());
                        let path = resolve(&id)
                            .map(|(_, path)| path)
                            .unwrap_or_else(|_| item.path());
                        result.push(entry(id, &root, path, linked, true));
                    }
                }
            }
        } else {
            let entries = match fs::read_dir(&root.path) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("读取 skill 目录失败：{e}")),
            };
            for item in entries {
                let item = item.map_err(|e| format!("读取 skill 目录失败：{e}"))?;
                let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !valid_skill_name(&name) {
                    continue;
                }
                let active = item.path().join("SKILL.md");
                let disabled = item.path().join("SKILL.md.disabled");
                let is_active = active.is_file();
                let is_disabled = !is_active && disabled.is_file();
                if is_active || is_disabled {
                    let id = CustomizationId {
                        provider: provider.into(),
                        root: root.id.into(),
                        name,
                    };
                    let target_path = if is_active { active } else { disabled };
                    let linked = is_link(&item.path()) || is_link(&target_path);
                    let actual = resolve(&id).map(|(_, path)| path).unwrap_or(target_path);
                    result.push(entry(id, &root, actual, linked, is_active));
                }
            }
        }
    }
    if provider == "claude"
        && paths::path_has_command(&["claude", "claude.exe", "claude.cmd", "claude.ps1"])
    {
        // Claude ships these inside its CLI. This documents known commands,
        // not the active session's version, visibility or policy overrides.
        result.extend(
            [
                ("batch", "拆分并行执行跨文件的批量任务。"),
                ("claude-api", "提供 Claude API 与 SDK 的开发指引。"),
                ("code-review", "检查代码变更中的问题。"),
                ("debug", "诊断 Claude Code 的运行问题。"),
                ("doctor", "检查 Claude Code 安装和配置。"),
                ("loop", "在会话中按间隔重复运行提示。"),
                ("simplify", "检查并简化最近的代码变更。"),
                ("verify", "运行应用验证变更。"),
            ]
            .into_iter()
            .map(|(name, description)| CustomizationEntry {
                id: CustomizationId {
                    provider: provider.into(),
                    root: "claude-builtin-catalog".into(),
                    name: name.into(),
                },
                path: String::new(),
                kind: "skill",
                exists: true,
                editable: false,
                builtin: true,
                builtin_description: Some(description),
                linked: false,
                enabled: true,
                instruction_entry: false,
            }),
        );
    }
    result.sort_by(|a, b| {
        a.kind
            .cmp(b.kind)
            .then(a.id.root.cmp(&b.id.root))
            .then(a.id.name.cmp(&b.id.name))
    });
    Ok(result)
}
#[tauri::command]
pub(crate) async fn list_customizations(
    provider: String,
) -> Result<Vec<CustomizationEntry>, String> {
    tauri::async_runtime::spawn_blocking(move || list(&provider))
        .await
        .map_err(|_| "读取任务失败")?
}
#[tauri::command]
pub(crate) async fn read_customization(id: CustomizationId) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || read(&id))
        .await
        .map_err(|_| "读取任务失败")?
}
#[tauri::command]
pub(crate) async fn save_customization(
    id: CustomizationId,
    expected: Option<String>,
    content: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || save(&id, expected, content))
        .await
        .map_err(|_| "保存任务失败")?
}

fn prepare_instr(provider: &str, root_id: &str, name: &str) -> Result<CustomizationEntry, String> {
    let name = name.trim();
    if !valid_instruction_name(name) {
        return Err("无效的文件名，必须以 .md 结尾且仅包含字母、数字、点、连字符和下划线".into());
    }
    let all_roots = roots(provider)?;
    let root = all_roots
        .into_iter()
        .find(|r| r.id == root_id && r.kind == "instruction")
        .ok_or("不支持的指令目录")?;
    let path = root.path.join(name);
    if fs::symlink_metadata(&path).is_ok() {
        return Err("同名指令文件已存在".into());
    }
    let id = CustomizationId {
        provider: provider.into(),
        root: root.id.into(),
        name: name.into(),
    };
    Ok(entry(id, &root, path, false, true))
}

fn delete_instr(id: &CustomizationId) -> Result<(), String> {
    let (root, resolved_path) = resolve(id)?;
    if root.kind != "instruction" {
        return Err("仅支持删除指令文件".into());
    }

    let path = root.path.join(&id.name);
    if is_link(&path) {
        return Err("符号链接文件不支持直接删除，请在原位置管理".into());
    }
    check_target(&root, &path)?;
    if !path.is_file() {
        return Err("指令文件不存在".into());
    }
    fs::remove_file(&resolved_path).map_err(|e| format!("删除指令文件失败：{e}"))
}

fn rename_item(id: &CustomizationId, new_name: &str) -> Result<CustomizationEntry, String> {
    let new_name = new_name.trim();
    let root = roots(&id.provider)?
        .into_iter()
        .find(|r| r.id == id.root)
        .ok_or("不支持的文件位置")?;

    root.require_writable()?;
    if root.kind == "instruction" {
        if !valid_instruction_name(new_name) {
            return Err("无效的指令文件名，必须以 .md 结尾且不包含特殊路径字符".into());
        }
        let old_path = root.path.join(&id.name);
        if !old_path.is_file() {
            return Err("原指令文件不存在".into());
        }
        if is_link(&old_path) {
            return Err("符号链接文件不支持直接重命名，请在原位置管理".into());
        }
        let new_path = root.path.join(new_name);
        if new_path.exists() {
            return Err("目标文件名已存在".into());
        }
        fs::rename(&old_path, &new_path).map_err(|e| format!("重命名文件失败：{e}"))?;
        let new_id = CustomizationId {
            provider: id.provider.clone(),
            root: id.root.clone(),
            name: new_name.into(),
        };
        Ok(entry(new_id, &root, new_path, false, true))
    } else if root.kind == "skill" {
        if !valid_skill_name(new_name) {
            return Err("无效的 skill 名称，仅允许字母、数字、连字符和下划线".into());
        }
        let old_dir = root.path.join(&id.name);
        if !old_dir.is_dir() {
            return Err("原 skill 目录不存在".into());
        }
        if is_link(&old_dir) {
            return Err("符号链接目录不支持直接重命名，请在原位置管理".into());
        }
        let new_dir = root.path.join(new_name);
        if new_dir.exists() {
            return Err("目标 skill 名称已存在".into());
        }
        fs::rename(&old_dir, &new_dir).map_err(|e| format!("重命名 skill 失败：{e}"))?;
        let new_id = CustomizationId {
            provider: id.provider.clone(),
            root: id.root.clone(),
            name: new_name.into(),
        };
        let active = new_dir.join("SKILL.md");
        let is_active = active.is_file();
        let target_file = if is_active {
            active
        } else {
            new_dir.join("SKILL.md.disabled")
        };
        Ok(entry(new_id, &root, target_file, false, is_active))
    } else {
        Err("不支持的类型".into())
    }
}

fn toggle_skill(id: &CustomizationId, enabled: bool) -> Result<bool, String> {
    let (root, _) = resolve(id)?;
    root.require_writable()?;
    if root.kind != "skill" {
        return Err("仅支持对 skill 启用或停用".into());
    }
    let skill_dir = root.path.join(&id.name);
    let active = skill_dir.join("SKILL.md");
    let disabled = skill_dir.join("SKILL.md.disabled");
    if enabled {
        if active.is_file() {
            return Ok(true);
        }
        if !disabled.is_file() {
            return Err("找不到可启用的 SKILL.md.disabled 文件".into());
        }
        fs::rename(&disabled, &active).map_err(|e| format!("启用 skill 失败：{e}"))?;
        Ok(true)
    } else {
        if disabled.is_file() && !active.is_file() {
            return Ok(false);
        }
        if !active.is_file() {
            return Err("找不到可停用的 SKILL.md 文件".into());
        }
        fs::rename(&active, &disabled).map_err(|e| format!("停用 skill 失败：{e}"))?;
        Ok(false)
    }
}

#[tauri::command]
pub(crate) async fn prepare_instruction(
    provider: String,
    root: String,
    name: String,
) -> Result<CustomizationEntry, String> {
    tauri::async_runtime::spawn_blocking(move || prepare_instr(&provider, &root, &name))
        .await
        .map_err(|_| "准备指令草稿失败")?
}

#[tauri::command]
pub(crate) async fn delete_instruction(id: CustomizationId) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete_instr(&id))
        .await
        .map_err(|_| "删除指令任务失败")?
}

#[tauri::command]
pub(crate) async fn rename_customization(
    id: CustomizationId,
    new_name: String,
) -> Result<CustomizationEntry, String> {
    tauri::async_runtime::spawn_blocking(move || rename_item(&id, &new_name))
        .await
        .map_err(|_| "重命名任务失败")?
}

#[tauri::command]
pub(crate) async fn toggle_skill_enabled(
    id: CustomizationId,
    enabled: bool,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || toggle_skill(&id, enabled))
        .await
        .map_err(|_| "切换状态任务失败")?
}

#[cfg(test)]
mod tests {
    use super::paths::canonical_target;
    use super::*;
    use std::{
        env,
        path::Path,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn windows_extended_paths_are_friendly_for_display() {
        assert_eq!(
            display_path(Path::new(r"\\?\C:\Users\Jin\skill\SKILL.md")),
            r"C:\Users\Jin\skill\SKILL.md"
        );
        assert_eq!(
            display_path(Path::new(r"\\?\UNC\server\share\SKILL.md")),
            r"\\server\share\SKILL.md"
        );
    }

    #[test]
    fn rejects_path_components_and_internal_skill_directory() {
        for name in ["", "..", "../escape", ".system", "a/b", "a\\b"] {
            assert!(!valid_skill_name(name));
        }
        assert!(valid_skill_name("code-review_2"));
    }

    #[test]
    fn saves_only_when_file_matches_loaded_content() {
        let dir = env::temp_dir().join(format!(
            "tokenscope-customization-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let root = Root {
            id: "test",
            path: dir.clone(),
            kind: "instruction",
            names: &["AGENTS.md"],
        };
        let path = dir.join("AGENTS.md");
        save_path(&root, &path, None, "first".into()).unwrap();
        assert_eq!(read_path(&root, &path).unwrap().as_deref(), Some("first"));
        fs::write(&path, "external").unwrap();
        assert!(save_path(&root, &path, Some("first".into()), "changed".into()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external");
        save_path(&root, &path, Some("external".into()), "updated".into()).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "updated");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn linked_skill_is_saved_at_target_without_replacing_link() {
        let dir = env::temp_dir().join(format!(
            "tokenscope-skill-link-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let target = dir.join("source");
        let link = dir.join("deployed");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("SKILL.md"), "original").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&target, &link);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &link);
        if linked.is_err() {
            fs::remove_dir_all(dir).unwrap();
            return; // Some Windows installations do not grant symlink creation.
        }
        let root = Root {
            id: "test",
            path: dir.clone(),
            kind: "skill",
            names: &[],
        };
        let path = canonical_target(&link.join("SKILL.md")).unwrap();
        save_path(&root, &path, Some("original".into()), "updated".into()).unwrap();
        assert_eq!(
            fs::read_to_string(target.join("SKILL.md")).unwrap(),
            "updated"
        );
        assert!(is_link(&link));
        #[cfg(windows)]
        fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        fs::remove_file(&link).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn validates_instruction_names() {
        assert!(valid_instruction_name("AGENTS.md"));
        assert!(valid_instruction_name("custom_rules.md"));
        assert!(valid_instruction_name("rules-2.md"));
        assert!(!valid_instruction_name(""));
        assert!(!valid_instruction_name(".md"));
        assert!(!valid_instruction_name("rules.txt"));
        assert!(!valid_instruction_name("../rules.md"));
        assert!(!valid_instruction_name("dir/rules.md"));
    }

    #[test]
    fn toggles_skill_between_enabled_and_disabled() {
        let dir = env::temp_dir().join(format!(
            "tokenscope-toggle-test-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let skill_dir = dir.join("test-skill");
        fs::create_dir_all(&skill_dir).unwrap();
        let skill_file = skill_dir.join("SKILL.md");
        fs::write(&skill_file, "# Test Skill").unwrap();

        // Disable
        // Verify file rename
        fs::rename(&skill_file, skill_dir.join("SKILL.md.disabled")).unwrap();
        assert!(skill_dir.join("SKILL.md.disabled").is_file());
        assert!(!skill_file.exists());

        // Re-enable
        fs::rename(skill_dir.join("SKILL.md.disabled"), &skill_file).unwrap();
        assert!(skill_file.is_file());
        assert!(!skill_dir.join("SKILL.md.disabled").exists());

        fs::remove_dir_all(dir).unwrap();
    }
}
