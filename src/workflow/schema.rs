//! Workflow Schema Types
//!
//! Defines the structure for YAML-based automation workflows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Debug Configuration Types
// =============================================================================

/// Debug configuration for workflow execution.
///
/// Uses `Option<T>` for fields to distinguish "not set" from "set to default".
/// This enables proper merge semantics where only explicitly-set fields override.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugConfig {
    /// Master switch for debug mode (None = inherit from parent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Log verbosity level (None = inherit from parent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LogLevel>,

    /// Screenshot capture mode (None = inherit from parent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<CaptureMode>,

    /// Flash element with red border before interaction (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<bool>,

    /// Milliseconds to pause between actions (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<u64>,

    /// Wait for user input before continuing (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause: Option<bool>,

    /// Capture browser console output (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console: Option<bool>,

    /// Capture network requests (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<bool>,

    /// Preset configuration profile
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<DebugProfile>,
}

impl DebugConfig {
    /// Create a new DebugConfig with debug enabled
    pub fn enabled() -> Self {
        Self {
            enabled: Some(true),
            ..Default::default()
        }
    }

    /// Resolve final values (apply defaults for None fields)
    pub fn resolve(&self) -> ResolvedDebugConfig {
        ResolvedDebugConfig {
            enabled: self.enabled.unwrap_or(false),
            level: self.level.unwrap_or_default(),
            capture: self.capture.unwrap_or_default(),
            highlight: self.highlight.unwrap_or(false),
            delay: self.delay.unwrap_or(0),
            pause: self.pause.unwrap_or(false),
            console: self.console.unwrap_or(false),
            network: self.network.unwrap_or(false),
        }
    }

    /// Apply profile defaults, then merge explicit options
    pub fn with_profile(self) -> Self {
        if let Some(profile) = self.profile {
            let defaults = profile.to_config();
            // Profile sets defaults, explicit fields override
            defaults.merge(&self)
        } else {
            self
        }
    }

    /// Merge with another config (other's Some values take precedence)
    pub fn merge(&self, other: &DebugConfig) -> Self {
        Self {
            enabled: other.enabled.or(self.enabled),
            level: other.level.or(self.level),
            capture: other.capture.or(self.capture),
            highlight: other.highlight.or(self.highlight),
            delay: other.delay.or(self.delay),
            pause: other.pause.or(self.pause),
            console: other.console.or(self.console),
            network: other.network.or(self.network),
            profile: other.profile.or(self.profile),
        }
    }
}

/// Resolved debug config with concrete values (no Options).
/// Used during execution after all merging is complete.
#[derive(Debug, Clone)]
pub struct ResolvedDebugConfig {
    pub enabled: bool,
    pub level: LogLevel,
    pub capture: CaptureMode,
    pub highlight: bool,
    pub delay: u64,
    pub pause: bool,
    pub console: bool,
    pub network: bool,
}

impl Default for ResolvedDebugConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            level: LogLevel::Info,
            capture: CaptureMode::Failure,
            highlight: false,
            delay: 0,
            pause: false,
            console: false,
            network: false,
        }
    }
}

/// Log verbosity level for debug output
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Step start/complete, errors
    #[default]
    Info,
    /// + Action parameters, timing, variable state
    Debug,
    /// + Selector resolution, element details, injected JS
    Trace,
}

/// Screenshot capture mode
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    /// No screenshots captured
    None,
    /// Capture only when a step fails (default)
    #[default]
    Failure,
    /// Capture before each action
    Before,
    /// Capture after each action
    After,
    /// Capture before and after each action
    All,
}

/// Debug profile presets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebugProfile {
    /// `capture: failure` only
    Minimal,
    /// `level: trace`, `capture: all`, `console: true`, `network: true`
    Verbose,
    /// `capture: failure`, `console: true`, `network: true`
    Ci,
    /// `highlight: true`, `delay: 1000`
    Demo,
}

