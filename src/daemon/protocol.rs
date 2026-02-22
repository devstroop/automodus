//! Socket Protocol
//!
//! Length-prefixed JSON messages over Unix domain sockets.
//!
//! ## Framing
//!
//! Each message is sent as:
//! - 4 bytes: big-endian u32 payload length
//! - N bytes: UTF-8 JSON payload
//!
//! ## Request/Response
//!
//! Client sends a `SocketRequest`, daemon responds with a `SocketResponse`.
//! Each request gets exactly one response.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Maximum message size (16 MB)
const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

// ============================================================================
// Request Types
// ============================================================================

/// Client → Daemon request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SocketRequest {
    /// Ping — check daemon is alive
    Ping,

    /// Get daemon status
    Status,

    // --- Session Management ---
    /// Create a new session
    SessionCreate {
        name: Option<String>,
        #[serde(default)]
        keep_alive: bool,
    },
    /// List all sessions
    SessionList,
    /// Get session by ID
    SessionGet { id: String },
    /// Find session by ID or name
    SessionFind { id_or_name: String },
    /// Close a session
    SessionClose { id: String },
    /// Set keep-alive on a session
    SessionSetKeepAlive { id: String, keep_alive: bool },

    // --- Browser Commands ---
    /// Navigate to URL
    BrowserGoto { url: String },
    /// Click an element
    BrowserClick { selector: String },
    /// Type text into an element
    BrowserType { selector: String, text: String },
    /// Wait for an element
    BrowserWait {
        selector: String,
        timeout: Option<u64>,
    },
    /// Take a screenshot
    BrowserScreenshot {
        #[serde(default)]
        full_page: bool,
    },
    /// Execute JavaScript
    BrowserEval { script: String },
    /// Get element text
    BrowserGetText { selector: String },
    /// Get current page URL
    BrowserGetUrl,
    /// Navigate back
    BrowserBack,
    /// Navigate forward
    BrowserForward,
    /// Reload page
    BrowserReload,
    /// Highlight an element
    BrowserHighlight { selector: String },

    // --- Workflows ---
    /// Run a workflow by file path
    WorkflowRun {
        path: String,
        #[serde(default)]
        params: HashMap<String, String>,
    },
    /// List available workflows
    WorkflowList,
}

// ============================================================================
// Response Types
// ============================================================================

/// Daemon → Client response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocketResponse {
    /// Whether the request succeeded
    pub ok: bool,
    /// Response data (present on success)
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
    /// Error message (present on failure)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SocketResponse {
    /// Create a success response with no data
    pub fn ok() -> Self {
        Self {
            ok: true,
            data: serde_json::Value::Null,
            error: None,
        }
    }

    /// Create a success response with data
    pub fn ok_data(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            data,
            error: None,
        }
    }

    /// Create an error response
    pub fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: serde_json::Value::Null,
            error: Some(message.into()),
        }
    }
}

// ============================================================================
// Wire Format: Length-Prefixed JSON
// ============================================================================

/// Write a message to a socket (length-prefixed JSON)
pub async fn write_message<W, T>(writer: &mut W, msg: &T) -> Result<(), ProtocolError>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let payload = serde_json::to_vec(msg).map_err(ProtocolError::Serialize)?;
    let len = payload.len() as u32;

    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge(len));
    }

    writer
        .write_all(&len.to_be_bytes())
        .await
        .map_err(ProtocolError::Io)?;
    writer
        .write_all(&payload)
        .await
        .map_err(ProtocolError::Io)?;
    writer.flush().await.map_err(ProtocolError::Io)?;

    Ok(())
}

/// Read a message from a socket (length-prefixed JSON)
pub async fn read_message<R, T>(reader: &mut R) -> Result<T, ProtocolError>
where
    R: AsyncReadExt + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut len_buf = [0u8; 4];
    reader
        .read_exact(&mut len_buf)
        .await
        .map_err(ProtocolError::Io)?;
    let len = u32::from_be_bytes(len_buf);

    if len > MAX_MESSAGE_SIZE {
        return Err(ProtocolError::MessageTooLarge(len));
    }

    let mut payload = vec![0u8; len as usize];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(ProtocolError::Io)?;

    serde_json::from_slice(&payload).map_err(ProtocolError::Deserialize)
}

// ============================================================================
// Errors
// ============================================================================

/// Protocol errors
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    Io(std::io::Error),

    #[error("Serialization error: {0}")]
    Serialize(serde_json::Error),

    #[error("Deserialization error: {0}")]
    Deserialize(serde_json::Error),

    #[error("Message too large: {0} bytes (max {MAX_MESSAGE_SIZE})")]
    MessageTooLarge(u32),

    #[error("Connection closed")]
    ConnectionClosed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = SocketRequest::Ping;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"ping\""));

        let req = SocketRequest::BrowserGoto {
            url: "https://example.com".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"browser_goto\""));
        assert!(json.contains("https://example.com"));
    }

    #[test]
    fn test_request_deserialization() {
        let json = r#"{"type":"ping"}"#;
        let req: SocketRequest = serde_json::from_str(json).unwrap();
        assert!(matches!(req, SocketRequest::Ping));

        let json = r#"{"type":"session_create","name":"test","keep_alive":true}"#;
        let req: SocketRequest = serde_json::from_str(json).unwrap();
        match req {
            SocketRequest::SessionCreate { name, keep_alive } => {
                assert_eq!(name, Some("test".to_string()));
                assert!(keep_alive);
            }
            _ => panic!("Expected SessionCreate"),
        }
    }

    #[test]
    fn test_response_serialization() {
        let resp = SocketResponse::ok();
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ok\":true"));
        assert!(!json.contains("data")); // Null data skipped

        let resp = SocketResponse::err("test error");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"ok\":false"));
        assert!(json.contains("test error"));
    }

    #[tokio::test]
    async fn test_message_roundtrip() {
        use tokio::io::duplex;

        let (mut client, mut server) = duplex(4096);

        let req = SocketRequest::BrowserGoto {
            url: "https://example.com".to_string(),
        };
        write_message(&mut client, &req).await.unwrap();

        let received: SocketRequest = read_message(&mut server).await.unwrap();
        match received {
            SocketRequest::BrowserGoto { url } => assert_eq!(url, "https://example.com"),
            _ => panic!("Expected BrowserGoto"),
        }

        let resp = SocketResponse::ok_data(serde_json::json!({"url": "https://example.com"}));
        write_message(&mut server, &resp).await.unwrap();

        let received: SocketResponse = read_message(&mut client).await.unwrap();
        assert!(received.ok);
        assert_eq!(received.data["url"], "https://example.com");
    }
}
