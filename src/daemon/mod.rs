//! Daemon Module
//!
//! Long-running background process that owns browser sessions and workflow execution.
//! Implements Docker-style daemon/client separation.

pub mod config;
pub mod protocol;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::actions::BrowserHandle;
use crate::core::AppCore;

pub use config::{
    ensure_config_exists, load_config, validate_config, BrowserSection, DaemonConfigFile,
    DaemonSection, HttpSection, LimitsSection, Viewport,
};
pub use protocol::{SocketRequest, SocketResponse};

/// Daemon configuration
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Unix socket path (e.g., ~/.automodus/automodus.sock)
    pub socket_path: PathBuf,
    /// PID file path (e.g., ~/.automodus/daemon.pid)
    pub pid_file: PathBuf,
    /// Log file path (e.g., ~/.automodus/daemon.log)
    pub log_file: PathBuf,
    /// HTTP server host
    pub http_host: String,
    /// HTTP server port
    pub http_port: u16,
    /// Maximum number of browser sessions
    pub max_sessions: usize,
    /// Debug output directory
    pub debug_dir: PathBuf,
    /// Enable HTTP server
    pub enable_http: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let base_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".automodus");

        Self {
            socket_path: base_dir.join("automodus.sock"),
            pid_file: base_dir.join("daemon.pid"),
            log_file: base_dir.join("daemon.log"),
            http_host: "127.0.0.1".to_string(),
            http_port: 8080,
            max_sessions: 10,
            debug_dir: PathBuf::from("data/debug"),
            enable_http: true,
        }
    }
}

/// Events broadcast by the daemon to connected clients
#[derive(Debug, Clone)]
pub enum DaemonEvent {
    /// Workflow execution started
    ExecutionStarted {
        id: String,
        workflow: String,
    },
    /// Workflow step executed
    ExecutionStep {
        id: String,
        step: usize,
        action: String,
    },
    /// Workflow execution completed
    ExecutionComplete {
        id: String,
        success: bool,
        duration_ms: i64,
    },
    /// Workflow execution failed
    ExecutionError {
        id: String,
        error: String,
    },
    /// Execution paused (debug mode)
    ExecutionPaused {
        id: String,
        step: usize,
    },
    /// Console log captured
    ConsoleLog {
        level: String,
        message: String,
    },
    /// Network request captured
    NetworkRequest {
        method: String,
        url: String,
        status: Option<u32>,
    },
    /// Session created
    SessionCreated {
        id: String,
        name: Option<String>,
    },
    /// Session closed
    SessionClosed {
        id: String,
    },
}

/// Daemon process - owns browser pool and workflows
pub struct Daemon {
    /// Application core (shared state)
    core: Arc<AppCore>,
    /// Daemon configuration
    config: DaemonConfig,
    /// Unix socket listener handle
    socket_listener: Option<UnixListener>,
    /// Shutdown signal sender
    shutdown_tx: broadcast::Sender<()>,
}

impl Daemon {
    /// Create a new daemon instance
    pub fn new(config: DaemonConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let mut core = AppCore::new(&config);
        // Apply browser settings from AppConfig when available (engine, headless).
        // launch_session dispatches on engine (chromium | firefox).
        if let Ok(app_cfg) = crate::config::AppConfig::load() {
            core.set_headless(app_cfg.browser.headless);
            core.set_engine(app_cfg.browser.engine);
        }
        Self {
            core: Arc::new(core),
            config,
            socket_listener: None,
            shutdown_tx,
        }
    }

    /// Get the application core
    pub fn core(&self) -> Arc<AppCore> {
        self.core.clone()
    }

    /// Start the daemon
    pub async fn start(&mut self) -> Result<(), DaemonError> {
        info!("Starting automodus daemon...");

        // Ensure base directory exists
        if let Some(parent) = self.config.socket_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DaemonError::StartupFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        // Check for existing daemon
        if self.is_running() {
            return Err(DaemonError::AlreadyRunning);
        }

        // Write PID file
        self.write_pid_file()?;

        // Remove stale socket if exists
        if self.config.socket_path.exists() {
            std::fs::remove_file(&self.config.socket_path).ok();
        }

        // Start Unix socket listener
        let listener = UnixListener::bind(&self.config.socket_path).map_err(|e| {
            DaemonError::StartupFailed(format!("Failed to bind socket: {}", e))
        })?;
        self.socket_listener = Some(listener);

        info!(
            socket = %self.config.socket_path.display(),
            pid_file = %self.config.pid_file.display(),
            "Daemon started"
        );

        Ok(())
    }

