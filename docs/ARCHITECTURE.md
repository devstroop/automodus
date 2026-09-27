# Architecture

This document describes the architecture of Automodus — a programmable browser automation platform with YAML-based workflows.

## System Overview

```
┌───────────────────────────────────────────────────────────────────┐
│                        automodus daemon                           │
│                                                                   │
│  ┌──────────┐  ┌──────────────┐  ┌─────────────┐  ┌───────────┐  │
│  │ AppCore  │  │   Workflow   │  │  Session    │  │  Session  │  │
│  │ (shared  │──│   Engine     │──│  Adapter    │──│  Store    │  │
│  │  state)  │  │              │  │ (3 engines) │  │           │  │
│  └──────────┘  └──────────────┘  └─────────────┘  └───────────┘  │
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
│   ├── engine.rs           # WorkflowEngine (step execution, loops, handlers, debug, pause)
│   ├── context.rs          # ExecutionContext (params, store, debug config)
│   ├── template.rs         # {{variable}} interpolation (root allow-list + |json)
│   └── json_path.rs        # JSONPath-style extraction for templates
│
├── actions/                # Action system
│   ├── registry.rs         # Action trait, BrowserHandle, BrowserCapabilities, registry
│   └── control.rs          # Module-agnostic actions (log, emit)
│
├── modules/                # Pluggable automation modules
│   ├── browser/
│   │   ├── session.rs      # SessionAdapter: engine dispatch + shared capability map
│   │   ├── adapter.rs      # ChromePageAdapter (chromiumoxide CDP, BrowserHandle impl)
│   │   ├── firefox.rs      # Firefox BiDi backend (rustenium, BrowserHandle impl)
│   │   ├── driver.rs       # BrowserService (lifecycle management)
│   │   ├── launch.rs       # LaunchOptions, launch_session() (engine dispatch)
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
│   ├── protocol.rs         # Typed socket protocol (SocketRequest/SocketResponse, framing)
│   └── config.rs           # DaemonConfig defaults + TOML loader (see Configuration)
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
    ├── debug.rs            # Debug utilities
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
    │   ├── Evaluate step conditions / condition + loop (engine pseudo-actions)
    │   ├── Debug: delay, highlight, pause, screenshot
    │   ├── ActionRegistry::execute()  ──► Dispatch to Action impl
    │   │       │
    │   │       ├── BrowserHandle method (click, type, goto...)
    │   │       └── Returns ActionOutput (data, store vars, events)
    │   │
    │   ├── Update ExecutionContext.store
    │   ├── Error handling (step retry, on_failure handlers, screenshot)
    │   └── Handle goto/skip from ActionOutput / on_success handlers
    │
    └── Return WorkflowResult (success, duration, output, screenshots)
```

### Action System

Actions implement the `Action` trait (`src/actions/registry.rs`):

```rust
#[async_trait]
pub trait Action: Send + Sync {
    fn name(&self) -> &'static str;
    async fn execute(
        &self,
        params: &HashMap<String, serde_yaml::Value>,
        ctx: &ActionContext,
        browser: &dyn BrowserHandle,
    ) -> Result<ActionOutput, ActionError>;
}
```

Actions register in `register_builtins()`; the same registry backs runtime
dispatch **and** `automodus validate` — an unknown `action:` fails validation
with the full known-action list (including `loop`/`call`/`condition` and
aliases `navigate`/`input`/`wait`).

The `BrowserHandle` trait abstracts browser operations:

```rust
#[async_trait]
pub trait BrowserHandle: Send + Sync {
    /// Runtime feature flags for this backend (default: BrowserCapabilities::NONE)
    fn capabilities(&self) -> BrowserCapabilities { BrowserCapabilities::NONE }

    async fn goto(&self, url: &str) -> Result<(), ActionError>;
    async fn click(&self, selector: &str) -> Result<(), ActionError>;
    async fn type_text(&self, selector: &str, text: &str, clear: bool) -> Result<(), ActionError>;
    async fn wait_for(&self, selector: &str, timeout_ms: u64) -> Result<(), ActionError>;
    async fn eval(&self, script: &str) -> Result<Value, ActionError>;
    async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>, ActionError>;
    async fn new_tab(&self, url: Option<&str>) -> Result<usize, ActionError>;
    async fn pdf(&self) -> Result<Vec<u8>, ActionError>;   // Chromium-only
    // ... more methods (tabs, file inputs, history)
}
```

