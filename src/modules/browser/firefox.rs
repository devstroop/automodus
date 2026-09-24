//! Firefox adapter
//!
//! Implements [`BrowserHandle`] on top of rustenium's WebDriver BiDi
//! [`FirefoxBrowser`]. Firefox exposes BiDi natively (no geckodriver).
//!
//! DOM interaction goes through BiDi `script.evaluate` using the shared
//! selector JS helpers so extended selectors (`text:`, `role:`, `xpath:`)
//! work the same as on Chromium. Chromium-only capabilities (PDF, file
//! chooser interception) return [`ActionError::Unsupported`].

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use rustenium::browsers::{
    BidiBrowser, BrowserScreenshotOptionsBuilder, EvaluateScriptOptionsBuilder,
    FindNodesOptionsBuilder, FirefoxConfig, FirefoxLaunchMode, NavigateOptionsBuilder,
};
use rustenium::domain::context::BrowsingContext as DomainBrowsingContext;
use rustenium_bidi_definitions::browsing_context::types::{
    CreateType, CssLocator, CssLocatorType, Locator, ReadinessState, XPathLocator, XPathLocatorType,
};
use rustenium_bidi_definitions::script::types::{
    ContextTarget, PrimitiveProtocolValue, RemoteValue,
};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, info};

use super::selector::{
    parse_selector, selector_click_js, selector_exists_js, selector_get_attribute_js,
    selector_get_text_js, selector_type_js, SelectorType,
};
use crate::actions::{
    ActionError, BrowserCapabilities, BrowserHandle, ConsoleEntry, NetworkEntry, TabInfo,
};

type BidiContext = rustenium_bidi_definitions::browsing_context::types::BrowsingContext;

/// Pick a free TCP port on 127.0.0.1.
fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to bind free port: {}", e))?;
    listener
        .local_addr()
        .map(|a| a.port())
        .map_err(|e| format!("Failed to read local port: {}", e))
}

