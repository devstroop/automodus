//! Extract actions: extract, eval

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Extract data from page
pub struct ExtractAction;

#[async_trait]
impl Action for ExtractAction {
    fn name(&self) -> &'static str {
        "extract"
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

        // Determine what to extract
        let attribute = params.get("attribute").and_then(|v| v.as_str());
        let many = params
            .get("many")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let result = if many {
            // Extract from multiple elements
            let script = if let Some(attr) = attribute {
                format!(
                    r#"
                    Array.from(document.querySelectorAll('{}')).map(el => el.getAttribute('{}'))
                    "#,
                    selector.replace('\'', "\\'"),
                    attr.replace('\'', "\\'")
                )
            } else {
                format!(
                    r#"
                    Array.from(document.querySelectorAll('{}')).map(el => el.textContent.trim())
                    "#,
                    selector.replace('\'', "\\'")
                )
            };
            browser.eval(&script).await?
        } else {
            // Extract from single element
            if let Some(attr) = attribute {
                let value = browser.get_attribute(selector, attr).await?;
                json!(value)
            } else {
                let text = browser.get_text(selector).await?;
                json!(text)
            }
        };

        // Store the result if store key provided (support both 'as' and 'store_as')
        let store_key = params
            .get("as")
            .or_else(|| params.get("store_as"))
            .and_then(|v| v.as_str());

        if let Some(key) = store_key {
            let mut output = ActionOutput::store(key, result);
            output.data = output.store.get(key).cloned();
            return Ok(output);
        }

        Ok(ActionOutput::with_data(result))
    }
}

/// Execute JavaScript
pub struct EvalAction;

#[async_trait]
impl Action for EvalAction {
    fn name(&self) -> &'static str {
        "eval"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let script = params
            .get("script")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("script".to_string()))?;

        let result = browser.eval(script).await?;

        // Store the result if store key provided (support both 'as' and 'store_as')
        let store_key = params
            .get("as")
            .or_else(|| params.get("store_as"))
            .and_then(|v| v.as_str());

        if let Some(key) = store_key {
            let mut output = ActionOutput::store(key, result);
            output.data = output.store.get(key).cloned();
            return Ok(output);
        }

        Ok(ActionOutput::with_data(result))
    }
}
