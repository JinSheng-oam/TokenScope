//! M0-12: single-instance daemon, local Antigravity quota refresh and cooperative desktop shutdown.
//! --daemon loops scans; default remains a one-shot invocation for automation.
mod antigravity_rpc;
mod codex_rpc;
mod dashboard;
mod lock;
mod quota;
mod session_control;
mod shutdown;
use lock::AgentLock;
use std::{
    env,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tokenscope_local_ipc::{DashboardResponse, SessionAnalysisResponse};
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_antigravity::collect_antigravity_tolerant;
use tokenscope_source_claude::collect_claude;
use tokenscope_source_codex::collect_codex_precise_tolerant;
use tokenscope_sync_client::{
    worker::{pull_usage_changes_once, sync_outbox_once},
    CredentialKey, HubClient, HubUrl, KeyringCredentialStore, SecretStore,
};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|parts| parts[0] == name)
        .map(|parts| parts[1].clone())
}
fn config_home(env_name: &str, child: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(dir) = env::var_os(env_name) {
        return Ok(PathBuf::from(dir));
    }
    let base = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .ok_or("missing HOME/USERPROFILE")?;
    Ok(PathBuf::from(base).join(child))
}
fn usage() {
    println!(
        r#"Usage: tokenscope-agent [--scan-codex | --scan-claude | --scan-antigravity | --scan-all]
    --db PATH [--device-id ID] [--codex-home PATH] [--claude-home PATH]
    [--antigravity-home PATH] [--antigravity-estimates]
    [--import-antigravity-quota-json PATH] [--refresh-antigravity-quota] [--refresh-codex-quota] [--show-antigravity]
    [--display-name NAME] [--time-zone ZONE] [--daemon --interval-seconds 120]
    --dashboard-json [--range today|7d|30d|all]
    --session-analysis-json [--range today|7d|30d|all] [--session-search TEXT]
    --shutdown-file PATH (daemon only; desktop lifecycle signal)
    --sync --hub-url HTTPS_OR_LOOPBACK | --sync-config PATH (daemon mode enables periodic sync)
    --sync-config PATH (plain-text non-secret Hub origin; read dynamically by daemon)
    --sync-once --hub-url HTTPS_OR_LOOPBACK (one-shot push + pull)
    --sync-status (show local Outbox retry / conflict state)"#
    );
}

struct CollectionOptions<'a> {
    codex: bool,
    claude: bool,
    antigravity: bool,
    codex_root: &'a Path,
    claude_root: &'a Path,
    antigravity_root: &'a Path,
    allow_antigravity_estimates: bool,
    device_id: &'a str,
}

fn collect_once(
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
    Ok(())
}

