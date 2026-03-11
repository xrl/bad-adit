use crate::stats::TunnelStats;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Child;
use tokio::sync::watch;

pub struct ProxyListener {
    shutdown_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
    privileged_child: Option<Child>,
}

impl ProxyListener {
    pub async fn start(
        local_port: u16,
        ephemeral_port: u16,
        stats: Arc<TunnelStats>,
    ) -> Result<Self, String> {
        // Try normal bind first
        match TcpListener::bind(format!("127.0.0.1:{}", local_port)).await {
            Ok(listener) => {
                let (shutdown_tx, shutdown_rx) = watch::channel(false);
                let task = tokio::spawn(accept_loop(listener, ephemeral_port, stats, shutdown_rx));
                Ok(Self {
                    shutdown_tx,
                    task,
                    privileged_child: None,
                })
            }
            Err(e) if local_port < 1024 && is_permission_error(&e) => {
                // Privileged port — use osascript for macOS sudo prompt
                #[cfg(target_os = "macos")]
                {
                    Self::start_privileged(local_port, ephemeral_port, stats).await
                }
                #[cfg(not(target_os = "macos"))]
                {
                    Err(format!(
                        "Port {} requires root privileges: {}",
                        local_port, e
                    ))
                }
            }
            Err(e) => Err(format!("Failed to bind local port {}: {}", local_port, e)),
        }
    }

    #[cfg(target_os = "macos")]
    async fn start_privileged(
        local_port: u16,
        ephemeral_port: u16,
        stats: Arc<TunnelStats>,
    ) -> Result<Self, String> {
        use tokio::process::Command;

        // Start a stats-tracking proxy on a random port that forwards to the SSH tunnel
        let stats_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| format!("Failed to bind stats proxy: {}", e))?;
        let stats_port = stats_listener
            .local_addr()
            .map_err(|e| format!("Failed to get stats proxy addr: {}", e))?
            .port();

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(accept_loop(
            stats_listener,
            ephemeral_port,
            stats,
            shutdown_rx,
        ));

        // Re-exec ourselves as root via osascript to bind the privileged port
        let exe = std::env::current_exe()
            .map_err(|e| format!("Failed to get current exe path: {}", e))?;
        let exe_str = exe.to_string_lossy();

        let parent_pid = std::process::id();
        let shell_cmd = format!(
            "{} --privileged-forwarder {} {} {}",
            exe_str, local_port, stats_port, parent_pid
        );

        let applescript = format!(
            r#"do shell script "{}" with administrator privileges"#,
            shell_cmd.replace('\\', "\\\\").replace('"', "\\\"")
        );

        log::info!(
            "Requesting admin privileges for port {} (forwarding via stats proxy on {})",
            local_port,
            stats_port
        );

        let child = Command::new("osascript")
            .arg("-e")
            .arg(&applescript)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to launch osascript for privileged port: {}", e))?;

        // Allow up to 60 seconds for the user to approve the admin dialog
        match wait_for_port(local_port, 120, 500).await {
            Ok(()) => {}
            Err(e) => {
                let _ = shutdown_tx.send(true);
                return Err(format!(
                    "Privileged proxy on port {} did not start (authorization may have been denied): {}",
                    local_port, e
                ));
            }
        }

        Ok(Self {
            shutdown_tx,
            task,
            privileged_child: Some(child),
        })
    }

    pub async fn stop(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(mut child) = self.privileged_child.take() {
            let _ = child.kill().await;
        }
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.task).await;
    }
}

fn is_permission_error(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied
        || e.raw_os_error() == Some(13) // EACCES
        || e.raw_os_error() == Some(1) // EPERM
}

async fn wait_for_port(port: u16, retries: u32, delay_ms: u64) -> Result<(), String> {
    for i in 0..retries {
        match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(_) => return Ok(()),
            Err(_) if i < retries - 1 => {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
            Err(e) => {
                return Err(format!("Port {} not ready after {} retries: {}", port, retries, e));
            }
        }
    }
    Ok(())
}

async fn accept_loop(
    listener: TcpListener,
    ephemeral_port: u16,
    stats: Arc<TunnelStats>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((client_stream, _addr)) => {
                        let stats = Arc::clone(&stats);
                        tokio::spawn(handle_connection(client_stream, ephemeral_port, stats));
                    }
                    Err(e) => {
                        log::error!("Accept error: {}", e);
                        break;
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                break;
            }
        }
    }
}

async fn handle_connection(client_stream: TcpStream, ephemeral_port: u16, stats: Arc<TunnelStats>) {
    stats.connection_opened();

    let result = async {
        let ssh_stream = TcpStream::connect(format!("127.0.0.1:{}", ephemeral_port))
            .await
            .map_err(|e| format!("Failed to connect to SSH tunnel port: {}", e))?;

        relay(client_stream, ssh_stream, &stats).await
    }
    .await;

    if let Err(e) = result {
        log::debug!("Connection error: {}", e);
    }

    stats.connection_closed();
}

