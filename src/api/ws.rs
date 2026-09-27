//! WebSocket Handler
//!
//! Real-time event streaming via WebSocket connection.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use super::state::ServerState;

use crate::core::engine::PauseResponse;

/// WebSocket protocol version
pub const WS_PROTOCOL_VERSION: u32 = 1;

/// Events sent from server to client
#[derive(Debug, Clone, Serialize)]
pub struct WsEvent {
    /// Protocol version
    pub version: u32,
    /// Event type
    #[serde(rename = "type")]
    pub event_type: String,
    /// Event payload
    #[serde(flatten)]
    pub payload: serde_json::Value,
}

impl WsEvent {
    /// Create a new WebSocket event
    pub fn new(event_type: &str, payload: serde_json::Value) -> Self {
        Self {
            version: WS_PROTOCOL_VERSION,
            event_type: event_type.to_string(),
            payload,
        }
    }

    /// Create a connected event
    pub fn connected() -> Self {
        Self::new("connected", serde_json::json!({}))
    }

    /// Create an execution started event
    pub fn execution_started(id: &str, workflow: &str) -> Self {
        Self::new(
            "execution.started",
            serde_json::json!({
                "id": id,
                "workflow": workflow
            }),
        )
    }

    /// Create an execution step event
    pub fn execution_step(id: &str, step: usize, action: &str) -> Self {
        Self::new(
            "execution.step",
            serde_json::json!({
                "id": id,
                "step": step,
                "action": action
            }),
        )
    }

    /// Create an execution complete event
    pub fn execution_complete(id: &str, success: bool) -> Self {
        Self::new(
            "execution.complete",
            serde_json::json!({
                "id": id,
                "success": success
            }),
        )
    }

    /// Create an execution error event
    pub fn execution_error(id: &str, error: &str) -> Self {
        Self::new(
            "execution.error",
            serde_json::json!({
                "id": id,
                "error": error
            }),
        )
    }

    /// Create an execution paused event
    pub fn execution_paused(id: &str, step: usize) -> Self {
        Self::new(
            "execution.paused",
            serde_json::json!({
                "id": id,
                "step": step
            }),
        )
    }

    /// Create a session created event
    pub fn session_created(id: &str) -> Self {
        Self::new("session.created", serde_json::json!({"id": id}))
    }

    /// Create a session closed event
    pub fn session_closed(id: &str) -> Self {
        Self::new("session.closed", serde_json::json!({"id": id}))
    }
}

/// Commands sent from client to server
#[derive(Debug, Clone, Deserialize)]
pub struct WsCommand {
    /// Command type
    #[serde(rename = "type")]
    pub command_type: String,
    /// Command payload
    #[serde(flatten)]
    pub payload: serde_json::Value,
}

/// Server event sent via broadcast channel
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Execution started
    ExecutionStarted { id: String, workflow: String },
    /// Execution step
    ExecutionStep {
        id: String,
        step: usize,
        action: String,
    },
    /// Execution complete
    ExecutionComplete { id: String, success: bool },
    /// Execution error
    ExecutionError { id: String, error: String },
    /// Execution paused
    ExecutionPaused { id: String, step: usize },
    /// Session created
    SessionCreated { id: String },
    /// Session closed
    SessionClosed { id: String },
}

impl From<ServerEvent> for WsEvent {
    fn from(event: ServerEvent) -> Self {
        match event {
            ServerEvent::ExecutionStarted { id, workflow } => {
                WsEvent::execution_started(&id, &workflow)
            }
            ServerEvent::ExecutionStep { id, step, action } => {
                WsEvent::execution_step(&id, step, &action)
            }
            ServerEvent::ExecutionComplete { id, success } => {
                WsEvent::execution_complete(&id, success)
            }
            ServerEvent::ExecutionError { id, error } => WsEvent::execution_error(&id, &error),
            ServerEvent::ExecutionPaused { id, step } => WsEvent::execution_paused(&id, step),
            ServerEvent::SessionCreated { id } => WsEvent::session_created(&id),
            ServerEvent::SessionClosed { id } => WsEvent::session_closed(&id),
        }
    }
}

