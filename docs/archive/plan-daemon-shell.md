# Archive: Shell & Daemon Implementation Plan

> **Historical document.** This plan was part of `docs/SHELL.md` and was
> extracted here during the September 2026 docs-vs-code revision. It describes
> work as *planned* at the time; it is kept only for historical context.
>
> For the current, verified description of the shell, CLI, and HTTP API, see
> [`SHELL.md`](../SHELL.md). Known gaps that were never shipped are listed in
> its "Known Gaps" section.

## Outcome

Roughly how the plan landed when the revision was written:

**Shipped**

- Daemon process with PID file, Unix socket, HTTP API on `127.0.0.1:8080`
- `automodus daemon start|stop|status|restart|logs` (`-f`, `--lines=<n>`)
- `AppCore` shared state with `SessionStore` (create/get/close/list,
  `keep_alive`, idle cleanup)
- Shell attaches to a running daemon when present, otherwise starts a
  standalone browser
- Rustyline REPL with history and completion (commands, workflow files,
  session names)
- Session, navigation, interaction, inspection, tab, PDF, and debug shell
  commands (see `SHELL.md` for the authoritative list)
- Session/workflow/browser/execution HTTP endpoints and `/ws` WebSocket events
- `run` debug flags (`--debug[=level]`, `--delay=`, `--capture=`, `--profile=`,
  `--highlight`, `--pause`, `--console`, `--network`), pause handlers for both
  shell and API, trace JSONL output, `debug clean` + `POST /api/debug/cleanup`

**Not shipped (still open)**

- Shell output-format switching (`human`/`json`/`table`) and a
  `~/.config/automodus/shell.toml` preferences file
- Direct CLI commands outside the shell (`automodus sessions`,
  `automodus session new/close`)
- API run request accepting a `debug` object or `session_id`; API responses
  carrying debug payloads
- Streaming console/network entries over WebSocket (`console.log` /
  `network.request` events)
- Surfacing captured console/network buffers at all (the listeners run, but
  nothing drains their buffers)
- `shell --start-daemon` flag (the shell never auto-starts the daemon)
- Reading `~/.automodus/daemon.toml`: the loader exists but is never called;
  the daemon always runs with built-in defaults

---

# Implementation Phases

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

# Migration Checklist

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