/// Poll until `host:port` accepts a TCP connection (or timeout).
async fn wait_for_port(host: &str, port: u16, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    let addr = format!("{}:{}", host, port);
    loop {
        match tokio::net::TcpStream::connect(&addr).await {
            Ok(_) => return Ok(()),
            Err(e) if Instant::now() < deadline => {
                tracing::debug!("Waiting for Firefox port {}: {}", addr, e);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => {
                return Err(format!(
                    "Firefox remote-debugging port {} did not open within {:?}: {}",
                    addr, timeout, e
                ));
            }
        }
    }
}

/// Adapter that implements BrowserHandle for rustenium FirefoxBrowser (BiDi).
#[derive(Clone)]
pub struct FirefoxPageAdapter {
    browser: Arc<Mutex<Option<rustenium::browsers::FirefoxBrowser>>>,
    /// Tracked tab context ids (index 0 is the initial tab).
    tabs: Arc<Mutex<Vec<BidiContext>>>,
    current_tab: Arc<Mutex<usize>>,
    closed: Arc<AtomicBool>,
}

impl FirefoxPageAdapter {
    /// Wrap a running FirefoxBrowser with its initial browsing context.
    pub fn new(browser: rustenium::browsers::FirefoxBrowser, initial: BidiContext) -> Self {
        Self {
            browser: Arc::new(Mutex::new(Some(browser))),
            tabs: Arc::new(Mutex::new(vec![initial])),
            current_tab: Arc::new(Mutex::new(0)),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Launch Firefox via BiDi and return a ready adapter.
    ///
    /// Spawns Firefox ourselves and waits until the remote-debugging port
    /// accepts TCP, then attaches with [`FirefoxLaunchMode::Remote`].
    /// rustenium's built-in `SpawnAndAttach` only waits 500ms with three
    /// short retries — too aggressive for cold Firefox starts (~1s).
    pub async fn launch(
        firefox_path: PathBuf,
        profile_dir: PathBuf,
        headless: bool,
        extra_args: &[String],
    ) -> Result<Self, String> {
        std::fs::create_dir_all(&profile_dir)
            .map_err(|e| format!("Failed to create Firefox profile dir: {}", e))?;

        let port = free_port()?;
        let mut flags: Vec<String> = vec![
            format!("--remote-debugging-port={}", port),
            "--profile".to_string(),
            profile_dir.to_string_lossy().into_owned(),
            "--no-remote".to_string(),
        ];
        if headless {
            flags.push("--headless".to_string());
        }
        flags.extend(extra_args.iter().cloned());

        info!(
            path = %firefox_path.display(),
            headless,
            port,
            "Launching Firefox (WebDriver BiDi)"
        );

        let mut child = std::process::Command::new(&firefox_path)
            .args(&flags)
            .env("MOZ_LAUNCHER_PROCESS", "0")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("Failed to spawn Firefox at {}: {}", firefox_path.display(), e))?;

        if let Err(e) = wait_for_port("127.0.0.1", port, Duration::from_secs(15)).await {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }

        // Remote mode: rustenium only connects (we already spawned).
        // close() still kills by remote_debugging_port, so our child is reaped.
        let config = FirefoxConfig {
            host: Some("127.0.0.1".to_string()),
            launch_mode: FirefoxLaunchMode::Remote(port),
            remote_debugging_port: Some(port),
            firefox_executable_path: Some(firefox_path.to_string_lossy().into_owned()),
            profile_dir: Some(profile_dir.to_string_lossy().into_owned()),
            browser_flags: None,
            ..FirefoxConfig::default()
        };

        let mut browser = rustenium::browsers::firefox(Some(config)).await;

        // Existing tabs do not re-fire contextCreated after subscribe; if the
        // driver has no active context, open one so we always have a target.
        let initial = match browser.get_active_context_id() {
            Ok(ctx) => ctx,
            Err(_) => {
                let domain = browser.create_context(false).await.map_err(|e| {
                    let _ = child.kill();
                    format!("Firefox connected but no browsing context: {}", e)
                })?;
                domain.id().clone()
            }
        };

        info!("Firefox launched successfully");
        Ok(Self::new(browser, initial))
    }

    /// Close the underlying browser if this adapter still owns it.
    pub async fn close_browser(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let mut guard = self.browser.lock().await;
        if let Some(browser) = guard.take() {
            if let Err(e) = browser.close().await {
                debug!("close_browser: {}", e);
            } else {
                debug!("close_browser: firefox closed");
            }
        }
    }

    fn ensure_open(&self) -> Result<(), ActionError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ActionError::BrowserError(
                "Browser has been closed".into(),
            ));
        }
        Ok(())
    }

    async fn lock_browser(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<rustenium::browsers::FirefoxBrowser>>, ActionError>
    {
        self.ensure_open()?;
        let guard = self.browser.lock().await;
        if guard.is_none() {
            return Err(ActionError::BrowserError(
                "Firefox is not running".into(),
            ));
        }
        Ok(guard)
    }

    async fn current_context(&self) -> Result<BidiContext, ActionError> {
        let idx = *self.current_tab.lock().await;
        let tabs = self.tabs.lock().await;
        tabs.get(idx).cloned().ok_or_else(|| {
            ActionError::Internal(format!(
                "Tab index {} out of range (have {} tabs)",
                idx,
                tabs.len()
            ))
        })
    }

    async fn eval_in(&self, context: &BidiContext, expr: String) -> Result<Value, ActionError> {
        let mut guard = self.lock_browser().await?;
        let browser = guard.as_mut().expect("checked above");
        let options = EvaluateScriptOptionsBuilder::default()
            .target(ContextTarget::new(context.clone()))
            .build();
        let result = browser
            .evaluate_script_with_options(expr, true, options)
            .await
            .map_err(|e| ActionError::BrowserError(format!("Eval failed: {}", e)))?;
        Ok(remote_value_to_json(&result.result))
    }

    async fn eval_str(&self, context: &BidiContext, expr: String) -> Result<String, ActionError> {
        let v = self.eval_in(context, expr).await?;
        Ok(match v {
            Value::String(s) => s,
            Value::Null => String::new(),
            other => other.to_string(),
        })
    }

    async fn navigate_to(&self, context: &BidiContext, url: &str) -> Result<(), ActionError> {
        let mut guard = self.lock_browser().await?;
        let browser = guard.as_mut().expect("checked above");
        let options = NavigateOptionsBuilder::default()
            .wait(ReadinessState::Complete)
            .context_id(context.clone())
            .build();
        browser
            .navigate_with_options(url, options)
            .await
            .map_err(|e| ActionError::BrowserError(format!("Navigation failed: {}", e)))?;
        Ok(())
    }

    async fn find_exists(&self, locator: Locator, context: &BidiContext) -> Result<bool, ActionError> {
        let mut guard = self.lock_browser().await?;
        let browser = guard.as_mut().expect("checked above");
        let options = FindNodesOptionsBuilder::default()
            .context_id(context.clone())
            .max_node_count(1u64)
            .build();
        let nodes = browser
            .find_nodes_with_options(locator, options)
            .await
            .map_err(|e| ActionError::BrowserError(e.to_string()))?;
        Ok(!nodes.is_empty())
    }
}

