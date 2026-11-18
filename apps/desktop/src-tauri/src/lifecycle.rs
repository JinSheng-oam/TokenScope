use crate::{dashboard::spawn_dashboard_warmup, native_splash, settings, state::AppLifecycle};
use std::{sync::atomic::Ordering, thread, time::Duration};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    webview::{PageLoadEvent, WebviewWindowBuilder},
    AppHandle, Manager,
};

pub(crate) fn reveal_window_now(app: &AppHandle) -> Result<(), String> {
    let lifecycle = app.state::<AppLifecycle>();
    lifecycle.main_window_revealed.store(true, Ordering::SeqCst);
    native_splash::close();
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
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

#[tauri::command]
pub(crate) fn reveal_main_window(app: AppHandle) -> Result<(), String> {
    reveal_window_now(&app)
}

pub(crate) fn show_or_create_main_window(app: &AppHandle) -> Result<(), String> {
    if app.get_webview_window("main").is_some() {
        return reveal_window_now(app);
    }

    let lifecycle = app.state::<AppLifecycle>();
    let generation = lifecycle
        .main_window_generation
        .fetch_add(1, Ordering::SeqCst)
        + 1;
    lifecycle
        .main_window_revealed
        .store(false, Ordering::SeqCst);
    let is_dark = settings::resolve_is_dark(app);
    native_splash::show(is_dark);

    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "main")
        .ok_or_else(|| "Tauri 配置中缺少 main 窗口".to_string())?;

    let handle_for_load = app.clone();
    let built = WebviewWindowBuilder::from_config(app, config)
        .and_then(|builder| {
            let builder = builder.background_color(settings::canvas_color(is_dark));
            let builder = if cfg!(windows) {
                builder.visible(false)
            } else {
                builder
            };
            builder
                .on_page_load(move |window, payload| {
                    if !matches!(payload.event(), PageLoadEvent::Finished) {
                        return;
                    }
                    let app_handle = window.app_handle().clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(1500));
                        let lifecycle = app_handle.state::<AppLifecycle>();
                        if lifecycle.main_window_generation.load(Ordering::SeqCst) == generation
                            && !lifecycle.main_window_revealed.load(Ordering::SeqCst)
                        {
                            let _ = reveal_window_now(&app_handle);
                        }
                    });
                })
                .build()
        })
        .map_err(|e| {
            native_splash::close();
            format!("创建 TokenScope 窗口失败：{e}")
        })?;

    if !cfg!(windows) {
        let _ = built.center();
        let _ = built.show();
        let _ = built.unminimize();
        let _ = built.set_focus();
    } else {
        // Safety deadline in case WebView2 navigation stalls before PageLoadEvent::Finished.
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(12));
            let lifecycle = handle_for_load.state::<AppLifecycle>();
            if lifecycle.main_window_generation.load(Ordering::SeqCst) == generation
                && !lifecycle.main_window_revealed.load(Ordering::SeqCst)
            {
                let _ = reveal_window_now(&handle_for_load);
            }
        });
    }

    spawn_dashboard_warmup(app.clone());
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
                native_splash::close();
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
