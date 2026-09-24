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

    #[error("Unsupported by this browser engine: {0}")]
    Unsupported(String),
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

/// Information about a browser tab
#[derive(Debug, Clone)]
pub struct TabInfo {
    /// Tab index
    pub index: usize,
    /// Current URL of the tab
    pub url: String,
    /// Whether this tab is the active tab
    pub active: bool,
}

/// Console log entry captured from the browser
#[derive(Debug, Clone)]
pub struct ConsoleEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: String,
    pub message: String,
}

impl ConsoleEntry {
    /// Format as log line: [CONSOLE] 14:32:01.123 LOG: message
    pub fn format(&self) -> String {
        format!(
            "[CONSOLE] {} {}: {}",
            self.timestamp.format("%H:%M:%S%.3f"),
            self.level.to_uppercase(),
            self.message
        )
    }
}

/// Network request entry captured from the browser
#[derive(Debug, Clone)]
pub struct NetworkEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub method: String,
    pub url: String,
    pub status: Option<u32>,
    pub duration_ms: Option<u64>,
}

impl NetworkEntry {
    /// Format as log line: [NETWORK] GET https://... → 200 (45ms)
    pub fn format(&self) -> String {
        let status = self
            .status
            .map(|s| s.to_string())
            .unwrap_or_else(|| "?".to_string());
        let timing = self
            .duration_ms
            .map(|d| format!(" ({}ms)", d))
            .unwrap_or_default();
        format!(
            "[NETWORK] {} {} → {}{}",
            self.method, self.url, status, timing
        )
    }
}

/// Runtime capability flags for a browser backend.
///
/// Actions and callers use this to fail fast with
/// [`ActionError::Unsupported`] before invoking Chromium-only methods.
/// Defaults are intentionally empty (no optional features) so a new
/// backend only advertises what it actually implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BrowserCapabilities {
    /// CDP `Page.printToPDF` (and equivalents)
    pub pdf: bool,
    /// Programmatic file-input population (`DOM.setFileInputFiles`)
    pub file_input: bool,
    /// File-chooser interception (`Page.setInterceptFileChooserDialog`)
    pub file_chooser: bool,
    /// Live console event stream
    pub console_events: bool,
    /// Live network request/response event stream
    pub network_events: bool,
    /// Target crash detection listener
    pub crash_events: bool,
}

impl BrowserCapabilities {
    /// Full Chromium/CDP feature set (what `ChromePageAdapter` advertises).
    pub const CHROMIUM: Self = Self {
        pdf: true,
        file_input: true,
        file_chooser: true,
        console_events: true,
        network_events: true,
        crash_events: true,
    };

    /// No optional features (safe default for mocks and future backends).
    pub const NONE: Self = Self {
        pdf: false,
        file_input: false,
        file_chooser: false,
        console_events: false,
        network_events: false,
        crash_events: false,
    };

    /// Firefox / WebDriver BiDi feature set (what `FirefoxPageAdapter` advertises).
    ///
    /// No CDP-only features: PDF, programmatic file-input population, and
    /// file-chooser interception are unavailable. Console/network/crash event
    /// streams are not wired yet on the BiDi backend (defaults are no-ops).
    pub const FIREFOX: Self = Self {
        pdf: false,
        file_input: false,
        file_chooser: false,
        console_events: false,
        network_events: false,
        crash_events: false,
    };

    /// Lightpanda CDP feature set (what `SessionAdapter::Lightpanda` advertises).
    ///
    /// Lightpanda speaks a CDP subset: no `Page.printToPDF`, no file-chooser
    /// interception, no `DOM.setFileInputFiles`. Console/network listeners are
    /// wired through to the Chromium adapter but Lightpanda does not reliably
    /// emit those events — advertise `false` until proven otherwise.
    pub const LIGHTPANDA: Self = Self {
        pdf: false,
        file_input: false,
        file_chooser: false,
        console_events: false,
        network_events: false,
        crash_events: false,
    };
}

/// Browser handle passed to actions
///
/// This is the sole browser-agnostic contract between the workflow engine and
/// any browser backend (Chromium via CDP today; Firefox via WebDriver BiDi later).
///
/// # Capability gates
///
/// Methods documented as **Chromium-only** may return
/// `ActionError::Unsupported` on non-Chromium backends. Callers should check
/// [`BrowserHandle::capabilities`] first (or treat `Unsupported` as
/// "feature unavailable" rather than a hard failure).
#[async_trait]
pub trait BrowserHandle: Send + Sync {
    /// Runtime feature flags for this backend.
    ///
    /// Default is [`BrowserCapabilities::NONE`] — backends must opt in to
    /// Chromium-only features by overriding this.
    fn capabilities(&self) -> BrowserCapabilities {
        BrowserCapabilities::NONE
    }

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

