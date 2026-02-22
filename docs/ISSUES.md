# Issue Tracker

Track implementation progress for automodus. Update status as work progresses.

**Legend:** ⬜ Not Started | 🟡 In Progress | ✅ Done | ❌ Blocked

**Summary:** 34 issues across 8 priorities

| Priority | Issues | Description | Est. Total |
|----------|--------|-------------|------------|
| **P1** | #1-4 | Debug schema (foundation) | ~1 day |
| **P2** | #5-8 | Engine debug integration | ~1.5 days |
| **P3** | #9-10 | CDP listeners | ~1 day |
| **P4** | #11-15b | Daemon architecture | ~2 weeks |
| **P5** | #16-19 | Shell refactor | ~1 week |
| **P6** | #20-23 | API enhancement | ~1 week |
| **P7** | #24-27 | Debug UX | ~2 days |
| **P8** | #31-33 | Integration tests | ~1 day |
| **Backlog** | #29-30 | Future work | - |

---

## Priority 1: Debug Schema (Foundation)

Low-risk, immediate value. Do first before daemon architecture.

### Issue #1: Add DebugConfig to workflow schema
**Status:** ✅ Done  
**File:** `src/workflow/schema.rs`  
**Estimate:** 2-4 hours

**Tasks:**
- [x] Add `DebugConfig` struct with `Option<T>` fields
- [x] Add `LogLevel` enum (Info, Debug, Trace)
- [x] Add `CaptureMode` enum (None, Failure, Before, After, All)
- [x] Add `DebugProfile` enum (Minimal, Verbose, Ci, Demo)
- [x] Add `ResolvedDebugConfig` struct (concrete values)
- [x] Implement `DebugConfig::merge()` and `DebugConfig::resolve()`
- [x] Implement `DebugConfig::with_profile()`

**Acceptance:**
```rust
let cfg = DebugConfig { profile: Some(DebugProfile::Ci), capture: Some(CaptureMode::All), ..Default::default() };
let resolved = cfg.with_profile().resolve();
assert_eq!(resolved.capture, CaptureMode::All); // explicit overrides profile
```

---

### Issue #2: Add debug field to Workflow and Step
**Status:** ✅ Done  
**File:** `src/workflow/schema.rs`  
**Depends:** #1  
**Estimate:** 1 hour

**Tasks:**
- [x] Add `#[serde(default)] pub debug: DebugConfig` to `Workflow`
- [x] Add `#[serde(default)] pub debug: Option<DebugConfig>` to `Step`
- [x] Add unit test for YAML parsing with debug section
- [x] Verify backwards compatibility (existing workflows still parse)

**Acceptance:**
```yaml
name: test
debug:
  profile: verbose
steps:
  - action: click
    selector: "#btn"
    debug:
      pause: true
```

---

### Issue #3: CLI debug flags
**Status:** ✅ Done  
**File:** `src/bin/automodus.rs`  
**Depends:** #1  
**Estimate:** 2-3 hours

**Tasks:**
- [x] Add `--debug` flag to `run` command
- [x] Add `--debug=<level>` variant (info/debug/trace)
- [x] Add `--delay=<ms>` flag
- [x] Add `--capture=<mode>` flag
- [x] Add `--profile=<name>` flag
- [x] Parse flags into `DebugConfig`
- [x] Merge CLI config with workflow config
- [x] Add `--highlight`, `--pause`, `--console`, `--network` flags

**Acceptance:**
```bash
automodus run workflow.yaml --debug --delay=500 --capture=all
automodus run workflow.yaml --debug=trace --profile=ci
```

---

### Issue #4: Environment variable debug config
**Status:** ✅ Done  
**File:** `src/bin/automodus.rs`  
**Depends:** #1  
**Estimate:** 1 hour

**Tasks:**
- [x] Read `AUTOMODUS_DEBUG` (bool)
- [x] Read `AUTOMODUS_DEBUG_LEVEL` (info/debug/trace)
- [x] Read `AUTOMODUS_DEBUG_PROFILE` (profile name)
- [x] Read `AUTOMODUS_DEBUG_DELAY` (ms)
- [x] Read `AUTOMODUS_DEBUG_CAPTURE` (mode)
- [x] Merge with CLI/workflow config (lowest precedence)

**Acceptance:**
```bash
AUTOMODUS_DEBUG=true AUTOMODUS_DEBUG_LEVEL=trace automodus run workflow.yaml
```

