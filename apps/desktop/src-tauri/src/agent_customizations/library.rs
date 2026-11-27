use super::{display_path, is_link, list, resolve, roots, valid_skill_name, CustomizationId};
use crate::{
    ai_assistant::{read_cached_summary_in_dir, SkillSummary},
    skill_sources::{load_manager_source_index, read_local_source_record},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const PROVIDERS: &[&str] = &["codex", "claude", "antigravity", "pi", "deepseek"];

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SkillDeployment {
    pub(crate) id: CustomizationId,
    pub(crate) provider: String,
    pub(crate) root: String,
    pub(crate) path: String,
    pub(crate) enabled: bool,
    pub(crate) linked: bool,
    pub(crate) sync_mode: String, // "link" | "copy"
    pub(crate) editable: bool,
    pub(crate) builtin: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SkillSourceInfo {
    pub(crate) source_type: String,
    pub(crate) source_group: String,
    pub(crate) source_ref: Option<String>,
    pub(crate) resolved_url: Option<String>,
    pub(crate) subpath: Option<String>,
    pub(crate) revision: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct SkillLibraryItem {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) primary_id: CustomizationId,
    pub(crate) path: String,
    pub(crate) exists: bool,
    pub(crate) editable: bool,
    pub(crate) builtin: bool,
    pub(crate) linked: bool,
    pub(crate) enabled: bool,
    pub(crate) description: String,
    pub(crate) summary: Option<SkillSummary>,
    pub(crate) source: SkillSourceInfo,
    pub(crate) deployments: Vec<SkillDeployment>,
}

fn read_prefix(path: &Path, limit: usize) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; limit];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

pub(crate) fn extract_frontmatter_description(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n");
    let mut lines = normalized.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    if first.trim() != "---" {
        return String::new();
    }

    let mut fm_lines = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        fm_lines.push(line);
    }

    let mut idx = 0;
    while idx < fm_lines.len() {
        let line = fm_lines[idx];
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("description:") {
            let value = rest.trim();
            if matches!(value, ">" | "|" | ">-" | "|-" | ">+" | "|+") {
                let mut block = Vec::new();
                idx += 1;
                while idx < fm_lines.len() {
                    let next = fm_lines[idx];
                    if next.starts_with(' ') || next.starts_with('\t') || next.trim().is_empty() {
                        let part = next.trim();
                        if !part.is_empty() {
                            block.push(part);
                        }
                        idx += 1;
                    } else {
                        break;
                    }
                }
                return block.join(" ");
            }
            return value
                .trim_matches(|c| c == '"' || c == '\'')
                .trim()
                .to_string();
        }
        idx += 1;
    }
    String::new()
}

fn extract_github_owner_repo(raw: &str) -> Option<(String, String)> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("git@github.com:") {
        let clean = rest.trim_end_matches(".git");
        let mut parts = clean.split('/');
        let owner = parts.next()?.trim();
        let repo = parts.next()?.trim();
        if !owner.is_empty() && !repo.is_empty() {
            return Some((
                format!("{owner}/{repo}"),
                format!("https://github.com/{owner}/{repo}.git"),
            ));
        }
    }
    if let Some(pos) = trimmed.find("github.com/") {
        let rest = &trimmed[pos + "github.com/".len()..];
        let clean = rest.trim_end_matches(".git");
        let mut parts = clean.split('/');
        let owner = parts.next()?.trim();
        let repo = parts.next()?.trim();
        if !owner.is_empty() && !repo.is_empty() {
            return Some((
                format!("{owner}/{repo}"),
                format!("https://github.com/{owner}/{repo}.git"),
            ));
        }
    }
    if !trimmed.contains("://") && !trimmed.contains(' ') && !trimmed.starts_with('.') {
        let mut parts = trimmed.split('/');
        let owner = parts.next()?.trim();
        let repo = parts.next()?.trim().trim_end_matches(".git");
        if !owner.is_empty()
            && !repo.is_empty()
            && owner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            && repo
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Some((
                format!("{owner}/{repo}"),
                format!("https://github.com/{owner}/{repo}.git"),
            ));
        }
    }
    None
}

