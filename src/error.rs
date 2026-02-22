//! Error types for Automodus
//!
//! Unified error types for browser automation, daemon, and API.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

/// Result type for Automodus operations
pub type Result<T> = std::result::Result<T, AutomodusError>;

/// Comprehensive error types for the Automodus library
#[derive(Error, Debug)]
pub enum AutomodusError {
    #[error("Browser initialization failed: {0}")]
    BrowserInit(String),

    #[error("Browser navigation failed: {0}")]
    BrowserNavigation(String),

    #[error("Browser connection lost: {0}")]
    BrowserConnection(String),

    #[error("Workflow execution failed: {0}")]
    WorkflowExecution(String),

    #[error("Action failed: {0}")]
    ActionFailed(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Internal error: {0}")]
    Internal(String),

    // Daemon-specific errors
    #[error("Daemon not running")]
    DaemonNotRunning,

    #[error("Daemon already running (PID {0})")]
    DaemonAlreadyRunning(u32),

    #[error("Daemon connection failed: {0}")]
    DaemonConnectionFailed(String),

    // Session errors
    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Session limit reached")]
    SessionLimitReached,

    // Workflow errors
    #[error("Workflow not found: {0}")]
    WorkflowNotFound(String),

    #[error("Workflow invalid: {0}")]
    WorkflowInvalid(String),

    #[error("Workflow timeout after {0}ms")]
    WorkflowTimeout(u64),

    // Browser errors
    #[error("Selector not found: {0}")]
    SelectorNotFound(String),

    #[error("Selector timeout: {0}")]
    SelectorTimeout(String),
}

/// Standardized error codes for API responses
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // Daemon errors
    DaemonNotRunning,
    DaemonAlreadyRunning,
    DaemonConnectionFailed,

    // Session errors
    SessionNotFound,
    SessionLimitReached,

    // Workflow errors
    WorkflowNotFound,
    WorkflowInvalid,
    WorkflowTimeout,

    // Execution errors
    ExecutionFailed,
    ExecutionCancelled,
    StepFailed,

    // Browser errors
    BrowserLaunchFailed,
    BrowserDisconnected,
    SelectorNotFound,
    SelectorTimeout,
    NavigationFailed,

    // General errors
    InvalidRequest,
    InternalError,
}

/// Application error with code for API responses
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

impl AppError {
    /// Create a new AppError
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }

    /// Create with details
    pub fn with_details(code: ErrorCode, message: impl Into<String>, details: Value) -> Self {
        Self {
            code,
            message: message.into(),
            details: Some(details),
        }
    }

    /// Selector not found error
    pub fn selector_not_found(selector: &str, timeout_ms: u64) -> Self {
        Self::with_details(
            ErrorCode::SelectorNotFound,
            format!("Element not found: {}", selector),
            json!({ "selector": selector, "timeout_ms": timeout_ms }),
        )
    }

    /// Workflow not found error
    pub fn workflow_not_found(name: &str) -> Self {
        Self::new(
            ErrorCode::WorkflowNotFound,
            format!("Workflow '{}' not found", name),
        )
    }

    /// Session not found error
    pub fn session_not_found(id: &str) -> Self {
        Self::new(
            ErrorCode::SessionNotFound,
            format!("Session '{}' not found", id),
        )
    }

    /// Daemon not running error
    pub fn daemon_not_running() -> Self {
        Self::new(
            ErrorCode::DaemonNotRunning,
            "Daemon is not running. Start with: automodus daemon start",
        )
    }

    /// Daemon already running error
    pub fn daemon_already_running(pid: u32) -> Self {
        Self::with_details(
            ErrorCode::DaemonAlreadyRunning,
            format!("Daemon already running with PID {}", pid),
            json!({ "pid": pid }),
        )
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<AutomodusError> for AppError {
    fn from(err: AutomodusError) -> Self {
        match err {
            AutomodusError::BrowserInit(msg) => {
                AppError::new(ErrorCode::BrowserLaunchFailed, msg)
            }
            AutomodusError::BrowserNavigation(msg) => {
                AppError::new(ErrorCode::NavigationFailed, msg)
            }
            AutomodusError::BrowserConnection(msg) => {
                AppError::new(ErrorCode::BrowserDisconnected, msg)
            }
            AutomodusError::WorkflowExecution(msg) => {
                AppError::new(ErrorCode::ExecutionFailed, msg)
            }
            AutomodusError::ActionFailed(msg) => {
                AppError::new(ErrorCode::StepFailed, msg)
            }
            AutomodusError::Config(msg) => {
                AppError::new(ErrorCode::InvalidRequest, msg)
            }
            AutomodusError::Io(err) => {
                AppError::new(ErrorCode::InternalError, err.to_string())
            }
            AutomodusError::Serialization(msg) => {
                AppError::new(ErrorCode::InvalidRequest, msg)
            }
            AutomodusError::Internal(msg) => {
                AppError::new(ErrorCode::InternalError, msg)
            }
            AutomodusError::DaemonNotRunning => {
                AppError::daemon_not_running()
            }
            AutomodusError::DaemonAlreadyRunning(pid) => {
                AppError::daemon_already_running(pid)
            }
            AutomodusError::DaemonConnectionFailed(msg) => {
                AppError::new(ErrorCode::DaemonConnectionFailed, msg)
            }
            AutomodusError::SessionNotFound(id) => {
                AppError::session_not_found(&id)
            }
            AutomodusError::SessionLimitReached => {
                AppError::new(ErrorCode::SessionLimitReached, "Maximum session limit reached")
            }
            AutomodusError::WorkflowNotFound(name) => {
                AppError::workflow_not_found(&name)
            }
            AutomodusError::WorkflowInvalid(msg) => {
                AppError::new(ErrorCode::WorkflowInvalid, msg)
            }
            AutomodusError::WorkflowTimeout(ms) => {
                AppError::with_details(
                    ErrorCode::WorkflowTimeout,
                    format!("Workflow timed out after {}ms", ms),
                    json!({ "timeout_ms": ms }),
                )
            }
            AutomodusError::SelectorNotFound(selector) => {
                AppError::selector_not_found(&selector, 0)
            }
            AutomodusError::SelectorTimeout(msg) => {
                AppError::new(ErrorCode::SelectorTimeout, msg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_code_serialization() {
        let code = ErrorCode::DaemonNotRunning;
        let json = serde_json::to_string(&code).unwrap();
        assert_eq!(json, "\"DAEMON_NOT_RUNNING\"");
    }

    #[test]
    fn test_app_error_from_automodus_error() {
        let err = AutomodusError::DaemonNotRunning;
        let app_err: AppError = err.into();
        assert_eq!(app_err.code, ErrorCode::DaemonNotRunning);
    }

    #[test]
    fn test_app_error_with_details() {
        let err = AppError::selector_not_found("button.submit", 5000);
        assert_eq!(err.code, ErrorCode::SelectorNotFound);
        assert!(err.details.is_some());
        let details = err.details.unwrap();
        assert_eq!(details["timeout_ms"], 5000);
    }

    #[test]
    fn test_app_error_display() {
        let err = AppError::daemon_not_running();
        let display = format!("{}", err);
        assert!(display.contains("DaemonNotRunning"));
    }
}
