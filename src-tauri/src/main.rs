#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod error_log;
mod format;
mod proxy;
mod ssh;
mod stats;
mod tray;
mod tunnel;

use error_log::ErrorLog;
use tauri::Manager;
use tunnel::TunnelManager;

fn main() {
    // Check for privileged forwarder mode before initializing Tauri
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 4 && args[1] == "--privileged-forwarder" {
        let local_port: u16 = args[2].parse().expect("invalid local port");
        let target_port: u16 = args[3].parse().expect("invalid target port");
        proxy::run_privileged_forwarder(local_port, target_port);
        return;
    }

    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(TunnelManager::new())
        .manage(ErrorLog::new())
        .setup(|app| {
            // Hide from dock (LSUIElement only works in bundled .app builds)
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::setup_tray(app)?;

            // Hide window on close instead of destroying it
            let window = app.get_webview_window("main").unwrap();
            let window_clone = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window_clone.hide();
                }
            });

            Ok(())
        })
        .on_menu_event(|app, event| {
            tray::handle_menu_event(app, event.id().as_ref());
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_tunnels,
            commands::add_tunnel,
            commands::update_tunnel,
            commands::remove_tunnel,
            commands::start_tunnel,
            commands::stop_tunnel,
            commands::restart_tunnel,
            commands::get_tunnel_stats,
            commands::get_all_tunnel_status,
            commands::get_home_dir,
            commands::get_error_log,
            commands::get_error_count,
            commands::clear_error_log,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // Keep the app running when all windows are closed (tray stays alive)
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
            }
        });
}
