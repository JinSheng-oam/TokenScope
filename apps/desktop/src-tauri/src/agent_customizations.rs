//! Explicitly requested local editing of coding-agent instructions and skills.
//! Content never enters the collector, database, logs, or synchronization.
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE: usize = 1024 * 1024;

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
    linked: bool,
}

pub(crate) struct Root {
    pub(crate) id: &'static str,
    pub(crate) path: PathBuf,
    pub(crate) kind: &'static str,
    names: &'static [&'static str],
}

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
pub(crate) fn resolve(id: &CustomizationId) -> Result<(Root, PathBuf), String> {
    let root = roots(&id.provider)?
        .into_iter()
        .find(|r| r.id == id.root)
        .ok_or("不支持的文件位置")?;
    let path = if root.kind == "skill" {
        if !valid_skill_name(&id.name) {
            return Err("无效的 skill 名称".into());
        }
        root.path.join(&id.name).join("SKILL.md")
    } else {
        if !root.names.contains(&id.name.as_str()) {
            return Err("不支持的指令文件".into());
        }
        root.path.join(&id.name)
    };
    // Follow an explicitly listed skill/instruction link to its actual target.
    // Replacing the link itself would silently break skill-manager deployments.
    Ok((root, canonical_target(&path)?))
}
fn canonical_target(path: &Path) -> Result<PathBuf, String> {
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
fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}
fn check_target(root: &Root, path: &Path) -> Result<(), String> {
    if (root.kind == "skill" && is_link(path.parent().ok_or("无效路径")?)) || is_link(path) {
        return Err("此文件或 skill 目录是符号链接，请在原位置管理".into());
    }
    Ok(())
}
fn entry(id: CustomizationId, root: &Root, path: PathBuf, linked: bool) -> CustomizationEntry {
    let editable = check_target(root, &path).is_ok();
    CustomizationEntry {
        id,
        path: display_path(&path),
        kind: root.kind,
        exists: path.is_file(),
        editable,
        linked,
    }
}
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
    }
}
fn list(provider: &str) -> Result<Vec<CustomizationEntry>, String> {
    let mut result = Vec::new();
    for root in roots(provider)? {
        if root.kind == "instruction" {
            for name in root.names {
                let id = CustomizationId {
                    provider: provider.into(),
                    root: root.id.into(),
                    name: (*name).into(),
                };
                let path = resolve(&id)
                    .map(|(_, path)| path)
                    .unwrap_or_else(|_| root.path.join(name));
                let linked = is_link(&root.path.join(name));
                result.push(entry(id, &root, path, linked));
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
                let path = item.path().join("SKILL.md");
                if path.is_file() {
                    let id = CustomizationId {
                        provider: provider.into(),
                        root: root.id.into(),
                        name,
                    };
                    let linked = is_link(&item.path()) || is_link(&path);
                    let actual = resolve(&id).map(|(_, path)| path).unwrap_or(path);
                    result.push(entry(id, &root, actual, linked));
                }
            }
        }
    }
    result.sort_by(|a, b| {
        a.kind
            .cmp(b.kind)
            .then(a.id.root.cmp(&b.id.root))
            .then(a.id.name.cmp(&b.id.name))
    });
    Ok(result)
}
fn read(id: &CustomizationId) -> Result<Option<String>, String> {
    let (root, path) = resolve(id)?;
    read_path(&root, &path)
}
fn read_path(root: &Root, path: &Path) -> Result<Option<String>, String> {
    check_target(root, path)?;
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_FILE as u64 => fs::read_to_string(path)
            .map(Some)
            .map_err(|e| format!("读取文件失败：{e}")),
        Ok(_) => Err("文件不是普通文件或超过 1 MiB".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取文件失败：{e}")),
    }
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path, exists: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // ReplaceFileW preserves the existing target's security attributes. For a
    // new file, MoveFileExW without REPLACE_EXISTING rejects a racing creator.
    let result = if exists {
        unsafe {
            ReplaceFileW(
                to.as_ptr(),
                from.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    } else {
        unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x8) }
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path, exists: bool) -> std::io::Result<()> {
    if exists {
        fs::rename(from, to)
    } else {
        fs::hard_link(from, to)
    }
}

fn save(id: &CustomizationId, expected: Option<String>, content: String) -> Result<(), String> {
    let (root, path) = resolve(id)?;
    save_path(&root, &path, expected, content)
}
fn save_path(
    root: &Root,
    path: &Path,
    expected: Option<String>,
    content: String,
) -> Result<(), String> {
    if content.len() > MAX_FILE {
        return Err("文件超过 1 MiB".into());
    }
    if read_path(root, path)? != expected {
        return Err("文件已在外部更改，请重新加载后再保存".into());
    }
    let parent = path.parent().ok_or("无效路径")?;
    fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
    check_target(root, path)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间无效")?
        .as_nanos();
    let temp = parent.join(format!(".tokenscope-{}-{stamp}.tmp", std::process::id()));
    let result = (|| {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts
            .open(&temp)
            .map_err(|e| format!("创建临时文件失败：{e}"))?;
        if let Ok(meta) = fs::metadata(path) {
            file.set_permissions(meta.permissions())
                .map_err(|e| format!("设置文件权限失败：{e}"))?;
        }
        file.write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("写入文件失败：{e}"))?;
        drop(file);
        if read_path(root, path)? != expected {
            return Err("文件已在外部更改，请重新加载后再保存".into());
        }
        replace_file(&temp, path, expected.is_some()).map_err(|e| format!("保存文件失败：{e}"))
    })();
    let _ = fs::remove_file(&temp);
    result
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
