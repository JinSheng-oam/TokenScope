use std::path::Path;
use tokenscope_local_store::LocalStore;
use tokenscope_sync_client::{
    webdav::{
        sync_webdav_cycle, KeyringWebDavStore, WebDavClient, WebDavCredentialKey,
        WebDavCredentials, WebDavDeviceInfo, WebDavSecretStore, WebDavSyncReport, WebDavUrl,
    },
    worker::{
        pull_project_names_once, pull_usage_changes_once, push_project_names_once, sync_outbox_once,
    },
    CredentialKey, HubClient, HubUrl, KeyringCredentialStore, SecretStore,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WebDavSyncConfigFile {
    pub enabled: bool,
    pub url: String,
    pub username: String,
}

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

pub(super) fn resolve_webdav_config(
    config_path: Option<&Path>,
) -> Result<Option<WebDavSyncConfigFile>, Box<dyn std::error::Error>> {
    let Some(path) = config_path else {
        return Ok(None);
    };
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            let config: WebDavSyncConfigFile = serde_json::from_str(trimmed)?;
            if !config.enabled || config.url.trim().is_empty() || config.username.trim().is_empty()
            {
                return Ok(None);
            }
            Ok(Some(config))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Box::new(error)),
    }
}

pub(super) fn platform_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

pub(super) fn sync_webdav_device_once(
    store: &mut LocalStore,
    config: &WebDavSyncConfigFile,
    device_id: &str,
    display_name: &str,
    time_zone: &str,
) -> Result<(WebDavSyncReport, Vec<WebDavDeviceInfo>), Box<dyn std::error::Error>> {
    let url = WebDavUrl::parse(&config.url)?;
    let key = WebDavCredentialKey::for_endpoint(&config.url, &config.username);
    let store_keyring = KeyringWebDavStore;
    let password = store_keyring
        .load(&key)
        .map_err(|e| format!("无法从安全密码库读取 WebDAV 凭据：{e}"))?
        .ok_or("未找到该 WebDAV 服务的密码，请重新保存 WebDAV 配置")?;

    let credentials = WebDavCredentials {
        username: config.username.clone(),
        password,
    };
    let client = WebDavClient::new(url, credentials)?;

    let dev_info = WebDavDeviceInfo {
        device_id: device_id.to_string(),
        display_name: display_name.to_string(),
        os_name: platform_os_name().to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        time_zone: time_zone.to_string(),
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        last_seen_ms: chrono::Utc::now().timestamp_millis(),
    };

    let result = sync_webdav_cycle(&client, store, &dev_info)
        .map_err(|e| format!("WebDAV 同步失败：{e}"))?;
    Ok(result)
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
    push_project_names_once(&client, store, &token)
        .map_err(|e| format!("project name push failed: {e:?}"))?;
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
    pull_project_names_once(&client, store, &token)
        .map_err(|e| format!("project name pull failed: {e:?}"))?;
    Ok(applied)
}
