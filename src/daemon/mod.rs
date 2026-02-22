//! Daemon Module
//!
//! Long-running background process that owns browser sessions and workflow execution.
//! Implements Docker-style daemon/client separation.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::UnixListener;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::core::AppCore;

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
        Self {
            core: Arc::new(AppCore::new(&config)),
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

            tokio::spawn(async move {
                match start_http_server(http_host, http_port, http_shutdown_rx).await {
                    Ok(_) => info!("HTTP server stopped"),
                    Err(e) => error!("HTTP server error: {}", e),
                }
            });

            info!(
                "HTTP server running on http://{}:{}",
                self.config.http_host, self.config.http_port
            );
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
    _stream: tokio::net::UnixStream,
    _core: Arc<AppCore>,
) -> Result<(), DaemonError> {
    // TODO: Implement socket protocol
    // For now, this is a placeholder
    Ok(())
}

/// Start HTTP server (runs until shutdown signal)
async fn start_http_server(
    host: String,
    port: u16,
    mut shutdown_rx: broadcast::Receiver<()>,
) -> Result<(), DaemonError> {
    use crate::api;
    use crate::config::AppConfig;

    // Load configuration (use defaults if not found)
    let mut config = AppConfig::load().unwrap_or_default();
    config.server.host = host.clone();
    config.server.port = port;

    // Create server state
    let state = api::create_state(config);

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

    Ok(DaemonClient { stream })
}

/// Client for communicating with the daemon
pub struct DaemonClient {
    #[allow(dead_code)]
    stream: tokio::net::UnixStream,
}

impl DaemonClient {
    /// Send a command to the daemon
    pub async fn send_command(&mut self, _cmd: &str) -> Result<String, DaemonError> {
        // TODO: Implement protocol
        Ok("OK".to_string())
    }
}
