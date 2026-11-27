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

#[cfg(windows)]
fn parse_windows_run_executable(command: &str) -> Option<PathBuf> {
    let trimmed = command.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix('"') {
        let end = rest.find('"')?;
        let exe = &rest[..end];
        if exe.is_empty() {
            return None;
        }
        return Some(PathBuf::from(exe));
    }
    let exe = trimmed.split_whitespace().next()?;
    if exe.is_empty() {
        None
    } else {
        Some(PathBuf::from(exe))
    }
}

#[cfg(windows)]
fn normalize_windows_path(path: &std::path::Path) -> String {
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = resolved.to_string_lossy();
    let stripped = text.strip_prefix(r"\\?\").unwrap_or(&text);
    stripped.to_ascii_lowercase()
}

#[cfg(windows)]
fn read_windows_run_command(app_name: &str) -> Option<String> {
    use winreg::{
        enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ},
        RegKey,
    };

    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let Ok(key) = RegKey::predef(root)
            .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run", KEY_READ)
        else {
            continue;
        };
        if let Ok(value) = key.get_value::<String, _>(app_name) {
            return Some(value);
        }
    }
    None
}

#[cfg(windows)]
fn windows_autostart_enabled(app: &AppHandle) -> Result<bool, String> {
    if !app
        .autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())?
    {
        return Ok(false);
    }
    let Some(command) = read_windows_run_command(&app.package_info().name) else {
        return Ok(false);
    };
    let Some(registered_exe) = parse_windows_run_executable(&command) else {
        return Ok(false);
    };
    if !registered_exe.is_file() {
        return Ok(false);
    }
    let current_exe = std::env::current_exe().map_err(|error| error.to_string())?;
    Ok(normalize_windows_path(&registered_exe) == normalize_windows_path(&current_exe))
}

#[tauri::command]
pub(crate) fn get_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    #[cfg(windows)]
    {
        windows_autostart_enabled(&app).map_err(|e| format!("读取开机启动状态失败：{e}"))
    }
    #[cfg(not(windows))]
    {
        app.autolaunch()
            .is_enabled()
            .map_err(|e| format!("读取开机启动状态失败：{e}"))
    }
}

#[tauri::command]
pub(crate) async fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        let result = if enabled {
            enable_windows_autostart(&app)
        } else {
            disable_windows_autostart(&app)
        };
        #[cfg(not(windows))]
        let result = {
            let manager = app.autolaunch();
            if enabled {
                manager.enable()
            } else {
                manager.disable()
            }
            .map_err(|error| error.to_string())
        };
        result.map_err(|e| format!("修改开机启动设置失败：{e}"))?;
        #[cfg(windows)]
        let actual =
            windows_autostart_enabled(&app).map_err(|e| format!("确认开机启动状态失败：{e}"))?;
        #[cfg(not(windows))]
        let actual = app
            .autolaunch()
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

#[tauri::command]
pub(crate) async fn rename_local_device(
    app: AppHandle,
    display_name: String,
) -> Result<String, String> {
    let trimmed = display_name.trim().to_string();
    if trimmed.is_empty() || trimmed.chars().count() > 128 || trimmed.chars().any(char::is_control)
    {
        return Err("设备名称须为 1–128 个有效字符".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let db_file = crate::paths::database_path(&app)?;
        let mut store = tokenscope_local_store::LocalStore::open(&db_file)
            .map_err(|e| format!("打开本地数据库失败：{e}"))?;
        let mut identity = store
            .identity()
            .map_err(|e| format!("读取本机身份失败：{e}"))?
            .ok_or_else(|| "本机设备身份尚未初始化，请先启动采集器".to_string())?;
        identity.display_name = trimmed.clone();
        store
            .init_identity(&identity)
            .map_err(|e| format!("更新本机名称失败：{e}"))?;
        if let Ok(mut cache) = app.state::<crate::state::DashboardWatch>().cache.lock() {
            cache.clear();
        }
        Ok(trimmed)
    })
    .await
    .map_err(|e| format!("修改本机名称任务失败：{e}"))?
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

    #[cfg(windows)]
    #[test]
    fn parses_quoted_and_unquoted_windows_run_commands() {
        use super::parse_windows_run_executable;
        assert_eq!(
            parse_windows_run_executable(
                r#""C:\Program Files\TokenScope\tokenscope-desktop.exe" --background"#
            ),
            Some(Path::new(r"C:\Program Files\TokenScope\tokenscope-desktop.exe").to_path_buf())
        );
        assert_eq!(
            parse_windows_run_executable(r"D:\TokenScope\tokenscope-desktop.exe --background"),
            Some(Path::new(r"D:\TokenScope\tokenscope-desktop.exe").to_path_buf())
        );
        assert_eq!(parse_windows_run_executable(""), None);
    }
}
