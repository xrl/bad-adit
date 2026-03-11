use std::time::Duration;

pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

pub fn format_uptime(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_cases() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1), "1 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
        // u64::MAX is huge, just verify it doesn't panic and shows GB
        let result = format_bytes(u64::MAX);
        assert!(result.ends_with("GB"), "got: {}", result);
    }

    #[test]
    fn format_uptime_cases() {
        assert_eq!(format_uptime(Duration::from_secs(0)), "0s");
        assert_eq!(format_uptime(Duration::from_secs(12)), "12s");
        assert_eq!(format_uptime(Duration::from_secs(90)), "1m 30s");
        assert_eq!(format_uptime(Duration::from_secs(3600 + 120)), "1h 2m");
        assert_eq!(
            format_uptime(Duration::from_secs(3 * 86400 + 3600)),
            "3d 1h"
        );
    }
}
