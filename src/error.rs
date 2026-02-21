//! Error types for Automodus
//!
//! Generic error types for browser automation.

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
}
