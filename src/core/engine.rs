//! Workflow Execution Engine
//!
//! Executes workflows step by step with browser automation.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::actions::{ActionError, ActionOutput, ActionRegistry, BrowserHandle};
use crate::utils::yaml_to_json;
use crate::workflow::schema::{CaptureMode, DebugConfig, ResolvedDebugConfig, StepHandler};
use crate::workflow::{CompleteHandler, ErrorHandler, Step, Workflow, WorkflowResolver};

use super::context::ExecutionContext;
use super::template::TemplateEngine;

/// How execution proceeds after a dispatch action's outcome handlers ran.
enum DispatchFlow {
    /// Advance to the next step
    Next,
    /// Jump to the given step index
    Jump(usize),
    /// Fail the workflow with this error
    Fail(WorkflowError),
}

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

/// Interactive pause handler that prompts on stdin.
pub struct ShellPauseHandler;

#[async_trait::async_trait]
impl PauseHandler for ShellPauseHandler {
    async fn on_pause(
        &self,
        workflow: &str,
        step: usize,
        action: &str,
        selector: Option<&str>,
    ) -> PauseResponse {
        let sel = selector.unwrap_or("-");
        println!(
            "\n⏸  Paused at step {} ({}) selector={} in '{}'",
            step, action, sel, workflow
        );
        println!("   [c]ontinue  [s]kip  [a]bort");

        // Read from stdin on a blocking thread so we don't block the runtime
        let response = tokio::task::spawn_blocking(|| loop {
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_err() {
                return PauseResponse::Continue;
            }
            match input.trim().to_lowercase().as_str() {
                "c" | "continue" | "" => return PauseResponse::Continue,
                "s" | "skip" => return PauseResponse::Skip,
                "a" | "abort" => return PauseResponse::Abort,
                _ => println!("   [c]ontinue  [s]kip  [a]bort"),
            }
        })
        .await;

        response.unwrap_or(PauseResponse::Continue)
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

    #[error("Workflow cancelled")]
    Cancelled,

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

/// Maximum nesting depth for workflow `call` actions
const MAX_CALL_DEPTH: usize = 16;

/// Workflow execution engine
pub struct WorkflowEngine {
    /// Action registry
    registry: ActionRegistry,

    /// Event handlers (event name -> list of workflows to trigger)
    event_handlers: Arc<RwLock<HashMap<String, Vec<String>>>>,

    /// Debug output directory
    debug_dir: std::path::PathBuf,

    /// Optional resolver for sub-workflow `call` actions
    resolver: Option<Arc<dyn WorkflowResolver>>,
}

impl WorkflowEngine {
    /// Create a new workflow engine
    pub fn new() -> Self {
        Self {
            registry: ActionRegistry::new(),
            event_handlers: Arc::new(RwLock::new(HashMap::new())),
            debug_dir: std::path::PathBuf::from("data/debug"),
            resolver: None,
        }
    }

    /// Set the debug output directory
    pub fn set_debug_dir(&mut self, dir: std::path::PathBuf) {
        self.debug_dir = dir;
    }

    /// Create a new workflow engine with a workflow resolver for `call` actions
    pub fn with_resolver(resolver: Arc<dyn WorkflowResolver>) -> Self {
        Self {
            registry: ActionRegistry::new(),
            event_handlers: Arc::new(RwLock::new(HashMap::new())),
            debug_dir: std::path::PathBuf::from("data/debug"),
            resolver: Some(resolver),
        }
    }

    /// Set the workflow resolver (for `call` actions)
    pub fn set_resolver(&mut self, resolver: Arc<dyn WorkflowResolver>) {
        self.resolver = Some(resolver);
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
            None,
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
        cancel_token: Option<CancellationToken>,
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
            .execute_steps(
                workflow,
                browser,
                &mut ctx,
                &mut debug_screenshots,
                pause_handler,
                &cancel_token,
            )
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
        cancel_token: &Option<CancellationToken>,
    ) -> Result<Value, WorkflowError> {
        while ctx.step_index < workflow.steps.len() {
            // Check cancellation before each step
            if let Some(token) = cancel_token {
                if token.is_cancelled() {
                    return Err(WorkflowError::Cancelled);
                }
            }

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
                if let Some(path) = self.capture_debug_screenshot(browser, ctx, "before").await {
                    debug_screenshots.push(path);
                }
            }

            // Handle `call` / `condition` actions — dispatched before the
            // step-level `if:` gate (condition consumes `if:` as its own
            // condition; call predates the gate). Their on_success/on_failure
            // handlers still run via apply_dispatch_hooks.
            if step.action == "call" || step.action == "condition" {
                let outcome = if step.action == "call" {
                    self.execute_call_action(
                        step,
                        browser,
                        ctx,
                        debug_screenshots,
                        pause_handler,
                        cancel_token,
                    )
                    .await
                } else {
                    self.execute_condition_action(
                        workflow,
                        step,
                        browser,
                        ctx,
                        debug_screenshots,
                        pause_handler,
                        cancel_token,
                    )
                    .await
                };
                match self
                    .apply_dispatch_hooks(
                        workflow,
                        step,
                        outcome,
                        browser,
                        ctx,
                        debug_screenshots,
                        pause_handler,
                        cancel_token,
                    )
                    .await?
                {
                    DispatchFlow::Jump(idx) => {
                        ctx.goto_step(idx);
                        continue;
                    }
                    DispatchFlow::Fail(err) => return Err(err),
                    DispatchFlow::Next => {
                        ctx.next_step();
                        continue;
                    }
                }
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

            // Handle `loop` action — after the step-level `if:` gate so a loop
            // can be skipped by its own condition.
            if step.action == "loop" {
                let outcome = self
                    .execute_loop_action(
                        workflow,
                        step,
                        browser,
                        ctx,
                        debug_screenshots,
                        pause_handler,
                        cancel_token,
                    )
                    .await;
                match self
                    .apply_dispatch_hooks(
                        workflow,
                        step,
                        outcome,
                        browser,
                        ctx,
                        debug_screenshots,
                        pause_handler,
                        cancel_token,
                    )
                    .await?
                {
                    DispatchFlow::Jump(idx) => {
                        ctx.goto_step(idx);
                        continue;
                    }
                    DispatchFlow::Fail(err) => return Err(err),
                    DispatchFlow::Next => {
                        ctx.next_step();
                        continue;
                    }
                }
            }

            // Execute step with retries
            let result = self
                .execute_step_with_retry(
                    step,
                    browser,
                    ctx,
                    &step_debug,
                    debug_screenshots,
                    pause_handler,
                )
                .await;

            // Handle execution result
            let output = match result {
                Ok(output) => {
                    // Capture screenshot after step if configured
                    if matches!(step_debug.capture, CaptureMode::After | CaptureMode::All) {
                        if let Some(path) =
                            self.capture_debug_screenshot(browser, ctx, "after").await
                        {
                            debug_screenshots.push(path);
                        }
                    }
                    output
                }
                Err(e) => {
                    // Capture screenshot on failure if configured
                    if matches!(step_debug.capture, CaptureMode::Failure | CaptureMode::All) {
                        if let Some(path) =
                            self.capture_debug_screenshot(browser, ctx, "failure").await
                        {
                            debug_screenshots.push(path);
                        }
                    }

                    // Run step-level on_failure handler; a `goto` in the
                    // handler can recover from the failure and continue.
                    if let Some(handler) = &step.on_failure {
                        match self
                            .run_step_handler(
                                handler,
                                "on_failure",
                                workflow,
                                browser,
                                ctx,
                                debug_screenshots,
                                pause_handler,
                                cancel_token,
                                Some(e.to_string()),
                            )
                            .await
                        {
                            Ok(Some(idx)) => {
                                ctx.goto_step(idx);
                                continue;
                            }
                            Ok(None) => {}
                            Err(handler_err) => return Err(handler_err),
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

            // Run step-level on_success handler; `goto` jumps, `abort` fails here
            if let Some(handler) = &step.on_success {
                if let Some(idx) = self
                    .run_step_handler(
                        handler,
                        "on_success",
                        workflow,
                        browser,
                        ctx,
                        debug_screenshots,
                        pause_handler,
                        cancel_token,
                        None,
                    )
                    .await?
                {
                    ctx.goto_step(idx);
                    continue;
                }
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
                            .capture_debug_screenshot(
                                browser,
                                ctx,
                                &format!("retry{}_failure", attempt),
                            )
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

    /// Execute a `call` action — invoke a sub-workflow by name.
    ///
    /// The sub-workflow runs in the same browser context, sharing the page.
    /// Its output is stored under the step's `store_as` or step `id`.
    async fn execute_call_action(
        &self,
        step: &Step,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
        pause_handler: &dyn PauseHandler,
        cancel_token: &Option<CancellationToken>,
    ) -> Result<(), WorkflowError> {
        // Check recursion depth
        if ctx.call_depth >= MAX_CALL_DEPTH {
            return Err(WorkflowError::InvalidConfig(format!(
                "Maximum call depth ({}) exceeded — possible recursive workflow loop",
                MAX_CALL_DEPTH
            )));
        }

        // Resolve the workflow name
        let workflow_name = step
            .params
            .get("workflow")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                WorkflowError::InvalidConfig("call action requires 'workflow' parameter".into())
            })?;

        // Render the name through templates (supports {{params.x}})
        let rendered_name = TemplateEngine::render(workflow_name, ctx);

        let resolver = self.resolver.as_ref().ok_or_else(|| {
            WorkflowError::InvalidConfig(
                "No workflow resolver configured — cannot use 'call' action. \
                 Ensure the engine was created with WorkflowEngine::with_resolver()."
                    .into(),
            )
        })?;

        let sub_workflow = resolver.resolve(&rendered_name).await.map_err(|e| {
            WorkflowError::NotFound(format!("Called workflow '{}': {}", rendered_name, e))
        })?;

        info!(
            workflow = %ctx.workflow_name,
            calling = %sub_workflow.name,
            depth = ctx.call_depth + 1,
            "Calling sub-workflow"
        );

        // Build sub-workflow params from the call step
        let mut sub_params: HashMap<String, serde_json::Value> = HashMap::new();
        if let Some(params_val) = step.params.get("params") {
            if let Some(mapping) = params_val.as_mapping() {
                for (k, v) in mapping {
                    if let Some(key) = k.as_str() {
                        let rendered = TemplateEngine::render_yaml(v, ctx);
                        sub_params.insert(key.to_string(), yaml_to_json(&rendered));
                    }
                }
            }
        }

        // Provide defaults from sub-workflow param definitions
        for (name, def) in &sub_workflow.params {
            if !sub_params.contains_key(name) {
                if let Some(default) = &def.default {
                    sub_params.insert(name.clone(), yaml_to_json(default));
                }
            }
        }

        // Create child execution context
        let child_debug = if let Some(step_dbg) = &step.debug {
            let parent_debug = DebugConfig {
                enabled: Some(ctx.debug.enabled),
                level: Some(ctx.debug.level),
                capture: Some(ctx.debug.capture),
                highlight: Some(ctx.debug.highlight),
                delay: Some(ctx.debug.delay),
                pause: Some(ctx.debug.pause),
                console: Some(ctx.debug.console),
                network: Some(ctx.debug.network),
                profile: None,
            };
            parent_debug
                .merge(step_dbg)
                .merge(&sub_workflow.debug)
                .resolve()
        } else {
            sub_workflow.debug.clone().with_profile().resolve()
        };

        let mut child_ctx = ExecutionContext::new(&sub_workflow.name, &ctx.instance_id)
            .with_debug(child_debug)
            .with_params(sub_params);
        child_ctx.call_depth = ctx.call_depth + 1;
        child_ctx.tab_index = ctx.tab_index;

        // Set sub-workflow variables
        if let Some(vars) = &sub_workflow.vars {
            for (key, value) in vars {
                child_ctx.vars.insert(key.clone(), yaml_to_json(value));
            }
        }

        // Execute sub-workflow steps (Box::pin for recursive async)
        let result = Box::pin(self.execute_steps(
            &sub_workflow,
            browser,
            &mut child_ctx,
            debug_screenshots,
            pause_handler,
            cancel_token,
        ))
        .await;

        // Propagate tab index changes back to parent
        ctx.tab_index = child_ctx.tab_index;

        match result {
            Ok(output) => {
                info!(
                    workflow = %ctx.workflow_name,
                    called = %sub_workflow.name,
                    duration_ms = child_ctx.duration().num_milliseconds(),
                    "Sub-workflow completed successfully"
                );

                // Store output under store_as or step id
                let store_key = step
                    .params
                    .get("store_as")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .or_else(|| step.id.clone());

                if let Some(key) = store_key {
                    ctx.store_value(key, output);
                }

                // Propagate events from sub-workflow
                for (event, data) in child_ctx.events {
                    ctx.emit_event(event, data);
                }

                Ok(())
            }
            Err(e) => {
                error!(
                    workflow = %ctx.workflow_name,
                    called = %sub_workflow.name,
                    error = %e,
                    "Sub-workflow failed"
                );
                Err(e)
            }
        }
    }

    /// Execute a nested block of steps (condition branches, loop bodies,
    /// `steps:` handlers) in a child context that inherits the parent's
    /// variables, store, step outputs and tab state.
    ///
    /// After the block, child mutations are merged back into the parent and
    /// `vars_overlay` keys are restored to their pre-block values so item
    /// variables from loops don't leak past the block.
    ///
    /// Blocks increment `call_depth`, giving nested loops/conditions the same
    /// runaway protection as recursive `call`s (MAX_CALL_DEPTH).
    #[allow(clippy::too_many_arguments)]
    async fn execute_block(
        &self,
        parent: &Workflow,
        block_name: &str,
        steps: Vec<Step>,
        vars_overlay: HashMap<String, Value>,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
        pause_handler: &dyn PauseHandler,
        cancel_token: &Option<CancellationToken>,
    ) -> Result<Value, WorkflowError> {
        if steps.is_empty() {
            return Ok(Value::Null);
        }
        if ctx.call_depth >= MAX_CALL_DEPTH {
            return Err(WorkflowError::InvalidConfig(format!(
                "Maximum call depth ({}) exceeded — possible workflow loop",
                MAX_CALL_DEPTH
            )));
        }

        // Remember pre-block values of overlay keys so they can be restored
        let saved_overlay: Vec<(String, Option<Value>)> = vars_overlay
            .keys()
            .map(|k| (k.clone(), ctx.vars.get(k).cloned()))
            .collect();

        // Temp workflow sharing everything with the parent except the step list
        let mut block_workflow = parent.clone();
        block_workflow.name = format!("{}[{}]", parent.name, block_name);
        block_workflow.steps = steps;
        block_workflow.output = None;
        block_workflow.on_complete = None;
        block_workflow.on_error = None;

        let mut child = ExecutionContext::new(&block_workflow.name, &ctx.instance_id)
            .with_debug(ctx.debug.clone())
            .with_params(ctx.params.clone());
        child.call_depth = ctx.call_depth + 1;
        child.tab_index = ctx.tab_index;
        child.vars = ctx.vars.clone();
        child.store = ctx.store.clone();
        child.step_outputs = ctx.step_outputs.clone();
        for (key, value) in vars_overlay {
            child.vars.insert(key, value);
        }

        let result = Box::pin(self.execute_steps(
            &block_workflow,
            browser,
            &mut child,
            debug_screenshots,
            pause_handler,
            cancel_token,
        ))
        .await;

        // Merge child state back (also on error: keep partial progress)
        ctx.tab_index = child.tab_index;
        for (key, value) in std::mem::take(&mut child.vars) {
            ctx.vars.insert(key, value);
        }
        for (key, value) in std::mem::take(&mut child.store) {
            ctx.store.insert(key, value);
        }
        for (key, value) in std::mem::take(&mut child.step_outputs) {
            ctx.step_outputs.insert(key, value);
        }
        for (name, data) in std::mem::take(&mut child.events) {
            ctx.emit_event(name, data);
        }

        // Restore overlay keys (loop item vars etc.) to pre-block values
        for (key, saved) in saved_overlay {
            match saved {
                Some(value) => {
                    ctx.vars.insert(key, value);
                }
                None => {
                    ctx.vars.remove(&key);
                }
            }
        }

        result
    }

    /// Execute a `condition` action by running the selected then/else branch
    /// through [`Self::execute_block`] — branches therefore support the full
    /// step vocabulary (retry, `if:`, `call`, `loop`, handlers, goto, …).
    #[allow(clippy::too_many_arguments)]
    async fn execute_condition_action(
        &self,
        workflow: &Workflow,
        step: &Step,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
        pause_handler: &dyn PauseHandler,
        cancel_token: &Option<CancellationToken>,
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
        let branch = if condition_result { "then" } else { "else" };
        let steps_to_execute: Vec<Step> = step
            .params
            .get(branch)
            .and_then(|v| serde_yaml::from_value(v.clone()).ok())
            .unwrap_or_default();

        self.execute_block(
            workflow,
            branch,
            steps_to_execute,
            HashMap::new(),
            browser,
            ctx,
            debug_screenshots,
            pause_handler,
            cancel_token,
        )
        .await?;

        Ok(())
    }

    /// Execute a `loop` action: iterate `items` and run `steps` once per item
    /// with the current item bound to the `as` variable (and optionally the
    /// zero-based index to `index_as`), each iteration as an isolated block.
    #[allow(clippy::too_many_arguments)]
    async fn execute_loop_action(
        &self,
        workflow: &Workflow,
        step: &Step,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
        pause_handler: &dyn PauseHandler,
        cancel_token: &Option<CancellationToken>,
    ) -> Result<(), WorkflowError> {
        let items_param = step.params.get("items").ok_or_else(|| {
            WorkflowError::InvalidConfig("loop action requires 'items' parameter".into())
        })?;
        let item_var = step
            .params
            .get("as")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                WorkflowError::InvalidConfig("loop action requires 'as' parameter".into())
            })?;
        let index_var = step
            .params
            .get("index_as")
            .and_then(|v| v.as_str())
            .map(String::from);

        let body: Vec<Step> = step
            .params
            .get("steps")
            .cloned()
            .and_then(|v| serde_yaml::from_value(v).ok())
            .ok_or_else(|| {
                WorkflowError::InvalidConfig("loop action requires 'steps' to be a list".into())
            })?;
        if body.is_empty() {
            return Ok(());
        }

        // Items resolve through templates: Sequence = list, Null = empty,
        // anything else = single item
        let rendered_items = TemplateEngine::render_yaml(items_param, ctx);
        let items: Vec<serde_yaml::Value> = match rendered_items {
            serde_yaml::Value::Sequence(seq) => seq,
            serde_yaml::Value::Null => Vec::new(),
            other => vec![other],
        };

        debug!(
            workflow = %ctx.workflow_name,
            step = ctx.step_index,
            items = items.len(),
            item_var = %item_var,
            "Executing loop"
        );

        for (idx, item) in items.iter().enumerate() {
            let mut overlay: HashMap<String, Value> = HashMap::new();
            overlay.insert(item_var.to_string(), yaml_to_json(item));
            if let Some(index_key) = &index_var {
                overlay.insert(index_key.clone(), json!(idx));
            }

            self.execute_block(
                workflow,
                &format!("loop[{}]", idx),
                body.clone(),
                overlay,
                browser,
                ctx,
                debug_screenshots,
                pause_handler,
                cancel_token,
            )
            .await?;
        }

        Ok(())
    }

    /// Run `on_success`/`on_failure` handlers for a dispatch action
    /// (call/condition/loop) and decide how execution proceeds.
    #[allow(clippy::too_many_arguments)]
    async fn apply_dispatch_hooks(
        &self,
        workflow: &Workflow,
        step: &Step,
        outcome: Result<(), WorkflowError>,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
        pause_handler: &dyn PauseHandler,
        cancel_token: &Option<CancellationToken>,
    ) -> Result<DispatchFlow, WorkflowError> {
        match outcome {
            Ok(()) => {
                if let Some(handler) = &step.on_success {
                    if let Some(idx) = self
                        .run_step_handler(
                            handler,
                            "on_success",
                            workflow,
                            browser,
                            ctx,
                            debug_screenshots,
                            pause_handler,
                            cancel_token,
                            None,
                        )
                        .await?
                    {
                        return Ok(DispatchFlow::Jump(idx));
                    }
                }
                Ok(DispatchFlow::Next)
            }
            Err(e) => {
                if let Some(handler) = &step.on_failure {
                    if let Some(idx) = self
                        .run_step_handler(
                            handler,
                            "on_failure",
                            workflow,
                            browser,
                            ctx,
                            debug_screenshots,
                            pause_handler,
                            cancel_token,
                            Some(e.to_string()),
                        )
                        .await?
                    {
                        return Ok(DispatchFlow::Jump(idx));
                    }
                }
                Ok(DispatchFlow::Fail(e))
            }
        }
    }

    /// Run a step-level handler (`on_success` / `on_failure`).
    ///
    /// Returns `Some(index)` when the handler jumps via `goto` — the caller
    /// must `goto_step(index)` and continue. `Abort` yields [`WorkflowError::Aborted`].
    #[allow(clippy::too_many_arguments)]
    async fn run_step_handler(
        &self,
        handler: &StepHandler,
        kind: &str,
        workflow: &Workflow,
        browser: &dyn BrowserHandle,
        ctx: &mut ExecutionContext,
        debug_screenshots: &mut Vec<String>,
        pause_handler: &dyn PauseHandler,
        cancel_token: &Option<CancellationToken>,
        error_context: Option<String>,
    ) -> Result<Option<usize>, WorkflowError> {
        match handler {
            StepHandler::Goto { goto } => {
                let idx = workflow
                    .steps
                    .iter()
                    .position(|s| s.id.as_deref() == Some(goto.as_str()))
                    .ok_or_else(|| {
                        WorkflowError::InvalidConfig(format!(
                            "{} handler: no step with id '{}' in workflow",
                            kind, goto
                        ))
                    })?;
                info!(
                    workflow = %ctx.workflow_name,
                    step = ctx.step_index,
                    handler = kind,
                    target = %goto,
                    "Handler jumping to step"
                );
                Ok(Some(idx))
            }
            StepHandler::Abort { abort, error } => {
                if *abort {
                    let message = match error {
                        Some(msg) => TemplateEngine::render(msg, ctx),
                        None => error_context
                            .clone()
                            .unwrap_or_else(|| format!("{} handler aborted the workflow", kind)),
                    };
                    Err(WorkflowError::Aborted(message))
                } else {
                    Ok(None)
                }
            }
            StepHandler::Emit { emit, data } => {
                let name = TemplateEngine::render(emit, ctx);
                let mut payload = serde_json::Map::new();
                if let Some(err) = &error_context {
                    payload.insert("error".into(), Value::String(err.clone()));
                }
                payload.insert("workflow".into(), json!(ctx.workflow_name));
                payload.insert("step".into(), json!(ctx.step_index));
                payload.insert("handler".into(), json!(kind));
                for (key, value) in data {
                    let rendered = TemplateEngine::render_yaml(value, ctx);
                    payload.insert(key.clone(), yaml_to_json(&rendered));
                }
                ctx.emit_event(&name, Value::Object(payload));
                Ok(None)
            }
            StepHandler::Steps { steps } | StepHandler::StepsList(steps) => {
                if steps.is_empty() {
                    return Ok(None);
                }
                self.execute_block(
                    workflow,
                    kind,
                    steps.clone(),
                    HashMap::new(),
                    browser,
                    ctx,
                    debug_screenshots,
                    pause_handler,
                    cancel_token,
                )
                .await?;
                Ok(None)
            }
        }
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

        // Handle step-level emit (data values are template-rendered)
        if let Some(emit) = &step.emit {
            let mut result = output;
            let mut data_map = serde_json::Map::new();
            if let Some(emit_data) = &emit.data {
                for (key, value) in emit_data {
                    let rendered = TemplateEngine::render_yaml(value, ctx);
                    data_map.insert(key.clone(), yaml_to_json(&rendered));
                }
            }
            let data = json!({
                "workflow": ctx.workflow_name,
                "step": ctx.step_index,
                "data": Value::Object(data_map)
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
                level: Some(ctx.debug.level),
                capture: Some(ctx.debug.capture),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::TabInfo;
    use crate::workflow::{Workflow, WorkflowParser, WorkflowResolver};

    /// Mock browser that does nothing (for testing non-browser actions)
    struct MockBrowser;

    #[async_trait::async_trait]
    impl BrowserHandle for MockBrowser {
        async fn goto(&self, _url: &str) -> Result<(), ActionError> {
            Ok(())
        }
        async fn click(&self, _sel: &str) -> Result<(), ActionError> {
            Ok(())
        }
        async fn type_text(
            &self,
            _sel: &str,
            _text: &str,
            _clear: bool,
        ) -> Result<(), ActionError> {
            Ok(())
        }
        async fn get_text(&self, _sel: &str) -> Result<String, ActionError> {
            Ok(String::new())
        }
        async fn get_attribute(
            &self,
            _sel: &str,
            _attr: &str,
        ) -> Result<Option<String>, ActionError> {
            Ok(None)
        }
        async fn wait_for(&self, _sel: &str, _timeout: u64) -> Result<(), ActionError> {
            Ok(())
        }
        async fn wait_for_hidden(&self, _sel: &str, _timeout: u64) -> Result<(), ActionError> {
            Ok(())
        }
        async fn wait_for_url(&self, _cond: &str, _timeout: u64) -> Result<(), ActionError> {
            Ok(())
        }
        async fn screenshot(&self, _full: bool) -> Result<Vec<u8>, ActionError> {
            Ok(vec![])
        }
        async fn eval(&self, _script: &str) -> Result<Value, ActionError> {
            Ok(Value::Null)
        }
        async fn current_url(&self) -> Result<String, ActionError> {
            Ok("about:blank".into())
        }
        async fn back(&self) -> Result<(), ActionError> {
            Ok(())
        }
        async fn forward(&self) -> Result<(), ActionError> {
            Ok(())
        }
        async fn reload(&self) -> Result<(), ActionError> {
            Ok(())
        }
        async fn new_tab(&self, _url: Option<&str>) -> Result<usize, ActionError> {
            Ok(0)
        }
        async fn switch_tab(&self, _idx: usize) -> Result<(), ActionError> {
            Ok(())
        }
        async fn close_tab(&self, _idx: usize) -> Result<(), ActionError> {
            Ok(())
        }
        async fn tab_count(&self) -> Result<usize, ActionError> {
            Ok(1)
        }
        async fn list_tabs(&self) -> Result<Vec<TabInfo>, ActionError> {
            Ok(vec![])
        }
        async fn set_file_input_files(
            &self,
            _sel: &str,
            _paths: Vec<String>,
        ) -> Result<(), ActionError> {
            Ok(())
        }
        async fn set_file_chooser_intercept(&self, _enabled: bool) -> Result<(), ActionError> {
            Ok(())
        }
        async fn upload_via_file_chooser(
            &self,
            _trigger: Option<&str>,
            _paths: Vec<String>,
            _timeout: u64,
        ) -> Result<(), ActionError> {
            Ok(())
        }
        async fn pdf(&self) -> Result<Vec<u8>, ActionError> {
            Ok(vec![])
        }
    }

    /// Mock resolver that returns workflows from a HashMap
    struct MockResolver {
        workflows: HashMap<String, Workflow>,
    }

    impl MockResolver {
        fn new() -> Self {
            Self {
                workflows: HashMap::new(),
            }
        }

        fn add(&mut self, yaml: &str) -> String {
            let wf = WorkflowParser::parse(yaml).expect("valid yaml");
            let name = wf.name.clone();
            self.workflows.insert(name.clone(), wf);
            name
        }
    }

    #[async_trait::async_trait]
    impl WorkflowResolver for MockResolver {
        async fn resolve(&self, name: &str) -> anyhow::Result<Workflow> {
            self.workflows
                .get(name)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Workflow '{}' not found", name))
        }
    }

    #[tokio::test]
    async fn test_call_action_basic() {
        // Sub-workflow that logs and stores a value
        let mut resolver = MockResolver::new();
        resolver.add(
            r#"
name: greet
params:
  name:
    type: string
    default: "World"
steps:
  - action: log
    message: "Hello {{params.name}}"
  - id: result
    action: eval
    script: "return 'greeted'"
    store_as: greeting_status
output:
  status: "{{store.greeting_status}}"
"#,
        );

        let engine = WorkflowEngine::with_resolver(Arc::new(resolver));
        let browser = MockBrowser;

        // Parent workflow calls sub-workflow
        let parent = WorkflowParser::parse(
            r#"
name: parent
steps:
  - action: call
    workflow: greet
    params:
      name: "Alice"
    store_as: greet_result
  - action: log
    message: "Done"
"#,
        )
        .unwrap();

        let result = engine
            .execute(&parent, &browser, HashMap::new())
            .await
            .unwrap();
        assert!(result.success, "Parent workflow should succeed");
        assert_eq!(result.steps_executed, 2);
    }

    #[tokio::test]
    async fn test_call_action_nested() {
        // Three-level nesting: A calls B calls C
        let mut resolver = MockResolver::new();
        resolver.add(
            r#"
name: level_c
steps:
  - action: log
    message: "Level C"
"#,
        );
        resolver.add(
            r#"
name: level_b
steps:
  - action: call
    workflow: level_c
  - action: log
    message: "Level B"
"#,
        );

        let engine = WorkflowEngine::with_resolver(Arc::new(resolver));
        let browser = MockBrowser;

        let top = WorkflowParser::parse(
            r#"
name: level_a
steps:
  - action: call
    workflow: level_b
  - action: log
    message: "Level A"
"#,
        )
        .unwrap();

        let result = engine
            .execute(&top, &browser, HashMap::new())
            .await
            .unwrap();
        assert!(result.success, "Three-level nested call should succeed");
    }

    #[tokio::test]
    async fn test_call_action_no_resolver() {
        // Engine without resolver should fail on call action
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: no-resolver
steps:
  - action: call
    workflow: something
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(!result.success, "Should fail without resolver");
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("No workflow resolver"));
    }

    #[tokio::test]
    async fn test_call_action_not_found() {
        let resolver = MockResolver::new(); // empty
        let engine = WorkflowEngine::with_resolver(Arc::new(resolver));
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: missing-call
steps:
  - action: call
    workflow: nonexistent
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(!result.success, "Should fail for missing workflow");
        assert!(result.error.as_ref().unwrap().contains("nonexistent"));
    }

    #[tokio::test]
    async fn test_call_action_max_depth() {
        // Create a workflow that calls itself
        let mut resolver = MockResolver::new();
        resolver.add(
            r#"
name: recursive
steps:
  - action: call
    workflow: recursive
"#,
        );

        let engine = WorkflowEngine::with_resolver(Arc::new(resolver));
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: start-recursion
steps:
  - action: call
    workflow: recursive
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(!result.success, "Should fail at max depth");
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("Maximum call depth"));
    }

    #[tokio::test]
    async fn test_call_action_params_forwarding() {
        let mut resolver = MockResolver::new();
        resolver.add(
            r#"
name: echo
params:
  message:
    type: string
    required: true
steps:
  - action: log
    message: "{{params.message}}"
"#,
        );

        let engine = WorkflowEngine::with_resolver(Arc::new(resolver));
        let browser = MockBrowser;

        let parent = WorkflowParser::parse(
            r#"
name: caller
params:
  greeting:
    type: string
    default: "hi"
steps:
  - action: call
    workflow: echo
    params:
      message: "{{params.greeting}}"
"#,
        )
        .unwrap();

        let mut params = HashMap::new();
        params.insert(
            "greeting".to_string(),
            serde_json::Value::String("hello world".into()),
        );

        let result = engine.execute(&parent, &browser, params).await.unwrap();
        assert!(result.success, "Param forwarding should work");
    }

    #[tokio::test]
    async fn test_call_action_with_condition() {
        let mut resolver = MockResolver::new();
        resolver.add(
            r#"
name: optional-step
steps:
  - action: log
    message: "ran optional step"
"#,
        );

        let engine = WorkflowEngine::with_resolver(Arc::new(resolver));
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: conditional-call
steps:
  - action: call
    workflow: optional-step
    if: "false"
  - action: log
    message: "after conditional call"
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success);
        // The call should be skipped due to condition, so only 1 step actually runs
        // (the log step, since the call is skipped)
        assert_eq!(result.steps_executed, 2);
    }

    // --- Pause handler tests ---

    /// Custom pause handler that records calls and returns a configured response
    struct TestPauseHandler {
        response: PauseResponse,
        calls: std::sync::Mutex<Vec<(String, usize, String)>>,
    }

    impl TestPauseHandler {
        fn new(response: PauseResponse) -> Self {
            Self {
                response: response,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl PauseHandler for TestPauseHandler {
        async fn on_pause(
            &self,
            workflow: &str,
            step: usize,
            action: &str,
            _selector: Option<&str>,
        ) -> PauseResponse {
            self.calls
                .lock()
                .unwrap()
                .push((workflow.to_string(), step, action.to_string()));
            self.response.clone()
        }
    }

    #[tokio::test]
    async fn test_pause_handler_continue() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;
        let handler = TestPauseHandler::new(PauseResponse::Continue);

        let wf = WorkflowParser::parse(
            r#"
name: pause-test
debug:
  enabled: true
  pause: true
steps:
  - action: log
    message: "step one"
  - action: log
    message: "step two"
"#,
        )
        .unwrap();

        let debug = wf.debug.resolve();
        let result = engine
            .execute_with_pause_handler(&wf, &browser, HashMap::new(), debug, &handler, None)
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.steps_executed, 2);
        assert_eq!(
            handler.call_count(),
            2,
            "Pause handler should be called for each step"
        );
    }

    #[tokio::test]
    async fn test_pause_handler_skip() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;
        let handler = TestPauseHandler::new(PauseResponse::Skip);

        let wf = WorkflowParser::parse(
            r#"
name: skip-test
debug:
  enabled: true
  pause: true
steps:
  - action: log
    message: "step one"
  - action: log
    message: "step two"
"#,
        )
        .unwrap();

        let debug = wf.debug.resolve();
        let result = engine
            .execute_with_pause_handler(&wf, &browser, HashMap::new(), debug, &handler, None)
            .await
            .unwrap();
        assert!(result.success);
        // Both steps are skipped, but step_index still advances
        assert_eq!(handler.call_count(), 2);
    }

    #[tokio::test]
    async fn test_pause_handler_abort() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;
        let handler = TestPauseHandler::new(PauseResponse::Abort);

        let wf = WorkflowParser::parse(
            r#"
name: abort-test
debug:
  enabled: true
  pause: true
steps:
  - action: log
    message: "step one"
  - action: log
    message: "step two"
"#,
        )
        .unwrap();

        let debug = wf.debug.resolve();
        let result = engine
            .execute_with_pause_handler(&wf, &browser, HashMap::new(), debug, &handler, None)
            .await
            .unwrap();
        assert!(!result.success, "Abort should fail the workflow");
        assert!(
            result.error.as_ref().unwrap().contains("abort"),
            "Error should mention abort"
        );
        assert_eq!(
            handler.call_count(),
            1,
            "Only first step should pause before abort"
        );
    }

    #[tokio::test]
    async fn test_pause_not_called_when_disabled() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;
        let handler = TestPauseHandler::new(PauseResponse::Continue);

        let wf = WorkflowParser::parse(
            r#"
name: no-pause-test
steps:
  - action: log
    message: "step one"
  - action: log
    message: "step two"
"#,
        )
        .unwrap();

        let result = engine
            .execute_with_pause_handler(
                &wf,
                &browser,
                HashMap::new(),
                ResolvedDebugConfig::default(),
                &handler,
                None,
            )
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(
            handler.call_count(),
            0,
            "Pause handler should NOT be called when pause is disabled"
        );
    }

    #[tokio::test]
    async fn test_default_pause_handler_always_continues() {
        let handler = DefaultPauseHandler;
        let response = handler.on_pause("wf", 0, "click", Some("button")).await;
        assert_eq!(response, PauseResponse::Continue);
    }

    // --- Loop action tests ---

    #[tokio::test]
    async fn test_loop_basic_iterates_all_items() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: loop-basic
steps:
  - action: loop
    items: ["alpha", "beta", "gamma"]
    as: item
    steps:
      - action: emit
        event: each
        data:
          v: "{{vars.item}}"
  - action: emit
    event: after_loop
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "loop should succeed: {:?}", result.error);

        let each: Vec<&Value> = result
            .events
            .iter()
            .filter(|(n, _)| n == "each")
            .map(|(_, d)| d)
            .collect();
        assert_eq!(each.len(), 3, "loop body should run once per item");
        assert_eq!(each[0]["data"]["v"], json!("alpha"));
        assert_eq!(each[1]["data"]["v"], json!("beta"));
        assert_eq!(each[2]["data"]["v"], json!("gamma"));
        assert!(result.events.iter().any(|(n, _)| n == "after_loop"));
    }

    #[tokio::test]
    async fn test_loop_index_is_zero_based() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: loop-index
steps:
  - action: loop
    items: ["x", "y"]
    as: val
    index_as: idx
    steps:
      - action: emit
        event: iter
        data:
          i: "{{vars.idx}}"
          v: "{{vars.val}}"
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);

        let idxs: Vec<&Value> = result
            .events
            .iter()
            .filter(|(n, _)| n == "iter")
            .map(|(_, d)| &d["data"]["i"])
            .collect();
        assert_eq!(idxs, vec![&json!("0"), &json!("1")]);
        let vals: Vec<&Value> = result
            .events
            .iter()
            .filter(|(n, _)| n == "iter")
            .map(|(_, d)| &d["data"]["v"])
            .collect();
        assert_eq!(vals, vec![&json!("x"), &json!("y")]);
    }

