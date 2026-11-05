use std::time::{SystemTime, UNIX_EPOCH};

use tokenscope_local_store::LocalStore;

use super::{
    client::{WebDavClient, WebDavClientError},
    model::{
        WebDavBatchEntry, WebDavBatchFile, WebDavDeviceInfo, WebDavDeviceManifest,
        WebDavProjectNamesFile, WebDavSyncReport,
    },
};
use crate::sync::MAX_SYNC_BATCH;

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Push local outbox records to WebDAV for this device.
pub fn sync_webdav_push(
    client: &WebDavClient,
    store: &mut LocalStore,
    device_info: &WebDavDeviceInfo,
) -> Result<(usize, usize), WebDavClientError> {
    let now = now_ms();
    let pending = store
        .pending_outbox_due(MAX_SYNC_BATCH, now)
        .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

    // Always update device presence file in devices/{device_id}.json
    client.ensure_collection("devices")?;
    client.ensure_collection("data")?;
    let dev_data_dir = format!("data/{}", device_info.device_id);
    client.ensure_collection(&dev_data_dir)?;
    let mut updated_info = device_info.clone();
    updated_info.last_seen_ms = now;
    client.put_json(
        &format!("devices/{}.json", device_info.device_id),
        &updated_info,
    )?;

    let project_names = store
        .project_names()
        .map_err(|e| WebDavClientError::Transport(e.to_string()))?;
    let names_file = WebDavProjectNamesFile {
        protocol_version: 1,
        device_id: device_info.device_id.clone(),
        updated_at_ms: now,
        project_names,
    };
    client.put_json(&format!("{dev_data_dir}/project_names.json"), &names_file)?;

    if pending.is_empty() {
        return Ok((0, 0));
    }

    let manifest_path = format!("{dev_data_dir}/manifest.json");
    let mut manifest: WebDavDeviceManifest = client
        .get_json(&manifest_path)?
        .unwrap_or_else(|| WebDavDeviceManifest::new(device_info.device_id.clone()));

    let mut current_pending = pending;
    let mut total_acked = 0usize;
    let mut total_batches = 0usize;

    // Drain pending outbox records in bounded batches (up to 150 batches = 30,000 records per push)
    while !current_pending.is_empty() && total_batches < 150 {
        let batch_now = now_ms();
        let records: Vec<_> = current_pending.iter().map(|e| e.envelope.clone()).collect();
        let next_seq = manifest.last_batch_seq + 1;
        let batch_id = uuid::Uuid::new_v4().simple().to_string();
        let file_name = format!("batch_{next_seq:06}.json");
        let batch_file_path = format!("{dev_data_dir}/{file_name}");

        let batch_file = WebDavBatchFile {
            protocol_version: 1,
            batch_id: batch_id.clone(),
            device_id: device_info.device_id.clone(),
            batch_seq: next_seq,
            records,
        };

        // 1. Upload batch file
        client.put_json(&batch_file_path, &batch_file)?;

        // 2. Update manifest
        manifest.batches.push(WebDavBatchEntry {
            batch_seq: next_seq,
            batch_id,
            count: current_pending.len(),
            file_name,
            created_at_ms: batch_now,
        });
        manifest.last_batch_seq = next_seq;
        manifest.updated_at_ms = batch_now;
        client.put_json(&manifest_path, &manifest)?;

        // 3. Acknowledge uploaded records locally
        for entry in &current_pending {
            if store
                .ack_outbox_if_revision(
                    &entry.envelope.record.record_id,
                    entry.envelope.record_revision,
                )
                .map_err(|e| WebDavClientError::Transport(e.to_string()))?
            {
                total_acked += 1;
            }
        }
        total_batches += 1;

        // Fetch next pending batch
        current_pending = store
            .pending_outbox_due(MAX_SYNC_BATCH, batch_now)
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;
    }

    Ok((total_acked, total_batches))
}

