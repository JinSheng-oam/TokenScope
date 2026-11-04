use crate::{
    dashboard::query_agent_fresh,
    paths::{
        clear_webdav_config_file, database_path, persist_webdav_config_file, webdav_config_path,
    },
};
use serde::{Deserialize, Serialize};
use std::fs;
use tauri::AppHandle;
use tokenscope_sync_client::{
    pairing::{DeviceListResponse, DeviceSummary},
    webdav::{
        sync_webdav_cycle, KeyringWebDavStore, WebDavClient, WebDavCredentialKey,
        WebDavCredentials, WebDavDeviceInfo, WebDavSecretStore, WebDavSyncReport, WebDavUrl,
    },
};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WebDavConfigFile {
    pub enabled: bool,
    pub url: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WebDavPublicConfig {
    pub enabled: bool,
    pub url: String,
    pub username: String,
}

fn platform_os_name() -> &'static str {
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

#[tauri::command]
pub(crate) async fn test_webdav_connection(
    url: String,
    username: String,
    password: String,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let parsed_url = WebDavUrl::parse(&url).map_err(|e| e.to_string())?;
        let credentials = WebDavCredentials {
            username: username.trim().to_string(),
            password: Zeroizing::new(password),
        };
        let client = WebDavClient::new(parsed_url, credentials).map_err(|e| e.to_string())?;
        client.test_connection().map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
    .map_err(|e| format!("测试连接任务失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn save_webdav_config(
    app: AppHandle,
    url: String,
    username: String,
    password: String,
) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let parsed_url = WebDavUrl::parse(&url).map_err(|e| e.to_string())?;
        let username_trimmed = username.trim().to_string();
        if username_trimmed.is_empty() {
            return Err("WebDAV 用户名不能为空".into());
        }

        // Test connection first
        let credentials = WebDavCredentials {
            username: username_trimmed.clone(),
            password: Zeroizing::new(password.clone()),
        };
        let client = WebDavClient::new(parsed_url.clone(), credentials).map_err(|e| e.to_string())?;
        client.test_connection().map_err(|e| format!("WebDAV 连接测试未通过：{e}"))?;

        // Save password in system keyring
        let store = KeyringWebDavStore;
        store.preflight().map_err(|e| format!("系统凭据库预检失败：{e}"))?;
        let key = WebDavCredentialKey::for_endpoint(parsed_url.as_str(), &username_trimmed);
        store.save(&key, &password).map_err(|e| format!("保存 WebDAV 凭据到系统安全库失败：{e}"))?;

        // Persist config file
        let config = WebDavConfigFile {
            enabled: true,
            url: parsed_url.as_str().to_string(),
            username: username_trimmed,
        };
        let json_str = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("序列化 WebDAV 配置失败：{e}"))?;
        persist_webdav_config_file(&app, &json_str)?;

        Ok(true)
    })
    .await
    .map_err(|e| format!("保存 WebDAV 设置任务失败：{e}"))?
}

#[tauri::command]
pub(crate) fn get_webdav_config(app: AppHandle) -> Result<Option<WebDavPublicConfig>, String> {
    let path = webdav_config_path(&app)?;
    if !path.is_file() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path).map_err(|e| format!("读取 WebDAV 配置文件失败：{e}"))?;
    let config: WebDavConfigFile = serde_json::from_str(&content)
        .map_err(|e| format!("解析 WebDAV 配置文件失败：{e}"))?;
    Ok(Some(WebDavPublicConfig {
        enabled: config.enabled,
        url: config.url,
        username: config.username,
    }))
}

