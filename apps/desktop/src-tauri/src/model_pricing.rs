use crate::paths::{agent_is_running, database_path};
use crate::state::DashboardWatch;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};
use tokenscope_local_ipc::{
    PriceCatalogRefreshRequest, PriceCatalogRefreshResponse, PriceCatalogStatus,
    AGENT_CONTROL_DIRECTORY, PRICE_CATALOG_STATUS_FILENAME,
};
use tokenscope_local_store::{LocalStore, ModelPriceEntry, PricingPreferences};

#[derive(Serialize)]
pub(crate) struct ModelPriceCatalog {
    updated_at_ms: Option<i64>,
    last_attempt_ms: Option<i64>,
    last_error: Option<String>,
    entries: Vec<ModelPriceEntry>,
    preferences: PricingPreferences,
}

fn read_price_status(db_path: &Path) -> PriceCatalogStatus {
    let Some(path) = db_path.parent().map(|dir| dir.join(PRICE_CATALOG_STATUS_FILENAME)) else {
        return PriceCatalogStatus::default();
    };
    let Ok(bytes) = fs::read(path) else {
        return PriceCatalogStatus::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

#[tauri::command]
pub(crate) async fn get_model_price_catalog(app: AppHandle) -> Result<ModelPriceCatalog, String> {
    let path = database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let preferences = PricingPreferences::read(&path).map_err(|error| error.to_string())?;
        let status = read_price_status(&path);
        if !path.exists() {
            return Ok(ModelPriceCatalog {
                updated_at_ms: status.last_success_ms,
                last_attempt_ms: status.last_attempt_ms,
                last_error: status.last_error,
                entries: Vec::new(),
                preferences,
            });
        }
        let store = LocalStore::open_readonly(&path).map_err(|error| error.to_string())?;
        let (updated_at_ms, entries) = store
            .active_model_price_catalog()
            .map_err(|error| error.to_string())?;
        let state = store.model_price_catalog_state().ok();
        let last_attempt_ms = status
            .last_attempt_ms
            .or_else(|| state.as_ref().and_then(|s| s.last_attempt_ms));
        Ok(ModelPriceCatalog {
            updated_at_ms: updated_at_ms.or(status.last_success_ms),
            last_attempt_ms,
            last_error: status.last_error,
            entries,
            preferences,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub(crate) async fn refresh_model_price_catalog(
    app: AppHandle,
) -> Result<PriceCatalogRefreshResponse, String> {
    let db = database_path(&app)?;
    let response = tauri::async_runtime::spawn_blocking(move || -> Result<PriceCatalogRefreshResponse, String> {
        if !agent_is_running(&db)? {
            return Err("本地采集器未运行，请先确认采集运行后再试".into());
        }
        let directory = db
            .parent()
            .ok_or("无效数据库路径")?
            .join(AGENT_CONTROL_DIRECTORY);
        fs::create_dir_all(&directory).map_err(|e| format!("创建 Agent 控制目录失败：{e}"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("设置 Agent 控制目录权限失败：{e}"))?;
        }

        static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
        let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let request_id = format!("{:x}-{:x}-{:x}", std::process::id(), timestamp, sequence);
        let request = PriceCatalogRefreshRequest {
            request_id: request_id.clone(),
        };
        let body =
            serde_json::to_vec(&request).map_err(|e| format!("编码价格目录刷新请求失败：{e}"))?;
        let temporary = directory.join(format!("price-refresh-request-{request_id}.tmp"));
        let request_path = directory.join(format!("price-refresh-request-{request_id}.json"));
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let write_request = (|| -> io::Result<()> {
            let mut file = options.open(&temporary)?;
            file.write_all(&body)?;
            file.sync_all()?;
            fs::rename(&temporary, &request_path)
        })();
        if let Err(error) = write_request {
            let _ = fs::remove_file(&temporary);
            return Err(format!("提交价格目录刷新请求失败：{error}"));
        }

        let response_path = directory.join(format!("price-refresh-response-{request_id}.json"));
        let deadline = Instant::now() + Duration::from_secs(28);
        loop {
            match fs::read(&response_path) {
                Ok(bytes) => {
                    let _ = fs::remove_file(&response_path);
                    if bytes.len() > 8192 {
                        return Err("Agent 价格刷新响应超出大小限制".into());
                    }
                    let response: PriceCatalogRefreshResponse = serde_json::from_slice(&bytes)
                        .map_err(|e| format!("读取 Agent 价格刷新响应失败：{e}"))?;
                    if response.request_id != request_id {
                        return Err("Agent 价格刷新响应标识不匹配".into());
                    }
                    if let Some(error) = response.error.as_deref() {
                        return Err(error.to_owned());
                    }
                    return Ok(response);
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if Instant::now() >= deadline {
                        let _ = fs::remove_file(&request_path);
                        return Err("等待 Agent 刷新价格目录超时，请检查网络连接或稍后重试".into());
                    }
                    thread::sleep(Duration::from_millis(80));
                }
                Err(error) => {
                    let _ = fs::remove_file(&request_path);
                    return Err(format!("读取 Agent 价格刷新响应失败：{error}"));
                }
            }
        }
    })
    .await
    .map_err(|error| error.to_string())??;

    if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
        cache.clear();
    }
    Ok(response)
}

#[tauri::command]
pub(crate) async fn save_model_pricing_preferences(
    app: AppHandle,
    preferences: PricingPreferences,
) -> Result<(), String> {
    let path = database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        preferences.write(&path).map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;
    if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
        cache.clear();
    }
    Ok(())
}
