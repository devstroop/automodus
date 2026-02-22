# Issue Tracker

Track implementation progress for automodus. Update status as work progresses.

**Legend:** ⬜ Not Started | 🟡 In Progress | ✅ Done | ❌ Blocked

---

## Priority 1: Debug Schema (Foundation)

Low-risk, immediate value. Do first before daemon architecture.

### Issue #1: Add DebugConfig to workflow schema
**Status:** ⬜ Not Started  
**File:** `src/workflow/schema.rs`  
**Estimate:** 2-4 hours

**Tasks:**
- [ ] Add `DebugConfig` struct with `Option<T>` fields
- [ ] Add `LogLevel` enum (Info, Debug, Trace)
- [ ] Add `CaptureMode` enum (None, Failure, Before, After, All)
- [ ] Add `DebugProfile` enum (Minimal, Verbose, Ci, Demo)
- [ ] Add `ResolvedDebugConfig` struct (concrete values)
- [ ] Implement `DebugConfig::merge()` and `DebugConfig::resolve()`
- [ ] Implement `DebugConfig::with_profile()`

**Acceptance:**
```rust
let cfg = DebugConfig { profile: Some(DebugProfile::Ci), capture: Some(CaptureMode::All), ..Default::default() };
let resolved = cfg.with_profile().resolve();
assert_eq!(resolved.capture, CaptureMode::All); // explicit overrides profile
```

---

### Issue #2: Add debug field to Workflow and Step
**Status:** ⬜ Not Started  
**File:** `src/workflow/schema.rs`  
**Depends:** #1  
**Estimate:** 1 hour

**Tasks:**
- [ ] Add `#[serde(default)] pub debug: DebugConfig` to `Workflow`
- [ ] Add `#[serde(default)] pub debug: Option<DebugConfig>` to `Step`
- [ ] Add unit test for YAML parsing with debug section
- [ ] Verify backwards compatibility (existing workflows still parse)

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
**Status:** ⬜ Not Started  
**File:** `src/bin/automodus.rs`  
**Depends:** #1  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Add `--debug` flag to `run` command
- [ ] Add `--debug=<level>` variant (info/debug/trace)
- [ ] Add `--delay=<ms>` flag
- [ ] Add `--capture=<mode>` flag
- [ ] Add `--profile=<name>` flag
- [ ] Parse flags into `DebugConfig`
- [ ] Merge CLI config with workflow config

**Acceptance:**
```bash
automodus run workflow.yaml --debug --delay=500 --capture=all
automodus run workflow.yaml --debug=trace --profile=ci
```

---

### Issue #4: Environment variable debug config
**Status:** ⬜ Not Started  
**File:** `src/bin/automodus.rs`  
**Depends:** #1  
**Estimate:** 1 hour

**Tasks:**
- [ ] Read `AUTOMODUS_DEBUG` (bool)
- [ ] Read `AUTOMODUS_DEBUG_LEVEL` (info/debug/trace)
- [ ] Read `AUTOMODUS_DEBUG_PROFILE` (profile name)
- [ ] Merge with CLI/workflow config (lowest precedence)

**Acceptance:**
```bash
AUTOMODUS_DEBUG=true AUTOMODUS_DEBUG_LEVEL=trace automodus run workflow.yaml
```

---

## Priority 2: Engine Debug Integration

Wire debug config through execution.

### Issue #5: Thread DebugConfig through ExecutionContext
**Status:** ⬜ Not Started  
**File:** `src/core/context.rs`, `src/core/engine.rs`  
**Depends:** #1, #2  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] Add `debug: ResolvedDebugConfig` field to `ExecutionContext`
- [ ] Merge workflow + step + CLI debug configs in engine
- [ ] Pass resolved config to each step execution
- [ ] Add delay between steps when `delay > 0`

**Acceptance:**
```rust
// In engine, before step execution:
if ctx.debug.delay > 0 {
    tokio::time::sleep(Duration::from_millis(ctx.debug.delay)).await;
}
```

---

### Issue #6: Screenshot capture on failure
**Status:** ⬜ Not Started  
**File:** `src/core/engine.rs`  
**Depends:** #5  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Create `data/debug/` directory on init
- [ ] Capture screenshot when step fails and `capture != None`
- [ ] Implement filename format: `{timestamp}_{workflow}_step{n}_{phase}.png`
- [ ] Add capture path to `WorkflowResult`