impl DebugProfile {
    /// Convert profile to its default DebugConfig
    pub fn to_config(self) -> DebugConfig {
        match self {
            DebugProfile::Minimal => DebugConfig {
                enabled: Some(true),
                capture: Some(CaptureMode::Failure),
                ..Default::default()
            },
            DebugProfile::Verbose => DebugConfig {
                enabled: Some(true),
                level: Some(LogLevel::Trace),
                capture: Some(CaptureMode::All),
                console: Some(true),
                network: Some(true),
                ..Default::default()
            },
            DebugProfile::Ci => DebugConfig {
                enabled: Some(true),
                capture: Some(CaptureMode::Failure),
                console: Some(true),
                network: Some(true),
                ..Default::default()
            },
            DebugProfile::Demo => DebugConfig {
                enabled: Some(true),
                highlight: Some(true),
                delay: Some(1000),
                ..Default::default()
            },
        }
    }
}

// =============================================================================
// Workflow Types
// =============================================================================

/// A complete workflow definition parsed from YAML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Workflow name (unique identifier)
    pub name: String,

    /// Workflow version
    #[serde(default = "default_version")]
    pub version: String,

    /// Optional description
    #[serde(default)]
    pub description: Option<String>,

    /// Browser configuration
    #[serde(default)]
    pub browser: BrowserConfig,

    /// Triggers that start this workflow
    #[serde(default, rename = "on")]
    pub triggers: Triggers,

    /// Input parameters schema
    #[serde(default)]
    pub params: HashMap<String, ParamDef>,

    /// Workflow-level variables
    #[serde(default)]
    pub vars: Option<HashMap<String, serde_yaml::Value>>,

    /// Automation steps
    pub steps: Vec<Step>,

    /// Output definition (map of name -> value template)
    #[serde(default)]
    pub output: Option<HashMap<String, serde_yaml::Value>>,

    /// Actions on successful completion
    #[serde(default)]
    pub on_complete: Option<CompleteHandler>,

    /// Actions on error
    #[serde(default)]
    pub on_error: Option<ErrorHandler>,

    /// Debug configuration for this workflow
    #[serde(default)]
    pub debug: DebugConfig,
}

fn default_version() -> String {
    "1.0".to_string()
}

/// Browser instance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    /// Number of parallel browser instances
    #[serde(default = "default_instances")]
    pub instances: u32,

    /// Run in headless mode
    #[serde(default = "default_headless")]
    pub headless: bool,

    /// User data directory (supports templates)
    #[serde(default)]
    pub data_dir: Option<String>,

    /// Window width
    #[serde(default = "default_width")]
    pub width: u32,

    /// Window height
    #[serde(default = "default_height")]
    pub height: u32,

    /// Additional Chrome flags
    #[serde(default)]
    pub args: Vec<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            instances: 1,
            headless: true,
            data_dir: None,
            width: 1280,
            height: 720,
            args: Vec::new(),
        }
    }
}

fn default_instances() -> u32 {
    1
}
fn default_headless() -> bool {
    true
}
fn default_width() -> u32 {
    1280
}
fn default_height() -> u32 {
    720
}

/// Flow triggers configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Triggers {
    /// REST API trigger
    #[serde(default)]
    pub api: Option<ApiTrigger>,

    /// Cron schedule trigger
    #[serde(default)]
    pub schedule: Option<String>,

    /// Event-based trigger
    #[serde(default)]
    pub event: Option<String>,

    /// Webhook trigger
    #[serde(default)]
    pub webhook: Option<WebhookTrigger>,

    /// File watcher trigger
    #[serde(default)]
    pub watch: Option<WatchTrigger>,

    /// Manual-only (no auto trigger)
    #[serde(default)]
    pub manual: bool,
}

/// API endpoint trigger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTrigger {
    /// URL path
    pub path: String,

    /// HTTP method
    #[serde(default = "default_method")]
    pub method: String,

    /// Authentication type
    #[serde(default)]
    pub auth: Option<String>,
}

fn default_method() -> String {
    "POST".to_string()
}

/// Webhook trigger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookTrigger {
    /// URL path
    pub path: String,

    /// Secret for signature verification
    #[serde(default)]
    pub secret: Option<String>,
}

