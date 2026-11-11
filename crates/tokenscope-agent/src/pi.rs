use chrono::{DateTime, Utc};
use serde_json::Value;
use std::{
    collections::HashSet,
    fs::{self, File, Metadata},
    io::{self, BufRead, BufReader, Read},
    path::Path,
    time::UNIX_EPOCH,
};
use tokenscope_core_domain::{
    project_key, session_key, stable_record_id, CanonicalRecord, Precision, TokenBreakdown,
    UsageScope,
};
use tokenscope_local_store::{LocalStore, SourceCheckpoint};

const TOOL: &str = "pi";
const VERSION: u32 = 1;
const MAX_LINE: usize = 16 * 1024 * 1024;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Default, Debug)]
pub(super) struct Report {
    pub files: usize,
    pub cached: usize,
    pub skipped: usize,
    pub inserted: usize,
    pub revised: usize,
}

fn text<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key)?.as_str().filter(|s| !s.trim().is_empty())
}

fn timestamp(row: &Value) -> Option<i64> {
    let value = row.get("timestamp")?;
    value.as_i64().or_else(|| {
        DateTime::parse_from_rfc3339(value.as_str()?)
            .ok()
            .map(|time| time.timestamp_millis())
    })
}

fn next_row(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut line = Vec::new();
    reader
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() > MAX_LINE || !line.ends_with(b"\n") {
        return Err("oversized or incomplete Pi log row".into());
    }
    Ok(Some(
        serde_json::from_slice(&line).map_err(|_| "invalid Pi log row")?,
    ))
}

fn fingerprint(metadata: &Metadata) -> io::Result<String> {
    let modified = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Ok(format!("{}:{}", metadata.len(), modified.as_nanos()))
}

fn title(value: &str) -> Option<String> {
    let value: String = value
        .trim()
        .chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .take(400)
        .collect();
    let value: String = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect();
    (!value.is_empty()).then_some(value)
}

fn project(header: &Value) -> Option<(String, String)> {
    let cwd = Path::new(text(header, "cwd")?);
    if !cwd.is_absolute() {
        return None;
    }
    let root = cwd
        .ancestors()
        .take(64)
        .find(|p| p.join(".git").exists())
        .unwrap_or(cwd);
    let name = title(root.file_name()?.to_str()?)?;
    let path = root.to_string_lossy();
    let identity = if cfg!(windows) {
        path.replace('\\', "/").to_lowercase()
    } else {
        path.into_owned()
    };
    Some((project_key(TOOL, identity.as_bytes()).ok()?, name))
}

fn parse_entries(
    reader: &mut impl BufRead,
    header: &Value,
    key: &str,
    device: &str,
    project_id: Option<&str>,
) -> Result<(Option<String>, Vec<CanonicalRecord>)> {
    let created = timestamp(header).ok_or("missing Pi session timestamp")?;
    let fork = text(header, "parentSession").is_some();
    let mut first_prompt = None;
    let mut name = None;
    let mut records = Vec::new();
    let mut seen = HashSet::new();
    let mut ordinal = 0u64;
    while let Some(row) = next_row(reader)? {
        ordinal += 1;
        let kind = text(&row, "type");
        if kind == Some("session_info") {
            name = text(&row, "name").and_then(title);
        }
        let message = row.get("message").unwrap_or(&Value::Null);
        let role = text(message, "role");
        if kind == Some("message") && role == Some("user") && first_prompt.is_none() {
            if let Some(content) = message.get("content") {
                first_prompt = content.as_str().and_then(title).or_else(|| {
                    content.as_array()?.iter().find_map(|block| {
                        (text(block, "type") == Some("text"))
                            .then(|| text(block, "text").and_then(title))
                            .flatten()
                    })
                });
            }
        }
        let event = match kind {
            Some("message") if matches!(role, Some("assistant" | "toolResult")) => message,
            Some("usage" | "compaction" | "branch_summary") => &row,
            _ => continue,
        };
        let Some(counters) = event.get("usage").filter(|v| v.is_object()) else {
            continue;
        };
        let event_at = timestamp(&row).or_else(|| timestamp(event));
        // Pi copies ancestor entries with their original timestamps into forks.
        if fork && event_at.is_none_or(|time| time < created) {
            continue;
        }
        let count = |field| {
            counters
                .get(field)
                .and_then(Value::as_u64)
                .unwrap_or_default()
        };
        let mut usage = TokenBreakdown {
            input_uncached_tokens: count("input"),
            cache_read_tokens: count("cacheRead"),
            cache_write_tokens: count("cacheWrite"),
            output_tokens: count("output"),
            unclassified_tokens: 0,
            reasoning_tokens: None,
        };
        usage.unclassified_tokens = count("totalTokens").saturating_sub(usage.total()?);
        if usage.total()? == 0 {
            continue;
        }
        let identity = text(&row, "id")
            .map(|id| format!("entry:{id}"))
            .unwrap_or_else(|| format!("v1-line:{ordinal}"));
        if !seen.insert(identity.clone()) {
            continue;
        }
        records.push(
            CanonicalRecord {
                record_id: stable_record_id(TOOL, key.as_bytes(), identity.as_bytes())?,
                owner_device_id: device.to_owned(),
                tool_id: TOOL.to_owned(),
                scope: UsageScope::DeviceLocal,
                precision: Precision::Exact,
                account_id: None,
                model_id: text(event, "model")
                    .filter(|s| s.len() <= 256 && !s.chars().any(char::is_control))
                    .map(str::to_owned),
                project_id: project_id.map(str::to_owned),
                session_id: Some(key.to_owned()),
                event_at_ms: Some(event_at.unwrap_or(created)),
                usage,
                estimated_cost_microusd: None,
                price_version: None,
                is_deleted: false,
            }
            .normalized()?,
        );
    }
    Ok((name.or(first_prompt), records))
}

