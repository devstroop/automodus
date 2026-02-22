# Shell & API Design

This document outlines the unified design for the interactive shell (CLI) and REST API interfaces. Both interfaces serve the same core functionality through different interaction models.

## Current State

| Component | Status | Location |
|-----------|--------|----------|
| Shell REPL | ✅ Full | `bin/automodus.rs::run_shell()`, `shell/client.rs` |
| API Server | ✅ Full | `api/server.rs`, `api/handlers.rs` (OpenAPI + Swagger) |
| Browser management (shell) | ✅ Via AppCore | `core/app.rs`, `bin/automodus.rs` |
| Browser management (API) | ✅ Via AppCore | `api/state.rs::ServerState` |
| Daemon | ✅ Implemented | `daemon/mod.rs` (socket, PID, config) |
| `AppCore` | ✅ Implemented | `core/app.rs` |
| `SessionManager` | ✅ Implemented | `core/app.rs` (integrated) |
| Readline/history | ✅ Implemented | `shell/client.rs` (`rustyline`) |
| WebSocket | ✅ Implemented | `api/ws.rs` |

> **Note:** The daemon architecture described below is now implemented. The
> "Current Problem" section below is kept for historical context — the
> daemon/client split has resolved these issues.

### Current Problem

```
Shell Process (current)
├── Browser (owned)    ← Dies when shell exits
├── Adapter            ← Auth state lost (WhatsApp QR, cookies)
└── REPL loop          ← Can't share browser between instances
```

**Issues:**
- Browser restarts on every `automodus shell` invocation
- Authentication state lost when shell exits
- Can't attach multiple shells to same browser
- Can't run shell and API against same browser

### Existing Shell Commands

```
run <workflow.yaml> [params...]   ✅ Implemented
list                              ✅ Implemented  
goto <url>                        ✅ Implemented
status                            ✅ Implemented
help                              ✅ Implemented
quit / exit                       ✅ Implemented
```

### Existing API Endpoints

```
GET  /api/health                  ✅ Implemented
GET  /api/workflows               ✅ Implemented
POST /api/workflows/reload        ✅ Implemented
GET  /api/workflows/:name         ✅ Implemented
POST /api/workflows/:name/run     ✅ Implemented
GET  /api/browser/screenshot      ✅ Implemented
POST /api/browser/goto            ✅ Implemented
```

## Overview

Inspired by Docker's architecture: daemon owns resources, clients are stateless.

```
┌───────────────────────────────────────────────────────────────┐
│                    automodus daemon                            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐        │
│  │   Session    │  │   Workflow   │  │   Browser    │        │
│  │   Manager    │  │   Engine     │  │   Pool       │        │
│  └──────────────┘  └──────────────┘  └──────────────┘        │
│                                                               │
│  Listens on:                                                  │
│  • Unix socket: ~/.automodus/automodus.sock                   │
│  • TCP/HTTP:    127.0.0.1:8080 (REST API)                    │
│  • WebSocket:   127.0.0.1:8080/ws                            │
└───────────────────────────────────────────────────────────────┘
         ▲                    ▲                   ▲
         │                    │                   │
    ┌────┴────┐          ┌────┴────┐         ┌────┴────┐
    │ shell 1 │          │ shell 2 │         │  REST   │
    │ (REPL)  │          │ (REPL)  │         │  API    │
    │stateless│          │stateless│         │ client  │
    └─────────┘          └─────────┘         └─────────┘
```

**Key principles (Docker-style):**
- Daemon owns all browser instances and sessions
- Shell/CLI is stateless - just sends commands to daemon
- Multiple shells can connect to same daemon simultaneously
- Browser sessions survive shell exit
- Explicit cleanup required (`session close`)

**Benefits:**
- Auth state preserved across shell sessions
- No browser restart delay
- Multiple terminals can control same browser
- Shell and API share same browser pool

## CLI Commands

### Daemon Commands

```bash
# Daemon lifecycle (like: systemctl start/stop docker)
automodus daemon start        # Start daemon in background
automodus daemon start -f     # Start daemon in foreground
automodus daemon stop         # Stop daemon gracefully
automodus daemon status       # Check if daemon is running
automodus daemon restart      # Restart daemon
automodus daemon logs         # View daemon logs
```

