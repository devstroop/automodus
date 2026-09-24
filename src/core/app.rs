//! Application Core
//!
//! Shared application state owned by the daemon, providing access to:
//! - Workflow engine
//! - Session manager
//! - Configuration
//! - Event broadcasting

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{info, warn};

use crate::actions::BrowserHandle;
use crate::modules::browser::launch::{launch_session, LaunchOptions};
use crate::modules::SessionAdapter;
use crate::workflow::schema::{DebugConfig, ResolvedDebugConfig, Workflow};

/// Application core - shared state for all daemon operations
pub struct AppCore {
    /// Current session adapter (lazily launched; backends stay inside modules/browser)
    page_adapter: Mutex<Option<SessionAdapter>>,
    /// Whether to run browser headless
    headless: bool,
    /// Browser engine backend (chromium or firefox; see launch_session)
    engine: crate::config::BrowserEngine,
    /// Session manager for browser lifecycle
    sessions: Arc<RwLock<SessionStore>>,
    /// Global debug configuration
    debug_config: Arc<RwLock<ResolvedDebugConfig>>,
    /// Loaded workflow cache
    workflows: Arc<RwLock<HashMap<String, Workflow>>>,
    /// Event broadcaster for WebSocket clients
    event_tx: broadcast::Sender<CoreEvent>,
    /// Debug output directory
    debug_dir: PathBuf,
    /// Maximum concurrent sessions
    max_sessions: usize,
    /// Session idle timeout in seconds (0 = no timeout)
    session_idle_timeout: u64,
}

impl AppCore {
    /// Create a new application core
    pub fn new(config: &crate::daemon::DaemonConfig) -> Self {
        let (event_tx, _) = broadcast::channel(256);

        Self {
            page_adapter: Mutex::new(None),
            headless: true,
            engine: crate::config::BrowserEngine::default(),
            sessions: Arc::new(RwLock::new(SessionStore::new())),
            debug_config: Arc::new(RwLock::new(ResolvedDebugConfig::default())),
            workflows: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            debug_dir: config.debug_dir.clone(),
            max_sessions: config.max_sessions,
            session_idle_timeout: 300, // Default: 5 minutes
        }
    }