**Acceptance:**
- Step fails → screenshot saved to `data/debug/20260222_150000_myworkflow_step3_failure.png`

---

### Issue #7: Debug directory cleanup
**Status:** ⬜ Not Started  
**File:** `src/utils/debug.rs` (new)  
**Depends:** #6  
**Estimate:** 1-2 hours

**Tasks:**
- [ ] Create `DebugCapture` struct
- [ ] Implement `cleanup_if_needed()` per DEBUG.md spec
- [ ] Delete oldest when `> MAX_DEBUG_FILES` (100)
- [ ] Delete oldest when `> MAX_DEBUG_SIZE_MB` (500)
- [ ] Call cleanup on engine init

---

### Issue #8: Element highlighting
**Status:** ⬜ Not Started  
**File:** `src/modules/browser/adapter.rs`  
**Depends:** #5  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Add `highlight_element()` method to browser adapter
- [ ] Inject highlight JS before interaction when `highlight: true`
- [ ] Brief pause (200ms) then remove highlight
- [ ] Call from click/type/hover actions

**JS injection:**
```javascript
(function(el) {
  const orig = el.style.outline;
  el.style.outline = '3px solid red';
  el.style.outlineOffset = '2px';
  setTimeout(() => el.style.outline = orig, 200);
})(targetElement);
```

---

## Priority 3: CDP Listeners

Browser console and network capture.

### Issue #9: Console capture via CDP
**Status:** ⬜ Not Started  
**File:** `src/modules/browser/adapter.rs`  
**Depends:** #5  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] Setup `ConsoleAPICalledEvent` listener when `console: true`
- [ ] Store captured logs in `ExecutionContext`
- [ ] Format with timestamp and level
- [ ] Include in `WorkflowResult` when debug enabled

**Output format:**
```
[CONSOLE] 14:32:01.123 LOG: message
[CONSOLE] 14:32:01.456 WARN: warning
[CONSOLE] 14:32:02.789 ERROR: error
```

---

### Issue #10: Network capture via CDP
**Status:** ⬜ Not Started  
**File:** `src/modules/browser/adapter.rs`  
**Depends:** #5  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] Setup `ResponseReceivedEvent` listener when `network: true`
- [ ] Capture method, URL, status, timing
- [ ] Store in `ExecutionContext`
- [ ] Include in `WorkflowResult`

**Output format:**
```
[NETWORK] GET https://api.example.com/user → 200 (45ms)
[NETWORK] POST https://api.example.com/login → 401 (120ms)
```

---

## Priority 4: Daemon Architecture

Foundation for persistent sessions. Largest effort.

### Issue #11: Create Daemon struct
**Status:** ⬜ Not Started  
**File:** `src/daemon/mod.rs` (new)  
**Estimate:** 1-2 days

**Tasks:**
- [ ] Create `src/daemon/mod.rs`
- [ ] Implement `Daemon` struct per SHELL.md
- [ ] PID file management (`~/.automodus/daemon.pid`)
- [ ] Log file (`~/.automodus/daemon.log`)
- [ ] Unix socket listener (`~/.automodus/automodus.sock`)
- [ ] Graceful shutdown handling

---

### Issue #12: Create AppCore struct
**Status:** ⬜ Not Started  
**File:** `src/core/app.rs` (new)  
**Depends:** #11  
**Estimate:** 4-6 hours

**Tasks:**
- [ ] Create `src/core/app.rs`
- [ ] Move `WorkflowEngine` ownership from various places
- [ ] Add `RwLock<WorkflowRegistry>`
- [ ] Add `RwLock<DebugConfig>`
- [ ] Add `broadcast::Sender<DaemonEvent>` for WebSocket
- [ ] Implement `run_workflow()` and `browser_command()` methods

---

### Issue #13: Create SessionManager
**Status:** ⬜ Not Started  
**File:** `src/core/session.rs` (new)  
**Depends:** #12  
**Estimate:** 1 day

**Tasks:**
- [ ] Create `src/core/session.rs`
- [ ] Extract browser launch logic from `api/state.rs` and `bin/automodus.rs`
- [ ] Implement `Session` struct with `keep_alive` field
- [ ] Implement `SessionManager` with create/get/close/list
- [ ] Implement `cleanup_idle()` for idle timeout
- [ ] Single source of truth for browser lifecycle