/// File watcher trigger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchTrigger {
    /// Path pattern to watch
    pub path: String,

    /// Events to watch for
    #[serde(default)]
    pub events: Vec<String>,
}

/// Parameter definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParamDef {
    /// Parameter type: string, number, boolean, object, array
    #[serde(rename = "type", default = "default_param_type")]
    pub param_type: String,

    /// Whether the parameter is required
    #[serde(default)]
    pub required: bool,

    /// Default value
    #[serde(default)]
    pub default: Option<serde_yaml::Value>,

    /// Description
    #[serde(default)]
    pub description: Option<String>,
}

fn default_param_type() -> String {
    "string".to_string()
}

/// A single automation step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    /// Step identifier (optional, auto-generated if not provided)
    #[serde(default)]
    pub id: Option<String>,

    /// Action to perform
    pub action: String,

    /// Action-specific parameters (flattened)
    #[serde(flatten)]
    pub params: HashMap<String, serde_yaml::Value>,

    /// Retry configuration
    #[serde(default)]
    pub retry: Option<RetryConfig>,

    /// Conditional execution
    #[serde(rename = "if", default)]
    pub condition: Option<String>,

    /// Event to emit after step completes
    #[serde(default)]
    pub emit: Option<EventEmit>,

    /// Success handler
    #[serde(default)]
    pub on_success: Option<StepHandler>,

    /// Failure handler
    #[serde(default)]
    pub on_failure: Option<StepHandler>,

    /// Step-level debug overrides
    #[serde(default)]
    pub debug: Option<DebugConfig>,
}

/// Retry configuration for a step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum retry attempts
    #[serde(default)]
    pub max: Option<u32>,

    /// Delay between retries in milliseconds
    #[serde(default)]
    pub delay_ms: Option<u64>,
}

/// Handler for step success/failure
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepHandler {
    /// Simple goto another step
    Goto { goto: String },

    /// Emit an event
    Emit {
        emit: String,
        #[serde(default)]
        data: HashMap<String, serde_yaml::Value>,
    },

    /// Abort workflow
    Abort {
        abort: bool,
        #[serde(default)]
        error: Option<String>,
    },

    /// Execute nested steps (map form: `steps: [...]`)
    Steps { steps: Vec<Step> },

    /// Execute nested steps (list form: a bare list of steps)
    StepsList(Vec<Step>),
}

/// Output definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputDef {
    /// Output name
    pub name: String,

    /// Value expression (template)
    pub value: String,

    /// Output type for serialization
    #[serde(rename = "type", default)]
    pub output_type: Option<String>,
}

/// Event emission configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEmit {
    /// Event name to emit
    pub event: String,

    /// Event data
    #[serde(default)]
    pub data: Option<HashMap<String, serde_yaml::Value>>,
}

/// Error handler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorHandler {
    /// Event to emit on error
    #[serde(default)]
    pub emit: Option<EventEmit>,

    /// Take screenshot on error
    #[serde(default)]
    pub screenshot: Option<bool>,

    /// Steps to execute on error
    #[serde(default)]
    pub steps: Option<Vec<Step>>,
}

/// Completion handler configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompleteHandler {
    /// Event to emit on completion
    #[serde(default)]
    pub emit: Option<EventEmit>,

    /// Steps to execute on completion
    #[serde(default)]
    pub steps: Option<Vec<Step>>,
}

// =============================================================================
// Built-in Action Parameter Types
// =============================================================================

/// Parameters for navigate/goto action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GotoParams {
    pub url: String,
    #[serde(default)]
    pub wait_until: Option<String>,
}

/// Parameters for click action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClickParams {
    pub selector: String,
    #[serde(default)]
    pub button: Option<String>,
    #[serde(default)]
    pub wait_after: Option<String>,
}

/// Parameters for type action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeParams {
    pub selector: String,
    pub text: String,
    #[serde(default)]
    pub clear: bool,
    #[serde(default)]
    pub delay: Option<String>,
}

