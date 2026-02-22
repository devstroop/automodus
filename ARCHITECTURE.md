# Architecture

This document describes the architecture of Automodus — a programmable browser automation platform with YAML-based workflows.

## System Overview

```
┌───────────────────────────────────────────────────────────────────┐
│                        automodus daemon                           │
│                                                                   │
│  ┌──────────┐  ┌──────────────┐  ┌────────────┐  ┌───────────┐  │
│  │ AppCore  │  │   Workflow   │  │  Browser   │  │  Session  │  │
│  │ (shared  │──│   Engine     │──│  Adapter   │  │  Store    │  │
│  │  state)  │  │              │  │  (CDP)     │  │           │  │
│  └──────────┘  └──────────────┘  └────────────┘  └───────────┘  │
│       │                                                           │
│  ┌────┴──────────────────────────────────────────────────────┐   │
│  │  Interfaces                                                │   │
│  │  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌──────────┐ │   │
│  │  │  Unix    │  │  HTTP    │  │ WebSocket │  │  Shell   │ │   │
│  │  │  Socket  │  │  REST   │  │  /ws      │  │  REPL    │ │   │
│  │  └──────────┘  └──────────┘  └───────────┘  └──────────┘ │   │
│  └────────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────┘
```

Automodus follows a Docker-inspired daemon/client architecture:
- **Daemon** owns browser instances, sessions, and workflow execution
- **Clients** (shell, REST API, WebSocket) are stateless interfaces

## Directory Structure

```
src/
├── bin/automodus.rs        # CLI entry point, command parsing
├── lib.rs                  # Library root, public re-exports
├── config.rs               # AppConfig (TOML loading)
├── error.rs                # Unified error types (AutomodusError, AppError, ErrorCode)
│
├── workflow/               # Workflow definition layer
│   ├── schema.rs           # Workflow, Step, DebugConfig, Triggers types
│   ├── parser.rs           # YAML parsing and validation
│   └── loader.rs           # Directory scanning and caching
│
├── core/                   # Execution engine
│   ├── app.rs              # AppCore (shared state, session store)
│   ├── engine.rs           # WorkflowEngine (step execution, debug, pause)
│   ├── context.rs          # ExecutionContext (params, store, debug config)
│   └── template.rs         # {{variable}} interpolation
│
├── actions/                # Action system
│   ├── registry.rs         # Action trait, ActionContext, BrowserHandle
│   └── control.rs          # Module-agnostic actions (log, emit)
│
├── modules/                # Pluggable automation modules
│   ├── browser/
│   │   ├── adapter.rs      # ChromePageAdapter (BrowserHandle impl)
│   │   ├── driver.rs       # BrowserService (lifecycle management)
│   │   ├── launch.rs       # LaunchOptions, launch_browser()
│   │   ├── selector.rs     # Extended selectors (text:, role:, xpath:)
│   │   └── actions/        # Browser actions
│   │       ├── navigate.rs # goto, back, forward, reload
│   │       ├── interact.rs # click, type, select, hover
│   │       ├── wait.rs     # wait_for, sleep
│   │       ├── extract.rs  # extract, eval
│   │       ├── capture.rs  # screenshot
│   │       ├── upload.rs   # file upload, file chooser
│   │       └── tabs.rs     # tab management
│   └── http/
│       ├── client.rs       # HttpClient
│       └── actions/        # HTTP actions (get, post, put, patch, delete)
│
├── daemon/                 # Daemon process
│   ├── mod.rs              # Daemon struct, lifecycle, socket listener
│   └── config.rs           # DaemonConfig (TOML from ~/.automodus/)
│
├── shell/                  # Interactive shell
│   ├── mod.rs              # Re-exports
│   └── client.rs           # ShellClient (rustyline, commands, completion)
│
├── api/                    # REST API + WebSocket
│   ├── server.rs           # Router, OpenAPI/Swagger docs
│   ├── handlers.rs         # Request handlers
│   ├── schemas.rs          # Request/response DTOs
│   ├── state.rs            # ServerState (wraps AppCore)
│   └── ws.rs               # WebSocket endpoint, event broadcasting
│
├── triggers/               # Trigger definitions (API, schedule, event, webhook)
│   └── mod.rs              # Type re-exports from schema
│
└── utils/
    ├── logging.rs          # Tracing/subscriber setup
    ├── convert.rs          # YAML↔JSON conversion
    ├── trace.rs            # TraceLogger (JSONL debug output)
    └── metrics.rs          # Metrics collection
```

