use crate::{
    model::{Inventory, LogCandidate, SessionHeader},
    DeepSeekHarnessError, Result, MAX_LINE_BYTES, MAX_SESSION_DIRECTORIES, TAIL_BYTES, TOOL_ID,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata},
    io::{self, BufRead, Read, Seek, SeekFrom},
    path::Path,
    time::UNIX_EPOCH,
};
use tokenscope_core_domain::project_key;
use zstd::stream::read::Decoder;

pub(super) fn parse_generation(name: &str) -> Option<(u32, bool)> {
    let (stem, compressed) = if let Some(stem) = name.strip_suffix(".jsonl.zstd") {
        (stem, true)
    } else if let Some(stem) = name.strip_suffix(".jsonl") {
        (stem, false)
    } else {
        return None;
    };
    if stem == "session" {
        return Some((0, compressed));
    }
    let digits = stem.strip_prefix("session.v")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let generation = digits.parse::<u32>().ok()?;
    (generation > 0).then_some((generation, compressed))
}

pub(super) fn discover_sessions(home: &Path) -> Result<Inventory> {
    let sessions_root = home.join("sessions");
    let root_metadata = match fs::symlink_metadata(&sessions_root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Inventory::default()),
        Err(error) => return Err(error.into()),
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Ok(Inventory::default());
    }

    let mut inventory = Inventory {
        available: true,
        complete: true,
        ..Inventory::default()
    };
    let mut directory_count = 0usize;
    for project_entry in fs::read_dir(&sessions_root)? {
        let project_entry = project_entry?;
        let project_type = project_entry.file_type()?;
        if project_type.is_symlink() {
            inventory.complete = false;
            inventory.skipped = inventory.skipped.saturating_add(1);
            continue;
        }
        if !project_type.is_dir() {
            continue;
        }
        for session_entry in fs::read_dir(project_entry.path())? {
            let session_entry = session_entry?;
            let session_type = session_entry.file_type()?;
            if session_type.is_symlink() {
                inventory.complete = false;
                inventory.skipped = inventory.skipped.saturating_add(1);
                continue;
            }
            if !session_type.is_dir() {
                continue;
            }
            directory_count = directory_count.saturating_add(1);
            if directory_count > MAX_SESSION_DIRECTORIES {
                return Err(DeepSeekHarnessError::Limit("too many session directories"));
            }

            let mut candidates = Vec::new();
            for file_entry in fs::read_dir(session_entry.path())? {
                let file_entry = file_entry?;
                let file_type = file_entry.file_type()?;
                if file_type.is_symlink() {
                    inventory.complete = false;
                    inventory.skipped = inventory.skipped.saturating_add(1);
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let Some(name) = file_entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                let Some((generation, compressed)) = parse_generation(&name) else {
                    continue;
                };
                candidates.push(LogCandidate {
                    path: file_entry.path(),
                    generation,
                    compressed,
                });
            }
            if candidates.is_empty() {
                continue;
            }
            let latest_generation = candidates
                .iter()
                .map(|candidate| candidate.generation)
                .max()
                .unwrap_or_default();
            let mut latest = candidates
                .into_iter()
                .filter(|candidate| candidate.generation == latest_generation);
            let Some(candidate) = latest.next() else {
                continue;
            };
            if latest.next().is_some() {
                // A root must have one physical encoding per generation.
                inventory.complete = false;
                inventory.skipped = inventory.skipped.saturating_add(1);
                continue;
            }
            inventory.paths.push(candidate);
        }
    }
    if inventory.paths.len() > MAX_SESSION_DIRECTORIES {
        return Err(DeepSeekHarnessError::Limit("too many session logs"));
    }
    inventory
        .paths
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(inventory)
}

pub(super) fn open_log_reader(path: &Path, compressed: bool) -> Result<Box<dyn Read>> {
    let file = File::open(path)?;
    if compressed {
        // The default streaming Decoder consumes concatenated frames, which is
        // how Harness appends each durable flush to a compressed session log.
        Ok(Box::new(Decoder::new(file)?))
    } else {
        Ok(Box::new(file))
    }
}

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
            .position(|&byte| byte == b'\n')
            .map_or(chunk.len(), |index| index + 1);
        if output
            .len()
            .checked_add(until)
            .is_none_or(|length| length > MAX_LINE_BYTES)
        {
            return Err(DeepSeekHarnessError::Limit("JSONL row exceeds 16 MiB"));
        }
        let complete = chunk[until - 1] == b'\n';
        output.extend_from_slice(&chunk[..until]);
        reader.consume(until);
        if complete {
            return Ok(Some((output, true)));
        }
    }
}

pub(super) fn json_line(mut line: Vec<u8>) -> Result<Value> {
    while matches!(line.last(), Some(b'\n' | b'\r')) {
        line.pop();
    }
    Ok(serde_json::from_slice(&line)?)
}

pub(super) fn json_time(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
        .filter(|time| *time >= 0)
}

pub(super) fn parse_header(value: &Value) -> Result<SessionHeader> {
    if value.get("type").and_then(Value::as_str) != Some("session") {
        return Err(DeepSeekHarnessError::Invalid("missing session header"));
    }
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control))
        .ok_or(DeepSeekHarnessError::Invalid("session identity"))?
        .to_owned();
    let cwd = value
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| cwd.len() <= 4096 && !cwd.chars().any(char::is_control))
        .map(str::to_owned);
    let seed_length = value.get("seedLength").and_then(Value::as_u64);
    Ok(SessionHeader {
        version: value.get("version").and_then(Value::as_u64).unwrap_or(0),
        id,
        cwd,
        created_at_ms: json_time(value.get("createdAt")),
        is_seeded: value
            .get("isSeeded")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        seed_length,
    })
}

fn normalized_path_identity(path: &Path) -> String {
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

pub(super) fn project_metadata(cwd: &str) -> Option<(String, String)> {
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
    let project_id = project_key(TOOL_ID, normalized_path_identity(root).as_bytes()).ok()?;
    Some((project_id, name.to_owned()))
}

pub(super) fn clean_title(value: &str) -> Option<String> {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() || collapsed.chars().any(char::is_control) {
        return None;
    }
    Some(collapsed.chars().take(80).collect())
}

fn modified_ns(metadata: &Metadata) -> Result<u128> {
    metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| DeepSeekHarnessError::Invalid("file time before epoch"))
}

pub(super) fn fingerprint(path: &Path, metadata: &Metadata, compressed: bool) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(metadata.len().saturating_sub(TAIL_BYTES)))?;
    let mut tail = Vec::with_capacity(TAIL_BYTES as usize);
    file.take(TAIL_BYTES).read_to_end(&mut tail)?;
    Ok(format!(
        "v1:{}:{}:{}:{:x}",
        u8::from(compressed),
        metadata.len(),
        modified_ns(metadata)?,
        Sha256::digest(tail)
    ))
}

pub(super) fn current_metadata(
    path: &Path,
    before: &Metadata,
    fingerprint_before: &str,
    compressed: bool,
) -> Result<bool> {
    let after = fs::symlink_metadata(path)?;
    if !after.file_type().is_file() || after.file_type().is_symlink() {
        return Ok(false);
    }
    if after.len() != before.len() || modified_ns(&after)? != modified_ns(before)? {
        return Ok(false);
    }
    Ok(fingerprint(path, &after, compressed)? == fingerprint_before)
}
