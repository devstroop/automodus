//! Server State
//!
//! Shared state for the API server.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use crate::actions::BrowserHandle;
use crate::config::AppConfig;
use crate::core::WorkflowEngine;
use crate::modules::ChromePageAdapter;
use crate::workflow::{Workflow, WorkflowParser};

/// Shared state for the API server
pub struct ServerState {
    /// Workflow execution engine
    pub engine: WorkflowEngine,
    /// Loaded workflows (name -> Workflow)
    pub workflows: RwLock<HashMap<String, Workflow>>,
    /// Browser instance (lazily initialized)
    pub browser: Mutex<Option<chromiumoxide::browser::Browser>>,
    /// Current page adapter
    pub page_adapter: Mutex<Option<ChromePageAdapter>>,
    /// Configuration
    pub config: AppConfig,
}

impl ServerState {
    /// Create a new server state
    pub fn new(config: AppConfig) -> Self {
        Self {
            engine: WorkflowEngine::new(),
            workflows: RwLock::new(HashMap::new()),
            browser: Mutex::new(None),
            page_adapter: Mutex::new(None),
            config,
        }
    }

    /// Load workflows from directory
    pub async fn load_workflows(
        &self,
        dir: &str,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut workflows = self.workflows.write().await;
        workflows.clear();

        let pattern = format!("{}/**/*.yaml", dir);
        for entry in glob::glob(&pattern)? {
            if let Ok(path) = entry {
                match std::fs::read_to_string(&path) {
                    Ok(content) => match WorkflowParser::parse(&content) {
                        Ok(workflow) => {
                            info!("Loaded workflow: {} from {}", workflow.name, path.display());
                            workflows.insert(workflow.name.clone(), workflow);
                        }
                        Err(e) => {
                            warn!("Failed to parse {}: {}", path.display(), e);
                        }
                    },
                    Err(e) => {
                        warn!("Failed to read {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(workflows.len())
    }

    /// Get or create browser and page
    pub async fn get_page(&self) -> Result<ChromePageAdapter, String> {
        use chromiumoxide::browser::{Browser, BrowserConfig};
        use futures_util::stream::StreamExt;

        let mut browser_guard = self.browser.lock().await;
        let mut page_guard = self.page_adapter.lock().await;

        // Check if we have a valid page
        if let Some(ref adapter) = *page_guard {
            // Try to verify it's still valid
            if adapter.current_url().await.is_ok() {
                return Ok(adapter.clone());
            }
        }

        // Need to create browser
        if browser_guard.is_none() {
            info!("Launching browser...");

            let data_dir = std::env::temp_dir().join("automodus-server");
            let _ = std::fs::create_dir_all(&data_dir);

            let headless = self.config.browser.headless;
            let mut config_builder = BrowserConfig::builder();

            if !headless {
                config_builder = config_builder.with_head();
            }

            config_builder = config_builder
                .arg("--no-sandbox")
                .arg("--disable-setuid-sandbox")
                .arg("--disable-dev-shm-usage")
                .arg("--disable-web-security")
                .arg("--disable-extensions")
                .arg("--disable-gpu")
                .arg("--no-first-run")
                .arg("--disable-session-crashed-bubble")
                .arg(format!("--user-data-dir={}", data_dir.display()));

            let browser_config = config_builder
                .build()
                .map_err(|e| format!("Failed to build browser config: {}", e))?;

            let (browser, mut handler) = Browser::launch(browser_config)
                .await
                .map_err(|e| format!("Failed to launch browser: {}", e))?;

            // Spawn handler task
            tokio::spawn(async move {
                while let Some(h) = handler.next().await {
                    if let Err(e) = h {
                        if e.to_string().contains("connection closed") {
                            break;
                        }
                    }
                }
            });

            *browser_guard = Some(browser);
            info!("Browser launched");
        }

        // Get or create page
        let browser = browser_guard.as_ref().unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        let pages = browser
            .pages()
            .await
            .map_err(|e| format!("Failed to get pages: {}", e))?;

        let page = if pages.is_empty() {
            browser
                .new_page("about:blank")
                .await
                .map_err(|e| format!("Failed to create page: {}", e))?
        } else {
            pages.into_iter().next().unwrap()
        };

        let adapter = ChromePageAdapter::new(page);
        *page_guard = Some(adapter.clone());

        Ok(adapter)
    }
}

/// Create a shared server state wrapped in Arc
pub fn create_state(config: AppConfig) -> Arc<ServerState> {
    Arc::new(ServerState::new(config))
}
