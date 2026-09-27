//! Server State
//!
//! Shared state for the API server. Session management is delegated to AppCore
//! to maintain a single source of truth.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::api::schemas::ExecutionStatus;
use crate::api::ws::ServerEvent;
use crate::config::AppConfig;
use crate::core::engine::PauseResponse;
use crate::core::{AppCore, CoreEvent, Session, SessionInfo as CoreSessionInfo, WorkflowEngine};
use crate::modules::SessionAdapter;
use crate::workflow::{Workflow, WorkflowParser};

/// Maximum executions to keep in history
const MAX_EXECUTION_HISTORY: usize = 100;

/// Shared state for the API server
pub struct ServerState {
    /// Application core (owns browser lifecycle + sessions)
    pub core: Arc<AppCore>,
    /// Workflow execution engine
    pub engine: WorkflowEngine,
    /// Loaded workflows (name -> Workflow)
    pub workflows: RwLock<HashMap<String, Workflow>>,
    /// Configuration
    pub config: AppConfig,
    /// Execution history (id -> Execution)
    pub executions: RwLock<HashMap<String, ServerExecution>>,
    /// Event broadcaster for WebSocket clients
    event_tx: tokio::sync::broadcast::Sender<ServerEvent>,
    /// Cancellation tokens for running executions (exec_id -> token)
    cancel_tokens: RwLock<HashMap<String, CancellationToken>>,
    /// Pending pause signals waiting for WS/API response (exec_id -> sender)
    pending_pauses:
        tokio::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<PauseResponse>>>,
}

/// Workflow execution record
#[derive(Debug, Clone)]
pub struct ServerExecution {
    /// Unique execution ID
    pub id: String,
    /// Workflow name
    pub workflow: String,
    /// Status
    pub status: ExecutionStatus,
    /// Started timestamp
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Completed timestamp
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Duration in ms
    pub duration_ms: Option<i64>,
    /// Steps executed
    pub steps_executed: usize,
    /// Total steps in workflow
    pub total_steps: usize,
    /// Parameters used
    pub params: HashMap<String, serde_json::Value>,
    /// Output data
    pub output: serde_json::Value,
    /// Error message if failed
    pub error: Option<String>,
}

impl ServerState {
    /// Create a new server state with a default AppCore
    pub fn new(config: AppConfig) -> Self {
        let daemon_config = crate::daemon::DaemonConfig::default();
        let mut core = AppCore::new(&daemon_config);
        core.set_headless(config.browser.headless);
        core.set_engine(config.browser.engine);
        Self::with_core(config, Arc::new(core))
    }

    /// Create a new server state with a shared AppCore
    pub fn with_core(config: AppConfig, core: Arc<AppCore>) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        let workflows_dir =
            std::env::var("AUTOMODUS_WORKFLOWS").unwrap_or_else(|_| "workflows".to_string());
        let loader = Arc::new(crate::workflow::WorkflowLoader::new(&workflows_dir));