    /// Run the daemon main loop
    pub async fn run(&mut self) -> Result<(), DaemonError> {
        let listener = self
            .socket_listener
            .take()
            .ok_or_else(|| DaemonError::NotStarted)?;

        let core = self.core.clone();
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        // Optionally start HTTP server
        if self.config.enable_http {
            let http_host = self.config.http_host.clone();
            let http_port = self.config.http_port;
            let http_shutdown_rx = self.shutdown_tx.subscribe();
            let http_core = self.core.clone();

            tokio::spawn(async move {
                match start_http_server(http_host, http_port, http_shutdown_rx, http_core).await {
                    Ok(_) => info!("HTTP server stopped"),
                    Err(e) => error!("HTTP server error: {}", e),
                }
            });

            info!(
                "HTTP server running on http://{}:{}",
                self.config.http_host, self.config.http_port
            );
        }

        // Spawn periodic session cleanup task
        {
            let cleanup_core = self.core.clone();
            let mut cleanup_shutdown = self.shutdown_tx.subscribe();
            tokio::spawn(async move {
                let interval = std::time::Duration::from_secs(60);
                loop {
                    tokio::select! {
                        _ = tokio::time::sleep(interval) => {
                            let removed = cleanup_core.cleanup_idle_sessions().await;
                            if !removed.is_empty() {
                                info!("Cleaned up {} idle session(s)", removed.len());
                            }
                        }
                        _ = cleanup_shutdown.recv() => break,
                    }
                }
            });
        }

        loop {
            tokio::select! {
                // Accept new socket connection
                result = listener.accept() => {
                    match result {
                        Ok((stream, _addr)) => {
                            let core = core.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle_socket_connection(stream, core).await {
                                    warn!("Socket connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }

                // Handle shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("Received shutdown signal");
                    break;
                }

                // Handle SIGTERM/SIGINT
                _ = tokio::signal::ctrl_c() => {
                    info!("Received interrupt signal");
                    break;
                }
            }
        }

        self.cleanup().await;
        Ok(())
    }

    /// Stop the daemon gracefully
    pub async fn stop(&self) -> Result<(), DaemonError> {
        info!("Stopping daemon...");

        // Send shutdown signal
        let _ = self.shutdown_tx.send(());

        // Cleanup will be done in run() after receiving the signal
        Ok(())
    }

    /// Check if daemon is already running
    pub fn is_running(&self) -> bool {
        if let Ok(pid_str) = std::fs::read_to_string(&self.config.pid_file) {
            if let Ok(pid) = pid_str.trim().parse::<i32>() {
                // Check if process exists (Unix-specific)
                #[cfg(unix)]
                {
                    let result = std::process::Command::new("kill")
                        .arg("-0")
                        .arg(pid.to_string())
                        .output();
                    return result.map(|o| o.status.success()).unwrap_or(false);
                }
                #[cfg(not(unix))]
                {
                    return false;
                }
            }
        }
        false
    }

    /// Get daemon status
    pub fn status(&self) -> DaemonStatus {
        if self.is_running() {
            if let Ok(pid_str) = std::fs::read_to_string(&self.config.pid_file) {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    return DaemonStatus::Running { pid };
                }
            }
            DaemonStatus::Running { pid: 0 }
        } else {
            DaemonStatus::Stopped
        }
    }

    /// Write PID file
    fn write_pid_file(&self) -> Result<(), DaemonError> {
        let pid = std::process::id();
        std::fs::write(&self.config.pid_file, pid.to_string()).map_err(|e| {
            DaemonError::StartupFailed(format!("Failed to write PID file: {}", e))
        })?;
        Ok(())
    }

    /// Cleanup on shutdown
    async fn cleanup(&self) {
        info!("Cleaning up daemon resources...");

        // Close all sessions
        self.core.close_all_sessions().await;

        // Remove PID file
        if self.config.pid_file.exists() {
            let _ = std::fs::remove_file(&self.config.pid_file);
        }

        // Remove socket file
        if self.config.socket_path.exists() {
            let _ = std::fs::remove_file(&self.config.socket_path);
        }

        info!("Daemon cleanup complete");
    }
}

/// Daemon status
#[derive(Debug, Clone)]
pub enum DaemonStatus {
    Running { pid: u32 },
    Stopped,
}

impl std::fmt::Display for DaemonStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonStatus::Running { pid } => write!(f, "Running (PID {})", pid),
            DaemonStatus::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Daemon error types
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("Daemon is already running")]
    AlreadyRunning,

    #[error("Daemon is not running")]
    NotRunning,

    #[error("Daemon not started")]
    NotStarted,

    #[error("Failed to start daemon: {0}")]
    StartupFailed(String),

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Command failed: {0}")]
    CommandFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Handle a Unix socket connection
async fn handle_socket_connection(
    stream: tokio::net::UnixStream,
    core: Arc<AppCore>,
) -> Result<(), DaemonError> {
    use protocol::{read_message, write_message, SocketRequest};

    let (mut reader, mut writer) = tokio::io::split(stream);

    loop {
        let request: SocketRequest = match read_message(&mut reader).await {
            Ok(req) => req,
            Err(protocol::ProtocolError::Io(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                // Client disconnected
                break;
            }
            Err(e) => {
                warn!("Protocol error: {}", e);
                break;
            }
        };

        let response = dispatch_request(&core, request).await;

        if let Err(e) = write_message(&mut writer, &response).await {
            warn!("Failed to send response: {}", e);
            break;
        }
    }

    Ok(())
}

/// Dispatch a socket request to the appropriate handler
async fn dispatch_request(core: &Arc<AppCore>, request: SocketRequest) -> SocketResponse {
    use base64::Engine;

    match request {
        SocketRequest::Ping => SocketResponse::ok_data(serde_json::json!({"pong": true})),

        SocketRequest::Status => {
            let sessions = core.list_sessions().await;
            let has_browser = core.has_browser().await;
            SocketResponse::ok_data(serde_json::json!({
                "browser_running": has_browser,
                "sessions": sessions.len(),
            }))
        }

        // --- Session Commands ---
        SocketRequest::SessionCreate { name, keep_alive } => {
            match core.create_session(name.clone()).await {
                Ok(id) => {
                    if keep_alive {
                        let _ = core.set_session_keep_alive(&id, true).await;
                    }
                    SocketResponse::ok_data(serde_json::json!({"id": id}))
                }
                Err(e) => SocketResponse::err(e.to_string()),
            }
        }

        SocketRequest::SessionList => {
            let sessions = core.list_sessions().await;
            let list: Vec<serde_json::Value> = sessions
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "name": s.name,
                        "created_at": s.created_at.to_rfc3339(),
                        "last_activity": s.last_activity.to_rfc3339(),
                        "keep_alive": s.keep_alive,
                    })
                })
                .collect();
            SocketResponse::ok_data(serde_json::json!({"sessions": list}))
        }

