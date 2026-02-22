//! Interaction actions: click, type, select, hover
//! Supports extended selectors: text:, text*:, role:, xpath:, CSS

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};
use crate::modules::browser::selector::{parse_selector, selector_hover_js, selector_to_js};

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

        // Parse and resolve selector
        let parsed = parse_selector(selector);
        let element_js = selector_to_js(&parsed);

        // Use JavaScript to select option with extended selector support
        let script = format!(
            r#"(function() {{
                const select = {};
                if (!select) throw new Error('Select not found: {}');
                select.value = {};
                select.dispatchEvent(new Event('change', {{ bubbles: true }}));
                return true;
            }})()"#,
            element_js,
            selector.replace('\'', "\\'"),
            serde_json::to_string(value).unwrap()
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

        // Parse selector and use JavaScript with extended selector support
        let parsed = parse_selector(selector);
        let script = selector_hover_js(&parsed);

        browser.eval(&script).await?;

        Ok(ActionOutput::with_data(json!({
            "hovered": selector
        })))
    }
}
