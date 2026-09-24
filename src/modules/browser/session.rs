//! Session adapter
//!
//! Engine-agnostic session handle returned by [`launch_session`](super::launch::launch_session).
//! Wraps the concrete backend adapters so the rest of the crate only depends
//! on [`BrowserHandle`] plus a few lifecycle helpers.

use async_trait::async_trait;

use super::{ChromePageAdapter, FirefoxPageAdapter};
use crate::actions::{
    ActionError, BrowserCapabilities, BrowserHandle, ConsoleEntry, NetworkEntry, TabInfo,
};

/// A live browser session on either supported engine.
#[derive(Clone)]
pub enum SessionAdapter {
    /// Chromium via CDP (chromiumoxide)
    Chrome(ChromePageAdapter),
    /// Firefox via WebDriver BiDi (rustenium)
    Firefox(FirefoxPageAdapter),
}

impl SessionAdapter {
    /// Close the underlying browser if this adapter still owns it.
    ///
    /// Safe to call multiple times; subsequent calls are no-ops.
    pub async fn close_browser(&self) {
        match self {
            SessionAdapter::Chrome(c) => c.close_browser().await,
            SessionAdapter::Firefox(f) => f.close_browser().await,
        }
    }

    /// Whether the underlying browser target is still considered alive.
    pub fn is_browser_alive(&self) -> bool {
        match self {
            SessionAdapter::Chrome(c) => c.is_browser_alive(),
            SessionAdapter::Firefox(f) => f.is_browser_alive(),
        }
    }

    /// Start the native console listener (Chromium CDP; no-op on Firefox).
    pub async fn start_console_listener(&self) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.start_console_listener().await,
            SessionAdapter::Firefox(f) => f.start_console_listener().await,
        }
    }

    /// Start the native network listener (Chromium CDP; no-op on Firefox).
    pub async fn start_network_listener(&self) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.start_network_listener().await,
            SessionAdapter::Firefox(f) => f.start_network_listener().await,
        }
    }

    /// Start the crash detection listener (Chromium CDP; no-op on Firefox).
    pub async fn start_crash_listener(&self) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.start_crash_listener().await,
            SessionAdapter::Firefox(f) => f.start_crash_listener().await,
        }
    }
}

#[async_trait]
impl BrowserHandle for SessionAdapter {
    fn capabilities(&self) -> BrowserCapabilities {
        match self {
            SessionAdapter::Chrome(c) => c.capabilities(),
            SessionAdapter::Firefox(f) => f.capabilities(),
        }
    }

