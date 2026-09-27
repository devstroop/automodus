//! Daemon configuration file loading
//!
//! Loads daemon configuration from:
//! 1. `~/.automodus/daemon.toml` (user global)
//! 2. `config/app.toml` (workspace override)
//! 3. Environment variables (AUTOMODUS_*)
//! 4. Defaults

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Configuration file structure matching SHELL.md spec
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DaemonConfigFile {
    #[serde(default)]
    pub daemon: DaemonSection,
    #[serde(default)]
    pub http: HttpSection,
    #[serde(default)]
    pub browser: BrowserSection,
    #[serde(default)]
    pub limits: LimitsSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DaemonSection {
    #[serde(default = "default_socket_path")]
    pub socket_path: String,
    #[serde(default = "default_pid_file")]
    pub pid_file: String,
    #[serde(default = "default_log_file")]
    pub log_file: String,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for DaemonSection {
    fn default() -> Self {
        Self {
            socket_path: default_socket_path(),
            pid_file: default_pid_file(),
            log_file: default_log_file(),
            log_level: default_log_level(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpSection {
    #[serde(default = "default_http_host")]
    pub host: String,
    #[serde(default = "default_http_port")]
    pub port: u16,
    #[serde(default)]
    pub cors_origins: Vec<String>,
    #[serde(default = "default_enable_http")]
    pub enabled: bool,
}

impl Default for HttpSection {
    fn default() -> Self {
        Self {
            host: default_http_host(),
            port: default_http_port(),
            cors_origins: vec!["*".to_string()],
            enabled: default_enable_http(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSection {
    #[serde(default)]
    pub headless: bool,
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
    #[serde(default)]
    pub default_viewport: Option<Viewport>,
    #[serde(default)]
    pub user_data_dir: Option<String>,
}

impl Default for BrowserSection {
    fn default() -> Self {
        Self {
            headless: false,
            max_sessions: default_max_sessions(),
            default_viewport: Some(Viewport::default()),
            user_data_dir: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewport {
    #[serde(default = "default_viewport_width")]
    pub width: u32,
    #[serde(default = "default_viewport_height")]
    pub height: u32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            width: default_viewport_width(),
            height: default_viewport_height(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsSection {
    #[serde(default = "default_execution_timeout")]
    pub execution_timeout: u64,
    #[serde(default = "default_session_idle_timeout")]
    pub session_idle_timeout: u64,
}

impl Default for LimitsSection {
    fn default() -> Self {
        Self {
            execution_timeout: default_execution_timeout(),
            session_idle_timeout: default_session_idle_timeout(),
        }
    }
}

// Default value functions
fn default_socket_path() -> String {
    "~/.automodus/automodus.sock".to_string()
}

fn default_pid_file() -> String {
    "~/.automodus/daemon.pid".to_string()
}

fn default_log_file() -> String {
    "~/.automodus/daemon.log".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_http_host() -> String {
    "127.0.0.1".to_string()
}

fn default_http_port() -> u16 {
    8080
}

fn default_enable_http() -> bool {
    true
}

fn default_max_sessions() -> usize {
    10
}

fn default_viewport_width() -> u32 {
    1280
}

fn default_viewport_height() -> u32 {
    720
}

fn default_execution_timeout() -> u64 {
    300_000 // 5 minutes
}

fn default_session_idle_timeout() -> u64 {
    3_600_000 // 1 hour
}

/// Expand ~ to home directory
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Get the user config file path
pub fn user_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".automodus")
        .join("daemon.toml")
}

/// Get the workspace config file path
pub fn workspace_config_path() -> PathBuf {
    PathBuf::from("config/app.toml")
}

/// Load configuration from files and environment
pub fn load_config() -> DaemonConfigFile {
    let mut config = DaemonConfigFile::default();

    // Load user config if exists
    let user_path = user_config_path();
    if user_path.exists() {
        debug!("Loading user config from {:?}", user_path);
        if let Ok(content) = fs::read_to_string(&user_path) {
            match toml::from_str::<DaemonConfigFile>(&content) {
                Ok(user_config) => {
                    config = merge_configs(config, user_config);
                    info!("Loaded daemon config from {:?}", user_path);
                }
                Err(e) => {
                    warn!("Failed to parse {:?}: {}", user_path, e);
                }
            }
        }
    }

    // Load workspace config if exists (overrides user config)
    let workspace_path = workspace_config_path();
    if workspace_path.exists() {
        debug!("Loading workspace config from {:?}", workspace_path);
        if let Ok(content) = fs::read_to_string(&workspace_path) {
            match toml::from_str::<DaemonConfigFile>(&content) {
                Ok(workspace_config) => {
                    config = merge_configs(config, workspace_config);
                    info!("Loaded workspace config from {:?}", workspace_path);
                }
                Err(e) => {
                    warn!("Failed to parse {:?}: {}", workspace_path, e);
                }
            }
        }
    }

    // Override with environment variables
    config = apply_env_overrides(config);

    config
}

/// Merge two configs (right takes precedence for non-default values)
fn merge_configs(base: DaemonConfigFile, overlay: DaemonConfigFile) -> DaemonConfigFile {
    // For simplicity, overlay completely replaces base
    // In a more sophisticated implementation, we'd merge field-by-field
    DaemonConfigFile {
        daemon: DaemonSection {
            socket_path: overlay.daemon.socket_path,
            pid_file: overlay.daemon.pid_file,
            log_file: overlay.daemon.log_file,
            log_level: overlay.daemon.log_level,
        },
        http: HttpSection {
            host: overlay.http.host,
            port: overlay.http.port,
            cors_origins: if overlay.http.cors_origins.is_empty() {
                base.http.cors_origins
            } else {
                overlay.http.cors_origins
            },
            enabled: overlay.http.enabled,
        },
        browser: overlay.browser,
        limits: overlay.limits,
    }
}

/// Apply environment variable overrides
fn apply_env_overrides(mut config: DaemonConfigFile) -> DaemonConfigFile {
    if let Ok(val) = std::env::var("AUTOMODUS_SOCKET_PATH") {
        config.daemon.socket_path = val;
    }
    if let Ok(val) = std::env::var("AUTOMODUS_HTTP_HOST") {
        config.http.host = val;
    }
    if let Ok(val) = std::env::var("AUTOMODUS_HTTP_PORT") {
        if let Ok(port) = val.parse() {
            config.http.port = port;
        }
    }
    if let Ok(val) = std::env::var("AUTOMODUS_MAX_SESSIONS") {
        if let Ok(max) = val.parse() {
            config.browser.max_sessions = max;
        }
    }
    if let Ok(val) = std::env::var("AUTOMODUS_LOG_LEVEL") {
        config.daemon.log_level = val;
    }
    config
}

/// Convert file config to DaemonConfig
impl From<DaemonConfigFile> for super::DaemonConfig {
    fn from(file: DaemonConfigFile) -> Self {
        Self {
            socket_path: expand_tilde(&file.daemon.socket_path),
            pid_file: expand_tilde(&file.daemon.pid_file),
            log_file: expand_tilde(&file.daemon.log_file),
            http_host: file.http.host,
            http_port: file.http.port,
            max_sessions: file.browser.max_sessions,
            debug_dir: PathBuf::from("data/debug"),
            enable_http: file.http.enabled,
        }
    }
}

/// Create default config file if it doesn't exist
pub fn ensure_config_exists() -> std::io::Result<PathBuf> {
    let config_path = user_config_path();

    // Ensure parent directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Create default config if it doesn't exist
    if !config_path.exists() {
        let default_content = r#"# Automodus Daemon Configuration
# See: https://github.com/automodus/automodus/docs/SHELL.md

[daemon]
socket_path = "~/.automodus/automodus.sock"
pid_file = "~/.automodus/daemon.pid"
log_file = "~/.automodus/daemon.log"
log_level = "info"              # info | debug | trace

[http]
host = "127.0.0.1"
port = 8080
cors_origins = ["*"]
enabled = true

[browser]
headless = false
max_sessions = 10

[browser.default_viewport]
width = 1280
height = 720

[limits]
execution_timeout = 300000      # 5 minutes
session_idle_timeout = 3600000  # 1 hour
"#;
        fs::write(&config_path, default_content)?;
        info!("Created default config at {:?}", config_path);
    }

    Ok(config_path)
}

/// Validate configuration values
pub fn validate_config(config: &DaemonConfigFile) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    // Port range validation
    if config.http.port == 0 {
        errors.push("HTTP port cannot be 0".to_string());
    }

    // Session limit validation
    if config.browser.max_sessions == 0 {
        errors.push("max_sessions must be at least 1".to_string());
    }
    if config.browser.max_sessions > 100 {
        errors.push("max_sessions cannot exceed 100".to_string());
    }

    // Log level validation
    let valid_levels = ["trace", "debug", "info", "warn", "error"];
    if !valid_levels.contains(&config.daemon.log_level.as_str()) {
        errors.push(format!(
            "Invalid log_level '{}', must be one of: {:?}",
            config.daemon.log_level, valid_levels
        ));
    }

    // Timeout validation
    if config.limits.execution_timeout < 1000 {
        errors.push("execution_timeout must be at least 1000ms".to_string());
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DaemonConfigFile::default();
        assert_eq!(config.http.port, 8080);
        assert_eq!(config.browser.max_sessions, 10);
    }

    #[test]
    fn test_expand_tilde() {
        let expanded = expand_tilde("~/.automodus/test");
        assert!(!expanded.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn test_validate_valid_config() {
        let config = DaemonConfigFile::default();
        assert!(validate_config(&config).is_ok());
    }

    #[test]
    fn test_validate_invalid_port() {
        let mut config = DaemonConfigFile::default();
        config.http.port = 0;
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_max_sessions() {
        let mut config = DaemonConfigFile::default();
        config.browser.max_sessions = 101;
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_log_level() {
        let mut config = DaemonConfigFile::default();
        config.daemon.log_level = "invalid".to_string();
        let result = validate_config(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_paths() {
        let user_path = user_config_path();
        assert!(user_path.to_string_lossy().contains("daemon.toml"));

        let workspace_path = workspace_config_path();
        assert_eq!(workspace_path, PathBuf::from("config/app.toml"));
    }
}