enum Scan {
    Cached,
    Changed(Box<ChangedScan>),
}

struct ChangedScan {
    checkpoint: SourceCheckpoint,
    previous: Option<SourceCheckpoint>,
    project: Option<(String, String)>,
    title: Option<String>,
    records: Vec<CanonicalRecord>,
}

fn scan(path: &Path, store: &LocalStore, device: &str) -> Result<Scan> {
    let file = File::open(path)?;
    let before = file.metadata()?;
    let stamp = fingerprint(&before)?;
    let mut reader = BufReader::new(file);
    let header = next_row(&mut reader)?.ok_or("empty Pi log")?;
    if text(&header, "type") != Some("session")
        || header["version"]
            .as_u64()
            .is_some_and(|v| !(1..=3).contains(&v))
    {
        return Err("unsupported Pi session format".into());
    }
    let id = text(&header, "id")
        .filter(|id| id.len() <= 200 && !id.chars().any(char::is_control))
        .ok_or("invalid Pi session ID")?;
    let key = session_key(TOOL, id.as_bytes())?;
    if store.is_session_suppressed(TOOL, &key)? {
        return Ok(Scan::Cached);
    }
    let previous = store.checkpoint(TOOL, &key)?;
    if previous.as_ref().is_some_and(|old| {
        old.parser_version == VERSION
            && old.source_fingerprint == stamp
            && old.last_status == "complete"
    }) {
        return Ok(Scan::Cached);
    }
    let project = project(&header);
    let (title, records) = parse_entries(
        &mut reader,
        &header,
        &key,
        device,
        project.as_ref().map(|(id, _)| id.as_str()),
    )?;
    if fingerprint(&reader.get_ref().metadata()?)? != stamp
        || fingerprint(&fs::metadata(path)?)? != stamp
    {
        return Err("Pi log changed during scan".into());
    }
    let now = Utc::now().timestamp_millis().max(
        previous
            .as_ref()
            .map_or(0, |old| old.last_scan_ms.saturating_add(1)),
    );
    Ok(Scan::Changed(Box::new(ChangedScan {
        checkpoint: SourceCheckpoint {
            tool_id: TOOL.to_owned(),
            source_key: key,
            source_fingerprint: stamp,
            read_offset: before.len(),
            parser_version: VERSION,
            last_scan_ms: now,
            last_complete_scan_ms: Some(now),
            last_status: "complete".into(),
        },
        previous,
        project,
        title,
        records,
    })))
}

pub(super) fn collect(root: &Path, store: &mut LocalStore, device: &str) -> Result<Report> {
    let mut report = Report::default();
    if !root.is_dir() {
        return Ok(report);
    }
    let mut dirs = vec![root.to_path_buf()];
    let mut visited = 0;
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            report.skipped += 1;
            continue;
        };
        for entry in entries {
            visited += 1;
            if visited > 100_000 {
                return Err("Pi session directory exceeds scan limit".into());
            }
            let Ok(entry) = entry else {
                report.skipped += 1;
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                report.skipped += 1;
                continue;
            };
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if entry.metadata()?.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            if kind.is_dir() {
                dirs.push(entry.path());
            } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "jsonl") {
                report.files += 1;
                match scan(&entry.path(), store, device) {
                    Ok(Scan::Cached) => report.cached += 1,
                    Ok(Scan::Changed(changed)) => {
                        let ChangedScan {
                            checkpoint,
                            previous,
                            project,
                            title,
                            records,
                        } = *changed;
                        let now = checkpoint.last_scan_ms;
                        if let Some((id, name)) = project {
                            store.upsert_project_name(&id, &name, now)?;
                        }
                        if let Some(name) = title {
                            store.upsert_session_name(&checkpoint.source_key, TOOL, &name, now)?;
                        }
                        let result = store.reconcile_complete_source(
                            previous.as_ref(),
                            &checkpoint,
                            records,
                            now,
                        )?;
                        report.inserted += result.applied.inserted;
                        report.revised += result.applied.revised;
                    }
                    Err(error) if error.is::<tokenscope_local_store::StoreError>() => {
                        return Err(error)
                    }
                    Err(_) => report.skipped += 1,
                }
            }
        }
    }
    Ok(report)
}
