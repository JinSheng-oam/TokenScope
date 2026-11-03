use std::path::Path;
use tokenscope_local_store::LocalStore;
use tokenscope_sync_client::{
    worker::{pull_usage_changes_once, sync_outbox_once},
    CredentialKey, HubClient, HubUrl, KeyringCredentialStore, SecretStore,
};
pub(super) fn resolve_sync_hub_url(
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

pub(super) fn sync_device_once(
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

pub(super) fn pull_device_once(
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