### Shell Commands

```bash
# Interactive mode (connects to daemon)
automodus shell               # Start REPL (requires daemon running)
automodus shell --start-daemon  # Auto-start daemon if not running

# Inside shell, or as direct commands:
```

### Session Commands

```bash
# Session management (browser contexts in daemon)
session new [--name=NAME] [--keep-alive]  # Create new browser session
session list                               # List active sessions  
session switch <id|name>                   # Switch active session
session close [id]                         # Close session (default: current)
session info                               # Show current session details
session keep-alive [id] [on|off]           # Toggle keep-alive for session

# Direct CLI (without entering shell)
automodus sessions            # List sessions (like: docker ps)
automodus session new         # Create session
automodus session close <id>  # Close session
```

### Workflow Commands

```bash
# Workflow execution
run <workflow> [params...]    # Run workflow with params
  -p, --param key=value       # Pass parameter
  -d, --debug                 # Enable debug mode
  --profile=<PROFILE>         # Debug profile (minimal|verbose|ci|demo)

list [pattern]                # List workflows matching pattern
show <workflow>               # Show workflow details
validate <path>               # Validate workflow file
reload                        # Reload workflows from disk
```

### Browser Commands

```bash
# Navigation
goto <url>                    # Navigate to URL
back                          # Go back
forward                       # Go forward
refresh                       # Reload page

# Interaction
click <selector>              # Click element
type <selector> <text>        # Type into element
select <selector> <value>     # Select option
hover <selector>              # Hover over element
wait <selector> [timeout]     # Wait for element

# Inspection
page [url|title|html]         # Get page info
find <selector>               # Find elements, show count
text <selector>               # Get element text
attr <selector> <name>        # Get element attribute
eval <js>                     # Execute JavaScript

# Capture
screenshot [path]             # Take screenshot
pdf [path]                    # Save as PDF

# Tabs
tabs                          # List open tabs
tab <index>                   # Switch to tab
tab new [url]                 # Open new tab
tab close [index]             # Close tab
```

### Debug Commands

```bash
# Debug mode
debug on [--profile=PROFILE]  # Enable debug mode
debug off                     # Disable debug mode
debug status                  # Show debug settings

# Inspection
highlight <selector>          # Highlight element on page
trace <workflow>              # Run with trace logging
pause                         # Pause execution (in debug mode)
step                          # Execute next step (when paused)
continue                      # Resume execution
```

### System Commands

```bash
help [command]                # Show help
history [count]               # Show command history
clear                         # Clear screen
quit | exit                   # Exit shell
```

## REST API Endpoints

### Sessions

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/sessions` | Create new session |
| GET | `/api/sessions` | List sessions |
| GET | `/api/sessions/:id` | Get session info |
| DELETE | `/api/sessions/:id` | Close session |

### Workflows

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/workflows` | List workflows |
| POST | `/api/workflows/reload` | Reload from disk |
| GET | `/api/workflows/:name` | Get workflow details |
| POST | `/api/workflows/:name/run` | Execute workflow |
| POST | `/api/workflows/:name/validate` | Validate workflow |

### Executions

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/api/executions` | List recent executions |
| GET | `/api/executions/:id` | Get execution details |
| DELETE | `/api/executions/:id` | Cancel execution |
| GET | `/api/executions/:id/output` | Get execution output |

### Browser

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/api/browser/goto` | Navigate to URL |
| POST | `/api/browser/click` | Click element |
| POST | `/api/browser/type` | Type into element |
| POST | `/api/browser/eval` | Execute JavaScript |
| GET | `/api/browser/screenshot` | Take screenshot |
| GET | `/api/browser/page` | Get page info |
| GET | `/api/browser/tabs` | List tabs |
| POST | `/api/browser/tabs` | Create new tab |

### WebSocket

| Endpoint | Description |
|----------|-------------|
| `/ws` | Real-time execution updates |

WebSocket protocol includes version for compatibility:

```json
{ "version": 1, "type": "execution.started", "id": "...", "workflow": "..." }
{ "version": 1, "type": "execution.step", "id": "...", "step": 1, "action": "click" }
{ "version": 1, "type": "execution.complete", "id": "...", "success": true }
{ "version": 1, "type": "execution.error", "id": "...", "error": "..." }
{ "version": 1, "type": "execution.paused", "id": "...", "step": 3 }
{ "version": 1, "type": "console.log", "level": "info", "message": "..." }
{ "version": 1, "type": "network.request", "method": "GET", "url": "..." }
```

