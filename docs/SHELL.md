# Shell & API Reference

Reference for the automodus CLI, interactive shell, and HTTP/WebSocket API.
Everything documented here was verified against the code during the
September 2026 docs-vs-code revision.

For workflow syntax see [DESIGN.md](DESIGN.md), for internals see
[ARCHITECTURE.md](ARCHITECTURE.md). The original daemon/shell design plan and
its migration checklist now live in
[archive/plan-daemon-shell.md](archive/plan-daemon-shell.md).

## Current State

| Component | Status | Location |
|-----------|--------|----------|
| Shell REPL | ✅ Implemented (daemon-attached or standalone) | `src/bin/automodus.rs` (`run_shell`), `src/shell/client.rs` |
| HTTP API | ✅ Implemented (axum, OpenAPI + Swagger UI) | `src/api/server.rs`, `src/api/handlers.rs`, `src/api/schemas.rs` |
| WebSocket | ✅ Implemented (`/ws`) | `src/api/ws.rs` |
| Daemon | ✅ Implemented (Unix socket, PID file, logs, HTTP) | `src/daemon/mod.rs` |
| Shared state | ✅ Implemented (`AppCore`, `SessionStore`) | `src/core/app.rs` |
| Completion & history | ✅ Implemented (rustyline) | `src/shell/client.rs` |

## Architecture

Docker-style split: a daemon can own browser sessions while clients stay
stateless. In practice the tool supports three run modes:

```
┌───────────────────────────────────────────────────────────────┐
│                    automodus daemon (optional)                 │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐        │
│  │ SessionStore │  │ Workflow     │  │ AppCore      │        │
│  │ (contexts)   │  │ Engine       │  │ (shared)     │        │
│  └──────────────┘  └──────────────┘  └──────────────┘        │
│                                                               │
│  Listens on:                                                  │
│  • Unix socket: <data-dir>/.automodus/automodus.sock          │
│  • TCP/HTTP:    127.0.0.1:8080 (REST API)                     │
│  • WebSocket:   127.0.0.1:8080/ws                             │
└───────────────────────────────────────────────────────────────┘
         ▲                    ▲                   ▲
         │                    │                   │
    ┌────┴────┐          ┌────┴────┐         ┌────┴────┐
    │ shell 1 │          │ shell 2 │         │  REST   │
    │ (REPL)  │          │ (REPL)  │         │  API    │
    └─────────┘          └─────────┘         └─────────┘
```

Run modes:

| Mode | Started by | Browser ownership |
|------|-----------|-------------------|
| Standalone shell | `automodus shell` when no daemon answers ping | Shell launches its own browser (`browser.engine` from config) |
| Daemon-attached shell | `automodus shell` with daemon running | Daemon sessions; browser survives shell exit |
| One-shot run | `automodus run <file>` | Fresh browser per run, removed on exit (`TempProfile`) |
| Daemon HTTP | `automodus daemon start` | Daemon serves REST/WS on `127.0.0.1:8080` |

Runtime files live in the platform data directory (e.g.
`~/.local/share/.automodus/` on Linux):

| File | Purpose |
|------|---------|
| `automodus.sock` | Unix socket for shell ↔ daemon |
| `daemon.pid` | Daemon process ID |
| `daemon.log` | Daemon log output |

Notes:

- The shell never auto-starts the daemon; start it with
  `automodus daemon start`.
- `automodus run` does **not** send work to the daemon — it launches its own
  browser even when a daemon is running.
- `automodus serve` is deprecated; it runs the HTTP server in the
  foreground. Use `automodus daemon start` instead.

## CLI Commands

`automodus` uses hand-rolled argument parsing (no clap). Run `automodus help`
for the built-in overview.