    /// Subscribe to core events
    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.event_tx.subscribe()
    }

    /// Broadcast an event to all subscribers
    pub fn broadcast(&self, event: CoreEvent) {
        let _ = self.event_tx.send(event);
    }

    /// Get the debug directory
    pub fn debug_dir(&self) -> &PathBuf {
        &self.debug_dir
    }

    /// Get max sessions limit
    pub fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    /// Set headless mode
    pub fn set_headless(&mut self, headless: bool) {
        self.headless = headless;
    }

    /// Set browser engine backend
    pub fn set_engine(&mut self, engine: crate::config::BrowserEngine) {
        self.engine = engine;
    }

    // --- Browser Lifecycle ---

    /// Get or create browser page adapter
    ///
    /// Lazily launches the browser on first call. Returns an existing valid
    /// adapter if available. Chromiumoxide types never leave `modules/browser`.
    ///
    /// Single-flight: the adapter lock is held across check → close → launch →
    /// store so concurrent callers cannot launch duplicate Chromium processes
    /// on the same profile directory. A stale adapter is closed before
    /// relaunch so the next launch does not contend on a live profile lock.
    pub async fn get_page(&self) -> Result<SessionAdapter, String> {
        let mut guard = self.page_adapter.lock().await;

        if let Some(adapter) = guard.take() {
            if adapter.current_url().await.is_ok() {
                *guard = Some(adapter.clone());
                return Ok(adapter);
            }
            warn!("AppCore: cached browser adapter is stale; closing and relaunching");
            adapter.close_browser().await;
        }

        info!("AppCore: launching browser...");
        let options = LaunchOptions::for_server(self.headless).engine(self.engine);
        let adapter = launch_session(&options).await?;

        if let Err(e) = adapter.start_console_listener().await {
            warn!("Failed to start console listener: {}", e);
        }
        if let Err(e) = adapter.start_network_listener().await {
            warn!("Failed to start network listener: {}", e);
        }
        if let Err(e) = adapter.start_crash_listener().await {
            warn!("Failed to start crash listener: {}", e);
        }
        *guard = Some(adapter.clone());

        Ok(adapter)
    }

    /// Check if browser is currently running
    pub async fn has_browser(&self) -> bool {
        self.page_adapter.lock().await.is_some()
    }

    // --- Debug Configuration ---

    /// Get current global debug configuration
    pub async fn get_debug_config(&self) -> ResolvedDebugConfig {
        self.debug_config.read().await.clone()
    }

    /// Update global debug configuration
    pub async fn set_debug_config(&self, config: ResolvedDebugConfig) {
        *self.debug_config.write().await = config;
        info!("Global debug config updated");
    }

    /// Merge debug config with global settings
    pub async fn resolve_debug_config(&self, workflow_config: Option<DebugConfig>) -> ResolvedDebugConfig {
        let global = self.debug_config.read().await.clone();
        
        match workflow_config {
            Some(wf) => {
                // Workflow config takes precedence for provided values
                let resolved = wf.resolve();
                ResolvedDebugConfig {
                    enabled: resolved.enabled,
                    level: resolved.level,
                    capture: resolved.capture,
                    highlight: resolved.highlight,
                    delay: resolved.delay,
                    pause: resolved.pause,
                    console: resolved.console,
                    network: resolved.network,
                }
            }
            None => global,
        }
    }

    // --- Workflow Cache ---

    /// Load a workflow into cache
    pub async fn load_workflow(&self, name: String, workflow: Workflow) {
        self.workflows.write().await.insert(name, workflow);
    }

    /// Get a cached workflow
    pub async fn get_workflow(&self, name: &str) -> Option<Workflow> {
        self.workflows.read().await.get(name).cloned()
    }

    /// Remove a workflow from cache
    pub async fn unload_workflow(&self, name: &str) -> Option<Workflow> {
        self.workflows.write().await.remove(name)
    }

    /// List all cached workflows
    pub async fn list_workflows(&self) -> Vec<String> {
        self.workflows.read().await.keys().cloned().collect()
    }

    // --- Session Management ---

    /// Create a new browser session
    pub async fn create_session(&self, name: Option<String>) -> Result<String, SessionError> {
        let mut sessions = self.sessions.write().await;
        
        if sessions.count() >= self.max_sessions {
            return Err(SessionError::MaxSessionsReached);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let session = Session {
            id: id.clone(),
            name,
            created_at: chrono::Utc::now(),
            last_activity: chrono::Utc::now(),
            keep_alive: true,
        };

        sessions.insert(session);

        self.broadcast(CoreEvent::SessionCreated { id: id.clone() });
        
        Ok(id)
    }

    /// Get a session by ID
    pub async fn get_session(&self, id: &str) -> Option<Session> {
        self.sessions.read().await.get(id)
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions.read().await.list()
    }

    /// Close a specific session
    pub async fn close_session(&self, id: &str) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        
        if sessions.remove(id).is_none() {
            return Err(SessionError::NotFound);
        }

        self.broadcast(CoreEvent::SessionClosed { id: id.to_string() });
        
        Ok(())
    }

    /// Close all sessions
    pub async fn close_all_sessions(&self) {
        let mut sessions = self.sessions.write().await;
        let ids: Vec<String> = sessions.list().into_iter().map(|s| s.id).collect();
        
        for id in ids {
            sessions.remove(&id);
            self.broadcast(CoreEvent::SessionClosed { id });
        }
    }

    /// Update session activity timestamp
    pub async fn touch_session(&self, id: &str) {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.last_activity = chrono::Utc::now();
        }
    }

    /// Find a session by name (returns first match)
    pub async fn find_session_by_name(&self, name: &str) -> Option<Session> {
        self.sessions.read().await.find_by_name(name)
    }

    /// Find a session by ID or name
    pub async fn find_session(&self, id_or_name: &str) -> Option<Session> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(id_or_name) {
            return Some(session);
        }
        sessions.find_by_name(id_or_name)
    }

    /// Set keep_alive on a session
    pub async fn set_session_keep_alive(&self, id: &str, keep_alive: bool) -> Result<(), SessionError> {
        let mut sessions = self.sessions.write().await;
        match sessions.get_mut(id) {
            Some(session) => {
                session.keep_alive = keep_alive;
                Ok(())
            }
            None => Err(SessionError::NotFound),
        }
    }

    /// Set session idle timeout in seconds (0 = no timeout)
    pub fn set_session_idle_timeout(&mut self, seconds: u64) {
        self.session_idle_timeout = seconds;
    }

    /// Get session idle timeout in seconds
    pub fn session_idle_timeout(&self) -> u64 {
        self.session_idle_timeout
    }

    /// Clean up sessions that have been idle longer than the timeout.
    ///
    /// Sessions with `keep_alive: true` are never cleaned up.
    /// Returns the IDs of sessions that were removed.
    pub async fn cleanup_idle_sessions(&self) -> Vec<String> {
        if self.session_idle_timeout == 0 {
            return vec![];
        }

        let now = chrono::Utc::now();
        let timeout = chrono::Duration::seconds(self.session_idle_timeout as i64);
        let mut removed = vec![];

        let mut sessions = self.sessions.write().await;
        let idle_ids: Vec<String> = sessions
            .list()
            .into_iter()
            .filter(|s| !s.keep_alive && (now - s.last_activity) > timeout)
            .map(|s| s.id)
            .collect();

        for id in idle_ids {
            sessions.remove(&id);
            self.broadcast(CoreEvent::SessionClosed { id: id.clone() });
            info!("Cleaned up idle session: {}", id);
            removed.push(id);
        }

        removed
    }
}