### Browser Engine Layer

Three backends implement `BrowserHandle`:

| Backend | File | Protocol | Notes |
|---------|------|----------|-------|
| Chromium | `modules/browser/adapter.rs` (`ChromePageAdapter`) | CDP via `chromiumoxide` | Full capability set |
| Firefox | `modules/browser/firefox.rs` | WebDriver BiDi via `rustenium` (no geckodriver) | No PDF/file-chooser/console events yet |
| Lightpanda | `modules/browser/session.rs` + CDP attach | CDP over `lightpanda serve` | Reuses Chromium CDP paths; single-page: no tab management |

`launch_session(&LaunchOptions)` (`modules/browser/launch.rs`) dispatches on
`BrowserEngine` and returns a `SessionAdapter` — the single handle the engine,
shell, and API code use. Capability gating is data-driven:

- `BrowserCapabilities` flags (`src/actions/registry.rs`): `pdf`, `file_input`,
  `file_chooser`, `console_events`, `network_events`, `crash_events`
- Per-action checks (e.g. `upload` requires `file_input`) fail with
  `ActionError::Unsupported` on backends that lack the flag
- Process lifecycle: every backend is wrapped in a kill-on-drop guard so the
  last adapter drop kills and waits for the spawned child (no orphaned
  firefox/lightpanda processes); per-run temp profiles (`/tmp/automodus-workflow-<pid>`)
  are removed by the `TempProfile` guard on CLI exit

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

`{{variable}}` interpolation in step fields (`src/core/template.rs`), resolving
from a strict root allow-list:

- `params.*` — workflow parameters
- `vars.*` — workflow/loop variables (loops bind `vars.<as>` / `vars.<index_as>`)
- `store.*` — values saved by previous steps
- `steps.<id>.*` — outputs of a named step
- `env.*` — environment variables
- `instance.id`, `timestamp`, `workflow.name`, `workflow.id`
- a bare `{{name}}` falls back to a `store` lookup; the only filter is `| json`

## Component Details

### AppCore (`src/core/app.rs`)

Central shared state owned by the daemon:

```
AppCore
├── page_adapter: Mutex<Option<SessionAdapter>>   # Lazily launched backend
├── headless: bool
├── engine: BrowserEngine                         # chromium | firefox | lightpanda
├── sessions: Arc<RwLock<SessionStore>>           # Session CRUD
├── workflows: Arc<RwLock<HashMap>>               # Workflow cache
├── debug_config: Arc<RwLock<ResolvedDebugConfig>># Global debug
├── event_tx: broadcast::Sender<CoreEvent>        # Event bus (WS)
└── config: debug_dir, max_sessions, session_idle_timeout
```

Key methods:
- `get_page()` — lazy launch via `launch_session()`, returns `SessionAdapter`
- `create_session()` / `close_session()` / `list_sessions()`
- `find_session(id_or_name)` — lookup by UUID or friendly name
- `cleanup_idle_sessions()` — removes non-keep-alive sessions past timeout

### Daemon (`src/daemon/mod.rs`)

Long-running background process:

