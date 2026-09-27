<div align="center">

# Automodus

**Workflow Automation Infrastructure**

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-ready-blue.svg)](Dockerfile)

[Features](#features) • [Quick Start](#quick-start) • [Workflow YAML](#workflow-yaml-format) • [API](#api-reference) • [Configuration](#configuration)

</div>

---

## What is Automodus?

Automodus is a **programmable workflow automation platform** that lets you define automation workflows in YAML and run them via:

- **REST API** - `POST /api/workflows/{name}/run` calls a workflow like a function
- **CLI** - `automodus run workflow.yaml`

Workflows can also *declare* triggers in an `on:` block (API path, cron schedule,
event, webhook, file watch). The declarations are parsed, validated, and shown by
`automodus list`; automatic dispatch (cron/webhook routing) is planned — today you
trigger runs explicitly via the CLI or the REST API.

Think of it as **"automation as code"** — define once, run anywhere.

### Modules

| Module | Description |
|--------|-------------|
| **Browser** | Browser automation on three engines: Chromium (CDP), Firefox (WebDriver BiDi, no geckodriver), Lightpanda (CDP) |
| **HTTP** | REST API requests (GET, POST, PUT, PATCH, DELETE) |
| **LLM** | AI integration (planned) |
| **Storage** | Data persistence (planned) |

## Features

| Feature | Description |
|---------|-------------|
| **YAML Workflows** | Define automation in declarative YAML |
| **Built-in Actions** | 28 actions + `loop`/`call`/`condition`: goto, click, type, extract, screenshot, http, tabs, upload, … |
| **Browser Engines** | `engine = "chromium"` / `"firefox"` / `"lightpanda"` in config; per-engine capability gating (PDF, multi-tab, ...) |
| **Control Flow** | `call`, `condition`, `loop` (scoped `as`/`index_as` items), step-level `on_success`/`on_failure` handlers (goto/emit/abort/steps) |
| **HTTP Module** | Make API requests within workflows (GET, POST, PUT, PATCH, DELETE) |
| **Multi-Instance** | Run multiple browser instances with isolated data directories |
| **Multi-Tab** | Control multiple tabs within each browser instance |
| **Template Engine** | `{{params.x}}`, `{{store.y}}`, `{{env.Z}}` interpolation |
| **Event System** | `emit` publishes events from workflows (react-to-event dispatch is planned) |
| **REST API** | Trigger workflows via HTTP endpoints |
| **CLI** | Run, validate, and manage workflows from command line |

## Quick Start

### Prerequisites

- **Rust** 1.70+
- A browser: **Chrome/Chromium**, **Firefox**, or **Lightpanda** (install any one; engine selected via `engine` in `[browser]`)
- macOS, Linux, or Windows

### Installation

```bash
# Clone (with the examples submodule)
git clone --recurse-submodules https://github.com/devstroop/automodus.git
cd automodus
# (already cloned without submodules? run: git submodule update --init)

# Build
cargo build --release

# Validate example workflows (examples/ is a git submodule, see examples/README.md)
./target/release/automodus validate examples/

# List available workflows
./target/release/automodus list
```

### Create Your First Workflow

Create `workflows/hello.yaml` in your own workflows directory
(`AUTOMODUS_WORKFLOWS` defaults to `workflows/`; ready-made examples live in the
`examples/` submodule — `browser/`, `http/`, `compose/`, `control-flow/`,
`debug/`, `whatsapp/`):

```yaml
name: hello_world
description: Simple example workflow

params:
  url:
    type: string
    required: true

steps:
  - action: goto
    url: "{{params.url}}"

  - action: wait_for
    selector: body
    timeout: 10000

  - action: screenshot
    path: "screenshots/hello.png"

  - action: extract
    selector: title
    store_as: page_title

output:
  title: "{{store.page_title}}"
  url: "{{params.url}}"
```

### Run the Workflow

```bash
# Via CLI
./target/release/automodus run workflows/hello.yaml url=https://example.com

# Via API (daemon HTTP server must be running)
curl -X POST http://localhost:8080/api/workflows/hello_world/run \
  -H "Content-Type: application/json" \
  -d '{"params": {"url": "https://example.com"}}'
```

## Workflow YAML Format

### Basic Structure

```yaml
name: workflow-name           # Required: unique identifier
version: "1.0"            # Optional: version string
description: "..."        # Optional: description

# Declared triggers — validated and shown by `automodus list`;
# automatic dispatch (cron/webhook routing) is planned. Run explicitly
# via the CLI or POST /api/workflows/{name}/run.
on:
  api:                    # REST API trigger
    path: /workflows/my-workflow
    method: POST
  schedule: "0 * * * *"   # Cron schedule (5-6 fields)
  event: "user.login"     # Event-based trigger
  webhook:                # Webhook trigger
    path: /hooks/my-hook
    secret: "..."

# Input parameters with validation
params:
  query:
    type: string
    required: true
    description: "Search term"
  count:
    type: number
    default: 10

# Workflow-level variables
vars:
  base_url: "https://example.com"
  timeout: 30000

# Browser configuration
browser:
  headless: true
  width: 1280
  height: 720
  data_dir: "./data/{{instance.id}}"

# Automation steps
steps:
  - action: goto
    url: "{{vars.base_url}}"
  
  - action: type
    selector: "input[name=q]"
    text: "{{params.query}}"

# Output definition
output:
  query: "{{params.query}}"
  results: "{{store.results}}"

# Lifecycle hooks
on_complete:
  emit:
    event: workflow.completed
    
on_error:
  screenshot: true
  emit:
    event: workflow.failed
```

### Available Actions

28 registered actions (validated against the registry — a typo yields an
"unknown action" error listing all of them):

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `goto` | Navigate to URL (alias: `navigate`) | `url` |
| `back` / `forward` / `reload` | History navigation / page reload | — |
| `click` | Click element | `selector` |
| `type` | Type text (alias: `input`) | `selector`, `text` |
| `hover` | Hover over element | `selector` |
| `select` | Select dropdown option | `selector`, `value` |
| `wait_for` | Wait for element/URL/text (alias: `wait`) | `selector`, `url`, or `text` (+ `timeout`, `state`) |
| `sleep` | Pause execution | `duration` (e.g., "2s") or `ms` |
| `extract` | Extract data from page | `selector`, `store_as` (or `as`) |
| `eval` | Execute JavaScript | `script` |
| `screenshot` | Capture screenshot | (optional: `path`, `full_page`) |
| `tab.list` | List open tabs | — |
| `tab.new` | Open new tab | (optional: `url`) |
| `tab.switch` | Switch to tab | `index` |
| `tab.close` | Close tab | (optional: `index`) |
| `upload` | Upload files to a file input | `file_path`/`files` (optional `selector`; Chromium-only) |
| `wait_upload` | Wait for file-chooser event, then upload | `file_path`/`files` (optional `trigger`; Chromium-only) |
| `file_chooser` | Enable/disable file-chooser interception | (optional: `enabled`, `file_path`) (Chromium-only) |
| `http.get` | HTTP GET request | `url` |
| `http.post` | HTTP POST request | `url` |
| `http.put` | HTTP PUT request | `url` |
| `http.patch` | HTTP PATCH request | `url` |
| `http.delete` | HTTP DELETE request | `url` |
| `http.request` | Generic HTTP request | `method`, `url` |
| `emit` | Emit event | `event` |
| `log` | Log message | `message` |

Engine pseudo-actions (handled by the workflow engine, not the registry):

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `loop` | Iterate over items, running `steps` each pass | `items`, `as`, `steps` (optional `index_as`; `parallel: true` rejected) |
| `condition` | Branch on an expression | `if`, `then` (optional `else`) |
| `call` | Invoke another workflow | `workflow` |

Each step can also declare step-level handlers: `on_success:` / `on_failure:`
(see [docs/DESIGN.md](docs/DESIGN.md)).

### Template Syntax

Use `{{...}}` for variable interpolation. Valid roots: `params`, `vars`,
`store`, `steps`, `env`, `instance`, `timestamp`, `workflow`
(a bare `{{name}}` falls back to a `store` lookup):

| Reference | Description |
|-----------|-------------|
| `{{params.name}}` | Input parameter |
| `{{vars.name}}` | Workflow variable (loop iterations bind `{{vars.<as>}}` / `{{vars.<index_as>}}`) |
| `{{store.name}}` | Stored value from extract/eval |
| `{{steps.id.output}}` | Output from step with id |
| `{{env.NAME}}` | Environment variable |
| `{{instance.id}}` | Browser instance ID |
| `{{timestamp}}` | Current RFC3339 timestamp |
| `{{workflow.name}}` / `{{workflow.id}}` | Workflow name / id |
| `{{x \| json}}` | Only built-in filter (serializes to JSON) |

## CLI Reference

```bash
# Run a specific workflow (see examples/ for ready-made workflows)
automodus run examples/browser/search_form.yaml

# Run with flags / parameter overrides
automodus run examples/compose/pipeline.yaml --debug --profile=verbose \
  --delay=1000 --capture=failure --keep-open key=value

# Start the API server (foreground; HTTP on 127.0.0.1:3000)
automodus serve            # [DEPRECATED] prefer 'daemon start'

# Daemon management (HTTP on 127.0.0.1:8080)
automodus daemon start     # background process
automodus daemon status
automodus daemon logs -f --lines=100
automodus daemon restart
automodus daemon stop

# Interactive shell (keeps the browser running)
automodus shell

# Validate workflow files (default: workflows/)
automodus validate examples/

# List all loaded workflows
automodus list

# Show help
automodus help
```

`run` flags: `-k/--keep-open`, `-d/--debug`, `--debug=<info|debug|trace>`,
`--delay=<ms>`, `--capture=<none|failure|before|after|all>`,
`--profile=<minimal|verbose|ci|demo>`, `--highlight`, `--pause`, `--console`,
`--network`, plus `key=value` parameter overrides.

**Exit codes:** `run` exits `1` if the workflow fails; `validate` exits `1` if
any file is invalid (useful for CI gates).

## API Reference

### Documentation

Access interactive API documentation via Swagger UI:

```bash
# Foreground server (port 3000)
automodus serve
open http://localhost:3000/swagger-ui

# ...or the daemon (port 8080)
automodus daemon start
open http://localhost:8080/swagger-ui
```

- **Swagger UI**: `http://localhost:3000/swagger-ui` (or `:8080` behind the daemon)
- **OpenAPI JSON**: `http://localhost:3000/api/openapi.json`

### Workflow Execution

```bash
# Run a workflow by name (workflow must be in AUTOMODUS_WORKFLOWS)
POST /api/workflows/{name}/run
Content-Type: application/json

{
  "params": {
    "param1": "value1",
    "param2": "value2"
  }
}
```

Response: `{ "success": bool, "workflow_name": ..., "duration_ms": ...,
"steps_executed": ..., "output": ..., "error": ... }`

### Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| `GET` | `/api/health` | Health check |
| `GET` | `/api/workflows` | List loaded workflows |
| `POST` | `/api/workflows/reload` | Rescan the workflows directory |
| `GET` | `/api/workflows/{name}` | Workflow details |
| `POST` | `/api/workflows/{name}/run` | Run a workflow |
| `GET` | `/api/executions` / `/api/executions/{id}` | List / inspect executions |
| `DELETE` | `/api/executions/{id}` | Cancel a running execution |
| `GET` | `/api/browser/screenshot` | Screenshot of the current page |
| `POST` | `/api/browser/{goto,click,type,wait,eval}` | Direct browser control |
| `GET` | `/api/browser/page` | Current page info |
| `GET/POST` | `/api/browser/tabs`, `/api/browser/tabs/switch` | Tab management |
| `DELETE` | `/api/browser/tabs/{index}` | Close a tab |
| `GET` | `/api/browser/pdf` | Render page as PDF (Chromium-only) |
| `POST` | `/api/debug/cleanup` | Prune old debug artifacts |
| `GET/POST/DELETE` | `/api/sessions[/{id}]` | Session CRUD |
| `GET` | `/ws` | WebSocket: execution events (`execution.started/step/complete/error/paused`) |
| `GET` | `/swagger-ui`, `/api/openapi.json` | API documentation |

Workflow YAML validation is CLI-only: `automodus validate <path>`.

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `AUTOMODUS_CONFIG` | Config file path | `config/app.toml` |
| `AUTOMODUS_WORKFLOWS` | Workflows directory | `workflows/` |
| `AUTOMODUS_CHROME_PATH` | Chromium/Chrome binary path | auto-detect |
| `AUTOMODUS_FIREFOX_PATH` | Firefox binary path | auto-detect |
| `AUTOMODUS_LIGHTPANDA_PATH` | Lightpanda binary path | auto-detect |
| `AUTOMODUS_DEBUG` | Enable debug mode (`1`, `true`, `yes`, `on`) | unset |
| `AUTOMODUS_DEBUG_LEVEL` | Debug log level (`info`, `debug`, `trace`) | `info` |
| `AUTOMODUS_DEBUG_PROFILE` | Debug preset (`minimal`, `verbose`, `ci`, `demo`) | unset |
| `AUTOMODUS_DEBUG_DELAY` | Step delay in ms | `0` |
| `AUTOMODUS_DEBUG_CAPTURE` | Screenshot mode (`none`, `failure`, `before`, `after`, `all`) | `failure` |
| `AUTOMODUS_LOG_LEVEL` | Daemon log level | `info` |
| `AUTOMODUS_HTTP_HOST` / `AUTOMODUS_HTTP_PORT` | Daemon HTTP bind | `127.0.0.1` / `8080` |
| `AUTOMODUS_SOCKET_PATH` | Daemon socket path | platform data dir |
| `AUTOMODUS_MAX_SESSIONS` | Daemon max sessions | `10` |
| `AUTOMODUS_NO_PROXY` | Disable proxy env for browser | unset |
| `AUTOMODUS_DISABLE_IPV6` | Disable IPv6 for browser | unset |
| `RUST_LOG` / `LOG_JSON` | Log level / JSON log format | `info` / unset |
| `CHROME` / `FIREFOX` / `LIGHTPANDA` | Alternate binary-path env vars | unset |

Any `AUTOMODUS_*` variable also maps onto `config/app.toml` keys
(e.g. `AUTOMODUS_BROWSER_ENGINE=firefox`); debug variables are honored on the
`automodus run` path only (see [docs/DEBUG.md](docs/DEBUG.md)).

### Config File (`config/app.toml`)

```toml
[server]
host = "127.0.0.1"
port = 3000

[browser]
headless = true
engine = "chromium"  # "chromium" (CDP) | "firefox" (BiDi, no geckodriver) | "lightpanda" (CDP)
timeout_ms = 30000
# chrome_path = "/usr/bin/chromium"   # overrides auto-detection
# firefox_path = "/usr/bin/firefox"
# lightpanda_path = "/home/you/.local/bin/lightpanda"

[workflows]
directory = "workflows/"
auto_reload = true
```

See [config/app.example.toml](config/app.example.toml) for a commented template;
sections it marks as reserved are not read by the current loader.

## Project Structure

```
automodus/
├── src/
│   ├── actions/            # Action registry + shared action types
│   │   ├── registry.rs     # Registry, aliases, engine capabilities
│   │   └── control.rs      # emit / log actions
│   ├── api/                # REST API server (axum + utoipa)
│   │   ├── handlers.rs     # API endpoint handlers
│   │   ├── schemas.rs      # Request/response types
│   │   ├── server.rs       # Router setup, OpenAPI docs
│   │   ├── state.rs        # Shared application state
│   │   └── ws.rs           # WebSocket execution events
│   ├── core/               # Workflow execution engine
│   │   ├── engine.rs       # Action execution, loops, handlers
│   │   ├── app.rs          # AppCore (daemon/shell shared state)
│   │   ├── context.rs      # Step context and store
│   │   ├── template.rs     # Variable interpolation
│   │   └── json_path.rs    # JSONPath-style extraction
│   ├── daemon/             # Daemon process (mod, config, protocol)
│   ├── shell/              # Interactive shell client + command types
│   ├── modules/            # Automation modules
│   │   ├── browser/        # Browser automation (chromiumoxide / rustenium)
│   │   │   ├── session.rs  # SessionAdapter: engine dispatch
│   │   │   ├── adapter.rs  # Chromium CDP page adapter
│   │   │   ├── firefox.rs  # Firefox BiDi backend
│   │   │   ├── driver.rs   # Browser service management
│   │   │   ├── launch.rs   # Launch configuration helpers
│   │   │   └── actions/    # Browser-specific actions
│   │   └── http/           # HTTP client module (reqwest)
│   │       ├── client.rs   # HTTP client wrapper
│   │       └── actions/    # HTTP request actions
│   ├── workflow/           # YAML parsing and loading
│   │   ├── loader.rs       # File/directory loading
│   │   ├── parser.rs       # Validation (registry-backed)
│   │   └── schema.rs       # Workflow data structures
│   ├── triggers/           # Trigger types (api, schedule, webhook)
│   ├── utils/              # Shared utilities
│   │   ├── convert.rs      # YAML/JSON conversion
│   │   ├── debug.rs        # Debug artifact cleanup
│   │   ├── trace.rs        # Trace logger
│   │   ├── logging.rs      # Log initialization
│   │   └── metrics.rs      # Workflow metrics
│   ├── config.rs           # AppConfig loader (config/app.toml)
│   ├── lib.rs / error.rs
│   └── bin/
│       └── automodus.rs    # CLI entry point
├── config/                 # Configuration templates
├── examples/               # Example workflows (git submodule)
├── scripts/
│   └── smoke.sh            # Engine-matrix example smoke runner
├── tests/                  # Integration/session/shell/daemon tests
├── .github/workflows/      # CI (fmt, clippy, tests, smoke matrix)
└── data/screenshots/       # Default screenshot output
```

> Example workflows live in the `examples/` git submodule
> (`browser/`, `http/`, `compose/`, `control-flow/`, `debug/`, `whatsapp/`) —
> see `examples/README.md`. At runtime, point `AUTOMODUS_WORKFLOWS`
> at that directory or at your own `workflows/` directory.

## License

MIT License - see [LICENSE](LICENSE) for details.

---

<div align="center">

**Built with Rust by [Devstroop Technologies](https://devstroop.com)**

</div>
