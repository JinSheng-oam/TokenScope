use crate::{
    dashboard::query_agent_fresh,
    paths::{clear_sync_hub_url, persist_sync_hub_url},
};
use serde::Serialize;
use tauri::AppHandle;
use tokenscope_sync_client::{
    pairing::{DeviceListResponse, PairRequest},
    CredentialKey, HubClient, HubUrl, KeyringCredentialStore, SecretStore,
};
use zeroize::Zeroizing;

#[derive(Serialize)]
pub(crate) struct PairHubResult {
    hub_origin: String,
    device_id: String,
    created_at_ms: i64,
}

fn sync_context(
    app: &AppHandle,
    hub_url: &str,
) -> Result<(HubUrl, CredentialKey, String, String), String> {
    let hub = HubUrl::parse(hub_url).map_err(|e| e.to_string())?;
    let dashboard = query_agent_fresh(app, "all", 0, None, None)?;
    let identity = dashboard
        .snapshot
        .identity
        .ok_or_else(|| "本机设备身份尚未初始化，请先启动 Agent 并完成一次采集".to_string())?;
    let key = CredentialKey::for_device(&hub, &identity.device_id).map_err(|e| e.to_string())?;
    Ok((hub, key, identity.device_id, dashboard.snapshot.time_zone))
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
pub(crate) async fn pair_hub_device(
    app: AppHandle,
    hub_url: String,
    pairing_code: String,
    display_name: String,
) -> Result<PairHubResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut pairing_code = Zeroizing::new(pairing_code);
        let (hub, key, device_id, time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        // Probe the native secure store before consuming the one-time pairing code.
        store.preflight().map_err(|e| e.to_string())?;
        let request = PairRequest {
            pairing_code: std::mem::take(&mut *pairing_code),
            device_id: device_id.clone(),
            display_name: display_name.trim().to_string(),
            os_name: platform_os_name().to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            time_zone,
        };
        request.validate().map_err(|field| format!("配对信息无效：{field}"))?;
        let client = HubClient::new(hub.clone()).map_err(|e| e.to_string())?;
        let response = client.pair(&request).map_err(|e| e.to_string())?;
        if let Err(error) = store.save(&key, &response.device_token) {
            // The Hub has already consumed the code and registered the device.
            // Never expose the one-time token in UI/logs as a fallback.
            return Err(format!("Hub 已注册设备，但操作系统安全凭证库保存失败（{error}）。请在 Hub 管理端撤销该设备，再生成新配对码重新配对。"));
        }
        if let Err(error) = persist_sync_hub_url(&app, hub.origin()) {
            let _ = store.delete(&key);
            return Err(format!("设备已在 Hub 注册，但本机同步配置保存失败（{error}）。请在 Hub 管理端撤销该设备后重新配对。"));
        }
        Ok(PairHubResult {
            hub_origin: hub.origin().to_string(),
            device_id: response.device_id,
            created_at_ms: response.created_at_ms,
        })
    }).await.map_err(|e| format!("设备配对任务失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn list_hub_devices(
    app: AppHandle,
    hub_url: String,
) -> Result<DeviceListResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, _device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        let token = store
            .load(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "本机尚未与该 Hub 配对".to_string())?;
        HubClient::new(hub)
            .and_then(|client| client.list_devices(&token))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("读取 Hub 设备列表失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn rename_hub_device(
    app: AppHandle,
    hub_url: String,
    display_name: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        let token = store
            .load(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "本机尚未与该 Hub 配对".to_string())?;
        let response = HubClient::new(hub)
            .and_then(|client| client.rename_self(&token, &display_name))
            .map_err(|e| e.to_string())?;
        if response.device_id != device_id {
            return Err("Hub 返回的设备身份与本机不一致".into());
        }
        Ok(response.display_name)
    })
    .await
    .map_err(|e| format!("修改 Hub 设备名称失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn rotate_hub_device_token(
    app: AppHandle,
    hub_url: String,
) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        let old_token = store.load(&key).map_err(|e| e.to_string())?
            .ok_or_else(|| "本机尚未与该 Hub 配对".to_string())?;
        let rotated = HubClient::new(hub).and_then(|client| client.rotate_self_token(&old_token, &device_id)).map_err(|e| e.to_string())?;
        store.save(&key, &rotated.device_token).map_err(|error| format!(
            "Hub 已轮换凭证，但新凭证无法写入系统安全存储（{error}）。旧凭证已失效，请在 Hub 管理端撤销设备并重新配对。"
        ))?;
        Ok(rotated.created_at_ms)
    }).await.map_err(|e| format!("轮换 Hub 设备凭证失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn disconnect_hub_device(app: AppHandle, hub_url: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, _device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let _ = hub;
        let deleted = KeyringCredentialStore
            .delete(&key)
            .map_err(|e| e.to_string())?;
        clear_sync_hub_url(&app)?;
        Ok(deleted)
    })
    .await
    .map_err(|e| format!("删除本机 Hub 凭证失败：{e}"))?
}
