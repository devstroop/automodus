//! API Request and Response Schemas
//!
//! Data types for the REST API endpoints.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::ToSchema;

// ============================================================================
// Health
// ============================================================================

/// Health check response
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Server status
    pub status: String,
    /// Server version
    pub version: String,
    /// Number of workflows loaded
    pub workflows_loaded: usize,
    /// Whether browser is running
    pub browser_running: bool,
}

// ============================================================================
// Workflows
// ============================================================================

/// Workflow information
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkflowInfo {
    /// Workflow name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Number of steps
    pub steps: usize,
    /// Parameter names
    pub params: Vec<String>,
}

/// Response for listing workflows
#[derive(Debug, Serialize, ToSchema)]
pub struct WorkflowListResponse {
    /// List of available workflows
    pub workflows: Vec<WorkflowInfo>,
}

/// Response for workflow reload
#[derive(Debug, Serialize, ToSchema)]
pub struct ReloadResponse {
    /// Whether reload succeeded
    pub success: bool,
    /// Number of workflows loaded
    pub count: usize,
    /// Status message
    pub message: String,
}

/// Request to run a workflow
#[derive(Debug, Deserialize, ToSchema)]
pub struct RunWorkflowRequest {
    /// Parameters to pass to the workflow
    #[serde(default)]
    pub params: HashMap<String, serde_json::Value>,
}

/// Response from workflow execution
#[derive(Debug, Serialize, ToSchema)]
pub struct RunWorkflowResponse {
    /// Whether execution succeeded
    pub success: bool,
    /// Name of the workflow
    pub workflow_name: String,
    /// Execution duration in milliseconds
    pub duration_ms: i64,
    /// Number of steps executed
    pub steps_executed: usize,
    /// Output data
    pub output: serde_json::Value,
    /// Error message if failed
    pub error: Option<String>,
}

// ============================================================================
// Browser
// ============================================================================

/// Request to navigate browser
#[derive(Debug, Deserialize, ToSchema)]
pub struct GotoRequest {
    /// URL to navigate to
    pub url: String,
}

/// Response from navigation
#[derive(Debug, Serialize, ToSchema)]
pub struct GotoResponse {
    /// Whether navigation succeeded
    pub success: bool,
    /// Current URL after navigation
    pub url: String,
    /// Error message if failed
    pub error: Option<String>,
}

// ============================================================================
// Error Response
// ============================================================================

/// Generic error response
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            error: message.into(),
        }
    }
}

// ============================================================================
// Sessions
// ============================================================================

/// Request to create a new session
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateSessionRequest {
    /// Optional session name
    #[serde(default)]
    pub name: Option<String>,
    /// Keep browser alive between workflows
    #[serde(default)]
    pub keep_alive: bool,
}

/// Session information
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionInfo {
    /// Session ID
    pub id: String,
    /// Optional session name
    pub name: Option<String>,
    /// Creation timestamp (ISO 8601)
    pub created_at: String,
    /// Last activity timestamp (ISO 8601)
    pub last_activity: String,
    /// Keep browser alive between workflows
    pub keep_alive: bool,
}

/// Response for listing sessions
#[derive(Debug, Serialize, ToSchema)]
pub struct SessionListResponse {
    /// List of active sessions
    pub sessions: Vec<SessionInfo>,
}

/// Response for creating a session
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateSessionResponse {
    /// Whether creation succeeded
    pub success: bool,
    /// Session ID
    pub id: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Response for deleting a session
#[derive(Debug, Serialize, ToSchema)]
pub struct DeleteSessionResponse {
    /// Whether deletion succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

// ============================================================================
// Browser Control
// ============================================================================

/// Request to click an element
#[derive(Debug, Deserialize, ToSchema)]
pub struct ClickRequest {
    /// CSS selector for the element
    pub selector: String,
}

/// Request to type into an element
#[derive(Debug, Deserialize, ToSchema)]
pub struct TypeRequest {
    /// CSS selector for the element
    pub selector: String,
    /// Text to type
    pub text: String,
}

/// Request to wait for an element
#[derive(Debug, Deserialize, ToSchema)]
pub struct WaitRequest {
    /// CSS selector for the element
    pub selector: String,
    /// Timeout in milliseconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,
}

fn default_timeout() -> u64 {
    30000
}

/// Request to evaluate JavaScript
#[derive(Debug, Deserialize, ToSchema)]
pub struct EvalRequest {
    /// JavaScript code to execute
    pub script: String,
}

/// Response from browser actions
#[derive(Debug, Serialize, ToSchema)]
pub struct BrowserActionResponse {
    /// Whether action succeeded
    pub success: bool,
    /// Optional result data
    pub result: Option<serde_json::Value>,
    /// Error message if failed
    pub error: Option<String>,
}

/// Page information
#[derive(Debug, Serialize, ToSchema)]
pub struct PageInfoResponse {
    /// Current page URL
    pub url: String,
    /// Page title
    pub title: String,
}
