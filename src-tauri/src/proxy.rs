use crate::stats::TunnelStats;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

pub struct ProxyListener {
    shutdown_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl ProxyListener {
    pub async fn start(
        local_port: u16,
        ephemeral_port: u16,
        stats: Arc<TunnelStats>,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", local_port))
            .await
            .map_err(|e| format!("Failed to bind local port {}: {}", local_port, e))?;

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn(accept_loop(listener, ephemeral_port, stats, shutdown_rx));

        Ok(Self { shutdown_tx, task })
    }

    pub async fn stop(self) {
        let _ = self.shutdown_tx.send(true);
        // Give in-flight connections a moment to drain
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.task).await;
    }
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
