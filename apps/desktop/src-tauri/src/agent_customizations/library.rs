use super::{display_path, list, CustomizationId};
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
    // Shorthand like "mattpcock/skills/codebase-design" or "pbakaus/impeccable"
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

fn canonical_group_key(path: &str, id: &CustomizationId) -> (String, Option<PathBuf>) {
    if path.is_empty() {
        return (format!("builtin:{}:{}", id.root, id.name), None);
    }
    let file_path = PathBuf::from(path);
    let Some(parent) = file_path.parent() else {
        return (format!("entry:{}:{}:{}", id.provider, id.root, id.name), None);
    };
    let canon = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let normalized = display_path(&canon);
    let key = if cfg!(windows) {
        normalized.to_ascii_lowercase()
    } else {
        normalized
    };
    (key, Some(canon))
}

fn build_skill_library() -> Result<Vec<SkillLibraryItem>, String> {
    let manager_index = load_manager_source_index();
    let mut items: BTreeMap<String, SkillLibraryItem> = BTreeMap::new();

    for provider in PROVIDERS {
        let Ok(entries) = list(provider) else {
            continue;
        };
        for entry in entries {
            if entry.kind != "skill" {
                continue;
            }
            let (key, dir) = canonical_group_key(&entry.path, &entry.id);
            let deployment = SkillDeployment {
                id: entry.id.clone(),
                provider: entry.id.provider.clone(),
                root: entry.id.root.clone(),
                path: entry.path.clone(),
                enabled: entry.enabled,
                linked: entry.linked,
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
                if !existing.editable && deployment.editable {
                    existing.editable = true;
                    existing.primary_id = deployment.id.clone();
                }
                if existing.builtin && !deployment.builtin {
                    existing.builtin = false;
                    existing.primary_id = deployment.id.clone();
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

#[tauri::command]
pub(crate) async fn list_skill_library() -> Result<Vec<SkillLibraryItem>, String> {
    tauri::async_runtime::spawn_blocking(build_skill_library)
        .await
        .map_err(|_| "读取技能库任务失败")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_single_and_multiline_frontmatter_description() {
        let single = "---\nname: tdd\ndescription: \"Test-driven development workflow.\"\n---\n# TDD";
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