## Core Concepts

### Workflows

A workflow is a YAML file defining a sequence of browser or HTTP actions:

```yaml
name: search_example
params:
  query: { type: string, required: true }
steps:
  - action: goto
    url: "https://example.com/search"
  - action: type
    selector: "input[name=q]"
    text: "{{params.query}}"
  - action: click
    selector: "button[type=submit]"
  - action: extract
    selector: ".result-item"
    many: true
    as: results
output:
  results: "{{store.results}}"
```

Key schema types (in `src/workflow/schema.rs`):

| Type | Purpose |
|------|---------|
| `Workflow` | Top-level: name, params, steps, triggers, debug, output |
| `Step` | Single action with selector, error handling, conditions |
| `DebugConfig` | Optional debug settings (`Option<T>` fields for merge) |
| `ResolvedDebugConfig` | Concrete debug values after merging |
| `Triggers` | API, schedule, event, webhook, manual trigger definitions |

### Execution Pipeline

```
YAML file
   │
   ▼
WorkflowParser::parse()     ──► Workflow struct
   │
   ▼
WorkflowEngine::execute()
   │
   ├── Build ExecutionContext (params, store, debug config)
   │
   ├── For each Step:
   │   ├── TemplateEngine::render()   ──► Resolve {{variables}}
   │   ├── Evaluate step conditions
   │   ├── Debug: delay, highlight, pause, screenshot
   │   ├── ActionRegistry::execute()  ──► Dispatch to Action impl
   │   │       │
   │   │       ├── BrowserHandle method (click, type, goto...)
   │   │       └── Returns ActionOutput (data, store vars, events)
   │   │
   │   ├── Update ExecutionContext.store
   │   ├── Error handling (retry, screenshot on failure)
   │   └── Handle goto/skip from ActionOutput
   │
   └── Return WorkflowResult (success, duration, output, screenshots)
```

### Action System

Actions implement the `Action` trait:

```rust
#[async_trait]
pub trait Action: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(
        &self,
        ctx: &ActionContext<'_>,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError>;
}
```

The `BrowserHandle` trait abstracts browser operations:

```rust
#[async_trait]
pub trait BrowserHandle: Send + Sync {
    async fn goto(&self, url: &str) -> Result<(), ActionError>;
    async fn click(&self, selector: &str) -> Result<(), ActionError>;
    async fn type_text(&self, selector: &str, text: &str, clear: bool) -> Result<(), ActionError>;
    async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError>;
    async fn eval(&self, js: &str) -> Result<Value, ActionError>;
    async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>, ActionError>;
    // ... more methods
}
```

`ChromePageAdapter` implements `BrowserHandle` using `chromiumoxide::Page`.

### Extended Selectors

The selector system (`src/modules/browser/selector.rs`) supports:

| Prefix | Example | Resolution |
|--------|---------|------------|
| *(none)* | `#submit` | CSS selector |
| `text:` | `text:Submit` | Exact text match via JS |
| `text*:` | `text*:Subm` | Partial text match via JS |
| `role:` | `role:button[Submit]` | ARIA role + accessible name |
| `xpath:` | `xpath://div[@id='x']` | XPath evaluation |

### Template Engine

`{{variable}}` interpolation in step fields, resolving from:
- `params.*` — workflow parameters
- `store.*` — values saved by previous steps
- `env.*` — environment variables

## Component Details

### AppCore (`src/core/app.rs`)

Central shared state owned by the daemon:

```
AppCore
├── browser: Mutex<Option<Browser>>          # Lazily launched browser
├── page_adapter: Mutex<Option<Adapter>>     # Current page
├── sessions: RwLock<SessionStore>           # Session CRUD
├── workflows: RwLock<HashMap<String, Workflow>>  # Workflow cache
├── debug_config: RwLock<ResolvedDebugConfig>     # Global debug
├── event_tx: broadcast::Sender<CoreEvent>        # Event bus
└── config (max_sessions, idle_timeout, debug_dir)
```

Key methods:
- `get_page()` — lazy browser launch, returns `ChromePageAdapter`
- `create_session()` / `close_session()` / `list_sessions()`
- `find_session(id_or_name)` — lookup by UUID or friendly name
- `cleanup_idle_sessions()` — removes non-keep-alive sessions past timeout

