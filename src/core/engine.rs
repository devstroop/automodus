//! Workflow Execution Engine
//!
//! Executes workflows step by step with browser automation.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::actions::{ActionError, ActionOutput, ActionRegistry, BrowserHandle};
use crate::utils::yaml_to_json;
use crate::workflow::schema::{CaptureMode, DebugConfig, ResolvedDebugConfig};
use crate::workflow::{CompleteHandler, ErrorHandler, Step, Workflow};

use super::context::ExecutionContext;
use super::template::TemplateEngine;

/// Pause response from handler
#[derive(Debug, Clone, PartialEq)]
pub enum PauseResponse {
    /// Continue execution
    Continue,
    /// Skip current step
    Skip,
    /// Abort workflow
    Abort,
}

/// Handler for debug pause events
#[async_trait::async_trait]
pub trait PauseHandler: Send + Sync {
    /// Called when execution is paused at a step
    /// Returns the action to take (continue, skip, or abort)
    async fn on_pause(
        &self,
        workflow: &str,
        step: usize,
        action: &str,
        selector: Option<&str>,
    ) -> PauseResponse;
}

/// Default pause handler that always continues
pub struct DefaultPauseHandler;

#[async_trait::async_trait]
impl PauseHandler for DefaultPauseHandler {
    async fn on_pause(
        &self,
        _workflow: &str,
        _step: usize,
        _action: &str,
        _selector: Option<&str>,
    ) -> PauseResponse {
        PauseResponse::Continue
    }
}

/// Workflow execution error
#[derive(Debug, Error)]
pub enum WorkflowError {
    #[error("Workflow not found: {0}")]
    NotFound(String),

    #[error("Action not found: {0}")]
    ActionNotFound(String),