---

## Priority 2: Engine Debug Integration

Wire debug config through execution.

### Issue #5: Thread DebugConfig through ExecutionContext
**Status:** ✅ Done  
**File:** `src/core/context.rs`, `src/core/engine.rs`  
**Depends:** #1, #2  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Add `debug: ResolvedDebugConfig` field to `ExecutionContext`
- [x] Add `with_debug()` builder method to context
- [x] Merge workflow + step debug configs in engine
- [x] Pass resolved config to each step execution
- [x] Add delay between steps when `delay > 0`
- [x] Add `execute_with_debug()` method to engine

**Acceptance:**
```rust
// In engine, before step execution:
if ctx.debug.delay > 0 {
    tokio::time::sleep(Duration::from_millis(ctx.debug.delay)).await;
}
```

---

### Issue #6: Screenshot capture on failure
**Status:** ✅ Done  
**File:** `src/core/engine.rs`  
**Depends:** #5  
**Estimate:** 2-3 hours

> **Note:** `ErrorHandler::screenshot: Option<bool>` already exists in `schema.rs` but isn't wired to execution.

**Tasks:**
- [x] Create `data/debug/` directory on init
- [x] Wire existing `ErrorHandler::screenshot` to debug system
- [x] Capture screenshot when step fails and `capture != None`
- [x] Implement filename format: `{timestamp}_{workflow}_step{n}_{phase}.png`
- [x] Add `debug_screenshots: Vec<String>` to `WorkflowResult`
- [x] Capture before/after screenshots based on CaptureMode

**Acceptance:**
- Step fails → screenshot saved to `data/debug/20260222_150000_myworkflow_step3_failure.png`

---

### Issue #7: Debug directory cleanup
**Status:** ✅ Done  
**File:** `src/utils/debug.rs`  
**Depends:** #6  
**Estimate:** 1-2 hours

**Implemented:**
- `CleanupPolicy` with `max_age`, `max_files`, `max_size_bytes` options
- `cleanup_debug_dir()` — three-phase cleanup (age → count → size)
- `debug clean` shell command (standalone + daemon)
- `POST /api/debug/cleanup` API endpoint
- `DebugClean` daemon protocol message
- Engine now uses configurable `debug_dir` instead of hardcoded path
- 5 unit tests for cleanup logic

---

### Issue #8: Element highlighting
**Status:** ✅ Done  
**File:** `src/core/engine.rs`  
**Depends:** #5  
**Estimate:** 2-3 hours

**Tasks:**
- [x] Add `highlight_element()` method to engine
- [x] Inject highlight JS before interaction when `highlight: true`
- [x] Brief pause (300ms) then remove highlight
- [x] Auto-scroll element into view
- [x] Call from execute_step for actions with selectors

---

## Priority 3: CDP Listeners

Browser console and network capture.

### Issue #9: Console capture via CDP
**Status:** ✅ Done  
**File:** `src/modules/browser/adapter.rs`  
**Depends:** #5  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Add ConsoleEntry struct with format() method
- [x] Add console_logs storage to ChromePageAdapter
- [x] Add capture_console_logs() as CDP buffer drain
- [x] Add get_console_logs() and clear_console_logs()
- [x] Setup real CDP `ConsoleAPICalledEvent` listener via `start_console_listener()`

**Output format:**
```
[CONSOLE] 14:32:01.123 LOG: message
[CONSOLE] 14:32:01.456 WARN: warning
[CONSOLE] 14:32:02.789 ERROR: error
```

---

### Issue #10: Network capture via CDP
**Status:** ✅ Done  
**File:** `src/modules/browser/adapter.rs`  
**Depends:** #5  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Add NetworkEntry struct with format() method
- [x] Add network_logs storage to ChromePageAdapter
- [x] Add capture_network_logs() as CDP buffer drain
- [x] Add get_network_logs() and clear_network_logs()
- [x] Setup real CDP `ResponseReceivedEvent` + `RequestWillBeSent` listener via `start_network_listener()`
- [x] Correlate request/response by `request_id` (method, url, status, duration)

**Output format:**
```
[NETWORK] GET https://api.example.com/user → 200 (45ms)
[NETWORK] POST https://api.example.com/login → 401 (120ms)
```

---

## Priority 4: Daemon Architecture