### Daemon (`src/daemon/mod.rs`)

Long-running background process:

```
Daemon::start()
├── Write PID file (~/.automodus/daemon.pid)
├── Bind Unix socket (~/.automodus/automodus.sock)
└── Check for stale PID

Daemon::run()
├── Accept socket connections (loop)
├── Spawn HTTP server (if enable_http)
├── Spawn session cleanup task (every 60s)
├── Handle SIGTERM/SIGINT → graceful shutdown
└── Cleanup: close sessions, remove PID + socket files
```

The HTTP server inside the daemon uses `create_state_with_core()` to share the daemon's `AppCore` with the API layer.

### ServerState (`src/api/state.rs`)

API-layer state wrapping AppCore:

```
ServerState
├── core: Arc<AppCore>              # Shared with daemon
├── engine: WorkflowEngine          # Step execution
├── workflows: RwLock<HashMap>      # Loaded workflows
├── sessions: RwLock<HashMap>       # API-layer sessions
├── executions: RwLock<HashMap>     # Execution history
└── event_tx: broadcast::Sender     # WebSocket events
```

Browser access delegates to `self.core.get_page()`.

### ShellClient (`src/shell/client.rs`)

Interactive REPL with rustyline:

```
ShellClient
├── editor: Editor<ShellCompleter>  # Readline with completion
├── config: ShellConfig             # History path, prompt, dirs
└── session_names: Arc<RwLock<Vec>> # Dynamic completion data
```

Commands are parsed into `ShellCommand` variants:

| Category | Commands |
|----------|----------|
| Navigation | `goto`, `back`, `forward`, `refresh` |
| Interaction | `click`, `type`, `wait`, `eval`, `text` |
| Capture | `screenshot`, `highlight` |
| Workflows | `run`, `list`, `trace` |
| Sessions | `session new/list/switch/close/info/keep-alive` |
| Debug | `debug on/off/status` |
| System | `help`, `quit`, `status` |

Completion supports: command names, workflow paths, session names, file paths, session subcommands.

### WebSocket (`src/api/ws.rs`)

Real-time event streaming over `/ws`:

```
Client ←── ServerEvent (version 1)
  execution.started  { id, workflow }
  execution.step     { id, step, action }
  execution.complete { id, success }
  execution.error    { id, error }
  execution.paused   { id, step }

Client ──► WsCommand
  execution.continue { id }
  execution.skip     { id }
  execution.abort    { id }
```

### Debug System

Debug configuration flows through three merge layers:

```
Environment vars (lowest precedence)
    ↓ merge
Workflow YAML debug section
    ↓ merge
CLI flags (highest precedence)
    ↓ resolve
ResolvedDebugConfig (concrete values)
```

Debug features wired into the engine:
- **Delay**: `tokio::sleep` between steps
- **Highlight**: JS injection to flash red outline on target element
- **Pause**: `PauseHandler` trait — `DefaultPauseHandler` auto-continues
- **Capture**: Screenshots at configurable points (before, after, failure, all)
- **Console/Network**: CDP event listeners (`start_console_listener`, `start_network_listener`) via `ChromePageAdapter`
- **Trace**: JSONL output to `data/debug/trace.jsonl` via `TraceLogger`

Debug profiles (`--profile=<name>`):

| Profile | Behavior |
|---------|----------|
| `minimal` | Logging only, no captures |
| `verbose` | Full logging, highlights, network capture |
| `ci` | Capture on failure, no delays |
| `demo` | Slow execution with delays + highlights |

## Configuration

### Files

| File | Scope | Purpose |
|------|-------|---------|
| `config/app.toml` | Workspace | Server host/port, browser settings, workflow dir |
| `~/.automodus/daemon.toml` | User | Daemon socket/PID paths, HTTP config, limits |
| `~/.local/share/automodus/history.txt` | User | Shell command history |
| `~/.automodus/daemon.pid` | Runtime | Daemon process ID |
| `~/.automodus/automodus.sock` | Runtime | Unix domain socket |
| `~/.automodus/daemon.log` | Runtime | Daemon log output |

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `AUTOMODUS_CONFIG` | Config file path (default: `config/app.toml`) |
| `AUTOMODUS_WORKFLOWS` | Workflows directory (default: `workflows/`) |
| `RUST_LOG` | Log level filter |
| `AUTOMODUS_DEBUG` | Enable debug mode (`1`, `true`, `yes`, `on`) |
| `AUTOMODUS_DEBUG_LEVEL` | Debug log level (`info`, `debug`, `trace`) |
| `AUTOMODUS_DEBUG_PROFILE` | Debug preset (`minimal`, `verbose`, `ci`, `demo`) |
| `AUTOMODUS_DEBUG_DELAY` | Step delay in milliseconds |
| `AUTOMODUS_DEBUG_CAPTURE` | Screenshot mode (`none`, `failure`, `before`, `after`, `all`) |

