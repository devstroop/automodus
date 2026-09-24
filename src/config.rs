//! Configuration for Automodus
//!
//! Loads configuration from TOML files and environment variables.

use serde::Deserialize;
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    /// Server configuration
    #[serde(default)]
    pub server: ServerConfig,

    /// Browser configuration
    #[serde(default)]
    pub browser: BrowserConfig,

    /// Workflows configuration
    #[serde(default)]
    pub workflows: WorkflowsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            browser: BrowserConfig::default(),
            workflows: WorkflowsConfig::default(),
        }
    }
}

impl AppConfig {
    /// Load configuration from file and environment
    pub fn load() -> Result<Self, config::ConfigError> {
        let config_path =
            std::env::var("AUTOMODUS_CONFIG").unwrap_or_else(|_| "config/app.toml".to_string());

        let builder = config::Config::builder()
            // Start with defaults
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 3000)?
            .set_default("browser.engine", "chromium")?
            .set_default("browser.headless", false)?
            .set_default("browser.timeout_ms", 30000)?
            .set_default("workflows.directory", "workflows")?;

        // Add config file if exists
        let builder = if std::path::Path::new(&config_path).exists() {
            builder.add_source(config::File::with_name(&config_path))
        } else {
            builder
        };

        // Add environment variables with AUTOMODUS_ prefix
        let builder = builder.add_source(
            config::Environment::with_prefix("AUTOMODUS")
                .separator("_")
                .try_parsing(true),
        );

        builder.build()?.try_deserialize()
    }
}

/// Server configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Host to bind to
    #[serde(default = "default_host")]
    pub host: String,

    /// Port to listen on
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3000
}

/// Browser engine backend.
///
/// `chromium` and `lightpanda` use CDP; `firefox` uses WebDriver BiDi via
/// rustenium (no geckodriver). `launch_session` dispatches on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BrowserEngine {
    /// Chromium / Chrome via CDP (chromiumoxide)
    #[default]
    Chromium,
    /// Firefox via WebDriver BiDi (rustenium; no geckodriver)
    Firefox,
    /// Lightpanda via CDP (spawn `lightpanda serve`, connect chromiumoxide)
    Lightpanda,
}

impl std::fmt::Display for BrowserEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserEngine::Chromium => write!(f, "chromium"),
            BrowserEngine::Firefox => write!(f, "firefox"),
            BrowserEngine::Lightpanda => write!(f, "lightpanda"),
        }
    }
}

/// Browser configuration
#[derive(Debug, Clone, Deserialize)]
pub struct BrowserConfig {
    /// Browser engine backend (chromium | firefox | lightpanda)
    #[serde(default)]
    pub engine: BrowserEngine,

    /// Run browser in headless mode
    #[serde(default)]
    pub headless: bool,

    /// Default timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,

    /// Chrome executable path (auto-detected if not set)
    pub chrome_path: Option<PathBuf>,

    /// Firefox executable path (auto-detected if not set; used when engine = firefox)
    pub firefox_path: Option<PathBuf>,

    /// Lightpanda executable path (auto-detected if not set; used when engine = lightpanda)
    pub lightpanda_path: Option<PathBuf>,

    /// User data directory for browser profile
    pub user_data_dir: Option<PathBuf>,

    /// Additional browser arguments
    #[serde(default)]
    pub args: Vec<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            engine: BrowserEngine::default(),
            headless: false,
            timeout_ms: default_timeout(),
            chrome_path: None,
            firefox_path: None,
            lightpanda_path: None,
            user_data_dir: None,
            args: vec![],
        }
    }
}

fn default_timeout() -> u64 {
    30000
}

/// Workflows configuration
#[derive(Debug, Clone, Deserialize)]
pub struct WorkflowsConfig {
    /// Directory containing workflow YAML files
    #[serde(default = "default_workflows_dir")]
    pub directory: String,

    /// Auto-reload workflows on change
    #[serde(default)]
    pub auto_reload: bool,
}

impl Default for WorkflowsConfig {
    fn default() -> Self {
        Self {
            directory: default_workflows_dir(),
            auto_reload: false,
        }
    }
}

fn default_workflows_dir() -> String {
    "workflows".to_string()
}

#[cfg(test)]
mod engine_load_tests {
    use super::*;
    use std::sync::Mutex;

    /// `AUTOMODUS_CONFIG` is process-global; serialize tests that set it.
    static CONFIG_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn loads_firefox_engine_from_config_file() {
        let _guard = CONFIG_ENV_LOCK.lock().unwrap();
        let path = std::env::temp_dir().join("automodus_firefox_test.toml");
        std::fs::write(&path, "[browser]\nengine = \"firefox\"\nheadless = true\n").unwrap();
        std::env::set_var("AUTOMODUS_CONFIG", &path);
        let cfg = super::AppConfig::load().expect("load");
        std::env::remove_var("AUTOMODUS_CONFIG");
        assert_eq!(cfg.browser.engine, super::BrowserEngine::Firefox, "engine={:?}", cfg.browser.engine);
        assert!(cfg.browser.headless);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loads_lightpanda_engine_from_config_file() {
        let _guard = CONFIG_ENV_LOCK.lock().unwrap();
        let path = std::env::temp_dir().join("automodus_lightpanda_test.toml");
        std::fs::write(&path, "[browser]\nengine = \"lightpanda\"\nheadless = true\n").unwrap();
        std::env::set_var("AUTOMODUS_CONFIG", &path);
        let cfg = super::AppConfig::load().expect("load");
        std::env::remove_var("AUTOMODUS_CONFIG");
        assert_eq!(
            cfg.browser.engine,
            super::BrowserEngine::Lightpanda,
            "engine={:?}",
            cfg.browser.engine
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lightpanda_display_is_lowercase() {
        assert_eq!(BrowserEngine::Lightpanda.to_string(), "lightpanda");
    }

    #[test]
    fn browser_config_has_lightpanda_path_field() {
        let cfg = BrowserConfig::default();
        assert!(cfg.lightpanda_path.is_none());
        assert_eq!(cfg.engine, BrowserEngine::Chromium);
    }
}