Foundation for persistent sessions. Largest effort.

### Issue #11: Create Daemon struct
**Status:** ✅ Done  
**File:** `src/daemon/mod.rs` (new)  
**Estimate:** 2-3 days

> **Note:** Unix socket + PID management has edge cases. Budget extra time.

**Tasks:**
- [x] Create `src/daemon/mod.rs`
- [x] Implement `Daemon` struct per SHELL.md
- [x] PID file management (`~/.automodus/daemon.pid`)
- [x] Log file (`~/.automodus/daemon.log`)
- [x] Unix socket listener (`~/.automodus/automodus.sock`)
- [x] Graceful shutdown handling (SIGTERM, SIGINT)
- [x] Stale PID file detection and cleanup

---

### Issue #12: Create AppCore struct
**Status:** ✅ Done  
**File:** `src/core/app.rs` (new)  
**Depends:** #11  
**Estimate:** 4-6 hours

**Tasks:**
- [x] Create `src/core/app.rs`
- [x] Session management with create/get/close/list
- [x] Add `RwLock` for workflow cache
- [x] Add `RwLock<DebugConfig>`
- [x] Add `broadcast::Sender<CoreEvent>` for events
- [x] Debug config resolution with merge

---

### Issue #13: Create SessionManager
**Status:** ✅ Done  
**File:** `src/core/app.rs` (integrated into AppCore)  
**Depends:** #12  
**Estimate:** 1.5-2 days

> **Note:** Browser lifecycle has edge cases (crash recovery, zombie processes).
> **Audit (2026-02-22):** Session CRUD works. Browser launch was duplicated.
> **Fix (2026-02-22):** All browser launch sites now use `modules::browser::launch` helpers.
> AppCore owns the browser; ServerState delegates via `core.get_page()`. Idle cleanup implemented.

**Tasks:**
- [x] Session management integrated into `src/core/app.rs`
- [x] Extract browser launch logic from `api/state.rs` and `bin/automodus.rs`
- [x] Implement `Session` struct with `keep_alive` field
- [x] Implement session create/get/close/list in AppCore
- [x] Implement `cleanup_idle_sessions()` for idle timeout
- [x] Single source of truth for browser lifecycle (AppCore)
- [x] Handle browser crash/disconnect gracefully (`start_crash_listener()`, `EventTargetCrashed`, `check_browser_health()`)

**Unified browser launch:**
- `src/modules/browser/launch.rs` — single launch helper used everywhere
- `src/core/app.rs` — AppCore owns browser + page adapter
- `src/api/state.rs` — ServerState delegates `get_page()` to AppCore

---

### Issue #14: Daemon CLI commands
**Status:** ✅ Done  
**File:** `src/bin/automodus.rs`  
**Depends:** #11  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Add `daemon start` command (background, `-f` for foreground)
- [x] Add `daemon stop` command
- [x] Add `daemon status` command
- [x] Add `daemon restart` command
- [x] Add `daemon logs` command (with `-f` follow, `--lines=N`)

**Acceptance:**
```bash
automodus daemon start
automodus daemon status  # → "Daemon running (PID 12345)"
automodus daemon stop
```

---

### Issue #14a: Unify error types
**Status:** ✅ Done  
**File:** `src/error.rs`  
**Depends:** #11  
**Estimate:** 2-3 hours

> **Note:** Moved from backlog - daemon needs `ErrorCode` enum for proper API responses.

**Tasks:**
- [x] Merge `AutomodusError` and proposed `AppError`
- [x] Add `ErrorCode` enum per SHELL.md spec
- [x] Add daemon-specific codes: `DaemonNotRunning`, `DaemonAlreadyRunning`, `DaemonConnectionFailed`
- [x] Implement `From<AutomodusError>` for API error response
- [x] Update all error handling sites

---

### Issue #15: Merge HTTP server into daemon
**Status:** ✅ Done  
**File:** `src/daemon/mod.rs`, `src/api/server.rs`  
**Depends:** #11, #12  
**Estimate:** 4-6 hours

> **Audit (2026-02-22):** HTTP server CAN start inside `Daemon::run()`, but `serve` command
> still calls `api::run_server()` directly (not daemon).
> **Fix (2026-02-22):** `serve` now routes through `Daemon::start()` + `Daemon::run()`.
> `ServerState` delegates browser to `AppCore` via `create_state_with_core()`. Daemon passes
> its `AppCore` to the HTTP server, eliminating parallel state.

