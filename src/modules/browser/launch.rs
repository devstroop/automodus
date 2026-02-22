//! Browser Launch Helpers
//!
//! Common utilities for launching and configuring Chromium browser instances.

use chromiumoxide::browser::{Browser, BrowserConfig};
use futures_util::stream::StreamExt;
use std::path::PathBuf;
use tracing::{debug, info};

/// Options for launching a browser
#[derive(Debug, Clone)]
pub struct LaunchOptions {
    /// Run browser in headless mode
    pub headless: bool,
    /// User data directory for browser profile
    pub user_data_dir: PathBuf,
    /// Additional Chrome arguments
    pub extra_args: Vec<String>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            headless: true,
            user_data_dir: std::env::temp_dir().join("automodus-browser"),
            extra_args: vec![],
        }
    }
}

impl LaunchOptions {
    /// Create options for running workflow (uses temp profile)
    pub fn for_workflow() -> Self {
        Self {
            headless: true,
            user_data_dir: std::env::temp_dir()
                .join(format!("automodus-workflow-{}", std::process::id())),
            extra_args: vec![],
        }
    }

    /// Create options for shell mode (non-headless, temp profile)
    pub fn for_shell() -> Self {
        Self {
            headless: false,
            user_data_dir: std::env::temp_dir()
                .join(format!("automodus-shell-{}", std::process::id())),
            extra_args: vec![],
        }
    }

    /// Create options for server mode
    pub fn for_server(headless: bool) -> Self {
        Self {
            headless,
            user_data_dir: std::env::temp_dir().join("automodus-server"),
            extra_args: vec![],
        }
    }

    /// Set headless mode
    pub fn headless(mut self, headless: bool) -> Self {
        self.headless = headless;
        self
    }

    /// Set user data directory
    pub fn user_data_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.user_data_dir = path.into();
        self
    }

    /// Add extra Chrome argument
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.extra_args.push(arg.into());
        self
    }
}

/// Default Chrome arguments for stability
pub fn default_chrome_args() -> Vec<&'static str> {
    vec![
        "--no-sandbox",
        "--disable-setuid-sandbox",
        "--disable-dev-shm-usage",
        "--disable-web-security",
        "--disable-extensions",
        "--disable-gpu",
        "--no-first-run",
        "--disable-session-crashed-bubble",
        "--disable-infobars",
    ]
}

/// Build a browser configuration from launch options
pub fn build_browser_config(options: &LaunchOptions) -> Result<BrowserConfig, String> {
    // Ensure user data directory exists
    std::fs::create_dir_all(&options.user_data_dir)
        .map_err(|e| format!("Failed to create user data dir: {}", e))?;

    let mut builder = BrowserConfig::builder();

    // Headless mode
    if !options.headless {
        builder = builder.with_head();
    }

    // Add default stability args
    for arg in default_chrome_args() {
        builder = builder.arg(arg);
    }

    // Add user data dir
    builder = builder.arg(format!(
        "--user-data-dir={}",
        options.user_data_dir.display()
    ));

    // Add extra args
    for arg in &options.extra_args {
        builder = builder.arg(arg.as_str());
    }

    builder
        .build()
        .map_err(|e| format!("Failed to build browser config: {}", e))
}

/// Launch a browser with the given options
///
/// Returns the browser instance. The handler task is spawned automatically.
pub async fn launch_browser(options: &LaunchOptions) -> Result<Browser, String> {
    let config = build_browser_config(options)?;

    info!(
        headless = options.headless,
        user_data_dir = %options.user_data_dir.display(),
        "Launching browser"
    );

    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| format!("Failed to launch browser: {}", e))?;

    // Spawn handler task to process browser events
    tokio::spawn(async move {
        while let Some(event) = handler.next().await {
            if let Err(e) = event {
                debug!("Browser event: {:?}", e);
                if e.to_string().contains("connection closed") {
                    break;
                }
            }
        }
    });

    info!("Browser launched successfully");
    Ok(browser)
}

/// Get or create a page in the browser
pub async fn get_or_create_page(
    browser: &Browser,
    url: Option<&str>,
) -> Result<chromiumoxide::page::Page, String> {
    // Wait a bit for browser to settle
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let pages = browser
        .pages()
        .await
        .map_err(|e| format!("Failed to get pages: {}", e))?;

    if pages.is_empty() {
        let url = url.unwrap_or("about:blank");
        browser
            .new_page(url)
            .await
            .map_err(|e| format!("Failed to create page: {}", e))
    } else {
        Ok(pages.into_iter().next().unwrap())
    }
}