Client commands:
```json
{ "version": 1, "type": "execution.continue", "id": "..." }
{ "version": 1, "type": "execution.skip", "id": "..." }
{ "version": 1, "type": "execution.abort", "id": "..." }
```

## Request/Response Schemas

### Run Workflow

```bash
# Shell
run send_message phone=1234 message="Hello"
```

```http
POST /api/workflows/send_message/run
Content-Type: application/json

{
  "params": {
    "phone": "1234",
    "message": "Hello"
  },
  "session_id": "optional-session-id",
  "debug": {
    "enabled": true,
    "profile": "verbose"
  }
}
```

```json
{
  "execution_id": "exec-abc123",
  "workflow_name": "send_message",
  "success": true,
  "duration_ms": 1234,
  "steps_executed": 5,
  "output": { "sent": true },
  "error": null,
  "debug": {
    "captures": ["exec-abc123_step1.png"],
    "console": [...],
    "network": [...]
  }
}
```

### Browser Click

```bash
# Shell
click "text:Submit"
```

```http
POST /api/browser/click
Content-Type: application/json

{
  "selector": "text:Submit",
  "session_id": "optional",
  "debug": {
    "highlight": true
  }
}
```

### Session Create

```bash
# Shell
session new --name=whatsapp
```

```http
POST /api/sessions
Content-Type: application/json

{
  "name": "whatsapp",
  "keep_alive": true,             // Prevent idle timeout (optional)
  "browser": {
    "headless": false,
    "viewport": { "width": 1280, "height": 720 }
  }
}
```

```json
{
  "id": "sess-xyz789",
  "name": "whatsapp",
  "keep_alive": true,
  "created_at": "2026-02-22T10:00:00Z",
  "browser": {
    "headless": false,
    "viewport": { "width": 1280, "height": 720 }
  },
  "url": "about:blank"
}
```

## Architecture

### Daemon (Server Process)

The daemon owns all resources and exposes them via socket/HTTP:

```rust
/// Daemon process - owns browser pool and workflows
pub struct Daemon {
    /// Application core (shared state)
    core: Arc<AppCore>,
    /// Unix socket path
    socket_path: PathBuf,
    /// HTTP server handle
    http_server: Option<JoinHandle<()>>,
}

impl Daemon {
    /// Start the daemon
    pub async fn start(config: DaemonConfig) -> Result<Self> {
        let core = Arc::new(AppCore::new(config.clone()));
        
        // Start Unix socket listener
        let socket = UnixListener::bind(&config.socket_path)?;
        tokio::spawn(Self::handle_socket(socket, core.clone()));
        
        // Start HTTP/WS server
        let http = tokio::spawn(run_http_server(core.clone(), config.http_port));
        
        Ok(Self { core, socket_path: config.socket_path, http_server: Some(http) })
    }
    
    /// Graceful shutdown
    pub async fn stop(&self) -> Result<()>;
}

/// Daemon configuration
pub struct DaemonConfig {
    pub socket_path: PathBuf,     // ~/.automodus/automodus.sock
    pub http_host: String,        // 127.0.0.1
    pub http_port: u16,           // 8080
    pub pid_file: PathBuf,        // ~/.automodus/daemon.pid
    pub log_file: PathBuf,        // ~/.automodus/daemon.log
    pub debug_dir: PathBuf,       // data/debug (configurable)
    pub max_sessions: usize,      // 10
}
```

### AppCore (Shared State)

Owned by daemon, accessed by all clients:

```rust
/// Shared application core (lives in daemon)
pub struct AppCore {
    /// Session manager (browser contexts)
    pub sessions: RwLock<SessionManager>,
    /// Workflow engine
    pub engine: WorkflowEngine,
    /// Workflow registry
    pub workflows: RwLock<WorkflowRegistry>,
    /// Debug configuration
    pub debug: RwLock<DebugConfig>,
    /// Event broadcaster for WebSocket
    pub events: broadcast::Sender<DaemonEvent>,
}

impl AppCore {
    /// Execute a workflow
    pub async fn run_workflow(
        &self,
        name: &str,
        params: HashMap<String, Value>,
        session_id: Option<&str>,
        debug: Option<DebugConfig>,
    ) -> Result<ExecutionResult, Error>;
    
    /// Execute a browser command (click, type, etc.)
    pub async fn browser_command(
        &self,
        cmd: BrowserCommand,
        session_id: Option<&str>,
    ) -> Result<CommandResult, Error>;
}
```

