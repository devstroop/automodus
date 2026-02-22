//! Tab management actions: tab.list, tab.new, tab.switch, tab.close

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// List open tabs
pub struct TabListAction;

#[async_trait]
impl Action for TabListAction {
    fn name(&self) -> &'static str {
        "tab.list"
    }

    async fn execute(
        &self,
        _params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let tabs = browser.list_tabs().await?;
        let tab_data: Vec<serde_json::Value> = tabs
            .iter()
            .map(|t| {
                json!({
                    "index": t.index,
                    "url": t.url,
                    "active": t.active,
                })
            })
            .collect();

        Ok(ActionOutput::with_data(json!({
            "tabs": tab_data,
            "count": tabs.len(),
        })))
    }
}

/// Open new tab
pub struct TabNewAction;

#[async_trait]
impl Action for TabNewAction {
    fn name(&self) -> &'static str {
        "tab.new"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let url = params.get("url").and_then(|v| v.as_str());

        let tab_index = browser.new_tab(url).await?;
        let count = browser.tab_count().await?;

        Ok(ActionOutput::with_data(json!({
            "tab_index": tab_index,
            "total_tabs": count,
            "url": url
        })))
    }
}

/// Switch to tab
pub struct TabSwitchAction;

#[async_trait]
impl Action for TabSwitchAction {
    fn name(&self) -> &'static str {
        "tab.switch"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let index = params
            .get("index")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| ActionError::MissingParameter("index".to_string()))?
            as usize;

        browser.switch_tab(index).await?;
        let url = browser.current_url().await?;

        Ok(ActionOutput::with_data(json!({
            "switched_to": index,
            "url": url
        })))
    }
}

/// Close tab
pub struct TabCloseAction;

#[async_trait]
impl Action for TabCloseAction {
    fn name(&self) -> &'static str {
        "tab.close"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let index = params
            .get("index")
            .and_then(|v| v.as_u64())
            .map(|i| i as usize)
            .unwrap_or(ctx.tab_index);

        browser.close_tab(index).await?;
        let count = browser.tab_count().await?;

        Ok(ActionOutput::with_data(json!({
            "closed": index,
            "remaining_tabs": count
        })))
    }
}
