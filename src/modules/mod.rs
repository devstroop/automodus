//! Automation Modules
//!
//! Pluggable automation modules for different automation types.
//!
//! # Architecture
//!
//! Automodus supports multiple automation modules:
//! - **Browser**: Chromium browser automation via CDP
//! - **HTTP**: REST API / HTTP request automation
//! - **LLM** (planned): AI/LLM integration
//!
//! Each module provides its own set of actions that can be used in workflows.

pub mod browser;
pub mod http;

pub use browser::launch::{build_browser_config, launch_browser, launch_session, LaunchOptions};
pub use browser::{BrowserService, BrowserServiceConfig, ChromePageAdapter};
pub use http::{HttpClient, HttpConfig};