### Session Manager

```rust
/// Manages browser sessions (lives in daemon)
pub struct SessionManager {
    sessions: HashMap<String, Session>,
    default_session: Option<String>,
    max_sessions: usize,
}

pub struct Session {
    id: String,
    name: Option<String>,
    browser: Browser,
    page: Page,
    created_at: DateTime<Utc>,
    last_activity: DateTime<Utc>,
    keep_alive: bool,              // Skip idle timeout when true
    config: BrowserConfig,
}

impl SessionManager {
    pub async fn create(&mut self, config: SessionConfig) -> Result<Session>;
    pub async fn get(&self, id: &str) -> Option<&Session>;
    pub async fn get_or_default(&self, id: Option<&str>) -> Result<&Session>;
    pub async fn close(&mut self, id: &str) -> Result<()>;
    pub fn list(&self) -> Vec<SessionInfo>;
    
    /// Close sessions idle longer than timeout (skip keep_alive sessions)
    pub async fn cleanup_idle(&mut self, timeout: Duration) -> Vec<String>;
}
```

### Shell Client

Stateless REPL that connects to daemon:

```rust
/// Shell client - connects to daemon
pub struct ShellClient {
    /// Connection to daemon (socket or HTTP)
    conn: DaemonConnection,
    /// Current session ID
    current_session: Option<String>,
    /// Readline editor
    editor: Editor<ShellHelper>,
}

impl ShellClient {
    /// Connect to running daemon
    pub async fn connect() -> Result<Self> {
        let socket_path = dirs::data_dir()
            .unwrap_or_default()
            .join(".automodus/automodus.sock");
        
        if !socket_path.exists() {
            return Err(Error::DaemonNotRunning);
        }
        
        let conn = DaemonConnection::unix(&socket_path).await?;
        Ok(Self { conn, current_session: None, editor: create_editor()? })
    }
    
    /// Run REPL loop
    pub async fn run(&mut self) -> Result<()> {
        loop {
            let line = self.editor.readline(&self.prompt())?;
            self.execute_command(&line).await?;
        }
    }
    
    /// Execute command via daemon
    async fn execute_command(&mut self, line: &str) -> Result<()> {
        let cmd = parse_command(line)?;
        let response = self.conn.send(cmd).await?;
        self.print_response(response);
        Ok(())
    }
}
```

### Debug Integration

Both interfaces honor debug settings:

```rust
/// Debug configuration for execution
pub struct DebugConfig {
    pub enabled: bool,
    pub level: LogLevel,
    pub capture: CaptureMode,
    pub highlight: bool,
    pub delay: u64,
    pub pause: bool,
    pub console: bool,
    pub network: bool,
    pub profile: Option<String>,
}

impl DebugConfig {
    /// Apply profile defaults
    pub fn with_profile(profile: &str) -> Self;
    
    /// Merge with another config (other takes precedence)
    pub fn merge(&self, other: &DebugConfig) -> Self;
}
```

### Command Mapping

All commands route through the daemon:

```
Shell Command ──► Unix Socket ──► Daemon ──► AppCore
                                    │
API Request ────► HTTP Server ──────┘
```

| Shell | API Endpoint | Daemon Method |
|-------|--------------|---------------|
| `run workflow` | `POST /workflows/:name/run` | `core.run_workflow()` |
| `click selector` | `POST /browser/click` | `core.browser_command(Click)` |
| `goto url` | `POST /browser/goto` | `core.browser_command(Goto)` |
| `screenshot` | `GET /browser/screenshot` | `core.browser_command(Screenshot)` |
| `session new` | `POST /sessions` | `core.sessions.create()` |
| `sessions` | `GET /sessions` | `core.sessions.list()` |
| `list` | `GET /workflows` | `core.workflows.list()` |

### Backward Compatibility

