//! Actions Module
//!
//! Core action infrastructure and module-agnostic actions.
//!
//! Browser-specific actions are in `crate::modules::browser::actions`.
//! HTTP actions are in `crate::modules::http::actions`.

pub mod control;
pub mod registry;

pub use registry::{
    Action, ActionContext, ActionError, ActionOutput, ActionRegistry, BrowserHandle, TabInfo,
};

// Core actions (module-agnostic)
pub use control::{EmitAction, LogAction};

// Re-export browser actions for backwards compatibility
pub use crate::modules::browser::actions::{
    // Navigate
    BackAction,
    // Interact
    ClickAction,
    // Extract
    EvalAction,
    ExtractAction,
    // Upload
    FileChooserAction,
    ForwardAction,
    GotoAction,
    HoverAction,
    ReloadAction,
    // Capture
    ScreenshotAction,
    SelectAction,
    // Wait
    SleepAction,
    // Tabs
    TabCloseAction,
    TabListAction,
    TabNewAction,
    TabSwitchAction,
    TypeAction,
    UploadAction,
    WaitForAction,
    WaitUploadAction,
};

// Re-export HTTP actions
pub use crate::modules::http::actions::{
    HttpDeleteAction, HttpGetAction, HttpPatchAction, HttpPostAction, HttpPutAction,
    HttpRequestAction,
};
