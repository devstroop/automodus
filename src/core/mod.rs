//! Core Execution Engine
//!
//! Executes workflows and manages browser instances.

pub mod context;
pub mod engine;
pub mod template;

pub use context::ExecutionContext;
pub use engine::{WorkflowEngine, WorkflowResult};
pub use template::TemplateEngine;