```
Daemon::start()
├── Write PID file (<data-dir>/.automodus/daemon.pid)
├── Bind Unix socket (<data-dir>/.automodus/automodus.sock)
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
| Interaction | `click`, `type`, `wait`, `eval`, `text`, `find` |
| Capture | `screenshot`, `highlight`, `pdf` |
| Tabs | `tabs`, `tab new/switch/close` |
| Workflows | `run`, `list`, `trace` |
| Sessions | `session new/list/switch/close/info/keep-alive` |
| Debug | `debug on/off/status/clean` |
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
- **Console/Network**: CDP event listeners (`start_console_listener`, `start_network_listener`) on the Chromium adapter — **Chromium-only** (Firefox returns no-op; Lightpanda capability-gated off). Entries are buffered and emitted to the `tracing` debug log; surfacing them in API/shell output is not yet wired (see [DEBUG.md](DEBUG.md))
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
| `config/app.toml` | Workspace | Server host/port, browser settings, workflow dir (`AUTOMODUS_CONFIG` overrides the path) |
| `~/.automodus/daemon.toml` | User | Daemon socket/PID paths, HTTP config, limits — **loader currently unused**: the daemon runs on `DaemonConfig::default()` paths (see Known Debt) |
| `~/.local/share/automodus/history.txt` | User | Shell command history |
| `<data-dir>/.automodus/daemon.pid` | Runtime | Daemon process ID (default `~/.local/share/.automodus/`) |
| `<data-dir>/.automodus/automodus.sock` | Runtime | Unix domain socket |
| `<data-dir>/.automodus/daemon.log` | Runtime | Daemon log output |

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `AUTOMODUS_CONFIG` | Config file path (default: `config/app.toml`) |
| `AUTOMODUS_WORKFLOWS` | Workflows directory (default: `workflows/`) |
| `AUTOMODUS_CHROME_PATH` / `AUTOMODUS_FIREFOX_PATH` / `AUTOMODUS_LIGHTPANDA_PATH` | Browser binary overrides |
| `AUTOMODUS_SOCKET_PATH` / `AUTOMODUS_HTTP_HOST` / `AUTOMODUS_HTTP_PORT` / `AUTOMODUS_MAX_SESSIONS` / `AUTOMODUS_LOG_LEVEL` | Daemon overrides |
| `RUST_LOG` | Log level filter |
| `AUTOMODUS_DEBUG` | Enable debug mode (`1`, `true`, `yes`, `on`) |
| `AUTOMODUS_DEBUG_LEVEL` | Debug log level (`info`, `debug`, `trace`) |
| `AUTOMODUS_DEBUG_PROFILE` | Debug preset (`minimal`, `verbose`, `ci`, `demo`) |
| `AUTOMODUS_DEBUG_DELAY` | Step delay in milliseconds |
| `AUTOMODUS_DEBUG_CAPTURE` | Screenshot mode (`none`, `failure`, `before`, `after`, `all`) |

(`AUTOMODUS_DEBUG*` are read only by `automodus run`; any `AUTOMODUS_*` name can
also override a `config/app.toml` key.)

## CLI Commands

```
automodus run <workflow.yaml> [key=value ...] [--debug] [--debug=level] [--delay=ms]
                                [--capture=mode] [--profile=name] [--highlight]
                                [--pause] [--console] [--network] [--keep-open]
automodus shell                     # Interactive REPL
automodus serve                     # [DEPRECATED] Start HTTP server (use daemon start)
automodus daemon start|stop|status|restart|logs [-f|--lines=n]
automodus validate [path]           # Validate workflow YAML files (exit 1 if invalid)
automodus list                      # List loaded workflows
automodus help                      # Show help
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
| GET/POST | `/api/browser/tabs` | List / open tabs |
| POST | `/api/browser/tabs/switch` | Switch tab |
| DELETE | `/api/browser/tabs/:index` | Close tab |
| GET | `/api/browser/pdf` | Render page as PDF (Chromium-only) |
| POST | `/api/debug/cleanup` | Prune old debug artifacts |
| GET | `/api/executions` | List executions |
| GET | `/api/executions/:id` | Execution details |
| DELETE | `/api/executions/:id` | Cancel execution |
| WS | `/ws` | Real-time events |

Swagger UI available at `/swagger-ui`; OpenAPI JSON at `/api/openapi.json`.
Workflow validation is CLI-only (`automodus validate`).

## Error Handling

Two error types unified in `src/error.rs`:

- **`AutomodusError`** — library-level errors (browser, workflow, IO, daemon, session)
- **`AppError`** — API-facing errors with `ErrorCode` enum for structured responses

