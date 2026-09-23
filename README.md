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

Automodus is a **programmable workflow automation platform** that lets you define automation workflows in YAML and trigger them via:

- **REST API** - Call workflows like functions
- **Schedules** - Cron-based automation
- **Events** - React to system events
- **Webhooks** - Trigger from external systems

Think of it as **"automation as code"** — define once, run anywhere.

### Modules

| Module | Description |
|--------|-------------|
| **Browser** | Chromium automation via CDP (navigate, click, extract, screenshot) |
| **HTTP** | REST API requests (GET, POST, PUT, PATCH, DELETE) |
| **LLM** | AI integration (planned) |
| **Storage** | Data persistence (planned) |

## Features

| Feature | Description |
|---------|-------------|
| **YAML Workflows** | Define automation in declarative YAML |
| **Built-in Actions** | 20+ actions: navigate, click, type, extract, screenshot, http, etc. |
| **HTTP Module** | Make API requests within workflows (GET, POST, PUT, PATCH, DELETE) |
| **Multi-Instance** | Run multiple browser instances with isolated data directories |
| **Multi-Tab** | Control multiple tabs within each browser instance |
| **Template Engine** | `{{params.x}}`, `{{store.y}}`, `{{env.Z}}` interpolation |
| **Event System** | Emit and react to events between workflows |
| **REST API** | Trigger workflows via HTTP endpoints |
| **CLI** | Run, validate, and manage workflows from command line |

## Quick Start

### Prerequisites

- **Rust** 1.70+
- **Chrome/Chromium** browser installed
- macOS, Linux, or Windows

### Installation

```bash
# Clone
git clone https://github.com/devstroop/automodus.git
cd automodus

# Build
cargo build --release

# Validate example workflows (workspace sibling, see ../examples/README.md)
./target/release/automodus validate ../examples/

# List available workflows
./target/release/automodus list
```

### Create Your First Workflow

Create `workflows/hello.yaml` in your own workflows directory
(`AUTOMODUS_WORKFLOWS` defaults to `workflows/`; examples live in `../examples/`):

```yaml
name: hello_world
description: Simple example workflow

on:
  api:
    path: /workflows/hello
    method: POST

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
./target/release/automodus run workflows/hello.yaml

# Via API (when server is running)
curl -X POST http://localhost:3000/workflows/hello \
  -H "Content-Type: application/json" \
  -d '{"url": "https://example.com"}'
```

## Workflow YAML Format

### Basic Structure

```yaml
name: workflow-name           # Required: unique identifier
version: "1.0"            # Optional: version string
description: "..."        # Optional: description

# How the workflow can be triggered
on:
  api:                    # REST API trigger
    path: /workflows/my-workflow
    method: POST
  schedule: "0 * * * *"   # Cron schedule
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

| Action | Description | Required Params |
|--------|-------------|-----------------|
| `goto` | Navigate to URL | `url` |
| `click` | Click element | `selector` |
| `type` | Type text | `selector`, `text` |
| `wait_for` | Wait for element/URL | `selector` or `url` |
| `sleep` | Pause execution | `duration` (e.g., "2s") |
| `extract` | Extract data from page | `selector`, `store_as` |
| `eval` | Execute JavaScript | `script` |
| `screenshot` | Capture screenshot | (optional: `path`, `full_page`) |
| `tab.new` | Open new tab | (optional: `url`) |
| `tab.switch` | Switch to tab | `index` |
| `tab.close` | Close tab | (optional: `index`) |
| `http.get` | HTTP GET request | `url` |
| `http.post` | HTTP POST request | `url` |
| `http.put` | HTTP PUT request | `url` |
| `http.patch` | HTTP PATCH request | `url` |
| `http.delete` | HTTP DELETE request | `url` |
| `http.request` | Generic HTTP request | `method`, `url` |
| `emit` | Emit event | `event` |
| `log` | Log message | `message` |

### Template Syntax

Use `{{...}}` for variable interpolation:

| Reference | Description |
|-----------|-------------|
| `{{params.name}}` | Input parameter |
| `{{vars.name}}` | Workflow variable |
| `{{store.name}}` | Stored value from extract/eval |
| `{{steps.id.output}}` | Output from step with id |
| `{{env.NAME}}` | Environment variable |
| `{{instance.id}}` | Browser instance ID |
| `{{timestamp}}` | Current ISO timestamp |

## CLI Reference

```bash
# Run a specific workflow (see ../examples/ for ready-made workflows)
automodus run ../examples/browser/search_form.yaml

