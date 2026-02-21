//! Workflow Triggers
//!
//! Defines how workflows can be triggered:
//!
//! - **API**: HTTP REST API endpoints (`POST /workflows/my-workflow`)
//! - **Schedule**: Cron-like scheduling (`"0 * * * *"`)
//! - **Event**: Internal event bus (subscribe to workflow events)
//! - **Webhook**: External webhook callbacks with signature verification
//! - **Manual**: User-initiated via CLI (`automodus run <workflow.yaml>`)
//!
//! # Trigger Configuration in Workflows
//!
//! Triggers are defined in the `on:` section of workflow YAML files:
//!
//! ```yaml
//! name: my-workflow
//! on:
//!   api:                        # REST API trigger
//!     path: /workflows/my-flow
//!     method: POST
//!   schedule: "0 */4 * * *"     # Cron schedule (every 4 hours)
//!   event: "user.login"         # Event-based trigger
//!   webhook:                    # Webhook trigger
//!     path: /hooks/my-hook
//!     secret: "webhook-secret"
//!   manual: true                # Manual trigger only
//! ```
//!
//! # Note
//!
//! Currently trigger types are defined in `workflow::schema::Triggers`.
//! This module is reserved for future trigger implementation logic
//! (e.g., cron scheduler, event bus, webhook verification).

// Re-export trigger types from workflow schema for convenience
pub use crate::workflow::schema::{ApiTrigger, Triggers, WatchTrigger, WebhookTrigger};
