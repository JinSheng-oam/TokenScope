use crate::{
    background_quota::QuotaWorkers,
    collection::{collect_once, CollectionOptions},
    pricing, session_control, shutdown, sync,
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};
use tokenscope_local_store::LocalStore;

pub(super) struct DaemonOptions<'a> {
    pub(super) interval: u64,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) shutdown_file: Option<&'a Path>,
    pub(super) refresh_antigravity_quota: bool,
    pub(super) refresh_codex_quota: bool,
    pub(super) sync_enabled: bool,
    pub(super) sync_hub_url: Option<&'a str>,
    pub(super) sync_config_path: Option<&'a Path>,
    pub(super) sync_webdav_config_path: Option<&'a Path>,
}

impl DaemonOptions<'_> {
    fn shutdown_requested(&self) -> bool {
        self.stop.load(Ordering::Relaxed) || shutdown::consume(self.shutdown_file)
    }
}

fn sleep_interruptible(
    options: &DaemonOptions<'_>,
    seconds: u64,
    store: &mut LocalStore,
    db_path: &Path,
    quotas: &QuotaWorkers,
) -> bool {
    for _ in 0..seconds.saturating_mul(4) {
        if options.shutdown_requested() {
            return true;
        }
        if let Err(error) = session_control::process_pending_requests(store, db_path) {
            eprintln!("TokenScope session control warning: {error}");
        }
        quotas.persist_pending(store);
        thread::sleep(Duration::from_millis(250));
    }
    options.shutdown_requested()
}

pub(super) fn run(
    store: &mut LocalStore,
    collection: &CollectionOptions<'_>,
    db: &Path,
    device: &str,
    options: DaemonOptions<'_>,
) -> Result<(), Box<dyn std::error::Error>> {
    let stop_for_signal = options.stop.clone();
    ctrlc::set_handler(move || stop_for_signal.store(true, Ordering::Relaxed))?;
    if options.shutdown_requested() {
        return Ok(());
    }
    let quotas = QuotaWorkers::start(
        store,
        options.refresh_codex_quota,
        options.refresh_antigravity_quota,
    );
    let mut consecutive_failures = 0_u32;
    while !options.shutdown_requested() {
        if let Err(error) = session_control::process_pending_requests(store, db) {
            eprintln!("TokenScope session control warning: {error}");
        }
        quotas.persist_pending(store);
        if options.shutdown_requested() {
            break;
        }
        if collection.codex
            || collection.claude
            || collection.antigravity
            || collection.deepseek_harness
            || collection.pi
        {
            pricing::refresh_if_due(store);
            if options.shutdown_requested() {
                break;
            }
            match collect_once(store, collection) {
                Ok(()) => consecutive_failures = 0,
                Err(err) => {
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    eprintln!("TokenScope scan error (retry #{consecutive_failures}): {err}");
                }
            }
        }
        if options.shutdown_requested() {
            break;
        }
        quotas.persist_pending(store);
        if options.shutdown_requested() {
            break;
        }
        if options.sync_enabled {
            // 1. Hub Sync
            match sync::resolve_sync_hub_url(options.sync_hub_url, options.sync_config_path) {
                Ok(Some(raw_url)) => match sync::sync_device_once(store, &raw_url, device) {
                    Ok(_) if options.shutdown_requested() => break,
                    Ok(report) => match sync::pull_device_once(store, &raw_url, device) {
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
            if options.shutdown_requested() {
                break;
            }

            // 2. WebDAV Sync
            match sync::resolve_webdav_config(options.sync_webdav_config_path) {
                Ok(Some(config)) => {
                    let identity = store.identity().unwrap_or(None);
                    let display_name = identity
                        .as_ref()
                        .map(|i| i.display_name.as_str())
                        .unwrap_or("This device");
                    let time_zone = identity
                        .as_ref()
                        .map(|i| i.time_zone.as_str())
                        .unwrap_or("UTC");
                    match sync::sync_webdav_device_once(
                        store,
                        &config,
                        device,
                        display_name,
                        time_zone,
                    ) {
                        Ok((report, devices)) => {
                            if report.uploaded_records > 0 || report.pulled_records > 0 {
                                println!(
                                    "webdav sync uploaded_records={} uploaded_batches={} pulled_records={} pulled_batches={} remaining_due={} remote_devices={}",
                                    report.uploaded_records,
                                    report.uploaded_batches,
                                    report.pulled_records,
                                    report.pulled_batches,
                                    report.remaining_due,
                                    devices.len()
                                );
                            }
                        }
                        Err(err) => eprintln!("TokenScope WebDAV sync deferred: {err}"),
                    }
                }
                Ok(None) => {} // WebDAV not configured or disabled
                Err(err) => eprintln!("TokenScope WebDAV configuration invalid: {err}"),
            }
        }
        let backoff = if consecutive_failures == 0 {
            options.interval
        } else {
            options
                .interval
                .saturating_mul(1_u64 << consecutive_failures.min(4))
                .min(300)
        };
        if sleep_interruptible(&options, backoff, store, db, &quotas) {
            break;
        }
    }
    println!("TokenScope stopped cleanly");
    Ok(())
}