**Removes duplication from:**
- `src/api/state.rs:70-130`
- `src/bin/automodus.rs:235-270`

---

### Issue #14: Daemon CLI commands
**Status:** ⬜ Not Started  
**File:** `src/bin/automodus.rs`  
**Depends:** #11  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] Add `daemon start` command (background, `-f` for foreground)
- [ ] Add `daemon stop` command
- [ ] Add `daemon status` command
- [ ] Add `daemon restart` command
- [ ] Add `daemon logs` command

**Acceptance:**
```bash
automodus daemon start
automodus daemon status  # → "Daemon running (PID 12345)"
automodus daemon stop
```

---

### Issue #15: Merge HTTP server into daemon
**Status:** ⬜ Not Started  
**File:** `src/daemon/mod.rs`, `src/api/server.rs`  
**Depends:** #11, #12  
**Estimate:** 4-6 hours

**Tasks:**
- [ ] Move HTTP server startup into `Daemon::start()`
- [ ] Share `AppCore` between socket and HTTP handlers
- [ ] Update `serve` command to start daemon (backward compat)
- [ ] Remove standalone server state management

---

## Priority 5: Shell Refactor

Stateless shell connecting to daemon.

### Issue #16: Create ShellClient
**Status:** ⬜ Not Started  
**File:** `src/shell/client.rs` (new)  
**Depends:** #11, cargo deps (rustyline, dirs)  
**Estimate:** 1 day

**Tasks:**
- [ ] Create `src/shell/mod.rs` and `src/shell/client.rs`
- [ ] Implement daemon connection via Unix socket
- [ ] Replace inline browser launch with daemon commands
- [ ] Add rustyline for readline support
- [ ] Implement history save/load

---

### Issue #17: Shell command expansion
**Status:** ⬜ Not Started  
**File:** `src/shell/client.rs`  
**Depends:** #16  
**Estimate:** 1-2 days

**Tasks:**
- [ ] Add `click <selector>` command
- [ ] Add `type <selector> <text>` command  
- [ ] Add `wait <selector> [timeout]` command
- [ ] Add `screenshot [path]` command
- [ ] Add `eval <js>` command
- [ ] Add `text <selector>` command
- [ ] Add `find <selector>` command
- [ ] Add navigation: `back`, `forward`, `refresh`

---

### Issue #18: Shell autocomplete
**Status:** ⬜ Not Started  
**File:** `src/shell/client.rs`  
**Depends:** #16  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] Implement `Completer` trait for rustyline
- [ ] Command name completion
- [ ] Workflow name completion for `run`
- [ ] Session name completion for session commands
- [ ] File path completion for `screenshot`

---

### Issue #19: Session shell commands
**Status:** ⬜ Not Started  
**File:** `src/shell/client.rs`  
**Depends:** #13, #16  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Add `session new [--name=NAME] [--keep-alive]`
- [ ] Add `session list`
- [ ] Add `session switch <id|name>`
- [ ] Add `session close [id]`
- [ ] Add `session info`
- [ ] Add `session keep-alive [id] [on|off]`

---

## Priority 6: API Enhancement

Full API parity with shell.

### Issue #20: Session API endpoints
**Status:** ⬜ Not Started  
**File:** `src/api/handlers.rs`  
**Depends:** #13  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] `POST /api/sessions` - create session (with `keep_alive` option)
- [ ] `GET /api/sessions` - list sessions
- [ ] `GET /api/sessions/:id` - get session info
- [ ] `DELETE /api/sessions/:id` - close session
- [ ] Add to OpenAPI schema

---

### Issue #21: Browser control API endpoints
**Status:** ⬜ Not Started  
**File:** `src/api/handlers.rs`  
**Depends:** #12  
**Estimate:** 4-6 hours

**Tasks:**
- [ ] `POST /api/browser/click` - click element
- [ ] `POST /api/browser/type` - type into element
- [ ] `POST /api/browser/wait` - wait for element
- [ ] `POST /api/browser/eval` - execute JavaScript
- [ ] `GET /api/browser/page` - get page info
- [ ] Add to OpenAPI schema

---

