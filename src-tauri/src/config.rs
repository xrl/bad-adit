use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub id: String,
    pub name: String,
    pub ssh_host: String,
    pub ssh_user: String,
    pub ssh_key_path: String,
    #[serde(default = "default_target_host")]
    pub target_host: String,
    pub target_port: u16,
    pub local_port: u16,
    #[serde(default)]
    pub auto_reconnect: bool,
}

fn default_target_host() -> String {
    "localhost".to_string()
}

#[derive(Debug)]
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new() -> Self {
        let dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("bad-adit");
        Self {
            path: dir.join("tunnels.json"),
        }
    }

    #[cfg(test)]
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> Vec<TunnelConfig> {
        match fs::read_to_string(&self.path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    pub fn save(&self, configs: &[TunnelConfig]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create config dir: {e}"))?;
        }
        let data =
            serde_json::to_string_pretty(configs).map_err(|e| format!("Serialize error: {e}"))?;
        fs::write(&self.path, data).map_err(|e| format!("Failed to write config: {e}"))?;
        Ok(())
    }
}

pub fn validate_config(config: &TunnelConfig, all_configs: &[TunnelConfig]) -> Result<(), String> {
    if config.name.trim().is_empty() {
        return Err("Tunnel name cannot be empty".to_string());
    }
    if config.local_port == 0 {
        return Err("Local port must be between 1 and 65535".to_string());
    }
    if config.target_port == 0 {
        return Err("Target port must be between 1 and 65535".to_string());
    }
    if config.ssh_host.trim().is_empty() {
        return Err("SSH host cannot be empty".to_string());
    }
    if config.ssh_user.trim().is_empty() {
        return Err("SSH user cannot be empty".to_string());
    }

    // Check for duplicate local ports (excluding this tunnel's own config)
    let mut seen_ports = HashSet::new();
    for c in all_configs {
        if c.id != config.id {
            seen_ports.insert(c.local_port);
        }
    }
    if seen_ports.contains(&config.local_port) {
        return Err(format!(
            "Local port {} is already used by another tunnel",
            config.local_port
        ));
    }

    // Warn about SSH key path (don't block)
    if !config.ssh_key_path.is_empty() {
        let key_path = PathBuf::from(&config.ssh_key_path);
        if !key_path.exists() {
            log::warn!("SSH key path does not exist: {}", config.ssh_key_path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_config(id: &str, name: &str, local_port: u16) -> TunnelConfig {
        TunnelConfig {
            id: id.to_string(),
            name: name.to_string(),
            ssh_host: "example.com".to_string(),
            ssh_user: "user".to_string(),
            ssh_key_path: String::new(),
            target_host: "localhost".to_string(),
            target_port: 5432,
            local_port,
            auto_reconnect: false,
        }
    }

    #[test]
    fn round_trip_serialize() {
        let configs = vec![make_config("1", "Test", 5432)];
        let json = serde_json::to_string(&configs).unwrap();
        let parsed: Vec<TunnelConfig> = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "Test");
        assert_eq!(parsed[0].local_port, 5432);
    }

    #[test]
    fn config_store_round_trip() {
        let mut tmp = NamedTempFile::new().unwrap();
        write!(tmp, "[]").unwrap();
        let store = ConfigStore::with_path(tmp.path().to_path_buf());

        let configs = vec![make_config("1", "DB", 5432)];
        store.save(&configs).unwrap();

        let loaded = store.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "DB");
    }

    #[test]
    fn validate_rejects_empty_name() {
        let c = make_config("1", "", 5432);
        assert!(validate_config(&c, &[]).is_err());
    }

    #[test]
    fn validate_rejects_zero_port() {
        let c = make_config("1", "Test", 0);
        assert!(validate_config(&c, &[]).is_err());
    }

    #[test]
    fn validate_rejects_duplicate_local_port() {
        let existing = vec![make_config("1", "Existing", 5432)];
        let new = make_config("2", "New", 5432);
        assert!(validate_config(&new, &existing).is_err());
    }

    #[test]
    fn validate_allows_same_port_on_self_edit() {
        let existing = vec![make_config("1", "Existing", 5432)];
        let edited = make_config("1", "Edited", 5432);
        assert!(validate_config(&edited, &existing).is_ok());
    }
}
