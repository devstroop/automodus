//! Action Registry
//!
//! Defines the Action trait and manages available actions.

use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

/// Error type for action execution
#[derive(Debug, Error)]
pub enum ActionError {
    #[error("Missing required parameter: {0}")]
    MissingParameter(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Browser error: {0}")]
    BrowserError(String),

    #[error("Element not found: {0}")]
    ElementNotFound(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Condition failed: {0}")]
    ConditionFailed(String),

    #[error("Workflow error: {0}")]
    WorkflowError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result of an action execution
#[derive(Debug, Clone, Default)]
pub struct ActionOutput {
    /// Output data (varies by action)
    pub data: Option<Value>,

    /// Variables to store in context
    pub store: HashMap<String, Value>,

    /// Event to emit (if any)
    pub emit: Option<(String, Value)>,

    /// Whether to skip remaining steps
    pub skip: bool,

    /// Step ID to jump to
    pub goto: Option<String>,
}

impl ActionOutput {
    /// Create empty output
    pub fn empty() -> Self {
        Self::default()
    }

    /// Create output with data
    pub fn with_data(data: Value) -> Self {
        Self {
            data: Some(data),
            ..Default::default()
        }
    }

    /// Create output that stores a variable
    pub fn store(key: impl Into<String>, value: Value) -> Self {
        let mut store = HashMap::new();
        store.insert(key.into(), value);
        Self {
            store,
            ..Default::default()
        }
    }

    /// Create output that emits an event
    pub fn emit(event: impl Into<String>, data: Value) -> Self {
        Self {
            emit: Some((event.into(), data)),
            ..Default::default()
        }
    }

    /// Create output that jumps to another step
    pub fn goto(step_id: impl Into<String>) -> Self {
        Self {
            goto: Some(step_id.into()),
            ..Default::default()
        }
    }
}

/// Execution context passed to actions
#[derive(Debug, Clone)]
pub struct ActionContext {
    /// Workflow name
    pub workflow_name: String,

    /// Current step index
    pub step_index: usize,

    /// Step ID (if provided)
    pub step_id: Option<String>,

    /// Workflow variables
    pub vars: HashMap<String, Value>,

    /// Trigger parameters
    pub params: HashMap<String, Value>,

    /// Stored values from previous steps
    pub store: HashMap<String, Value>,

    /// Browser instance ID
    pub instance_id: String,

    /// Current tab index
    pub tab_index: usize,
}

impl ActionContext {
    /// Get a variable value
    pub fn get_var(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    /// Get a parameter value
    pub fn get_param(&self, name: &str) -> Option<&Value> {
        self.params.get(name)
    }

    /// Get a stored value
    pub fn get_stored(&self, name: &str) -> Option<&Value> {
        self.store.get(name)
    }

    /// Resolve a value reference ({{vars.x}}, {{params.y}}, {{store.z}})
    pub fn resolve(&self, key: &str) -> Option<&Value> {
        if let Some(name) = key.strip_prefix("vars.") {
            self.get_var(name)
        } else if let Some(name) = key.strip_prefix("params.") {
            self.get_param(name)
        } else if let Some(name) = key
            .strip_prefix("steps.")
            .or_else(|| key.strip_prefix("store."))
        {
            self.get_stored(name)
        } else {
            self.get_stored(key)
        }
    }
}

/// Browser handle passed to actions
#[async_trait]
pub trait BrowserHandle: Send + Sync {
    /// Navigate to URL
    async fn goto(&self, url: &str) -> Result<(), ActionError>;

    /// Click element
    async fn click(&self, selector: &str) -> Result<(), ActionError>;

    /// Type text into element
    async fn type_text(&self, selector: &str, text: &str, clear: bool) -> Result<(), ActionError>;

    /// Get element text
    async fn get_text(&self, selector: &str) -> Result<String, ActionError>;

    /// Get element attribute
    async fn get_attribute(
        &self,
        selector: &str,
        attr: &str,
    ) -> Result<Option<String>, ActionError>;

    /// Wait for element to appear
    async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError>;

    /// Wait for element to disappear/be hidden
    async fn wait_for_hidden(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError>;

    /// Wait for URL condition
    async fn wait_for_url(&self, condition: &str, timeout_ms: u64) -> Result<(), ActionError>;

    /// Take screenshot
    async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>, ActionError>;

    /// Execute JavaScript
    async fn eval(&self, script: &str) -> Result<Value, ActionError>;

    /// Get current URL
    async fn current_url(&self) -> Result<String, ActionError>;

    /// Go back
    async fn back(&self) -> Result<(), ActionError>;

    /// Go forward
    async fn forward(&self) -> Result<(), ActionError>;

    /// Reload page
    async fn reload(&self) -> Result<(), ActionError>;

    /// Open new tab
    async fn new_tab(&self, url: Option<&str>) -> Result<usize, ActionError>;

    /// Switch to tab
    async fn switch_tab(&self, index: usize) -> Result<(), ActionError>;

    /// Close tab
    async fn close_tab(&self, index: usize) -> Result<(), ActionError>;

    /// Get tab count
    async fn tab_count(&self) -> Result<usize, ActionError>;

    /// Set file(s) on a file input element via CDP
    /// This is the only way to programmatically set files on file inputs
    async fn set_file_input_files(
        &self,
        selector: &str,
        file_paths: Vec<String>,
    ) -> Result<(), ActionError>;

    /// Enable/disable file chooser interception to prevent native dialog
    async fn set_file_chooser_intercept(&self, enabled: bool) -> Result<(), ActionError>;

    /// Upload files via file chooser event handling
    /// Subscribes to fileChooserOpened event, optionally clicks a trigger element,
    /// then waits for the event and sets files using backend_node_id
    /// * trigger_selector: Optional selector to click that triggers file input
    /// * file_paths: Files to upload
    /// * timeout_ms: Timeout in milliseconds for waiting for event
    async fn upload_via_file_chooser(
        &self,
        trigger_selector: Option<&str>,
        file_paths: Vec<String>,
        timeout_ms: u64,
    ) -> Result<(), ActionError>;
}

/// Action trait that all actions implement
#[async_trait]
pub trait Action: Send + Sync {
    /// Action name (e.g., "click", "goto")
    fn name(&self) -> &'static str;

    /// Execute the action
    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError>;
}

/// Registry of available actions
pub struct ActionRegistry {
    actions: HashMap<String, Arc<dyn Action>>,
}

impl ActionRegistry {
    /// Create a new registry with built-in actions
    pub fn new() -> Self {
        let mut registry = Self {
            actions: HashMap::new(),
        };

        // Register built-in actions
        registry.register_builtins();

        registry
    }

    /// Register built-in actions
    fn register_builtins(&mut self) {
        use super::control;
        use crate::modules::browser::actions::{
            capture, extract, interact, navigate, tabs, upload, wait,
        };
        use crate::modules::http::actions::{
            HttpDeleteAction, HttpGetAction, HttpPatchAction, HttpPostAction, HttpPutAction,
            HttpRequestAction,
        };

        // Navigation
        self.register(Arc::new(navigate::GotoAction));
        self.register(Arc::new(navigate::BackAction));
        self.register(Arc::new(navigate::ForwardAction));
        self.register(Arc::new(navigate::ReloadAction));

        // Interaction
        self.register(Arc::new(interact::ClickAction));
        self.register(Arc::new(interact::TypeAction));
        self.register(Arc::new(interact::SelectAction));
        self.register(Arc::new(interact::HoverAction));

        // Waiting
        self.register(Arc::new(wait::WaitForAction));
        self.register(Arc::new(wait::SleepAction));

        // Extraction
        self.register(Arc::new(extract::ExtractAction));
        self.register(Arc::new(extract::EvalAction));

        // Capture
        self.register(Arc::new(capture::ScreenshotAction));

        // Tabs
        self.register(Arc::new(tabs::TabNewAction));
        self.register(Arc::new(tabs::TabSwitchAction));
        self.register(Arc::new(tabs::TabCloseAction));

        // Control flow
        self.register(Arc::new(control::EmitAction));
        self.register(Arc::new(control::LogAction));

        // File upload
        self.register(Arc::new(upload::UploadAction));
        self.register(Arc::new(upload::WaitUploadAction));
        self.register(Arc::new(upload::FileChooserAction));

        // HTTP requests
        self.register(Arc::new(HttpGetAction));
        self.register(Arc::new(HttpPostAction));
        self.register(Arc::new(HttpPutAction));
        self.register(Arc::new(HttpPatchAction));
        self.register(Arc::new(HttpDeleteAction));
        self.register(Arc::new(HttpRequestAction));
    }

    /// Register an action
    pub fn register(&mut self, action: Arc<dyn Action>) {
        self.actions.insert(action.name().to_string(), action);
    }

    /// Get an action by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Action>> {
        // Handle aliases
        let name = match name {
            "navigate" => "goto",
            "input" => "type",
            "wait" => "wait_for",
            _ => name,
        };
        self.actions.get(name).cloned()
    }

    /// List all registered action names
    pub fn list(&self) -> Vec<&str> {
        self.actions.keys().map(|s| s.as_str()).collect()
    }

    /// Check if action exists
    pub fn has(&self, name: &str) -> bool {
        self.get(name).is_some()
    }
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}