#[tauri::command]
pub(crate) async fn disconnect_webdav(app: AppHandle) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = webdav_config_path(&app)?;
        if path.is_file() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(config) = serde_json::from_str::<WebDavConfigFile>(&content) {
                    let key = WebDavCredentialKey::for_endpoint(&config.url, &config.username);
                    let _ = KeyringWebDavStore.delete(&key);
                }
            }
        }
        clear_webdav_config_file(&app)?;
        Ok(true)
    })
    .await
    .map_err(|e| format!("断开 WebDAV 任务失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn list_webdav_devices(app: AppHandle) -> Result<DeviceListResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = webdav_config_path(&app)?;
        if !path.is_file() {
            return Err("尚未配置 WebDAV 同步".into());
        }
        let content = fs::read_to_string(&path).map_err(|e| format!("读取配置文件失败：{e}"))?;
        let config: WebDavConfigFile = serde_json::from_str(&content)
            .map_err(|e| format!("解析配置文件失败：{e}"))?;

        let parsed_url = WebDavUrl::parse(&config.url).map_err(|e| e.to_string())?;
        let key = WebDavCredentialKey::for_endpoint(&config.url, &config.username);
        let password = KeyringWebDavStore
            .load(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "未找到已保存的 WebDAV 密码".to_string())?;

        let credentials = WebDavCredentials {
            username: config.username,
            password,
        };
        let client = WebDavClient::new(parsed_url, credentials).map_err(|e| e.to_string())?;
        let filenames = client.list_device_files().map_err(|e| e.to_string())?;

        let mut devices = Vec::new();
        for file in filenames {
            let path = format!("devices/{file}");
            if let Ok(Some(info)) = client.get_json::<WebDavDeviceInfo>(&path) {
                devices.push(DeviceSummary {
                    device_id: info.device_id,
                    display_name: info.display_name,
                    os_name: info.os_name,
                    app_version: info.app_version,
                    time_zone: info.time_zone,
                    created_at_ms: info.created_at_ms,
                    last_seen_ms: Some(info.last_seen_ms),
                    revoked_at_ms: None,
                });
            }
        }

        Ok(DeviceListResponse {
            protocol_version: 1,
            devices,
        })
    })
    .await
    .map_err(|e| format!("读取 WebDAV 设备列表失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn sync_webdav_now(app: AppHandle) -> Result<WebDavSyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = webdav_config_path(&app)?;
        if !path.is_file() {
            return Err("尚未配置 WebDAV 同步".into());
        }
        let content = fs::read_to_string(&path).map_err(|e| format!("读取配置文件失败：{e}"))?;
        let config: WebDavConfigFile = serde_json::from_str(&content)
            .map_err(|e| format!("解析配置文件失败：{e}"))?;

        let parsed_url = WebDavUrl::parse(&config.url).map_err(|e| e.to_string())?;
        let key = WebDavCredentialKey::for_endpoint(&config.url, &config.username);
        let password = KeyringWebDavStore
            .load(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "未找到已保存的 WebDAV 密码".to_string())?;

        let credentials = WebDavCredentials {
            username: config.username,
            password,
        };
        let client = WebDavClient::new(parsed_url, credentials).map_err(|e| e.to_string())?;

        let db_file = database_path(&app)?;
        let mut store = tokenscope_local_store::LocalStore::open(&db_file)
            .map_err(|e| format!("打开本地数据库失败：{e}"))?;

        let dashboard = query_agent_fresh(&app, "today", 0, None, None)?;
        let identity = dashboard
            .snapshot
            .identity
            .ok_or_else(|| "本机设备身份尚未初始化，请先启动采集器".to_string())?;

        let dev_info = WebDavDeviceInfo {
            device_id: identity.device_id,
            display_name: identity.display_name,
            os_name: platform_os_name().to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            time_zone: dashboard.snapshot.time_zone,
            created_at_ms: chrono::Utc::now().timestamp_millis(),
            last_seen_ms: chrono::Utc::now().timestamp_millis(),
        };

        let (report, _) = sync_webdav_cycle(&client, &mut store, &dev_info)
            .map_err(|e| format!("WebDAV 同步失败：{e}"))?;

        Ok(report)
    })
    .await
    .map_err(|e| format!("立即同步任务失败：{e}"))?
}
