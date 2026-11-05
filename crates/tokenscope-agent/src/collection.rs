use std::path::Path;
use tokenscope_local_store::LocalStore;
use tokenscope_source_antigravity::collect_antigravity_tolerant;
use tokenscope_source_claude::collect_claude;
use tokenscope_source_codex::collect_codex_precise_tolerant;
use tokenscope_source_deepseek_harness::collect_deepseek_harness;

pub(super) struct CollectionOptions<'a> {
    pub(super) codex: bool,
    pub(super) claude: bool,
    pub(super) antigravity: bool,
    pub(super) deepseek_harness: bool,
    pub(super) codex_root: &'a Path,
    pub(super) claude_root: &'a Path,
    pub(super) antigravity_root: &'a Path,
    pub(super) deepseek_harness_root: &'a Path,
    pub(super) allow_antigravity_estimates: bool,
    pub(super) device_id: &'a str,
}

pub(super) fn collect_once(
    store: &mut LocalStore,
    options: &CollectionOptions<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut failures = Vec::new();
    let mut succeeded = 0usize;
    if options.codex {
        match collect_codex_precise_tolerant(options.codex_root, store, options.device_id) {
            Ok(report) => {
                succeeded += 1;
                println!(
            "codex scanned={} inserted={} revised={} unchanged={} parsed_lines={} partial={} skipped={} missing_source_tombstones={}",
            report.files,
            report.inserted,
            report.revised,
            report.unchanged,
            report.lines,
            report.pending,
            report.unsupported,
            report.missing_source_tombstones
        );
            }
            Err(error) => {
                eprintln!("TokenScope: codex scan skipped: {error}");
                failures.push(format!("codex: {error}"));
            }
        }
    }
    if options.claude {
        match collect_claude(options.claude_root, store, options.device_id) {
            Ok(report) => {
                succeeded += 1;
                println!(
            "claude scanned={} cached={} pending={} inserted={} revised={} deleted={} lines={}",
            report.files,
            report.unchanged_files,
            report.pending_files,
            report.inserted,
            report.revised,
            report.deleted,
            report.parsed_lines
        );
            }
            Err(error) => {
                eprintln!("TokenScope: claude scan skipped: {error}");
                failures.push(format!("claude: {error}"));
            }
        }
    }
    if options.antigravity {
        // An unreadable Antigravity corpus must not abort Codex/Claude collection.
        match collect_antigravity_tolerant(
            options.antigravity_root,
            store,
            options.device_id,
            options.allow_antigravity_estimates,
        ) {
            Ok(r) => {
                succeeded += 1;
                println!("antigravity files={} conversations={} model_turns={} unknown_model_turns={} ambiguous={} measured_records={} estimated_records={} inserted={} revised={} tombstoned={} unchanged={} skipped={} skipped_rows={} precision={}",
                r.source_files, r.conversations, r.model_turns, r.unknown_model_turns,
                r.ambiguous_conversations, r.measured_records, r.estimated_records, r.inserted, r.revised,
                r.tombstoned, r.unchanged, r.skipped_sources, r.skipped_rows,
                if options.allow_antigravity_estimates { "ESTIMATED" } else { "ACTIVITY_ONLY" });
            }
            Err(err) => {
                eprintln!("TokenScope: antigravity scan skipped: {err}");
                failures.push(format!("antigravity: {err}"));
            }
        }
    }
    if options.deepseek_harness {
        match collect_deepseek_harness(options.deepseek_harness_root, store, options.device_id) {
            Ok(report) => {
                succeeded += 1;
                println!(
                "deepseek-harness files={} sessions={} model_calls={} inserted={} revised={} unchanged={} pending={} skipped={} deleted={}",
                report.source_files,
                report.sessions,
                report.model_calls,
                report.inserted,
                report.revised,
                report.unchanged,
                report.pending,
                report.skipped,
                report.deleted,
            );
            }
            Err(error) => {
                eprintln!("TokenScope: deepseek-harness scan skipped: {error}");
                failures.push(format!("deepseek-harness: {error}"));
            }
        }
    }
    if succeeded == 0 && !failures.is_empty() {
        return Err(failures.join("; ").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokenscope_local_store::LocalIdentity;

    #[test]
    fn broken_claude_log_does_not_block_later_harness_session() {
        let root =
            std::env::temp_dir().join(format!("tokenscope-isolation-{}", uuid::Uuid::new_v4()));
        let claude = root.join("claude");
        let harness = root.join("harness");
        std::fs::create_dir_all(claude.join("projects/demo")).unwrap();
        std::fs::write(claude.join("projects/demo/broken.jsonl"), "{broken}\n").unwrap();
        let session = harness.join("sessions/demo/session");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(
            session.join("session.jsonl"),
            "{\"type\":\"session\",\"id\":\"healthy-session\",\"version\":1}\n",
        )
        .unwrap();
        let mut store = LocalStore::open_in_memory().unwrap();
        store
            .init_identity(&LocalIdentity {
                device_id: "test".into(),
                display_name: "Test".into(),
                time_zone: "UTC".into(),
            })
            .unwrap();
        let result = collect_once(
            &mut store,
            &CollectionOptions {
                codex: false,
                claude: true,
                antigravity: false,
                deepseek_harness: true,
                codex_root: &root,
                claude_root: &claude,
                antigravity_root: &root,
                deepseek_harness_root: &harness,
                allow_antigravity_estimates: false,
                device_id: "test",
            },
        );
        let checkpoints = store.source_checkpoints().unwrap();
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(&root).unwrap();
        result.unwrap();
        assert!(checkpoints
            .iter()
            .any(|cp| cp.tool_id == "deepseek-harness" && cp.last_status == "complete"));
        assert!(checkpoints.iter().all(|cp| cp.tool_id != "claude"));
    }
}
