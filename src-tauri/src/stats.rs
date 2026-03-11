use serde::Serialize;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

pub struct TunnelStats {
    pub bytes_uploaded: AtomicU64,
    pub bytes_downloaded: AtomicU64,
    pub connections_open: AtomicU32,
    pub connections_total: AtomicU64,
    pub started_at: Instant,
    pub last_reconnect: std::sync::Mutex<Option<Instant>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsSnapshot {
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
    pub connections_open: u32,
    pub connections_total: u64,
    pub uptime_seconds: u64,
    pub bytes_uploaded_formatted: String,
    pub bytes_downloaded_formatted: String,
    pub uptime_formatted: String,
    pub last_reconnect_ago: Option<String>,
}

impl Default for TunnelStats {
    fn default() -> Self {
        Self::new()
    }
}

impl TunnelStats {
    pub fn new() -> Self {
        Self {
            bytes_uploaded: AtomicU64::new(0),
            bytes_downloaded: AtomicU64::new(0),
            connections_open: AtomicU32::new(0),
            connections_total: AtomicU64::new(0),
            started_at: Instant::now(),
            last_reconnect: std::sync::Mutex::new(None),
        }
    }

    pub fn record_upload(&self, n: u64) {
        self.bytes_uploaded.fetch_add(n, Ordering::Relaxed);
    }

    pub fn record_download(&self, n: u64) {
        self.bytes_downloaded.fetch_add(n, Ordering::Relaxed);
    }

    pub fn connection_opened(&self) {
        self.connections_open.fetch_add(1, Ordering::Relaxed);
        self.connections_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn connection_closed(&self) {
        self.connections_open.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let uptime = self.started_at.elapsed();
        let up = self.bytes_uploaded.load(Ordering::Relaxed);
        let down = self.bytes_downloaded.load(Ordering::Relaxed);
        let reconnect_ago = self
            .last_reconnect
            .lock()
            .unwrap()
            .map(|t| crate::format::format_uptime(t.elapsed()));

        StatsSnapshot {
            bytes_uploaded: up,
            bytes_downloaded: down,
            connections_open: self.connections_open.load(Ordering::Relaxed),
            connections_total: self.connections_total.load(Ordering::Relaxed),
            uptime_seconds: uptime.as_secs(),
            bytes_uploaded_formatted: crate::format::format_bytes(up),
            bytes_downloaded_formatted: crate::format::format_bytes(down),
            uptime_formatted: crate::format::format_uptime(uptime),
            last_reconnect_ago: reconnect_ago,
        }
    }

    pub fn reset(&self) {
        self.bytes_uploaded.store(0, Ordering::Relaxed);
        self.bytes_downloaded.store(0, Ordering::Relaxed);
        self.connections_open.store(0, Ordering::Relaxed);
        self.connections_total.store(0, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_counting() {
        let stats = TunnelStats::new();
        stats.record_upload(100);
        stats.record_upload(200);
        stats.record_download(50);

        let snap = stats.snapshot();
        assert_eq!(snap.bytes_uploaded, 300);
        assert_eq!(snap.bytes_downloaded, 50);
    }

    #[test]
    fn connection_tracking() {
        let stats = TunnelStats::new();
        stats.connection_opened();
        stats.connection_opened();
        stats.connection_opened();

        let snap = stats.snapshot();
        assert_eq!(snap.connections_open, 3);
        assert_eq!(snap.connections_total, 3);

        stats.connection_closed();
        let snap = stats.snapshot();
        assert_eq!(snap.connections_open, 2);
        assert_eq!(snap.connections_total, 3);
    }

    #[test]
    fn reset_clears_counters() {
        let stats = TunnelStats::new();
        stats.record_upload(1000);
        stats.record_download(2000);
        stats.connection_opened();

        stats.reset();
        let snap = stats.snapshot();
        assert_eq!(snap.bytes_uploaded, 0);
        assert_eq!(snap.bytes_downloaded, 0);
        assert_eq!(snap.connections_open, 0);
        assert_eq!(snap.connections_total, 0);
    }

    #[test]
    fn concurrent_updates() {
        use std::sync::Arc;
        use std::thread;

        let stats = Arc::new(TunnelStats::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let s = Arc::clone(&stats);
            handles.push(thread::spawn(move || {
                for _ in 0..1000 {
                    s.record_upload(1);
                    s.record_download(1);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let snap = stats.snapshot();
        assert_eq!(snap.bytes_uploaded, 10_000);
        assert_eq!(snap.bytes_downloaded, 10_000);
    }
}
