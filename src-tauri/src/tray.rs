use crate::error_log::ErrorLog;
use crate::format::format_bytes;
use crate::tunnel::{TunnelManager, TunnelState, TunnelStatus};
use std::sync::{Arc, Mutex};
use tauri::menu::{MenuBuilder, MenuItem, MenuItemBuilder};
use tauri::{AppHandle, Manager};

/// Holds references to menu items that can be updated in-place.
struct MenuState {
    /// Tunnel item handles keyed by position, matching tunnel_ids order
    tunnel_items: Vec<MenuItem<tauri::Wry>>,
    /// IDs of tunnels in menu order — used to detect structural changes
    tunnel_ids: Vec<String>,
    /// Whether an error item is currently shown
    has_error_item: bool,
    /// Handle to the error menu item (if present)
    error_item: Option<MenuItem<tauri::Wry>>,
}

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // Build initial empty menu
    let menu_state = build_menu_from_statuses(&handle, &[], 0)?;
    let menu_state = Arc::new(Mutex::new(menu_state));

    // Set up periodic refresh — gather statuses on async task,
    // but update menu on the main thread to avoid race with macOS event processing
    let refresh_handle = handle.clone();
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
        let mut last_labels: Vec<String> = Vec::new();
        let mut last_error_count: usize = 0;
        loop {
            interval.tick().await;
            let statuses: Vec<TunnelStatus> = {
                let manager = refresh_handle.state::<TunnelManager>();
                let inner = manager.0.lock().await;
                inner.get_all_status()
            };
            let error_count = {
                let log = refresh_handle.state::<ErrorLog>();
                log.error_count()
            };

            // Skip if nothing changed
            let current_labels: Vec<String> = statuses
                .iter()
                .map(|s| format_status_label(s))
                .collect();
            if current_labels == last_labels && error_count == last_error_count {
                continue;
            }
            last_labels = current_labels.clone();
            last_error_count = error_count;

            // Check if structure changed (tunnel IDs or error item presence)
            let current_ids: Vec<String> = statuses.iter().map(|s| s.id.clone()).collect();
            let has_errors = error_count > 0;
            let needs_rebuild = {
                let state = menu_state.lock().unwrap();
                state.tunnel_ids != current_ids || state.has_error_item != has_errors
            };

            if needs_rebuild {
                // Structural change — full rebuild
                let handle = refresh_handle.clone();
                let menu_state = Arc::clone(&menu_state);
                let _ = refresh_handle.run_on_main_thread(move || {
                    match build_menu_from_statuses(&handle, &statuses, error_count) {
                        Ok(new_state) => {
                            *menu_state.lock().unwrap() = new_state;
                        }
                        Err(e) => {
                            log::error!("Failed to rebuild tray menu: {}", e);
                        }
                    }
                });
            } else {
                // Only labels changed — update text in-place
                let menu_state = Arc::clone(&menu_state);
                let _ = refresh_handle.run_on_main_thread(move || {
                    let state = menu_state.lock().unwrap();
                    for (i, label) in current_labels.iter().enumerate() {
                        if let Some(item) = state.tunnel_items.get(i) {
                            let _ = item.set_text(label);
                        }
                    }
                    if let Some(ref error_item) = state.error_item {
                        let error_label = format!(
                            "⚠ {} error{}",
                            error_count,
                            if error_count == 1 { "" } else { "s" }
                        );
                        let _ = error_item.set_text(error_label);
                    }
                });
            }
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
    error_count: usize,
) -> Result<MenuState, Box<dyn std::error::Error>> {
    let mut menu_builder = MenuBuilder::new(handle);

    let title = MenuItemBuilder::with_id("title", "Bad Adit")
        .enabled(false)
        .build(handle)?;
    menu_builder = menu_builder.item(&title).separator();

    let mut tunnel_items = Vec::new();
    let mut tunnel_ids = Vec::new();
    for status in statuses {
        let label = format_status_label(status);
        let item =
            MenuItemBuilder::with_id(format!("tunnel:{}", status.id), label).build(handle)?;
        menu_builder = menu_builder.item(&item);
        tunnel_items.push(item);
        tunnel_ids.push(status.id.clone());
    }

    menu_builder = menu_builder.separator();

    let mut error_item_handle = None;
    if error_count > 0 {
        let error_label = format!("⚠ {} error{}", error_count, if error_count == 1 { "" } else { "s" });
        let error_item =
            MenuItemBuilder::with_id("show_errors", error_label).build(handle)?;
        menu_builder = menu_builder.item(&error_item);
        error_item_handle = Some(error_item);
    }

    let edit_item =
        MenuItemBuilder::with_id("edit_configs", "Edit Tunnel Configurations...").build(handle)?;
    menu_builder = menu_builder.item(&edit_item);

    let quit_item = MenuItemBuilder::with_id("quit", "Quit Bad Adit").build(handle)?;
    menu_builder = menu_builder.separator().item(&quit_item);

    let menu = menu_builder.build()?;

    if let Some(tray) = handle.tray_by_id("main") {
        tray.set_menu(Some(menu))?;
    }

    Ok(MenuState {
        tunnel_items,
        tunnel_ids,
        has_error_item: error_count > 0,
        error_item: error_item_handle,
    })
}

pub fn handle_menu_event(app: &AppHandle, id: &str) {
    if id == "quit" {
        // Stop all tunnels before exiting
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            {
                let manager = handle.state::<TunnelManager>();
                let mut inner = manager.0.lock().await;
                inner.stop_all_tunnels().await;
            }
            std::process::exit(0);
        });
        return;
    }

    if id == "edit_configs" || id == "show_errors" {
        if let Some(window) = app.get_webview_window("main") {
            if window.is_minimized().unwrap_or(false) {
                let _ = window.unminimize();
            }
            let _ = window.show();
            let _ = window.set_focus();
            // Navigate to console view if showing errors
            if id == "show_errors" {
                let _ = window.eval("window.__navigateTo && window.__navigateTo('console')");
            }
        }
        return;
    }

    if let Some(tunnel_id) = id.strip_prefix("tunnel:") {
        let tunnel_id = tunnel_id.to_string();
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            let manager = handle.state::<TunnelManager>();
            let error_log = handle.state::<ErrorLog>();

            let (is_running, tunnel_name) = {
                let inner = manager.0.lock().await;
                let running = inner
                    .tunnels
                    .get(&tunnel_id)
                    .is_some_and(|t| t.state == TunnelState::Running);
                let name = inner
                    .tunnels
                    .get(&tunnel_id)
                    .map(|t| t.config.name.clone());
                (running, name)
            };

            if is_running {
                let mut inner = manager.0.lock().await;
                if let Err(e) = inner.stop_tunnel(&tunnel_id).await {
                    log::error!("Failed to stop tunnel: {}", e);
                    error_log.error(e, tunnel_name);
                }
            } else {
                let config = {
                    let inner = manager.0.lock().await;
                    let configs = inner.config_store.load();
                    configs.into_iter().find(|c| c.id == tunnel_id)
                };
                if let Some(config) = config {
                    let name = config.name.clone();
                    let mut inner = manager.0.lock().await;
                    if let Err(e) = inner.start_tunnel(&config).await {
                        log::error!("Failed to start tunnel: {}", e);
                        error_log.error(e, Some(name));
                    }
                }
            }
        });
    }
}
