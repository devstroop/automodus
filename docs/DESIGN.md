# Automodus - Programmable Workflow Automation Platform

> **Status of this document:** the design spec, checked against the current
> implementation. Sections are marked **✅ implemented** (matches the code;
> every YAML snippet validates with `automodus validate`) or **📋 planned**
> (schema may accept the syntax, but nothing wires it up yet).

## Vision

A **general-purpose workflow automation platform** with declarative YAML
workflows, driven by the CLI, the REST API, or the interactive shell.

## Core Concepts

### 1. Workflows ✅

A workflow is a YAML-defined automation sequence that runs in a browser
instance.

```yaml
# workflows/checkout.yaml
name: checkout-workflow
version: "1.0"
description: Open a product page and capture the order total

# Browser configuration
browser:
  headless: true                 # Show browser window when false
  data_dir: "./data/{{workflow.name}}/{{instance.id}}"
  width: 1280
  height: 720

# Input parameters (supplied by the runner: CLI key=value or API "params")
params:
  product_url:
    type: string
    required: true
  quantity:
    type: number
    default: 1

# Variables available throughout the workflow
vars:
  base_url: "https://shop.example.com"

# The automation steps
steps:
  - id: navigate
    action: goto
    url: "{{params.product_url}}"

  - id: add_to_cart
    action: click
    selector: "button.add-to-cart"

  - id: check_added
    action: wait_for
    selector: ".cart-count"
    timeout: 10000
    # Step-level gate: rendered before evaluation, supports == / != or
    # truthiness (true/yes/1/false/no/0/empty)
    if: "{{params.quantity}} == 1"
    on_failure:
      goto: checkout

  - id: checkout
    action: goto
    url: "{{vars.base_url}}/checkout"

  - id: screenshot
    action: screenshot
    path: "./output/{{timestamp}}_checkout.png"

  - id: extract_total
    action: extract
    selector: ".order-total"
    store_as: order_total

# What to return when the workflow completes
# (map of name -> template; read back as {{output.name}} is not supported —
# the map is resolved into WorkflowResult.output)
output:
  total: "{{store.order_total}}"
  url: "{{params.product_url}}"

on_complete:
  emit:
    event: checkout.completed

on_error:
  screenshot: true
  emit:
    event: checkout.failed
```

Note: workflow-level `on_complete`/`on_error` emits receive an automatic
payload (workflow name, duration, step, error) — custom `data:` on those two
handlers is accepted by the schema but not included. Step-handler
(`on_success`/`on_failure`) and action-level `emit` **do** render and include
their `data:` map.

Key schema types (in `src/workflow/schema.rs`):

| Type | Purpose |
|------|---------|
| `Workflow` | Top-level: name, version, browser, on, params, vars, steps, output, on_complete, on_error, debug |
| `Step` | id, action, flattened params, retry, `if:`, emit, on_success, on_failure, debug |
| `RetryConfig` | `max` (attempts) + `delay_ms` |
| `StepHandler` | One of: `goto:`, `emit:`+`data:`, `abort:`+`error:`, `steps:` (or a bare list of steps) |
| `DebugConfig` | Optional debug settings (`Option<T>` fields for merge) |
| `ResolvedDebugConfig` | Concrete debug values after merging |
| `Triggers` | api, schedule, event, webhook, watch, manual (📋 schema-only) |

### 2. Actions ✅

28 registered actions + 3 engine pseudo-actions. Validation is
registry-backed — an unknown action fails `automodus validate` and prints the
known list. Aliases: `navigate` → `goto`, `input` → `type`, `wait` → `wait_for`.

| Action | Description | Key Params |
|--------|-------------|------------|
| `goto` | Navigate to URL | `url`, `wait_until` |
| `back` / `forward` / `reload` | History navigation / reload | — |
| `click` | Click element | `selector` |
| `type` | Type text | `selector`, `text`, `clear` |
| `hover` | Hover element | `selector` |
| `select` | Select dropdown option | `selector`, `value` |
| `wait_for` | Wait for element/URL/text | `selector` \| `url` \| `text`, `timeout`, `state` |
| `sleep` | Wait fixed time | `duration` or `ms` |
| `extract` | Extract data from page | `selector`, `attribute`, `many`, `store_as` (or `as`) |
| `eval` | Run JavaScript | `script`, `store_as` |
| `screenshot` | Capture screenshot | `path`, `full_page` (literal bool) |
| `tab.list` / `tab.new` / `tab.switch` / `tab.close` | Tab management | `url` / `index` / `index` (literal ints) |
| `upload` / `wait_upload` / `file_chooser` | File inputs (Chromium-only) | `file_path`/`files`, `trigger`, `enabled` |
| `http.get/post/put/patch/delete` | HTTP requests | `url`, `headers`, `body`, `store_as` |
| `http.request` | Generic HTTP request | `method`, `url` |
| `emit` | Emit event (recorded on the event bus) | `event`, `data` |
| `log` | Log message | `message` |
| `loop` (pseudo) | Iterate items, run `steps` per item | `items`, `as`, `steps`, `index_as` |
| `condition` (pseudo) | Branch | `if`, `then`, `else` |
| `call` (pseudo) | Invoke another workflow | `workflow`, `params` |

