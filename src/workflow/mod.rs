//! Workflow Module
//!
//! YAML-based automation workflow parsing, validation, and loading.

pub mod loader;
pub mod parser;
pub mod schema;

pub use loader::WorkflowLoader;
pub use parser::WorkflowParser;
pub use schema::*;

/// Trait for resolving workflow names to definitions at runtime.
///
/// Used by the engine to look up sub-workflows during `call` action execution.
#[async_trait::async_trait]
pub trait WorkflowResolver: Send + Sync {
    /// Resolve a workflow by name (e.g. "auth/qr_login", "send_message")
    async fn resolve(&self, name: &str) -> anyhow::Result<Workflow>;
}

/// WorkflowLoader implements WorkflowResolver directly.
#[async_trait::async_trait]
impl WorkflowResolver for WorkflowLoader {
    async fn resolve(&self, name: &str) -> anyhow::Result<Workflow> {
        self.load(name).await
    }
}