    #[tokio::test]
    async fn test_loop_item_var_does_not_leak_after_loop() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: loop-no-leak
steps:
  - action: loop
    items: ["leak"]
    as: item
    steps:
      - action: emit
        event: iter
  - action: condition
    if: "{{vars.item}} == leak"
    then:
      - action: emit
        event: leaked
    else:
      - action: emit
        event: clean
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        assert!(result.events.iter().any(|(n, _)| n == "clean"));
        assert!(
            !result.events.iter().any(|(n, _)| n == "leaked"),
            "loop item var must be scoped to the loop"
        );
    }

    #[tokio::test]
    async fn test_loop_respects_step_if_gate() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: loop-skipped
steps:
  - action: loop
    if: "false"
    items: ["a"]
    as: item
    steps:
      - action: emit
        event: never
  - action: emit
    event: after_loop
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        assert!(!result.events.iter().any(|(n, _)| n == "never"));
        assert!(result.events.iter().any(|(n, _)| n == "after_loop"));
    }

    #[tokio::test]
    async fn test_loop_empty_items_runs_nothing() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: loop-empty
steps:
  - action: loop
    items: []
    as: item
    steps:
      - action: emit
        event: never
  - action: emit
    event: after_loop
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        assert!(!result.events.iter().any(|(n, _)| n == "never"));
        assert!(result.events.iter().any(|(n, _)| n == "after_loop"));
    }

    #[tokio::test]
    async fn test_nested_loops() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: nested-loops