## CLI Commands

```
automodus run <workflow.yaml> [--debug] [--delay=ms] [--capture=mode] [--profile=name]
automodus shell                     # Interactive REPL
automodus serve                     # [DEPRECATED] Start HTTP server (use daemon start)
automodus daemon start|stop|status|restart|logs
automodus validate [path]           # Validate workflow YAML files
automodus list                      # List loaded workflows
```

## API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/api/health` | Health check |
| GET | `/api/workflows` | List workflows |
| POST | `/api/workflows/reload` | Reload from disk |
| GET | `/api/workflows/:name` | Workflow details |
| POST | `/api/workflows/:name/run` | Execute workflow |
| POST | `/api/sessions` | Create session |
| GET | `/api/sessions` | List sessions |
| GET | `/api/sessions/:id` | Session details |
| DELETE | `/api/sessions/:id` | Close session |
| POST | `/api/browser/goto` | Navigate |
| POST | `/api/browser/click` | Click element |
| POST | `/api/browser/type` | Type text |
| POST | `/api/browser/wait` | Wait for element |
| POST | `/api/browser/eval` | Execute JavaScript |
| GET | `/api/browser/screenshot` | Take screenshot |
| GET | `/api/browser/page` | Page info |
| GET | `/api/executions` | List executions |
| GET | `/api/executions/:id` | Execution details |
| DELETE | `/api/executions/:id` | Cancel execution |
| WS | `/ws` | Real-time events |

Swagger UI available at `/swagger-ui`.

## Error Handling

Two error types unified in `src/error.rs`:

- **`AutomodusError`** — library-level errors (browser, workflow, IO, daemon, session)
- **`AppError`** — API-facing errors with `ErrorCode` enum for structured responses

`ErrorCode` variants: `DaemonNotRunning`, `SessionNotFound`, `SessionLimitReached`, `WorkflowNotFound`, `WorkflowInvalid`, `WorkflowTimeout`, `ExecutionFailed`, `SelectorNotFound`, `SelectorTimeout`, `NavigationFailed`, `BrowserLaunchFailed`, `BrowserDisconnected`, `InvalidRequest`, `InternalError`.

API errors return:
```json
{ "error": { "code": "SELECTOR_NOT_FOUND", "message": "Element not found: #btn" } }
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `chromiumoxide` | Browser automation via CDP |
| `tokio` | Async runtime |
| `axum` | HTTP framework |
| `rustyline` | Readline/history for shell |
| `serde` / `serde_yaml` / `serde_json` | Serialization |
| `tracing` | Structured logging |
| `uuid` | Session/execution IDs |
| `chrono` | Timestamps |
| `thiserror` / `anyhow` | Error handling |
| `utoipa` | OpenAPI documentation |
| `tokio-tungstenite` | WebSocket support |
| `rusqlite` | SQLite for execution history |
| `reqwest` | HTTP client for outbound requests |
| `glob` | Workflow file discovery |
| `regex` | Template pattern matching |

## Known Architectural Debt

1. **Pause handlers are partial** — `PauseHandler` trait exists with `DefaultPauseHandler` (auto-continue), but `ShellPauseHandler` (interactive stdin) and `WebSocketPauseHandler` (client command wait) are not implemented.

### Resolved Debt

- ~~Shell runs standalone~~ — Shell now auto-detects running daemon and routes commands via Unix socket protocol (`handle_socket_connection`, `dispatch_request`, `DaemonClient`).
- ~~Dual session stores~~ — `ServerSession` removed. `ServerState` delegates to `AppCore`'s `SessionStore` as single source of truth.
- ~~Console/network capture uses JS injection~~ — Replaced with native CDP event listeners (`EventConsoleApiCalled`, `EventRequestWillBeSent`/`EventResponseReceived`) via `start_console_listener()` and `start_network_listener()`.

See [docs/ISSUES.md](docs/ISSUES.md) for the full issue tracker.
