//! Desktop shell: packaged Agent supervisor, tray lifecycle and read-only stdio queries.
//! The WebView never opens SQLite or reads source logs.
mod agent;
mod agent_customizations;
mod ai_assistant;
mod dashboard;
mod diagnostics;
mod hub;
mod instruction_sync;
mod lifecycle;
mod model_pricing;
mod paths;
mod session_activity;
mod session_history;
mod settings;
mod skill_sources;
mod state;
mod webdav;

use agent::{start_agent_inner, stop_agent_inner};
use dashboard::spawn_dashboard_watch;
use lifecycle::{build_tray, show_or_create_main_window};
use state::{AgentProcess, AppLifecycle, DashboardWatch};
use std::{env, sync::atomic::Ordering, thread, time::Duration};
use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(AgentProcess::default())
        .manage(AppLifecycle::default())
        .manage(DashboardWatch::default())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .setup(|app| {
            build_tray(app)?;
            let process = app.state::<AgentProcess>();
            let time_zone = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
            if let Err(error) = start_agent_inner(app.handle(), &time_zone, process.inner()) {
                eprintln!("TokenScope Agent startup warning: {error}");
            }
            spawn_dashboard_watch(app.handle().clone());
            instruction_sync::start_polling(app.handle().clone());
            // Login autostart runs the collector and tray only. No WebView is
            // created until the user chooses Open, keeping idle memory lower.
            let is_bg = env::args().any(|arg| arg == "--background");
            if !is_bg {
                if let Err(error) = show_or_create_main_window(app.handle()) {
                    eprintln!("TokenScope window startup warning: {error}");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Destroy the WebView instead of merely hiding it. The tray
                // process and Agent remain alive; opening the app recreates it.
                window
                    .app_handle()
                    .state::<AppLifecycle>()
                    .keep_alive_after_window_close
                    .store(true, Ordering::SeqCst);
                api.prevent_close();
                let app_handle = window.app_handle().clone();
                let _ = window.destroy();
                // Clear a stale keep-alive marker if this platform does not
                // emit ExitRequested when the final WebView is destroyed.
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(1));
                    app_handle
                        .state::<AppLifecycle>()
                        .keep_alive_after_window_close
                        .store(false, Ordering::SeqCst);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            model_pricing::get_model_price_catalog,
            model_pricing::save_model_pricing_preferences,
            agent::start_agent,
            agent::stop_agent,
            agent::get_agent_status,
            agent::refresh_codex_quota_now,
            agent_customizations::list_customizations,
            agent_customizations::read_customization,
            agent_customizations::save_customization,
            agent_customizations::prepare_instruction,
            agent_customizations::delete_instruction,
            agent_customizations::rename_customization,
            agent_customizations::toggle_skill_enabled,
            skill_sources::inspect_skill_source,
            skill_sources::install_skill_source,
            skill_sources::check_skill_update,
            skill_sources::update_skill_source,
            skill_sources::detect_skill_source,
            skill_sources::adopt_skill_source,
            ai_assistant::get_ai_config,
            ai_assistant::save_ai_config,
            ai_assistant::fetch_ai_models,
            ai_assistant::get_skill_summary,
            ai_assistant::generate_skill_summary,
            session_activity::recent_session_activity,
            session_history::get_session_history,
            session_history::save_session_message,
            session_history::remove_source_session,
            dashboard::get_dashboard_snapshot,
            dashboard::get_session_analysis,
            dashboard::delete_session,
            diagnostics::run_diagnostics,
            settings::get_autostart_enabled,
            settings::set_autostart_enabled,
            hub::pair_hub_device,
            hub::list_hub_devices,
            hub::rename_hub_device,
            hub::rotate_hub_device_token,
            hub::disconnect_hub_device,
            instruction_sync::shared_instruction,
            webdav::test_webdav_connection,
            webdav::save_webdav_config,
            webdav::get_webdav_config,
            webdav::disconnect_webdav,
            webdav::list_webdav_devices,
            webdav::sync_webdav_now
        ])
        .build(tauri::generate_context!())
        .expect("TokenScope desktop initialization failed");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            // Only suppress the exit request associated with our own window
            // destruction. OS shutdown and unrelated exit requests are allowed.
            let lifecycle = app_handle.state::<AppLifecycle>();
            if !lifecycle.quit_requested.load(Ordering::SeqCst)
                && lifecycle
                    .keep_alive_after_window_close
                    .swap(false, Ordering::SeqCst)
            {
                api.prevent_exit();
            }
        }
        tauri::RunEvent::Exit => {
            let process = app_handle.state::<AgentProcess>();
            let _ = stop_agent_inner(app_handle, process.inner());
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows: false,
        } => {
            let handle = app_handle.clone();
            thread::spawn(move || {
                if let Err(error) = show_or_create_main_window(&handle) {
                    eprintln!("TokenScope reopen warning: {error}");
                }
            });
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use crate::paths::{rotate_agent_log, shutdown_path, validate_time_zone};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tokenscope-desktop-{n}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn shutdown_marker_is_a_sibling_of_the_database() {
        let path = PathBuf::from("/tmp/tokenscope.db");
        assert_eq!(
            shutdown_path(&path),
            PathBuf::from("/tmp/tokenscope.db.shutdown")
        );
    }

    #[test]
    fn log_rotation_keeps_bounded_generations() {
        let dir = temp_dir();
        let log = dir.join("agent.log");
        fs::write(&log, b"123456789").unwrap();
        rotate_agent_log(&log, 4, 3).unwrap();
        assert!(!log.exists());
        assert_eq!(fs::read(dir.join("agent.log.1")).unwrap(), b"123456789");
        fs::write(&log, b"abcdefgh").unwrap();
        rotate_agent_log(&log, 4, 3).unwrap();
        assert_eq!(fs::read(dir.join("agent.log.1")).unwrap(), b"abcdefgh");
        assert_eq!(fs::read(dir.join("agent.log.2")).unwrap(), b"123456789");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timezone_input_is_bounded_and_rejects_path_characters() {
        assert!(validate_time_zone("Asia/Shanghai").is_ok());
        assert!(validate_time_zone("../../etc/passwd").is_err());
        assert!(validate_time_zone(&"a".repeat(129)).is_err());
    }
}
