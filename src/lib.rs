//! # Automodus - Programmable Workflow Automation Platform
//!
//! A powerful browser automation engine with YAML-based workflows.
//!
//! ## Architecture
//!
//! The library provides:
//!
//! - **Workflow Engine**: YAML-defined workflows with triggers, actions, and event handling
//! - **Actions**: Built-in browser automation actions (navigate, click, extract, etc.)
//! - **Triggers**: API endpoints, schedules, events, webhooks for workflow invocation
//! - **Browser**: Multi-instance Chromium browser automation via CDP
//!
//! ## Features
//!
//! ```toml
//! [dependencies]
//! automodus = "0.1"
//! ```
//!
//! ## Quick Start
//!
//! ```yaml
//! # ../examples/browser/search_form.yaml
//! name: search_example
//! description: Search and extract results
//!
//! triggers:
//!   api:
//!     path: /search
//!     method: POST
//!
//! params:
//!   query:
//!     type: string
//!     required: true
//!
//! steps:
//!   - action: goto
//!     url: "https://example.com/search"
//!   
//!   - action: type
//!     selector: "input[name=q]"
//!     text: "{{params.query}}"
//!   
//!   - action: click
//!     selector: "button[type=submit]"
//!   
//!   - action: wait_for
//!     selector: ".results"
//!   
//!   - action: extract
//!     selector: ".result-item"
//!     many: true
//!     as: results
//!
//! output:
//!   query: "{{params.query}}"
//!   results: "{{store.results}}"
//! ```

// ============================================================================
// Core Modules
// ============================================================================

pub mod config;
pub mod error;
pub mod utils;

// ============================================================================
// Workflow Engine
// ============================================================================

/// Workflow schema and parsing
pub mod workflow;

/// Built-in actions
pub mod actions;

/// Execution engine
pub mod core;

/// Triggers (API, schedule, events)
pub mod triggers;

/// Daemon process
pub mod daemon;

/// Interactive shell
pub mod shell;

// ============================================================================
// Browser Automation
// ============================================================================

pub mod modules;

// ============================================================================
// REST API Server
// ============================================================================

pub mod api;

// ============================================================================
// Public API Re-exports
// ============================================================================

// Workflow types
pub use workflow::{Step, Workflow, WorkflowLoader, WorkflowParser};

// Action types
pub use actions::{
    Action, ActionContext, ActionError, ActionOutput, ActionRegistry, BrowserCapabilities,
    BrowserHandle, ConsoleEntry, NetworkEntry,
};

// Engine types
pub use core::{
    AppCore, CoreEvent, ExecutionContext, Session, SessionError, SessionInfo, WorkflowEngine,
    WorkflowResult,
};

// Daemon types
pub use daemon::{Daemon, DaemonClient, DaemonConfig, DaemonError, DaemonEvent, DaemonStatus};

// Browser types
pub use modules::{BrowserService, ChromePageAdapter, FirefoxPageAdapter, SessionAdapter};

// Configuration
pub use config::{AppConfig, BrowserEngine};

// Errors
pub use error::{AppError, AutomodusError, ErrorCode, Result};