        SocketRequest::SessionGet { id } => match core.get_session(&id).await {
            Some(s) => SocketResponse::ok_data(serde_json::json!({
                "id": s.id,
                "name": s.name,
                "created_at": s.created_at.to_rfc3339(),
                "last_activity": s.last_activity.to_rfc3339(),
                "keep_alive": s.keep_alive,
            })),
            None => SocketResponse::err(format!("Session '{}' not found", id)),
        },

        SocketRequest::SessionFind { id_or_name } => match core.find_session(&id_or_name).await {
            Some(s) => SocketResponse::ok_data(serde_json::json!({
                "id": s.id,
                "name": s.name,
                "created_at": s.created_at.to_rfc3339(),
                "last_activity": s.last_activity.to_rfc3339(),
                "keep_alive": s.keep_alive,
            })),
            None => SocketResponse::err(format!("No session matching '{}'", id_or_name)),
        },

        SocketRequest::SessionClose { id } => match core.close_session(&id).await {
            Ok(()) => SocketResponse::ok(),
            Err(e) => SocketResponse::err(e.to_string()),
        },

        SocketRequest::SessionSetKeepAlive { id, keep_alive } => {
            match core.set_session_keep_alive(&id, keep_alive).await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            }
        }

        // --- Browser Commands ---
        SocketRequest::BrowserGoto { url } => {
            match core.get_page().await {
                Ok(adapter) => match adapter.goto(&url).await {
                    Ok(()) => {
                        let current = adapter.current_url().await.unwrap_or_default();
                        SocketResponse::ok_data(serde_json::json!({"url": current}))
                    }
                    Err(e) => SocketResponse::err(e.to_string()),
                },
                Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
            }
        }

        SocketRequest::BrowserClick { selector } => match core.get_page().await {
            Ok(adapter) => match adapter.click(&selector).await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserType { selector, text } => match core.get_page().await {
            Ok(adapter) => match adapter.type_text(&selector, &text, true).await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserWait { selector, timeout } => match core.get_page().await {
            Ok(adapter) => match adapter.wait_for(&selector, timeout.unwrap_or(5000)).await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserScreenshot { full_page } => match core.get_page().await {
            Ok(adapter) => match adapter.screenshot(full_page).await {
                Ok(bytes) => {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                    SocketResponse::ok_data(serde_json::json!({"png_base64": b64, "size": bytes.len()}))
                }
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserEval { script } => match core.get_page().await {
            Ok(adapter) => match adapter.eval(&script).await {
                Ok(value) => SocketResponse::ok_data(serde_json::json!({"result": value})),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserGetText { selector } => match core.get_page().await {
            Ok(adapter) => match adapter.get_text(&selector).await {
                Ok(text) => SocketResponse::ok_data(serde_json::json!({"text": text})),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserGetUrl => match core.get_page().await {
            Ok(adapter) => match adapter.current_url().await {
                Ok(url) => SocketResponse::ok_data(serde_json::json!({"url": url})),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserBack => match core.get_page().await {
            Ok(adapter) => match adapter.back().await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserForward => match core.get_page().await {
            Ok(adapter) => match adapter.forward().await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserReload => match core.get_page().await {
            Ok(adapter) => match adapter.reload().await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserHighlight { selector } => match core.get_page().await {
            Ok(adapter) => {
                let js = format!(
                    r#"(function() {{
                        const el = document.querySelector('{}');
                        if (!el) return 'not found';
                        el.style.outline = '3px solid red';
                        el.style.outlineOffset = '2px';
                        setTimeout(() => {{ el.style.outline = ''; el.style.outlineOffset = ''; }}, 3000);
                        return 'highlighted';
                    }})()
                    "#,
                    selector.replace('\\', "\\\\").replace('\'', "\\'")
                );
                match adapter.eval(&js).await {
                    Ok(val) => {
                        let result = val.as_str().unwrap_or("done");
                        if result == "not found" {
                            SocketResponse::err(format!("Element not found: {}", selector))
                        } else {
                            SocketResponse::ok()
                        }
                    }
                    Err(e) => SocketResponse::err(e.to_string()),
                }
            }
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserFind { selector } => match core.get_page().await {
            Ok(adapter) => {
                let js = format!(
                    r#"(function() {{
                        var els = document.querySelectorAll('{}');
                        var results = [];
                        for (var i = 0; i < Math.min(els.length, 10); i++) {{
                            var el = els[i];
                            results.push({{
                                tag: el.tagName.toLowerCase(),
                                id: el.id || null,
                                classes: el.className || null,
                                text: (el.textContent || '').trim().substring(0, 100)
                            }});
                        }}
                        return JSON.stringify({{ count: els.length, matches: results }});
                    }})()
                    "#,
                    selector.replace('\\', "\\\\").replace('\'', "\\'")
                );
                match adapter.eval(&js).await {
                    Ok(val) => {
                        let json_str = val.as_str().unwrap_or("{}");
                        match serde_json::from_str::<serde_json::Value>(json_str) {
                            Ok(data) => SocketResponse::ok_data(data),
                            Err(_) => SocketResponse::ok_data(serde_json::json!({"count": 0, "matches": []})),
                        }
                    }
                    Err(e) => SocketResponse::err(e.to_string()),
                }
            }
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        // --- Tab Management ---
        SocketRequest::BrowserTabList => match core.get_page().await {
            Ok(adapter) => match adapter.list_tabs().await {
                Ok(tabs) => {
                    let tab_data: Vec<serde_json::Value> = tabs
                        .iter()
                        .map(|t| serde_json::json!({"index": t.index, "url": t.url, "active": t.active}))
                        .collect();
                    SocketResponse::ok_data(serde_json::json!({"tabs": tab_data}))
                }
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserTabNew { url } => match core.get_page().await {
            Ok(adapter) => match adapter.new_tab(url.as_deref()).await {
                Ok(index) => SocketResponse::ok_data(serde_json::json!({"index": index})),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserTabSwitch { index } => match core.get_page().await {
            Ok(adapter) => match adapter.switch_tab(index).await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        SocketRequest::BrowserTabClose { index } => match core.get_page().await {
            Ok(adapter) => match adapter.close_tab(index).await {
                Ok(()) => SocketResponse::ok(),
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        // --- PDF ---
        SocketRequest::BrowserPdf => match core.get_page().await {
            Ok(adapter) => match adapter.pdf().await {
                Ok(bytes) => {
                    let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);
                    SocketResponse::ok_data(serde_json::json!({"pdf_base64": b64, "size": bytes.len()}))
                }
                Err(e) => SocketResponse::err(e.to_string()),
            },
            Err(e) => SocketResponse::err(format!("Browser error: {}", e)),
        },

        // --- Debug ---
        SocketRequest::DebugClean => {
            let debug_dir = core.debug_dir();
            let policy = crate::utils::CleanupPolicy::default();
            match crate::utils::cleanup_debug_dir(debug_dir, &policy) {
                Ok(stats) => SocketResponse::ok_data(serde_json::json!({
                    "files_removed": stats.files_removed,
                    "bytes_freed": stats.bytes_freed,
                    "files_remaining": stats.files_remaining,
                })),
                Err(e) => SocketResponse::err(format!("Cleanup failed: {}", e)),
            }
        }

        // --- Workflow Commands ---
        SocketRequest::WorkflowRun { path, params } => {
            let file_path = std::path::Path::new(&path);
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(e) => return SocketResponse::err(format!("Failed to read workflow: {}", e)),
            };

            let workflow = match crate::workflow::WorkflowParser::parse(&content) {
                Ok(w) => w,
                Err(e) => return SocketResponse::err(format!("Parse error: {}", e)),
            };

            let adapter = match core.get_page().await {
                Ok(a) => a,
                Err(e) => return SocketResponse::err(format!("Browser error: {}", e)),
            };

            let engine = crate::core::WorkflowEngine::new();
            let json_params: std::collections::HashMap<String, serde_json::Value> = params
                .into_iter()
                .map(|(k, v)| (k, serde_json::Value::String(v)))
                .collect();

            match engine.execute(&workflow, &adapter, json_params).await {
                Ok(result) => SocketResponse::ok_data(serde_json::json!({
                    "success": result.success,
                    "workflow_name": result.workflow_name,
                    "duration_ms": result.duration_ms,
                    "steps_executed": result.steps_executed,
                    "output": result.output,
                    "error": result.error,
                })),
                Err(e) => SocketResponse::err(e.to_string()),
            }
        }

        SocketRequest::WorkflowList => {
            let workflows_dir =
                std::env::var("AUTOMODUS_WORKFLOWS").unwrap_or_else(|_| "workflows".to_string());
            let mut names = Vec::new();
            if let Ok(entries) = glob::glob(&format!("{}/**/*.yaml", workflows_dir)) {
                for entry in entries.flatten() {
                    names.push(entry.display().to_string());
                }
            }
            SocketResponse::ok_data(serde_json::json!({"workflows": names}))
        }
    }
}

/// Start HTTP server (runs until shutdown signal)
async fn start_http_server(
    host: String,
    port: u16,
    mut shutdown_rx: broadcast::Receiver<()>,
    core: Arc<AppCore>,
) -> Result<(), DaemonError> {
    use crate::api;
    use crate::config::AppConfig;

    // Load configuration (use defaults if not found)
    let mut config = AppConfig::load().unwrap_or_default();
    config.server.host = host.clone();
    config.server.port = port;

    // Create server state backed by shared AppCore
    let state = api::create_state_with_core(config, core);

    // Load workflows
    let workflows_dir =
        std::env::var("AUTOMODUS_WORKFLOWS").unwrap_or_else(|_| "workflows".to_string());
    if let Err(e) = state.load_workflows(&workflows_dir).await {
        warn!("Failed to load workflows: {}", e);
    }

    // Create router
    let app = api::create_router(state);

    // Bind listener
    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| DaemonError::StartupFailed(format!("Failed to bind HTTP server: {}", e)))?;

    info!("HTTP server listening on http://{}", addr);

    // Graceful shutdown
    let shutdown_signal = async move {
        let _ = shutdown_rx.recv().await;
        info!("HTTP server shutting down...");
    };

    // Serve
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .map_err(|e| DaemonError::StartupFailed(format!("HTTP server error: {}", e)))?;

    Ok(())
}

/// Connect to running daemon via Unix socket
pub async fn connect_to_daemon(config: &DaemonConfig) -> Result<DaemonClient, DaemonError> {
    if !config.socket_path.exists() {
        return Err(DaemonError::NotRunning);
    }

    let stream = tokio::net::UnixStream::connect(&config.socket_path)
        .await
        .map_err(|e| DaemonError::ConnectionFailed(e.to_string()))?;

    Ok(DaemonClient::new(stream))
}

/// Client for communicating with the daemon via Unix socket.
///
/// All methods send a typed request and return the parsed response.
pub struct DaemonClient {
    reader: tokio::io::ReadHalf<tokio::net::UnixStream>,
    writer: tokio::io::WriteHalf<tokio::net::UnixStream>,
}

impl DaemonClient {
    fn new(stream: tokio::net::UnixStream) -> Self {
        let (reader, writer) = tokio::io::split(stream);
        Self { reader, writer }
    }

    /// Send a request and get a response
    async fn request(&mut self, req: SocketRequest) -> Result<SocketResponse, DaemonError> {
        protocol::write_message(&mut self.writer, &req)
            .await
            .map_err(|e| DaemonError::CommandFailed(format!("Send failed: {}", e)))?;

        protocol::read_message(&mut self.reader)
            .await
            .map_err(|e| DaemonError::CommandFailed(format!("Recv failed: {}", e)))
    }

    /// Convenience: extract response data or return error
    fn unwrap_response(resp: SocketResponse) -> Result<serde_json::Value, String> {
        if resp.ok {
            Ok(resp.data)
        } else {
            Err(resp.error.unwrap_or_else(|| "Unknown error".to_string()))
        }
    }

    // --- High-level API ---

    /// Ping the daemon
    pub async fn ping(&mut self) -> Result<(), DaemonError> {
        let resp = self.request(SocketRequest::Ping).await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    /// Get daemon status
    pub async fn status(&mut self) -> Result<serde_json::Value, DaemonError> {
        let resp = self.request(SocketRequest::Status).await?;
        Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)
    }

    // --- Sessions ---

    pub async fn session_create(
        &mut self,
        name: Option<String>,
        keep_alive: bool,
    ) -> Result<String, DaemonError> {
        let resp = self
            .request(SocketRequest::SessionCreate { name, keep_alive })
            .await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["id"].as_str().unwrap_or("").to_string())
    }

    pub async fn session_list(&mut self) -> Result<Vec<serde_json::Value>, DaemonError> {
        let resp = self.request(SocketRequest::SessionList).await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["sessions"]
            .as_array()
            .cloned()
            .unwrap_or_default())
    }

    pub async fn session_get(&mut self, id: &str) -> Result<serde_json::Value, DaemonError> {
        let resp = self
            .request(SocketRequest::SessionGet { id: id.to_string() })
            .await?;
        Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)
    }

    pub async fn session_find(&mut self, id_or_name: &str) -> Result<serde_json::Value, DaemonError> {
        let resp = self
            .request(SocketRequest::SessionFind {
                id_or_name: id_or_name.to_string(),
            })
            .await?;
        Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)
    }

    pub async fn session_close(&mut self, id: &str) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::SessionClose { id: id.to_string() })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn session_set_keep_alive(
        &mut self,
        id: &str,
        keep_alive: bool,
    ) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::SessionSetKeepAlive {
                id: id.to_string(),
                keep_alive,
            })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    // --- Browser ---

    pub async fn browser_goto(&mut self, url: &str) -> Result<String, DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserGoto {
                url: url.to_string(),
            })
            .await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["url"].as_str().unwrap_or("").to_string())
    }

    pub async fn browser_click(&mut self, selector: &str) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserClick {
                selector: selector.to_string(),
            })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_type(
        &mut self,
        selector: &str,
        text: &str,
    ) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserType {
                selector: selector.to_string(),
                text: text.to_string(),
            })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_wait(
        &mut self,
        selector: &str,
        timeout: Option<u64>,
    ) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserWait {
                selector: selector.to_string(),
                timeout,
            })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_screenshot(&mut self) -> Result<Vec<u8>, DaemonError> {
        use base64::Engine;
        let resp = self
            .request(SocketRequest::BrowserScreenshot { full_page: false })
            .await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        let b64 = data["png_base64"].as_str().unwrap_or("");
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| DaemonError::CommandFailed(format!("Invalid screenshot data: {}", e)))
    }

    pub async fn browser_eval(
        &mut self,
        script: &str,
    ) -> Result<serde_json::Value, DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserEval {
                script: script.to_string(),
            })
            .await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["result"].clone())
    }

    pub async fn browser_get_text(&mut self, selector: &str) -> Result<String, DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserGetText {
                selector: selector.to_string(),
            })
            .await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["text"].as_str().unwrap_or("").to_string())
    }

    pub async fn browser_get_url(&mut self) -> Result<String, DaemonError> {
        let resp = self.request(SocketRequest::BrowserGetUrl).await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["url"].as_str().unwrap_or("").to_string())
    }

    pub async fn browser_back(&mut self) -> Result<(), DaemonError> {
        let resp = self.request(SocketRequest::BrowserBack).await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_forward(&mut self) -> Result<(), DaemonError> {
        let resp = self.request(SocketRequest::BrowserForward).await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_reload(&mut self) -> Result<(), DaemonError> {
        let resp = self.request(SocketRequest::BrowserReload).await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_highlight(&mut self, selector: &str) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserHighlight {
                selector: selector.to_string(),
            })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_find(&mut self, selector: &str) -> Result<serde_json::Value, DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserFind {
                selector: selector.to_string(),
            })
            .await?;
        Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)
    }

    // --- Tabs ---

    pub async fn browser_tab_list(&mut self) -> Result<Vec<serde_json::Value>, DaemonError> {
        let resp = self.request(SocketRequest::BrowserTabList).await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["tabs"].as_array().cloned().unwrap_or_default())
    }

    pub async fn browser_tab_new(&mut self, url: Option<&str>) -> Result<usize, DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserTabNew {
                url: url.map(String::from),
            })
            .await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["index"].as_u64().unwrap_or(0) as usize)
    }

    pub async fn browser_tab_switch(&mut self, index: usize) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserTabSwitch { index })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    pub async fn browser_tab_close(&mut self, index: usize) -> Result<(), DaemonError> {
        let resp = self
            .request(SocketRequest::BrowserTabClose { index })
            .await?;
        Self::unwrap_response(resp).map(|_| ()).map_err(DaemonError::CommandFailed)
    }

    // --- PDF ---

    pub async fn browser_pdf(&mut self) -> Result<Vec<u8>, DaemonError> {
        use base64::Engine;
        let resp = self.request(SocketRequest::BrowserPdf).await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        let b64 = data["pdf_base64"].as_str().unwrap_or("");
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| DaemonError::CommandFailed(format!("Invalid PDF data: {}", e)))
    }

    // --- Debug ---

    pub async fn debug_clean(&mut self) -> Result<serde_json::Value, DaemonError> {
        let resp = self.request(SocketRequest::DebugClean).await?;
        Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)
    }

    // --- Workflows ---

    pub async fn workflow_run(
        &mut self,
        path: &str,
        params: std::collections::HashMap<String, String>,
    ) -> Result<serde_json::Value, DaemonError> {
        let resp = self
            .request(SocketRequest::WorkflowRun {
                path: path.to_string(),
                params,
            })
            .await?;
        Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)
    }

    pub async fn workflow_list(&mut self) -> Result<Vec<String>, DaemonError> {
        let resp = self.request(SocketRequest::WorkflowList).await?;
        let data = Self::unwrap_response(resp).map_err(DaemonError::CommandFailed)?;
        Ok(data["workflows"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default())
    }
}
