use super::{io, MAX_LINE};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};
use tokenscope_core_domain::session_key;

pub(super) fn locate(tool: &str, id: &str) -> Result<(Vec<PathBuf>, Option<usize>), String> {
    let (env, fallback, roots): (_, _, &[_]) = match tool {
        "codex" => ("CODEX_HOME", ".codex", &["sessions", "archived_sessions"]),
        "claude" => ("CLAUDE_CONFIG_DIR", ".claude", &["projects"]),
        "deepseek-harness" => ("DSH_HOME", ".dsh", &["sessions"]),
        _ => return Err("此 Agent 暂不支持历史会话编辑".into()),
    };
    let home = crate::session_activity::tool_home(env, fallback).ok_or("未找到 Agent 目录")?;
    locate_in(tool, id, &home, roots)
}

pub(super) fn locate_in(
    tool: &str,
    id: &str,
    home: &Path,
    roots: &[&str],
) -> Result<(Vec<PathBuf>, Option<usize>), String> {
    let mut candidates = Vec::new();
    for name in roots {
        let root = home.join(name);
        if !root.is_dir() || crate::agent_customizations::is_link(&root) {
            continue;
        }
        let mut pending = vec![root.clone()];
        let mut count = 0;
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(dir).map_err(io)? {
                let entry = entry.map_err(io)?;
                count += 1;
                if count > 100_000 {
                    return Err("Agent 目录过大，无法安全定位会话".into());
                }
                let path = entry.path();
                let ty = entry.file_type().map_err(io)?;
                if crate::agent_customizations::is_link(&path) {
                    continue;
                }
                if ty.is_dir() {
                    pending.push(path);
                    continue;
                }
                if !ty.is_file() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                    continue;
                };
                let Some(stem) = name.strip_suffix(".jsonl").or_else(|| {
                    (tool == "codex")
                        .then(|| name.strip_suffix(".jsonl.zst"))
                        .flatten()
                }) else {
                    continue;
                };
                let native = if tool == "claude" {
                    path.strip_prefix(&root)
                        .ok()
                        .and_then(Path::to_str)
                        .map(str::to_owned)
                } else if tool == "codex" {
                    if name.ends_with(".zst") {
                        stem.rsplit_once('T')
                            .and_then(|(_, tail)| tail.get(9..))
                            .and_then(|ids| ids.split('_').next())
                            .map(str::to_owned)
                    } else {
                        header_id(&path, tool)
                    }
                } else {
                    header_id(&path, tool)
                };
                if let Some(native) = native {
                    candidates.push((native, path));
                }
            }
        }
    }
    let mut preferred = None;
    let native = candidates
        .iter()
        .find(|(native, _)| matches_session(tool, native, id))
        .map(|(native, _)| native.clone())
        .or_else(|| {
            if tool != "codex" {
                return None;
            }
            // Older collectors exposed a path-derived key for each continuation.
            candidates.iter().find_map(|(native, path)| {
                let legacy = legacy_codex_identity(native, path)?;
                if !matches_session(tool, &legacy, id) {
                    return None;
                }
                preferred = Some(path.clone());
                Some(native.clone())
            })
        });
    let mut matches: Vec<_> = candidates
        .into_iter()
        .filter_map(|(candidate, path)| (Some(&candidate) == native.as_ref()).then_some(path))
        .collect();
    if matches.is_empty() {
        return Err("未找到对应的本地会话文件；文件可能已移走或不在当前 Agent 目录中。".into());
    }
    if tool != "codex" && matches.len() > 1 {
        return Err("此会话对应多个源文件，暂不支持改写".into());
    }
    matches.sort_by(|a, b| a.file_name().cmp(&b.file_name()).then_with(|| a.cmp(b)));
    let index = preferred.and_then(|path| matches.iter().position(|p| *p == path));
    Ok((matches, index))
}

fn matches_session(tool: &str, native: &str, id: &str) -> bool {
    session_key(tool, native.as_bytes()).ok().as_deref() == Some(id)
}

pub(super) fn legacy_codex_identity(native: &str, path: &Path) -> Option<String> {
    let canonical = path.canonicalize().ok()?;
    let digest = format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    );
    Some(format!("{native}#{}", &digest[..8]))
}

pub(super) fn antigravity_transcripts(home: &Path, id: &str) -> Result<Vec<PathBuf>, String> {
    let mut paths = Vec::new();
    for product in [
        "antigravity",
        "antigravity-cli",
        "antigravity-ide",
        "antigravity-backup",
    ] {
        let root = home.join(product).join("brain");
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(e)),
        };
        let canonical_root = root.canonicalize().map_err(io)?;
        for (index, entry) in entries.enumerate() {
            if index >= 20_000 {
                return Err("Antigravity 会话目录过大".into());
            }
            let entry = entry.map_err(io)?;
            if !entry.file_type().map_err(io)?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let Some(native) = name.to_str() else {
                continue;
            };
            if tokenscope_core_domain::stable_record_id(
                "antigravity",
                b"conversation",
                native.as_bytes(),
            )
            .ok()
            .as_deref()
                != Some(id)
            {
                continue;
            }
            let transcript = entry
                .path()
                .join(".system_generated")
                .join("logs")
                .join("transcript.jsonl");
            let canonical = match transcript.canonicalize() {
                Ok(path) => path,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(io(e)),
            };
            if canonical.starts_with(&canonical_root)
                && canonical.is_file()
                && !paths.contains(&canonical)
            {
                paths.push(canonical);
            }
        }
    }
    if paths.is_empty() {
        return Err(
            "未找到这个 Antigravity 会话的本地正文日志；旧格式或未生成日志的会话暂不可查看。"
                .into(),
        );
    }
    Ok(paths)
}

pub(super) fn part_path(paths: &[PathBuf], part: usize) -> Result<&Path, String> {
    let path = paths.get(part).ok_or("会话分段已变化，请重新打开")?;
    if path.extension().is_some_and(|ext| ext == "zst") {
        return Err("此分段已压缩归档，暂不支持编辑".into());
    }
    Ok(path)
}

pub(super) fn file_revision(path: &Path, content_revision: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(content_revision.as_bytes());
    hash.update(path.as_os_str().as_encoded_bytes());
    format!("{:x}", hash.finalize())
}

fn header_id(path: &Path, tool: &str) -> Option<String> {
    let mut reader = BufReader::new(File::open(path).ok()?.take(MAX_LINE as u64 + 1));
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line).ok()?;
    let value: Value = serde_json::from_slice(&line).ok()?;
    match (tool, value.get("type")?.as_str()?) {
        ("codex", "session_meta") => value.pointer("/payload/id")?.as_str().map(str::to_owned),
        ("deepseek-harness" | "pi", "session") => value.get("id")?.as_str().map(str::to_owned),
        _ => None,
    }
}