fn read_git_origin_url(dir: &Path) -> Option<String> {
    let config_path = dir.join(".git").join("config");
    let text = fs::read_to_string(config_path).ok()?;
    let mut in_origin = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed.eq_ignore_ascii_case("[remote \"origin\"]");
            continue;
        }
        if in_origin {
            if let Some(rest) = trimmed.strip_prefix("url") {
                if let Some((_, val)) = rest.split_once('=') {
                    let url = val.trim();
                    if !url.is_empty() {
                        return Some(url.to_string());
                    }
                }
            }
        }
    }
    None
}

fn normalize_source_type(raw: &str) -> String {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "skills.sh" | "skills_sh" | "skillssh" | "registry" => "skills.sh".into(),
        "git" | "github" | "repo" => "git".into(),
        "import" | "imported" => "import".into(),
        "local" | "manual" => "local".into(),
        "builtin" => "builtin".into(),
        other if !other.is_empty() => other.to_string(),
        _ => "local".into(),
    }
}

fn resolve_source_info(
    dir: Option<&Path>,
    builtin: bool,
    manager_index: &HashMap<PathBuf, (String, Option<String>)>,
) -> SkillSourceInfo {
    if builtin {
        return SkillSourceInfo {
            source_type: "builtin".into(),
            source_group: "内置技能".into(),
            source_ref: None,
            resolved_url: None,
            subpath: None,
            revision: None,
        };
    }

    if let Some(dir) = dir {
        if let Ok(canon) = fs::canonicalize(dir) {
            if let Some((raw_type, source_ref)) = manager_index.get(&canon) {
                let source_type = normalize_source_type(raw_type);
                let (source_group, resolved_url) = source_ref
                    .as_deref()
                    .and_then(extract_github_owner_repo)
                    .map(|(g, url)| (g, Some(url)))
                    .unwrap_or_else(|| ("未分类".into(), None));
                return SkillSourceInfo {
                    source_type,
                    source_group,
                    source_ref: source_ref.clone(),
                    resolved_url,
                    subpath: None,
                    revision: None,
                };
            }
        }

        if let Some(meta) = read_local_source_record(dir) {
            let (source_group, resolved_url) = extract_github_owner_repo(&meta.url)
                .map(|(g, url)| (g, Some(url)))
                .unwrap_or_else(|| ("未分类".into(), Some(meta.url.clone())));
            return SkillSourceInfo {
                source_type: "git".into(),
                source_group,
                source_ref: Some(meta.url),
                resolved_url,
                subpath: (!meta.subpath.is_empty()).then_some(meta.subpath),
                revision: (!meta.revision.is_empty()).then_some(meta.revision),
            };
        }

        if let Some(origin_url) = read_git_origin_url(dir) {
            let (source_group, resolved_url) = extract_github_owner_repo(&origin_url)
                .map(|(g, url)| (g, Some(url)))
                .unwrap_or_else(|| ("未分类".into(), Some(origin_url.clone())));
            return SkillSourceInfo {
                source_type: "git".into(),
                source_group,
                source_ref: Some(origin_url),
                resolved_url,
                subpath: None,
                revision: None,
            };
        }
    }

    SkillSourceInfo {
        source_type: "local".into(),
        source_group: "未分类".into(),
        source_ref: None,
        resolved_url: None,
        subpath: None,
        revision: None,
    }
}

fn item_group_key(builtin: bool, id: &CustomizationId) -> String {
    if builtin {
        format!("builtin:{}:{}", id.root, id.name.to_ascii_lowercase())
    } else {
        format!("skill:{}", id.name.to_ascii_lowercase())
    }
}

