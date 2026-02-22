use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct SimulationConfig {
    #[serde(default = "default_tick_rate")]
    pub tick_rate_hz: f32,
    #[serde(default = "default_snapshot_interval")]
    pub snapshot_interval: u32,
    #[serde(default = "default_max_snapshots")]
    pub max_snapshots: u32,
    #[serde(default = "default_snapshot_directory")]
    pub snapshot_directory: String,
    #[serde(default = "default_websocket_port")]
    pub websocket_port: u16,
    #[serde(default = "default_websocket_bind")]
    pub websocket_bind: String,
    #[serde(default = "default_rule_directory")]
    pub rule_directory: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_season_length")]
    pub season_length: u32,
    #[serde(default = "default_rule_timeout_ms")]
    pub rule_timeout_ms: u32,
    #[serde(default = "default_native_evaluation")]
    pub native_evaluation: bool,
}

fn default_tick_rate() -> f32 {
    1.0
}
fn default_snapshot_interval() -> u32 {
    100
}
fn default_max_snapshots() -> u32 {
    10
}
fn default_snapshot_directory() -> String {
    "./snapshots".to_string()
}
fn default_websocket_port() -> u16 {
    8118
}
fn default_websocket_bind() -> String {
    "127.0.0.1".to_string()
}
fn default_rule_directory() -> String {
    "./rules".to_string()
}
fn default_log_level() -> String {
    "info".to_string()
}
fn default_season_length() -> u32 {
    90
}
fn default_rule_timeout_ms() -> u32 {
    10
}
fn default_native_evaluation() -> bool {
    true
}

impl SimulationConfig {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
        Self::from_toml_str(&content, path)
    }

    pub fn from_toml_str(content: &str, source_path: &Path) -> Result<Self, String> {
        let config: SimulationConfig =
            toml::from_str(content).map_err(|e| format!("{}: {}", source_path.display(), e))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut errors = Vec::new();

        if self.tick_rate_hz <= 0.0 {
            errors.push(format!(
                "tick_rate_hz must be > 0.0, got {}. Example: tick_rate_hz = 1.0",
                self.tick_rate_hz
            ));
        }

        if self.snapshot_interval == 0 {
            errors.push(format!(
                "snapshot_interval must be > 0, got {}. Example: snapshot_interval = 100",
                self.snapshot_interval
            ));
        }

        if self.max_snapshots == 0 {
            errors.push(format!(
                "max_snapshots must be > 0, got {}. Example: max_snapshots = 10",
                self.max_snapshots
            ));
        }

        if !(1024..=65535).contains(&self.websocket_port) {
            errors.push(format!(
                "websocket_port must be 1024-65535, got {}. Example: websocket_port = 8118",
                self.websocket_port
            ));
        }

        if self.season_length == 0 {
            errors.push(format!(
                "season_length must be > 0, got {}. Example: season_length = 90",
                self.season_length
            ));
        }

        if self.rule_timeout_ms == 0 {
            errors.push(format!(
                "rule_timeout_ms must be > 0, got {}. Example: rule_timeout_ms = 10",
                self.rule_timeout_ms
            ));
        }

        let valid_levels = ["error", "warn", "info", "debug", "trace"];
        if !valid_levels.contains(&self.log_level.as_str()) {
            errors.push(format!(
                "log_level must be one of {:?}, got '{}'. Example: log_level = \"info\"",
                valid_levels, self.log_level
            ));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn test_path() -> PathBuf {
        PathBuf::from("test-config.toml")
    }

    #[test]
    fn valid_config_loads_all_fields() {
        let toml = r#"
            tick_rate_hz = 2.0
            snapshot_interval = 50
            max_snapshots = 5
            snapshot_directory = "./data/snapshots"
            websocket_port = 9090
            websocket_bind = "0.0.0.0"
            rule_directory = "./my-rules"
            log_level = "debug"
            season_length = 120
            rule_timeout_ms = 20
        "#;
        let config = SimulationConfig::from_toml_str(toml, &test_path()).unwrap();
        assert_eq!(config.tick_rate_hz, 2.0);
        assert_eq!(config.snapshot_interval, 50);
        assert_eq!(config.max_snapshots, 5);
        assert_eq!(config.snapshot_directory, "./data/snapshots");
        assert_eq!(config.websocket_port, 9090);
        assert_eq!(config.websocket_bind, "0.0.0.0");
        assert_eq!(config.rule_directory, "./my-rules");
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.season_length, 120);
        assert_eq!(config.rule_timeout_ms, 20);
    }

    #[test]
    fn defaults_applied_for_empty_config() {
        let config = SimulationConfig::from_toml_str("", &test_path()).unwrap();
        assert_eq!(config.tick_rate_hz, 1.0);
        assert_eq!(config.snapshot_interval, 100);
        assert_eq!(config.max_snapshots, 10);
        assert_eq!(config.snapshot_directory, "./snapshots");
        assert_eq!(config.websocket_port, 8118);
        assert_eq!(config.websocket_bind, "127.0.0.1");
        assert_eq!(config.rule_directory, "./rules");
        assert_eq!(config.log_level, "info");
        assert_eq!(config.season_length, 90);
        assert_eq!(config.rule_timeout_ms, 10);
    }

    #[test]
    fn invalid_tick_rate_rejected() {
        let err = SimulationConfig::from_toml_str("tick_rate_hz = -1.0", &test_path()).unwrap_err();
        assert!(err.contains("tick_rate_hz"));
        assert!(err.contains("> 0.0"));
    }

    #[test]
    fn invalid_snapshot_interval_rejected() {
        let err =
            SimulationConfig::from_toml_str("snapshot_interval = 0", &test_path()).unwrap_err();
        assert!(err.contains("snapshot_interval"));
    }

    #[test]
    fn invalid_websocket_port_rejected() {
        let err =
            SimulationConfig::from_toml_str("websocket_port = 80", &test_path()).unwrap_err();
        assert!(err.contains("websocket_port"));
        assert!(err.contains("1024-65535"));
    }

    #[test]
    fn invalid_log_level_rejected() {
        let err =
            SimulationConfig::from_toml_str(r#"log_level = "verbose""#, &test_path()).unwrap_err();
        assert!(err.contains("log_level"));
    }

    #[test]
    fn invalid_season_length_rejected() {
        let err =
            SimulationConfig::from_toml_str("season_length = 0", &test_path()).unwrap_err();
        assert!(err.contains("season_length"));
    }

    #[test]
    fn multiple_errors_reported_together() {
        let toml = "tick_rate_hz = 0.0\nsnapshot_interval = 0\nseason_length = 0";
        let err = SimulationConfig::from_toml_str(toml, &test_path()).unwrap_err();
        assert!(err.contains("tick_rate_hz"));
        assert!(err.contains("snapshot_interval"));
        assert!(err.contains("season_length"));
    }

    #[test]
    fn malformed_toml_includes_source_path() {
        let err =
            SimulationConfig::from_toml_str("tick_rate_hz = [invalid", &test_path()).unwrap_err();
        assert!(err.contains("test-config.toml"));
    }

    #[test]
    fn from_file_loads_valid_config() {
        let mut tmp = NamedTempFile::new().unwrap();
        use std::io::Write;
        writeln!(tmp, "tick_rate_hz = 5.0").unwrap();
        let config = SimulationConfig::from_file(tmp.path()).unwrap();
        assert_eq!(config.tick_rate_hz, 5.0);
    }

    #[test]
    fn from_file_missing_file_error() {
        let err = SimulationConfig::from_file(Path::new("/nonexistent/config.toml")).unwrap_err();
        assert!(err.contains("Cannot read"));
    }
}
