//! Interaction actions: click, type, select, hover

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Click on element
pub struct ClickAction;

#[async_trait]
impl Action for ClickAction {
    fn name(&self) -> &'static str {
        "click"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("selector".to_string()))?;

        browser.click(selector).await?;

        Ok(ActionOutput::with_data(json!({
            "clicked": selector
        })))
    }
}

/// Type text into element
pub struct TypeAction;

#[async_trait]
impl Action for TypeAction {
    fn name(&self) -> &'static str {
        "type"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("selector".to_string()))?;

        let text = params
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("text".to_string()))?;

        let clear = params
            .get("clear")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        browser.type_text(selector, text, clear).await?;

        Ok(ActionOutput::with_data(json!({
            "typed": text,
            "into": selector
        })))
    }
}

/// Select option from dropdown
pub struct SelectAction;

#[async_trait]
impl Action for SelectAction {
    fn name(&self) -> &'static str {
        "select"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("selector".to_string()))?;

        let value = params
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("value".to_string()))?;

        // Use JavaScript to select option
        let script = format!(
            r#"
            const select = document.querySelector('{}');
            if (!select) throw new Error('Select not found');
            select.value = '{}';
            select.dispatchEvent(new Event('change', {{ bubbles: true }}));
            "#,
            selector.replace('\'', "\\'"),
            value.replace('\'', "\\'")
        );

        browser.eval(&script).await?;

        Ok(ActionOutput::with_data(json!({
            "selected": value,
            "in": selector
        })))
    }
}

/// Hover over element
pub struct HoverAction;

#[async_trait]
impl Action for HoverAction {
    fn name(&self) -> &'static str {
        "hover"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let selector = params
            .get("selector")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("selector".to_string()))?;

        // Use JavaScript to trigger hover events
        let script = format!(
            r#"
            const el = document.querySelector('{}');
            if (!el) throw new Error('Element not found');
            el.dispatchEvent(new MouseEvent('mouseover', {{ bubbles: true }}));
            el.dispatchEvent(new MouseEvent('mouseenter', {{ bubbles: true }}));
            "#,
            selector.replace('\'', "\\'")
        );

        browser.eval(&script).await?;

        Ok(ActionOutput::with_data(json!({
            "hovered": selector
        })))
    }
}
