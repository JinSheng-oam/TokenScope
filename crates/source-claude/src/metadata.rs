use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File, Metadata};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokenscope_core_domain::{project_key, session_key};

use super::model::{ClaudeError, Result};

const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
const MAX_FILES: usize = 20_000;
const TAIL_BYTES: u64 = 256;

pub(super) fn clean_title(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty() {
        return None;
    }
    let text: String = trimmed.chars().take(80).collect();
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text)
}

fn project_name_from_cwd(cwd: &str) -> Option<String> {
    let path = Path::new(cwd);
    if cwd.len() > 4096 || !path.is_absolute() || cwd.chars().any(char::is_control) {
        return None;
    }
    let mut root = path;
    for ancestor in path.ancestors().take(64) {
        if ancestor.join(".git").exists() {
            root = ancestor;
            break;
        }
    }
    let name = root.file_name()?.to_str()?.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
        || name.contains('/')
        || name.contains('\\')
    {
        return None;
    }
    Some(name.to_owned())
}

pub(super) fn project_name_from_transcript(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    for _ in 0..32 {
        let (line, _) = bounded_line(&mut reader).ok()??;
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if let Some(name) = value
            .get("cwd")
            .and_then(Value::as_str)
            .and_then(project_name_from_cwd)
        {
            return Some(name);
        }
    }
    None
}

pub(super) fn now_ms() -> Result<i64> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ClaudeError::Invalid("system clock"))?
        .as_millis();
    i64::try_from(millis).map_err(|_| ClaudeError::Limit("timestamp"))
}
pub(super) fn modified_ns(meta: &Metadata) -> Result<u128> {
    Ok(meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ClaudeError::Invalid("mtime"))?
        .as_nanos())
}
fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut result = String::with_capacity(64);
    for b in digest {
        result.push_str(&format!("{b:02x}"));
    }
    result
}
fn tail_digest(path: &Path, size: u64) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(size.saturating_sub(TAIL_BYTES)))?;
    let mut buf = Vec::with_capacity(TAIL_BYTES as usize);
    file.take(TAIL_BYTES).read_to_end(&mut buf)?;
    // Only a 256-byte tail is needed for the *fast unchanged hint*, not for
    // content identity. Changed sources are always fully reconciled.
    Ok(hex_digest(&buf))
}
pub(super) fn fingerprint(path: &Path, meta: &Metadata) -> Result<String> {
    Ok(format!(
        "v1:{}:{}:{}",
        meta.len(),
        modified_ns(meta)?,
        tail_digest(path, meta.len())?
    ))
}

/// Allocates at most MAX_LINE_BYTES per JSONL line; unlike read_until, it cannot
/// consume unbounded memory from a corrupted transcript.
pub(super) fn bounded_line<R: BufRead>(reader: &mut R) -> Result<Option<(Vec<u8>, bool)>> {
    let mut output = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return if output.is_empty() {
                Ok(None)
            } else {
                Ok(Some((output, false)))
            };
        }
        let until = chunk
            .iter()
            .position(|&b| b == b'\n')
            .map_or(chunk.len(), |n| n + 1);
        if output
            .len()
            .checked_add(until)
            .is_none_or(|n| n > MAX_LINE_BYTES)
        {
            return Err(ClaudeError::Limit("JSONL line exceeds 4 MiB"));
        }
        let done = chunk[until - 1] == b'\n';
        output.extend_from_slice(&chunk[..until]);
        reader.consume(until);
        if done {
            return Ok(Some((output, true)));
        }
    }
}

/// Traverse only configured `projects` tree. Never traverse symlinks, even if
/// they point outside the tree, to avoid accidental disclosure or cycles.
pub fn discover_claude_files(config_dir: &Path) -> Result<Vec<PathBuf>> {
    let root = config_dir.join("projects");
    if !root.exists() {
        return Ok(Vec::new());
    }
    if !fs::symlink_metadata(&root)?.file_type().is_dir() {
        return Err(ClaudeError::Invalid(
            "projects root is not a real directory",
        ));
    }
    let mut result = Vec::new();
    let mut stack = vec![root];
    let mut visited = 0_usize;
    while let Some(dir) = stack.pop() {
        for child in fs::read_dir(dir)? {
            let entry = child?;
            visited = visited
                .checked_add(1)
                .ok_or(ClaudeError::Limit("entries"))?;
            if visited > 100_000 {
                return Err(ClaudeError::Limit("directory entries > 100000"));
            }
            let path = entry.path();
            let ty = entry.file_type()?;
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                stack.push(path);
            } else if ty.is_file() && path.extension().is_some_and(|s| s == "jsonl") {
                result.push(path);
                if result.len() > MAX_FILES {
                    return Err(ClaudeError::Limit("transcript files > 20000"));
                }
            }
        }
    }
    result.sort();
    Ok(result)
}

pub(super) fn file_identity(path: &Path, config_dir: &Path) -> Result<(String, String, String)> {
    let rel = path
        .strip_prefix(config_dir.join("projects"))
        .map_err(|_| ClaudeError::Invalid("transcript outside configured projects"))?;
    let rel_str = rel
        .to_str()
        .ok_or(ClaudeError::Invalid("unrepresentable path"))?;
    let project_name = rel
        .components()
        .next()
        .ok_or(ClaudeError::Invalid("project name"))?;
    // Hash the project and the relative transcript path: never store raw paths.
    let project = project_key(
        "claude",
        project_name.as_os_str().to_string_lossy().as_bytes(),
    )?;
    let session = session_key("claude", rel_str.as_bytes())?;
    Ok((session.clone(), session, project)) // source_key, session_id, project_id
}
