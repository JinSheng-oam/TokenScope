use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

#[tauri::command]
pub(crate) fn get_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("读取开机启动状态失败：{e}"))
}

#[tauri::command]
pub(crate) async fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|e| format!("修改开机启动设置失败：{e}"))?;
        manager
            .is_enabled()
            .map_err(|e| format!("确认开机启动状态失败：{e}"))
    })
    .await
    .map_err(|e| format!("开机启动设置任务失败：{e}"))?
}
