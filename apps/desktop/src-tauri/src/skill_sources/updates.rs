use super::manager::{managed, manager};
use super::source::{clone_source, discover, git, source_from};
use super::storage::{copy_skill, read_meta, skill_root, snapshot, stage, write_meta};
use super::{Metadata, UpdateInfo};
use crate::agent_customizations::{display_path, resolve, CustomizationId};
use std::fs;

pub(super) fn check(id: &CustomizationId) -> Result<UpdateInfo, String> {
    if let Some(skill) = managed(id)? {
        let report = manager(&["skills", "check", &skill.id])?;
        let row = report
            .as_array()
            .and_then(|rows| rows.first())
            .ok_or("Skills Manager 检查结果为空")?;
        let status = row
            .get("update_status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let detail = match status {
            "available" | "update_available" => "Skills Manager 检测到更新",
            "up_to_date" | "current" => "Skills Manager 未发现更新",
            "source_missing" => "Skills Manager 找不到原始来源",
            _ => "Skills Manager 无法确认是否有更新，可尝试手动更新",
        };
        return Ok(UpdateInfo {
            kind: "manager".into(),
            source: skill.source_ref,
            status: status.into(),
            detail: format!("{detail}（来源类型：{}）", skill.source_type),
        });
    }
    let (_, path) = resolve(id)?;
    let dir = path.parent().ok_or("无效路径")?;
    let Some(meta) = read_meta(dir)? else {
        return Ok(UpdateInfo {
            kind: "unknown".into(),
            source: None,
            status: "unsupported".into(),
            detail: "没有可验证的来源记录，无法自动更新".into(),
        });
    };
    let changed = snapshot(dir)? != meta.files;
    let (_tmp, _repo, latest) = clone_source(&meta.source)?;
    Ok(UpdateInfo {
        kind: "independent".into(),
        source: Some(meta.source.url),
        status: if changed {
            "local_changes"
        } else if latest == meta.revision {
            "current"
        } else {
            "available"
        }
        .into(),
        detail: if changed {
            "本地文件已修改，更新已锁定"
        } else if latest == meta.revision {
            "当前已是最新版本"
        } else {
            "检测到仓库更新"
        }
        .into(),
    })
}
pub(super) fn update(id: &CustomizationId) -> Result<String, String> {
    let root = skill_root(id)?;
    if let Some(skill) = managed(id)? {
        let report = manager(&["skills", "update", &skill.id])?;
        if report.to_string().contains("held_back_removals") {
            return Err(format!("Skills Manager 保留了更新以保护文件：{report}"));
        }
        let refreshed = report.get("refreshed").and_then(|v| v.as_bool()) == Some(true)
            || report.as_array().is_some_and(|rows| {
                rows.iter()
                    .any(|r| r.get("refreshed").and_then(|v| v.as_bool()) == Some(true))
            });
        return Ok(if refreshed {
            "Skills Manager 已更新 skill".into()
        } else {
            "Skills Manager 未应用更新，请检查来源或稍后重试".into()
        });
    }
    let target = root.join(&id.name);
    if fs::symlink_metadata(&target)
        .map_err(|_| "skill 不存在")?
        .file_type()
        .is_symlink()
    {
        return Err("独立更新不支持软链接，请通过原管理工具更新".into());
    }
    let meta = read_meta(&target)?.ok_or("没有可验证的来源记录，无法自动更新")?;
    if snapshot(&target)? != meta.files {
        return Err("本地文件已修改，请先备份或还原后再更新".into());
    }
    let (_tmp, repo, revision) = clone_source(&meta.source)?;
    if revision == meta.revision {
        return Ok("当前已是最新版本".into());
    }
    if !discover(&repo, &meta.source)?
        .iter()
        .any(|c| c.path == meta.skill_path)
    {
        return Err("来源中找不到原 skill".into());
    }
    let from = repo.join(&meta.skill_path);
    let files = snapshot(&from)?;
    let staging = stage(&root)?;
    let backup = stage(&root)?.with_extension("backup");
    let result = (|| {
        copy_skill(&from, &staging, &files)?;
        write_meta(
            &staging,
            &Metadata {
                source: meta.source,
                revision,
                skill_path: meta.skill_path,
                files,
            },
        )?;
        if snapshot(&target)? != meta.files {
            return Err("文件在更新期间发生改变，已取消".into());
        }
        fs::rename(&target, &backup).map_err(|e| format!("准备更新失败：{e}"))?;
        if let Err(e) = fs::rename(&staging, &target) {
            if fs::rename(&backup, &target).is_err() {
                return Err(format!("更新失败，原文件保留在 {}", display_path(&backup)));
            }
            return Err(format!("更新失败，原文件已恢复：{e}"));
        }
        fs::remove_dir_all(&backup)
            .map_err(|_| format!("更新完成，原文件备份保留在 {}", display_path(&backup)))?;
        Ok("skill 已更新".into())
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

pub(super) fn detect_source(id: &CustomizationId) -> Result<Option<String>, String> {
    if let Some(skill) = managed(id)? {
        if let Some(s) = skill.source_ref {
            return Ok(Some(s));
        }
    }
    let (_, path) = resolve(id)?;
    let dir = path.parent().ok_or("无效路径")?;
    let git_dir = dir.join(".git");
    if git_dir.exists() {
        if let Ok(url) = git(&["config", "--get", "remote.origin.url"], Some(dir)) {
            let trimmed = url.trim();
            if !trimmed.is_empty() {
                let https_url = if let Some(rem) = trimmed.strip_prefix("git@github.com:") {
                    format!("https://github.com/{}", rem.trim_end_matches(".git"))
                } else if trimmed.starts_with("https://") {
                    trimmed.trim_end_matches(".git").to_string()
                } else {
                    trimmed.to_string()
                };
                return Ok(Some(https_url));
            }
        }
    }
    Ok(None)
}

pub(super) fn adopt(
    id: &CustomizationId,
    source_input: &str,
    candidate_path: Option<&str>,
) -> Result<UpdateInfo, String> {
    skill_root(id)?;
    let source = source_from(source_input)?;
    let (_tmp, repo, latest) = clone_source(&source)?;
    let candidates = discover(&repo, &source)?;

    let matched = if let Some(cp) = candidate_path {
        candidates
            .iter()
            .find(|c| c.path == cp)
            .cloned()
            .ok_or("指定的子目录不是有效的 skill")?
    } else if candidates.len() == 1 {
        candidates[0].clone()
    } else if let Some(m) = candidates.iter().find(|c| c.name == id.name) {
        m.clone()
    } else {
        return Err("来源包含多个 skill，请指定具体子目录".into());
    };

    let (_, path) = resolve(id)?;
    let dir = path.parent().ok_or("无效路径")?;
    let canonical_dir = if dir.is_symlink() {
        fs::canonicalize(dir).map_err(|e| format!("解析符号链接失败：{e}"))?
    } else {
        dir.to_path_buf()
    };

    let files = snapshot(&canonical_dir)?;
    write_meta(
        &canonical_dir,
        &Metadata {
            source,
            revision: latest,
            skill_path: matched.path,
            files,
        },
    )?;

    check(id)
}