| Old Command | New Equivalent |
|-------------|----------------|
| `automodus serve` | `automodus daemon start` (HTTP enabled by default) |
| `automodus shell` | `automodus shell` (now connects to daemon) |
| `automodus run workflow.yaml` | Same (sends to daemon) |

## Shell Implementation

### Connection to Daemon

```rust
use rustyline::{Editor, Config, CompletionType};

pub async fn run_shell() -> Result<()> {
    // Connect to daemon
    let client = ShellClient::connect().await.map_err(|e| {
        match e {
            Error::DaemonNotRunning => {
                eprintln!("Daemon not running. Start with: automodus daemon start");
                eprintln!("Or use: automodus shell --start-daemon");
            }
            _ => eprintln!("Connection failed: {}", e),
        }
        e
    })?;
    
    client.run_repl().await
}

impl ShellClient {
    pub async fn run_repl(&mut self) -> Result<()> {
        let config = Config::builder()
            .completion_type(CompletionType::List)
            .build();
        
        let mut rl = Editor::with_config(config)?;
        
        // Load history
        let history_path = dirs::data_dir()
            .map(|d| d.join("automodus/history.txt"));
        if let Some(ref path) = history_path {
            let _ = rl.load_history(path);
        }
    
        loop {
            let prompt = self.format_prompt().await;
            
            match rl.readline(&prompt) {
                Ok(line) => {
                    rl.add_history_entry(&line)?;
                    
                    // Send command to daemon via socket
                    if let Err(e) = self.send_command(&line).await {
                        eprintln!("Error: {}", e);
                    }
                }
                Err(ReadlineError::Interrupted) => continue,
                Err(ReadlineError::Eof) => break,
                Err(e) => return Err(e.into()),
            }
        }
        
        // Save history
        if let Some(path) = history_path {
            let _ = rl.save_history(&path);
        }
        
        Ok(())
    }
    
    /// Send command to daemon and print response
    async fn send_command(&mut self, line: &str) -> Result<()> {
        let cmd = parse_command(line)?;
        let response = self.conn.request(cmd).await?;
        self.print_response(response);
        Ok(())
    }
}
```

### Autocomplete

```rust
impl Completer for ShellHelper {
    fn complete(&self, line: &str, pos: usize) -> Result<(usize, Vec<Pair>)> {
        let words: Vec<&str> = line[..pos].split_whitespace().collect();
        
        match words.as_slice() {
            // Command completion
            [] | [""] => Ok((0, self.commands.clone())),
            
            // Workflow name completion (fetched from daemon)
            ["run"] | ["show"] | ["r"] => {
                let workflows = self.client.list_workflows_cached();
                Ok((pos, workflows.into_iter()
                    .map(|w| Pair { display: w.clone(), replacement: w })
                    .collect()))
            }
            
            // Session completion (fetched from daemon)
            ["session", "switch"] | ["session", "close"] => {
                let sessions = self.client.list_sessions_cached();
                Ok((pos, sessions.into_iter()
                    .map(|s| Pair { display: s.name_or_id(), replacement: s.id })
                    .collect()))
            }
            
            _ => Ok((pos, vec![]))
        }
    }
}
```

### Output Formatting

```rust
pub enum OutputFormat {
    Human,  // Pretty printed, colored
    Json,   // JSON for scripting
    Table,  // Tabular data
}

impl ShellContext {
    pub fn print(&self, data: &impl Serialize) {
        match self.format {
            OutputFormat::Human => {
                // Pretty print with colors
                println!("{}", format_human(data));
            }
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(data).unwrap());
            }
            OutputFormat::Table => {
                // Table format for lists
                println!("{}", format_table(data));
            }
        }
    }
}
```

## API Implementation

### Unified Handlers

```rust
// All handlers delegate to core

pub async fn run_workflow_handler(
    State(core): State<Arc<AppCore>>,
    Path(name): Path<String>,
    Json(req): Json<RunWorkflowRequest>,
) -> impl IntoResponse {
    let result = core.run_workflow(
        &name,
        req.params,
        req.session_id.as_deref(),
        req.debug,
    ).await;
    
    match result {
        Ok(exec) => (StatusCode::OK, Json(exec.to_response())),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse::from(e))),
    }
}

pub async fn click_handler(
    State(core): State<Arc<AppCore>>,
    Json(req): Json<ClickRequest>,
) -> impl IntoResponse {
    let result = core.browser_command(
        BrowserCommand::Click { selector: req.selector },
        req.session_id.as_deref(),
    ).await;
    
    // ...
}
```