`ErrorCode` variants: `DaemonNotRunning`, `DaemonAlreadyRunning`, `DaemonConnectionFailed`, `SessionNotFound`, `SessionLimitReached`, `WorkflowNotFound`, `WorkflowInvalid`, `WorkflowTimeout`, `ExecutionFailed`, `ExecutionCancelled`, `StepFailed`, `SelectorNotFound`, `SelectorTimeout`, `NavigationFailed`, `BrowserLaunchFailed`, `BrowserDisconnected`, `InvalidRequest`, `InternalError`.

API errors return:
```json
{ "error": { "code": "SELECTOR_NOT_FOUND", "message": "Element not found: #btn" } }
```

## Validation, Testing & CI

- **Validation** (`WorkflowParser::validate`): parse → schema checks →
  registry-backed action names (with aliases + `loop`/`call`/`condition`) →
  required-parameter rules → recursive checks of nested step lists (condition
  branches, loop bodies, `on_success`/`on_failure` handler steps) and `goto`
  targets. `automodus validate [path]` exits `1` if anything is invalid.
- **Tests**: `cargo test --features mcp` — unit + integration/session/shell/daemon suites.
- **CI** (`.github/workflows/ci.yml`, pushes to `main`/`develop` and PRs):
  fmt (`cargo fmt --all -- --check`), clippy (`--features mcp -- -D warnings`),
  build + tests, and an **engine smoke matrix** (chromium, firefox, lightpanda)
  that runs `scripts/smoke.sh <engine>`: validates the `examples/` submodule,
  then runs each standalone example workflow under Xvfb with per-engine skips
  (e.g. multi-tab on Lightpanda), timeouts, and orphan/profile-leak checks.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `chromiumoxide` | Chromium CDP automation |
| `rustenium` (+ `rustenium-bidi-definitions`) | Firefox WebDriver BiDi (no geckodriver) |
| `tokio` | Async runtime |
| `axum` | HTTP framework |
| `rustyline` | Readline/history for shell |
| `serde` / `serde_yaml` / `serde_json` | Serialization |
| `tracing` | Structured logging |
| `uuid` | Session/execution IDs |
| `chrono` | Timestamps |
| `thiserror` / `anyhow` | Error handling |
| `utoipa` / `utoipa-swagger-ui` | OpenAPI documentation |
| `tokio-tungstenite` | WebSocket support |
| `rusqlite` | SQLite for execution history |
| `reqwest` | HTTP client for outbound requests |
| `glob` | Workflow file discovery |
| `regex` | Template pattern matching |
| `config` / `toml` | AppConfig loading |
| `dirs` | Platform data/config directories |

## Known Architectural Debt

- **Daemon TOML config loader is dead code**: `daemon::config::load_config()` /
  `ensure_config_exists()` are exported but never called — the daemon runs on
  `DaemonConfig::default()`, so `~/.automodus/daemon.toml` is not read.
- **Trigger declarations are schema-only**: `on:` (api/schedule/event/webhook/watch)
  validates and shows in `list`, but nothing dispatches them.
- **Debug config only merges on the `automodus run` path**: shell/API executions
  pass `ResolvedDebugConfig::default()`; step-level `debug:` still applies
  everywhere via the engine. Console/network captures are buffered but not
  surfaced in API/shell/WS output.
- **`on_error`/`on_complete` emit events have no subscribers** (event bus exists;
  nothing reacts to events yet).

### Resolved Debt

- ~~Shell runs standalone~~ — Shell now auto-detects running daemon and routes commands via Unix socket protocol (`handle_socket_connection`, `dispatch_request`, `DaemonClient`).
- ~~Dual session stores~~ — `ServerSession` removed. `ServerState` delegates to `AppCore`'s `SessionStore` as single source of truth.
- ~~Console/network capture uses JS injection~~ — Replaced with native CDP event listeners (`EventConsoleApiCalled`, `EventRequestWillBeSent`/`EventResponseReceived`) via `start_console_listener()` and `start_network_listener()`.
- ~~Pause handlers are partial~~ — `ShellPauseHandler` (stdin-based) wired into CLI and shell. `WebSocketPauseHandler` (oneshot channel) wired into API. Both support Continue/Skip/Abort.

See [ISSUES.md](ISSUES.md) for the issue tracker.
