//! Capture actions: screenshot

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use tokio::fs;

use crate::actions::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Take screenshot
pub struct ScreenshotAction;

#[async_trait]
impl Action for ScreenshotAction {
    fn name(&self) -> &'static str {
        "screenshot"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let full_page = params
            .get("full_page")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let bytes = browser.screenshot(full_page).await?;
        let base64 = STANDARD.encode(&bytes);

        // Save to file if path provided
        let saved_path = if let Some(path) = params.get("path").and_then(|v| v.as_str()) {
            let path = Path::new(path);

            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await.map_err(|e| {
                    ActionError::Internal(format!("Failed to create directory: {}", e))
                })?;
            }

            fs::write(path, &bytes)
                .await
                .map_err(|e| ActionError::Internal(format!("Failed to write screenshot: {}", e)))?;

            Some(path.to_string_lossy().to_string())
        } else {
            None
        };

        let mut result = json!({
            "size_bytes": bytes.len(),
            "full_page": full_page,
            "instance_id": ctx.instance_id,
        });

        // Include base64 data if requested or no file path
        if params
            .get("include_base64")
            .and_then(|v| v.as_bool())
            .unwrap_or(saved_path.is_none())
        {
            result["base64"] = json!(base64);
        }

        if let Some(path) = saved_path {
            result["path"] = json!(path);
        }

        Ok(ActionOutput::with_data(result))
    }
}
