use crate::state::AppLifecycle;
use std::sync::atomic::Ordering;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    webview::WebviewWindowBuilder,
    AppHandle, Manager,
};

pub(crate) fn show_or_create_main_window(app: &AppHandle) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window("main") {
        window
    } else {
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|window| window.label == "main")
            .ok_or_else(|| "Tauri 配置中缺少 main 窗口".to_string())?;
        WebviewWindowBuilder::from_config(app, config)
            .and_then(|builder| builder.build())
            .map_err(|e| format!("创建 TokenScope 窗口失败：{e}"))?
    };
    let _ = window.center();
    window
        .show()
        .map_err(|e| format!("显示 TokenScope 窗口失败：{e}"))?;
    window
        .unminimize()
        .map_err(|e| format!("还原 TokenScope 窗口失败：{e}"))?;
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    let _ = window.set_always_on_top(false);
    Ok(())
}

pub(crate) fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "打开 TokenScope", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 TokenScope", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &separator, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("missing default tray icon")?;
    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let handle = tray.app_handle().clone();
                let _ = show_or_create_main_window(&handle);
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                let handle = app.clone();
                let _ = show_or_create_main_window(&handle);
            }
            "quit" => {
                app.state::<AppLifecycle>()
                    .quit_requested
                    .store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}
