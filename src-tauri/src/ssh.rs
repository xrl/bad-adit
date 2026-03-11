use crate::config::TunnelConfig;
use std::process::ExitStatus;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

#[allow(dead_code)]
pub struct SshProcess {
    child: Child,
    pub ephemeral_port: u16,
    _stderr_task: tokio::task::JoinHandle<()>,
}

impl SshProcess {
    pub async fn spawn(
        config: &TunnelConfig,
        ephemeral_port: u16,
        log_tx: mpsc::UnboundedSender<String>,
    ) -> Result<Self, String> {
        let args = build_ssh_args(config, ephemeral_port);

        log::info!(
            "Spawning SSH for tunnel '{}': ssh {}",
            config.name,
            args.join(" ")
        );

        let mut child = Command::new("ssh")
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn ssh: {e}"))?;

        let stderr = child.stderr.take().unwrap();
        let tunnel_name = config.name.clone();
        let stderr_task = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                log::debug!("[{}] {}", tunnel_name, line);
                let _ = log_tx.send(line);
            }
        });

        Ok(Self {
            child,
            ephemeral_port,
            _stderr_task: stderr_task,
        })
    }

    pub async fn kill(&mut self) -> Result<(), String> {
        let _ = self.child.kill().await;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn wait_for_exit(&mut self) -> Result<ExitStatus, String> {
        self.child
            .wait()
            .await
            .map_err(|e| format!("Failed to wait for ssh: {e}"))
    }
}

pub fn allocate_ephemeral_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to allocate ephemeral port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get local addr: {e}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn build_ssh_args(config: &TunnelConfig, ephemeral_port: u16) -> Vec<String> {
    let mut args = vec![
        "-N".to_string(),
        "-L".to_string(),
        format!(
            "{}:{}:{}",
            ephemeral_port, config.target_host, config.target_port
        ),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-o".to_string(),
        "ExitOnForwardFailure=yes".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=3".to_string(),
    ];

    if !config.ssh_key_path.is_empty() {
        args.push("-i".to_string());
        args.push(config.ssh_key_path.clone());
    }

    args.push(format!("{}@{}", config.ssh_user, config.ssh_host));

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> TunnelConfig {
        TunnelConfig {
            id: "test".to_string(),
            name: "Test".to_string(),
            ssh_host: "example.com".to_string(),
            ssh_user: "user".to_string(),
            ssh_key_path: "/path/to/key".to_string(),
            target_host: "localhost".to_string(),
            target_port: 5432,
            local_port: 15432,
            auto_reconnect: false,
        }
    }

    #[test]
    fn ssh_args_construction() {
        let config = test_config();
        let args = build_ssh_args(&config, 54321);
        assert!(args.contains(&"-N".to_string()));
        assert!(args.contains(&"54321:localhost:5432".to_string()));
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/path/to/key".to_string()));
        assert!(args.contains(&"user@example.com".to_string()));
    }

    #[test]
    fn ssh_args_without_key() {
        let mut config = test_config();
        config.ssh_key_path = String::new();
        let args = build_ssh_args(&config, 54321);
        assert!(!args.contains(&"-i".to_string()));
    }

    #[test]
    fn ephemeral_port_allocator() {
        let port = allocate_ephemeral_port().unwrap();
        assert!(port > 0);

        // Allocate a second one — should be different
        let port2 = allocate_ephemeral_port().unwrap();
        // They *could* be the same in theory but almost never are
        assert!(port2 > 0);
    }
}