steps:
  - action: loop
    items: ["a", "b"]
    as: outer
    steps:
      - action: loop
        items: ["1", "2"]
        as: inner
        steps:
          - action: emit
            event: combo
            data:
              o: "{{vars.outer}}"
              i: "{{vars.inner}}"
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        let combos: Vec<(&Value, &Value)> = result
            .events
            .iter()
            .filter(|(n, _)| n == "combo")
            .map(|(_, d)| (&d["data"]["o"], &d["data"]["i"]))
            .collect();
        assert_eq!(combos.len(), 4);
        assert_eq!(
            combos,
            vec![
                (&json!("a"), &json!("1")),
                (&json!("a"), &json!("2")),
                (&json!("b"), &json!("1")),
                (&json!("b"), &json!("2")),
            ]
        );
    }

    // --- Condition branch block tests ---

    #[tokio::test]
    async fn test_condition_branch_supports_loop() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: cond-loop
steps:
  - action: condition
    if: "true"
    then:
      - action: loop
        items: ["p", "q"]
        as: item
        steps:
          - action: emit
            event: in_loop
            data:
              v: "{{vars.item}}"
    else:
      - action: emit
        event: wrong_branch
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        let seen: Vec<&Value> = result
            .events
            .iter()
            .filter(|(n, _)| n == "in_loop")
            .map(|(_, d)| &d["data"]["v"])
            .collect();
        assert_eq!(seen, vec![&json!("p"), &json!("q")]);
        assert!(!result.events.iter().any(|(n, _)| n == "wrong_branch"));
    }

    // --- Step-level handler tests ---

    #[tokio::test]
    async fn test_on_success_emit() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: on-success-emit
