//! Integration tests for Bad Adit tunnel lifecycle.
//!
//! These tests require a real SSH environment. Set these env vars:
//! - TEST_SSH_PORT: port where sshd is listening (e.g. 2222)
//! - TEST_SSH_KEY: path to private key authorized on the sshd
//! - TEST_ECHO_PORT: port where a TCP echo server is listening (e.g. 9999)
//!
//! If any are missing, all tests in this file are skipped.

use std::env;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

fn get_test_env() -> Option<(u16, String, u16)> {
    let ssh_port: u16 = env::var("TEST_SSH_PORT").ok()?.parse().ok()?;
    let ssh_key = env::var("TEST_SSH_KEY").ok()?;
    let echo_port: u16 = env::var("TEST_ECHO_PORT").ok()?.parse().ok()?;
    Some((ssh_port, ssh_key, echo_port))
}

fn create_test_config(
    ssh_port: u16,
    ssh_key: &str,
    echo_port: u16,
    local_port: u16,
) -> TestTunnelConfig {
    TestTunnelConfig {
        ssh_host: "127.0.0.1".to_string(),
        ssh_port,
        ssh_user: whoami(),
        ssh_key: ssh_key.to_string(),
        target_host: "127.0.0.1".to_string(),
        target_port: echo_port,
        local_port,
    }
}

struct TestTunnelConfig {
    ssh_host: String,
    ssh_port: u16,
    ssh_user: String,
    ssh_key: String,
    target_host: String,
    target_port: u16,
    local_port: u16,
}

fn whoami() -> String {
    env::var("USER")
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "test".to_string())
}

/// Allocate a free port for local use in tests
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Spawn an SSH tunnel and return the child process + ephemeral port
async fn spawn_ssh_tunnel(
    config: &TestTunnelConfig,
    ephemeral_port: u16,
) -> tokio::process::Child {
    tokio::process::Command::new("ssh")
        .args([
            "-N",
            "-L",
            &format!(
                "{}:{}:{}",
                ephemeral_port, config.target_host, config.target_port
            ),
            "-i",
            &config.ssh_key,
            "-p",
            &config.ssh_port.to_string(),
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ExitOnForwardFailure=yes",
            "-o",
            "BatchMode=yes",
            &format!("{}@{}", config.ssh_user, config.ssh_host),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("Failed to spawn ssh")
}

async fn wait_for_port(port: u16, timeout_ms: u64) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);
    while tokio::time::Instant::now() < deadline {
        if TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .is_ok()
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn basic_tunnel_lifecycle() {
    let Some((ssh_port, ssh_key, echo_port)) = get_test_env() else {
        eprintln!("Skipping integration test: TEST_SSH_PORT/TEST_SSH_KEY/TEST_ECHO_PORT not set");
        return;
    };

    let local_port = free_port();
    let ephemeral_port = free_port();
    let config = create_test_config(ssh_port, &ssh_key, echo_port, local_port);

    // Start SSH tunnel
    let mut ssh_child = spawn_ssh_tunnel(&config, ephemeral_port).await;

    // Wait for SSH to be ready
    assert!(
        wait_for_port(ephemeral_port, 5000).await,
        "SSH tunnel port not ready"
    );

    // Start proxy
    let stats = std::sync::Arc::new(bad_adit::stats::TunnelStats::new());
    let proxy =
        bad_adit::proxy::ProxyListener::start(local_port, ephemeral_port, stats.clone())
            .await
            .expect("Failed to start proxy");

    // Connect and send data
    let mut client = TcpStream::connect(format!("127.0.0.1:{}", local_port))
        .await
        .expect("Failed to connect to local port");

    client.write_all(b"hello").await.unwrap();
    client.shutdown().await.unwrap();

    let mut buf = vec![0u8; 1024];
    let n = client.read(&mut buf).await.unwrap();
    assert_eq!(&buf[..n], b"hello");

    // Verify stats
    tokio::time::sleep(Duration::from_millis(50)).await;
    let snap = stats.snapshot();
    assert!(snap.bytes_uploaded >= 5);
    assert!(snap.bytes_downloaded >= 5);
    assert_eq!(snap.connections_total, 1);

    // Stop
    proxy.stop().await;
    ssh_child.kill().await.unwrap();

    // Verify local port is closed
    assert!(
        TcpStream::connect(format!("127.0.0.1:{}", local_port))
            .await
            .is_err(),
        "Local port should be closed after stop"
    );
}

#[tokio::test]
async fn multiple_concurrent_connections() {
    let Some((ssh_port, ssh_key, echo_port)) = get_test_env() else {
        eprintln!("Skipping integration test");
        return;
    };

    let local_port = free_port();
    let ephemeral_port = free_port();
    let config = create_test_config(ssh_port, &ssh_key, echo_port, local_port);

    let mut ssh_child = spawn_ssh_tunnel(&config, ephemeral_port).await;
    assert!(wait_for_port(ephemeral_port, 5000).await);

    let stats = std::sync::Arc::new(bad_adit::stats::TunnelStats::new());
    let proxy =
        bad_adit::proxy::ProxyListener::start(local_port, ephemeral_port, stats.clone())
            .await
            .unwrap();

    // Open 5 connections
    let mut clients = Vec::new();
    for _ in 0..5 {
        let client = TcpStream::connect(format!("127.0.0.1:{}", local_port))
            .await
            .unwrap();
        clients.push(client);
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    let snap = stats.snapshot();
    assert_eq!(snap.connections_open, 5);
    assert_eq!(snap.connections_total, 5);

    // Close 3
    for _ in 0..3 {
        drop(clients.pop());
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let snap = stats.snapshot();
    assert_eq!(snap.connections_open, 2);
    assert_eq!(snap.connections_total, 5);

    // Close remaining
    clients.clear();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let snap = stats.snapshot();
    assert_eq!(snap.connections_open, 0);

    proxy.stop().await;
    ssh_child.kill().await.unwrap();
}

#[tokio::test]
async fn stats_accuracy() {
    let Some((ssh_port, ssh_key, echo_port)) = get_test_env() else {
        eprintln!("Skipping integration test");
        return;
    };

    let local_port = free_port();
    let ephemeral_port = free_port();
    let config = create_test_config(ssh_port, &ssh_key, echo_port, local_port);

    let mut ssh_child = spawn_ssh_tunnel(&config, ephemeral_port).await;
    assert!(wait_for_port(ephemeral_port, 5000).await);

    let stats = std::sync::Arc::new(bad_adit::stats::TunnelStats::new());
    let proxy =
        bad_adit::proxy::ProxyListener::start(local_port, ephemeral_port, stats.clone())
            .await
            .unwrap();

    // Send a known 4096-byte payload
    let payload = vec![0xABu8; 4096];
    let mut client = TcpStream::connect(format!("127.0.0.1:{}", local_port))
        .await
        .unwrap();

    client.write_all(&payload).await.unwrap();
    client.shutdown().await.unwrap();

    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response.len(), 4096);

    tokio::time::sleep(Duration::from_millis(100)).await;
    let snap = stats.snapshot();
    assert_eq!(snap.bytes_uploaded, 4096);
    assert_eq!(snap.bytes_downloaded, 4096);
    assert_eq!(snap.connections_total, 1);
    assert_eq!(snap.connections_open, 0);

    proxy.stop().await;
    ssh_child.kill().await.unwrap();
}