/// Events emitted by the application core
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// Session created
    SessionCreated { id: String },
    /// Session closed
    SessionClosed { id: String },
    /// Workflow loaded
    WorkflowLoaded { name: String },
    /// Workflow unloaded
    WorkflowUnloaded { name: String },
    /// Debug config changed
    DebugConfigChanged,
    /// Execution started
    ExecutionStarted { id: String, workflow: String },
    /// Execution step
    ExecutionStep { id: String, step: usize, action: String },
    /// Execution complete
    ExecutionComplete { id: String, success: bool },
    /// Execution error
    ExecutionError { id: String, error: String },
}

/// Browser session
#[derive(Debug, Clone)]
pub struct Session {
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

/// Session information for listing
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub name: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_activity: chrono::DateTime<chrono::Utc>,
    pub keep_alive: bool,
}

impl From<&Session> for SessionInfo {
    fn from(s: &Session) -> Self {
        Self {
            id: s.id.clone(),
            name: s.name.clone(),
            created_at: s.created_at,
            last_activity: s.last_activity,
            keep_alive: s.keep_alive,
        }
    }
}

/// Internal session storage
struct SessionStore {
    sessions: HashMap<String, Session>,
}

impl SessionStore {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    fn insert(&mut self, session: Session) {
        self.sessions.insert(session.id.clone(), session);
    }

    fn get(&self, id: &str) -> Option<Session> {
        self.sessions.get(id).cloned()
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut Session> {
        self.sessions.get_mut(id)
    }

    fn remove(&mut self, id: &str) -> Option<Session> {
        self.sessions.remove(id)
    }

    fn count(&self) -> usize {
        self.sessions.len()
    }

    fn list(&self) -> Vec<SessionInfo> {
        self.sessions.values().map(SessionInfo::from).collect()
    }

    fn find_by_name(&self, name: &str) -> Option<Session> {
        self.sessions
            .values()
            .find(|s| s.name.as_deref() == Some(name))
            .cloned()
    }
}

/// Session-related errors
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Session not found")]
    NotFound,

    #[error("Maximum sessions reached")]
    MaxSessionsReached,

    #[error("Session already exists")]
    AlreadyExists,

    #[error("Browser launch failed: {0}")]
    BrowserLaunchFailed(String),
}