steps:
  - id: a
    action: emit
    event: did_a
    on_success:
      emit: after_a
      data:
        from: "handler"
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        assert!(result.events.iter().any(|(n, _)| n == "did_a"));
        let handler_event = result
            .events
            .iter()
            .find(|(n, _)| n == "after_a")
            .expect("on_success emit event");
        assert_eq!(handler_event.1["handler"], json!("on_success"));
        assert_eq!(handler_event.1["from"], json!("handler"));
    }

    #[tokio::test]
    async fn test_on_success_goto_skips_intermediate_steps() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: on-success-goto
steps:
  - id: a
    action: emit
    event: ev_a
    on_success:
      goto: c
  - id: b
    action: emit
    event: ev_b
  - id: c
    action: emit
    event: ev_c
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        let names: Vec<&str> = result.events.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["ev_a", "ev_c"]);
    }

    // Action that always fails, for on_failure tests
    struct MockFailAction;

    #[async_trait::async_trait]
    impl crate::actions::Action for MockFailAction {
        fn name(&self) -> &'static str {
            "mock.fail"
        }

        async fn execute(
            &self,
            _params: &HashMap<String, serde_yaml::Value>,
            _ctx: &crate::actions::ActionContext,
            _browser: &dyn BrowserHandle,
        ) -> Result<ActionOutput, ActionError> {
            Err(ActionError::Internal("intentional test failure".into()))
        }
    }

    fn workflow_with_fail_action(yaml: &str) -> Workflow {
        // Bypass WorkflowParser::validate: mock.fail is only registered on the
        // engine instance, so the shared registry check would reject it.
        serde_yaml::from_str(yaml).expect("valid workflow yaml")
    }

    #[tokio::test]
    async fn test_on_failure_emit_and_still_fails() {
        let mut engine = WorkflowEngine::new();
        engine.registry.register(Arc::new(MockFailAction));
        let browser = MockBrowser;

        let wf = workflow_with_fail_action(
            r#"
name: on-failure-emit
steps:
  - action: mock.fail
    on_failure:
      emit: step_failed
      data:
        why: "boom"
"#,
        );

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(!result.success, "workflow should still fail");
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("intentional test failure"));
        let failed_event = result
            .events
            .iter()
            .find(|(n, _)| n == "step_failed")
            .expect("on_failure emit event");
        assert_eq!(failed_event.1["handler"], json!("on_failure"));
        assert_eq!(failed_event.1["why"], json!("boom"));
        assert!(
            failed_event.1["error"]
                .as_str()
                .unwrap()
                .contains("intentional test failure"),
            "handler payload should carry the error: {:?}",
            failed_event.1
        );
    }

    #[tokio::test]
    async fn test_on_failure_goto_recovers_workflow() {
        let mut engine = WorkflowEngine::new();
        engine.registry.register(Arc::new(MockFailAction));
        let browser = MockBrowser;

        let wf = workflow_with_fail_action(
            r#"
name: on-failure-recover
steps:
  - id: f
    action: mock.fail
    on_failure:
      goto: recover
  - id: skipme
    action: emit
    event: skipped
  - id: recover
    action: emit
    event: recovered
"#,
        );

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(
            result.success,
            "goto recovery should succeed: {:?}",
            result.error
        );
        let names: Vec<&str> = result.events.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["recovered"]);
    }

    #[tokio::test]
    async fn test_on_failure_abort_fails_workflow() {
        let mut engine = WorkflowEngine::new();
        engine.registry.register(Arc::new(MockFailAction));
        let browser = MockBrowser;

        let wf = workflow_with_fail_action(
            r#"
name: on-failure-abort
steps:
  - action: mock.fail
    on_failure:
      abort: true
      error: "handler says stop"
"#,
        );

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("handler says stop"));
    }

    #[tokio::test]
    async fn test_on_failure_missing_goto_target_fails() {
        let mut engine = WorkflowEngine::new();
        engine.registry.register(Arc::new(MockFailAction));
        let browser = MockBrowser;

        let wf = workflow_with_fail_action(
            r#"
name: on-failure-bad-goto
steps:
  - action: mock.fail
    on_failure:
      goto: nowhere
"#,
        );

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(!result.success);
        assert!(result
            .error
            .as_ref()
            .unwrap()
            .contains("no step with id 'nowhere'"));
    }

    #[tokio::test]
    async fn test_on_success_abort_stops_workflow() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: on-success-abort
steps:
  - action: emit
    event: first
    on_success:
      abort: true
      error: "not allowed"
  - action: emit
    event: never
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("not allowed"));
        assert!(!result.events.iter().any(|(n, _)| n == "never"));
    }

    #[tokio::test]
    async fn test_on_success_steps_handler_runs_nested_steps() {
        let engine = WorkflowEngine::new();
        let browser = MockBrowser;

        let wf = WorkflowParser::parse(
            r#"
name: on-success-steps
steps:
  - action: emit
    event: base
    on_success:
      steps:
        - action: emit
          event: nested_one
        - action: emit
          event: nested_two
  - action: emit
    event: after
"#,
        )
        .unwrap();

        let result = engine.execute(&wf, &browser, HashMap::new()).await.unwrap();
        assert!(result.success, "{:?}", result.error);
        let names: Vec<&str> = result.events.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["base", "nested_one", "nested_two", "after"]);
    }
}