        // Bridge CoreEvent → ServerEvent so WebSocket clients receive daemon events
        let mut core_rx = core.subscribe();
        let bridge_tx = event_tx.clone();
        tokio::spawn(async move {
            loop {
                match core_rx.recv().await {
                    Ok(event) => {
                        if let Some(server_event) = convert_core_event(event) {
                            let _ = bridge_tx.send(server_event);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("CoreEvent bridge lagged, missed {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        Self {
            core,
            engine: WorkflowEngine::with_resolver(loader),
            workflows: RwLock::new(HashMap::new()),
            config,
            executions: RwLock::new(HashMap::new()),
            event_tx,
            cancel_tokens: RwLock::new(HashMap::new()),
            pending_pauses: tokio::sync::Mutex::new(HashMap::new()),
        }
    }

    /// Subscribe to server events
    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<ServerEvent> {
        self.event_tx.subscribe()
    }

    /// Broadcast an event to all WebSocket clients
    pub fn broadcast_event(&self, event: ServerEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Load workflows from directory
    pub async fn load_workflows(
        &self,
        dir: &str,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        let mut workflows = self.workflows.write().await;
        workflows.clear();

        let pattern = format!("{}/**/*.yaml", dir);
        for path in glob::glob(&pattern)?.flatten() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match WorkflowParser::parse(&content) {
                    Ok(workflow) => {
                        info!("Loaded workflow: {} from {}", workflow.name, path.display());
                        workflows.insert(workflow.name.clone(), workflow);
                    }
                    Err(e) => {
                        warn!("Failed to parse {}: {}", path.display(), e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read {}: {}", path.display(), e);
                }
            }
        }

        Ok(workflows.len())
    }

    /// Get or create browser and page (delegates to AppCore)
    pub async fn get_page(&self) -> Result<SessionAdapter, String> {
        self.core.get_page().await
    }

    /// Get the debug output directory
    pub fn debug_dir(&self) -> std::path::PathBuf {
        self.core.debug_dir().clone()
    }

    // --- Session Management (delegated to AppCore) ---

    /// Create a new session
    pub async fn create_session(
        &self,
        name: Option<String>,
        keep_alive: bool,
    ) -> Result<String, String> {
        let id = self
            .core
            .create_session(name)
            .await
            .map_err(|e| e.to_string())?;

        // AppCore defaults keep_alive to true; set to false if requested
        if !keep_alive {
            let _ = self.core.set_session_keep_alive(&id, false).await;
        }

        // Broadcast to WebSocket clients
        self.broadcast_event(ServerEvent::SessionCreated { id: id.clone() });

        Ok(id)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Option<Session> {
        self.core.get_session(id).await
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Vec<CoreSessionInfo> {
        self.core.list_sessions().await
    }

    /// Close a session by ID
    pub async fn close_session(&self, id: &str) -> Result<(), String> {
        self.core
            .close_session(id)
            .await
            .map_err(|e| e.to_string())?;

        // Broadcast to WebSocket clients
        self.broadcast_event(ServerEvent::SessionClosed { id: id.to_string() });

        Ok(())
    }

    /// Touch a session to update last activity
    pub async fn touch_session(&self, id: &str) {
        self.core.touch_session(id).await;
    }

    // --- Execution Tracking ---

    /// Start tracking a new execution, returns (exec_id, CancellationToken)
    pub async fn start_execution(
        &self,
        workflow: &str,
        total_steps: usize,
        params: HashMap<String, serde_json::Value>,
    ) -> (String, CancellationToken) {
        let id = uuid::Uuid::new_v4().to_string();
        let token = CancellationToken::new();
        let execution = ServerExecution {
            id: id.clone(),
            workflow: workflow.to_string(),
            status: ExecutionStatus::Running,
            started_at: chrono::Utc::now(),
            completed_at: None,
            duration_ms: None,
            steps_executed: 0,
            total_steps,
            params,
            output: serde_json::Value::Null,
            error: None,
        };

        let mut executions = self.executions.write().await;
        executions.insert(id.clone(), execution);

        // Trim old executions if needed
        if executions.len() > MAX_EXECUTION_HISTORY {
            // Remove oldest completed executions
            let mut completed: Vec<_> = executions
                .iter()
                .filter(|(_, e)| e.status != ExecutionStatus::Running)
                .map(|(id, e)| (id.clone(), e.started_at))
                .collect();
            completed.sort_by_key(|(_, t)| *t);

            for (old_id, _) in completed
                .iter()
                .take(executions.len() - MAX_EXECUTION_HISTORY)
            {
                executions.remove(old_id);
            }
        }

        // Broadcast event
        self.broadcast_event(ServerEvent::ExecutionStarted {
            id: id.clone(),
            workflow: workflow.to_string(),
        });

        // Store cancellation token
        self.cancel_tokens
            .write()
            .await
            .insert(id.clone(), token.clone());

        info!("Started execution: {} for workflow: {}", id, workflow);
        (id, token)
    }

    /// Update execution progress
    pub async fn update_execution_progress(&self, id: &str, steps_executed: usize, action: &str) {
        if let Some(execution) = self.executions.write().await.get_mut(id) {
            execution.steps_executed = steps_executed;

            // Broadcast step event
            self.broadcast_event(ServerEvent::ExecutionStep {
                id: id.to_string(),
                step: steps_executed,
                action: action.to_string(),
            });
        }
    }

    /// Complete an execution
    pub async fn complete_execution(
        &self,
        id: &str,
        success: bool,
        output: serde_json::Value,
        error: Option<String>,
    ) {
        // Clean up cancellation token
        self.cancel_tokens.write().await.remove(id);
        // Clean up any pending pause
        self.pending_pauses.lock().await.remove(id);

        if let Some(execution) = self.executions.write().await.get_mut(id) {
            let now = chrono::Utc::now();
            execution.completed_at = Some(now);
            execution.duration_ms = Some((now - execution.started_at).num_milliseconds());
            execution.status = if success {
                ExecutionStatus::Completed
            } else {
                ExecutionStatus::Failed
            };
            execution.output = output;
            execution.error = error.clone();

            // Broadcast event
            if success {
                self.broadcast_event(ServerEvent::ExecutionComplete {
                    id: id.to_string(),
                    success: true,
                });
            } else {
                self.broadcast_event(ServerEvent::ExecutionError {
                    id: id.to_string(),
                    error: error.unwrap_or_else(|| "Unknown error".to_string()),
                });
            }

            info!("Completed execution: {} success={}", id, success);
        }
    }

    /// Get execution by ID
    pub async fn get_execution(&self, id: &str) -> Option<ServerExecution> {
        self.executions.read().await.get(id).cloned()
    }

    /// List all executions (most recent first)
    pub async fn list_executions(&self, limit: Option<usize>) -> Vec<ServerExecution> {
        let executions = self.executions.read().await;
        let mut list: Vec<_> = executions.values().cloned().collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.started_at));
        if let Some(limit) = limit {
            list.truncate(limit);
        }
        list
    }

    /// Cancel an execution
    pub async fn cancel_execution(&self, id: &str) -> Result<(), String> {
        // Cancel the token to signal the engine to stop
        if let Some(token) = self.cancel_tokens.write().await.remove(id) {
            token.cancel();
        }

        if let Some(execution) = self.executions.write().await.get_mut(id) {
            if execution.status == ExecutionStatus::Running {
                execution.status = ExecutionStatus::Cancelled;
                execution.completed_at = Some(chrono::Utc::now());
                execution.error = Some("Cancelled by user".to_string());
                info!("Cancelled execution: {}", id);
                return Ok(());
            }
            return Err("Execution is not running".to_string());
        }
        Err(format!("Execution '{}' not found", id))
    }

    // --- Pause Signaling ---

    /// Register a pending pause for an execution. Returns a receiver to await the response.
    pub async fn register_pause(&self, id: &str) -> tokio::sync::oneshot::Receiver<PauseResponse> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_pauses.lock().await.insert(id.to_string(), tx);
        rx
    }

    /// Resolve a pending pause by sending a response. Returns Err if no pause is pending.
    pub async fn resolve_pause(&self, id: &str, response: PauseResponse) -> Result<(), String> {
        if let Some(tx) = self.pending_pauses.lock().await.remove(id) {
            tx.send(response)
                .map_err(|_| "Pause receiver dropped".to_string())
        } else {
            Err(format!("No pending pause for execution '{}'", id))
        }
    }
}

/// Create a shared server state wrapped in Arc
pub fn create_state(config: AppConfig) -> Arc<ServerState> {
    Arc::new(ServerState::new(config))
}

/// Create a shared server state backed by a shared AppCore
pub fn create_state_with_core(config: AppConfig, core: Arc<AppCore>) -> Arc<ServerState> {
    Arc::new(ServerState::with_core(config, core))
}

// ============================================================================
// WebSocketPauseHandler
// ============================================================================

use crate::core::engine::PauseHandler;

/// Pause handler that broadcasts pause events to WebSocket clients and waits for a response.
pub struct WebSocketPauseHandler {
    exec_id: String,
    state: Arc<ServerState>,
}

impl WebSocketPauseHandler {
    /// Create a new handler for a given execution.
    pub fn new(exec_id: String, state: Arc<ServerState>) -> Self {
        Self { exec_id, state }
    }
}

#[async_trait::async_trait]
impl PauseHandler for WebSocketPauseHandler {
    async fn on_pause(
        &self,
        _workflow: &str,
        step: usize,
        _action: &str,
        _selector: Option<&str>,
    ) -> PauseResponse {
        // Register a oneshot channel for the response
        let rx = self.state.register_pause(&self.exec_id).await;

        // Broadcast the paused event so WS clients know we're waiting
        self.state.broadcast_event(ServerEvent::ExecutionPaused {
            id: self.exec_id.clone(),
            step,
        });

        info!(exec_id = %self.exec_id, step, "Execution paused, waiting for WS signal");

        // Wait for continue/skip/abort from a WS client (or channel drop = continue)
        match rx.await {
            Ok(response) => response,
            Err(_) => {
                // Sender dropped (e.g. execution cancelled) — default to continue
                PauseResponse::Continue
            }
        }
    }
}

/// Convert a CoreEvent to a ServerEvent, returning None for events without a WS equivalent.
fn convert_core_event(event: CoreEvent) -> Option<ServerEvent> {
    match event {
        CoreEvent::SessionCreated { id } => Some(ServerEvent::SessionCreated { id }),
        CoreEvent::SessionClosed { id } => Some(ServerEvent::SessionClosed { id }),
        CoreEvent::ExecutionStarted { id, workflow } => {
            Some(ServerEvent::ExecutionStarted { id, workflow })
        }
        CoreEvent::ExecutionStep { id, step, action } => {
            Some(ServerEvent::ExecutionStep { id, step, action })
        }
        CoreEvent::ExecutionComplete { id, success } => {
            Some(ServerEvent::ExecutionComplete { id, success })
        }
        CoreEvent::ExecutionError { id, error } => Some(ServerEvent::ExecutionError { id, error }),
        // WorkflowLoaded, WorkflowUnloaded, DebugConfigChanged have no WS equivalent
        _ => None,
    }
}