    /// List all open tabs
    async fn list_tabs(&self) -> Result<Vec<TabInfo>, ActionError>;

    /// Print page to PDF and return the bytes
    ///
    /// **Chromium-only** (CDP `Page.printToPDF`). Non-Chromium backends
    /// should return `ActionError::Unsupported`.
    async fn pdf(&self) -> Result<Vec<u8>, ActionError>;

    /// Set file(s) on a file input element
    ///
    /// **Chromium-only** (CDP `DOM.setFileInputFiles`). Non-Chromium backends
    /// should return `ActionError::Unsupported`.
    async fn set_file_input_files(
        &self,
        selector: &str,
        file_paths: Vec<String>,
    ) -> Result<(), ActionError>;

    /// Enable/disable file chooser interception to prevent native dialog
    ///
    /// **Chromium-only** (CDP `Page.setInterceptFileChooserDialog`).
    async fn set_file_chooser_intercept(&self, enabled: bool) -> Result<(), ActionError>;

    /// Upload files via file chooser event handling
    ///
    /// **Chromium-only** — subscribes to `fileChooserOpened`, optionally clicks
    /// a trigger element, then sets files via CDP.
    /// * trigger_selector: Optional selector to click that triggers file input
    /// * file_paths: Files to upload
    /// * timeout_ms: Timeout in milliseconds for waiting for event
    async fn upload_via_file_chooser(
        &self,
        trigger_selector: Option<&str>,
        file_paths: Vec<String>,
        timeout_ms: u64,
    ) -> Result<(), ActionError>;

    // --- Observability (console / network / crash listeners) ---
    //
    // Default no-ops so backends without event streams (or test mocks) can
    // omit them. Chromium CDP overrides these with real listeners.

    /// Start capturing console logs (idempotent).
    async fn start_console_listener(&self) -> Result<(), ActionError> {
        Ok(())
    }

    /// Start capturing network request/response logs (idempotent).
    async fn start_network_listener(&self) -> Result<(), ActionError> {
        Ok(())
    }

    /// Start crash detection (idempotent).
    async fn start_crash_listener(&self) -> Result<(), ActionError> {
        Ok(())
    }

    /// Snapshot of captured console logs (does not clear).
    async fn get_console_logs(&self) -> Vec<ConsoleEntry> {
        Vec::new()
    }

    /// Snapshot of captured network logs (does not clear).
    async fn get_network_logs(&self) -> Vec<NetworkEntry> {
        Vec::new()
    }

    /// Drain captured console logs (clears the buffer).
    async fn capture_console_logs(&self) -> Result<Vec<ConsoleEntry>, ActionError> {
        Ok(Vec::new())
    }

    /// Drain captured network logs (clears the buffer).
    async fn capture_network_logs(&self) -> Result<Vec<NetworkEntry>, ActionError> {
        Ok(Vec::new())
    }

    /// Clear buffered console logs.
    async fn clear_console_logs(&self) {}

    /// Clear buffered network logs.
    async fn clear_network_logs(&self) {}

    /// Whether the underlying browser target is still alive (not crashed).
    fn is_browser_alive(&self) -> bool {
        true
    }
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
        self.register(Arc::new(tabs::TabListAction));
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

#[cfg(test)]
mod capability_tests {
    use super::*;

    #[test]
    fn default_browser_capabilities_are_none() {
        assert_eq!(BrowserCapabilities::default(), BrowserCapabilities::NONE);
        assert!(!BrowserCapabilities::NONE.pdf);
        assert!(!BrowserCapabilities::NONE.file_input);
        assert!(!BrowserCapabilities::NONE.file_chooser);
        assert!(!BrowserCapabilities::NONE.console_events);
    }

    #[test]
    fn chromium_capabilities_enable_optional_features() {
        let c = BrowserCapabilities::CHROMIUM;
        assert!(c.pdf && c.file_input && c.file_chooser);
        assert!(c.console_events && c.network_events && c.crash_events);
    }

    #[test]
    fn firefox_capabilities_match_none() {
        assert_eq!(BrowserCapabilities::FIREFOX, BrowserCapabilities::NONE);
    }

    #[test]
    fn lightpanda_capabilities_are_conservative() {
        let c = BrowserCapabilities::LIGHTPANDA;
        assert_eq!(c, BrowserCapabilities::NONE);
        assert!(!c.pdf && !c.file_input && !c.file_chooser);
        assert!(!c.console_events && !c.network_events && !c.crash_events);
        assert_ne!(c, BrowserCapabilities::CHROMIUM);
    }
}