**Tasks:**
- [x] Add `enable_http` config option to DaemonConfig
- [x] Move HTTP server startup into `Daemon::run()`
- [x] Share shutdown signal between socket and HTTP handlers
- [x] Update `serve` command to start daemon (backward compat)
- [x] ServerState delegates to AppCore (shared browser)

---

### Issue #15a: Daemon config file loading
**Status:** ✅ Done  
**File:** `src/daemon/config.rs` (new)  
**Depends:** #11  
**Estimate:** 2-3 hours

**Tasks:**
- [x] Create `DaemonConfig` struct matching SHELL.md spec
- [x] Load from `~/.automodus/daemon.toml` if exists
- [x] Fall back to defaults
- [x] Validate config values (port ranges, paths)
- [x] Create default config file on first run

**Config locations:**
- `~/.automodus/daemon.toml` (user global)
- `config/app.toml` (workspace override)

---

### Issue #15b: Backward compatibility for serve command
**Status:** ✅ Done  
**File:** `src/bin/automodus.rs`  
**Depends:** #14, #15  
**Estimate:** 1 hour

> **Audit (2026-02-22):** `serve` used to call `api::run_server()` directly.
> **Fix (2026-02-22):** Now calls `daemon.start()` + `daemon.run()` with HTTP enabled.

**Tasks:**
- [x] Keep `automodus serve` command working
- [x] Internally start daemon in foreground with HTTP enabled
- [x] Print deprecation warning
- [x] Document in help text

---

## Priority 5: Shell Refactor

Stateless shell connecting to daemon.

### Issue #16: Create ShellClient
**Status:** ✅ Done  
**File:** `src/shell/client.rs` (new)  
**Depends:** #11, cargo deps (rustyline, dirs)  
**Estimate:** 1 day

> **User pain point:** Current shell uses `std::io::BufRead` - no arrow key history, no line editing.

**Tasks:**
- [x] Create `src/shell/mod.rs` and `src/shell/client.rs`
- [x] Add rustyline for readline support
- [x] **Arrow up/down cycles through command history**
- [x] **Home/End, Ctrl+A/E for line navigation**
- [x] **Ctrl+R for reverse history search**
- [x] Implement history save/load (`~/.local/share/automodus/history.txt`)
- [x] Add command completion for commands and workflow paths
- [x] **Wire ShellClient into `bin/automodus.rs::run_shell()`**
- [x] Implement daemon connection via Unix socket
- [x] Replace inline browser launch with daemon commands
- [x] Create `src/daemon/protocol.rs` with typed `SocketRequest`/`SocketResponse` enums
- [x] Length-prefixed JSON framing (4-byte big-endian + JSON payload)
- [x] Server-side `handle_socket_connection()` + `dispatch_request()` in `daemon/mod.rs`
- [x] `DaemonClient` typed methods (ping, session_*, browser_*, workflow_*)
- [x] Shell auto-detects running daemon; falls back to standalone mode
- [x] Unified session store: `ServerState` delegates to `AppCore` (removed `ServerSession` + dual HashMap)

> **✅ Merge note (2026-02-23):** `feature/sub-workflows` was rebased onto main and merged via PR #1.
> All socket-protocol work preserved. Sub-workflow `call` action integrated with `cancel_token` threading.

**Acceptance:**
```
$ automodus shell
automodus> goto https://example.com
automodus> click "#btn"
automodus> [UP ARROW]  # Shows: click "#btn"
automodus> [UP ARROW]  # Shows: goto https://example.com
```

---

### Issue #17: Shell command expansion
**Status:** ✅ Done  
**File:** `src/shell/client.rs`, `src/bin/automodus.rs`  
**Depends:** #16  
**Estimate:** 1-2 days

**Tasks:**
- [x] Add `click <selector>` command
- [x] Add `type <selector> <text>` command  
- [x] Add `wait <selector> [timeout]` command
- [x] Add `screenshot [path]` command
- [x] Add `eval <js>` command
- [x] Add `text <selector>` command
- [x] Add `find <selector>` command
- [x] Add navigation: `back`, `forward`, `refresh`
- [x] Wire commands to browser adapter execution

---

### Issue #18: Shell autocomplete
**Status:** ✅ Done  
**File:** `src/shell/client.rs`  
**Depends:** #16  
**Estimate:** 3-4 hours

