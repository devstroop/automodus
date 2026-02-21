//! Browser Adapter
//!
//! Implements BrowserHandle trait using chromiumoxide Page.

use async_trait::async_trait;
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::Page;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::actions::{ActionError, BrowserHandle};

/// Adapter that implements BrowserHandle for chromiumoxide Page
#[derive(Clone)]
pub struct ChromePageAdapter {
    page: Arc<Mutex<Page>>,
    /// Additional pages/tabs (for future multi-tab support)
    #[allow(dead_code)]
    _tabs: Arc<Mutex<Vec<Page>>>,
    /// Current tab index (for future multi-tab support)
    #[allow(dead_code)]
    _current_tab: Arc<Mutex<usize>>,
}

impl ChromePageAdapter {
    /// Create a new adapter wrapping a chromiumoxide Page
    pub fn new(page: Page) -> Self {
        Self {
            page: Arc::new(Mutex::new(page)),
            _tabs: Arc::new(Mutex::new(Vec::new())),
            _current_tab: Arc::new(Mutex::new(0)),
        }
    }

    /// Get the underlying page (for direct access if needed)
    pub async fn page(&self) -> tokio::sync::MutexGuard<'_, Page> {
        self.page.lock().await
    }
}

#[async_trait]
impl BrowserHandle for ChromePageAdapter {
    async fn goto(&self, url: &str) -> Result<(), ActionError> {
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
        debug!("Clicking: {}", selector);
        let page = self.page.lock().await;

        let element = page
            .find_element(selector)
            .await
            .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

        element
            .click()
            .await
            .map_err(|e| ActionError::BrowserError(format!("Click failed: {}", e)))?;

        Ok(())
    }

    async fn type_text(&self, selector: &str, text: &str, clear: bool) -> Result<(), ActionError> {
        debug!("Typing into: {}", selector);
        let page = self.page.lock().await;

        let element = page
            .find_element(selector)
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

        Ok(())
    }

    async fn get_text(&self, selector: &str) -> Result<String, ActionError> {
        debug!("Getting text from: {}", selector);
        let page = self.page.lock().await;

        let element = page
            .find_element(selector)
            .await
            .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

        let text = element
            .inner_text()
            .await
            .map_err(|e| ActionError::BrowserError(format!("Get text failed: {}", e)))?
            .unwrap_or_default();

        Ok(text)
    }

    async fn get_attribute(
        &self,
        selector: &str,
        attr: &str,
    ) -> Result<Option<String>, ActionError> {
        debug!("Getting attribute {} from: {}", attr, selector);
        let page = self.page.lock().await;

        let element = page
            .find_element(selector)
            .await
            .map_err(|e| ActionError::ElementNotFound(format!("{}: {}", selector, e)))?;

        let value = element
            .attribute(attr)
            .await
            .map_err(|e| ActionError::BrowserError(format!("Get attribute failed: {}", e)))?;

        Ok(value)
    }

    async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!(
            "Waiting for element: {} (timeout: {}ms)",
            selector, timeout_ms
        );
        let page = self.page.lock().await;

        let duration = Duration::from_millis(timeout_ms);
        let start = std::time::Instant::now();

        loop {
            match page.find_element(selector).await {
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
    }

    async fn wait_for_hidden(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        debug!(
            "Waiting for element to disappear: {} (timeout: {}ms)",
            selector, timeout_ms
        );
        let page = self.page.lock().await;

        let duration = Duration::from_millis(timeout_ms);
        let start = std::time::Instant::now();

        loop {
            match page.find_element(selector).await {
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

    async fn new_tab(&self, _url: Option<&str>) -> Result<usize, ActionError> {
        // Note: Multi-tab support requires browser-level API
        // For now, return error as single-page adapter
        warn!("new_tab not fully implemented in single-page adapter");
        Err(ActionError::Internal(
            "Multi-tab not implemented yet".to_string(),
        ))
    }

    async fn switch_tab(&self, _index: usize) -> Result<(), ActionError> {
        warn!("switch_tab not fully implemented in single-page adapter");
        Err(ActionError::Internal(
            "Multi-tab not implemented yet".to_string(),
        ))
    }

    async fn close_tab(&self, _index: usize) -> Result<(), ActionError> {
        warn!("close_tab not fully implemented in single-page adapter");
        Err(ActionError::Internal(
            "Multi-tab not implemented yet".to_string(),
        ))
    }

    async fn tab_count(&self) -> Result<usize, ActionError> {
        Ok(1) // Single page adapter
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
}