fn dir_from_file_path(path: &str) -> Option<PathBuf> {
    if path.is_empty() {
        return None;
    }
    let parent = Path::new(path).parent()?;
    Some(fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf()))
}

fn build_skill_library() -> Result<Vec<SkillLibraryItem>, String> {
    let manager_index = load_manager_source_index();
    let mut items: BTreeMap<String, SkillLibraryItem> = BTreeMap::new();

    // 1. Scan central library hubs first so central copies act as canonical primary_id
    if let Ok(central_entries) = list("library") {
        for entry in central_entries {
            if entry.kind != "skill" {
                continue;
            }
            let key = item_group_key(false, &entry.id);
            if items.contains_key(&key) {
                continue;
            }
            let dir = dir_from_file_path(&entry.path);
            let description = if !entry.path.is_empty() {
                read_prefix(Path::new(&entry.path), 8192)
                    .map(|text| extract_frontmatter_description(&text))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            let summary = dir.as_deref().and_then(read_cached_summary_in_dir);
            let source = resolve_source_info(dir.as_deref(), false, &manager_index);

            items.insert(
                key.clone(),
                SkillLibraryItem {
                    key,
                    name: entry.id.name.clone(),
                    primary_id: entry.id,
                    path: entry.path,
                    exists: entry.exists,
                    editable: entry.editable,
                    builtin: false,
                    linked: false,
                    enabled: false,
                    description,
                    summary,
                    source,
                    deployments: Vec::new(),
                },
            );
        }
    }

    // 2. Scan all 5 agent providers and merge into unified skill items
    for provider in PROVIDERS {
        let Ok(entries) = list(provider) else {
            continue;
        };
        for entry in entries {
            if entry.kind != "skill" {
                continue;
            }
            let key = item_group_key(entry.builtin, &entry.id);
            let dir = dir_from_file_path(&entry.path);
            let deployment = SkillDeployment {
                id: entry.id.clone(),
                provider: entry.id.provider.clone(),
                root: entry.id.root.clone(),
                path: entry.path.clone(),
                enabled: entry.enabled,
                linked: entry.linked,
                sync_mode: if entry.linked { "link".into() } else { "copy".into() },
                editable: entry.editable,
                builtin: entry.builtin,
            };

            if let Some(existing) = items.get_mut(&key) {
                let dup = existing
                    .deployments
                    .iter()
                    .any(|d| d.provider == deployment.provider && d.root == deployment.root);
                if !dup {
                    existing.deployments.push(deployment.clone());
                }
                existing.enabled |= deployment.enabled;
                existing.linked |= deployment.linked;
                if existing.summary.is_none() {
                    existing.summary = dir.as_deref().and_then(read_cached_summary_in_dir);
                }
                if existing.description.is_empty() && !entry.path.is_empty() {
                    existing.description = read_prefix(Path::new(&entry.path), 8192)
                        .map(|text| extract_frontmatter_description(&text))
                        .unwrap_or_default();
                }
                if existing.source.source_type == "local" && existing.source.source_group == "未分类" {
                    let candidate_source =
                        resolve_source_info(dir.as_deref(), entry.builtin, &manager_index);
                    if candidate_source.source_type != "local"
                        || candidate_source.source_group != "未分类"
                    {
                        existing.source = candidate_source;
                    }
                }
                if !existing.editable && deployment.editable {
                    existing.editable = true;
                    if existing.primary_id.provider != "library" {
                        existing.primary_id = deployment.id.clone();
                    }
                }
                continue;
            }

            let description = if let Some(desc) = entry.builtin_description {
                desc.to_string()
            } else if !entry.path.is_empty() {
                read_prefix(Path::new(&entry.path), 8192)
                    .map(|text| extract_frontmatter_description(&text))
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let summary = dir.as_deref().and_then(read_cached_summary_in_dir);
            let source = resolve_source_info(dir.as_deref(), entry.builtin, &manager_index);

            items.insert(
                key.clone(),
                SkillLibraryItem {
                    key,
                    name: entry.id.name.clone(),
                    primary_id: entry.id,
                    path: entry.path,
                    exists: entry.exists,
                    editable: entry.editable,
                    builtin: entry.builtin,
                    linked: entry.linked,
                    enabled: entry.enabled,
                    description,
                    summary,
                    source,
                    deployments: vec![deployment],
                },
            );
        }
    }

    let mut result: Vec<SkillLibraryItem> = items.into_values().collect();
    result.sort_by(|a, b| {
        a.builtin
            .cmp(&b.builtin)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
            .then_with(|| a.key.cmp(&b.key))
    });
    Ok(result)
}

fn dedicated_skill_root_id(provider: &str) -> Result<&'static str, String> {
    match provider {
        "codex" => Ok("codex-skills"),
        "claude" => Ok("claude-skills"),
        "antigravity" => Ok("antigravity-skills"),
        "pi" => Ok("pi-skills"),
        "deepseek" => Ok("dsh-skills"),
        _ => Err("不支持的工具".into()),
    }
}

fn tokenscope_central_root() -> Result<PathBuf, String> {
    roots("library")?
        .into_iter()
        .find(|r| r.id == "tokenscope-skills")
        .map(|r| r.path)
        .ok_or_else(|| "无法确定 TokenScope 中央技能库目录".into())
}

fn is_under_central_hub(dir: &Path) -> bool {
    let Ok(hubs) = roots("library") else {
        return false;
    };
    let dir_norm = display_path(&fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()))
        .to_ascii_lowercase();
    hubs.into_iter().any(|h| {
        let hub_canon = fs::canonicalize(&h.path).unwrap_or(h.path);
        let hub_norm = display_path(&hub_canon).to_ascii_lowercase();
        dir_norm.starts_with(&hub_norm)
    })
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("创建目标技能目录失败：{e}"))?;
    for entry in fs::read_dir(src).map_err(|e| format!("读取源技能目录失败：{e}"))? {
        let entry = entry.map_err(|e| format!("读取源技能文件失败：{e}"))?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        let ft = entry
            .file_type()
            .map_err(|e| format!("读取文件类型失败：{e}"))?;
        if ft.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            fs::copy(&from, &to).map_err(|e| format!("复制技能文件失败：{e}"))?;
        }
    }
    Ok(())
}