# Start the API server
automodus serve

# Validate workflow files
automodus validate ../examples/

# List all loaded workflows
automodus list

# Show help
automodus --help
```

## API Reference

### Documentation

Access interactive API documentation via Swagger UI:

```bash
# Start the server
automodus serve

# Open in browser
open http://localhost:3000/swagger-ui
```

- **Swagger UI**: `http://localhost:3000/swagger-ui`
- **OpenAPI JSON**: `http://localhost:3000/api/openapi.json`

### Workflow Execution

```bash
# Trigger a workflow via its API endpoint
POST /workflows/{path}
Content-Type: application/json

{
  "param1": "value1",
  "param2": "value2"
}
```

### Workflow Management

```bash
# List all workflows
GET /api/v1/workflows

# Get workflow details
GET /api/v1/workflows/{name}

# Validate a workflow
POST /api/v1/workflows/validate
Content-Type: application/yaml

<yaml content>
```

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `AUTOMODUS_CONFIG` | Config file path | `config/app.toml` |
| `AUTOMODUS_WORKFLOWS` | Workflows directory | `workflows/` |
| `RUST_LOG` | Log level | `info` |

### Config File (`config/app.toml`)

```toml
[server]
host = "127.0.0.1"
port = 3000

[browser]
headless = true
executable = ""  # Auto-detect Chrome

[workflows]
directory = "workflows/"
auto_reload = true
```

## Project Structure

```
automodus/
├── src/
│   ├── api/                # REST API server (axum + utoipa)
│   │   ├── handlers.rs     # API endpoint handlers
│   │   ├── schemas.rs      # Request/response types
│   │   ├── server.rs       # Router setup, OpenAPI docs
│   │   └── state.rs        # Shared application state
│   ├── core/               # Workflow execution engine
│   │   ├── engine.rs       # Action execution logic
│   │   ├── context.rs      # Step context and store
│   │   └── template.rs     # Variable interpolation
│   ├── modules/            # Automation modules
│   │   ├── browser/        # Browser automation (chromiumoxide)
│   │   │   ├── adapter.rs  # Chrome page adapter
│   │   │   ├── driver.rs   # Browser service management
│   │   │   ├── launch.rs   # Launch configuration helpers
│   │   │   └── actions/    # Browser-specific actions
│   │   └── http/           # HTTP client module (reqwest)
│   │       ├── client.rs   # HTTP client wrapper
│   │       └── actions/    # HTTP request actions
│   ├── workflow/           # YAML parsing and loading
│   │   ├── loader.rs       # File/directory loading
│   │   ├── parser.rs       # YAML parsing logic
│   │   └── schema.rs       # Workflow data structures
│   ├── triggers/           # Trigger types (api, schedule, webhook)
│   ├── utils/              # Shared utilities
│   │   ├── convert.rs      # YAML/JSON conversion
│   │   ├── logging.rs      # Log initialization
│   │   └── metrics.rs      # Workflow metrics
│   └── bin/
│       └── automodus.rs    # CLI entry point
├── config/                 # Configuration files
├── templates/              # HTML templates (askama)
└── data/screenshots/       # Default screenshot output
```

> Example workflows live outside this repo in the workspace sibling
> `../examples/` (`browser/`, `http/`, `compose/`, `whatsapp/`) —
> see `../examples/README.md`. At runtime, point `AUTOMODUS_WORKFLOWS`
> at that directory or at your own `workflows/` directory.

## License

MIT License - see [LICENSE](LICENSE) for details.

---

<div align="center">

**Built with Rust by [Devstroop Technologies](https://devstroop.com)**

</div>