/// Parameters for wait_for action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitForParams {
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    /// State to wait for: "visible" (default), "hidden", "present", "absent"
    #[serde(default = "default_state")]
    pub state: String,
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout: String,
}

fn default_state() -> String {
    "visible".to_string()
}
fn default_timeout() -> String {
    "30s".to_string()
}

/// Parameters for screenshot action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotParams {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub full_page: bool,
    #[serde(default)]
    pub store_as: Option<String>,
}

/// Parameters for extract action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractParams {
    pub selector: String,
    #[serde(default = "default_attribute")]
    pub attribute: String,
    pub store_as: String,
}

fn default_attribute() -> String {
    "text".to_string()
}

/// Parameters for eval action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalParams {
    pub script: String,
    #[serde(default)]
    pub store_as: Option<String>,
}

/// Parameters for sleep action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepParams {
    pub duration: String,
}

/// Parameters for emit action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmitParams {
    pub event: String,
    #[serde(default)]
    pub data: HashMap<String, serde_yaml::Value>,
}

/// Parameters for condition action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionParams {
    #[serde(rename = "if")]
    pub condition: String,
    #[serde(rename = "then")]
    pub then_steps: Vec<Step>,
    #[serde(rename = "else", default)]
    pub else_steps: Vec<Step>,
}

/// Parameters for loop action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopParams {
    pub items: serde_yaml::Value,
    #[serde(rename = "as")]
    pub item_var: String,
    #[serde(default)]
    pub index_as: Option<String>,
    #[serde(default)]
    pub parallel: bool,
    pub steps: Vec<Step>,
}

/// Parameters for tab operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabNewParams {
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabSwitchParams {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub title: Option<String>,
}

/// Parameters for http action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpParams {
    pub url: String,
    #[serde(default = "default_http_method")]
    pub method: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: Option<serde_yaml::Value>,
    #[serde(default)]
    pub store_as: Option<String>,
}

fn default_http_method() -> String {
    "GET".to_string()
}

/// Parameters for call action (invoke another workflow)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallParams {
    pub workflow: String,
    #[serde(default)]
    pub params: HashMap<String, serde_yaml::Value>,
    #[serde(default = "default_await")]
    pub await_result: bool,
}

