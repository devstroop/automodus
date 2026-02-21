//! Browser Module
//!
//! Chromium browser automation via CDP (Chrome DevTools Protocol).
//!
//! This module provides:
//! - Browser service management (launch, connect, pool)
//! - Page adapter for workflow action execution
//! - Browser-specific automation actions
//! - Launch utilities for browser configuration

mod adapter;
mod driver;
pub mod actions;
pub mod launch;

pub use adapter::ChromePageAdapter;
pub use driver::{BrowserService, BrowserServiceConfig};
pub use launch::{build_browser_config, launch_browser, LaunchOptions};
