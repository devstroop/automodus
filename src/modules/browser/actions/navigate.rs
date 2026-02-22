//! Navigation actions: goto, back, forward, reload

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use tracing::debug;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Navigate to URL (smart navigation)
///
/// By default, skips navigation if already on the target page.
/// Use `force: true` to always navigate even if already on the page.
///
/// # Parameters
/// - `url` (required): Target URL to navigate to
/// - `force` (optional): Force navigation even if already on page (default: false)
///
/// # Output
/// - `url`: Current URL after action
/// - `navigated`: Whether navigation occurred (false if skipped)
/// - `skipped`: Whether navigation was skipped (already on page)
pub struct GotoAction;

/// Check if two URLs are effectively the same page
fn urls_match(current: &str, target: &str) -> bool {
    // Normalize URLs for comparison
    let normalize = |url: &str| -> String {
        let url = url.trim();
        // Remove trailing slash
        let url = url.trim_end_matches('/');
        // Remove protocol for comparison (treat http/https as same)
        let url = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .unwrap_or(url);
        // Remove www. prefix
        let url = url.strip_prefix("www.").unwrap_or(url);
        url.to_lowercase()
    };

    normalize(current) == normalize(target)
}

#[async_trait]
impl Action for GotoAction {
    fn name(&self) -> &'static str {
        "goto"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("url".to_string()))?;

        // Check for force flag
        let force = params
            .get("force")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Get current URL to check if we need to navigate
        let current = browser.current_url().await.unwrap_or_default();

        // Check if already on target page
        if !force && urls_match(&current, url) {
            debug!("Skipping navigation - already on page: {}", current);
            return Ok(ActionOutput::with_data(json!({
                "url": current,
                "navigated": false,
                "skipped": true
            })));
        }

        // Navigate to URL
        browser.goto(url).await?;

        let new_url = browser.current_url().await?;
        debug!("Navigated from {} to {}", current, new_url);

        Ok(ActionOutput::with_data(json!({
            "url": new_url,
            "navigated": true,
            "skipped": false
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urls_match_same() {
        assert!(urls_match("https://example.com", "https://example.com"));
    }

    #[test]
    fn test_urls_match_trailing_slash() {
        assert!(urls_match("https://example.com/", "https://example.com"));
        assert!(urls_match("https://example.com", "https://example.com/"));
    }

    #[test]
    fn test_urls_match_protocol() {
        assert!(urls_match("http://example.com", "https://example.com"));
    }

    #[test]
    fn test_urls_match_www() {
        assert!(urls_match("https://www.example.com", "https://example.com"));
        assert!(urls_match("https://example.com", "https://www.example.com"));
    }

    #[test]
    fn test_urls_match_case() {
        assert!(urls_match("https://Example.COM", "https://example.com"));
    }

    #[test]
    fn test_urls_different_path() {
        assert!(!urls_match("https://example.com/page1", "https://example.com/page2"));
    }

    #[test]
    fn test_urls_different_domain() {
        assert!(!urls_match("https://example.com", "https://other.com"));
    }
}

/// Navigate back
pub struct BackAction;

#[async_trait]
impl Action for BackAction {
    fn name(&self) -> &'static str {
        "back"
    }

    async fn execute(
        &self,
        _params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        browser.back().await?;

        let current = browser.current_url().await?;

        Ok(ActionOutput::with_data(json!({
            "url": current
        })))
    }
}

/// Navigate forward
pub struct ForwardAction;

#[async_trait]
impl Action for ForwardAction {
    fn name(&self) -> &'static str {
        "forward"
    }

    async fn execute(
        &self,
        _params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        browser.forward().await?;

        let current = browser.current_url().await?;

        Ok(ActionOutput::with_data(json!({
            "url": current
        })))
    }
}

/// Reload current page
pub struct ReloadAction;

#[async_trait]
impl Action for ReloadAction {
    fn name(&self) -> &'static str {
        "reload"
    }

    async fn execute(
        &self,
        _params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        browser.reload().await?;

        let current = browser.current_url().await?;

        Ok(ActionOutput::with_data(json!({
            "url": current
        })))
    }
}