**Engine notes:**

- `upload`/`wait_upload`/`file_chooser` require Chromium capabilities
  (`file_input`, `file_chooser`); `screenshot`/`eval`/tabs work on all engines
  (Lightpanda is single-page — no tab management).
- `screenshot` has no `store_as`; read the result via `{{steps.<id>...}}`.
- `tab.switch.index` must be a **literal integer** — template-rendered strings
  fail `as_u64()`.
- Booleans that gate behavior (`full_page`) must be literal too; branch on
  params with `if:` instead (see `examples/browser/screenshot.yaml`).

### 3. Triggers 📋 (schema-only)

The `on:` block is parsed and validated (`automodus list` shows it), but no
dispatcher exists yet — cron/webhook/event routing is **planned**. Run
workflows explicitly with `automodus run` or
`POST /api/workflows/{name}/run`.

```yaml
on:
  # REST API trigger (planned: dynamic endpoint from path)
  api:
    path: /trigger/my-workflow
    method: POST

  # Cron schedule (planned)
  schedule: "*/5 * * * *"  # Every 5 minutes

  # Event from another workflow (planned)
  event: user.signup

  # Webhook (planned)
  webhook:
    path: /webhook/stripe
    secret: "whsec_..."

  # File watcher (planned)
  watch:
    path: "./input/*.csv"
    events: [created, modified]

  # Manual only (the default when `on:` is omitted)
  manual: true
```

### 4. Events & Pipelines 📋 (partially implemented)

`emit` (action or `on_complete`/`on_error`/step-handler form) records events
on an in-process broadcast bus, and `on:`-declared `event:` triggers are
schema-validated — but **nothing subscribes yet**, so one workflow cannot
trigger another. `{{trigger.*}}` template roots do not exist.

```yaml
# workflow-a.yaml — emit as a step action: data IS rendered and recorded
steps:
  - id: process
    action: extract
    selector: ".data"
    store_as: result

  - action: emit
    event: data.processed
    data:
      result: "{{store.result}}"

---
# workflow-b.yaml — the declaration validates today, but nothing dispatches it
on:
  event: data.processed

steps:
  - id: use_data
    action: log
    message: "Event recorded"
```

### 5. Multi-Instance & Tabs ✅ (single instance today)

`browser.instances` is accepted by the schema, but a run launches one browser
per execution; isolation comes from `data_dir` (per-run temp profiles are
cleaned up automatically). Tab control works on Chromium and Firefox:

```yaml
browser:
  instances: 3                    # accepted; one browser is launched per run
  data_dir: "./profiles/{{instance.id}}"

steps:
  # Literal item lists iterate directly
  - id: open_tabs
    action: loop
    items: ["https://a.example.com", "https://b.example.com", "https://c.example.com"]
    as: url
    steps:
      - action: tab.new
        url: "{{vars.url}}"

  # Loop variables are read through vars.<as> / vars.<index_as>
  - id: log_indexes
    action: loop
    items: [0, 1, 2]
    as: tab_index
    index_as: i
    steps:
      - action: log
        message: "iteration {{vars.i}} -> tab {{vars.tab_index}}"

  # To iterate a stored YAML list, render it as JSON so it stays a sequence:
  #   items: "{{vars.sites | json}}"
  # (parallel loops are rejected by the validator)
```

## Architecture ✅