### WebSocket Updates

```rust
pub async fn ws_handler(
    State(core): State<Arc<AppCore>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, core))
}

async fn handle_ws(socket: WebSocket, core: Arc<AppCore>) {
    let (mut tx, mut rx) = socket.split();
    
    // Subscribe to execution events
    let mut events = core.subscribe_events();
    
    loop {
        tokio::select! {
            // Forward events to client
            Some(event) = events.recv() => {
                let msg = serde_json::to_string(&event).unwrap();
                if tx.send(Message::Text(msg)).await.is_err() {
                    break;
                }
            }
            
            // Handle client messages
            Some(Ok(msg)) = rx.next() => {
                // Handle commands via WebSocket
            }
            
            else => break,
        }
    }
}
```

## Configuration

### Config File Locations

| Config | Location | Scope |
|--------|----------|-------|
| Daemon | `~/.automodus/daemon.toml` | User (global) |
| Server/Browser | `config/app.toml` | Workspace (project) |
| Shell preferences | `~/.config/automodus/shell.toml` | User (global) |
| Shell history | `~/.local/share/automodus/history.txt` | User (global) |
| Daemon socket | `~/.automodus/automodus.sock` | Runtime |
| Daemon PID | `~/.automodus/daemon.pid` | Runtime |
| Daemon log | `~/.automodus/daemon.log` | Runtime |

### Daemon Config

```toml
# ~/.automodus/daemon.toml

[daemon]
socket_path = "~/.automodus/automodus.sock"
pid_file = "~/.automodus/daemon.pid"
log_file = "~/.automodus/daemon.log"
log_level = "info"                # info | debug | trace

[http]
host = "127.0.0.1"
port = 8080
cors_origins = ["*"]

[browser]
headless = false
max_sessions = 10
default_viewport = { width = 1280, height = 720 }
user_data_dir = "~/.automodus/browser-data"  # Persistent profile

[limits]
execution_timeout = 300000        # 5 minutes
session_idle_timeout = 3600000    # 1 hour (auto-close idle sessions)

[session]
# NOTE: session_idle_timeout affects long-running auth like WhatsApp Web.
# WhatsApp web disconnects after ~14 days idle. A 1-hour timeout may cause
# unexpected logouts for "always-on" use cases. Options:
#   1. Increase timeout for long-running sessions
#   2. Use keep_alive per session (see below)
default_keep_alive = false        # Default for new sessions

[debug]
dir = "data/debug"                # Debug output directory (screenshots, traces)
```

### Shell Config

```toml
# ~/.config/automodus/shell.toml

[shell]
prompt = "automodus"          # Prompt prefix
history_size = 1000           # Max history entries
output_format = "human"       # human | json | table

[shell.colors]
prompt = "green"
success = "green"
error = "red"
warning = "yellow"

[shell.aliases]
r = "run"
g = "goto"
s = "screenshot"
l = "list"
```

### Server Config

```toml
# config/app.toml

[server]
host = "127.0.0.1"
port = 8080
cors_origins = ["*"]

[server.auth]
enabled = false
api_key = ""

[server.limits]
max_sessions = 10
execution_timeout = 300000    # 5 minutes
```

## Error Handling

> **Note:** `AppError` below extends the existing `AutomodusError` in `src/error.rs`. 
> During implementation, consider unifying into a single error type.

Error codes defined in shared module (`src/error.rs`):