/// Pull records from all remote devices on WebDAV into local store.
pub fn sync_webdav_pull(
    client: &WebDavClient,
    store: &mut LocalStore,
    local_device_id: &str,
) -> Result<(usize, usize, Vec<WebDavDeviceInfo>), WebDavClientError> {
    let now = now_ms();
    let device_files = client.list_device_files()?;
    let mut pulled_records = 0usize;
    let mut pulled_batches = 0usize;
    let mut remote_devices = Vec::new();

    for file in device_files {
        let dev_id = file.trim_end_matches(".json");
        let dev_path = format!("devices/{file}");
        let info: Option<WebDavDeviceInfo> = client.get_json(&dev_path)?;
        let Some(info) = info else { continue };

        remote_devices.push(info.clone());

        if dev_id == local_device_id {
            continue;
        }

        let project_names_path = format!("data/{dev_id}/project_names.json");
        if let Some(names_file) = client.get_json::<WebDavProjectNamesFile>(&project_names_path)? {
            if names_file.protocol_version != 1
                || names_file.device_id != dev_id
                || names_file.updated_at_ms < 0
            {
                return Err(WebDavClientError::Serialization(
                    "invalid project names document".into(),
                ));
            }
            store
                .merge_project_names(&names_file.project_names)
                .map_err(|e| WebDavClientError::Transport(e.to_string()))?;
        }

        let current_cursor = store
            .webdav_cursor(dev_id)
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

        let manifest_path = format!("data/{dev_id}/manifest.json");
        let manifest: Option<WebDavDeviceManifest> = client.get_json(&manifest_path)?;
        let Some(manifest) = manifest else { continue };

        // Process batches that are strictly greater than current cursor in order
        let mut missing_batches: Vec<_> = manifest
            .batches
            .into_iter()
            .filter(|b| b.batch_seq > current_cursor)
            .collect();
        missing_batches.sort_by_key(|b| b.batch_seq);

        for batch_entry in missing_batches {
            let batch_path = format!("data/{dev_id}/{}", batch_entry.file_name);
            let batch: Option<WebDavBatchFile> = client.get_json(&batch_path)?;
            let Some(batch) = batch else { continue };

            let applied = store
                .apply_remote_device_records(dev_id, batch_entry.batch_seq, &batch.records, now)
                .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

            pulled_records += applied;
            pulled_batches += 1;
        }
    }

    Ok((pulled_records, pulled_batches, remote_devices))
}

/// Execute a complete WebDAV push and pull sync cycle.
pub fn sync_webdav_cycle(
    client: &WebDavClient,
    store: &mut LocalStore,
    device_info: &WebDavDeviceInfo,
) -> Result<(WebDavSyncReport, Vec<WebDavDeviceInfo>), WebDavClientError> {
    let now = now_ms();
    let (up_records, up_batches) = match sync_webdav_push(client, store, device_info) {
        Ok(res) => res,
        Err(err) => {
            let status = match err {
                WebDavClientError::Unauthorized => "auth_required",
                _ => "error",
            };
            let _ = store.update_sync_state(now, status, Some("webdav_push_failed"));
            return Err(err);
        }
    };

    let (down_records, down_batches, devices) =
        match sync_webdav_pull(client, store, &device_info.device_id) {
            Ok(res) => res,
            Err(err) => {
                let status = match err {
                    WebDavClientError::Unauthorized => "auth_required",
                    _ => "error",
                };
                let _ = store.update_sync_state(now, status, Some("webdav_pull_failed"));
                return Err(err);
            }
        };

    let finished = now_ms();
    let _ = store.update_sync_state(finished, "success", None);

    let status_snapshot = store
        .sync_outbox_status(finished)
        .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

    let report = WebDavSyncReport {
        uploaded_records: up_records,
        uploaded_batches: up_batches,
        pulled_records: down_records,
        pulled_batches: down_batches,
        remaining_due: status_snapshot.pending as usize,
    };

    Ok((report, devices))
}