```
automodus/
├── Cargo.toml
├── config/app.example.toml     # Config template (config/app.toml is gitignored)
├── examples/                   # Example workflow pack (git submodule)
│   ├── browser/                # Browser demos
│   ├── compose/                # Composition demos
│   ├── control-flow/           # loop / condition / call demos
│   ├── debug/                  # Debug-mode demos
│   ├── http/                   # HTTP demos
│   └── whatsapp/               # WhatsApp pack
├── scripts/smoke.sh            # Engine-matrix example smoke runner
├── src/
│   ├── bin/automodus.rs        # CLI entry point (hand-rolled arg parsing)
│   ├── lib.rs / error.rs / config.rs
│   ├── core/                   # Execution engine
│   │   ├── engine.rs           # Workflow execution, loops, handlers, debug
│   │   ├── context.rs          # ExecutionContext (vars, params, store)
│   │   ├── app.rs              # AppCore (daemon/shell shared state)
│   │   ├── template.rs         # {{variable}} interpolation (+ | json)
│   │   └── json_path.rs        # JSONPath-style extraction
│   ├── workflow/               # Workflow definition & parsing
│   │   ├── schema.rs           # YAML schema types
│   │   ├── parser.rs           # Registry-backed validation
│   │   └── loader.rs           # Load workflows from directory
│   ├── actions/                # Action registry + shared types
│   │   ├── registry.rs         # Action/BrowserHandle traits, capabilities
│   │   └── control.rs          # emit, log
│   ├── modules/
│   │   ├── browser/            # Multi-engine browser automation
│   │   │   ├── session.rs      # SessionAdapter dispatch + Lightpanda CDP
│   │   │   ├── adapter.rs      # Chromium CDP (chromiumoxide)
│   │   │   ├── firefox.rs      # Firefox BiDi (rustenium)
│   │   │   ├── launch.rs       # launch_session() per engine
│   │   │   ├── driver.rs / selector.rs
│   │   │   └── actions/        # navigate, interact, wait, extract, ...
│   │   └── http/               # HTTP client + http.* actions
│   ├── daemon/                 # Daemon (mod, config, protocol)
│   ├── shell/                  # Interactive shell client
│   ├── api/                    # REST API + WebSocket (server, handlers, ws)
│   ├── triggers/               # Trigger type re-exports (schema-only today)
│   └── utils/                  # logging, debug cleanup, trace, metrics
├── tests/                      # Integration/session/shell/daemon suites
└── .github/workflows/ci.yml    # fmt, clippy, tests, 3-engine smoke matrix
```

## API Endpoints ✅

```
GET    /api/health                      # Health check
GET    /api/workflows                   # List workflows
POST   /api/workflows/reload            # Rescan workflows directory
GET    /api/workflows/{name}            # Workflow details
POST   /api/workflows/{name}/run        # Run workflow {"params": {...}}
GET    /api/executions[/{id}]           # Execution history
DELETE /api/executions/{id}             # Cancel execution
GET    /api/browser/screenshot          # Screenshot
POST   /api/browser/{goto,click,type,wait,eval}
GET    /api/browser/page                # Page info
GET/POST /api/browser/tabs              # List / open tabs
POST   /api/browser/tabs/switch         # Switch tab
DELETE /api/browser/tabs/{index}        # Close tab
GET    /api/browser/pdf                 # PDF (Chromium-only)
POST   /api/debug/cleanup               # Prune debug artifacts
GET/POST/DELETE /api/sessions[/{id}]    # Session CRUD
WS     /ws                              # execution.started/step/complete/error/paused
GET    /swagger-ui, /api/openapi.json   # OpenAPI docs
```

Validation is CLI-only (`automodus validate [path]`, exit 1 when invalid).
There is no upload/delete workflow endpoint and no dynamic trigger routing
(see 📋 above).

## Template Syntax ✅

```yaml
# Roots (validated allow-list)
# {{params.email}}               Input parameter
# {{vars.base_url}}              Workflow variable (loop items: {{vars.<as>}})
# {{store.order_total}}          Value stored by extract/eval
# {{steps.extract_data.output}}  Output from a step id
# {{env.API_KEY}}                Environment variable
# {{instance.id}}                Browser instance id
# {{workflow.name}}              Current workflow name/id
# {{timestamp}}                  Current RFC3339 timestamp
# {{x | json}}                   Only built-in filter (JSON-serialize x)
#
# Conditions (step `if:`, condition action) — rendered first, then:
#   "left == right" / "left != right"   (case-insensitive string compare)
#   "true|yes|1" / "false|no|0|<empty>" (truthiness)

# Not supported (📋 or nonexistent): arithmetic ({{params.count + 1}}),
# pipes other than | json (| upper, | join, | entries), {{random.*}},
# {{trigger.*}}, ${env.VAR} / ${locators.*} substitution.
```

## Configuration ✅

```toml
# config/app.toml (template: config/app.example.toml)
[server]
host = "127.0.0.1"
port = 3000

[browser]
engine = "chromium"        # chromium | firefox | lightpanda
headless = true
timeout_ms = 30000
# chrome_path = "/usr/bin/chromium"
# firefox_path = "/usr/bin/firefox"
# lightpanda_path = "/home/you/.local/bin/lightpanda"

[workflows]
directory = "workflows"   # reserved: not read — use AUTOMODUS_WORKFLOWS
auto_reload = true        # reserved: never read
```

