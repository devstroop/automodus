//! Shell Module
//!
//! Interactive shell with readline support for command history,
//! line editing, and autocomplete.

mod client;

pub use client::{ShellClient, ShellCommand, ShellConfig};
