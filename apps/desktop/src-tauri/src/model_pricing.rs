use crate::agent_control::{next_request_id, send_control_request};
use crate::paths::database_path;
use crate::state::DashboardWatch;
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokenscope_local_ipc::{
    PriceCatalogRefreshRequest, PriceCatalogRefreshResponse, PriceCatalogStatus,
    PRICE_CATALOG_STATUS_FILENAME,
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
    let Some(path) = db_path
        .parent()
        .map(|dir| dir.join(PRICE_CATALOG_STATUS_FILENAME))
    else {
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
    let response = tauri::async_runtime::spawn_blocking(
        move || -> Result<PriceCatalogRefreshResponse, String> {
            let request_id = next_request_id();
            let request = PriceCatalogRefreshRequest {
                request_id: request_id.clone(),
            };
            let response: PriceCatalogRefreshResponse = send_control_request(
                &db,
                "price-refresh-request-",
                "price-refresh-response-",
                &request_id,
                &request,
                Duration::from_secs(28),
                "价格目录刷新",
            )?;
            if response.request_id != request_id {
                return Err("Agent 价格刷新响应标识不匹配".into());
            }
            if let Some(error) = response.error.as_deref() {
                return Err(error.to_owned());
            }
            Ok(response)
        },
    )
    .await
    .map_err(|error| error.to_string())??;

    if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
        cache.clear();
    }
    if let Ok(mut cache) = app.state::<DashboardWatch>().session_pages.lock() {
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
    if let Ok(mut cache) = app.state::<DashboardWatch>().session_pages.lock() {
        cache.clear();
    }
    Ok(())
}
