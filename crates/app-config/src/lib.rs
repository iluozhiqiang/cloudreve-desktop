use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

static CONFIG_MANAGER: OnceLock<ConfigManager> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    #[default]
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Trace => "trace",
            LogLevel::Debug => "debug",
            LogLevel::Info => "info",
            LogLevel::Warn => "warn",
            LogLevel::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub auto_start: bool,
    pub notify_credential_expired: bool,
    pub notify_file_conflict: bool,
    pub fast_popup_launch: bool,
    pub log_to_file: bool,
    pub log_level: LogLevel,
    pub log_max_files: usize,
    pub language: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            auto_start: true,
            notify_credential_expired: true,
            notify_file_conflict: true,
            fast_popup_launch: true,
            log_to_file: true,
            log_level: LogLevel::Debug,
            log_max_files: 5,
            language: None,
        }
    }
}

pub struct ConfigManager {
    config: RwLock<AppConfig>,
    config_path: PathBuf,
}

impl ConfigManager {
    pub fn init() -> Result<&'static ConfigManager> {
        let config_path = Self::get_config_path()?;
        let config = Self::load_from_path(&config_path)?;

        let manager = ConfigManager {
            config: RwLock::new(config),
            config_path,
        };

        Ok(CONFIG_MANAGER.get_or_init(|| manager))
    }

    pub fn get() -> &'static ConfigManager {
        CONFIG_MANAGER
            .get()
            .expect("ConfigManager::init() must be called before ConfigManager::get()")
    }

    pub fn try_get() -> Option<&'static ConfigManager> {
        CONFIG_MANAGER.get()
    }

    fn get_config_path() -> Result<PathBuf> {
        let home_dir = dirs::home_dir().context("Failed to get user home directory")?;
        Ok(home_dir.join(".cloudreve").join("config.json"))
    }

    fn load_from_path(path: &PathBuf) -> Result<AppConfig> {
        if !path.exists() {
            tracing::info!(target: "config", path = %path.display(), "Config file not found, using defaults");
            return Ok(AppConfig::default());
        }

        let content = fs::read_to_string(path).context("Failed to read config file")?;
        let config: AppConfig =
            serde_json::from_str(&content).context("Failed to parse config file")?;

        tracing::info!(target: "config", path = %path.display(), "Loaded configuration from file");

        Ok(config)
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.config_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).context("Failed to create config directory")?;
            }
        }

        let config = self.config.read().map_err(|e| {
            anyhow::anyhow!("Failed to acquire read lock on config: {}", e)
        })?;

        let content =
            serde_json::to_string_pretty(&*config).context("Failed to serialize config")?;

        fs::write(&self.config_path, content).context("Failed to write config file")?;

        tracing::debug!(target: "config", path = %self.config_path.display(), "Configuration saved");

        Ok(())
    }

    pub fn get_config(&self) -> AppConfig {
        self.config
            .read()
            .map(|c| c.clone())
            .unwrap_or_else(|_| AppConfig::default())
    }

    pub fn update<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut AppConfig),
    {
        {
            let mut config = self.config.write().map_err(|e| {
                anyhow::anyhow!("Failed to acquire write lock on config: {}", e)
            })?;
            f(&mut config);
        }
        self.save()
    }

    pub fn auto_start(&self) -> bool {
        self.config
            .read()
            .map(|c| c.auto_start)
            .unwrap_or(true)
    }

    pub fn set_auto_start(&self, enabled: bool) -> Result<()> {
        self.update(|config| {
            config.auto_start = enabled;
        })
    }

    pub fn notify_credential_expired(&self) -> bool {
        self.config
            .read()
            .map(|c| c.notify_credential_expired)
            .unwrap_or(true)
    }

    pub fn set_notify_credential_expired(&self, enabled: bool) -> Result<()> {
        self.update(|config| {
            config.notify_credential_expired = enabled;
        })
    }

    pub fn notify_file_conflict(&self) -> bool {
        self.config
            .read()
            .map(|c| c.notify_file_conflict)
            .unwrap_or(true)
    }

    pub fn set_notify_file_conflict(&self, enabled: bool) -> Result<()> {
        self.update(|config| {
            config.notify_file_conflict = enabled;
        })
    }

    pub fn fast_popup_launch(&self) -> bool {
        self.config
            .read()
            .map(|c| c.fast_popup_launch)
            .unwrap_or(true)
    }

    pub fn set_fast_popup_launch(&self, enabled: bool) -> Result<()> {
        self.update(|config| {
            config.fast_popup_launch = enabled;
        })
    }

    pub fn log_to_file(&self) -> bool {
        self.config
            .read()
            .map(|c| c.log_to_file)
            .unwrap_or(true)
    }

    pub fn set_log_to_file(&self, enabled: bool) -> Result<()> {
        self.update(|config| {
            config.log_to_file = enabled;
        })
    }

    pub fn log_level(&self) -> LogLevel {
        self.config
            .read()
            .map(|c| c.log_level)
            .unwrap_or(LogLevel::Info)
    }

    pub fn set_log_level(&self, level: LogLevel) -> Result<()> {
        self.update(|config| {
            config.log_level = level;
        })
    }

    pub fn log_max_files(&self) -> usize {
        self.config
            .read()
            .map(|c| c.log_max_files)
            .unwrap_or(5)
    }

    pub fn set_log_max_files(&self, max_files: usize) -> Result<()> {
        self.update(|config| {
            config.log_max_files = max_files;
        })
    }

    pub fn language(&self) -> Option<String> {
        self.config.read().ok().and_then(|c| c.language.clone())
    }

    pub fn set_language(&self, language: Option<String>) -> Result<()> {
        self.update(|config| {
            config.language = language;
        })
    }

    pub fn get_log_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".cloudreve")
            .join("logs")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert!(config.auto_start);
    }

    #[test]
    fn test_load_with_missing_fields() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, "{{}}").unwrap();

        let config = ConfigManager::load_from_path(&temp_file.path().to_path_buf()).unwrap();
        assert!(config.auto_start);
    }

    #[test]
    fn test_load_with_all_fields() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(temp_file, r#"{{"auto_start": false}}"#).unwrap();

        let config = ConfigManager::load_from_path(&temp_file.path().to_path_buf()).unwrap();
        assert!(!config.auto_start);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let path = PathBuf::from("/nonexistent/path/config.json");
        let config = ConfigManager::load_from_path(&path).unwrap();
        assert!(config.auto_start);
    }

    #[test]
    fn log_level_roundtrip_json() {
        let level = LogLevel::Warn;
        let j = serde_json::to_string(&level).unwrap();
        let back: LogLevel = serde_json::from_str(&j).unwrap();
        assert_eq!(back, level);
    }

    #[test]
    fn log_level_from_str_unknown_defaults_info() {
        assert_eq!(LogLevel::from_str("not-a-level"), LogLevel::Info);
        assert_eq!(LogLevel::from_str("DEBUG"), LogLevel::Debug);
    }

    #[test]
    fn app_config_partial_json_uses_defaults_for_omitted() {
        let j = r#"{"auto_start": false, "log_level": "error"}"#;
        let c: AppConfig = serde_json::from_str(j).unwrap();
        assert!(!c.auto_start);
        assert_eq!(c.log_level, LogLevel::Error);
        assert!(c.notify_file_conflict); // default
    }

    #[test]
    fn app_config_json_roundtrip() {
        let mut c = AppConfig::default();
        c.auto_start = false;
        c.language = Some("zh-CN".into());
        c.log_max_files = 12;
        let j = serde_json::to_string(&c).unwrap();
        let back: AppConfig = serde_json::from_str(&j).unwrap();
        assert_eq!(back.auto_start, c.auto_start);
        assert_eq!(back.language, c.language);
        assert_eq!(back.log_max_files, c.log_max_files);
    }
}