fn create_link_dir(src_dir: &Path, dst_dir: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(src_dir, dst_dir)
            .map_err(|e| format!("创建符号链接失败：{e}"))
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if std::os::windows::fs::symlink_dir(src_dir, dst_dir).is_ok() {
            return Ok(());
        }
        // NTFS directory junction works without admin or Developer Mode privileges
        let out = std::process::Command::new("cmd")
            .args([
                "/C",
                "mklink",
                "/J",
                &display_path(dst_dir),
                &display_path(src_dir),
            ])
            .creation_flags(0x08000000)
            .output()
            .map_err(|e| format!("创建目录链接失败：{e}"))?;
        if out.status.success() && dst_dir.exists() {
            return Ok(());
        }
        Err("创建静态链接失败（请检查目录权限或切换为文件复制模式）".into())
    }
}

fn link_or_copy_skill_dir(src_dir: &Path, dst_dir: &Path, sync_mode: &str) -> Result<(), String> {
    if sync_mode == "copy" {
        return copy_dir_recursive(src_dir, dst_dir);
    }
    if create_link_dir(src_dir, dst_dir).is_ok() {
        return Ok(());
    }
    copy_dir_recursive(src_dir, dst_dir)
}

fn remove_link_only(path: &Path) -> Result<(), String> {
    if !is_link(path) {
        return Err("仅允许移除符号链接或目录联接，避免误删源目录".into());
    }
    #[cfg(windows)]
    {
        fs::remove_dir(path)
            .or_else(|_| fs::remove_file(path))
            .map_err(|e| format!("移除技能链接失败：{e}"))
    }
    #[cfg(unix)]
    {
        fs::remove_file(path).map_err(|e| format!("移除技能链接失败：{e}"))
    }
}