> **Audit (2026-02-22):** `ShellCompleter` implements `Completer` trait with command name
> and workflow path completion. Missing: session name and file path completion.
> **Fix (2026-02-22):** Added `complete_session_subcommand()`, `complete_session_name()`,
> and `complete_file_path()`. Session names dynamically updated via `Arc<RwLock<Vec<String>>>`.
> File path completion supports directory traversal for `screenshot` command.

**Tasks:**
- [x] Implement `Completer` trait for rustyline
- [x] Command name completion
- [x] Workflow name completion for `run`
- [x] Session name completion for session commands
- [x] File path completion for `screenshot`

---

### Issue #19: Session shell commands
**Status:** ✅ Done  
**File:** `src/shell/client.rs`, `src/bin/automodus.rs`, `src/core/app.rs`  
**Depends:** #13, #16  
**Estimate:** 2-3 hours

> **Fix (2026-02-22):** All 6 session commands implemented with full execution wiring.
> Shell creates AppCore for session management. Prompt shows active session name.
> Added `find_session()` (by ID or name), `set_session_keep_alive()` to AppCore.
> Shortcuts: `sess=session`, `sw=switch`, `ka=keep-alive`, `ls=list`.

**Tasks:**
- [x] Add `session new [--name=NAME] [--keep-alive]`
- [x] Add `session list`
- [x] Add `session switch <id|name>`
- [x] Add `session close [id]`
- [x] Add `session info`
- [x] Add `session keep-alive [id] [on|off]`

---

## Priority 6: API Enhancement

Full API parity with shell.

### Issue #20: Session API endpoints
**Status:** ✅ Done  
**File:** `src/api/handlers.rs`, `src/api/state.rs`  
**Depends:** #13  
**Estimate:** 3-4 hours

**Tasks:**
- [x] `POST /api/sessions` - create session (with `keep_alive` option)
- [x] `GET /api/sessions` - list sessions
- [x] `GET /api/sessions/:id` - get session info
- [x] `DELETE /api/sessions/:id` - close session
- [x] Add to OpenAPI schema

---

### Issue #21: Browser control API endpoints
**Status:** ✅ Done  
**File:** `src/api/handlers.rs`  
**Depends:** #12  
**Estimate:** 4-6 hours

**Tasks:**
- [x] `POST /api/browser/click` - click element
- [x] `POST /api/browser/type` - type into element
- [x] `POST /api/browser/wait` - wait for element
- [x] `POST /api/browser/eval` - execute JavaScript
- [x] `GET /api/browser/page` - get page info
- [x] Add to OpenAPI schema

---

### Issue #22: Execution tracking endpoints
**Status:** ✅ Done  
**File:** `src/api/handlers.rs`, `src/api/state.rs`  
**Depends:** #12  
**Estimate:** 3-4 hours

**Tasks:**
- [x] `GET /api/executions` - list recent executions
- [x] `GET /api/executions/:id` - get execution details
- [x] `DELETE /api/executions/:id` - cancel execution
- [x] `GET /api/executions/:id/output` - get execution output (merged into detail)
- [x] Store execution history in memory/sqlite

---

### Issue #23: WebSocket implementation
**Status:** ✅ Done  
**File:** `src/api/ws.rs` (new)  
**Depends:** #12, cargo deps (tokio-tungstenite)  
**Estimate:** 1.5-2 days

> **Note:** Protocol handling and client reconnection need care.

**Tasks:**
- [x] Create `src/api/ws.rs`
- [x] Implement `/ws` endpoint
- [x] Subscribe to `AppCore::events` broadcast
- [x] Forward execution events to connected clients
- [x] Handle client commands (continue, skip, abort)
- [x] Version protocol messages

**Events:**
```json
{ "version": 1, "type": "execution.started", "id": "...", "workflow": "..." }
{ "version": 1, "type": "execution.step", "id": "...", "step": 1, "action": "click" }
{ "version": 1, "type": "execution.complete", "id": "...", "success": true }
{ "version": 1, "type": "execution.paused", "id": "...", "step": 3 }
```

---

## Priority 7: Debug UX

Final debug features.

### Issue #24: Shell debug commands
**Status:** ✅ Done  
**File:** `src/shell/client.rs`  
**Depends:** #16, #5  
**Estimate:** 2-3 hours

