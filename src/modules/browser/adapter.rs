//! Browser Adapter
//!
//! Implements BrowserHandle trait using chromiumoxide Page.
//! Supports extended selector patterns: text:, text*:, role:, xpath:, CSS

use async_trait::async_trait;
use chromiumoxide::cdp::browser_protocol::inspector::EventTargetCrashed;
use chromiumoxide::cdp::browser_protocol::network::{
    EventRequestWillBeSent, EventResponseReceived,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::cdp::js_protocol::runtime::EventConsoleApiCalled;
use chromiumoxide::page::Page;
use futures::StreamExt;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::selector::{
    parse_selector, selector_click_js, selector_exists_js, selector_get_attribute_js,
    selector_get_text_js, selector_type_js,
};
use crate::actions::{
    ActionError, BrowserHandle, ConsoleEntry, NetworkEntry, TabInfo,
};

/// Adapter that implements BrowserHandle for chromiumoxide Page
#[derive(Clone)]
pub struct ChromePageAdapter {
    page: Arc<Mutex<Page>>,
    /// All open tabs
    tabs: Arc<Mutex<Vec<Page>>>,
    /// Current tab index
    current_tab: Arc<Mutex<usize>>,
    /// Browser reference for multi-tab operations (optional)
    browser: Option<Arc<Mutex<Option<chromiumoxide::browser::Browser>>>>,
    /// Captured console logs (populated by CDP listener)
    console_logs: Arc<Mutex<Vec<ConsoleEntry>>>,
    /// Captured network requests (populated by CDP listener)
    network_logs: Arc<Mutex<Vec<NetworkEntry>>>,
    /// In-flight requests keyed by request_id (method, url, start_time)
    pending_requests: Arc<Mutex<HashMap<String, PendingRequest>>>,
    /// Whether the CDP console listener is already running
    console_listener_active: Arc<AtomicBool>,
    /// Whether the CDP network listener is already running
    network_listener_active: Arc<AtomicBool>,
    /// Set to true when the browser target crashes
    browser_crashed: Arc<AtomicBool>,
}

/// In-flight request tracked between RequestWillBeSent and ResponseReceived
#[derive(Debug, Clone)]
struct PendingRequest {
    method: String,
    url: String,
    timestamp: std::time::Instant,
}

impl ChromePageAdapter {
    /// Create a new adapter wrapping a chromiumoxide Page
    pub fn new(page: Page) -> Self {
        Self {
            page: Arc::new(Mutex::new(page.clone())),
            tabs: Arc::new(Mutex::new(vec![page])),
            current_tab: Arc::new(Mutex::new(0)),
            browser: None,
            console_logs: Arc::new(Mutex::new(Vec::new())),
            network_logs: Arc::new(Mutex::new(Vec::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            console_listener_active: Arc::new(AtomicBool::new(false)),
            network_listener_active: Arc::new(AtomicBool::new(false)),
            browser_crashed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create an adapter with a browser reference for multi-tab support
    pub fn with_browser(
        page: Page,
        browser: Arc<Mutex<Option<chromiumoxide::browser::Browser>>>,
    ) -> Self {
        Self {
            page: Arc::new(Mutex::new(page.clone())),
            tabs: Arc::new(Mutex::new(vec![page])),
            current_tab: Arc::new(Mutex::new(0)),
            browser: Some(browser),
            console_logs: Arc::new(Mutex::new(Vec::new())),
            network_logs: Arc::new(Mutex::new(Vec::new())),
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            console_listener_active: Arc::new(AtomicBool::new(false)),
            network_listener_active: Arc::new(AtomicBool::new(false)),
            browser_crashed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the underlying page (for direct access if needed)
    pub async fn page(&self) -> tokio::sync::MutexGuard<'_, Page> {
        self.page.lock().await
    }

    /// Get captured console logs
    pub async fn get_console_logs(&self) -> Vec<ConsoleEntry> {
        self.console_logs.lock().await.clone()
    }

    /// Get captured network logs
    pub async fn get_network_logs(&self) -> Vec<NetworkEntry> {
        self.network_logs.lock().await.clone()
    }

    /// Clear captured console logs
    pub async fn clear_console_logs(&self) {
        self.console_logs.lock().await.clear();
    }

    /// Clear captured network logs
    pub async fn clear_network_logs(&self) {
        self.network_logs.lock().await.clear();
    }

    /// Close the underlying browser if this adapter still owns it.
    ///
    /// Used when recovering from a stale/hung adapter so the next launch
    /// does not contend on a live profile directory. Safe to call multiple
    /// times; subsequent calls are no-ops if the browser was already taken.
    pub async fn close_browser(&self) {
        let Some(browser_arc) = self.browser.as_ref() else {
            return;
        };
        let mut guard = browser_arc.lock().await;
        if let Some(mut browser) = guard.take() {
            if let Err(e) = browser.close().await {
                debug!("close_browser: {}", e);
            } else {
                debug!("close_browser: browser closed");
            }
        }
    }

    /// Start the native CDP console listener.
    ///
    /// Enables `Runtime.enable` and subscribes to `EventConsoleApiCalled`.
    /// Incoming console events are pushed into `console_logs` in the background.
    /// Safe to call multiple times — only the first call starts the listener.
    pub async fn start_console_listener(&self) -> Result<(), ActionError> {
        if self.console_listener_active.swap(true, Ordering::SeqCst) {
            return Ok(()); // already running
        }

        let page = self.page.lock().await;

        // Enable the Runtime domain so Chrome emits consoleAPICalled events
        use chromiumoxide::cdp::js_protocol::runtime::EnableParams;
        page.execute(EnableParams {}).await.map_err(|e| {
            ActionError::BrowserError(format!("Failed to enable Runtime domain: {}", e))
        })?;

        // Subscribe to console events
        let mut event_stream = page.event_listener::<EventConsoleApiCalled>().await.map_err(|e| {
            ActionError::BrowserError(format!("Failed to subscribe to console events: {}", e))
        })?;
        drop(page); // release lock before spawning

        let logs = Arc::clone(&self.console_logs);
        let active = Arc::clone(&self.console_listener_active);

        tokio::spawn(async move {
            while let Some(event) = event_stream.next().await {
                let level = event.r#type.as_ref().to_string();
                let message = event
                    .args
                    .iter()
                    .map(|arg| {
                        // Prefer the JSON value, fall back to description
                        if let Some(val) = &arg.value {
                            match val {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            }
                        } else if let Some(desc) = &arg.description {
                            desc.clone()
                        } else {
                            String::from("undefined")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");

                let entry = ConsoleEntry {
                    timestamp: chrono::Utc::now(),
                    level,
                    message,
                };
                debug!("{}", entry.format());
                logs.lock().await.push(entry);
            }
            // Stream ended (page closed, etc.)
            active.store(false, Ordering::SeqCst);
        });

        info!("CDP console listener started");
        Ok(())
    }

    /// Drain captured console logs collected by the CDP listener.
    ///
    /// Returns all entries accumulated since the last drain and clears the buffer.
    /// If the listener hasn't been started yet, returns an empty vec.
    pub async fn capture_console_logs(&self) -> Result<Vec<ConsoleEntry>, ActionError> {
        let mut logs = self.console_logs.lock().await;
        let entries: Vec<ConsoleEntry> = logs.drain(..).collect();
        Ok(entries)
    }

    /// Start the native CDP network listener.
    ///
    /// Enables `Network.enable` and subscribes to `RequestWillBeSent` + `ResponseReceived`.
    /// Correlates request/response pairs by `request_id` to build `NetworkEntry` items
    /// with method, url, status, and duration. Safe to call multiple times.
    pub async fn start_network_listener(&self) -> Result<(), ActionError> {
        if self.network_listener_active.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let page = self.page.lock().await;

        // Enable the Network domain
        use chromiumoxide::cdp::browser_protocol::network::EnableParams;
        page.execute(EnableParams::default()).await.map_err(|e| {
            ActionError::BrowserError(format!("Failed to enable Network domain: {}", e))
        })?;

        // Subscribe to request and response events
        let mut req_stream = page.event_listener::<EventRequestWillBeSent>().await.map_err(|e| {
            ActionError::BrowserError(format!("Failed to subscribe to request events: {}", e))
        })?;
        let mut resp_stream = page.event_listener::<EventResponseReceived>().await.map_err(|e| {
            ActionError::BrowserError(format!("Failed to subscribe to response events: {}", e))
        })?;
        drop(page);

        // Spawn request tracker
        let pending = Arc::clone(&self.pending_requests);
        let req_active = Arc::clone(&self.network_listener_active);
        tokio::spawn(async move {
            while let Some(event) = req_stream.next().await {
                let id = event.request_id.inner().clone();
                let entry = PendingRequest {
                    method: event.request.method.clone(),
                    url: event.request.url.clone(),
                    timestamp: std::time::Instant::now(),
                };
                pending.lock().await.insert(id, entry);
            }
            req_active.store(false, Ordering::SeqCst);
        });

        // Spawn response tracker
        let pending2 = Arc::clone(&self.pending_requests);
        let logs = Arc::clone(&self.network_logs);
        let resp_active = Arc::clone(&self.network_listener_active);
        tokio::spawn(async move {
            while let Some(event) = resp_stream.next().await {
                let id = event.request_id.inner().clone();
                let (method, url, duration_ms) = {
                    let mut map = pending2.lock().await;
                    if let Some(req) = map.remove(&id) {
                        let dur = req.timestamp.elapsed().as_millis() as u64;
                        (req.method, req.url, Some(dur))
                    } else {
                        // Response without tracked request — use response data
                        (String::from("?"), event.response.url.clone(), None)
                    }
                };

                let status = event.response.status as u32;
                let entry = NetworkEntry {
                    timestamp: chrono::Utc::now(),
                    method,
                    url,
                    status: Some(status),
                    duration_ms,
                };
                debug!("{}", entry.format());
                logs.lock().await.push(entry);
            }
            resp_active.store(false, Ordering::SeqCst);
        });

        info!("CDP network listener started");
        Ok(())
    }

    /// Drain captured network logs collected by the CDP listener.
    ///
    /// Returns all entries accumulated since the last drain and clears the buffer.
    pub async fn capture_network_logs(&self) -> Result<Vec<NetworkEntry>, ActionError> {
        let mut logs = self.network_logs.lock().await;
        let entries: Vec<NetworkEntry> = logs.drain(..).collect();
        Ok(entries)
    }

    /// Start a crash detection listener.
    ///
    /// Subscribes to `Inspector.targetCrashed`. When the event fires, sets
    /// `browser_crashed` to true so callers can detect the crash.
    pub async fn start_crash_listener(&self) -> Result<(), ActionError> {
        if self.browser_crashed.load(Ordering::SeqCst) {
            return Ok(()); // already crashed, no point starting
        }

        let page = self.page.lock().await;
        let mut crash_stream = page.event_listener::<EventTargetCrashed>().await.map_err(|e| {
            ActionError::BrowserError(format!("Failed to subscribe to crash events: {}", e))
        })?;
        drop(page);

        let crashed = Arc::clone(&self.browser_crashed);
        tokio::spawn(async move {
            if let Some(_event) = crash_stream.next().await {
                warn!("Browser target crashed!");
                crashed.store(true, Ordering::SeqCst);
            }
        });

        info!("CDP crash listener started");
        Ok(())
    }

    /// Check if the browser is still alive (not crashed).
    pub fn is_browser_alive(&self) -> bool {
        !self.browser_crashed.load(Ordering::SeqCst)
    }

    /// Check browser health and return an error if crashed.
    fn check_browser_health(&self) -> Result<(), ActionError> {
        if self.browser_crashed.load(Ordering::SeqCst) {
            Err(ActionError::BrowserError(
                "Browser has crashed. Restart the browser to continue.".into(),
            ))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl BrowserHandle for ChromePageAdapter {
    async fn goto(&self, url: &str) -> Result<(), ActionError> {
        self.check_browser_health()?;
        debug!("Navigating to: {}", url);
        let page = self.page.lock().await;
        page.goto(url)
            .await
            .map_err(|e| ActionError::BrowserError(format!("Navigation failed: {}", e)))?;

        // Wait for page to load
        page.wait_for_navigation()
            .await
            .map_err(|e| ActionError::BrowserError(format!("Wait for navigation failed: {}", e)))?;

        Ok(())
    }

    async fn click(&self, selector: &str) -> Result<(), ActionError> {
        self.check_browser_health()?;
        debug!("Clicking: {}", selector);
        let page = self.page.lock().await;

        let parsed = parse_selector(selector);

        // For CSS selectors, use native chromiumoxide for better reliability
        if let Some(css) = parsed.as_css() {
            let element = page
                .find_element(css)
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

            element
                .click()
                .await
                .map_err(|e| ActionError::BrowserError(format!("Click failed: {}", e)))?;
        } else {
            // For extended selectors (text:, role:, xpath:), use JavaScript
            let js = selector_click_js(&parsed);
            page.evaluate(js.as_str())
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;
        }

        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str, clear: bool) -> Result<(), ActionError> {
        self.check_browser_health()?;
        debug!("Typing into: {}", selector);
        let page = self.page.lock().await;

        let parsed = parse_selector(selector);

        // For CSS selectors, use native chromiumoxide
        if let Some(css) = parsed.as_css() {
            let element = page
                .find_element(css)
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

            if clear {
                // Clear existing content by triple-clicking to select all, then type
                element.click().await.ok();
                element.click().await.ok();
                element.click().await.ok(); // Triple click to select all
                                            // Small delay
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }

            element
                .type_str(text)
                .await
                .map_err(|e| ActionError::BrowserError(format!("Type failed: {}", e)))?;
        } else {
            // For extended selectors, use JavaScript
            let js = selector_type_js(&parsed, text, clear);
            page.evaluate(js.as_str())
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;
        }

        Ok(())
    }

    async fn get_text(&self, selector: &str) -> Result<String, ActionError> {
        self.check_browser_health()?;
        debug!("Getting text from: {}", selector);
        let page = self.page.lock().await;

        let parsed = parse_selector(selector);

        // For CSS selectors, use native chromiumoxide
        if let Some(css) = parsed.as_css() {
            let element = page
                .find_element(css)
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

            let text = element
                .inner_text()
                .await
                .map_err(|e| ActionError::BrowserError(format!("Get text failed: {}", e)))?
                .unwrap_or_default();

            Ok(text)
        } else {
            // For extended selectors, use JavaScript
            let js = selector_get_text_js(&parsed);
            let result = page
                .evaluate(js.as_str())
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

            Ok(result
                .value()
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string())
        }
    }

    async fn get_attribute(
        &self,
        selector: &str,
        attr: &str,
    ) -> Result<Option<String>, ActionError> {
        debug!("Getting attribute {} from: {}", attr, selector);
        let page = self.page.lock().await;

        let parsed = parse_selector(selector);

        // For CSS selectors, use native chromiumoxide
        if let Some(css) = parsed.as_css() {
            let element = page
                .find_element(css)
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

            let value = element
                .attribute(attr)
                .await
                .map_err(|e| ActionError::BrowserError(format!("Get attribute failed: {}", e)))?;

            Ok(value)
        } else {
            // For extended selectors, use JavaScript
            let js = selector_get_attribute_js(&parsed, attr);
            let result = page
                .evaluate(js.as_str())
                .await
                .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

            Ok(result
                .value()
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()))
        }
    }

    async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!(
            "Waiting for element: {} (timeout: {}ms)",
            selector, timeout_ms
        );
        let page = self.page.lock().await;

        let duration = Duration::from_millis(timeout_ms);
        let start = std::time::Instant::now();
        let parsed = parse_selector(selector);

        // For CSS selectors, use native chromiumoxide
        if let Some(css) = parsed.as_css() {
            loop {
                match page.find_element(css).await {
                    Ok(_) => return Ok(()),
                    Err(_) if start.elapsed() < duration => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(e) => {
                        return Err(ActionError::Timeout(format!(
                            "Element {} not found after {}ms: {}",
                            selector, timeout_ms, e
                        )));
                    }
                }
            }
        } else {
            // For extended selectors, use JavaScript polling
            let js = selector_exists_js(&parsed);
            loop {
                match page.evaluate(js.as_str()).await {
                    Ok(result) if result.value().and_then(|v| v.as_bool()).unwrap_or(false) => {
                        return Ok(());
                    }
                    _ if start.elapsed() < duration => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    _ => {
                        return Err(ActionError::Timeout(format!(
                            "Element {} not found after {}ms",
                            selector, timeout_ms
                        )));
                    }
                }
            }
        }
    }

    async fn wait_for_hidden(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!(
            "Waiting for element to disappear: {} (timeout: {}ms)",
            selector, timeout_ms
        );
        let page = self.page.lock().await;

        let duration = Duration::from_millis(timeout_ms);
        let start = std::time::Instant::now();
        let parsed = parse_selector(selector);

        // For CSS selectors, use native chromiumoxide
        if let Some(css) = parsed.as_css() {
            loop {
                match page.find_element(css).await {
                    // Element still exists, continue waiting
                    Ok(_) if start.elapsed() < duration => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    // Element still exists but timeout reached
                    Ok(_) => {
                        return Err(ActionError::Timeout(format!(
                            "Element {} still visible after {}ms",
                            selector, timeout_ms
                        )));
                    }
                    // Element not found - success!
                    Err(_) => return Ok(()),
                }
            }
        } else {
            // For extended selectors, use JavaScript polling
            let js = selector_exists_js(&parsed);
            loop {
                match page.evaluate(js.as_str()).await {
                    // Element exists (returns true), continue waiting
                    Ok(result) if result.value().and_then(|v| v.as_bool()).unwrap_or(false) => {
                        if start.elapsed() >= duration {
                            return Err(ActionError::Timeout(format!(
                                "Element {} still visible after {}ms",
                                selector, timeout_ms
                            )));
                        }
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    // Element not found (returns false) - success!
                    _ => return Ok(()),
                }
            }
        }
    }

    async fn wait_for_url(&self, condition: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!("Waiting for URL: {} (timeout: {}ms)", condition, timeout_ms);
        let page = self.page.lock().await;

        let duration = Duration::from_millis(timeout_ms);
        let start = std::time::Instant::now();

        loop {
            let current_url = page
                .url()
                .await
                .map_err(|e| ActionError::BrowserError(e.to_string()))?
                .map(|u| u.to_string())
                .unwrap_or_default();

            // Check if URL matches condition (contains, starts with, etc.)
            let matches = if condition.starts_with("contains:") {
                current_url.contains(&condition[9..])
            } else if condition.starts_with("starts:") {
                current_url.starts_with(&condition[7..])
            } else if condition.starts_with("ends:") {
                current_url.ends_with(&condition[5..])
            } else {
                current_url.contains(condition) || current_url == condition
            };

            if matches {
                return Ok(());
            }

            if start.elapsed() >= duration {
                return Err(ActionError::Timeout(format!(
                    "URL condition '{}' not met after {}ms. Current: {}",
                    condition, timeout_ms, current_url
                )));
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>, ActionError> {
        self.check_browser_health()?;
        debug!("Taking screenshot (full_page: {})", full_page);
        let page = self.page.lock().await;

        let params = chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .capture_beyond_viewport(full_page)
            .build();

        let screenshot = page
            .execute(params)
            .await
            .map_err(|e| ActionError::BrowserError(format!("Screenshot failed: {}", e)))?;

        let data = screenshot.result.data;
        use base64::{engine::general_purpose::STANDARD, Engine};
        let bytes = STANDARD
            .decode(&data)
            .map_err(|e| ActionError::Internal(format!("Failed to decode screenshot: {}", e)))?;

        Ok(bytes)
    }

    async fn eval(&self, script: &str) -> Result<Value, ActionError> {
        self.check_browser_health()?;
        debug!("Evaluating script");
        let page = self.page.lock().await;

        let result = page
            .evaluate(script)
            .await
            .map_err(|e| ActionError::BrowserError(format!("Eval failed: {}", e)))?;

        // Try to convert to JSON value
        let value = result.value().cloned().unwrap_or(Value::Null);

        Ok(value)
    }

    async fn current_url(&self) -> Result<String, ActionError> {
        let page = self.page.lock().await;

        let url = page
            .url()
            .await
            .map_err(|e| ActionError::BrowserError(format!("Get URL failed: {}", e)))?
            .map(|u| u.to_string())
            .unwrap_or_default();

        Ok(url)
    }

    async fn back(&self) -> Result<(), ActionError> {
        // Navigate back via JavaScript
        let page = self.page.lock().await;

        page.evaluate("window.history.back()")
            .await
            .map_err(|e| ActionError::BrowserError(format!("Back navigation failed: {}", e)))?;

        // Wait a moment for navigation
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(())
    }

    async fn forward(&self) -> Result<(), ActionError> {
        // Forward navigation via JavaScript
        let page = self.page.lock().await;

        page.evaluate("window.history.forward()")
            .await
            .map_err(|e| ActionError::BrowserError(format!("Forward navigation failed: {}", e)))?;

        // Wait a moment for navigation
        tokio::time::sleep(Duration::from_millis(500)).await;

        Ok(())
    }

    async fn reload(&self) -> Result<(), ActionError> {
        let page = self.page.lock().await;

        page.reload()
            .await
            .map_err(|e| ActionError::BrowserError(format!("Reload failed: {}", e)))?;

        Ok(())
    }

    async fn new_tab(&self, url: Option<&str>) -> Result<usize, ActionError> {
        let browser_ref = self.browser.as_ref().ok_or_else(|| {
            ActionError::Internal("Multi-tab requires browser reference. Use with_browser() constructor.".into())
        })?;
        let browser_guard = browser_ref.lock().await;
        let browser = browser_guard.as_ref().ok_or_else(|| {
            ActionError::Internal("Browser not running".into())
        })?;

        let target_url = url.unwrap_or("about:blank");
        let new_page = browser.new_page(target_url).await.map_err(|e| {
            ActionError::BrowserError(format!("Failed to create new tab: {}", e))
        })?;
        drop(browser_guard);

        let new_index = {
            let mut tabs = self.tabs.lock().await;
            tabs.push(new_page.clone());
            tabs.len() - 1
        };

        *self.current_tab.lock().await = new_index;
        *self.page.lock().await = new_page;

        info!("Opened new tab {} at {}", new_index, target_url);
        Ok(new_index)
    }

    async fn switch_tab(&self, index: usize) -> Result<(), ActionError> {
        let page = {
            let tabs = self.tabs.lock().await;
            tabs.get(index).cloned().ok_or_else(|| {
                ActionError::InvalidParameter(format!(
                    "Tab index {} out of range (have {} tabs)",
                    index,
                    tabs.len()
                ))
            })?
        };

        *self.current_tab.lock().await = index;
        *self.page.lock().await = page;

        info!("Switched to tab {}", index);
        Ok(())
    }

    async fn close_tab(&self, index: usize) -> Result<(), ActionError> {
        let current = *self.current_tab.lock().await;

        let (new_page, new_index) = {
            let mut tabs = self.tabs.lock().await;
            if tabs.len() <= 1 {
                return Err(ActionError::Internal("Cannot close the last tab".into()));
            }
            if index >= tabs.len() {
                return Err(ActionError::InvalidParameter(format!(
                    "Tab index {} out of range (have {} tabs)",
                    index,
                    tabs.len()
                )));
            }
            tabs.remove(index);
            let new_index = if current >= tabs.len() {
                tabs.len() - 1
            } else if current > index {
                current - 1
            } else {
                current
            };
            (tabs[new_index].clone(), new_index)
        };

        *self.current_tab.lock().await = new_index;
        *self.page.lock().await = new_page;

        info!("Closed tab {}, now on tab {}", index, new_index);
        Ok(())
    }

    async fn tab_count(&self) -> Result<usize, ActionError> {
        Ok(self.tabs.lock().await.len())
    }

    async fn list_tabs(&self) -> Result<Vec<TabInfo>, ActionError> {
        let current = *self.current_tab.lock().await;
        let tabs = self.tabs.lock().await;
        let mut result = Vec::with_capacity(tabs.len());
        for (i, tab_page) in tabs.iter().enumerate() {
            let url = tab_page
                .url()
                .await
                .map_err(|e| ActionError::BrowserError(format!("Failed to get tab URL: {}", e)))?
                .map(|u| u.to_string())
                .unwrap_or_else(|| "about:blank".to_string());
            result.push(TabInfo {
                index: i,
                url,
                active: i == current,
            });
        }
        Ok(result)
    }

    async fn pdf(&self) -> Result<Vec<u8>, ActionError> {
        debug!("Generating PDF");
        let page = self.page.lock().await;

        let params = chromiumoxide::cdp::browser_protocol::page::PrintToPdfParams::default();

        let pdf_result = page
            .execute(params)
            .await
            .map_err(|e| ActionError::BrowserError(format!("PDF generation failed: {}", e)))?;

        let data = pdf_result.result.data;
        use base64::{engine::general_purpose::STANDARD, Engine};
        let bytes = STANDARD
            .decode(&data)
            .map_err(|e| ActionError::Internal(format!("Failed to decode PDF data: {}", e)))?;

        Ok(bytes)
    }

    async fn set_file_input_files(
        &self,
        selector: &str,
        file_paths: Vec<String>,
    ) -> Result<(), ActionError> {
        use chromiumoxide::cdp::browser_protocol::dom::{
            GetDocumentParams, QuerySelectorParams, SetFileInputFilesParams,
        };

        debug!(
            "Setting files on input: {} ({} files)",
            selector,
            file_paths.len()
        );
        let page = self.page.lock().await;

        // Get the document root
        let doc = page
            .execute(GetDocumentParams::default())
            .await
            .map_err(|e| ActionError::BrowserError(format!("Failed to get document: {}", e)))?;

        let root_id = doc.result.root.node_id;

        // Query for the file input element
        let query_result = page
            .execute(
                QuerySelectorParams::builder()
                    .node_id(root_id)
                    .selector(selector.to_string())
                    .build()
                    .map_err(|e| {
                        ActionError::InvalidParameter(format!("Invalid selector: {}", e))
                    })?,
            )
            .await
            .map_err(|e| {
                ActionError::ElementNotFound(format!(
                    "Failed to find element '{}': {}",
                    selector, e
                ))
            })?;

        let node_id = query_result.result.node_id;
        if *node_id.inner() == 0 {
            return Err(ActionError::ElementNotFound(format!(
                "File input element not found: {}",
                selector
            )));
        }

        // Set the files on the input element
        page.execute(
            SetFileInputFilesParams::builder()
                .files(file_paths.clone())
                .node_id(node_id)
                .build()
                .map_err(|e| {
                    ActionError::InvalidParameter(format!("Failed to build params: {}", e))
                })?,
        )
        .await
        .map_err(|e| ActionError::BrowserError(format!("Failed to set files: {}", e)))?;

        debug!(
            "Successfully set {} files on {}",
            file_paths.len(),
            selector
        );
        Ok(())
    }

    async fn set_file_chooser_intercept(&self, enabled: bool) -> Result<(), ActionError> {
        use chromiumoxide::cdp::browser_protocol::page::SetInterceptFileChooserDialogParams;

        debug!("Setting file chooser interception: {}", enabled);
        let page = self.page.lock().await;

        page.execute(
            SetInterceptFileChooserDialogParams::builder()
                .enabled(enabled)
                .build()
                .map_err(|e| {
                    ActionError::InvalidParameter(format!(
                        "Failed to build intercept params: {}",
                        e
                    ))
                })?,
        )
        .await
        .map_err(|e| {
            ActionError::BrowserError(format!("Failed to set file chooser intercept: {}", e))
        })?;

        Ok(())
    }

    async fn upload_via_file_chooser(
        &self,
        trigger_selector: Option<&str>,
        file_paths: Vec<String>,
        _timeout_ms: u64, // Reserved for future timeout implementation
    ) -> Result<(), ActionError> {
        use chromiumoxide::cdp::browser_protocol::dom::{
            GetDocumentParams, QuerySelectorParams, SetFileInputFilesParams,
        };
        use chromiumoxide::cdp::browser_protocol::page::SetInterceptFileChooserDialogParams;

        info!(
            "Upload via file chooser: trigger={:?}, files={}",
            trigger_selector,
            file_paths.len()
        );

        let page = self.page.lock().await;

        // Enable file chooser interception to prevent native dialog
        info!("Enabling file chooser interception...");
        page.execute(
            SetInterceptFileChooserDialogParams::builder()
                .enabled(true)
                .build()
                .map_err(|e| {
                    ActionError::InvalidParameter(format!(
                        "Failed to build intercept params: {}",
                        e
                    ))
                })?,
        )
        .await
        .map_err(|e| {
            ActionError::BrowserError(format!("Failed to enable file chooser intercept: {}", e))
        })?;

        // If trigger_selector provided, click it to trigger the file chooser
        if let Some(selector) = trigger_selector {
            info!("Clicking trigger element: {}", selector);

            // Use JavaScript to click the element - this triggers the file input
            let js = format!(
                r#"(function() {{
                    const el = document.querySelector({});
                    if (!el) {{
                        throw new Error('Trigger element not found: ' + {});
                    }}
                    el.click();
                    return true;
                }})()"#,
                serde_json::to_string(selector).unwrap(),
                serde_json::to_string(selector).unwrap()
            );

            page.evaluate(js).await.map_err(|e| {
                ActionError::ElementNotFound(format!(
                    "Failed to click trigger '{}': {}",
                    selector, e
                ))
            })?;

            info!("Clicked trigger element");
        }

        // Wait a moment for file input to be created in the DOM
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Try to find and set files on file input elements using multiple fallback selectors
        let file_input_selectors = [
            r#"input[accept="image/*,video/mp4,video/3gpp,video/quicktime"]"#,
            r#"input[accept="*"]"#,
            r#"input[accept="image/*"]"#,
            r#"input[type="file"]"#,
            r#"body > input[type="file"]"#,
            r#"body > input"#,
        ];

        // Get document root
        let doc = page
            .execute(GetDocumentParams::default())
            .await
            .map_err(|e| ActionError::BrowserError(format!("Failed to get document: {}", e)))?;

        let root_id = doc.result.root.node_id;

        let mut success = false;
        let mut last_error = String::new();

        for selector in file_input_selectors {
            info!("Trying file input selector: {}", selector);

            match page
                .execute(
                    QuerySelectorParams::builder()
                        .node_id(root_id)
                        .selector(selector.to_string())
                        .build()
                        .unwrap(),
                )
                .await
            {
                Ok(query_result) => {
                    let node_id = query_result.result.node_id;
                    if *node_id.inner() != 0 {
                        info!(
                            "Found file input with selector: {}, node_id: {:?}",
                            selector, node_id
                        );

                        // Set files on the input
                        match page
                            .execute(
                                SetFileInputFilesParams::builder()
                                    .files(file_paths.clone())
                                    .node_id(node_id)
                                    .build()
                                    .unwrap(),
                            )
                            .await
                        {
                            Ok(_) => {
                                info!(
                                    "Successfully set {} files via selector: {}",
                                    file_paths.len(),
                                    selector
                                );
                                success = true;
                                break;
                            }
                            Err(e) => {
                                last_error =
                                    format!("Failed to set files on '{}': {}", selector, e);
                                warn!("{}", last_error);
                            }
                        }
                    }
                }
                Err(e) => {
                    last_error = format!("Selector '{}' not found: {}", selector, e);
                }
            }
        }

        // Disable file chooser interception
        info!("Disabling file chooser interception...");
        let _ = page
            .execute(
                SetInterceptFileChooserDialogParams::builder()
                    .enabled(false)
                    .build()
                    .unwrap(),
            )
            .await;

        if success {
            info!("Successfully uploaded {} files", file_paths.len());
            Ok(())
        } else {
            // Try alternative: use JS to find and log file inputs
            let debug_js = r#"
                (function() {
                    const inputs = document.querySelectorAll('input[type="file"], body > input');
                    const results = [];
                    inputs.forEach((inp, i) => {
                        results.push({
                            index: i,
                            tagName: inp.tagName,
                            type: inp.type,
                            accept: inp.accept,
                            id: inp.id,
                            className: inp.className,
                            parentTag: inp.parentElement?.tagName
                        });
                    });
                    return { count: inputs.length, inputs: results };
                })()
            "#;

            let debug_result = page.evaluate(debug_js).await;
            warn!("File input debug: {:?}", debug_result);

            Err(ActionError::ElementNotFound(format!(
                "No working file input found. Last error: {}",
                last_error
            )))
        }
    }

    // --- Observability: forward to the inherent CDP implementations ---

    async fn start_console_listener(&self) -> Result<(), ActionError> {
        ChromePageAdapter::start_console_listener(self).await
    }

    async fn start_network_listener(&self) -> Result<(), ActionError> {
        ChromePageAdapter::start_network_listener(self).await
    }

    async fn start_crash_listener(&self) -> Result<(), ActionError> {
        ChromePageAdapter::start_crash_listener(self).await
    }

    async fn get_console_logs(&self) -> Vec<ConsoleEntry> {
        ChromePageAdapter::get_console_logs(self).await
    }

    async fn get_network_logs(&self) -> Vec<NetworkEntry> {
        ChromePageAdapter::get_network_logs(self).await
    }

    async fn capture_console_logs(&self) -> Result<Vec<ConsoleEntry>, ActionError> {
        ChromePageAdapter::capture_console_logs(self).await
    }

    async fn capture_network_logs(&self) -> Result<Vec<NetworkEntry>, ActionError> {
        ChromePageAdapter::capture_network_logs(self).await
    }

    async fn clear_console_logs(&self) {
        ChromePageAdapter::clear_console_logs(self).await
    }

    async fn clear_network_logs(&self) {
        ChromePageAdapter::clear_network_logs(self).await
    }

    fn is_browser_alive(&self) -> bool {
        ChromePageAdapter::is_browser_alive(self)
    }
}
