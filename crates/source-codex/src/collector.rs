use std::fs;
use std::path::{Path, PathBuf};
use tokenscope_core_domain::session_key;
use tokenscope_local_store::{ApplyReport, LocalStore};

use super::model::{CodexError, CollectReport, Result};
use super::rollout::{now_ms, read_session_header};
use super::scanner::scan_file;
use super::MAX_FILES;

pub fn discover_codex_files(home: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    for dir in [home.join("sessions"), home.join("archived_sessions")] {
        if !dir.exists() {
            continue;
        }
        let mut stack = vec![(dir, 0usize)];
        while let Some((root, depth)) = stack.pop() {
            if depth > 8 {
                return Err(CodexError::Limit("Codex directory depth"));
            }
            let mut entries = fs::read_dir(root)?.collect::<std::result::Result<Vec<_>, _>>()?;
            entries.sort_by_key(|e| e.file_name());
            for ent in entries {
                let file_type = ent.file_type()?;
                if file_type.is_symlink() {
                    continue;
                } // Never follow outside the Codex root.
                if file_type.is_dir() {
                    stack.push((ent.path(), depth + 1));
                    continue;
                }
                if !file_type.is_file() {
                    continue;
                }
                let name = ent.file_name();
                let name = name.to_string_lossy();
                if !(name.ends_with(".jsonl") || name.ends_with(".jsonl.zst")) {
                    continue;
                }
                result.push(ent.path());
                if result.len() > MAX_FILES {
                    return Err(CodexError::Limit("Codex file count"));
                }
            }
        }
    }
    result.sort();
    Ok(result)
}

pub fn collect_codex(
    home: &Path,
    store: &mut LocalStore,
    device_id: &str,
) -> Result<CollectReport> {
    let mut report = CollectReport::default();
    let mut session_sources = std::collections::HashSet::new();
    for file in discover_codex_files(home)? {
        let session_id = match read_session_header(&file) {
            Ok(id) => id,
            Err(CodexError::PendingHeader) => {
                report.pending_lines += 1;
                continue;
            }
            Err(e) => return Err(e),
        };
        let key = session_key("codex", session_id.as_bytes())?;
        if !session_sources.insert(key.clone()) {
            return Err(CodexError::Invalid(
                "duplicate Codex session; reconcile sources before collecting",
            ));
        }
        let prior = store.checkpoint("codex", &key)?;
        let scan = scan_file(&file, device_id, prior.as_ref(), now_ms()?)?;
        let records = scan.record.iter().cloned().collect::<Vec<_>>();
        let ApplyReport {
            inserted,
            revised,
            unchanged,
        } = store.apply_scan(
            prior.as_ref(),
            &scan.next,
            &records,
            scan.next.last_scan_ms,
            false,
        )?;
        report.scanned_files += 1;
        report.inserted += inserted;
        report.revised += revised;
        report.unchanged += unchanged;
        report.parsed_lines += scan.parsed_lines;
        if scan.pending_trailing_line {
            report.pending_lines += 1
        }
    }
    Ok(report)
}