**Tasks:**
- [x] Add `debug on [--profile=PROFILE]`
- [x] Add `debug off`
- [x] Add `debug status`
- [x] Add `highlight <selector>`
- [x] Add `trace <workflow>` (alias for `run --debug=trace`)

---

### Issue #25: Pause implementation (shell)
**Status:** ✅ Done (foundation)  
**File:** `src/shell/client.rs`, `src/core/engine.rs`  
**Depends:** #5, #16  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Detect `pause: true` in step debug config
- [x] Add PauseHandler trait to engine
- [x] Add PauseResponse enum (Continue, Skip, Abort)
- [x] Integrate pause handling into execute_steps
- [ ] Implement ShellPauseHandler (interactive stdin prompt) - deferred
- [ ] Handle stdin in shell context - deferred

---

### Issue #26: Pause implementation (API/WebSocket)
**Status:** ✅ Done (foundation)  
**File:** `src/api/ws.rs`, `src/core/engine.rs`  
**Depends:** #23, #5  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Add execution.paused event type
- [x] PauseHandler trait for async pause response
- [x] DefaultPauseHandler that continues automatically
- [ ] WebSocketPauseHandler implementation - deferred
- [ ] Wait for client command (`continue`, `skip`, `abort`) - deferred
- [ ] Timeout handling for unresponsive clients - deferred

---

### Issue #27: Trace-level JSONL output
**Status:** ✅ Done  
**File:** `src/utils/trace.rs` (new)  
**Depends:** #5  
**Estimate:** 2-3 hours

**Tasks:**
- [x] Create `data/debug/trace.jsonl` when `level: trace`
- [x] Log selector resolution details
- [x] Log element info (tag, role, text, attributes)
- [x] Log injected JS
- [x] Append line per event
- [x] TraceLogger struct with log methods
- [x] ElementInfo struct for element details

---

## Backlog / Future

### Issue #28: Sub-workflow composition
**Status:** ✅ Done  
**File:** `src/core/engine.rs`, `src/workflow/loader.rs`, `src/workflow/mod.rs`  
**Estimate:** 1-2 days

**Tasks:**
- [x] Add `WorkflowResolver` trait (`async fn resolve(&self, name: &str) -> Result<Workflow>`)
- [x] Implement `WorkflowLoader` as filesystem-based resolver
- [x] Add `call_depth` tracking to `ExecutionContext` (MAX_CALL_DEPTH = 16)
- [x] Implement `execute_call_action()` — sub-workflow invocation with param forwarding
- [x] Add `with_resolver()` constructor to `WorkflowEngine`
- [x] Wire `WorkflowLoader` as resolver in CLI and shell entry points
- [x] Thread `cancel_token` through recursive `execute_call_action` calls
- [x] Add 7 unit tests (basic, nested, no-resolver, not-found, max-depth, params, condition)
- [x] Add 3 example composition workflows (`workflows/examples/compose/`)
- [x] Add `ARCHITECTURE.md` documentation

**Workflow syntax:**
```yaml
steps:
  - action: call
    workflow: sub_workflow_name
    params:
      key: "{{params.value}}"
    store_as: result
```

---

### Issue #29: Tab management
**Status:** ✅ Done  
**File:** `src/modules/browser/actions/tabs.rs`  
**Estimate:** 4-6 hours

**Tasks:**
- [x] `tabs` - list open tabs
- [x] `tab <index>` - switch to tab
- [x] `tab new [url]` - open new tab
- [x] `tab close [index]` - close tab
- [x] Wire through shell and API
- [x] `TabListAction` workflow action
- [x] Daemon protocol (`BrowserTabList`, `BrowserTabNew`, `BrowserTabSwitch`, `BrowserTabClose`)
- [x] API endpoints (`GET/POST /api/browser/tabs`, `POST /api/browser/tabs/switch`, `DELETE /api/browser/tabs/:index`)

---

### Issue #30: PDF export
**Status:** ✅ Done  
**File:** `src/modules/browser/adapter.rs`  
**Estimate:** 2-3 hours

**Tasks:**
- [x] Add `pdf [path]` shell command
- [x] Add `GET /api/browser/pdf` endpoint
- [x] Use CDP `Page.printToPDF`
- [x] `BrowserPdf` daemon protocol message
- [x] `pdf()` method on `BrowserHandle` trait

---

