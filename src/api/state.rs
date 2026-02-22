//! Server State
//!
//! Shared state for the API server.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};

use crate::actions::BrowserHandle;
use crate::config::AppConfig;
use crate::core::WorkflowEngine;
use crate::modules::ChromePageAdapter;
use crate::workflow::{Workflow, WorkflowParser};
use crate::api::schemas::ExecutionStatus;

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
    /// Workflow execution engine
    pub engine: WorkflowEngine,
    /// Loaded workflows (name -> Workflow)
    pub workflows: RwLock<HashMap<String, Workflow>>,
    /// Browser instance (lazily initialized)
    pub browser: Mutex<Option<chromiumoxide::browser::Browser>>,
    /// Current page adapter
    pub page_adapter: Mutex<Option<ChromePageAdapter>>,
    /// Configuration
    pub config: AppConfig,
    /// Active sessions (id -> Session)
    pub sessions: RwLock<HashMap<String, ServerSession>>,
    /// Maximum concurrent sessions
    pub max_sessions: usize,
    /// Execution history (id -> Execution)
    pub executions: RwLock<HashMap<String, ServerExecution>>,
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
    /// Create a new server state
    pub fn new(config: AppConfig) -> Self {
        Self {
            engine: WorkflowEngine::new(),
            workflows: RwLock::new(HashMap::new()),
            browser: Mutex::new(None),
            page_adapter: Mutex::new(None),
            config,
            sessions: RwLock::new(HashMap::new()),
            max_sessions: 10, // Default max sessions
            executions: RwLock::new(HashMap::new()),
        }
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

    /// Get or create browser and page
    pub async fn get_page(&self) -> Result<ChromePageAdapter, String> {
        use chromiumoxide::browser::{Browser, BrowserConfig};
        use futures_util::stream::StreamExt;

        let mut browser_guard = self.browser.lock().await;
        let mut page_guard = self.page_adapter.lock().await;

        // Check if we have a valid page
        if let Some(ref adapter) = *page_guard {
            // Try to verify it's still valid
            if adapter.current_url().await.is_ok() {
                return Ok(adapter.clone());
            }
        }

        // Need to create browser
        if browser_guard.is_none() {
            info!("Launching browser...");

            let data_dir = std::env::temp_dir().join("automodus-server");
            let _ = std::fs::create_dir_all(&data_dir);

            let headless = self.config.browser.headless;
            let mut config_builder = BrowserConfig::builder();

            if !headless {
                config_builder = config_builder.with_head();
            }

            config_builder = config_builder
                .arg("--no-sandbox")
                .arg("--disable-setuid-sandbox")
                .arg("--disable-dev-shm-usage")
                .arg("--disable-web-security")
                .arg("--disable-extensions")
                .arg("--disable-gpu")
                .arg("--no-first-run")
                .arg("--disable-session-crashed-bubble")
                .arg(format!("--user-data-dir={}", data_dir.display()));

            let browser_config = config_builder
                .build()
                .map_err(|e| format!("Failed to build browser config: {}", e))?;

            let (browser, mut handler) = Browser::launch(browser_config)
                .await
                .map_err(|e| format!("Failed to launch browser: {}", e))?;

            // Spawn handler task
            tokio::spawn(async move {
                while let Some(h) = handler.next().await {
                    if let Err(e) = h {
                        if e.to_string().contains("connection closed") {
                            break;
                        }
                    }
                }
            });

            *browser_guard = Some(browser);
            info!("Browser launched");
        }

        // Get or create page
        let browser = browser_guard.as_ref().unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

        let pages = browser
            .pages()
            .await
            .map_err(|e| format!("Failed to get pages: {}", e))?;

        let page = if pages.is_empty() {
            browser
                .new_page("about:blank")
                .await
                .map_err(|e| format!("Failed to create page: {}", e))?
        } else {
            pages.into_iter().next().unwrap()
        };

        let adapter = ChromePageAdapter::new(page);
        *page_guard = Some(adapter.clone());

        Ok(adapter)
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

        info!("Started execution: {} for workflow: {}", id, workflow);
        id
    }

    /// Update execution progress
    pub async fn update_execution_progress(&self, id: &str, steps_executed: usize) {
        if let Some(execution) = self.executions.write().await.get_mut(id) {
            execution.steps_executed = steps_executed;
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
            execution.error = error;
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
