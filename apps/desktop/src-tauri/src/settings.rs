use std::{fs, path::PathBuf};
use tauri::{window::Color, AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as AutostartManagerExt;

const THEME_PREFERENCE_FILE: &str = "theme-preference.txt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThemePreference {
    Dark,
    Light,
    System,
}

fn parse_theme_preference(raw: &str) -> ThemePreference {
    match raw.trim() {
        "light" => ThemePreference::Light,
        "system" => ThemePreference::System,
        _ => ThemePreference::Dark,
    }
}

fn theme_preference_value(mode: &str) -> Result<&'static str, String> {
    match mode.trim() {
        "dark" => Ok("dark"),
        "light" => Ok("light"),
        "system" => Ok("system"),
        _ => Err("不支持的主题模式".into()),
    }
}

fn resolve_theme_darkness(pref: ThemePreference, system_is_dark: bool) -> bool {
    match pref {
        ThemePreference::Dark => true,
        ThemePreference::Light => false,
        ThemePreference::System => system_is_dark,
    }
}

#[cfg(windows)]
fn system_prefers_dark() -> bool {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize")
    else {
        return true;
    };
    let Ok(apps_use_light_theme) = key.get_value::<u32, _>("AppsUseLightTheme") else {
        return true;
    };
    apps_use_light_theme == 0
}

#[cfg(not(windows))]
fn system_prefers_dark() -> bool {
    true
}

fn theme_preference_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("无法获取应用本地数据目录：{e}"))?;
    Ok(dir.join(THEME_PREFERENCE_FILE))
}

pub(crate) fn resolve_is_dark(app: &AppHandle) -> bool {
    let pref = theme_preference_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .map(|raw| parse_theme_preference(&raw))
        .unwrap_or(ThemePreference::Dark);
    resolve_theme_darkness(pref, system_prefers_dark())
}

pub(crate) fn canvas_color(is_dark: bool) -> Color {
    if is_dark {
        Color(11, 13, 17, 255) // #0B0D11
    } else {
        Color(246, 247, 249, 255) // #F6F7F9
    }
}

#[tauri::command]
pub(crate) async fn set_theme_preference(app: AppHandle, mode: String) -> Result<(), String> {
    let value = theme_preference_value(&mode)?;
    let path = theme_preference_path(&app)?;
    let is_dark = resolve_theme_darkness(parse_theme_preference(value), system_prefers_dark());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_background_color(Some(canvas_color(is_dark)));
    }
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建主题配置目录失败：{e}"))?;
        }
        fs::write(&path, value).map_err(|e| format!("保存主题设置失败：{e}"))
    })
    .await
    .map_err(|e| format!("保存主题设置任务失败：{e}"))?
}

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

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::quoted_windows_executable;
    use super::{parse_theme_preference, resolve_theme_darkness, ThemePreference};
    #[cfg(windows)]
    use std::path::Path;

    #[test]
    fn theme_preference_resolves_dark_light_and_system() {
        assert_eq!(parse_theme_preference("light"), ThemePreference::Light);
        assert_eq!(parse_theme_preference("system"), ThemePreference::System);
        assert_eq!(parse_theme_preference("dark"), ThemePreference::Dark);
        assert_eq!(parse_theme_preference("unknown"), ThemePreference::Dark);

        assert!(resolve_theme_darkness(ThemePreference::Dark, false));
        assert!(!resolve_theme_darkness(ThemePreference::Light, true));
        assert!(resolve_theme_darkness(ThemePreference::System, true));
        assert!(!resolve_theme_darkness(ThemePreference::System, false));
    }

    #[cfg(windows)]
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