```rust
/// Application error with code for API responses
#[derive(Debug, Clone, Serialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
    pub details: Option<Value>,
}

/// Standardized error codes
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // Daemon errors
    DaemonNotRunning,
    DaemonAlreadyRunning,
    DaemonConnectionFailed,
    
    // Session errors
    SessionNotFound,
    SessionLimitReached,
    
    // Workflow errors  
    WorkflowNotFound,
    WorkflowInvalid,
    WorkflowTimeout,
    
    // Execution errors
    ExecutionFailed,
    ExecutionCancelled,
    StepFailed,
    
    // Browser errors
    BrowserLaunchFailed,
    BrowserDisconnected,
    SelectorNotFound,
    SelectorTimeout,
    NavigationFailed,
    
    // General errors
    InvalidRequest,
    InternalError,
}

impl AppError {
    pub fn selector_not_found(selector: &str, timeout_ms: u64) -> Self {
        Self {
            code: ErrorCode::SelectorNotFound,
            message: format!("Element not found: {}", selector),
            details: Some(json!({ "timeout_ms": timeout_ms })),
        }
    }
    
    pub fn workflow_not_found(name: &str) -> Self {
        Self {
            code: ErrorCode::WorkflowNotFound,
            message: format!("Workflow '{}' not found", name),
            details: None,
        }
    }
    // ... other constructors
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}
```

Shell output:
```
❌ Error [SELECTOR_NOT_FOUND]: Element not found: text:Submit
   Tried for 5000ms before timing out
```

API response:
```json
{
  "error": {
    "code": "SELECTOR_NOT_FOUND",
    "message": "Element not found: text:Submit",
    "details": { "timeout_ms": 5000 }
  }
}
```

## Implementation Phases

> **Note:** Phase 1 is approximately 60-70% of total implementation effort.

### Phase 1: Daemon Architecture (~2-3 weeks)

> **Note:** This phase is approximately 60-70% of total implementation effort. Split into sub-phases to reduce risk.

**Goal:** Docker-style daemon/client separation.

#### Phase 1a: Daemon Core (~1 week)

1. **Create `Daemon` struct** (`src/daemon/mod.rs`)
   - Long-running background process
   - PID file for process management
   - Unix socket listener for local clients

2. **Create `AppCore` struct** (`src/core/app.rs`)
   - Move `WorkflowEngine` ownership here
   - Move workflow loading from `ServerState`
   - Add `DebugConfig` field
   - Add event broadcaster for WebSocket

3. **Daemon CLI commands**
   - `automodus daemon start` - start daemon
   - `automodus daemon stop` - stop daemon
   - `automodus daemon status` - check status

**Checkpoint:** Test with existing API via HTTP, daemon serves requests.

#### Phase 1b: Session Manager (~3-4 days)

1. **Create `SessionManager`** (`src/core/session.rs`)
   - Extract browser launch logic (single implementation)
   - Session lifecycle: create, get, close, list
   - Sessions persist until explicitly closed
   - Idle cleanup with `keep_alive` support

2. **Merge `serve` command into daemon**
   - HTTP server becomes part of daemon
   - Remove standalone `serve` command

**Checkpoint:** Multiple API clients share same browser via sessions.

#### Phase 1c: Shell Refactor (~3-4 days)

1. **Create `ShellClient`** (`src/shell/client.rs`)
   - Stateless REPL connecting to daemon
   - Sends commands via Unix socket
   - Receives responses and prints output

2. **Add `rustyline` dependency**
   - Basic readline with history
   - Save/load history from `~/.local/share/automodus/history.txt`

3. **Test end-to-end**
   - Browser survives shell exit
   - Multiple shells can connect  
   - Auth state persists

**Deliverables (full Phase 1):**
- [ ] Daemon process with PID file
- [ ] Unix socket communication
- [ ] `AppCore` and `SessionManager`
- [ ] Shell connects to daemon (no local browser)
- [ ] `daemon start/stop/status` commands
- [ ] Readline with history
- [ ] Browser survives shell exit

### Phase 2: Shell Commands (~1-2 weeks)

**Goal:** Feature parity between shell and API for browser control.

1. **Browser commands**
   - `click <selector>` - click element
   - `type <selector> <text>` - type into element
   - `wait <selector> [timeout]` - wait for element
   - `screenshot [path]` - take screenshot
   - `eval <js>` - execute JavaScript
   - `text <selector>` - get element text
   - `find <selector>` - find elements, show count

2. **Navigation commands**
   - `back` - go back
   - `forward` - go forward  
   - `refresh` - reload page

3. **Tab commands** (if multi-tab support needed)
   - `tabs` - list tabs
   - `tab <index>` - switch tab
   - `tab new [url]` - new tab
   - `tab close` - close tab

4. **Autocomplete**
   - Command completion
   - Workflow name completion for `run`
   - File path completion

