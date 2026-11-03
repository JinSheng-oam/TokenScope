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
    if options.codex {
        let report = collect_codex_precise_tolerant(options.codex_root, store, options.device_id)?;
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
    if options.claude {
        let report = collect_claude(options.claude_root, store, options.device_id)?;
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
    if options.antigravity {
        // An unreadable Antigravity corpus must not abort Codex/Claude collection.
        match collect_antigravity_tolerant(
            options.antigravity_root,
            store,
            options.device_id,
            options.allow_antigravity_estimates,
        ) {
            Ok(r) => println!("antigravity files={} conversations={} model_turns={} unknown_model_turns={} ambiguous={} measured_records={} estimated_records={} inserted={} revised={} tombstoned={} unchanged={} skipped={} skipped_rows={} precision={}",
                r.source_files, r.conversations, r.model_turns, r.unknown_model_turns,
                r.ambiguous_conversations, r.measured_records, r.estimated_records, r.inserted, r.revised,
                r.tombstoned, r.unchanged, r.skipped_sources, r.skipped_rows,
                if options.allow_antigravity_estimates { "ESTIMATED" } else { "ACTIVITY_ONLY" }),
            Err(err) => eprintln!("TokenScope: antigravity scan skipped: {err}"),
        }
    }
    if options.deepseek_harness {
        match collect_deepseek_harness(
            options.deepseek_harness_root,
            store,
            options.device_id,
        ) {
            Ok(report) => println!(
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
            ),
            Err(error) => eprintln!("TokenScope: deepseek-harness scan skipped: {error}"),
        }
    }
    Ok(())
}