async fn relay(
    mut client: TcpStream,
    mut ssh: TcpStream,
    stats: &Arc<TunnelStats>,
) -> Result<(), String> {
    let (mut client_read, mut client_write) = client.split();
    let (mut ssh_read, mut ssh_write) = ssh.split();

    let stats_up = Arc::clone(stats);
    let stats_down = Arc::clone(stats);

    let upload = async move {
        let mut buf = [0u8; 8192];
        loop {
            let n = client_read
                .read(&mut buf)
                .await
                .map_err(|e| e.to_string())?;
            if n == 0 {
                let _ = ssh_write.shutdown().await;
                break;
            }
            stats_up.record_upload(n as u64);
            ssh_write
                .write_all(&buf[..n])
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    };

    let download = async move {
        let mut buf = [0u8; 8192];
        loop {
            let n = ssh_read.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 {
                let _ = client_write.shutdown().await;
                break;
            }
            stats_down.record_download(n as u64);
            client_write
                .write_all(&buf[..n])
                .await
                .map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    };

    let (r1, r2) = tokio::join!(upload, download);
    r1.and(r2)
}

/// Minimal TCP forwarder for privileged port mode.
/// Runs as root via osascript, binds the privileged port,
/// and forwards connections to the stats proxy port.
pub fn run_privileged_forwarder(local_port: u16, target_port: u16, parent_pid: u32) {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    // Handle SIGTERM gracefully
    unsafe {
        libc::signal(libc::SIGTERM, libc::SIG_DFL);
    }

    // Watchdog: exit when parent process dies (e.g. cargo tauri dev restart)
    thread::spawn(move || loop {
        thread::sleep(std::time::Duration::from_secs(2));
        // kill(pid, 0) checks if process exists without sending a signal
        let alive = unsafe { libc::kill(parent_pid as i32, 0) };
        if alive != 0 {
            eprintln!("Parent process {} exited, shutting down forwarder", parent_pid);
            std::process::exit(0);
        }
    });

    let listener = TcpListener::bind(format!("127.0.0.1:{}", local_port))
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind port {}: {}", local_port, e);
            std::process::exit(1);
        });

    for stream in listener.incoming() {
        match stream {
            Ok(client) => {
                thread::spawn(move || {
                    if let Err(e) = forward_connection(client, target_port) {
                        eprintln!("Forward error: {}", e);
                    }
                });
            }
            Err(e) => {
                eprintln!("Accept error: {}", e);
            }
        }
    }

    fn forward_connection(
        mut client: TcpStream,
        target_port: u16,
    ) -> Result<(), std::io::Error> {
        let mut target = TcpStream::connect(format!("127.0.0.1:{}", target_port))?;
        let mut client_clone = client.try_clone()?;
        let mut target_clone = target.try_clone()?;

        let t1 = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match client.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if target.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = target.shutdown(std::net::Shutdown::Write);
        });

        let t2 = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match target_clone.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if client_clone.write_all(&buf[..n]).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = client_clone.shutdown(std::net::Shutdown::Write);
        });

        let _ = t1.join();
        let _ = t2.join();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_counting() {
        let stats = Arc::new(TunnelStats::new());

        // Simulate what the relay does
        stats.connection_opened();
        stats.record_upload(100);
        stats.record_upload(200);
        stats.record_download(50);

        let snap = stats.snapshot();
        assert_eq!(snap.bytes_uploaded, 300);
        assert_eq!(snap.bytes_downloaded, 50);
        assert_eq!(snap.connections_open, 1);
        assert_eq!(snap.connections_total, 1);

        stats.connection_closed();
        let snap = stats.snapshot();
        assert_eq!(snap.connections_open, 0);
    }

    #[tokio::test]
    async fn relay_with_real_tcp() {
        // Start a simple echo server
        let echo_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_port = echo_listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            while let Ok((mut stream, _)) = echo_listener.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = stream.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });

        // Start our proxy
        let stats = Arc::new(TunnelStats::new());
        let proxy = ProxyListener::start(0, echo_port, Arc::clone(&stats)).await;
        // Port 0 won't work for ProxyListener since it binds a specific port.
        // Let's use a real port instead.
        drop(proxy);

        // Allocate a free port for the proxy
        let proxy_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        drop(proxy_listener);

        let proxy = ProxyListener::start(proxy_port, echo_port, Arc::clone(&stats))
            .await
            .unwrap();

        // Connect through the proxy
        let mut client = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", proxy_port))
            .await
            .unwrap();

        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        client.write_all(b"hello world").await.unwrap();
        client.shutdown().await.unwrap();

        let mut buf = vec![0u8; 1024];
        let n = client.read(&mut buf).await.unwrap();
        assert_eq!(&buf[..n], b"hello world");

        // Give stats a moment to update
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let snap = stats.snapshot();
        assert_eq!(snap.bytes_uploaded, 11);
        assert_eq!(snap.bytes_downloaded, 11);
        assert_eq!(snap.connections_total, 1);

        proxy.stop().await;
    }
}
