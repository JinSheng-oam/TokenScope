use crate::{
    metadata::{import_titles_for_conversations, project_index},
    models::Snapshot,
    now_ms,
    snapshot::{discover_databases, dominates, read_db, source_fingerprint, source_records},
    AntigravityError, AntigravityReport, Result, PARSER_VERSION,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use tokenscope_core_domain::{stable_record_id, Precision};
use tokenscope_local_store::{AntigravityActivity, LocalStore, SourceCheckpoint};

type FileStamp = (u64, SystemTime, Option<SystemTime>);

fn database_stamp(path: &Path) -> io::Result<FileStamp> {
    fn sidecar(path: &Path, suffix: &str) -> PathBuf {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        PathBuf::from(name)
    }
    // WAL frames can be reused without growing the file, and write times
    // can lag on Windows. With a sidecar present, always read SQLite.
    for suffix in ["-wal", "-journal"] {
        match fs::metadata(sidecar(path, suffix)) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            _ => return Err(io::Error::other("database may have a live journal")),
        }
    }
    let metadata = fs::metadata(path)?;
    Ok((
        metadata.len(),
        metadata.modified()?,
        metadata.created().ok(),
    ))
}

struct CachedSnapshot {
    stamp: FileStamp,
    device_id: String,
    snapshot: Arc<Snapshot>,
}

/// Daemon-local reuse of complete source reads. Reconciliation still runs over
/// all current candidates, including duplicates and changed project metadata.
#[derive(Default)]
pub struct AntigravityReadCache {
    entries: BTreeMap<PathBuf, CachedSnapshot>,
    last_full_read: Option<Instant>,
}

impl AntigravityReadCache {
    fn begin_scan(&mut self, paths: &[PathBuf]) {
        if self
            .last_full_read
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(600))
        {
            self.entries.clear();
            self.last_full_read = Some(Instant::now());
        }
        let current: BTreeSet<_> = paths.iter().collect();
        self.entries.retain(|path, _| current.contains(path));
    }

    fn read(
        &mut self,
        path: &Path,
        stem: String,
        device_id: &str,
        project_id: Option<String>,
    ) -> Result<Arc<Snapshot>> {
        let before = database_stamp(path).ok();
        if let (Some(stamp), Some(cached)) = (&before, self.entries.get(path)) {
            if *stamp == cached.stamp
                && cached.device_id == device_id
                && cached.snapshot.project_id == project_id
            {
                return Ok(Arc::clone(&cached.snapshot));
            }
        }
        self.entries.remove(path);
        let snapshot = Arc::new(read_db(path, stem, device_id, project_id)?);
        if let Some(stamp) = before {
            let complete_nonempty = snapshot.skipped_rows == 0
                && (snapshot.known_turns > 0
                    || snapshot.unknown_turns > 0
                    || !snapshot.records.is_empty());
            let retained_turns: u64 = self
                .entries
                .values()
                .map(|entry| {
                    entry
                        .snapshot
                        .known_turns
                        .saturating_add(entry.snapshot.unknown_turns)
                })
                .sum();
            if complete_nonempty
                && self.entries.len() < 512
                && retained_turns
                    .saturating_add(snapshot.known_turns)
                    .saturating_add(snapshot.unknown_turns)
                    <= 100_000
                && database_stamp(path).ok().as_ref() == Some(&stamp)
            {
                self.entries.insert(
                    path.to_owned(),
                    CachedSnapshot {
                        stamp,
                        device_id: device_id.to_owned(),
                        snapshot: Arc::clone(&snapshot),
                    },
                );
            }
        }
        Ok(snapshot)
    }
}

pub fn collect_antigravity(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
) -> Result<AntigravityReport> {
    collect_antigravity_with_policy(gemini_home, store, device_id, estimate_tokens, false, None)
}

/// Desktop collection policy: unreadable databases are counted in
/// `skipped_sources` instead of discarding every other conversation.
pub fn collect_antigravity_tolerant(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
) -> Result<AntigravityReport> {
    collect_antigravity_with_policy(gemini_home, store, device_id, estimate_tokens, true, None)
}

pub fn collect_antigravity_cached(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
    cache: &mut AntigravityReadCache,
) -> Result<AntigravityReport> {
    collect_antigravity_with_policy(
        gemini_home,
        store,
        device_id,
        estimate_tokens,
        true,
        Some(cache),
    )
}

