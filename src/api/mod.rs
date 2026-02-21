//! API Module
//!
//! REST API server for Automodus.
//!
//! # Architecture
//!
//! - `server` - HTTP server setup and routing
//! - `handlers` - Request handlers for each endpoint
//! - `schemas` - Request/response data structures
//! - `state` - Shared server state
//!
//! # Usage
//!
//! ```ignore
//! use automodus::api;
//!
//! #[tokio::main]
//! async fn main() {
//!     api::run_server().await.unwrap();
//! }
//! ```

pub mod handlers;
pub mod schemas;
pub mod server;
pub mod state;

pub use server::{create_router, run_server, ApiDoc};
pub use state::{create_state, ServerState};
