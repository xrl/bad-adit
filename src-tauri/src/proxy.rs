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
        _stats: Arc<TunnelStats>,
    ) -> Result<Self, String> {
        use tokio::process::Command;

        // Spawn a privileged TCP forwarder via osascript
        // The forwarder listens on the privileged port and forwards to ephemeral
        let python_script = format!(
            r#"import socket,threading,sys,signal,os
signal.signal(signal.SIGTERM, lambda *a: os._exit(0))
s=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
s.setsockopt(socket.SOL_SOCKET,socket.SO_REUSEADDR,1)
s.bind(('127.0.0.1',{local_port}))
s.listen(128)
def fwd(a,b):
 try:
  while True:
   d=a.recv(8192)
   if not d:break
   b.sendall(d)
 except:pass
 finally:a.close();b.close()
while True:
 c,_=s.accept()
 try:
  r=socket.socket()
  r.connect(('127.0.0.1',{ephemeral_port}))
  threading.Thread(target=fwd,args=(c,r),daemon=True).start()
  threading.Thread(target=fwd,args=(r,c),daemon=True).start()
 except:c.close()"#
        );

        let shell_cmd = format!(
            "python3 -c '{}'",
            python_script.replace('\'', "'\"'\"'")
        );

        let applescript = format!(
            r#"do shell script "{}" with administrator privileges"#,
            shell_cmd.replace('\\', "\\\\").replace('"', "\\\"")
        );

        log::info!(
            "Requesting admin privileges for port {} (forwarding to {})",
            local_port,
            ephemeral_port
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

        // Wait for the privileged proxy to start listening
        match wait_for_port(local_port, 15, 200).await {
            Ok(()) => {}
            Err(e) => {
                return Err(format!(
                    "Privileged proxy on port {} did not start (authorization may have been denied): {}",
                    local_port, e
                ));
            }
        }

        // Now connect our stats-tracking proxy to the privileged forwarder
        // by listening on a separate internal port and piping through
        // Actually, we can just track stats by intercepting at the ephemeral level
        // For now, the privileged forwarder handles the traffic directly

        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(async {});

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