### Issue #22: Execution tracking endpoints
**Status:** ⬜ Not Started  
**File:** `src/api/handlers.rs`  
**Depends:** #12  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] `GET /api/executions` - list recent executions
- [ ] `GET /api/executions/:id` - get execution details
- [ ] `DELETE /api/executions/:id` - cancel execution
- [ ] `GET /api/executions/:id/output` - get execution output
- [ ] Store execution history in memory/sqlite

---

### Issue #23: WebSocket implementation
**Status:** ⬜ Not Started  
**File:** `src/api/ws.rs` (new)  
**Depends:** #12, cargo deps (tokio-tungstenite)  
**Estimate:** 1 day

**Tasks:**
- [ ] Create `src/api/ws.rs`
- [ ] Implement `/ws` endpoint
- [ ] Subscribe to `AppCore::events` broadcast
- [ ] Forward execution events to connected clients
- [ ] Handle client commands (continue, skip, abort)
- [ ] Version protocol messages

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
**Status:** ⬜ Not Started  
**File:** `src/shell/client.rs`  
**Depends:** #16, #5  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Add `debug on [--profile=PROFILE]`
- [ ] Add `debug off`
- [ ] Add `debug status`
- [ ] Add `highlight <selector>`
- [ ] Add `trace <workflow>` (alias for `run --debug=trace`)

---

### Issue #25: Pause implementation (shell)
**Status:** ⬜ Not Started  
**File:** `src/shell/client.rs`, `src/core/engine.rs`  
**Depends:** #5, #16  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] Detect `pause: true` in step debug config
- [ ] Prompt user: "Press Enter to continue, 's' to skip, 'q' to quit"
- [ ] Handle stdin in shell context
- [ ] Skip step or abort workflow based on input

---

### Issue #26: Pause implementation (API/WebSocket)
**Status:** ⬜ Not Started  
**File:** `src/api/ws.rs`, `src/core/engine.rs`  
**Depends:** #23, #5  
**Estimate:** 3-4 hours

**Tasks:**
- [ ] Send `execution.paused` event via WebSocket
- [ ] Wait for client command (`continue`, `skip`, `abort`)
- [ ] If no WebSocket connected, skip pause with warning in response
- [ ] Timeout handling for unresponsive clients

---

### Issue #27: Trace-level JSONL output
**Status:** ⬜ Not Started  
**File:** `src/utils/debug.rs`  
**Depends:** #5  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Create `data/debug/trace.jsonl` when `level: trace`
- [ ] Log selector resolution details
- [ ] Log element info (tag, role, text, attributes)
- [ ] Log injected JS
- [ ] Append line per event

---

## Backlog / Future

### Issue #28: Unify error types
**Status:** ⬜ Not Started  
**File:** `src/error.rs`  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Merge `AutomodusError` and proposed `AppError`
- [ ] Add `ErrorCode` enum for API responses
- [ ] Implement `From<AutomodusError>` for API error response
- [ ] Update all error handling sites

---

### Issue #29: Tab management
**Status:** ⬜ Not Started  
**File:** `src/modules/browser/actions/tabs.rs`  
**Estimate:** 4-6 hours

**Tasks:**
- [ ] `tabs` - list open tabs
- [ ] `tab <index>` - switch to tab
- [ ] `tab new [url]` - open new tab
- [ ] `tab close [index]` - close tab
- [ ] Wire through shell and API

---

### Issue #30: PDF export
**Status:** ⬜ Not Started  
**File:** `src/modules/browser/adapter.rs`  
**Estimate:** 2-3 hours

**Tasks:**
- [ ] Add `pdf [path]` command
- [ ] Add `POST /api/browser/pdf` endpoint
- [ ] Use CDP `Page.printToPDF`

---

## Revision History

| Date | Changes |
|------|---------|
| 2026-02-22 | Initial issue list from SHELL.md and DEBUG.md review |

---

## Quick Reference: Implementation Order

```
Phase 0 (Debug Schema):     #1 → #2 → #3 → #4
Phase 1a (Daemon Core):     #11 → #12 → #14
Phase 1b (Sessions):        #13 → #15
Phase 1c (Shell):           #16 → #17 → #18 → #19
Phase 2 (Engine Debug):     #5 → #6 → #7 → #8
Phase 3 (CDP):              #9 → #10
Phase 4 (API):              #20 → #21 → #22 → #23
Phase 5 (Debug UX):         #24 → #25 → #26 → #27
Backlog:                    #28, #29, #30
```