**Deliverables:**
- [ ] All browser commands implemented
- [ ] Autocomplete for commands and workflows
- [ ] Help text for each command

### Phase 3: API Enhancement (~1-2 weeks)

**Goal:** Full API parity with shell + real-time updates.

1. **Session endpoints**
   - `POST /api/sessions` - create session
   - `GET /api/sessions` - list sessions
   - `GET /api/sessions/:id` - get session
   - `DELETE /api/sessions/:id` - close session

2. **Browser endpoints**
   - `POST /api/browser/click` - click element
   - `POST /api/browser/type` - type into element
   - `POST /api/browser/wait` - wait for element
   - `POST /api/browser/eval` - execute JavaScript
   - `GET /api/browser/page` - get page info

3. **Execution tracking**
   - `GET /api/executions` - list recent
   - `GET /api/executions/:id` - get details
   - `DELETE /api/executions/:id` - cancel

4. **WebSocket**
   - Implement `/ws` endpoint
   - Execution events (started, step, complete, error)
   - Pause/continue commands

**Deliverables:**
- [ ] Session CRUD endpoints
- [ ] Browser control endpoints
- [ ] Execution tracking endpoints
- [ ] WebSocket with event streaming

### Phase 4: Debug Integration (~1 week)

**Goal:** Wire debug mode through both interfaces.

1. **CLI flags**
   - `--debug` flag for `run` command
   - `--debug=trace` for level
   - `--delay=1000` for slow mode
   - `--profile=ci` for presets

2. **Shell debug commands**
   - `debug on [--profile=PROFILE]`
   - `debug off`
   - `debug status`
   - `highlight <selector>`

3. **API debug support**
   - Accept `debug` object in run request
   - Return debug data in response
   - Stream console/network via WebSocket

4. **Capture/Console/Network**
   - Screenshot capture based on `capture` mode
   - Console capture when `console: true`
   - Network capture when `network: true`

**Deliverables:**
- [ ] CLI debug flags working
- [ ] Shell debug commands working
- [ ] API accepts and returns debug config
- [ ] CDP listeners for console/network

## Migration Checklist

```
[ ] Phase 1a - Daemon Core
    [ ] Create src/daemon/mod.rs with Daemon struct
    [ ] Create src/core/app.rs with AppCore
    [ ] Implement Unix socket listener
    [ ] Implement daemon start/stop/status commands
    [ ] Add PID file management
    [ ] Test: daemon start/stop works
    [ ] Test: API works via daemon HTTP

[ ] Phase 1b - Session Manager
    [ ] Create src/core/session.rs with SessionManager
    [ ] Extract browser launch logic (remove duplicates)
    [ ] Implement idle cleanup with keep_alive support
    [ ] Merge api/server.rs into daemon (HTTP endpoint)
    [ ] Test: sessions persist across API calls
    [ ] Test: idle sessions cleaned up

[ ] Phase 1c - Shell Refactor
    [ ] Add rustyline + dirs to Cargo.toml
    [ ] Create src/shell/client.rs with ShellClient
    [ ] Refactor shell to connect to daemon (no local browser)
    [ ] Add history save/load
    [ ] Test: shell connects to daemon
    [ ] Test: browser survives shell exit
    [ ] Test: multiple shells can connect
    [ ] Test: auth state persists (WhatsApp QR)

[ ] Phase 2
    [ ] Implement click command
    [ ] Implement type command  
    [ ] Implement wait command
    [ ] Implement screenshot command
    [ ] Implement eval command
    [ ] Implement text command
    [ ] Implement find command
    [ ] Implement navigation commands
    [ ] Add autocomplete
    [ ] Update help text

[ ] Phase 3
    [ ] Add session endpoints
    [ ] Add browser control endpoints
    [ ] Add execution tracking endpoints
    [ ] Implement WebSocket
    [ ] Update OpenAPI schema

[ ] Phase 4
    [ ] Add CLI debug flags
    [ ] Add shell debug commands
    [ ] Wire debug through API
    [ ] Implement CDP listeners
    [ ] Add screenshot capture
```

## Related

- [DEBUG.md](DEBUG.md) - Debug mode design
- [DESIGN.md](../DESIGN.md) - Overall architecture
- [CONTRIBUTING.md](../CONTRIBUTING.md) - Development guidelines
