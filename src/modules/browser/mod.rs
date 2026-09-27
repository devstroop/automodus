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
//! - Console and network log capture

pub mod actions;
mod adapter;
mod driver;
pub mod firefox;
pub mod launch;
pub mod selector;
mod session;

pub use crate::actions::{ConsoleEntry, NetworkEntry};
pub use adapter::ChromePageAdapter;
pub use driver::{BrowserService, BrowserServiceConfig};
pub use firefox::FirefoxPageAdapter;
pub use launch::{
    build_browser_config, launch_browser, launch_session, resolve_chrome_path,
    resolve_chrome_path_with, resolve_firefox_path, resolve_lightpanda_path, LaunchOptions,
};
pub use selector::{parse_selector, SelectorType};
pub use session::SessionAdapter;