/// WebSocket upgrade handler
///
/// Connect to `/ws` for real-time execution events.
///
/// Events sent by server:
/// - `connected` - Initial connection established
/// - `execution.started` - Workflow execution began
/// - `execution.step` - Execution step completed
/// - `execution.complete` - Execution finished successfully
/// - `execution.error` - Execution failed
/// - `execution.paused` - Execution paused (debug mode)
/// - `session.created` - New session created
/// - `session.closed` - Session closed
///
/// Commands accepted from client:
/// - `continue` - Resume paused execution
/// - `skip` - Skip current step
/// - `abort` - Cancel execution
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    info!("WebSocket connection request");
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Handle individual WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<ServerState>) {
    let (mut sender, mut receiver) = socket.split();

    // Subscribe to server events
    let mut event_rx = state.subscribe_events();

    // Send connected event
    let connected = WsEvent::connected();
    if let Ok(json) = serde_json::to_string(&connected) {
        if let Err(e) = sender.send(Message::Text(json)).await {
            error!("Failed to send connected event: {}", e);
            return;
        }
    }

    info!("WebSocket client connected");

    // Spawn task to forward server events to client
    let send_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    let ws_event: WsEvent = event.into();
                    match serde_json::to_string(&ws_event) {
                        Ok(json) => {
                            debug!("Sending event: {}", ws_event.event_type);
                            if let Err(e) = sender.send(Message::Text(json)).await {
                                error!("Failed to send event: {}", e);
                                break;
                            }
                        }
                        Err(e) => {
                            error!("Failed to serialize event: {}", e);
                        }
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("WebSocket client lagged, missed {} events", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("Event channel closed");
                    break;
                }
            }
        }
    });

    // Handle incoming messages from client
    let state_clone = state.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    debug!("Received command: {}", text);
                    if let Ok(command) = serde_json::from_str::<WsCommand>(&text) {
                        handle_command(&state_clone, command).await;
                    } else {
                        warn!("Invalid WebSocket command: {}", text);
                    }
                }
                Ok(Message::Binary(_)) => {
                    debug!("Received binary message (ignored)");
                }
                Ok(Message::Ping(data)) => {
                    debug!("Received ping");
                    // Pong is sent automatically by axum
                    let _ = data;
                }
                Ok(Message::Pong(_)) => {
                    debug!("Received pong");
                }
                Ok(Message::Close(_)) => {
                    info!("WebSocket client disconnected");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {
            debug!("Send task finished");
        }
        _ = recv_task => {
            debug!("Receive task finished");
        }
    }

    info!("WebSocket connection closed");
}

/// Handle a command from the client
async fn handle_command(state: &Arc<ServerState>, command: WsCommand) {
    match command.command_type.as_str() {
        "continue" => {
            // Resume paused execution
            if let Some(id) = command.payload.get("id").and_then(|v| v.as_str()) {
                info!("Continue command for execution: {}", id);
                if let Err(e) = state.resolve_pause(id, PauseResponse::Continue).await {
                    warn!("Continue signal failed: {}", e);
                }
            }
        }
        "skip" => {
            // Skip current step
            if let Some(id) = command.payload.get("id").and_then(|v| v.as_str()) {
                info!("Skip command for execution: {}", id);
                if let Err(e) = state.resolve_pause(id, PauseResponse::Skip).await {
                    warn!("Skip signal failed: {}", e);
                }
            }
        }
        "abort" => {
            // Abort execution — resolve any pending pause AND cancel the token
            if let Some(id) = command.payload.get("id").and_then(|v| v.as_str()) {
                info!("Abort command for execution: {}", id);
                // Try resolving a pending pause first (engine returns Abort immediately)
                let _ = state.resolve_pause(id, PauseResponse::Abort).await;
                // Also cancel the token so non-paused executions stop at next step
                let _ = state.cancel_execution(id).await;
            }
        }
        "ping" => {
            debug!("Ping command received");
        }
        _ => {
            warn!("Unknown command: {}", command.command_type);
        }
    }
}
