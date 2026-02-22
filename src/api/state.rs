//! Server State
//!
//! Shared state for the API server.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::config::AppConfig;
use crate::core::{AppCore, WorkflowEngine};
use crate::modules::ChromePageAdapter;
use crate::workflow::{Workflow, WorkflowParser};
use crate::api::schemas::ExecutionStatus;
use crate::api::ws::ServerEvent;

/// Maximum executions to keep in history
const MAX_EXECUTION_HISTORY: usize = 100;

/// Browser session managed by the server
#[derive(Debug, Clone)]
pub struct ServerSession {
    /// Unique session ID
    pub id: String,
    /// Optional friendly name
    pub name: Option<String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last activity timestamp
    pub last_activity: chrono::DateTime<chrono::Utc>,
    /// Keep browser alive between workflows
    pub keep_alive: bool,
}

impl ServerSession {
    /// Create a new session
    pub fn new(name: Option<String>, keep_alive: bool) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            created_at: now,
            last_activity: now,
            keep_alive,
        }
    }

    /// Update last activity timestamp
    pub fn touch(&mut self) {
        self.last_activity = chrono::Utc::now();
    }
}

/// Shared state for the API server
pub struct ServerState {
    /// Application core (owns browser lifecycle)
    pub core: Arc<AppCore>,
    /// Workflow execution engine
    pub engine: WorkflowEngine,
    /// Loaded workflows (name -> Workflow)
    pub workflows: RwLock<HashMap<String, Workflow>>,
    /// Configuration
    pub config: AppConfig,
    /// Active sessions (id -> Session)
    pub sessions: RwLock<HashMap<String, ServerSession>>,
    /// Maximum concurrent sessions
    pub max_sessions: usize,
    /// Execution history (id -> Execution)
    pub executions: RwLock<HashMap<String, ServerExecution>>,
    /// Event broadcaster for WebSocket clients
    event_tx: tokio::sync::broadcast::Sender<ServerEvent>,
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
        Self::with_core(config, Arc::new(core))
    }

    /// Create a new server state with a shared AppCore
    pub fn with_core(config: AppConfig, core: Arc<AppCore>) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(256);

        Self {
            core,
            engine: WorkflowEngine::new(),
            workflows: RwLock::new(HashMap::new()),
            config,
            sessions: RwLock::new(HashMap::new()),
            max_sessions: 10,
            executions: RwLock::new(HashMap::new()),
            event_tx,
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
        for entry in glob::glob(&pattern)? {
            if let Ok(path) = entry {
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
        }

        Ok(workflows.len())
    }

    /// Get or create browser and page (delegates to AppCore)
    pub async fn get_page(&self) -> Result<ChromePageAdapter, String> {
        self.core.get_page().await
    }

    // --- Session Management ---

    /// Create a new session
    pub async fn create_session(
        &self,
        name: Option<String>,
        keep_alive: bool,
    ) -> Result<ServerSession, String> {
        let mut sessions = self.sessions.write().await;

        if sessions.len() >= self.max_sessions {
            return Err("Maximum sessions reached".to_string());
        }

        let session = ServerSession::new(name, keep_alive);
        let id = session.id.clone();
        sessions.insert(id.clone(), session.clone());

        // Broadcast event
        self.broadcast_event(ServerEvent::SessionCreated { id: id.clone() });

        info!("Created session: {}", id);
        Ok(session)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Option<ServerSession> {
        self.sessions.read().await.get(id).cloned()
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Vec<ServerSession> {
        self.sessions.read().await.values().cloned().collect()
    }

    /// Close a session by ID
    pub async fn close_session(&self, id: &str) -> Result<(), String> {
        let mut sessions = self.sessions.write().await;

        if sessions.remove(id).is_none() {
            return Err(format!("Session '{}' not found", id));
        }

        // Broadcast event
        self.broadcast_event(ServerEvent::SessionClosed { id: id.to_string() });

        info!("Closed session: {}", id);
        Ok(())
    }

    /// Touch a session to update last activity
    pub async fn touch_session(&self, id: &str) {
        if let Some(session) = self.sessions.write().await.get_mut(id) {
            session.touch();
        }
    }

    // --- Execution Tracking ---

    /// Start tracking a new execution
    pub async fn start_execution(
        &self,
        workflow: &str,
        total_steps: usize,
        params: HashMap<String, serde_json::Value>,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
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

            for (old_id, _) in completed.iter().take(executions.len() - MAX_EXECUTION_HISTORY) {
                executions.remove(old_id);
            }
        }

        // Broadcast event
        self.broadcast_event(ServerEvent::ExecutionStarted {
            id: id.clone(),
            workflow: workflow.to_string(),
        });

        info!("Started execution: {} for workflow: {}", id, workflow);
        id
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
        list.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        if let Some(limit) = limit {
            list.truncate(limit);
        }
        list
    }

    /// Cancel an execution
    pub async fn cancel_execution(&self, id: &str) -> Result<(), String> {
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
}

/// Create a shared server state wrapped in Arc
pub fn create_state(config: AppConfig) -> Arc<ServerState> {
    Arc::new(ServerState::new(config))
}

/// Create a shared server state backed by a shared AppCore
pub fn create_state_with_core(config: AppConfig, core: Arc<AppCore>) -> Arc<ServerState> {
    Arc::new(ServerState::with_core(config, core))
}
