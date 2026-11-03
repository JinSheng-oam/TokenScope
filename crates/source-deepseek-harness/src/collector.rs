/// Scans one configured `DSH_HOME`. Missing or unreadable individual sessions
/// retain their last complete projection; only a stable full inventory may
/// tombstone sessions removed by the source application.
pub fn collect_deepseek_harness(
    home: &Path,
    store: &mut LocalStore,
    device_id: &str,
) -> Result<DeepSeekHarnessReport> {
    let inventory = discover_sessions(home)?;
    let mut report = DeepSeekHarnessReport {
        source_files: inventory.paths.len(),
        skipped: inventory.skipped,
        ..DeepSeekHarnessReport::default()
    };
    if !inventory.available {
        return Ok(report);
    }

    let mut present_sources = HashSet::new();
    let mut inventory_complete = inventory.complete;
    for candidate in inventory.paths {
        let before = match fs::symlink_metadata(&candidate.path) {
            Ok(metadata)
                if metadata.file_type().is_file() && !metadata.file_type().is_symlink() =>
            {
                metadata
            }
            Ok(_) => {
                report.skipped += 1;
                inventory_complete = false;
                continue;
            }
            Err(_) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
        };
        let fingerprint = match fingerprint(&candidate.path, &before, candidate.compressed) {
            Ok(fingerprint) => fingerprint,
            Err(_) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
        };
        let header = match read_session_header(&candidate.path, candidate.compressed) {
            Ok(header) => header,
            Err(DeepSeekHarnessError::Pending) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
            Err(_) => {
                report.skipped += 1;
                inventory_complete = false;
                continue;
            }
        };
        let source_key = session_key(TOOL_ID, header.id.as_bytes())?;
        present_sources.insert(source_key.clone());
        if store.is_session_suppressed(TOOL_ID, &source_key)? {
            continue;
        }

        let project_id = if let Some(cwd) = header.cwd.as_deref() {
            if let Some((project_id, name)) = project_metadata(cwd) {
                store.upsert_project_name(&project_id, &name, now_ms()?)?;
                Some(project_id)
            } else {
                None
            }
        } else {
            None
        };
        let previous = store.checkpoint(TOOL_ID, &source_key)?;
        if previous.as_ref().is_some_and(|checkpoint| {
            checkpoint.parser_version == PARSER_VERSION
                && checkpoint.source_fingerprint == fingerprint
                && checkpoint.last_status == "complete"
        }) {
            if current_metadata(&candidate.path, &before, &fingerprint, candidate.compressed)? {
                report.sessions += 1;
                report.unchanged += 1;
                continue;
            }
            report.pending += 1;
            inventory_complete = false;
            continue;
        }

        let parsed = match parse_session(
            &candidate.path,
            candidate.compressed,
            &header.id,
            &source_key,
            device_id,
            project_id.as_deref(),
        ) {
            Ok(parsed) => parsed,
            Err(DeepSeekHarnessError::Pending) => {
                report.pending += 1;
                inventory_complete = false;
                continue;
            }
            Err(_) => {
                report.skipped += 1;
                inventory_complete = false;
                continue;
            }
        };
        if !current_metadata(&candidate.path, &before, &fingerprint, candidate.compressed)? {
            report.pending += 1;
            inventory_complete = false;
            continue;
        }

        if let Some(title) = parsed.title.as_deref() {
            store.upsert_session_name(&source_key, TOOL_ID, title, now_ms()?)?;
        }
        let observed_at_ms = now_ms()?;
        let last_scan_ms = previous.as_ref().map_or(observed_at_ms, |checkpoint| {
            observed_at_ms.max(checkpoint.last_scan_ms.saturating_add(1))
        });
        let checkpoint = SourceCheckpoint {
            tool_id: TOOL_ID.to_owned(),
            source_key: source_key.clone(),
            source_fingerprint: fingerprint,
            read_offset: before.len(),
            parser_version: PARSER_VERSION,
            last_scan_ms,
            last_complete_scan_ms: Some(last_scan_ms),
            last_status: "complete".into(),
        };
        let reconciled = store.reconcile_complete_source(
            previous.as_ref(),
            &checkpoint,
            parsed.records,
            last_scan_ms,
        )?;
        report.sessions += 1;
        report.model_calls = report.model_calls.saturating_add(parsed.model_calls);
        report.inserted += reconciled.applied.inserted;
        report.revised += reconciled.applied.revised;
    }

    if inventory_complete && report.pending == 0 && report.skipped == 0 {
        report.deleted = store.reconcile_missing_sources(TOOL_ID, &present_sources, now_ms()?)?;
    }
    Ok(report)
}

fn read_session_header(path: &Path, compressed: bool) -> Result<SessionHeader> {
    let mut reader = BufReader::new(open_log_reader(path, compressed)?);
    let (line, complete) =
        bounded_line(&mut reader)?.ok_or(DeepSeekHarnessError::Invalid("empty session log"))?;
    if !complete {
        return Err(DeepSeekHarnessError::Pending);
    }
    parse_header(&json_line(line)?)
}
use crate::{
    inventory::{
        bounded_line, current_metadata, discover_sessions, fingerprint, json_line, open_log_reader,
        parse_header, project_metadata,
    },
    model::SessionHeader,
    now_ms,
    parse::parse_session,
    DeepSeekHarnessError, DeepSeekHarnessReport, Result, PARSER_VERSION, TOOL_ID,
};
use std::{collections::HashSet, fs, io::BufReader, path::Path};
use tokenscope_core_domain::session_key;
use tokenscope_local_store::{LocalStore, SourceCheckpoint};
