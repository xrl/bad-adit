use crate::format::format_bytes;
use crate::tunnel::{TunnelManager, TunnelState, TunnelStatus};
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::{AppHandle, Manager};

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // Build initial empty menu
    build_menu_from_statuses(&handle, &[])?;

    // Set up periodic refresh — gather statuses on async task,
    // but rebuild menu on the main thread to avoid race with macOS event processing
    let refresh_handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        let mut last_labels: Vec<String> = Vec::new();
        loop {
            interval.tick().await;
            let statuses: Vec<TunnelStatus> = {
                let manager = refresh_handle.state::<TunnelManager>();
                let inner = manager.0.lock().await;
                inner.get_all_status()
            };

            // Only rebuild if something changed
            let current_labels: Vec<String> = statuses
                .iter()
                .map(|s| format_status_label(s))
                .collect();
            if current_labels == last_labels {
                continue;
            }
            last_labels = current_labels;

            let handle = refresh_handle.clone();
            let _ = refresh_handle.run_on_main_thread(move || {
                if let Err(e) = build_menu_from_statuses(&handle, &statuses) {
                    log::error!("Failed to rebuild tray menu: {}", e);
                }
            });
        }
    });

    Ok(())
}

fn format_status_label(status: &TunnelStatus) -> String {
    let icon = match &status.state {
        TunnelState::Running => "●",
        TunnelState::Starting | TunnelState::Reconnecting => "◐",
        _ => "○",
    };

    if let Some(ref stats) = status.stats {
        format!(
            "{} {}  ↑ {}  ↓ {}",
            icon,
            status.name,
            format_bytes(stats.bytes_uploaded),
            format_bytes(stats.bytes_downloaded)
        )
    } else {
        format!("{} {}", icon, status.name)
    }
}

fn build_menu_from_statuses(
    handle: &AppHandle,
    statuses: &[TunnelStatus],
) -> Result<(), Box<dyn std::error::Error>> {
    let mut menu_builder = MenuBuilder::new(handle);

    let title = MenuItemBuilder::with_id("title", "Bad Adit")
        .enabled(false)
        .build(handle)?;
    menu_builder = menu_builder.item(&title).separator();

    for status in statuses {
        let label = format_status_label(status);
        let item =
            MenuItemBuilder::with_id(format!("tunnel:{}", status.id), label).build(handle)?;
        menu_builder = menu_builder.item(&item);
    }

    menu_builder = menu_builder.separator();

    let edit_item =
        MenuItemBuilder::with_id("edit_configs", "Edit Tunnel Configurations...").build(handle)?;
    menu_builder = menu_builder.item(&edit_item);

    let menu = menu_builder.build()?;

    if let Some(tray) = handle.tray_by_id("main") {
        tray.set_menu(Some(menu))?;
    }

    Ok(())
}

pub fn handle_menu_event(app: &AppHandle, id: &str) {
    if id == "edit_configs" {
        if let Some(window) = app.get_webview_window("main") {
            // Unminimize if needed, then show and focus
            if window.is_minimized().unwrap_or(false) {
                let _ = window.unminimize();
            }
            let _ = window.show();
            let _ = window.set_focus();
        }
        return;
    }

    if let Some(tunnel_id) = id.strip_prefix("tunnel:") {
        let tunnel_id = tunnel_id.to_string();
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let manager = handle.state::<TunnelManager>();

            let is_running = {
                let inner = manager.0.lock().await;
                inner
                    .tunnels
                    .get(&tunnel_id)
                    .is_some_and(|t| t.state == TunnelState::Running)
            };

            if is_running {
                let mut inner = manager.0.lock().await;
                if let Err(e) = inner.stop_tunnel(&tunnel_id).await {
                    log::error!("Failed to stop tunnel: {}", e);
                }
            } else {
                let config = {
                    let inner = manager.0.lock().await;
                    let configs = inner.config_store.load();
                    configs.into_iter().find(|c| c.id == tunnel_id)
                };
                if let Some(config) = config {
                    let mut inner = manager.0.lock().await;
                    if let Err(e) = inner.start_tunnel(&config).await {
                        log::error!("Failed to start tunnel: {}", e);
                    }
                }
            }
        });
    }
}
