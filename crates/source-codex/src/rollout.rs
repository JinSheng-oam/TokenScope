use chrono::DateTime;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::model::{CodexError, Result};

const MAX_LINE: usize = 4 * 1024 * 1024;
const TAIL_WINDOW: usize = 256;
const MAX_DECOMPRESSED: usize = 512 * 1024 * 1024;

fn hex_digest(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}
pub(crate) fn now_ms() -> Result<i64> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CodexError::Invalid("clock"))?
        .as_millis();
    i64::try_from(ms).map_err(|_| CodexError::Invalid("clock overflow"))
}
pub(crate) fn mtime_ns(metadata: &fs::Metadata) -> Result<u128> {
    Ok(metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| CodexError::Invalid("mtime"))?
        .as_nanos())
}
pub(crate) fn event_ms(v: &Value) -> Result<Option<i64>> {
    let Some(s) = v.get("timestamp").and_then(Value::as_str) else {
        return Ok(None);
    };
    Ok(Some(
        DateTime::parse_from_rfc3339(s)
            .map_err(|_| CodexError::Invalid("timestamp"))?
            .timestamp_millis(),
    ))
}

// Reads at most MAX_LINE bytes even if JSONL contains a multi-gigabyte prompt
// line. An oversized line is drained and reported as `oversized` so the caller
// can keep scanning the rest of the file instead of discarding it entirely.
pub(crate) struct LineRead {
    pub(crate) bytes: Vec<u8>,
    pub(crate) complete: bool,
    pub(crate) oversized: bool,
}

pub(crate) fn bounded_line<R: BufRead>(reader: &mut R) -> Result<LineRead> {
    let mut result = Vec::new();
    let mut oversized = false;
    loop {
        let buf = reader.fill_buf()?;
        if buf.is_empty() {
            return Ok(LineRead {
                bytes: result,
                complete: false,
                oversized,
            });
        }
        let size = buf
            .iter()
            .position(|&x| x == b'\n')
            .map_or(buf.len(), |i| i + 1);
        let ended = buf[..size].last() == Some(&b'\n');
        if oversized || result.len().saturating_add(size) > MAX_LINE {
            oversized = true;
            result.clear();
            reader.consume(size);
            if ended {
                return Ok(LineRead {
                    bytes: Vec::new(),
                    complete: true,
                    oversized: true,
                });
            }
            continue;
        }
        result.extend_from_slice(&buf[..size]);
        reader.consume(size);
        if ended {
            return Ok(LineRead {
                bytes: result,
                complete: true,
                oversized,
            });
        }
    }
}
/// Codex rolls completed rollouts into `*.jsonl.zst`. Their payload is the same
/// JSONL stream; only the container differs.
pub fn is_zstd_rollout(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".jsonl.zst"))
}

/// Streams a zstd rollout into memory with a hard cap, so a corrupt or
/// adversarial frame cannot exhaust the process. The largest observed rollout
/// is ~100 MiB uncompressed.
pub fn decompress_rollout(path: &Path) -> Result<Vec<u8>> {
    let mut decoder = zstd::stream::read::Decoder::new(BufReader::new(File::open(path)?))?;
    let mut out = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let read = decoder.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if out.len().saturating_add(read) > MAX_DECOMPRESSED {
            return Err(CodexError::Limit("decompressed rollout > 512 MiB"));
        }
        out.extend_from_slice(&chunk[..read]);
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub(crate) struct SessionHeader {
    pub id: String,
    pub cwd: Option<String>,
}

pub fn header_from_bytes(bytes: &[u8]) -> Result<String> {
    Ok(session_header_from_reader(&mut BufReader::new(bytes))?.id)
}

pub(crate) fn read_session_header(path: &Path) -> Result<String> {
    Ok(read_session_header_metadata(path)?.id)
}

pub(crate) fn session_header_from_bytes(bytes: &[u8]) -> Result<SessionHeader> {
    session_header_from_reader(&mut BufReader::new(bytes))
}

pub(crate) fn read_session_header_metadata(path: &Path) -> Result<SessionHeader> {
    if is_zstd_rollout(path) {
        let mut decoder = zstd::stream::read::Decoder::new(BufReader::new(File::open(path)?))?;
        return session_header_from_reader(&mut BufReader::new(&mut decoder));
    }
    session_header_from_reader(&mut BufReader::new(File::open(path)?))
}

fn session_header_from_reader<R: BufRead>(rdr: &mut R) -> Result<SessionHeader> {
    let line = bounded_line(rdr)?;
    if line.oversized {
        return Err(CodexError::Invalid("oversized session header"));
    }
    if !line.complete {
        return Err(CodexError::PendingHeader);
    }
    let v: Value = serde_json::from_slice(&line.bytes)?;
    if v.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Err(CodexError::Invalid("not Codex rollout session_meta"));
    }
    let payload = v
        .get("payload")
        .ok_or(CodexError::Invalid("session payload"))?;
    // Codex Desktop and CLI names vary. Reject explicit foreign originators.
    if let Some(origin) = payload.get("originator").and_then(Value::as_str) {
        if !origin.to_ascii_lowercase().contains("codex") {
            return Err(CodexError::Invalid("non-Codex originator"));
        }
    }
    let id = payload
        .get("id")
        .or_else(|| payload.get("session_id"))
        .and_then(Value::as_str)
        .ok_or(CodexError::Invalid("missing session id"))?;
    if id.len() < 8
        || id.len() > 128
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(CodexError::Invalid("bad session id"));
    }
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|value| {
            !value.trim().is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned);
    Ok(SessionHeader {
        id: id.to_owned(),
        cwd,
    })
}
pub(crate) fn sha_tail(path: &Path, offset: u64) -> Result<String> {
    let mut f = File::open(path)?;
    let begin = offset.saturating_sub(TAIL_WINDOW as u64);
    f.seek(SeekFrom::Start(begin))?;
    let count = usize::try_from(offset - begin).map_err(|_| CodexError::Invalid("offset"))?;
    let mut buf = vec![0u8; count];
    f.read_exact(&mut buf)?;
    Ok(hex_digest(&buf))
}
pub fn sha_tail_bytes(bytes: &[u8]) -> String {
    let begin = bytes.len().saturating_sub(TAIL_WINDOW);
    hex_digest(&bytes[begin..])
}
pub(crate) fn hash_path(path: &Path) -> Result<String> {
    // Path stays local; store only a digest for change detection.
    Ok(hex_digest(
        path.canonicalize()?.to_string_lossy().as_bytes(),
    ))
}