    #[error("Action failed: {0}")]
    ActionFailed(#[from] ActionError),

    #[error("Condition failed: {0}")]
    ConditionFailed(String),

    #[error("Max retries exceeded for step {0}")]
    MaxRetries(String),

    #[error("Step timeout: {0}")]
    Timeout(String),

    #[error("Workflow aborted: {0}")]
    Aborted(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Browser error: {0}")]
    BrowserError(String),
}

/// Result of a workflow execution
#[derive(Debug, Clone)]
pub struct WorkflowResult {
    /// Workflow name
    pub workflow_name: String,

    /// Workflow execution ID
    pub workflow_id: String,

    /// Whether execution succeeded
    pub success: bool,

    /// Output data
    pub output: Value,

    /// Events emitted during execution
    pub events: Vec<(String, Value)>,

    /// Error message if failed
    pub error: Option<String>,

    /// Execution duration in milliseconds
    pub duration_ms: i64,

    /// Number of steps executed
    pub steps_executed: usize,

    /// Debug screenshots captured during execution
    pub debug_screenshots: Vec<String>,
}

/// Workflow execution engine
pub struct WorkflowEngine {
    /// Action registry
    registry: ActionRegistry,

    /// Event handlers (event name -> list of workflows to trigger)
    event_handlers: Arc<RwLock<HashMap<String, Vec<String>>>>,

    /// Debug output directory
    debug_dir: std::path::PathBuf,
}

impl WorkflowEngine {
    /// Create a new workflow engine
    pub fn new() -> Self {
        Self {
            registry: ActionRegistry::new(),
            event_handlers: Arc::new(RwLock::new(HashMap::new())),
            debug_dir: std::path::PathBuf::from("data/debug"),
        }
    }

    /// Set the debug output directory
    pub fn set_debug_dir(&mut self, dir: std::path::PathBuf) {
        self.debug_dir = dir;
    }

    /// Register an event handler
    pub async fn on_event(&self, event: &str, workflow_name: &str) {
        let mut handlers = self.event_handlers.write().await;
        handlers
            .entry(event.to_string())
            .or_default()
            .push(workflow_name.to_string());
    }

    /// Execute a workflow
    pub async fn execute(
        &self,
        workflow: &Workflow,
        browser: &dyn BrowserHandle,
        params: HashMap<String, Value>,
    ) -> Result<WorkflowResult, WorkflowError> {
        // Use default debug config, caller can use execute_with_debug for custom config
        self.execute_with_debug(workflow, browser, params, ResolvedDebugConfig::default())
            .await
    }

    /// Execute a workflow with debug configuration
    pub async fn execute_with_debug(
        &self,
        workflow: &Workflow,
        browser: &dyn BrowserHandle,
        params: HashMap<String, Value>,
        debug_config: ResolvedDebugConfig,
    ) -> Result<WorkflowResult, WorkflowError> {
        self.execute_with_pause_handler(
            workflow,
            browser,
            params,
            debug_config,
            &DefaultPauseHandler,
        )
        .await
    }

    /// Execute a workflow with debug configuration and custom pause handler
    pub async fn execute_with_pause_handler(
        &self,
        workflow: &Workflow,
        browser: &dyn BrowserHandle,
        params: HashMap<String, Value>,
        debug_config: ResolvedDebugConfig,
        pause_handler: &dyn PauseHandler,
    ) -> Result<WorkflowResult, WorkflowError> {
        let instance_id = uuid::Uuid::new_v4().to_string();

        info!(
            workflow = %workflow.name,
            instance_id = %instance_id,
            debug_enabled = debug_config.enabled,
            "Starting workflow execution"
        );

        // Create execution context with debug config
        let mut ctx = ExecutionContext::new(&workflow.name, &instance_id).with_debug(debug_config);

        // Track debug screenshots
        let mut debug_screenshots: Vec<String> = Vec::new();

        // Set initial variables from workflow definition
        if let Some(vars) = &workflow.vars {
            for (key, value) in vars {
                ctx.vars.insert(key.clone(), yaml_to_json(value));
            }
        }

        // Set trigger parameters
        ctx.params = params;

        // Execute steps
        let result = self
            .execute_steps(workflow, browser, &mut ctx, &mut debug_screenshots, pause_handler)
            .await;

        // Build result
        let duration_ms = ctx.duration().num_milliseconds();

        match result {
            Ok(output) => {
                info!(
                    workflow = %workflow.name,
                    duration_ms = duration_ms,
                    steps = ctx.step_index,
                    "Workflow completed successfully"
                );

                Ok(WorkflowResult {
                    workflow_name: workflow.name.clone(),
                    workflow_id: ctx.workflow_id.clone(),
                    success: true,
                    output,
                    events: ctx.events.clone(),
                    error: None,
                    duration_ms,
                    steps_executed: ctx.step_index,
                    debug_screenshots,
                })
            }
            Err(e) => {
                error!(
                    workflow = %workflow.name,
                    error = %e,
                    step = ctx.step_index,
                    "Workflow execution failed"
                );

                // Handle error callback if defined
                if let Some(on_error) = &workflow.on_error {
                    self.handle_error(on_error, &e, browser, &mut ctx, &mut debug_screenshots)
                        .await;
                }

                Ok(WorkflowResult {
                    workflow_name: workflow.name.clone(),
                    workflow_id: ctx.workflow_id.clone(),
                    success: false,
                    output: json!({ "error": e.to_string() }),
                    events: ctx.events.clone(),
                    error: Some(e.to_string()),
                    duration_ms,
                    steps_executed: ctx.step_index,
                    debug_screenshots,
                })
            }
        }
    }

    /// Execute all steps in a workflow
    async fn execute_steps(
        &self,
        workflow: &Workflow,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
        pause_handler: &dyn PauseHandler,
    ) -> Result<Value, WorkflowError> {
        while ctx.step_index < workflow.steps.len() {
            let step = &workflow.steps[ctx.step_index];

            // Merge step-level debug config with workflow-level
            let step_debug = self.resolve_step_debug(ctx, step);

            // Apply step delay if configured
            if step_debug.delay > 0 {
                debug!(
                    workflow = %workflow.name,
                    step = ctx.step_index,
                    delay_ms = step_debug.delay,
                    "Applying debug delay before step"
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(step_debug.delay)).await;
            }

            // Handle pause if configured
            if step_debug.pause {
                let selector = step.params.get("selector").and_then(|v| v.as_str());
                let response = pause_handler
                    .on_pause(&workflow.name, ctx.step_index, &step.action, selector)
                    .await;

                match response {
                    PauseResponse::Continue => {
                        debug!(
                            workflow = %workflow.name,
                            step = ctx.step_index,
                            "Continuing after pause"
                        );
                    }
                    PauseResponse::Skip => {
                        info!(
                            workflow = %workflow.name,
                            step = ctx.step_index,
                            "Skipping step due to pause response"
                        );
                        ctx.next_step();
                        continue;
                    }
                    PauseResponse::Abort => {
                        return Err(WorkflowError::Aborted("User aborted at pause".to_string()));
                    }
                }
            }

            // Capture screenshot before step if configured
            if matches!(step_debug.capture, CaptureMode::Before | CaptureMode::All) {
                if let Some(path) = self
                    .capture_debug_screenshot(browser, ctx, "before")
                    .await
                {
                    debug_screenshots.push(path);
                }
            }

            // Handle `condition` action specially (with then/else blocks)
            // Must check BEFORE step-level condition because `if:` gets captured into step.condition
            if step.action == "condition" {
                self.execute_condition_action(step, browser, ctx).await?;
                ctx.next_step();
                continue;
            }

            // Check condition (step-level `if:`) - for non-condition actions
            if let Some(condition) = &step.condition {
                if !self.evaluate_condition(condition, ctx) {
                    debug!(
                        workflow = %workflow.name,
                        step = ctx.step_index,
                        "Step skipped due to condition"
                    );
                    ctx.next_step();
                    continue;
                }
            }

            // Execute step with retries
            let result = self
                .execute_step_with_retry(step, browser, ctx, &step_debug, debug_screenshots, pause_handler)
                .await;

            // Handle execution result
            let output = match result {
                Ok(output) => {
                    // Capture screenshot after step if configured
                    if matches!(step_debug.capture, CaptureMode::After | CaptureMode::All) {
                        if let Some(path) = self
                            .capture_debug_screenshot(browser, ctx, "after")
                            .await
                        {
                            debug_screenshots.push(path);
                        }
                    }
                    output
                }
                Err(e) => {
                    // Capture screenshot on failure if configured
                    if matches!(
                        step_debug.capture,
                        CaptureMode::Failure | CaptureMode::All
                    ) {
                        if let Some(path) = self
                            .capture_debug_screenshot(browser, ctx, "failure")
                            .await
                        {
                            debug_screenshots.push(path);
                        }
                    }
                    return Err(e);
                }
            };

            // Store output
            if let Some(data) = &output.data {
                if let Some(id) = &step.id {
                    ctx.store_step_output(id, data.clone());
                }
            }

            // Merge store values
            for (key, value) in output.store {
                ctx.store_value(key, value);
            }

            // Emit events
            if let Some((event, data)) = output.emit {
                ctx.emit_event(&event, data);
            }

            // Handle control flow
            if output.skip {
                info!(workflow = %workflow.name, "Workflow skipped by step");
                break;
            }

            if let Some(goto_id) = output.goto {
                // Find step index by ID
                if let Some(idx) = workflow
                    .steps
                    .iter()
                    .position(|s| s.id.as_deref() == Some(&goto_id))
                {
                    ctx.goto_step(idx);
                    continue;
                } else {
                    return Err(WorkflowError::InvalidConfig(format!(
                        "Step not found: {}",
                        goto_id
                    )));
                }
            }

            ctx.next_step();
        }

        // Build output
        let output = if let Some(output_defs) = &workflow.output {
            ctx.build_output(output_defs)
        } else {
            // Default output: all stored values
            Value::Object(
                ctx.store
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            )
        };

        // Handle on_complete callback
        if let Some(on_complete) = &workflow.on_complete {
            self.handle_complete(on_complete, browser, ctx).await;
        }

        Ok(output)
    }

    /// Execute a step with retry logic
    async fn execute_step_with_retry(
        &self,
        step: &Step,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        step_debug: &ResolvedDebugConfig,
        debug_screenshots: &mut Vec<String>,
        _pause_handler: &dyn PauseHandler,
    ) -> Result<ActionOutput, WorkflowError> {
        let max_retries = step.retry.as_ref().map(|r| r.max.unwrap_or(3)).unwrap_or(0);

        let retry_delay = step
            .retry
            .as_ref()
            .map(|r| r.delay_ms.unwrap_or(1000))
            .unwrap_or(1000);

        let mut last_error = None;

        for attempt in 0..=max_retries {
            if attempt > 0 {
                warn!(
                    workflow = %ctx.workflow_name,
                    step = ctx.step_index,
                    attempt = attempt,
                    "Retrying step"
                );
                tokio::time::sleep(tokio::time::Duration::from_millis(retry_delay)).await;
            }

            match self.execute_step(step, browser, ctx, step_debug).await {
                Ok(output) => return Ok(output),
                Err(e) => {
                    // Capture screenshot on failure during retries
                    if matches!(step_debug.capture, CaptureMode::Failure | CaptureMode::All) {
                        if let Some(path) = self
                            .capture_debug_screenshot(browser, ctx, &format!("retry{}_failure", attempt))
                            .await
                        {
                            debug_screenshots.push(path);
                        }
                    }

                    if attempt < max_retries {
                        warn!(
                            workflow = %ctx.workflow_name,
                            step = ctx.step_index,
                            error = %e,
                            "Step failed, will retry"
                        );
                        last_error = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            WorkflowError::ActionFailed(ActionError::Internal("Unknown error".into()))
        }))
    }

    /// Execute a condition action with then/else branches
    async fn execute_condition_action(
        &self,
        step: &Step,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
    ) -> Result<(), WorkflowError> {
        // Get the `if` condition - check step.condition first (captured by serde rename),
        // then fall back to params["if"]
        let condition_str = step
            .condition
            .as_deref()
            .or_else(|| step.params.get("if").and_then(|v| v.as_str()))
            .ok_or_else(|| {
                WorkflowError::InvalidConfig("condition action requires 'if' parameter".into())
            })?;

        // Evaluate the condition
        let condition_result = self.evaluate_condition(condition_str, ctx);

        debug!(
            workflow = %ctx.workflow_name,
            step = ctx.step_index,
            condition = %condition_str,
            result = condition_result,
            "Evaluating condition"
        );

        // Get the appropriate branch
        let steps_to_execute: Vec<Step> = if condition_result {
            // Execute 'then' branch
            step.params
                .get("then")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .unwrap_or_default()
        } else {
            // Execute 'else' branch
            step.params
                .get("else")
                .and_then(|v| serde_yaml::from_value(v.clone()).ok())
                .unwrap_or_default()
        };

        // Execute the branch steps
        for nested_step in &steps_to_execute {
            // Handle nested condition actions recursively
            if nested_step.action == "condition" {
                // Use Box::pin for recursive async call
                Box::pin(self.execute_condition_action(nested_step, browser, ctx)).await?;
            } else {
                let step_debug = self.resolve_step_debug(ctx, nested_step);
                let output = self.execute_step(nested_step, browser, ctx, &step_debug).await?;

                // Store output
                if let Some(data) = &output.data {
                    if let Some(id) = &nested_step.id {
                        ctx.store_step_output(id, data.clone());
                    }
                }

                // Merge store values
                for (key, value) in output.store {
                    ctx.store_value(key, value);
                }

                // Emit events
                if let Some((event, data)) = output.emit {
                    ctx.emit_event(&event, data);
                }
            }
        }

        Ok(())
    }

    /// Execute a single step
    async fn execute_step(
        &self,
        step: &Step,
        browser: &dyn BrowserHandle,
        ctx: &ExecutionContext,
        step_debug: &ResolvedDebugConfig,
    ) -> Result<ActionOutput, WorkflowError> {
        // Get action from registry
        let action = self
            .registry
            .get(&step.action)
            .ok_or_else(|| WorkflowError::ActionNotFound(step.action.clone()))?;

        // Render parameters with template engine
        let rendered_params = TemplateEngine::render_params(&step.params, ctx);

        debug!(
            workflow = %ctx.workflow_name,
            step = ctx.step_index,
            action = %step.action,
            highlight = step_debug.highlight,
            "Executing action"
        );

        // Highlight element before interaction if configured
        if step_debug.highlight {
            if let Some(selector) = rendered_params.get("selector").and_then(|v| v.as_str()) {
                self.highlight_element(browser, selector).await;
            }
        }

        // Create action context
        let action_ctx = ctx.to_action_context(step.id.as_deref());

        // Execute action
        let output = action
            .execute(&rendered_params, &action_ctx, browser)
            .await?;

        // Handle step-level emit
        if let Some(emit) = &step.emit {
            let mut result = output;
            let data = json!({
                "workflow": ctx.workflow_name,
                "step": ctx.step_index,
                "data": emit.data
            });
            result.emit = Some((emit.event.clone(), data));
            return Ok(result);
        }

        Ok(output)
    }

    /// Evaluate a condition expression
    fn evaluate_condition(&self, condition: &str, ctx: &ExecutionContext) -> bool {
        // Render the condition with template values
        let rendered = TemplateEngine::render(condition, ctx);
        let rendered_lower = rendered.to_lowercase();
        let rendered_trimmed = rendered_lower.trim();

        // Handle equality expressions: "x == y" or "x != y"
        if let Some((left, right)) = rendered_trimmed.split_once("==") {
            let left = left.trim();
            let right = right.trim();
            // Handle != by checking if left ends with !
            if left.ends_with('!') {
                let left = left.trim_end_matches('!').trim();
                return left != right;
            }
            return left == right;
        }

        if let Some((left, right)) = rendered_trimmed.split_once("!=") {
            let left = left.trim();
            let right = right.trim();
            return left != right;
        }

        // Simple truthiness check
        match rendered_trimmed {
            "true" | "yes" | "1" => true,
            "false" | "no" | "0" | "" | "null" | "none" => false,
            _ => !rendered.is_empty() && !rendered.contains("{{"),
        }
    }

    /// Handle error callback
    async fn handle_error(
        &self,
        handler: &ErrorHandler,
        error: &WorkflowError,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
    ) {
        // Store error info
        ctx.store_value(
            "_error",
            json!({
                "message": error.to_string(),
                "step": ctx.step_index,
            }),
        );

        // Emit error event if specified
        if let Some(emit) = &handler.emit {
            ctx.emit_event(
                &emit.event,
                json!({
                    "workflow": ctx.workflow_name,
                    "error": error.to_string(),
                    "step": ctx.step_index,
                }),
            );
        }

        // Take screenshot if requested (also save to debug directory)
        if handler.screenshot.unwrap_or(false) {
            if let Some(path) = self
                .capture_debug_screenshot(browser, ctx, "error_handler")
                .await
            {
                debug_screenshots.push(path);
            }
            // Also store base64 in context for backwards compatibility
            if let Ok(bytes) = browser.screenshot(false).await {
                use base64::{engine::general_purpose::STANDARD, Engine};
                ctx.store_value("_error_screenshot", Value::String(STANDARD.encode(&bytes)));
            }
        }

        // Execute error steps if defined
        let step_debug = ResolvedDebugConfig::default();
        if let Some(steps) = &handler.steps {
            for step in steps {
                if let Err(e) = self.execute_step(step, browser, ctx, &step_debug).await {
                    error!(
                        workflow = %ctx.workflow_name,
                        error = %e,
                        "Error handler step failed"
                    );
                }
            }
        }
    }

    /// Handle completion callback
    async fn handle_complete(
        &self,
        handler: &CompleteHandler,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
    ) {
        // Emit completion event if specified
        if let Some(emit) = &handler.emit {
            ctx.emit_event(
                &emit.event,
                json!({
                    "workflow": ctx.workflow_name,
                    "duration_ms": ctx.duration().num_milliseconds(),
                    "steps": ctx.step_index,
                }),
            );
        }

        // Execute completion steps if defined
        let step_debug = ResolvedDebugConfig::default();
        if let Some(steps) = &handler.steps {
            for step in steps {
                if let Err(e) = self.execute_step(step, browser, ctx, &step_debug).await {
                    error!(
                        workflow = %ctx.workflow_name,
                        error = %e,
                        "Completion handler step failed"
                    );
                }
            }
        }
    }

    /// Resolve step-level debug config by merging with workflow-level config
    fn resolve_step_debug(&self, ctx: &ExecutionContext, step: &Step) -> ResolvedDebugConfig {
        if let Some(step_debug) = &step.debug {
            // Convert workflow's resolved config back to DebugConfig for merge
            let workflow_debug = DebugConfig {
                enabled: Some(ctx.debug.enabled),
                level: Some(ctx.debug.level.clone()),
                capture: Some(ctx.debug.capture.clone()),
                highlight: Some(ctx.debug.highlight),
                delay: Some(ctx.debug.delay),
                pause: Some(ctx.debug.pause),
                console: Some(ctx.debug.console),
                network: Some(ctx.debug.network),
                profile: None, // Profile already applied
            };
            workflow_debug.merge(step_debug).resolve()
        } else {
            ctx.debug.clone()
        }
    }

    /// Capture a debug screenshot and save to disk
    async fn capture_debug_screenshot(
        &self,
        browser: &dyn BrowserHandle,
        ctx: &ExecutionContext,
        phase: &str,
    ) -> Option<String> {
        let debug_dir = &self.debug_dir;
        if let Err(e) = std::fs::create_dir_all(debug_dir) {
            warn!("Failed to create debug directory: {}", e);
            return None;
        }

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let workflow_name = ctx
            .workflow_name
            .replace(|c: char| !c.is_alphanumeric(), "_");
        let filename = format!(
            "{}_{}_step{}_{}.png",
            timestamp, workflow_name, ctx.step_index, phase
        );
        let path = debug_dir.join(&filename);

        match browser.screenshot(false).await {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&path, &bytes) {
                    warn!("Failed to write debug screenshot: {}", e);
                    return None;
                }
                info!(path = %path.display(), "Captured debug screenshot");
                Some(path.to_string_lossy().to_string())
            }
            Err(e) => {
                warn!("Failed to capture debug screenshot: {}", e);
                None
            }
        }
    }

    /// Highlight an element before interaction
    async fn highlight_element(&self, browser: &dyn BrowserHandle, selector: &str) {
        let js = format!(
            r#"
            (function() {{
                try {{
                    const el = document.querySelector('{}');
                    if (el) {{
                        const orig = el.style.outline;
                        const origOffset = el.style.outlineOffset;
                        el.style.outline = '3px solid red';
                        el.style.outlineOffset = '2px';
                        el.scrollIntoView({{ behavior: 'smooth', block: 'center' }});
                        setTimeout(() => {{
                            el.style.outline = orig;
                            el.style.outlineOffset = origOffset;
                        }}, 300);
                    }}
                }} catch (e) {{}}
            }})();
            "#,
            selector.replace('\'', "\\'")
        );

        if let Err(e) = browser.eval(&js).await {
            debug!("Element highlight failed: {}", e);
        }

        // Brief pause to let the highlight be visible
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}