fn resolve_sync_hub_url(
    cli_url: Option<&str>,
    config_path: Option<&Path>,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    if let Some(url) = cli_url {
        return Ok(Some(url.to_string()));
    }
    let Some(path) = config_path else {
        return Ok(None);
    };
    match std::fs::read_to_string(path) {
        Ok(value) => {
            let value = value.trim();
            if value.is_empty() {
                return Ok(None);
            }
            let parsed = HubUrl::parse(value)?;
            Ok(Some(parsed.origin().to_string()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Box::new(error)),
    }
}

fn sync_device_once(
    store: &mut LocalStore,
    raw_url: &str,
    device: &str,
) -> Result<tokenscope_sync_client::worker::SyncReport, Box<dyn std::error::Error>> {
    let hub_url = HubUrl::parse(raw_url)?;
    let key = CredentialKey::for_device(&hub_url, device)?;
    let credentials = KeyringCredentialStore;
    let token = credentials
        .load(&key)?
        .ok_or("no credential for this Hub/device; pair this device first")?;
    let client = HubClient::new(hub_url)?;
    let report =
        sync_outbox_once(&client, store, &token).map_err(|e| format!("sync failed: {e:?}"))?;
    Ok(report)
}

fn pull_device_once(
    store: &mut LocalStore,
    raw_url: &str,
    device: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let hub_url = HubUrl::parse(raw_url)?;
    let key = CredentialKey::for_device(&hub_url, device)?;
    let credentials = KeyringCredentialStore;
    let token = credentials
        .load(&key)?
        .ok_or("no credential for this Hub/device; pair this device first")?;
    let client = HubClient::new(hub_url)?;
    let applied = pull_usage_changes_once(&client, &mut *store, &token, 200)
        .map_err(|e| format!("pull failed: {e:?}"))?;
    Ok(applied)
}

fn sleep_interruptible(
    stop: &AtomicBool,
    seconds: u64,
    shutdown_file: Option<&Path>,
    store: &mut LocalStore,
    db_path: &Path,
) -> bool {
    // Poll a file-based shutdown signal so Tauri can stop the daemon without
    // platform-specific signals. Collection is never interrupted mid-transaction.
    for _ in 0..seconds.saturating_mul(4) {
        if stop.load(Ordering::Relaxed) || shutdown::consume(shutdown_file) {
            return true;
        }
        if let Err(error) = session_control::process_pending_requests(store, db_path) {
            eprintln!("TokenScope session control warning: {error}");
        }
        thread::sleep(Duration::from_millis(250));
    }
    stop.load(Ordering::Relaxed) || shutdown::consume(shutdown_file)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|s| s == "--version" || s == "-V") {
        println!("tokenscope-agent {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let codex = args
        .iter()
        .any(|s| s == "--scan-codex" || s == "--scan-all");
    let claude = args
        .iter()
        .any(|s| s == "--scan-claude" || s == "--scan-all");
    let antigravity = args
        .iter()
        .any(|s| s == "--scan-antigravity" || s == "--scan-all");
    let ag_estimates = args.iter().any(|s| s == "--antigravity-estimates");
    let quota_path = arg(&args, "--import-antigravity-quota-json");
    let show_antigravity = args.iter().any(|s| s == "--show-antigravity");
    let refresh_antigravity_quota = args.iter().any(|s| s == "--refresh-antigravity-quota");
    let refresh_codex_quota = args.iter().any(|s| s == "--refresh-codex-quota");
    let dashboard_json = args.iter().any(|s| s == "--dashboard-json");
    let session_analysis_json = args.iter().any(|s| s == "--session-analysis-json");
    let sync_once = args.iter().any(|s| s == "--sync-once");
    let sync_enabled = args.iter().any(|s| s == "--sync");
    let sync_status = args.iter().any(|s| s == "--sync-status");
    let sync_hub_url = arg(&args, "--hub-url");
    let sync_config_path = arg(&args, "--sync-config").map(PathBuf::from);
    let shutdown_file = arg(&args, "--shutdown-file").map(PathBuf::from);
    let dashboard_range = arg(&args, "--range").unwrap_or_else(|| "7d".into());
    let session_offset: u64 = arg(&args, "--session-offset")
        .unwrap_or_else(|| "0".into())
        .parse()?;
    if !codex
        && !claude
        && !antigravity
        && quota_path.is_none()
        && !show_antigravity
        && !refresh_antigravity_quota
        && !refresh_codex_quota
        && !dashboard_json
        && !session_analysis_json
        && !sync_once
        && !sync_enabled
        && !sync_status
    {
        usage();
        return Ok(());
    }
    let daemon = args.iter().any(|s| s == "--daemon");
    if (sync_once || sync_enabled) && sync_hub_url.is_none() && sync_config_path.is_none() {
        return Err("--sync/--sync-once requires --hub-url or --sync-config".into());
    }
    if quota_path.is_some() && refresh_antigravity_quota {
        return Err("manual quota import and local RPC refresh cannot be combined".into());
    }
    if daemon
        && (quota_path.is_some()
            || show_antigravity
            || dashboard_json
            || session_analysis_json
            || sync_once
            || sync_status)
    {
        return Err(
            "quota import, dashboard query and status display are one-shot commands".into(),
        );
    }
    if shutdown_file.is_some() && !daemon {
        return Err("--shutdown-file is only valid with --daemon".into());
    }
    if dashboard_json {
        if codex
            || claude
            || antigravity
            || quota_path.is_some()
            || show_antigravity
            || refresh_antigravity_quota
            || refresh_codex_quota
            || session_analysis_json
            || sync_once
            || sync_enabled
            || sync_status
        {
            return Err(
                "dashboard query cannot be combined with collection or quota commands".into(),
            );
        }
        let db = PathBuf::from(arg(&args, "--db").ok_or("missing --db")?);
        let start_date = arg(&args, "--start-date");
        let end_date = arg(&args, "--end-date");
        let snapshot = dashboard::read_snapshot(
            &db,
            &dashboard_range,
            session_offset,
            start_date.as_deref(),
            end_date.as_deref(),
        )?;
        let response = DashboardResponse::new(snapshot);
        println!("{}", serde_json::to_string(&response)?);
        return Ok(());
    }
    if session_analysis_json {
        if codex
            || claude
            || antigravity
            || quota_path.is_some()
            || show_antigravity
            || refresh_antigravity_quota
            || refresh_codex_quota
            || dashboard_json
            || sync_once
            || sync_enabled
            || sync_status
        {
            return Err("session analysis query cannot be combined with collection or quota commands".into());
        }
        let db = PathBuf::from(arg(&args, "--db").ok_or("missing --db")?);
        let start_date = arg(&args, "--start-date");
        let end_date = arg(&args, "--end-date");
        let analysis = dashboard::read_session_analysis(
            &db,
            &dashboard_range,
            session_offset,
            arg(&args, "--session-search").as_deref(),
            arg(&args, "--session-tool").as_deref(),
            arg(&args, "--session-model").as_deref(),
            arg(&args, "--session-project").as_deref(),
            start_date.as_deref(),
            end_date.as_deref(),
        )?;
        println!("{}", serde_json::to_string(&SessionAnalysisResponse::new(analysis))?);
        return Ok(());
    }
    let interval: u64 = arg(&args, "--interval-seconds")
        .unwrap_or_else(|| "30".into())
        .parse()?;
    if !(5..=3600).contains(&interval) {
        return Err("interval must be 5..3600 seconds".into());
    }
    let db = PathBuf::from(arg(&args, "--db").ok_or("missing --db")?);
    // Lock before opening SQLite or touching local identity. OS releases this
    // lock if the process crashes or is killed.
    let _exclusive = AgentLock::acquire(&db)?;
    // Short-lived read commands must not open the write connection: doing so
    // checkpoints the WAL on every dashboard refresh and dominates latency.
    let mut store = if dashboard_json || show_antigravity {
        LocalStore::open_readonly(&db)?
    } else {
        LocalStore::open(&db)?
    };
    let prior = store.identity()?;
    // The stable device ID is generated once and persisted in SQLite on first
    // run. Existing databases always reuse their immutable stored identity.
    let device = arg(&args, "--device-id")
        .or_else(|| prior.as_ref().map(|id| id.device_id.clone()))
        .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
    let display = arg(&args, "--display-name")
        .or_else(|| prior.as_ref().map(|id| id.display_name.clone()))
        .unwrap_or_else(|| "This device".into());
    let zone = arg(&args, "--time-zone")
        .or_else(|| prior.as_ref().map(|id| id.time_zone.clone()))
        .unwrap_or_else(|| "UTC".into());
    store.init_identity(&LocalIdentity {
        device_id: device.clone(),
        display_name: display,
        time_zone: zone,
    })?;
    let codex_root = match arg(&args, "--codex-home") {
        Some(p) => PathBuf::from(p),
        None => config_home("CODEX_HOME", ".codex")?,
    };
    let claude_root = match arg(&args, "--claude-home") {
        Some(p) => PathBuf::from(p),
        None => config_home("CLAUDE_CONFIG_DIR", ".claude")?,
    };
    let ag_root = match arg(&args, "--antigravity-home") {
        Some(p) => PathBuf::from(p),
        None => config_home("TOKENSCOPE_GEMINI_HOME", ".gemini")?,
    };
    if let Some(path) = quota_path {
        let imported = quota::import_file(&mut store, &PathBuf::from(path))?;
        println!("antigravity quota snapshots updated={imported} (manual import, NOT OAuth/RPC)");
    }
    if sync_status {
        let now = chrono::Utc::now().timestamp_millis();
        let status = store.sync_outbox_status(now)?;
        println!("{}", serde_json::to_string(&status)?);
        return Ok(());
    }
    if !daemon {
        if codex || claude || antigravity {
            collect_once(
                &mut store,
                &CollectionOptions {
                    codex,
                    claude,
                    antigravity,
                    codex_root: &codex_root,
                    claude_root: &claude_root,
                    antigravity_root: &ag_root,
                    allow_antigravity_estimates: ag_estimates,
                    device_id: &device,
                },
            )?;
        }
        if refresh_antigravity_quota {
            let updated = antigravity_rpc::refresh_and_store(&mut store)?;
            println!("antigravity local RPC quota snapshots updated={updated}");
        }
        if refresh_codex_quota {
            let updated = codex_rpc::refresh_and_store(&mut store)?;
            println!("codex local app-server quota snapshots updated={updated}");
        }
        if show_antigravity {
            quota::print_status(&store)?;
        }
        if sync_once || sync_enabled {
            let raw_url =
                resolve_sync_hub_url(sync_hub_url.as_deref(), sync_config_path.as_deref())?
                    .ok_or("no Hub URL configured; pair this device or provide --hub-url")?;
            let report = sync_device_once(&mut store, &raw_url, &device)?;
            let pulled = pull_device_once(&mut store, &raw_url, &device)?;
            println!("sync uploaded={} unchanged={} superseded={} acknowledged={} blocked={} remaining_due={} remote_changes_applied={}",
                report.uploaded, report.unchanged, report.superseded, report.acknowledged, report.blocked, report.remaining_due, pulled);
        }
        return Ok(());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stop_for_signal = stop.clone();
    ctrlc::set_handler(move || stop_for_signal.store(true, Ordering::SeqCst))?;
    let mut consecutive_failures = 0_u32;
    while !stop.load(Ordering::SeqCst) {
        if shutdown::consume(shutdown_file.as_deref()) {
            break;
        }
        if let Err(error) = session_control::process_pending_requests(&mut store, &db) {
            eprintln!("TokenScope session control warning: {error}");
        }
        match collect_once(
            &mut store,
            &CollectionOptions {
                codex,
                claude,
                antigravity,
                codex_root: &codex_root,
                claude_root: &claude_root,
                antigravity_root: &ag_root,
                allow_antigravity_estimates: ag_estimates,
                device_id: &device,
            },
        ) {
            Ok(()) => {
                if refresh_antigravity_quota {
                    match antigravity_rpc::refresh_and_store(&mut store) {
                        Ok(updated) => {
                            consecutive_failures = 0;
                            println!("antigravity local RPC quota snapshots updated={updated}");
                        }
                        Err(err) => {
                            consecutive_failures = consecutive_failures.saturating_add(1);
                            eprintln!("Antigravity quota refresh failed: {err}");
                        }
                    }
                } else {
                    consecutive_failures = 0;
                }
                if refresh_codex_quota {
                    if let Err(err) = codex_rpc::refresh_and_store(&mut store) {
                        eprintln!("Codex quota refresh failed: {err}");
                    }
                }
            }
            Err(err) => {
                consecutive_failures = consecutive_failures.saturating_add(1);
                eprintln!("TokenScope scan error (retry #{consecutive_failures}): {err}");
            }
        }
        if sync_enabled {
            match resolve_sync_hub_url(sync_hub_url.as_deref(), sync_config_path.as_deref()) {
                Ok(Some(raw_url)) => match sync_device_once(&mut store, &raw_url, &device) {
                    Ok(report) => match pull_device_once(&mut store, &raw_url, &device) {
                        Ok(pulled) => {
                            if report.acknowledged > 0
                                || report.uploaded > 0
                                || report.unchanged > 0
                                || pulled > 0
                            {
                                println!("sync uploaded={} unchanged={} acknowledged={} blocked={} remaining_due={} remote_changes_applied={}",
                                        report.uploaded, report.unchanged, report.acknowledged, report.blocked, report.remaining_due, pulled);
                            }
                        }
                        Err(err) => eprintln!("TokenScope pull deferred: {err}"),
                    },
                    Err(err) => eprintln!("TokenScope sync deferred: {err}"),
                },
                Ok(None) => {} // No Hub has been paired on this device yet.
                Err(err) => eprintln!("TokenScope sync configuration invalid: {err}"),
            }
        }
        let backoff = if consecutive_failures == 0 {
            interval
        } else {
            interval
                .saturating_mul(1_u64 << consecutive_failures.min(4))
                .min(300)
        };
        if sleep_interruptible(&stop, backoff, shutdown_file.as_deref(), &mut store, &db) {
            break;
        }
    }
    println!("TokenScope stopped cleanly");
    Ok(())
}
fn main() {
    if let Err(err) = run() {
        eprintln!("TokenScope: {err}");
        std::process::exit(1);
    }
}
