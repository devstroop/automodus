//! Upload action: Set files on file input elements via CDP
//!
//! This is the only way to programmatically set files on file inputs
//! since browsers block JavaScript from setting file input values.

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;

use crate::actions::registry::{
    Action, ActionContext, ActionError, ActionOutput, BrowserHandle,
};

fn require_file_input(browser: &dyn BrowserHandle) -> Result<(), ActionError> {
    if !browser.capabilities().file_input {
        return Err(ActionError::Unsupported(
            "file input upload requires a backend with file_input capability".into(),
        ));
    }
    Ok(())
}

fn require_file_chooser(browser: &dyn BrowserHandle) -> Result<(), ActionError> {
    if !browser.capabilities().file_chooser {
        return Err(ActionError::Unsupported(
            "file chooser interception requires a backend with file_chooser capability".into(),
        ));
    }
    Ok(())
}

/// Upload file(s) to a file input element (selector-based approach)
pub struct UploadAction;

#[async_trait]
impl Action for UploadAction {
    fn name(&self) -> &'static str {
        "upload"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        // Get selector(s) - support single or multiple for fallback
        let selectors: Vec<&str> = if let Some(selector) = params.get("selector") {
            vec![selector.as_str().ok_or_else(|| {
                ActionError::InvalidParameter("selector must be a string".to_string())
            })?]
        } else if let Some(selectors_val) = params.get("selectors") {
            selectors_val
                .as_sequence()
                .ok_or_else(|| {
                    ActionError::InvalidParameter("selectors must be an array".to_string())
                })?
                .iter()
                .filter_map(|v| v.as_str())
                .collect()
        } else {
            // Default fallback selectors for file inputs
            vec![
                r#"input[accept="image/*,video/mp4,video/3gpp,video/quicktime"]"#,
                r#"input[accept="*"]"#,
                r#"input[accept="image/*"]"#,
                r#"body > input[type="file"]"#,
                r#"body > input"#,
                r#"input[type="file"]"#,
            ]
        };

        // Support both single file (file_path) and multiple files (files)
        let file_paths = get_file_paths(params)?;
        require_file_input(browser)?;

        // Try each selector until one works
        let mut last_error = None;
        let mut success_selector = None;

        for selector in &selectors {
            match browser
                .set_file_input_files(selector, file_paths.clone())
                .await
            {
                Ok(_) => {
                    success_selector = Some(*selector);
                    break;
                }
                Err(e) => {
                    last_error = Some(e);
                }
            }
        }

        if let Some(selector) = success_selector {
            Ok(ActionOutput::with_data(json!({
                "uploaded": file_paths.len(),
                "files": file_paths,
                "selector": selector
            })))
        } else {
            Err(last_error
                .unwrap_or_else(|| ActionError::ElementNotFound("No file input found".to_string())))
        }
    }
}

/// Wait for file chooser event and upload files
/// This is the proper way to handle file uploads - waits for the browser's
/// fileChooserOpened event and sets files using the backend_node_id from the event.
///
/// Parameters:
/// - file_path or files: Required file(s) to upload
/// - trigger: Optional CSS selector to click that triggers the file input
/// - timeout: How long to wait for the event (default 10000ms)
///
/// Usage: Enable file chooser interception first, then either:
/// 1. Provide trigger selector to click element and wait for event in one step
/// 2. Click separately before this action (legacy mode, may have timing issues)
pub struct WaitUploadAction;

#[async_trait]
impl Action for WaitUploadAction {
    fn name(&self) -> &'static str {
        "wait_upload"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let file_paths = get_file_paths(params)?;
        require_file_chooser(browser)?;

        // Get optional trigger selector - if provided, will click before waiting
        let trigger_selector = params
            .get("trigger")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Get timeout (default 10 seconds)
        let timeout_ms = params
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(10000);

        // Wait for file chooser event and upload
        browser
            .upload_via_file_chooser(trigger_selector.as_deref(), file_paths.clone(), timeout_ms)
            .await?;

        Ok(ActionOutput::with_data(json!({
            "uploaded": file_paths.len(),
            "files": file_paths,
            "method": "file_chooser_event",
            "trigger": trigger_selector
        })))
    }
}

/// Enable/disable file chooser interception (prevents native file dialog)
pub struct FileChooserAction;

#[async_trait]
impl Action for FileChooserAction {
    fn name(&self) -> &'static str {
        "file_chooser"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        _ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let enabled = params
            .get("intercept")
            .or(params.get("enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        require_file_chooser(browser)?;
        browser.set_file_chooser_intercept(enabled).await?;

        Ok(ActionOutput::with_data(json!({
            "intercept": enabled
        })))
    }
}

/// Helper function to extract file paths from params
fn get_file_paths(params: &HashMap<String, serde_yaml::Value>) -> Result<Vec<String>, ActionError> {
    if let Some(file_path) = params.get("file_path").or(params.get("path")) {
        // Single file
        let path = file_path.as_str().ok_or_else(|| {
            ActionError::InvalidParameter("file_path must be a string".to_string())
        })?;

        // Strip surrounding quotes if present (from shell input)
        let path = path.trim();
        let path = if (path.starts_with('"') && path.ends_with('"'))
            || (path.starts_with('\'') && path.ends_with('\''))
        {
            &path[1..path.len() - 1]
        } else {
            path
        };

        // Resolve to absolute path
        let abs_path = std::fs::canonicalize(path).map_err(|e| {
            ActionError::InvalidParameter(format!("Invalid file path '{}': {}", path, e))
        })?;

        let abs_str = abs_path.to_string_lossy().to_string();

        // Verify file exists
        if !Path::new(&abs_str).exists() {
            return Err(ActionError::InvalidParameter(format!(
                "File not found: {}",
                abs_str
            )));
        }

        Ok(vec![abs_str])
    } else if let Some(files) = params.get("files") {
        // Multiple files
        let files_seq = files
            .as_sequence()
            .ok_or_else(|| ActionError::InvalidParameter("files must be an array".to_string()))?;

        let mut paths = Vec::new();
        for file in files_seq {
            let path = file.as_str().ok_or_else(|| {
                ActionError::InvalidParameter("Each file must be a string path".to_string())
            })?;

            // Strip surrounding quotes if present
            let path = path.trim();
            let path = if (path.starts_with('"') && path.ends_with('"'))
                || (path.starts_with('\'') && path.ends_with('\''))
            {
                &path[1..path.len() - 1]
            } else {
                path
            };

            let abs_path = std::fs::canonicalize(path).map_err(|e| {
                ActionError::InvalidParameter(format!("Invalid file path '{}': {}", path, e))
            })?;

            let abs_str = abs_path.to_string_lossy().to_string();

            if !Path::new(&abs_str).exists() {
                return Err(ActionError::InvalidParameter(format!(
                    "File not found: {}",
                    abs_str
                )));
            }

            paths.push(abs_str);
        }
        Ok(paths)
    } else {
        Err(ActionError::MissingParameter(
            "file_path or files".to_string(),
        ))
    }
}
