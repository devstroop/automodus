//! Browser Driver
//!
//! Chrome browser lifecycle management using chromiumoxide.
//! Handles browser launch, page management, and session persistence.

use crate::config::AppConfig;
use crate::modules::browser::launch::resolve_chrome_path_with;
use anyhow::Result;
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::page::Page;
use futures_util::stream::StreamExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// Browser service configuration
///
/// Chrome executable resolution (aligned with [`LaunchOptions`]):
/// - `chrome_path: Some(path)` — **explicit programmatic** override; must exist
/// - `chrome_path: None` — [`resolve_chrome_path_with`] using the service's
///   held `AppConfig` (`AUTOMODUS_CHROME_PATH` → `CHROME` env →
///   `AppConfig.chrome_path` → Playwright/system auto-detect)
///
/// Note: this is the BrowserService path (daemon/shell). The workflow CLI uses
/// [`crate::modules::browser::LaunchOptions::chrome_path`] via `launch_browser`.
/// Both entry points share the same priority: explicit option → env → config → auto.
#[derive(Debug, Clone)]
pub struct BrowserServiceConfig {
    /// User data directory for Chrome profile
    pub user_data_dir: PathBuf,
    /// Whether to run headless
    pub headless: bool,
    /// Browser timeout in milliseconds
    pub timeout_ms: u64,
    /// Additional browser arguments
    pub args: Vec<String>,
    /// Explicit Chrome/Chromium executable path (programmatic override only;
    /// `AppConfig.browser.chrome_path` is resolved inside
    /// [`resolve_chrome_path_with`] **after** env vars, using the service's
    /// held AppConfig instance — no re-load from disk).
    pub chrome_path: Option<PathBuf>,
}

impl BrowserServiceConfig {
    /// Create config from AppConfig (legacy single-account mode)
    ///
    /// `chrome_path` is intentionally left as `None` here so env vars correctly
    /// take precedence over config-file values. The passed AppConfig's
    /// `browser.chrome_path` is preserved on `BrowserService.config` and
    /// consulted via [`resolve_chrome_path_with`] — no global re-load.
    pub fn from_app_config(config: &AppConfig) -> Self {
        let base_dir = if cfg!(target_os = "windows") {
            std::env::var("LOCALAPPDATA").unwrap_or_else(|_| {
                std::env::var("APPDATA").unwrap_or_else(|_| "C:\\Users\\Public".to_string())
            })
        } else {
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string())
        };

        Self {
            user_data_dir: PathBuf::from(format!("{}/automodus/chrome-profile", base_dir)),
            headless: config.browser.headless,
            timeout_ms: config.browser.timeout_ms,
            args: config.browser.args.clone(),
            // Do NOT copy AppConfig.browser.chrome_path into the explicit slot —
            // that would make config beat env. The held AppConfig is passed to
            // resolve_chrome_path_with at initialize time instead.
            chrome_path: None,
        }
    }

    /// Create config for a specific account
    pub fn for_account(account_data_dir: &Path, headless: bool, timeout_ms: u64) -> Self {
        Self {
            user_data_dir: account_data_dir.join("chrome-profile"),
            headless,
            timeout_ms,
            args: vec![
                "--disable-blink-features=AutomationControlled".to_string(),
                "--no-sandbox".to_string(),
                "--disable-setuid-sandbox".to_string(),
                "--disable-dev-shm-usage".to_string(),
                "--disable-extensions".to_string(),
                "--disable-popup-blocking".to_string(),
                "--disable-gpu".to_string(),
                "--disable-software-rasterizer".to_string(),
            ],
            chrome_path: None,
        }
    }
}

/// Browser service for managing Chrome browser instances
pub struct BrowserService {
    #[allow(dead_code)] // Reserved for future use
    config: Arc<AppConfig>,
    browser_config: BrowserServiceConfig,
    browser: Arc<Mutex<Option<Browser>>>,
    default_page: Arc<Mutex<Option<Page>>>,
    user_data_dir: Arc<Mutex<Option<String>>>,
}

