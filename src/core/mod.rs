//! Core Execution Engine
//!
//! Executes workflows and manages browser instances.

pub mod app;
pub mod context;
pub mod engine;
pub mod json_path;
pub mod template;

pub use app::{AppCore, CoreEvent, Session, SessionError, SessionInfo};
pub use context::ExecutionContext;
pub use engine::{
    DefaultPauseHandler, PauseHandler, PauseResponse, ShellPauseHandler, WorkflowEngine,
    WorkflowResult,
};
pub use template::TemplateEngine;
