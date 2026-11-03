use std::collections::HashSet;
use std::path::Path;
use tokenscope_local_store::LocalStore;

use super::metadata::{discover_claude_files, file_identity, now_ms, project_name_from_transcript};
use super::model::{ClaudeError, ClaudeReport, Result};
use super::scanner::scan_claude_file;

/// One complete file -> one atomic store transaction. Failures do NOT advance
/// this source's checkpoint. Missing source files are not treated as deletions.
pub fn collect_claude(
    config_dir: &Path,
    store: &mut LocalStore,
    device_id: &str,
) -> Result<ClaudeReport> {
    let mut report = ClaudeReport::default();
    let mut sources = HashSet::new();
    let mut present_sources = HashSet::new();
    let mut named_projects = HashSet::new();
    for path in discover_claude_files(config_dir)? {
        let (key, _, project_id) = file_identity(&path, config_dir)?;
        present_sources.insert(key.clone());
        if !sources.insert(key.clone()) {
            return Err(ClaudeError::Invalid("duplicate transcript identity"));
        }
        if !named_projects.contains(&project_id) {
            if let Some(name) = project_name_from_transcript(&path) {
                store.upsert_project_name(&project_id, &name, now_ms()?)?;
                named_projects.insert(project_id);
            }
        }
        let previous = store.checkpoint("claude", &key)?;
        let scan =
            match scan_claude_file(&path, config_dir, device_id, previous.as_ref(), now_ms()?) {
                Ok(Some(s)) => s,
                Ok(None) => {
                    report.unchanged_files += 1;
                    continue;
                }
                Err(ClaudeError::Pending) => {
                    report.pending_files += 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
        // Full file was validated, including stable mtime and a complete final
        // JSONL line. Reconcile inside ONE SQLite transaction, paging prior
        // records and generating tombstones only after incoming rows are seen.
        if let Some(name) = scan.title.as_deref() {
            store.upsert_session_name(
                &scan.next.source_key,
                "claude",
                name,
                scan.next.last_scan_ms,
            )?;
        }
        let result = store.reconcile_complete_source(
            previous.as_ref(),
            &scan.next,
            scan.records.into_iter(),
            scan.next.last_scan_ms,
        )?;
        report.deleted += result.tombstoned;
        report.files += 1;
        report.parsed_lines += scan.parsed_lines;
        report.inserted += result.applied.inserted;
        report.revised += result.applied.revised;
        report.unchanged_records += result.applied.unchanged;
    }
    if config_dir.join("projects").is_dir() && report.pending_files == 0 {
        report.deleted += store.reconcile_missing_sources("claude", &present_sources, now_ms()?)?;
    }
    Ok(report)
}
