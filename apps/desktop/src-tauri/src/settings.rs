use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

#[cfg(windows)]
fn quoted_windows_executable(path: &std::path::Path) -> Result<String, String> {
    let value = path.to_str().ok_or("应用路径包含无法识别的字符")?;
    if value.is_empty() || value.contains(['\"', '\0', '\r', '\n']) {
        return Err("应用路径无效".into());
    }
    Ok(format!("\"{value}\""))
}

#[cfg(windows)]
fn enable_windows_autostart(app: &AppHandle) -> Result<(), String> {
    use auto_launch::{AutoLaunchBuilder, WindowsEnableMode};

    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let quoted_path = quoted_windows_executable(&executable)?;
    let mut builder = AutoLaunchBuilder::new();
    builder
        .set_app_name(&app.package_info().name)
        .set_app_path(&quoted_path)
        .set_args(&["--background"])
        .set_windows_enable_mode(WindowsEnableMode::CurrentUser);
    builder
        .build()
        .and_then(|manager| manager.enable())
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn disable_windows_autostart(app: &AppHandle) -> Result<(), String> {
    use winreg::{
        enums::{HKEY_CURRENT_USER, KEY_SET_VALUE},
        RegKey,
    };

    let user = RegKey::predef(HKEY_CURRENT_USER);
    let key = match user.open_subkey_with_flags(
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        KEY_SET_VALUE,
    ) {
        Ok(key) => key,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    match key.delete_value(&app.package_info().name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

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
        #[cfg(windows)]
        let result = if enabled {
            enable_windows_autostart(&app)
        } else {
            disable_windows_autostart(&app)
        };
        #[cfg(not(windows))]
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .map_err(|error| error.to_string());
        result.map_err(|e| format!("修改开机启动设置失败：{e}"))?;
        let actual = manager
            .is_enabled()
            .map_err(|e| format!("确认开机启动状态失败：{e}"))?;
        if actual != enabled {
            return Err("系统启动项状态未改变，请检查系统启动设置或管理员策略。".into());
        }
        Ok(actual)
    })
    .await
    .map_err(|e| format!("开机启动设置任务失败：{e}"))?
}

#[cfg(all(test, windows))]
mod tests {
    use super::quoted_windows_executable;
    use std::path::Path;

    #[test]
    fn startup_executable_path_is_quoted_even_with_spaces() {
        assert_eq!(
            quoted_windows_executable(Path::new(
                r"C:\Program Files\TokenScope\tokenscope-desktop.exe"
            ))
            .unwrap(),
            r#""C:\Program Files\TokenScope\tokenscope-desktop.exe""#
        );
        assert!(quoted_windows_executable(Path::new("bad\"path")).is_err());
    }
}