```bash
# Workflow execution
automodus run <workflow.yaml> [key=value ...]   # key=value overrides params
    -k, --keep-open        Keep browser open after the workflow completes
    -d, --debug            Enable debug mode
    --debug=<level>        Debug level (info, debug, trace)
    --delay=<ms>           Delay between steps
    --capture=<mode>       Screenshots: none | failure | before | after | all
    --profile=<name>       Debug preset: minimal | verbose | ci | demo
    --highlight            Highlight elements before interaction
    --pause                Pause before each step (stdin confirmation)
    --console              Log browser console messages
    --network              Log network requests

# Daemon lifecycle
automodus daemon start              # Background process
automodus daemon stop               # Graceful shutdown
automodus daemon status             # Running? (PID, socket)
automodus daemon restart
automodus daemon logs [-f|--follow] [--lines=<n>]

# Other
automodus shell                     # Interactive REPL (see modes above)
automodus serve                     # [DEPRECATED] HTTP server in foreground
automodus validate [path]           # Validate workflows (default: workflows/)
automodus list                      # List loaded workflows
automodus help                      # Usage text
```

Exit codes:

| Command | Success | Failure |
|---------|---------|---------|
| `run` | `0` | `1` (workflow failed) |
| `validate` | `0` | `1` (any file invalid) |
| anything else | `0` | `1` (usage/runtime error) |

## Interactive Shell

`automodus shell` starts a rustyline REPL. If a daemon is running it attaches
to it ("✓ Connected to daemon"); otherwise it prints that it is starting
standalone mode and launches a browser.

### Commands

| Command | Aliases | Description |
|---------|---------|-------------|
| `run <file> [k=v ...]` | `r` | Run a workflow file with `key=value` params |
| `goto <url>` | `g`, `go` | Navigate to URL |
| `back` / `forward` | | History navigation |
| `refresh` | `reload` | Reload page |
| `click <selector>` | `c` | Click element |
| `type <sel> <text>` | `t` | Type into element |
| `wait <sel> [ms]` | `w` | Wait for element |
| `text <selector>` | | Element text |
| `find <selector>` | `f` | Count matching elements |
| `eval <js>` | `js` | Evaluate JavaScript |
| `screenshot [path]` | `ss` | Save screenshot |
| `pdf [path]` | | Export page as PDF (Chromium only) |
| `status` | `s` | Show current page URL |
| `list` | `ls` | List workflows |
| `session new [name] [--keep-alive]` | `sess` | Create session |
| `session list` | | List sessions |
| `session switch <id\|name>` | | Switch active session |
| `session close [id\|name]` | | Close session (current if omitted) |
| `session info` | | Current session details |
| `session keep-alive [id] [on\|off]` | | Toggle keep-alive |
| `tabs` | | List open tabs |
| `tab new [url]` | | Open tab |
| `tab switch <index>` / `tab <index>` | | Switch tab |
| `tab close [index]` | | Close tab (current if omitted) |
| `debug on [--profile=NAME]` | | *(prints confirmation only — see gaps)* |
| `debug off` / `debug status` | | *(prints confirmation only — see gaps)* |
| `debug clean` | `debug cleanup` | Remove old files from `data/debug/` |
| `highlight <selector>` | `hl` | Flash element outline on page |
| `trace <file> [k=v ...]` | | Run workflow with a "trace logging" banner (trace output itself is not wired — see gaps) |
| `help` | `h`, `?` | Shell help |
| `quit` | `exit`, `q` | Exit shell |

Shortcuts: `r`=run, `g`=goto, `c`=click, `t`=type, `w`=wait, `f`=find,
`s`=status, `ls`=list, `hl`=highlight, `ss`=screenshot, `sess`=session,
`q`=quit.

There are **no** `show`, `reload-workflows`, `clear`, `history`, `pause`,
`step`, or `continue` commands.

### Completion & history

- Tab-completes command names, workflow `*.yaml` paths for `run`, session
  subcommands and session names, and file paths.
- History is stored in the platform data directory (e.g.
  `~/.local/share/automodus/history.txt`), 1000 entries by default.

## REST API

