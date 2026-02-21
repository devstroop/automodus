//! Navigation actions: goto, back, forward, reload

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Navigate to URL
pub struct GotoAction;

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

        browser.goto(url).await?;

        let current = browser.current_url().await?;

        Ok(ActionOutput::with_data(json!({
            "url": current
        })))
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