/// Convert a BiDi RemoteValue into serde_json::Value.
fn remote_value_to_json(rv: &RemoteValue) -> Value {
    match rv {
        RemoteValue::PrimitiveProtocolValue(p) => match p {
            PrimitiveProtocolValue::StringValue(s) => Value::String(s.value.clone()),
            PrimitiveProtocolValue::NumberValue(n) => n.value.clone(),
            PrimitiveProtocolValue::BooleanValue(b) => Value::Bool(b.value),
            PrimitiveProtocolValue::NullValue(_) | PrimitiveProtocolValue::UndefinedValue(_) => {
                Value::Null
            }
            PrimitiveProtocolValue::BigIntValue(b) => Value::String(b.value.clone()),
        },
        RemoteValue::ArrayRemoteValue(a) => match &a.value {
            Some(list) => Value::Array(list.inner().iter().map(remote_value_to_json).collect()),
            None => Value::Array(vec![]),
        },
        RemoteValue::ObjectRemoteValue(o) => match &o.value {
            Some(mapping) => {
                let mut map = serde_json::Map::new();
                for entry in mapping.inner() {
                    if entry.len() >= 2 {
                        let key = match &entry[0] {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        map.insert(key, entry[1].clone());
                    }
                }
                Value::Object(map)
            }
            None => Value::Object(serde_json::Map::new()),
        },
        // Handles / non-serializable references — re-evaluate a concrete
        // expression if the workflow needs the underlying data.
        _ => Value::Null,
    }
}

/// Build a BiDi Locator for a CSS or XPath selector.
fn selector_to_locator(selector: &SelectorType) -> Option<Locator> {
    match selector {
        SelectorType::Css(css) => Some(Locator::CssLocator(CssLocator::new(
            CssLocatorType::Css,
            css.clone(),
        ))),
        SelectorType::XPath(xpath) => Some(Locator::XPathLocator(XPathLocator::new(
            XPathLocatorType::Xpath,
            xpath.clone(),
        ))),
        // text:/text*:/role: go through the shared JS helpers instead.
        _ => None,
    }
}

#[async_trait]
impl BrowserHandle for FirefoxPageAdapter {
    fn capabilities(&self) -> BrowserCapabilities {
        BrowserCapabilities::FIREFOX
    }

    async fn goto(&self, url: &str) -> Result<(), ActionError> {
        debug!("Navigating to: {}", url);
        let ctx = self.current_context().await?;
        self.navigate_to(&ctx, url).await
    }

    async fn click(&self, selector: &str) -> Result<(), ActionError> {
        debug!("Clicking: {}", selector);
        let ctx = self.current_context().await?;
        let parsed = parse_selector(selector);
        // Prefer native locate for CSS/XPath so ElementNotFound is accurate;
        // still fire the shared JS click so behavior matches Chromium.
        if let Some(locator) = selector_to_locator(&parsed) {
            if !self.find_exists(locator, &ctx).await? {
                return Err(ActionError::ElementNotFound(selector.to_string()));
            }
        }
        let js = selector_click_js(&parsed);
        self.eval_in(&ctx, js).await?;
        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str, clear: bool) -> Result<(), ActionError> {
        debug!("Typing into: {}", selector);
        let ctx = self.current_context().await?;
        let parsed = parse_selector(selector);
        let js = selector_type_js(&parsed, text, clear);
        self.eval_in(&ctx, js).await?;
        Ok(())
    }

    async fn get_text(&self, selector: &str) -> Result<String, ActionError> {
        debug!("Getting text from: {}", selector);
        let ctx = self.current_context().await?;
        let parsed = parse_selector(selector);
        let js = selector_get_text_js(&parsed);
        self.eval_str(&ctx, js).await
    }

    async fn get_attribute(
        &self,
        selector: &str,
        attr: &str,
    ) -> Result<Option<String>, ActionError> {
        debug!("Getting attribute {} from: {}", attr, selector);
        let ctx = self.current_context().await?;
        let parsed = parse_selector(selector);
        let js = selector_get_attribute_js(&parsed, attr);
        let v = self.eval_in(&ctx, js).await?;
        Ok(match v {
            Value::Null => None,
            Value::String(s) => Some(s),
            other => Some(other.to_string()),
        })
    }

    async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!("Waiting for element: {} (timeout: {}ms)", selector, timeout_ms);
        let ctx = self.current_context().await?;
        let parsed = parse_selector(selector);
        let duration = Duration::from_millis(timeout_ms);
        let start = Instant::now();

        if let Some(locator) = selector_to_locator(&parsed) {
            loop {
                if self.find_exists(locator.clone(), &ctx).await? {
                    return Ok(());
                }
                if start.elapsed() >= duration {
                    return Err(ActionError::Timeout(format!(
                        "Element {} not found after {}ms",
                        selector, timeout_ms
                    )));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        let js = selector_exists_js(&parsed);
        loop {
            let exists = self
                .eval_in(&ctx, js.clone())
                .await
                .map(|v| v.as_bool().unwrap_or(false))
                .unwrap_or(false);
            if exists {
                return Ok(());
            }
            if start.elapsed() >= duration {
                return Err(ActionError::Timeout(format!(
                    "Element {} not found after {}ms",
                    selector, timeout_ms
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn wait_for_hidden(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!(
            "Waiting for element to disappear: {} (timeout: {}ms)",
            selector, timeout_ms
        );
        let ctx = self.current_context().await?;
        let parsed = parse_selector(selector);
        let duration = Duration::from_millis(timeout_ms);
        let start = Instant::now();
        let js = selector_exists_js(&parsed);

        loop {
            let exists = self
                .eval_in(&ctx, js.clone())
                .await
                .map(|v| v.as_bool().unwrap_or(false))
                .unwrap_or(false);
            if !exists {
                return Ok(());
            }
            if start.elapsed() >= duration {
                return Err(ActionError::Timeout(format!(
                    "Element {} still visible after {}ms",
                    selector, timeout_ms
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn wait_for_url(&self, condition: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!("Waiting for URL: {} (timeout: {}ms)", condition, timeout_ms);
        let ctx = self.current_context().await?;
        let duration = Duration::from_millis(timeout_ms);
        let start = Instant::now();

        loop {
            let current_url = self
                .eval_str(&ctx, "location.href".to_string())
                .await
                .unwrap_or_default();

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
        debug!("Taking screenshot (full_page: {})", full_page);
        // BiDi scaffold: viewport screenshot only (full_page not yet mapped).
        let ctx = self.current_context().await?;
        let mut guard = self.lock_browser().await?;
        let browser = guard.as_mut().expect("checked above");
        let options = BrowserScreenshotOptionsBuilder::default()
            .context_id(ctx)
            .build();
        let b64 = browser
            .screenshot_with_options(options)
            .await
            .map_err(|e| ActionError::BrowserError(format!("Screenshot failed: {}", e)))?;

        let cleaned = b64
            .strip_prefix("data:image/png;base64,")
            .or_else(|| b64.strip_prefix("data:image/jpeg;base64,"))
            .unwrap_or(&b64);
        STANDARD
            .decode(cleaned)
            .map_err(|e| ActionError::Internal(format!("Failed to decode screenshot: {}", e)))
    }

    async fn eval(&self, script: &str) -> Result<Value, ActionError> {
        debug!("Evaluating script");
        let ctx = self.current_context().await?;
        self.eval_in(&ctx, script.to_string()).await
    }

    async fn current_url(&self) -> Result<String, ActionError> {
        self.ensure_open()?;
        let ctx = self.current_context().await?;
        self.eval_str(&ctx, "location.href".to_string()).await
    }

    async fn back(&self) -> Result<(), ActionError> {
        let ctx = self.current_context().await?;
        self.eval_in(&ctx, "window.history.back()".to_string())
            .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    async fn forward(&self) -> Result<(), ActionError> {
        let ctx = self.current_context().await?;
        self.eval_in(&ctx, "window.history.forward()".to_string())
            .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        Ok(())
    }

    async fn reload(&self) -> Result<(), ActionError> {
        let ctx = self.current_context().await?;
        self.eval_in(&ctx, "location.reload()".to_string())
            .await?;
        Ok(())
    }

    async fn new_tab(&self, url: Option<&str>) -> Result<usize, ActionError> {
        let target_url = url.unwrap_or("about:blank");
        let new_ctx = {
            let mut guard = self.lock_browser().await?;
            let browser = guard.as_mut().expect("checked above");
            let domain = browser.create_context(false).await.map_err(|e| {
                ActionError::BrowserError(format!("Failed to create new tab: {}", e))
            })?;
            domain.id().clone()
        };

        // Navigate the new context before making it current.
        self.navigate_to(&new_ctx, target_url).await?;

        let new_index = {
            let mut tabs = self.tabs.lock().await;
            tabs.push(new_ctx);
            tabs.len() - 1
        };
        *self.current_tab.lock().await = new_index;
        info!("Opened new tab {} at {}", new_index, target_url);
        Ok(new_index)
    }

    async fn switch_tab(&self, index: usize) -> Result<(), ActionError> {
        let len = self.tabs.lock().await.len();
        if index >= len {
            return Err(ActionError::InvalidParameter(format!(
                "Tab index {} out of range (have {} tabs)",
                index, len
            )));
        }
        *self.current_tab.lock().await = index;
        info!("Switched to tab {}", index);
        Ok(())
    }

    async fn close_tab(&self, index: usize) -> Result<(), ActionError> {
        let ctx = {
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
            tabs.remove(index)
        };

        {
            let mut guard = self.lock_browser().await?;
            let browser = guard.as_mut().expect("checked above");
            let _ = browser
                .close_context(DomainBrowsingContext::from_id(ctx, CreateType::Tab))
                .await;
        }

        let mut current = self.current_tab.lock().await;
        if *current >= index && *current > 0 {
            *current -= 1;
        }
        info!("Closed tab {}", index);
        Ok(())
    }

    async fn tab_count(&self) -> Result<usize, ActionError> {
        Ok(self.tabs.lock().await.len())
    }

    async fn list_tabs(&self) -> Result<Vec<TabInfo>, ActionError> {
        self.ensure_open()?;
        let current = *self.current_tab.lock().await;
        let tabs = self.tabs.lock().await;
        let mut result = Vec::with_capacity(tabs.len());
        for (i, ctx) in tabs.iter().enumerate() {
            let url = self
                .eval_str(ctx, "location.href".to_string())
                .await
                .unwrap_or_else(|_| "about:blank".to_string());
            result.push(TabInfo {
                index: i,
                url,
                active: i == current,
            });
        }
        Ok(result)
    }

    async fn pdf(&self) -> Result<Vec<u8>, ActionError> {
        Err(ActionError::Unsupported(
            "PDF generation is Chromium-only (CDP Page.printToPDF)".into(),
        ))
    }

    async fn set_file_input_files(
        &self,
        _selector: &str,
        _file_paths: Vec<String>,
    ) -> Result<(), ActionError> {
        Err(ActionError::Unsupported(
            "Programmatic file-input population is Chromium-only on this backend".into(),
        ))
    }

    async fn set_file_chooser_intercept(&self, _enabled: bool) -> Result<(), ActionError> {
        Err(ActionError::Unsupported(
            "File-chooser interception is Chromium-only".into(),
        ))
    }

    async fn upload_via_file_chooser(
        &self,
        _trigger_selector: Option<&str>,
        _file_paths: Vec<String>,
        _timeout_ms: u64,
    ) -> Result<(), ActionError> {
        Err(ActionError::Unsupported(
            "File-chooser upload is Chromium-only".into(),
        ))
    }

    // Observability: default no-ops (capabilities gate them off).
    async fn start_console_listener(&self) -> Result<(), ActionError> {
        Ok(())
    }

    async fn start_network_listener(&self) -> Result<(), ActionError> {
        Ok(())
    }

    async fn start_crash_listener(&self) -> Result<(), ActionError> {
        Ok(())
    }

    async fn get_console_logs(&self) -> Vec<ConsoleEntry> {
        Vec::new()
    }

    async fn get_network_logs(&self) -> Vec<NetworkEntry> {
        Vec::new()
    }

    async fn capture_console_logs(&self) -> Result<Vec<ConsoleEntry>, ActionError> {
        Ok(Vec::new())
    }

    async fn capture_network_logs(&self) -> Result<Vec<NetworkEntry>, ActionError> {
        Ok(Vec::new())
    }

    fn is_browser_alive(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustenium_bidi_definitions::script::types::{
        BooleanValue, BooleanValueType, NullValue, NullValueType, StringValue, StringValueType,
    };

    #[test]
    fn firefox_capabilities_disable_chromium_only_features() {
        let c = BrowserCapabilities::FIREFOX;
        assert!(!c.pdf);
        assert!(!c.file_input);
        assert!(!c.file_chooser);
        assert!(!c.console_events);
        assert!(!c.network_events);
        assert!(!c.crash_events);
    }

    #[test]
    fn remote_value_primitives_convert_to_json() {
        assert_eq!(
            remote_value_to_json(&RemoteValue::PrimitiveProtocolValue(
                PrimitiveProtocolValue::StringValue(StringValue::new(
                    StringValueType::String,
                    "hi"
                ))
            )),
            Value::String("hi".into())
        );
        assert_eq!(
            remote_value_to_json(&RemoteValue::PrimitiveProtocolValue(
                PrimitiveProtocolValue::BooleanValue(BooleanValue::new(
                    BooleanValueType::Boolean,
                    true
                ))
            )),
            Value::Bool(true)
        );
        assert_eq!(
            remote_value_to_json(&RemoteValue::PrimitiveProtocolValue(
                PrimitiveProtocolValue::NullValue(NullValue::new(NullValueType::Null))
            )),
            Value::Null
        );
    }

    #[test]
    fn css_and_xpath_map_to_locators() {
        assert!(selector_to_locator(&SelectorType::Css("a".into())).is_some());
        assert!(selector_to_locator(&SelectorType::XPath("//a".into())).is_some());
        assert!(selector_to_locator(&SelectorType::Text("x".into())).is_none());
    }

    #[test]
    fn closed_adapter_is_not_alive() {
        let adapter = FirefoxPageAdapter {
            browser: Arc::new(Mutex::new(None)),
            tabs: Arc::new(Mutex::new(vec![])),
            current_tab: Arc::new(Mutex::new(0)),
            closed: Arc::new(AtomicBool::new(false)),
        };
        assert!(adapter.is_browser_alive());
        adapter.closed.store(true, Ordering::SeqCst);
        assert!(!adapter.is_browser_alive());
    }
}
