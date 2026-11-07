use crate::paths::database_path;
use crate::state::DashboardWatch;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokenscope_local_store::{LocalStore, ModelPriceEntry, PricingPreferences};

#[derive(Serialize)]
pub(crate) struct ModelPriceCatalog {
    updated_at_ms: Option<i64>,
    entries: Vec<ModelPriceEntry>,
    preferences: PricingPreferences,
}

#[tauri::command]
pub(crate) async fn get_model_price_catalog(app: AppHandle) -> Result<ModelPriceCatalog, String> {
    let path = database_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let preferences = PricingPreferences::read(&path).map_err(|error| error.to_string())?;
        if !path.exists() {
            return Ok(ModelPriceCatalog {
                updated_at_ms: None,
                entries: Vec::new(),
                preferences,
            });
        }
        let store = LocalStore::open_readonly(&path).map_err(|error| error.to_string())?;
        let (updated_at_ms, entries) = store
            .active_model_price_catalog()
            .map_err(|error| error.to_string())?;
        Ok(ModelPriceCatalog {
            updated_at_ms,
            entries,
            preferences,
        })
    })
    .await
    .map_err(|error| error.to_string())?
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
