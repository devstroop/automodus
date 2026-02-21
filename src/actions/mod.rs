//! Actions Module
//!
//! Core action infrastructure and module-agnostic actions.
//!
//! Browser-specific actions are in `crate::modules::browser::actions`.
//! HTTP actions are in `crate::modules::http::actions`.

pub mod control;
pub mod registry;

pub use registry::{
    Action, ActionContext, ActionError, ActionOutput, ActionRegistry, BrowserHandle,
};

// Core actions (module-agnostic)
pub use control::{EmitAction, LogAction};

// Re-export browser actions for backwards compatibility
pub use crate::modules::browser::actions::{
    // Capture
    ScreenshotAction,
    // Extract
    EvalAction, ExtractAction,
    // Interact
    ClickAction, HoverAction, SelectAction, TypeAction,
    // Navigate
    BackAction, ForwardAction, GotoAction, ReloadAction,
    // Tabs
    TabCloseAction, TabNewAction, TabSwitchAction,
    // Upload
    FileChooserAction, UploadAction, WaitUploadAction,
    // Wait
    SleepAction, WaitForAction,
};

// Re-export HTTP actions
pub use crate::modules::http::actions::{
    HttpDeleteAction, HttpGetAction, HttpPatchAction, HttpPostAction, HttpPutAction,
    HttpRequestAction,
};
