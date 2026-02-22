//! Browser Module
//!
//! Chromium browser automation via CDP (Chrome DevTools Protocol).
//!
//! This module provides:
//! - Browser service management (launch, connect, pool)
//! - Page adapter for workflow action execution
//! - Browser-specific automation actions
//! - Launch utilities for browser configuration
//! - Extended selector support (text:, role:, xpath:, etc.)

pub mod actions;
mod adapter;
mod driver;
pub mod launch;
pub mod selector;

pub use adapter::ChromePageAdapter;
pub use driver::{BrowserService, BrowserServiceConfig};
pub use launch::{build_browser_config, launch_browser, LaunchOptions};
pub use selector::{parse_selector, SelectorType};
