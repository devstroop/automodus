//! Browser Actions
//!
//! Automation actions specific to browser automation via CDP.
//!
//! # Available Actions
//!
//! **Navigation**
//! - `goto` - Navigate to a URL
//! - `back` - Go back in browser history
//! - `forward` - Go forward in browser history
//! - `reload` - Reload the current page
//!
//! **Interaction**
//! - `click` - Click an element
//! - `type` - Type text into an element
//! - `select` - Select an option from a dropdown
//! - `hover` - Hover over an element
//!
//! **Extraction**
//! - `extract` - Extract data from elements
//! - `eval` - Evaluate JavaScript in the page
//!
//! **Wait**
//! - `wait_for` - Wait for an element to appear
//! - `sleep` - Wait for a fixed duration
//!
//! **Tabs**
//! - `tab_new` - Open a new tab
//! - `tab_switch` - Switch to a different tab
//! - `tab_close` - Close a tab
//!
//! **Capture**
//! - `screenshot` - Take a screenshot
//!
//! **Upload**
//! - `upload` - Upload a file
//! - `wait_upload` - Wait for file upload
//! - `file_chooser` - Handle file chooser dialog

pub mod capture;
pub mod extract;
pub mod interact;
pub mod navigate;
pub mod tabs;
pub mod upload;
pub mod wait;

// Re-export actions
pub use capture::ScreenshotAction;
pub use extract::{EvalAction, ExtractAction};
pub use interact::{ClickAction, HoverAction, SelectAction, TypeAction};
pub use navigate::{BackAction, ForwardAction, GotoAction, ReloadAction};
pub use tabs::{TabCloseAction, TabNewAction, TabSwitchAction};
pub use upload::{FileChooserAction, UploadAction, WaitUploadAction};
pub use wait::{SleepAction, WaitForAction};