## Priority 8: Integration Tests

> **Note:** Run these after each phase to catch regressions.

### Issue #31: Daemon integration tests
**Status:** ✅ Done  
**File:** `tests/daemon_tests.rs` (new)  
**Depends:** #11, #14  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Test daemon start/stop lifecycle
- [x] Test PID file creation and cleanup
- [x] Test stale PID detection
- [x] Test socket communication
- [x] Test graceful shutdown

---

### Issue #32: Shell-daemon integration tests  
**Status:** ✅ Done  
**File:** `tests/shell_tests.rs` (new)  
**Depends:** #16, #31  
**Estimate:** 3-4 hours

**Tasks:**
- [x] Test shell connects to running daemon
- [x] Test shell fails gracefully when daemon not running
- [x] Test commands route through daemon
- [x] Test browser survives shell exit
- [x] Test multiple shells can connect

---

### Issue #33: Session persistence tests
**Status:** ✅ Done  
**File:** `tests/session_tests.rs` (new)  
**Depends:** #13, #31  
**Estimate:** 2-3 hours

**Tasks:**
- [x] Test session survives shell disconnect
- [x] Test session idle timeout
- [x] Test `keep_alive` prevents timeout
- [x] Test auth state (cookies) persists

---

## Revision History

| Date | Changes |
|------|---------|
| 2026-02-22 | Initial issue list from SHELL.md and DEBUG.md review |
| 2026-02-22 | Codebase audit: corrected #13 → 🟡, #15 → 🟡, #15b → 🟡, #16 → 🟡 (not wired), #18 → 🟡 (basic done) |
| 2026-02-22 | #13 ✅: Unified browser lifecycle (AppCore owns browser, deduped launch), idle session cleanup |
| 2026-02-22 | #15 ✅: ServerState delegates to AppCore, daemon passes shared core to HTTP server |
| 2026-02-22 | #15b ✅: `serve` routes through daemon.start()+run() instead of api::run_server() |
| 2026-02-22 | #19 ✅: Session shell commands (6 commands + parsing + execution wiring + AppCore lookup methods) |
| 2026-02-22 | #18 ✅: Shell autocomplete (session name + file path + session subcommand completion) |
| 2026-02-22 | #16 deferred items ✅: Socket protocol (`daemon/protocol.rs`), server handler, DaemonClient typed methods |
| 2026-02-22 | Session store unified: removed `ServerSession`, `ServerState.sessions` delegates to `AppCore` |
| 2026-02-22 | Shell wired to daemon: `run_shell()` auto-detects daemon, `run_shell_daemon()` routes via socket |
| 2026-02-23 | #9: Updated task text — console capture uses CDP `ConsoleAPICalledEvent` listener (not JS injection) |
| 2026-02-23 | #10: Upgraded status from "Done (basic)" to "Done" — full CDP `ResponseReceivedEvent` + `RequestWillBeSent` with request/response correlation |
| 2026-02-23 | #13: Removed "(deferred)" from crash recovery task — fully implemented via `start_crash_listener()` + `EventTargetCrashed` |
| 2026-02-23 | #16: Updated merge note — `feature/sub-workflows` rebased and merged via PR #1 |
| 2026-02-23 | Added Issue #28: Sub-workflow composition (WorkflowResolver, call action, ARCHITECTURE.md) |
| 2026-02-23 | Audit: Verified all 34 issues match codebase reality |

---

## Quick Reference: Implementation Order

```
Phase 0 (Debug Schema):     #1 → #2 → #3 → #4
Phase 1a (Daemon Core):     #11 → #12 → #14 → #14a (errors)
Phase 1a+ (Config):         #15a (daemon config)
Phase 1b (Sessions):        #13 → #15 → #15b (serve compat)
Phase 1c (Shell):           #16 → #17 → #18 → #19
Phase 1d (Composition):     #28 (sub-workflows)
Phase 1-Tests:              #31 → #32 → #33
Phase 2 (Engine Debug):     #5 → #6 → #7 → #8
Phase 3 (CDP):              #9 → #10
Phase 4 (API):              #20 → #21 → #22 → #23
Phase 5 (Debug UX):         #24 → #25 → #26 → #27
Backlog:                    #29, #30
```

**Total Issues:** 33  
**Critical Path:** #1 → #11 → #12 → #13 → #16 (debug schema → daemon → shell)