fn default_await() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_workflow() {
        let yaml = r##"
name: test-workflow
steps:
  - action: goto
    url: "https://example.com"
  - action: click
    selector: "#button"
"##;
        let workflow: Workflow = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(workflow.name, "test-workflow");
        assert_eq!(workflow.steps.len(), 2);
    }

    #[test]
    fn test_parse_workflow_with_triggers() {
        let yaml = r##"
name: api-triggered
on:
  api:
    path: /trigger/test
    method: POST
  schedule: "0 * * * *"
steps:
  - action: goto
    url: "https://example.com"
"##;
        let workflow: Workflow = serde_yaml::from_str(yaml).unwrap();
        assert!(workflow.triggers.api.is_some());
        assert!(workflow.triggers.schedule.is_some());
    }

    #[test]
    fn test_parse_workflow_with_params() {
        let yaml = r##"
name: parameterized
params:
  url:
    type: string
    required: true
  count:
    type: number
    default: 1
steps:
  - action: goto
    url: "{{params.url}}"
"##;
        let workflow: Workflow = serde_yaml::from_str(yaml).unwrap();
        assert!(workflow.params.contains_key("url"));
        assert!(workflow.params.get("url").unwrap().required);
    }

    // =========================================================================
    // Debug Config Tests
    // =========================================================================

    #[test]
    fn test_debug_config_default() {
        let cfg = DebugConfig::default();
        assert!(cfg.enabled.is_none());
        assert!(cfg.level.is_none());
        assert!(cfg.capture.is_none());

        let resolved = cfg.resolve();
        assert!(!resolved.enabled);
        assert_eq!(resolved.level, LogLevel::Info);
        assert_eq!(resolved.capture, CaptureMode::Failure);
    }

    #[test]
    fn test_debug_config_merge() {
        let base = DebugConfig {
            enabled: Some(true),
            level: Some(LogLevel::Debug),
            capture: Some(CaptureMode::All),
            ..Default::default()
        };

        let override_cfg = DebugConfig {
            level: Some(LogLevel::Trace),
            delay: Some(500),
            ..Default::default()
        };

        let merged = base.merge(&override_cfg);
        assert_eq!(merged.enabled, Some(true)); // from base
        assert_eq!(merged.level, Some(LogLevel::Trace)); // overridden
        assert_eq!(merged.capture, Some(CaptureMode::All)); // from base
        assert_eq!(merged.delay, Some(500)); // from override
    }

    #[test]
    fn test_debug_config_with_profile() {
        // Profile sets defaults
        let cfg = DebugConfig {
            profile: Some(DebugProfile::Ci),
            ..Default::default()
        };
        let resolved = cfg.with_profile().resolve();
        assert!(resolved.enabled);
        assert_eq!(resolved.capture, CaptureMode::Failure);
        assert!(resolved.console);
        assert!(resolved.network);
    }

    #[test]
    fn test_debug_config_profile_override() {
        // Explicit capture overrides profile default
        let cfg = DebugConfig {
            profile: Some(DebugProfile::Ci),
            capture: Some(CaptureMode::All),
            ..Default::default()
        };
        let resolved = cfg.with_profile().resolve();
        assert_eq!(resolved.capture, CaptureMode::All); // explicit wins
        assert!(resolved.console); // from profile
    }

    #[test]
    fn test_debug_profile_verbose() {
        let cfg = DebugProfile::Verbose.to_config();
        let resolved = cfg.resolve();
        assert!(resolved.enabled);
        assert_eq!(resolved.level, LogLevel::Trace);
        assert_eq!(resolved.capture, CaptureMode::All);
        assert!(resolved.console);
        assert!(resolved.network);
    }

    #[test]
    fn test_debug_profile_demo() {
        let cfg = DebugProfile::Demo.to_config();
        let resolved = cfg.resolve();
        assert!(resolved.enabled);
        assert!(resolved.highlight);
        assert_eq!(resolved.delay, 1000);
    }

    #[test]
    fn test_parse_workflow_with_debug() {
        let yaml = r##"
name: debug-test
debug:
  enabled: true
  level: trace
  capture: all
  highlight: true
steps:
  - action: click
    selector: "#btn"
"##;
        let workflow: Workflow = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(workflow.debug.enabled, Some(true));
        assert_eq!(workflow.debug.level, Some(LogLevel::Trace));
        assert_eq!(workflow.debug.capture, Some(CaptureMode::All));
        assert_eq!(workflow.debug.highlight, Some(true));
    }

    #[test]
    fn test_parse_workflow_with_debug_profile() {
        let yaml = r##"
name: profile-test
debug:
  profile: verbose
steps:
  - action: click
    selector: "#btn"
"##;
        let workflow: Workflow = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(workflow.debug.profile, Some(DebugProfile::Verbose));

        let resolved = workflow.debug.with_profile().resolve();
        assert_eq!(resolved.level, LogLevel::Trace);
        assert_eq!(resolved.capture, CaptureMode::All);
    }

    #[test]
    fn test_parse_step_with_debug() {
        let yaml = r##"
name: step-debug-test
steps:
  - action: click
    selector: "#btn"
    debug:
      pause: true
      capture: before
"##;
        let workflow: Workflow = serde_yaml::from_str(yaml).unwrap();
        let step_debug = workflow.steps[0].debug.as_ref().unwrap();
        assert_eq!(step_debug.pause, Some(true));
        assert_eq!(step_debug.capture, Some(CaptureMode::Before));
    }

    #[test]
    fn test_workflow_backward_compatibility() {
        // Existing workflows without debug section should still parse
        let yaml = r##"
name: old-workflow
steps:
  - action: goto
    url: "https://example.com"
"##;
        let workflow: Workflow = serde_yaml::from_str(yaml).unwrap();
        assert!(workflow.debug.enabled.is_none()); // Default
        assert!(workflow.steps[0].debug.is_none()); // Not set
    }
}
