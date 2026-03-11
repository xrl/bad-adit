#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod config;
mod format;
mod proxy;
mod ssh;
mod stats;
mod tray;
mod tunnel;

use tunnel::TunnelManager;

fn main() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(TunnelManager::new())
        .setup(|app| {
            tray::setup_tray(app)?;
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
