use crate::config::{ConfigStore, TunnelConfig};
use crate::error_log::ErrorLog;
use crate::proxy::ProxyListener;
use crate::ssh::{self, SshProcess};
use crate::stats::{StatsSnapshot, TunnelStats};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[allow(dead_code)]
pub enum TunnelState {
    Stopped,
    Starting,
    Running,
    Reconnecting,
    Error(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct TunnelStatus {
    pub id: String,
    pub name: String,
    pub state: TunnelState,
    pub stats: Option<StatsSnapshot>,
    pub local_port: u16,
    pub target_host: String,
    pub target_port: u16,
}

#[allow(dead_code)]
pub struct RunningTunnel {
    pub config: TunnelConfig,
    pub state: TunnelState,
    pub stats: Arc<TunnelStats>,
    pub proxy: Option<ProxyListener>,
    pub ssh: Option<SshProcess>,
    pub log_lines: Vec<String>,
    pub log_rx: Option<mpsc::UnboundedReceiver<String>>,
    pub reconnect_cancel: Option<tokio::sync::watch::Sender<bool>>,
}

pub struct TunnelManager(pub Mutex<TunnelManagerInner>);

pub struct TunnelManagerInner {
    pub config_store: ConfigStore,
    pub tunnels: HashMap<String, RunningTunnel>,
}

impl Default for TunnelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelManager {
    pub fn new() -> Self {
        Self(Mutex::new(TunnelManagerInner {
            config_store: ConfigStore::new(),
            tunnels: HashMap::new(),
        }))
    }
}

impl TunnelManagerInner {
    pub async fn start_tunnel(&mut self, config: &TunnelConfig) -> Result<(), String> {
        let ephemeral_port = ssh::allocate_ephemeral_port()?;

        let (log_tx, log_rx) = mpsc::unbounded_channel();
        let ssh = SshProcess::spawn(config, ephemeral_port, log_tx).await?;

        // Wait briefly for SSH to set up the port forward
        wait_for_port_ready(ephemeral_port, 10, 200).await?;

        let stats = Arc::new(TunnelStats::new());
        let proxy =
            ProxyListener::start(config.local_port, ephemeral_port, Arc::clone(&stats)).await?;

        let tunnel = RunningTunnel {
            config: config.clone(),
            state: TunnelState::Running,
            stats,
            proxy: Some(proxy),
            ssh: Some(ssh),
            log_lines: Vec::new(),
            log_rx: Some(log_rx),
            reconnect_cancel: None,
        };

        self.tunnels.insert(config.id.clone(), tunnel);
        Ok(())
    }

    pub async fn stop_tunnel(&mut self, id: &str) -> Result<(), String> {
        let tunnel = self
            .tunnels
            .get_mut(id)
            .ok_or_else(|| format!("Tunnel {} not found", id))?;

        // Cancel reconnect watcher if running
        if let Some(cancel) = tunnel.reconnect_cancel.take() {
            let _ = cancel.send(true);
        }

        if let Some(proxy) = tunnel.proxy.take() {
            proxy.stop().await;
        }

        if let Some(mut ssh) = tunnel.ssh.take() {
            ssh.kill().await?;
        }

        tunnel.stats.reset();
        tunnel.state = TunnelState::Stopped;

        Ok(())
    }

    pub async fn stop_all_tunnels(&mut self) {
        let ids: Vec<String> = self.tunnels.keys().cloned().collect();
        for id in ids {
            if let Err(e) = self.stop_tunnel(&id).await {
                log::error!("Failed to stop tunnel {} during shutdown: {}", id, e);
            }
        }
    }

    pub async fn restart_tunnel(
        &mut self,
        id: &str,
        new_config: Option<TunnelConfig>,
    ) -> Result<(), String> {
        self.stop_tunnel(id).await?;

        let config = if let Some(c) = new_config {
            c
        } else {
            self.tunnels
                .get(id)
                .map(|t| t.config.clone())
                .ok_or_else(|| format!("Tunnel {} not found", id))?
        };

        self.start_tunnel(&config).await
    }

    pub fn get_all_status(&self) -> Vec<TunnelStatus> {
        let configs = self.config_store.load();
        configs
            .iter()
            .map(|c| {
                if let Some(tunnel) = self.tunnels.get(&c.id) {
                    let stats = if tunnel.state == TunnelState::Running {
                        Some(tunnel.stats.snapshot())
                    } else {
                        None
                    };
                    TunnelStatus {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        state: tunnel.state.clone(),
                        stats,
                        local_port: c.local_port,
                        target_host: c.target_host.clone(),
                        target_port: c.target_port,
                    }
                } else {
                    TunnelStatus {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        state: TunnelState::Stopped,
                        stats: None,
                        local_port: c.local_port,
                        target_host: c.target_host.clone(),
                        target_port: c.target_port,
                    }
                }
            })
            .collect()
    }

    /// Check all running tunnels for dead SSH processes.
    /// Returns a list of (tunnel_name, error_message) for tunnels that died.
    pub fn check_ssh_health(&mut self, error_log: &ErrorLog) -> Vec<String> {
        let mut dead_tunnels = Vec::new();

        for (id, tunnel) in &mut self.tunnels {
            if tunnel.state != TunnelState::Running {
                continue;
            }
            if let Some(ref mut ssh) = tunnel.ssh {
                if let Some(status) = ssh.try_wait() {
                    let name = tunnel.config.name.clone();
                    let msg = if status.success() {
                        "SSH process exited unexpectedly".to_string()
                    } else {
                        format!("SSH process exited with {}", status)
                    };
                    log::warn!("Tunnel '{}' ({}): {}", name, id, msg);
                    error_log.error(msg, Some(name.clone()));
                    tunnel.state = TunnelState::Error("SSH process died".to_string());
                    dead_tunnels.push(name);
                }
            }
        }

        dead_tunnels
    }

    pub fn get_tunnel_stats(&self, id: &str) -> Result<StatsSnapshot, String> {
        self.tunnels
            .get(id)
            .map(|t| t.stats.snapshot())
            .ok_or_else(|| format!("Tunnel {} not running", id))
    }
}

async fn wait_for_port_ready(port: u16, retries: u32, delay_ms: u64) -> Result<(), String> {
    for i in 0..retries {
        match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(_) => return Ok(()),
            Err(_) if i < retries - 1 => {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            Err(e) => {
                return Err(format!(
                    "SSH tunnel port {} not ready after {} retries: {}",
                    port, retries, e
                ));
            }
        }
    }
    Ok(())
}