fn ensure_active_skill_file(dir: &Path) -> Result<(), String> {
    let active = dir.join("SKILL.md");
    let disabled = dir.join("SKILL.md.disabled");
    if active.is_file() {
        return Ok(());
    }
    if disabled.is_file() {
        fs::rename(&disabled, &active).map_err(|e| format!("启用 SKILL.md 失败：{e}"))?;
        return Ok(());
    }
    Err("找不到可启用的 SKILL.md 文件".into())
}

fn disable_skill_file(dir: &Path) -> Result<(), String> {
    let active = dir.join("SKILL.md");
    let disabled = dir.join("SKILL.md.disabled");
    if disabled.is_file() && !active.is_file() {
        return Ok(());
    }
    if active.is_file() {
        fs::rename(&active, &disabled).map_err(|e| format!("停用 SKILL.md 失败：{e}"))?;
        return Ok(());
    }
    Err("找不到可停用的 SKILL.md 文件".into())
}

fn count_other_deployments_for_target(canonical_dir: &Path, exclude_path: &Path) -> usize {
    if is_under_central_hub(canonical_dir) {
        return 1;
    }
    let target_norm = display_path(canonical_dir).to_ascii_lowercase();
    let exclude_norm = display_path(exclude_path).to_ascii_lowercase();
    let mut count = 0;

    for provider in PROVIDERS {
        let Ok(provider_roots) = roots(provider) else {
            continue;
        };
        for root in provider_roots {
            if root.kind != "skill" || root.is_builtin() {
                continue;
            }
            let Ok(entries) = fs::read_dir(&root.path) else {
                continue;
            };
            for item in entries.flatten() {
                let p = item.path();
                if display_path(&p).to_ascii_lowercase() == exclude_norm {
                    continue;
                }
                if let Ok(canon) = fs::canonicalize(&p) {
                    if display_path(&canon).to_ascii_lowercase() == target_norm {
                        count += 1;
                    }
                }
            }
        }
    }
    count
}

fn same_canonical_dir(a: &Path, b: &Path) -> bool {
    let ca = fs::canonicalize(a).unwrap_or_else(|_| a.to_path_buf());
    let cb = fs::canonicalize(b).unwrap_or_else(|_| b.to_path_buf());
    display_path(&ca).eq_ignore_ascii_case(&display_path(&cb))
}

fn ensure_central_anchor(source_dir: &Path, skill_name: &str) -> Result<PathBuf, String> {
    if is_under_central_hub(source_dir) {
        return Ok(source_dir.to_path_buf());
    }
    let hub = tokenscope_central_root()?;
    fs::create_dir_all(&hub).map_err(|e| format!("创建中央技能库目录失败：{e}"))?;
    let central_dir = hub.join(skill_name);
    if !central_dir.exists() {
        copy_dir_recursive(source_dir, &central_dir)?;
        let _ = ensure_active_skill_file(&central_dir);
    }
    Ok(central_dir)
}