The daemon's HTTP server binds `127.0.0.1:8080` by default. **There is no
authentication.** Interactive documentation: `/swagger-ui`, schema:
`/api/openapi.json`.

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/health` | Status, version, workflow count |
| GET | `/api/workflows` | List loaded workflows |
| POST | `/api/workflows/reload` | Reload workflows from disk |
| GET | `/api/workflows/:name` | Workflow details |
| POST | `/api/workflows/:name/run` | Execute workflow |
| GET | `/api/browser/screenshot` | PNG screenshot (`image/png`) |
| POST | `/api/browser/goto` | Navigate |
| POST | `/api/browser/click` | Click element |
| POST | `/api/browser/type` | Type into element |
| POST | `/api/browser/wait` | Wait for element |
| POST | `/api/browser/eval` | Evaluate JavaScript |
| GET | `/api/browser/page` | URL + title |
| GET | `/api/browser/tabs` | List tabs |
| POST | `/api/browser/tabs` | New tab |
| POST | `/api/browser/tabs/switch` | Switch tab |
| DELETE | `/api/browser/tabs/:index` | Close tab |
| GET | `/api/browser/pdf` | PDF export (Chromium only) |
| POST | `/api/debug/cleanup` | Prune `data/debug/` |
| POST | `/api/sessions` | Create session |
| GET | `/api/sessions` | List sessions |
| GET | `/api/sessions/:id` | Session info |
| DELETE | `/api/sessions/:id` | Close session |
| GET | `/api/executions` | Recent executions |
| GET | `/api/executions/:id` | Execution detail + output |
| DELETE | `/api/executions/:id` | Cancel execution |
| GET | `/ws` | WebSocket (upgrade) |
| GET | `/swagger-ui` | Swagger UI |
| GET | `/api/openapi.json` | OpenAPI schema |

There is **no** `POST /api/workflows/:name/validate` (use the CLI
`automodus validate`) and no `/api/v1/*` prefix.

### Request / response schemas

Run a workflow — the request accepts **only** `params` (no `session_id`, no
`debug` object):

```http
POST /api/workflows/send_message/run
Content-Type: application/json

{ "params": { "phone": "1234", "message": "Hello" } }
```

```json
{
  "success": true,
  "workflow_name": "send_message",
  "duration_ms": 1234,
  "steps_executed": 5,
  "output": { "sent": true },
  "error": null
}
```

Browser actions:

```json
POST /api/browser/goto            { "url": "https://example.com" }
POST /api/browser/click           { "selector": "text:Submit" }
POST /api/browser/type            { "selector": "#q", "text": "hello" }
POST /api/browser/wait            { "selector": ".ready", "timeout": 30000 }
POST /api/browser/eval            { "script": "document.title" }
POST /api/browser/tabs            { "url": null }
POST /api/browser/tabs/switch     { "index": 1 }
```

Responses are `{"success": bool, "result"?: …, "error"?: …}` (page info:
`{"url", "title"}`; tab lists: `{"tabs": [{"index", "url", "active"}]}`).

Sessions:

```json
POST /api/sessions      { "name": "whatsapp", "keep_alive": true }
→ { "success": true, "id": "…" }

GET  /api/sessions
→ { "sessions": [{ "id": "…", "name": "whatsapp",
                   "created_at": "…", "last_activity": "…",
                   "keep_alive": true }] }
```

Errors are flat strings:

```json
{ "error": "Workflow not found" }
```

## WebSocket (`/ws`)

Server → client events (all carry `"version": 1`):

```json
{ "version": 1, "type": "connected" }
{ "version": 1, "type": "execution.started", "id": "…", "workflow": "…" }
{ "version": 1, "type": "execution.step", "id": "…", "step": 3, "action": "click" }
{ "version": 1, "type": "execution.complete", "id": "…", "success": true }
{ "version": 1, "type": "execution.error", "id": "…", "error": "…" }
{ "version": 1, "type": "execution.paused", "id": "…", "step": 3 }
{ "version": 1, "type": "session.created", "id": "…" }
{ "version": 1, "type": "session.closed", "id": "…" }
```

Client → server commands:

```json
{ "type": "continue", "id": "<execution-id>" }
{ "type": "skip",     "id": "<execution-id>" }
{ "type": "abort",    "id": "<execution-id>" }
{ "type": "ping" }
```

`continue` / `skip` / `abort` resolve a paused execution. While paused, the
execution **waits indefinitely** for one of these commands (there is no
timeout and no HTTP fallback) — if the channel is dropped, the engine
continues.

## Configuration

| Config | Location | Status |
|--------|----------|--------|
| Server/browser/app | `config/app.toml` (override: `AUTOMODUS_CONFIG`) | ✅ read via `src/config.rs` |
| Daemon config file | `~/.automodus/daemon.toml` | ⚠️ loader exists (`src/daemon/config.rs`) but is **never called** — the daemon always runs with built-in defaults (socket/PID/log in the platform data dir, HTTP `127.0.0.1:8080`) |
| Shell preferences | `~/.config/automodus/shell.toml` | ❌ not implemented (no such file is read) |
| Shell history | `<data-dir>/automodus/history.txt` | ✅ written by the REPL |

Effective daemon settings (defaults, since the TOML loader is dormant):
`127.0.0.1:8080`, HTTP enabled, 10 max sessions, `data/debug` for captures.

### Environment variables

Honored at runtime:

| Variable | Effect |
|----------|--------|
| `AUTOMODUS_CONFIG` | Config file path (default `config/app.toml`) |
| `AUTOMODUS_WORKFLOWS` | Workflows directory (default `workflows/`) |
| `AUTOMODUS_CHROME_PATH` / `AUTOMODUS_FIREFOX_PATH` / `AUTOMODUS_LIGHTPANDA_PATH` | Browser binaries |
| `AUTOMODUS_DEBUG`, `AUTOMODUS_DEBUG_LEVEL`, `AUTOMODUS_DEBUG_PROFILE`, `AUTOMODUS_DEBUG_DELAY`, `AUTOMODUS_DEBUG_CAPTURE` | Debug defaults for `automodus run` only |
| `AUTOMODUS_NO_PROXY` / `AUTOMODUS_DISABLE_IPV6` | Opt-in Chromium launch flags |
| `AUTOMODUS_*` | Any other `AUTOMODUS_`-prefixed variable maps onto `config/app.toml` keys (e.g. `AUTOMODUS_BROWSER_ENGINE=firefox`) |
| `RUST_LOG` / `LOG_JSON` | Log filter and JSON log format |

Parsed only by the dormant daemon TOML loader (currently inert):
`AUTOMODUS_LOG_LEVEL`, `AUTOMODUS_HTTP_HOST`, `AUTOMODUS_HTTP_PORT`,
`AUTOMODUS_SOCKET_PATH`, `AUTOMODUS_MAX_SESSIONS`.

## Error Handling

The HTTP layer returns flat JSON strings:

```json
{ "error": "Browser error: …" }
```

`AppError` / `ErrorCode` are exported from `src/error.rs` (daemon, session,
workflow, browser, and general codes) but are **not** what the HTTP handlers
emit today.

CLI/shell errors are human-readable messages on stderr; `run` and
`validate` exit with code `1` on failure.

## Known Gaps

Unshipped or partially shipped items (tracked in
[ISSUES.md](ISSUES.md)):

- `debug on` / `debug off` / `debug status` only print a confirmation; they
  do not change shell state (use CLI flags on `run`, or `trace`).
- The `debug.level` option, `--debug=<level>`, and the shell `trace` command
  are effectively schema-only: log verbosity comes from `RUST_LOG`, and
  `data/debug/trace.jsonl` is never written (`TraceLogger` is not wired).
- Workflow-level `debug.profile:` is not applied on the `automodus run` path
  — use `--profile=` on the CLI; profiles do apply for sub-workflows
  invoked with `call:`.
- The API run request accepts only `params`; no per-request `debug` or
  `session_id`, and responses carry no debug payload.
- Console/network listeners always start, but their buffers are never
  drained or surfaced (no `console.log` / `network.request` WS events, and
  the `console:` / `network:` debug flags currently gate nothing).
- No auth on the HTTP API (it binds `127.0.0.1` only).
- Daemon TOML config loader and the `AUTOMODUS_LOG_LEVEL`,
  `AUTOMODUS_HTTP_HOST`/`PORT`, `AUTOMODUS_SOCKET_PATH`, and
  `AUTOMODUS_MAX_SESSIONS` env vars are dormant.
- No `shell.toml` preferences file, no output-format switching
  (human/json/table), no direct CLI equivalents of `session ...` outside
  the shell.

## Related

- [DEBUG.md](DEBUG.md) - Debug mode reference
- [DESIGN.md](DESIGN.md) - Workflow design and syntax
- [ARCHITECTURE.md](ARCHITECTURE.md) - System internals
- [CONTRIBUTING.md](../CONTRIBUTING.md) - Development guidelines
- [archive/plan-daemon-shell.md](archive/plan-daemon-shell.md) - Historical plan
