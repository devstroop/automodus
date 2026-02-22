//! Extract actions: extract, eval
//! Supports extended selectors: text:, text*:, role:, xpath:, CSS

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};
use crate::modules::browser::selector::{parse_selector, SelectorType};

/// Generate JavaScript to find all matching elements and extract data
fn extract_many_js(selector: &SelectorType, attribute: Option<&str>) -> String {
    let extract_fn = match attribute {
        Some(attr) => format!(
            "el => el.getAttribute({})",
            serde_json::to_string(attr).unwrap()
        ),
        None => "el => el.textContent?.trim() || ''".to_string(),
    };

    match selector {
        SelectorType::Css(css) => {
            format!(
                "Array.from(document.querySelectorAll({})).map({})",
                serde_json::to_string(css).unwrap(),
                extract_fn
            )
        }
        SelectorType::Text(text) => {
            format!(
                r#"(function() {{
    const text = {};
    const results = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, null);
    while (walker.nextNode()) {{
        const el = walker.currentNode;
        const directText = Array.from(el.childNodes)
            .filter(n => n.nodeType === Node.TEXT_NODE)
            .map(n => n.textContent.trim())
            .join(' ')
            .trim();
        if (directText === text) results.push(el);
        else if (el.children.length === 0 && el.innerText?.trim() === text) results.push(el);
    }}
    return results.map({});
}})()"#,
                serde_json::to_string(text).unwrap(),
                extract_fn
            )
        }
        SelectorType::TextPartial(text) => {
            format!(
                r#"(function() {{
    const text = {};
    const results = [];
    const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT, null);
    while (walker.nextNode()) {{
        const el = walker.currentNode;
        const directText = Array.from(el.childNodes)
            .filter(n => n.nodeType === Node.TEXT_NODE)
            .map(n => n.textContent)
            .join(' ');
        if (directText.includes(text)) results.push(el);
        else if (el.children.length === 0 && el.innerText?.includes(text)) results.push(el);
    }}
    return results.map({});
}})()"#,
                serde_json::to_string(text).unwrap(),
                extract_fn
            )
        }
        SelectorType::Role { role, name } => {
            let name_filter = match name {
                Some(n) => format!(
                    r#"
        for (const el of byRole) {{
            const accName = el.getAttribute('aria-label') || el.innerText?.trim() || '';
            if (accName === {} || accName.includes({})) results.push(el);
        }}"#,
                    serde_json::to_string(n).unwrap(),
                    serde_json::to_string(n).unwrap()
                ),
                None => "results.push(...byRole);".to_string(),
            };
            format!(
                r#"(function() {{
    const role = {};
    const results = [];
    const byRole = document.querySelectorAll('[role="' + role + '"]');
    {}
    const implicitRoles = {{
        'button': 'button, input[type="button"], input[type="submit"]',
        'link': 'a[href]',
        'textbox': 'input[type="text"], input:not([type]), textarea',
        'checkbox': 'input[type="checkbox"]',
        'radio': 'input[type="radio"]',
        'combobox': 'select',
        'img': 'img',
        'heading': 'h1, h2, h3, h4, h5, h6'
    }};
    const implicitSelector = implicitRoles[role];
    if (implicitSelector) {{
        const elements = document.querySelectorAll(implicitSelector);
        {}
    }}
    return results.map({});
}})()"#,
                serde_json::to_string(role).unwrap(),
                name_filter,
                if name.is_some() {
                    format!(
                        r#"for (const el of elements) {{
            const accName = el.getAttribute('aria-label') || el.value || el.innerText?.trim() || '';
            if (accName === {} || accName.includes({})) results.push(el);
        }}"#,
                        serde_json::to_string(name.as_ref().unwrap()).unwrap(),
                        serde_json::to_string(name.as_ref().unwrap()).unwrap()
                    )
                } else {
                    "results.push(...elements);".to_string()
                },
                extract_fn
            )
        }
        SelectorType::XPath(xpath) => {
            format!(
                r#"(function() {{
    const xpath = {};
    const result = document.evaluate(xpath, document, null, XPathResult.ORDERED_NODE_SNAPSHOT_TYPE, null);
    const elements = [];
    for (let i = 0; i < result.snapshotLength; i++) {{
        elements.push(result.snapshotItem(i));
    }}
    return elements.map({});
}})()"#,
                serde_json::to_string(xpath).unwrap(),
                extract_fn
            )
        }
    }
}

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

        let parsed = parse_selector(selector);

        let result = if many {
            // Extract from multiple elements using extended selector support
            let script = extract_many_js(&parsed, attribute);
            browser.eval(&script).await?
        } else {
            // Extract from single element (uses adapter methods with extended selector support)
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