impl BrowserService {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let browser_config = BrowserServiceConfig::from_app_config(&config);
        Self {
            config,
            browser_config,
            browser: Arc::new(Mutex::new(None)),
            default_page: Arc::new(Mutex::new(None)),
            user_data_dir: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new browser service with custom configuration (for multi-account)
    pub fn with_config(config: Arc<AppConfig>, browser_config: BrowserServiceConfig) -> Self {
        Self {
            config,
            browser_config,
            browser: Arc::new(Mutex::new(None)),
            default_page: Arc::new(Mutex::new(None)),
            user_data_dir: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the user data directory path
    pub fn get_user_data_dir(&self) -> &PathBuf {
        &self.browser_config.user_data_dir
    }

    /// Initialize the browser service
    pub async fn initialize(&self) -> Result<()> {
        debug!("Initializing browser service with chromiumoxide");

        // Clean up any leftover Chrome processes
        self.cleanup_existing_chrome_processes().await;

        // Create browser config
        let mut browser_config = BrowserConfig::builder();

        // Chrome executable: explicit programmatic option → env → held AppConfig → auto.
        // Err = explicit path configured but missing — fail loudly, don't fall back.
        // Uses resolve_chrome_path_with(&self.config) so a programmatically-built
        // or custom-path AppConfig is preserved without re-loading from disk.
        match &self.browser_config.chrome_path {
            Some(chrome) => {
                if chrome.exists() {
                    browser_config = browser_config.chrome_executable(chrome.as_path());
                } else {
                    return Err(anyhow::anyhow!(
                        "browser chrome_path is set to '{}' but that path does not exist",
                        chrome.display()
                    ));
                }
            }
            None => {
                match resolve_chrome_path_with(Some(&self.config)) {
                    Ok(Some(chrome)) => {
                        browser_config = browser_config.chrome_executable(chrome.as_path());
                    }
                    Ok(None) => {
                        debug!("No Chrome path configured or auto-detected; using chromiumoxide defaults");
                    }
                    Err(e) => return Err(anyhow::anyhow!(e)),
                }
            }
        }

        // Set headless mode
        if !self.browser_config.headless {
            browser_config = browser_config.with_head();
        }

        // Add Chrome args from config
        for arg in &self.browser_config.args {
            browser_config = browser_config.arg(arg);
        }

        // Dynamic opt-in args (AUTOMODUS_NO_PROXY, AUTOMODUS_DISABLE_IPV6) —
        // same set applied by build_browser_config on the LaunchOptions path.
        for arg in crate::modules::browser::launch::dynamic_chrome_args() {
            browser_config = browser_config.arg(arg);
        }

        let user_data_dir = self
            .browser_config
            .user_data_dir
            .to_string_lossy()
            .to_string();

        // Ensure the directory exists
        std::fs::create_dir_all(&user_data_dir)
            .map_err(|e| anyhow::anyhow!("Failed to create user data directory: {}", e))?;

        debug!("Using persistent Chrome profile at: {}", user_data_dir);

        // Fix Chrome crash state (like .NET CrashFix)
        // This prevents the "Chrome didn't shut down correctly" dialog
        self.fix_chrome_crash_state(&user_data_dir);

        // Add essential args for stability
        browser_config = browser_config
            .arg("--no-sandbox")
            .arg("--disable-setuid-sandbox")
            .arg("--disable-dev-shm-usage")
            .arg("--disable-web-security")
            .arg("--disable-features=VizDisplayCompositor")
            .arg("--no-first-run")
            .arg("--disable-default-apps")
            .arg("--disable-background-timer-throttling")
            .arg("--disable-renderer-backgrounding")
            .arg("--disable-backgrounding-occluded-windows")
            .arg("--disable-extensions")
            .arg("--disable-plugins")
            .arg("--disable-gpu")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", user_data_dir));

        let config = browser_config
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build browser config: {}", e))?;

        // Store the user data directory for cleanup
        *self.user_data_dir.lock().await = Some(user_data_dir);

        // Launch browser with timeout
        info!("Launching Chrome browser with chromiumoxide...");

        match tokio::time::timeout(std::time::Duration::from_secs(30), Browser::launch(config))
            .await
        {
            Ok(Ok((browser, mut handler))) => {
                info!("Browser launched successfully with chromiumoxide");

                // Spawn handler task to manage browser process
                tokio::spawn(async move {
                    while let Some(h) = handler.next().await {
                        if let Err(e) = h {
                            tracing::debug!(
                                "Browser handler event error (this is normal): {:?}",
                                e
                            );
                            if e.to_string().contains("connection closed")
                                || e.to_string().contains("broken pipe")
                            {
                                tracing::error!("Critical browser connection error: {:?}", e);
                                break;
                            }
                        }
                    }
                    tracing::debug!("Browser handler task completed");
                });

                // Store browser first
                *self.browser.lock().await = Some(browser);

                // Browser is ready - no automatic navigation
                info!("Browser initialized and ready");

                Ok(())
            }
            Ok(Err(e)) => {
                tracing::error!("Failed to launch browser: {}", e);
                Err(anyhow::anyhow!("Browser initialization failed: {}", e))
            }
            Err(_) => {
                tracing::error!("Browser launch timed out after 30 seconds");
                Err(anyhow::anyhow!(
                    "Browser launch timeout - please ensure Chrome is installed"
                ))
            }
        }
    }

    /// User agent string for browser automation
    fn user_agent() -> &'static str {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
    }

    /// Fix Chrome crash state to prevent "Chrome didn't shut down correctly" dialog
    fn fix_chrome_crash_state(&self, user_data_dir: &str) {
        let profile_dirs = ["Default", "Profile 1"];

        for profile in profile_dirs {
            let preferences_path = format!("{}/{}/Preferences", user_data_dir, profile);

            if let Ok(content) = std::fs::read_to_string(&preferences_path) {
                if content.contains("\"Crashed\"") {
                    debug!("Fixing Chrome crash state in {}", preferences_path);
                    let fixed_content = content.replace("\"Crashed\"", "\"Normal\"");
                    if let Err(e) = std::fs::write(&preferences_path, fixed_content) {
                        tracing::warn!("Failed to fix Chrome crash state: {}", e);
                    } else {
                        debug!("Chrome crash state fixed successfully");
                    }
                }
            }
        }
    }

    /// Clean up any existing Chrome processes
    async fn cleanup_existing_chrome_processes(&self) {
        debug!("Checking for existing Chrome processes...");

        if cfg!(target_os = "windows") {
            let chrome_processes = ["chrome.exe", "msedge.exe", "chromium.exe"];
            for process in chrome_processes {
                let _ = tokio::process::Command::new("taskkill")
                    .args(["/F", "/IM", process])
                    .output()
                    .await;
            }
        } else {
            let chrome_processes = [
                "Google Chrome",
                "chromium-browser",
                "chrome",
                "google-chrome",
                "Chromium",
            ];
            for process in chrome_processes {
                let _ = tokio::process::Command::new("pkill")
                    .args(["-f", process])
                    .output()
                    .await;
            }

            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

            for process in chrome_processes {
                let _ = tokio::process::Command::new("pkill")
                    .args(["-9", "-f", process])
                    .output()
                    .await;
            }
        }

        // Clean up temp directories
        let temp_dir = if cfg!(target_os = "windows") {
            std::env::var("TEMP").unwrap_or_else(|_| {
                std::env::var("TMP").unwrap_or_else(|_| "C:\\Windows\\Temp".to_string())
            })
        } else {
            "/tmp".to_string()
        };

        if let Ok(entries) = std::fs::read_dir(&temp_dir) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.starts_with("chromiumoxide-automodus") {
                        let _ = std::fs::remove_dir_all(entry.path());
                    }
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        debug!("Chrome cleanup completed");
    }

    /// Get or create a page for the specified URL
    pub async fn get_or_create_page(&self, url: &str) -> Result<Page> {
        self.create_new_page(url).await
    }

    /// Page health check timeout (2 seconds)
    const PAGE_HEALTH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

    /// Get the persistent default page
    ///
    /// Includes timeout protection to avoid hanging when browser is unresponsive.
    pub async fn get_default_page(&self) -> Result<Page> {
        // First, try to reuse existing page with timeout-protected check
        {
            let page_guard = self.default_page.lock().await;
            if let Some(ref page) = *page_guard {
                // Use timeout to check if page is still responsive
                let page_check = async { page.url().await };

                match tokio::time::timeout(Self::PAGE_HEALTH_TIMEOUT, page_check).await {
                    Ok(Ok(_)) => {
                        debug!("Reusing existing default page");
                        return Ok(page.clone());
                    }
                    Ok(Err(e)) => {
                        debug!("Existing page is invalid: {}", e);
                    }
                    Err(_) => {
                        tracing::warn!("Page health check timed out - page may be unresponsive");
                    }
                }
            }
        }

        // Clear stale page reference
        *self.default_page.lock().await = None;

        debug!("Creating new default page");
        let page = self.create_new_page("about:blank").await?;
        *self.default_page.lock().await = Some(page.clone());
        Ok(page)
    }

    /// Create a new page for any URL
    async fn create_new_page(&self, url: &str) -> Result<Page> {
        let mut retries = 0;
        while self.browser.lock().await.is_none() && retries < 3 {
            info!(
                "Browser not initialized, attempting initialization (attempt {})",
                retries + 1
            );
            if let Err(e) = self.initialize().await {
                retries += 1;
                if retries >= 3 {
                    return Err(anyhow::anyhow!(
                        "Failed to initialize browser after {} attempts: {}",
                        retries,
                        e
                    ));
                }
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }
            break;
        }

        let browser = self.browser.lock().await;
        let browser = browser
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Browser not initialized"))?;

        let page = browser.new_page(url).await?;
        page.set_user_agent(Self::user_agent()).await?;

        debug!("Created new page and navigated to: {}", url);
        Ok(page)
    }

    /// Check if browser is running (basic check - only verifies handle exists)
    pub async fn is_running(&self) -> bool {
        self.browser.lock().await.is_some()
    }

    /// Check if browser is actually responsive (with timeout)
    ///
    /// This does a real check that the browser can respond to commands.
    /// Use this for health checks rather than `is_running()`.
    pub async fn is_responsive(&self) -> bool {
        if !self.is_running().await {
            return false;
        }

        // Try to get the page with timeout
        match tokio::time::timeout(Self::PAGE_HEALTH_TIMEOUT, self.get_default_page()).await {
            Ok(Ok(page)) => {
                // Try a simple evaluation to verify responsiveness
                match tokio::time::timeout(Self::PAGE_HEALTH_TIMEOUT, page.evaluate("true")).await {
                    Ok(Ok(_)) => true,
                    _ => {
                        tracing::warn!("Browser page not responding to commands");
                        false
                    }
                }
            }
            _ => {
                tracing::warn!("Failed to get browser page for health check");
                false
            }
        }
    }

    /// Force reset the browser state when it's unresponsive
    ///
    /// This clears the browser handle so the next operation will attempt
    /// to reinitialize. Useful when the browser process has died.
    pub async fn force_reset(&self) {
        tracing::warn!("Force resetting browser state");
        *self.default_page.lock().await = None;
        *self.browser.lock().await = None;
    }

    /// Close the browser and clean up resources
    pub async fn close(&self) -> Result<()> {
        info!("Closing browser service");

        *self.default_page.lock().await = None;

        if let Some(mut browser) = self.browser.lock().await.take() {
            if let Err(e) = browser.close().await {
                tracing::error!("Error closing browser: {}", e);
            }
            debug!("Browser closed");
        }

        if let Some(user_data_dir) = self.user_data_dir.lock().await.take() {
            debug!(
                "Preserving user data directory for session persistence: {}",
                user_data_dir
            );
        }

        info!("Browser service closed successfully");
        Ok(())
    }

    /// Take a screenshot of the current page
    ///
    /// Returns PNG image data as bytes. Useful for live feed/monitoring.
    pub async fn screenshot(&self) -> Result<Vec<u8>> {
        let page = self.get_default_page().await?;

        use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;

        let screenshot = page
            .screenshot(
                chromiumoxide::page::ScreenshotParams::builder()
                    .format(CaptureScreenshotFormat::Png)
                    .full_page(false)
                    .build(),
            )
            .await?;

        Ok(screenshot)
    }
}
