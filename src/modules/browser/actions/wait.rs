//! Wait actions: wait_for, sleep
//!
//! wait_for supports:
//!   - selector: CSS selector to wait for
//!   - state: "visible" (default), "hidden", "present", "absent"
//!   - text: Wait for text to appear/disappear
//!   - url: Wait for URL to match condition
//!   - timeout: Timeout in ms (default: 30000)

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use tokio::time::{sleep, Duration};

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Element state to wait for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitState {
    /// Element exists and is visible (default)
    Visible,
    /// Element is hidden or doesn't exist
    Hidden,
    /// Element exists in DOM (may be hidden)
    Present,
    /// Element does not exist in DOM
    Absent,
}

impl WaitState {
    fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "hidden" | "invisible" => Self::Hidden,
            "present" | "exists" => Self::Present,
            "absent" | "removed" | "gone" => Self::Absent,
            _ => Self::Visible,
        }
    }
}

/// Wait for element with configurable state
pub struct WaitForAction;

#[async_trait]
impl Action for WaitForAction {
    fn name(&self) -> &'static str {
        "wait_for"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let timeout = params
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30000);

        // Get state parameter (visible, hidden, present, absent)
        let state = params
            .get("state")
            .and_then(|v| v.as_str())
            .map(WaitState::from_str)
            .unwrap_or(WaitState::Visible);

        // Element selector
        if let Some(selector) = params.get("selector").and_then(|v| v.as_str()) {
            match state {
                WaitState::Visible | WaitState::Present => {
                    browser.wait_for(selector, timeout).await?;
                    return Ok(ActionOutput::with_data(json!({
                        "found": selector,
                        "state": "visible"
                    })));
                }
                WaitState::Hidden | WaitState::Absent => {
                    browser.wait_for_hidden(selector, timeout).await?;
                    return Ok(ActionOutput::with_data(json!({
                        "selector": selector,
                        "state": "hidden"
                    })));
                }
            }
        }

        // Wait for URL condition
        if let Some(url) = params.get("url").and_then(|v| v.as_str()) {
            browser.wait_for_url(url, timeout).await?;
            let current = browser.current_url().await?;
            return Ok(ActionOutput::with_data(json!({
                "url": current
            })));
        }

        // Wait for text to appear/disappear
        if let Some(text) = params.get("text").and_then(|v| v.as_str()) {
            // Use JavaScript to find text since XPath can be finicky
            let script = format!(
                r#"document.body.innerText.includes('{}')"#,
                text.replace('\'', "\\'").replace('\"', "\\\"")
            );

            let start = std::time::Instant::now();
            let duration = Duration::from_millis(timeout);

            loop {
                let result = browser.eval(&script).await?;
                let found = result.as_bool().unwrap_or(false);

                match state {
                    WaitState::Visible | WaitState::Present => {
                        if found {
                            return Ok(ActionOutput::with_data(json!({
                                "text_found": text,
                                "state": "visible"
                            })));
                        }
                    }
                    WaitState::Hidden | WaitState::Absent => {
                        if !found {
                            return Ok(ActionOutput::with_data(json!({
                                "text_gone": text,
                                "state": "hidden"
                            })));
                        }
                    }
                }

                if start.elapsed() >= duration {
                    let expected = match state {
                        WaitState::Visible | WaitState::Present => "appear",
                        WaitState::Hidden | WaitState::Absent => "disappear",
                    };
                    return Err(ActionError::Timeout(format!(
                        "Text '{}' did not {} within {}ms",
                        text, expected, timeout
                    )));
                }

                sleep(Duration::from_millis(100)).await;
            }
        }

        Err(ActionError::MissingParameter(
            "selector, url, or text".to_string(),
        ))
    }
}

/// Sleep for specified duration
pub struct SleepAction;

#[async_trait]
impl Action for SleepAction {
    fn name(&self) -> &'static str {
        "sleep"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let ms = params
            .get("ms")
            .or_else(|| params.get("duration"))
            .and_then(|v| v.as_u64())
            .unwrap_or(1000);

        sleep(Duration::from_millis(ms)).await;

        Ok(ActionOutput::with_data(json!({
            "slept_ms": ms
        })))
    }
}