fn collect_antigravity_with_policy(
    gemini_home: &Path,
    store: &mut LocalStore,
    device_id: &str,
    estimate_tokens: bool,
    skip_broken: bool,
    mut cache: Option<&mut AntigravityReadCache>,
) -> Result<AntigravityReport> {
    let paths = discover_databases(gemini_home)?;
    if let Some(cache) = cache.as_deref_mut() {
        cache.begin_scan(&paths);
    }
    let projects = project_index(gemini_home);
    let mut report = AntigravityReport {
        source_files: paths.len(),
        ..AntigravityReport::default()
    };
    let mut snapshots: BTreeMap<String, Arc<Snapshot>> = BTreeMap::new();
    let mut ambiguous: BTreeSet<String> = BTreeSet::new();
    let mut incomplete: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or(AntigravityError::Invalid("database filename"))?
            .to_string();
        if stem.is_empty() || stem.len() > 200 || stem.chars().any(char::is_control) {
            return Err(AntigravityError::Invalid("invalid conversation filename"));
        }
        let source_key = stable_record_id("antigravity", b"conversation", stem.as_bytes())?;
        let prefix = path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str());
        let project = prefix
            .and_then(|prefix| projects.get(&(prefix.to_owned(), stem.clone())))
            .cloned();
        if let Some(project) = project.as_ref() {
            if let Some(name) = project.display_name.as_deref() {
                store.upsert_project_name(&project.project_id, name, now_ms()?)?;
            }
        }
        let project_id = project.map(|project| project.project_id);
        if store.is_session_suppressed("antigravity", &source_key)? {
            continue;
        }
        let read = match cache.as_deref_mut() {
            Some(cache) => cache.read(&path, stem.clone(), device_id, project_id),
            None => read_db(&path, stem.clone(), device_id, project_id).map(Arc::new),
        };
        let snap = match read {
            Ok(snap) => snap,
            Err(_) if skip_broken => {
                report.skipped_sources += 1;
                incomplete.insert(stem);
                continue;
            }
            Err(e) => return Err(e),
        };
        if snap.skipped_rows > 0 {
            report.skipped_rows = report.skipped_rows.saturating_add(snap.skipped_rows);
            report.skipped_sources += 1;
            incomplete.insert(stem);
            continue;
        }
        if let Some(previous) = snapshots.get(&stem) {
            if dominates(&snap, previous) {
                snapshots.insert(stem, snap);
            } else if !dominates(previous, &snap) {
                ambiguous.insert(stem);
            }
        } else {
            snapshots.insert(stem, snap);
        }
    }
    report.conversations = snapshots
        .keys()
        .filter(|stem| !ambiguous.contains(*stem) && !incomplete.contains(*stem))
        .count();
    report.ambiguous_conversations = ambiguous.len();
    let snapshot_stems: Vec<String> = snapshots.keys().cloned().collect();
    for (stem, snapshot) in snapshots {
        if ambiguous.contains(&stem) || incomplete.contains(&stem) {
            continue;
        }
        report.model_turns = report.model_turns.saturating_add(snapshot.known_turns);
        report.unknown_model_turns = report
            .unknown_model_turns
            .saturating_add(snapshot.unknown_turns);
        let source_key = stable_record_id("antigravity", b"conversation", stem.as_bytes())?;
        // Keep the last complete estimate when this explicit-estimate scan
        // encounters unknown models; partial metadata cannot safely revise it.
        if estimate_tokens && snapshot.unknown_turns > 0 {
            store.clear_empty_source_candidate("antigravity", &source_key)?;
            continue;
        }
        let previous = store.checkpoint("antigravity", &source_key)?;
        let fp = source_fingerprint(&snapshot, estimate_tokens);
        let complete_empty = snapshot.records.is_empty()
            && snapshot.known_turns == 0
            && snapshot.unknown_turns == 0
            && snapshot.missing_usage.is_empty();
        if !complete_empty {
            store.clear_empty_source_candidate("antigravity", &source_key)?;
        }
        if previous.as_ref().is_some_and(|p| {
            p.source_fingerprint == fp
                && p.parser_version == PARSER_VERSION
                && p.last_status == "complete"
        }) {
            report.unchanged += 1;
            continue;
        }
        let observed_at = now_ms()?.max(previous.as_ref().map_or(0, |p| p.last_scan_ms));
        let next = SourceCheckpoint {
            tool_id: "antigravity".into(),
            source_key,
            source_fingerprint: fp,
            read_offset: 0,
            parser_version: PARSER_VERSION,
            last_scan_ms: observed_at,
            last_complete_scan_ms: Some(observed_at),
            last_status: "complete".into(),
        };
        let activity = AntigravityActivity {
            known_turns: snapshot.known_turns,
            unknown_turns: snapshot.unknown_turns,
            dated_unmeasured: snapshot
                .dated_unmeasured
                .iter()
                .map(|((model, time), count)| (model.clone(), *time, *count))
                .collect(),
            models: snapshot
                .counts
                .iter()
                .map(|(m, n)| (m.clone(), *n))
                .collect(),
        };
        if let Some(expected) = previous.as_ref() {
            let prior_records =
                store.records_for_source("antigravity", &next.source_key, 20_000)?;
            let active_prior = prior_records
                .iter()
                .map(|row| &row.record)
                .filter(|record| !record.is_deleted)
                .collect::<Vec<_>>();
            let missing_prior_native = active_prior.iter().any(|record| {
                record.precision != Precision::Estimated
                    && !snapshot
                        .records
                        .iter()
                        .any(|current| current.record_id == record.record_id)
            });
            if !active_prior.is_empty() && complete_empty {
                if let Some(result) = store.reconcile_confirmed_empty_source(
                    expected,
                    &next,
                    &activity,
                    observed_at,
                )? {
                    report.tombstoned += result.tombstoned;
                }
                continue;
            }
            let has_unmeasured_turns =
                !snapshot.missing_usage.is_empty() || snapshot.unknown_turns > 0;
            if has_unmeasured_turns && missing_prior_native {
                // A tokenless/partial snapshot cannot prove that previously
                // measured generations were deleted. Keep the last complete
                // projection so missing counters cannot create tombstones.
                continue;
            }
        }
        let records = source_records(&snapshot, device_id, estimate_tokens)?;
        report.measured_records += snapshot.records.len();
        report.estimated_records += records.len().saturating_sub(snapshot.records.len());
        let result = store.reconcile_source_with_activity(
            previous.as_ref(),
            &next,
            records,
            observed_at,
            Some(&activity),
        )?;
        report.inserted += result.applied.inserted;
        report.revised += result.applied.revised;
        report.tombstoned += result.tombstoned;
    }
    let conversation_ids: Vec<&str> = snapshot_stems
        .iter()
        .chain(ambiguous.iter())
        .chain(incomplete.iter())
        .map(String::as_str)
        .collect();
    // Titles live in a separate summaries DB; failing to read them must not
    // fail usage collection.
    let _ = import_titles_for_conversations(gemini_home, store, &conversation_ids);
    Ok(report)
}
