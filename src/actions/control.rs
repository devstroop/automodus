//! Control flow actions: emit, log

use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

use super::registry::{Action, ActionContext, ActionError, ActionOutput, BrowserHandle};

/// Emit an event
pub struct EmitAction;

#[async_trait]
impl Action for EmitAction {
    fn name(&self) -> &'static str {
        "emit"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let event = params
            .get("event")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("event".to_string()))?;

        // Build event data from params
        let mut data = json!({
            "workflow": ctx.workflow_name,
            "step_index": ctx.step_index,
        });

        if let Some(step_id) = &ctx.step_id {
            data["step_id"] = json!(step_id);
        }

        // Add custom data if provided
        if let Some(custom_data) = params.get("data") {
            data["data"] = serde_json::to_value(custom_data).unwrap_or(Value::Null);
        }

        info!(event = %event, workflow = %ctx.workflow_name, "Emitting event");

        Ok(ActionOutput::emit(event, data))
    }
}

/// Log a message
pub struct LogAction;

#[async_trait]
impl Action for LogAction {
    fn name(&self) -> &'static str {
        "log"
    }

    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        ctx: &ActionContext,
        _browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError> {
        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ActionError::MissingParameter("message".to_string()))?;

        let level = params
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("info");

        match level.to_lowercase().as_str() {
            "debug" => debug!(workflow = %ctx.workflow_name, "{}", message),
            "info" => info!(workflow = %ctx.workflow_name, "{}", message),
            "warn" | "warning" => warn!(workflow = %ctx.workflow_name, "{}", message),
            "error" => error!(workflow = %ctx.workflow_name, "{}", message),
            _ => info!(workflow = %ctx.workflow_name, "{}", message),
        }

        Ok(ActionOutput::with_data(json!({
            "logged": message,
            "level": level
        })))
    }
}