fn replace_copy_with_link(copy_dir: &Path, canonical_src: &Path) -> Result<(), String> {
    if same_canonical_dir(copy_dir, canonical_src) {
        return Ok(());
    }
    if is_link(copy_dir) {
        remove_link_only(copy_dir)?;
        return create_link_dir(canonical_src, copy_dir);
    }
    let backup = copy_dir.with_file_name(format!(
        ".tokenscope-backup-{}-{}",
        copy_dir.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    if backup.exists() {
        let _ = fs::remove_dir_all(&backup);
    }
    fs::rename(copy_dir, &backup).map_err(|e| format!("备份原目录失败：{e}"))?;
    if let Err(err) = create_link_dir(canonical_src, copy_dir) {
        let _ = fs::rename(&backup, copy_dir);
        return Err(err);
    }
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn set_provider_enabled(
    source_id: &CustomizationId,
    target_provider: &str,
    enabled: bool,
    sync_mode: &str,
) -> Result<bool, String> {
    if !valid_skill_name(&source_id.name) {
        return Err("无效的 skill 名称".into());
    }
    let (source_root, source_file_path) = resolve(source_id)?;
    source_root.require_writable()?;
    if source_root.kind != "skill" {
        return Err("仅支持对 skill 切换启用状态".into());
    }
    let source_dir = source_file_path
        .parent()
        .ok_or("无效的 skill 目录")?
        .to_path_buf();
    let target_root_id = dedicated_skill_root_id(target_provider)?;
    let target_roots = roots(target_provider)?;
    let dedicated_root = target_roots
        .iter()
        .find(|r| r.id == target_root_id)
        .ok_or("找不到目标工具的技能目录")?;
    dedicated_root.require_writable()?;

    let existing_in_provider: Vec<(String, PathBuf)> = target_roots
        .iter()
        .filter(|r| r.kind == "skill" && !r.is_builtin())
        .filter_map(|r| {
            let dir = r.path.join(&source_id.name);
            if fs::symlink_metadata(&dir).is_ok() {
                Some((r.id.to_string(), dir))
            } else {
                None
            }
        })
        .collect();

    if enabled {
        // Make sure source is active before linking or copying
        ensure_active_skill_file(&source_dir)?;

        // Clean up any broken links in target provider
        let mut live_entries = Vec::new();
        for (root_id, dir) in existing_in_provider {
            if is_link(&dir) && !dir.exists() {
                let _ = remove_link_only(&dir);
                continue;
            }
            if dir.join("SKILL.md").is_file() || dir.join("SKILL.md.disabled").is_file() {
                live_entries.push((root_id, dir));
            }
        }

        if !live_entries.is_empty() {
            for (_, dir) in live_entries {
                if sync_mode == "copy" && is_link(&dir) {
                    remove_link_only(&dir)?;
                    copy_dir_recursive(&source_dir, &dir)?;
                } else if sync_mode == "link" && !is_link(&dir) && !same_canonical_dir(&dir, &source_dir) {
                    let anchor = ensure_central_anchor(&source_dir, &source_id.name)?;
                    let _ = replace_copy_with_link(&dir, &anchor);
                }
                ensure_active_skill_file(&dir)?;
            }
            return Ok(true);
        }

        fs::create_dir_all(&dedicated_root.path)
            .map_err(|e| format!("创建目标工具技能目录失败：{e}"))?;
        let dest_dir = dedicated_root.path.join(&source_id.name);
        if is_link(&dest_dir) && !dest_dir.exists() {
            let _ = remove_link_only(&dest_dir);
        }
        if fs::symlink_metadata(&dest_dir).is_ok() {
            ensure_active_skill_file(&dest_dir)?;
            return Ok(true);
        }
        let effective_src = if sync_mode == "link" {
            ensure_central_anchor(&source_dir, &source_id.name).unwrap_or(source_dir)
        } else {
            source_dir
        };
        link_or_copy_skill_dir(&effective_src, &dest_dir, sync_mode)?;
        Ok(true)
    } else {
        if existing_in_provider.is_empty() {
            return Ok(false);
        }
        let shared_root_ids = ["shared-skills", "pi-shared-skills", "dsh-shared-skills"];
        for (root_id, dir) in existing_in_provider {
            if is_link(&dir) && !dir.exists() {
                let _ = remove_link_only(&dir);
                continue;
            }
            let is_shared = shared_root_ids.contains(&root_id.as_str());
            if is_link(&dir) {
                let canon = fs::canonicalize(&dir).unwrap_or_else(|_| source_dir.clone());
                if is_shared {
                    for sibling in ["codex", "pi", "deepseek"] {
                        if sibling == target_provider {
                            continue;
                        }
                        if let Ok(sib_root_id) = dedicated_skill_root_id(sibling) {
                            if let Ok(sib_roots) = roots(sibling) {
                                if let Some(sib_root) =
                                    sib_roots.into_iter().find(|r| r.id == sib_root_id)
                                {
                                    let sib_dest = sib_root.path.join(&source_id.name);
                                    if fs::symlink_metadata(&sib_dest).is_err() {
                                        let _ = fs::create_dir_all(&sib_root.path);
                                        let _ = link_or_copy_skill_dir(&canon, &sib_dest, sync_mode);
                                    }
                                }
                            }
                        }
                    }
                }
                let others = count_other_deployments_for_target(&canon, &dir);
                if others > 0 {
                    remove_link_only(&dir)?;
                } else {
                    // Anchor into central hub first so disabling never loses the skill
                    let _ = ensure_central_anchor(&canon, &source_id.name);
                    if count_other_deployments_for_target(&canon, &dir) > 0 {
                        remove_link_only(&dir)?;
                    } else {
                        disable_skill_file(&dir)?;
                    }
                }
            } else {
                // Real directory: if shared across codex/pi/deepseek, anchor to central hub & split siblings
                if is_shared {
                    if let Ok(anchor) = ensure_central_anchor(&dir, &source_id.name) {
                        for sibling in ["codex", "pi", "deepseek"] {
                            if sibling == target_provider {
                                continue;
                            }
                            if let Ok(sib_root_id) = dedicated_skill_root_id(sibling) {
                                if let Ok(sib_roots) = roots(sibling) {
                                    if let Some(sib_root) =
                                        sib_roots.into_iter().find(|r| r.id == sib_root_id)
                                    {
                                        let sib_dest = sib_root.path.join(&source_id.name);
                                        if fs::symlink_metadata(&sib_dest).is_err() {
                                            let _ = fs::create_dir_all(&sib_root.path);
                                            let _ =
                                                link_or_copy_skill_dir(&anchor, &sib_dest, sync_mode);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                disable_skill_file(&dir)?;
            }
        }
        Ok(false)
    }
}

fn set_all_providers_enabled(
    source_id: &CustomizationId,
    enabled: bool,
    sync_mode: &str,
    target_providers: Vec<String>,
    provider_sync_modes: &HashMap<String, String>,
) -> Result<bool, String> {
    let providers_to_touch: Vec<String> = if target_providers.is_empty() {
        PROVIDERS.iter().map(|s| (*s).to_string()).collect()
    } else {
        target_providers
    };

    let mode_for = |provider: &str| -> &str {
        provider_sync_modes
            .get(provider)
            .map(String::as_str)
            .unwrap_or(sync_mode)
    };

    if enabled {
        let mut any_existing = false;
        for provider in &providers_to_touch {
            let Ok(p_roots) = roots(provider) else {
                continue;
            };
            let has_entry = p_roots.iter().any(|r| {
                r.kind == "skill"
                    && !r.is_builtin()
                    && (r.path.join(&source_id.name).join("SKILL.md").is_file()
                        || r.path
                            .join(&source_id.name)
                            .join("SKILL.md.disabled")
                            .is_file())
            });
            if has_entry {
                any_existing = true;
                set_provider_enabled(source_id, provider, true, mode_for(provider))?;
            }
        }
        if !any_existing {
            for provider in &providers_to_touch {
                set_provider_enabled(source_id, provider, true, mode_for(provider))?;
            }
        }
        Ok(true)
    } else {
        for provider in &providers_to_touch {
            set_provider_enabled(source_id, provider, false, mode_for(provider))?;
        }
        Ok(false)
    }
}

fn convert_provider_sync_mode(target_provider: &str, sync_mode: &str) -> Result<usize, String> {
    let target_roots = roots(target_provider)?;
    let mut converted = 0;

    for root in target_roots {
        if root.kind != "skill" || root.is_builtin() {
            continue;
        }
        let Ok(entries) = fs::read_dir(&root.path) else {
            continue;
        };
        for item in entries.flatten() {
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !valid_skill_name(&name) {
                continue;
            }
            let dir = item.path();
            if !dir.join("SKILL.md").is_file() {
                continue;
            }
            let canon = fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            let anchor = ensure_central_anchor(&canon, &name)?;
            if sync_mode == "copy" && is_link(&dir) {
                remove_link_only(&dir)?;
                copy_dir_recursive(&anchor, &dir)?;
                converted += 1;
            } else if sync_mode == "link" && !is_link(&dir) {
                replace_copy_with_link(&dir, &anchor)?;
                converted += 1;
            }
        }
    }
    Ok(converted)
}

#[tauri::command]
pub(crate) async fn list_skill_library() -> Result<Vec<SkillLibraryItem>, String> {
    tauri::async_runtime::spawn_blocking(build_skill_library)
        .await
        .map_err(|_| "读取技能库任务失败")?
}

#[tauri::command]
pub(crate) async fn set_skill_provider_enabled(
    source_id: CustomizationId,
    target_provider: String,
    enabled: bool,
    sync_mode: Option<String>,
) -> Result<bool, String> {
    let mode = sync_mode.unwrap_or_else(|| "link".into());
    tauri::async_runtime::spawn_blocking(move || {
        set_provider_enabled(&source_id, &target_provider, enabled, &mode)
    })
    .await
    .map_err(|_| "切换工具技能状态任务失败")?
}

#[tauri::command]
pub(crate) async fn set_skill_all_enabled(
    source_id: CustomizationId,
    enabled: bool,
    sync_mode: Option<String>,
    target_providers: Option<Vec<String>>,
    provider_sync_modes: Option<HashMap<String, String>>,
) -> Result<bool, String> {
    let mode = sync_mode.unwrap_or_else(|| "link".into());
    let providers = target_providers.unwrap_or_default();
    let mode_map = provider_sync_modes.unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || {
        set_all_providers_enabled(&source_id, enabled, &mode, providers, &mode_map)
    })
    .await
    .map_err(|_| "批量切换技能状态任务失败")?
}

#[tauri::command]
pub(crate) async fn apply_provider_sync_mode(
    target_provider: String,
    sync_mode: String,
) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        convert_provider_sync_mode(&target_provider, &sync_mode)
    })
    .await
    .map_err(|_| "切换应用同步方式任务失败")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_single_and_multiline_frontmatter_description() {
        let single =
            "---\nname: tdd\ndescription: \"Test-driven development workflow.\"\n---\n# TDD";
        assert_eq!(
            extract_frontmatter_description(single),
            "Test-driven development workflow."
        );

        let multi = "---\nname: demo\ndescription: >\n  First line of description\n  second line here.\nauthor: test\n---\nBody";
        assert_eq!(
            extract_frontmatter_description(multi),
            "First line of description second line here."
        );

        assert_eq!(extract_frontmatter_description("# No frontmatter"), "");
    }

    #[test]
    fn normalizes_github_groups_from_various_refs() {
        assert_eq!(
            extract_github_owner_repo("mattpcock/skills/codebase-design"),
            Some((
                "mattpcock/skills".into(),
                "https://github.com/mattpcock/skills.git".into()
            ))
        );
        assert_eq!(
            extract_github_owner_repo("https://github.com/vercel-labs/agent-skills.git"),
            Some((
                "vercel-labs/agent-skills".into(),
                "https://github.com/vercel-labs/agent-skills.git".into()
            ))
        );
        assert_eq!(
            extract_github_owner_repo("git@github.com:pbakaus/impeccable.git"),
            Some((
                "pbakaus/impeccable".into(),
                "https://github.com/pbakaus/impeccable.git".into()
            ))
        );
    }
}
