use crate::{
    antigravity_rpc, codex_rpc,
    collection::{collect_once, CollectionOptions},
    daemon::{self, DaemonOptions},
    dashboard, diagnostics, identity,
    lock::AgentLock,
    quota, sync,
};
use std::{env, path::PathBuf};
use tokenscope_local_ipc::{DashboardResponse, SessionAnalysisResponse};
use tokenscope_local_store::{LocalIdentity, LocalStore};
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
fn deepseek_harness_home() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(home) =
        env::var_os("DSH_HOME").filter(|value| !value.to_string_lossy().trim().is_empty())
    {
        return Ok(PathBuf::from(home));
    }
    let base = env::var_os("USERPROFILE")
        .or_else(|| env::var_os("HOME"))
        .ok_or("missing USERPROFILE/HOME")?;
    Ok(PathBuf::from(base).join(".dsh"))
}
fn usage() {
    println!(
        r#"Usage: tokenscope-agent [--scan-codex | --scan-claude | --scan-antigravity | --scan-deepseek-harness | --scan-all]
    --db PATH [--device-id ID] [--codex-home PATH] [--claude-home PATH]
    [--antigravity-home PATH] [--deepseek-harness-home PATH] [--antigravity-estimates]
    [--import-antigravity-quota-json PATH] [--refresh-antigravity-quota] [--refresh-codex-quota] [--show-antigravity]
    [--display-name NAME] [--time-zone ZONE] [--daemon --interval-seconds 120]
    --dashboard-json [--range today|7d|30d|all]
    --diagnose-json
    --session-analysis-json [--range today|7d|30d|all] [--session-search TEXT]
    --shutdown-file PATH (daemon only; desktop lifecycle signal)
    --sync --hub-url HTTPS_OR_LOOPBACK | --sync-config PATH (daemon mode enables periodic sync)
    --sync-config PATH (plain-text non-secret Hub origin; read dynamically by daemon)
    --sync-once --hub-url HTTPS_OR_LOOPBACK (one-shot push + pull)
    --sync-status (show local Outbox retry / conflict state)"#
    );
}

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
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
    let deepseek_harness = args
        .iter()
        .any(|s| s == "--scan-deepseek-harness" || s == "--scan-all");
    let ag_estimates = args.iter().any(|s| s == "--antigravity-estimates");
    let quota_path = arg(&args, "--import-antigravity-quota-json");
    let show_antigravity = args.iter().any(|s| s == "--show-antigravity");
    let refresh_antigravity_quota = args.iter().any(|s| s == "--refresh-antigravity-quota");
    let refresh_codex_quota = args.iter().any(|s| s == "--refresh-codex-quota");
    let dashboard_json = args.iter().any(|s| s == "--dashboard-json");
    let diagnose_json = args.iter().any(|s| s == "--diagnose-json");
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
    if diagnose_json {
        if codex
            || claude
            || antigravity
            || deepseek_harness
            || quota_path.is_some()
            || show_antigravity
            || refresh_antigravity_quota
            || refresh_codex_quota
            || dashboard_json
            || session_analysis_json
            || sync_once
            || sync_enabled
            || sync_status
            || args.iter().any(|s| s == "--daemon")
        {
            return Err(
                "diagnostic query cannot be combined with collection or other commands".into(),
            );
        }
        let db = PathBuf::from(arg(&args, "--db").ok_or("missing --db")?);
        let report = diagnostics::diagnose(&db);
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    if !codex
        && !claude
        && !antigravity
        && quota_path.is_none()
        && !show_antigravity
        && !refresh_antigravity_quota
        && !refresh_codex_quota
        && !dashboard_json
        && !diagnose_json
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
            return Err(
                "session analysis query cannot be combined with collection or quota commands"
                    .into(),
            );
        }
        let db = PathBuf::from(arg(&args, "--db").ok_or("missing --db")?);
        let start_date = arg(&args, "--start-date");
        let end_date = arg(&args, "--end-date");
        let search = arg(&args, "--session-search");
        let tool_id = arg(&args, "--session-tool");
        let model_id = arg(&args, "--session-model");
        let project_id = arg(&args, "--session-project");
        let analysis = dashboard::read_session_analysis(
            &db,
            dashboard::SessionAnalysisOptions {
                range: &dashboard_range,
                session_offset,
                search: search.as_deref(),
                tool_id: tool_id.as_deref(),
                model_id: model_id.as_deref(),
                project_id: project_id.as_deref(),
                start_date: start_date.as_deref(),
                end_date: end_date.as_deref(),
            },
        )?;
        println!(
            "{}",
            serde_json::to_string(&SessionAnalysisResponse::new(analysis))?
        );
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
    let read_only_store = dashboard_json || show_antigravity;
    // A vanished database whose identity marker survived is a replacement, not
    // a first run. Refuse before SQLite recreates an empty file.
    let marker = identity::marker_path(&db);
    let marked = if read_only_store {
        None
    } else {
        identity::read(&marker)?
    };
    if marked.is_some() && !db.is_file() {
        return Err(format!(
            "database {} is missing but device identity marker {} exists; refusing to create a new database (restore the file, or remove the marker to start fresh)",
            db.display(),
            marker.display()
        )
        .into());
    }
    // Short-lived read commands must not open the write connection: doing so
    // checkpoints the WAL on every dashboard refresh and dominates latency.
    let mut store = if read_only_store {
        LocalStore::open_readonly(&db)?
    } else {
        LocalStore::open(&db)?
    };
    let prior = store.identity()?;
    if !read_only_store {
        if let (Some(prior), Some(marked)) = (prior.as_ref(), marked.as_ref()) {
            if prior.device_id != *marked {
                return Err(format!(
                    "device identity mismatch: database={} marker={} ({}); the database may have been replaced, refusing to scan (restore the original database, or remove the marker to adopt this one)",
                    prior.device_id,
                    marked,
                    marker.display()
                )
                .into());
            }
        }
        if prior.is_none() && marked.is_some() {
            return Err(format!(
                "database {} has no device identity although marker {} exists; refusing to initialize over an unexpected file",
                db.display(),
                marker.display()
            )
            .into());
        }
    }
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
    if !read_only_store {
        if marked.as_deref() != Some(device.as_str()) {
            identity::write(&marker, &device)?;
        }
        let schema = store.schema_version()?;
        println!(
            "TokenScope agent db={} schema={} device={} marker={}",
            db.display(),
            schema,
            device,
            if marked.is_some() {
                "present"
            } else {
                "created"
            }
        );
        store.quick_integrity_check()?;
        println!("TokenScope agent integrity=ok");
    }
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
    let deepseek_harness_root = match arg(&args, "--deepseek-harness-home") {
        Some(path) => PathBuf::from(path),
        None => deepseek_harness_home()?,
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
        if codex || claude || antigravity || deepseek_harness {
            collect_once(
                &mut store,
                &CollectionOptions {
                    codex,
                    claude,
                    antigravity,
                    deepseek_harness,
                    codex_root: &codex_root,
                    claude_root: &claude_root,
                    antigravity_root: &ag_root,
                    deepseek_harness_root: &deepseek_harness_root,
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
                sync::resolve_sync_hub_url(sync_hub_url.as_deref(), sync_config_path.as_deref())?
                    .ok_or("no Hub URL configured; pair this device or provide --hub-url")?;
            let report = sync::sync_device_once(&mut store, &raw_url, &device)?;
            let pulled = sync::pull_device_once(&mut store, &raw_url, &device)?;
            println!("sync uploaded={} unchanged={} superseded={} acknowledged={} blocked={} remaining_due={} remote_changes_applied={}",
                report.uploaded, report.unchanged, report.superseded, report.acknowledged, report.blocked, report.remaining_due, pulled);
        }
        return Ok(());
    }
    daemon::run(
        &mut store,
        &CollectionOptions {
            codex,
            claude,
            antigravity,
            deepseek_harness,
            codex_root: &codex_root,
            claude_root: &claude_root,
            antigravity_root: &ag_root,
            deepseek_harness_root: &deepseek_harness_root,
            allow_antigravity_estimates: ag_estimates,
            device_id: &device,
        },
        &db,
        &device,
        DaemonOptions {
            interval,
            shutdown_file: shutdown_file.as_deref(),
            refresh_antigravity_quota,
            refresh_codex_quota,
            sync_enabled,
            sync_hub_url: sync_hub_url.as_deref(),
            sync_config_path: sync_config_path.as_deref(),
        },
    )
}
