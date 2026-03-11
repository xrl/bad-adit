use std::process::Command;

/// Send a macOS notification using osascript.
/// This bypasses bundle ID issues that affect tauri-plugin-notification in dev mode.
pub fn send_notification(title: &str, message: &str) {
    let script = format!(
        r#"display notification "{}" with title "{}""#,
        message.replace('\\', "\\\\").replace('"', "\\\""),
        title.replace('\\', "\\\\").replace('"', "\\\""),
    );
    let _ = Command::new("osascript").arg("-e").arg(&script).output();
}
