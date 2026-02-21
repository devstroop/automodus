//! Workflow Schema Types
//!
//! Defines the structure for YAML-based automation workflows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

    /// Execute nested steps
    Steps(Vec<Step>),
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
}