    async fn goto(&self, url: &str) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.goto(url).await,
            SessionAdapter::Firefox(f) => f.goto(url).await,
        }
    }

    async fn click(&self, selector: &str) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.click(selector).await,
            SessionAdapter::Firefox(f) => f.click(selector).await,
        }
    }

    async fn type_text(&self, selector: &str, text: &str, clear: bool) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.type_text(selector, text, clear).await,
            SessionAdapter::Firefox(f) => f.type_text(selector, text, clear).await,
        }
    }

    async fn get_text(&self, selector: &str) -> Result<String, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.get_text(selector).await,
            SessionAdapter::Firefox(f) => f.get_text(selector).await,
        }
    }

    async fn get_attribute(
        &self,
        selector: &str,
        attr: &str,
    ) -> Result<Option<String>, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.get_attribute(selector, attr).await,
            SessionAdapter::Firefox(f) => f.get_attribute(selector, attr).await,
        }
    }

    async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.wait_for(selector, timeout_ms).await,
            SessionAdapter::Firefox(f) => f.wait_for(selector, timeout_ms).await,
        }
    }

    async fn wait_for_hidden(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.wait_for_hidden(selector, timeout_ms).await,
            SessionAdapter::Firefox(f) => f.wait_for_hidden(selector, timeout_ms).await,
        }
    }

    async fn wait_for_url(&self, condition: &str, timeout_ms: u64) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.wait_for_url(condition, timeout_ms).await,
            SessionAdapter::Firefox(f) => f.wait_for_url(condition, timeout_ms).await,
        }
    }

    async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.screenshot(full_page).await,
            SessionAdapter::Firefox(f) => f.screenshot(full_page).await,
        }
    }

    async fn eval(&self, script: &str) -> Result<serde_json::Value, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.eval(script).await,
            SessionAdapter::Firefox(f) => f.eval(script).await,
        }
    }

    async fn current_url(&self) -> Result<String, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.current_url().await,
            SessionAdapter::Firefox(f) => f.current_url().await,
        }
    }

    async fn back(&self) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.back().await,
            SessionAdapter::Firefox(f) => f.back().await,
        }
    }

    async fn forward(&self) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.forward().await,
            SessionAdapter::Firefox(f) => f.forward().await,
        }
    }

    async fn reload(&self) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.reload().await,
            SessionAdapter::Firefox(f) => f.reload().await,
        }
    }

    async fn new_tab(&self, url: Option<&str>) -> Result<usize, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.new_tab(url).await,
            SessionAdapter::Firefox(f) => f.new_tab(url).await,
        }
    }

    async fn switch_tab(&self, index: usize) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.switch_tab(index).await,
            SessionAdapter::Firefox(f) => f.switch_tab(index).await,
        }
    }

    async fn close_tab(&self, index: usize) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.close_tab(index).await,
            SessionAdapter::Firefox(f) => f.close_tab(index).await,
        }
    }

    async fn tab_count(&self) -> Result<usize, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.tab_count().await,
            SessionAdapter::Firefox(f) => f.tab_count().await,
        }
    }

    async fn list_tabs(&self) -> Result<Vec<TabInfo>, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.list_tabs().await,
            SessionAdapter::Firefox(f) => f.list_tabs().await,
        }
    }

    async fn pdf(&self) -> Result<Vec<u8>, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.pdf().await,
            SessionAdapter::Firefox(f) => f.pdf().await,
        }
    }

    async fn set_file_input_files(
        &self,
        selector: &str,
        file_paths: Vec<String>,
    ) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.set_file_input_files(selector, file_paths).await,
            SessionAdapter::Firefox(f) => f.set_file_input_files(selector, file_paths).await,
        }
    }

    async fn set_file_chooser_intercept(&self, enabled: bool) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.set_file_chooser_intercept(enabled).await,
            SessionAdapter::Firefox(f) => f.set_file_chooser_intercept(enabled).await,
        }
    }

    async fn upload_via_file_chooser(
        &self,
        trigger_selector: Option<&str>,
        file_paths: Vec<String>,
        timeout_ms: u64,
    ) -> Result<(), ActionError> {
        match self {
            SessionAdapter::Chrome(c) => {
                c.upload_via_file_chooser(trigger_selector, file_paths, timeout_ms)
                    .await
            }
            SessionAdapter::Firefox(f) => {
                f.upload_via_file_chooser(trigger_selector, file_paths, timeout_ms)
                    .await
            }
        }
    }

    async fn start_console_listener(&self) -> Result<(), ActionError> {
        SessionAdapter::start_console_listener(self).await
    }

    async fn start_network_listener(&self) -> Result<(), ActionError> {
        SessionAdapter::start_network_listener(self).await
    }

    async fn start_crash_listener(&self) -> Result<(), ActionError> {
        SessionAdapter::start_crash_listener(self).await
    }

    async fn get_console_logs(&self) -> Vec<ConsoleEntry> {
        match self {
            SessionAdapter::Chrome(c) => c.get_console_logs().await,
            SessionAdapter::Firefox(f) => f.get_console_logs().await,
        }
    }

    async fn get_network_logs(&self) -> Vec<NetworkEntry> {
        match self {
            SessionAdapter::Chrome(c) => c.get_network_logs().await,
            SessionAdapter::Firefox(f) => f.get_network_logs().await,
        }
    }

    async fn capture_console_logs(&self) -> Result<Vec<ConsoleEntry>, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.capture_console_logs().await,
            SessionAdapter::Firefox(f) => f.capture_console_logs().await,
        }
    }

    async fn capture_network_logs(&self) -> Result<Vec<NetworkEntry>, ActionError> {
        match self {
            SessionAdapter::Chrome(c) => c.capture_network_logs().await,
            SessionAdapter::Firefox(f) => f.capture_network_logs().await,
        }
    }

    async fn clear_console_logs(&self) {
        match self {
            SessionAdapter::Chrome(c) => c.clear_console_logs().await,
            SessionAdapter::Firefox(_) => {}
        }
    }

    async fn clear_network_logs(&self) {
        match self {
            SessionAdapter::Chrome(c) => c.clear_network_logs().await,
            SessionAdapter::Firefox(_) => {}
        }
    }

    fn is_browser_alive(&self) -> bool {
        SessionAdapter::is_browser_alive(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_adapter_is_cloneable() {
        // Compile-time check: Clone is required for AppCore cache reuse.
        fn assert_clone<T: Clone>() {}
        assert_clone::<SessionAdapter>();
    }
}