Any `AUTOMODUS_*` environment variable overrides a config key
(`AUTOMODUS_BROWSER_ENGINE=firefox`, …). Workflow-level `debug:` merges with
`AUTOMODUS_DEBUG*` and CLI flags on the `automodus run` path only
(see [DEBUG.md](DEBUG.md)). Daemon-specific keys (`[daemon]`, `[http]`, …) are
defined in `src/daemon/config.rs` but its TOML loader is currently unused —
the daemon runs on defaults.

## Example Workflows ✅

All three patterns below validate today (`automodus validate`).

### 1. Simple Screenshot Service

```yaml
name: screenshot-service

params:
  url:
    type: string
    default: "https://books.toscrape.com/"
  full_page:
    type: boolean
    default: false

steps:
  - action: goto
    url: "{{params.url}}"

  - action: wait_for
    selector: body
    timeout: 15000

  # Booleans gate behavior — branch instead of templating the flag
  - id: capture_viewport
    action: screenshot
    path: "data/screenshots/service.png"
    full_page: false
    if: "{{params.full_page}} != true"

  - id: capture_full
    action: screenshot
    path: "data/screenshots/service.png"
    full_page: true
    if: "{{params.full_page}} == true"

output:
  path: "data/screenshots/service.png"
  url: "{{params.url}}"
```

### 2. Multi-Tab Data Aggregator

```yaml
name: price-aggregator

vars:
  sites:
    - url: "https://books.toscrape.com/"
      selector: ".product_pod"
    - url: "https://books.toscrape.com/catalogue/category/books/travel_2/index.html"
      selector: ".product_pod"

steps:
  # One browser, one tab: iterate the stored list and scrape sequentially
  - id: scrape
    action: loop
    items: "{{vars.sites | json}}"
    as: site
    index_as: i
    steps:
      - action: goto
        url: "{{vars.site.url}}"
      - action: wait_for
        selector: "{{vars.site.selector}}"
        timeout: 15000
      - action: extract
        selector: "{{vars.site.selector}} h3 a"
        attribute: title
        store_as: "title_{{vars.i}}"

  - action: emit
    event: prices.collected
    data:
      first: "{{store.title_0}}"
      second: "{{store.title_1}}"
```

### 3. Form Automation with Retry

```yaml
name: form-submit

params:
  form_data:
    type: object
    default: {}

steps:
  - action: goto
    url: "https://example.com/form"

  # Literal field list — `| entries` does not exist; loop over pairs directly
  - action: loop
    items:
      - { key: email, value: "robot@example.com" }
      - { key: note, value: "automated" }
    as: field
    steps:
      - action: type
        selector: "[name='{{vars.field.key}}']"
        text: "{{vars.field.value}}"
        clear: true

  - id: submit
    action: click
    selector: "button[type=submit]"
    retry:
      max: 3
      delay_ms: 2000

  - action: wait_for
    url: "contains:/success"
    timeout: 30000
    on_failure:
      steps:
        - action: screenshot
          path: "data/screenshots/form_error.png"
        - action: log
          message: "Form submission failed"

output:
  success: true
```

`on_failure` / `on_success` handlers are a **single** object — pick one of
`goto:`, `emit:` (+`data:`), `abort:` (+`error:`), or `steps:` (or a bare
list of steps). Combining keys (e.g. `emit:` + `abort:`) is not supported: the
first matching form wins and extra keys are ignored.

## Implementation Status

| Area | Status |
|------|--------|
| YAML parser, registry-backed validator, template engine | ✅ shipped |
| 28 actions + `loop` / `condition` / `call`, step handlers, step retry | ✅ shipped |
| Chromium (CDP), Firefox (BiDi), Lightpanda engines + capability gates | ✅ shipped |
| CLI (`run`/`validate`/`list`/`shell`/`daemon`), REST API, WebSocket, daemon | ✅ shipped |
| Debug system (profiles, captures, pause, trace; see DEBUG.md) | ✅ shipped (console/network buffering Chromium-only, not yet surfaced) |
| CI: fmt, clippy, tests, 3-engine smoke matrix | ✅ shipped |
| Trigger dispatch (cron / webhook / dynamic API routes) | 📋 planned |
| Cross-workflow event subscriptions | 📋 planned |
| Surfacing console/network captures via API/shell | 📋 planned |
| Daemon TOML config loading (loader exists, not wired) | 📋 planned |
| Multi-instance parallel execution per workflow | 📋 planned |
| Web UI / workflow designer | 📋 planned |
