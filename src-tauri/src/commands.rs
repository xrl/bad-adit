use crate::config::{self, TunnelConfig};
use crate::error_log::{ErrorLog, LogEntry};
use crate::stats::StatsSnapshot;
use crate::tunnel::{TunnelManager, TunnelStatus};
use std::env;
use tauri::State;

#[tauri::command]
pub async fn get_tunnels(manager: State<'_, TunnelManager>) -> Result<Vec<TunnelConfig>, String> {
    let inner = manager.0.lock().await;
    Ok(inner.config_store.load())
}

#[tauri::command]
pub async fn add_tunnel(
    config: TunnelConfig,
    manager: State<'_, TunnelManager>,
) -> Result<TunnelConfig, String> {
    let inner = manager.0.lock().await;
    let mut configs = inner.config_store.load();

    let mut new_config = config;
    new_config.id = uuid::Uuid::new_v4().to_string();

    config::validate_config(&new_config, &configs)?;

    configs.push(new_config.clone());
    inner.config_store.save(&configs)?;

    Ok(new_config)
}

#[tauri::command]
pub async fn update_tunnel(
    config: TunnelConfig,
    manager: State<'_, TunnelManager>,
) -> Result<TunnelConfig, String> {
    let inner = manager.0.lock().await;
    let mut configs = inner.config_store.load();

    config::validate_config(&config, &configs)?;

    if let Some(existing) = configs.iter_mut().find(|c| c.id == config.id) {
        *existing = config.clone();
    } else {
        return Err("Tunnel not found".to_string());
    }

    inner.config_store.save(&configs)?;
    Ok(config)
}

#[tauri::command]
pub async fn remove_tunnel(id: String, manager: State<'_, TunnelManager>) -> Result<(), String> {
    let mut inner = manager.0.lock().await;

    // Stop the tunnel if running
    if inner.tunnels.contains_key(&id) {
        if let Some(tunnel) = inner.tunnels.get_mut(&id) {
            if let Some(mut ssh) = tunnel.ssh.take() {
                let _ = ssh.kill().await;
            }
            if let Some(proxy) = tunnel.proxy.take() {
                proxy.stop().await;
            }
        }
        inner.tunnels.remove(&id);
    }

    let mut configs = inner.config_store.load();
    configs.retain(|c| c.id != id);
    inner.config_store.save(&configs)?;

    Ok(())
}

#[tauri::command]
pub async fn start_tunnel(
    id: String,
    manager: State<'_, TunnelManager>,
    error_log: State<'_, ErrorLog>,
) -> Result<(), String> {
    let mut inner = manager.0.lock().await;
    let configs = inner.config_store.load();
    let config = configs
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("Tunnel {} not found", id))?;

    let tunnel_name = config.name.clone();
    match inner.start_tunnel(&config).await {
        Ok(()) => {
            error_log.info("Tunnel started".to_string(), Some(tunnel_name));
            Ok(())
        }
        Err(e) => {
            error_log.error(e.clone(), Some(tunnel_name));
            Err(e)
        }
    }
}

#[tauri::command]
pub async fn stop_tunnel(id: String, manager: State<'_, TunnelManager>) -> Result<(), String> {
    let mut inner = manager.0.lock().await;
    inner.stop_tunnel(&id).await
}

#[tauri::command]
pub async fn restart_tunnel(id: String, manager: State<'_, TunnelManager>) -> Result<(), String> {
    let mut inner = manager.0.lock().await;
    let configs = inner.config_store.load();
    let config = configs
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("Tunnel {} not found", id))?;

    inner.restart_tunnel(&id, Some(config)).await
}

#[tauri::command]
pub async fn get_tunnel_stats(
    id: String,
    manager: State<'_, TunnelManager>,
) -> Result<StatsSnapshot, String> {
    let inner = manager.0.lock().await;
    inner.get_tunnel_stats(&id)
}

#[tauri::command]
pub fn get_home_dir() -> Result<String, String> {
    env::var("HOME").map_err(|_| "Could not determine home directory".to_string())
}

#[tauri::command]
pub async fn get_all_tunnel_status(
    manager: State<'_, TunnelManager>,
) -> Result<Vec<TunnelStatus>, String> {
    let inner = manager.0.lock().await;
    Ok(inner.get_all_status())
}

#[tauri::command]
pub fn get_error_log(error_log: State<'_, ErrorLog>) -> Vec<LogEntry> {
    error_log.get_all()
}

#[tauri::command]
pub fn get_error_count(error_log: State<'_, ErrorLog>) -> usize {
    error_log.error_count()
}

#[tauri::command]
pub fn clear_error_log(error_log: State<'_, ErrorLog>) {
    error_log.clear();
}
