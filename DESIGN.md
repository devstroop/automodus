# Automodus - Programmable Workflow Automation Platform

## Vision

A **general-purpose, event-driven workflow automation platform** with declarative YAML workflows.

## Core Concepts

### 1. Workflows
A workflow is a YAML-defined automation sequence that runs in browser instances.

```yaml
# workflows/example.yaml
name: checkout-workflow
version: "1.0"

# Browser configuration
browser:
  instances: 1                    # Number of parallel browser instances
  headless: false                 # Show browser window
  data_dir: "./data/{{workflow.name}}/{{instance.id}}"
  
# What triggers this workflow
on:
  # Expose as REST API endpoint
  api:
    path: /workflows/checkout
    method: POST
    
  # Schedule-based trigger
  schedule: "0 */6 * * *"         # Every 6 hours
  
  # Event-based trigger (from other workflows or external)
  event: cart.ready
  
# Input parameters (from trigger)
params:
  product_url:
    type: string
    required: true
  quantity:
    type: number
    default: 1

# Variables available throughout workflow
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
    wait_after: 2s
    
  - id: check_added
    action: wait_for
    selector: ".cart-count"
    condition: text.equals("{{params.quantity}}")
    timeout: 10s
    on_success:
      goto: checkout
    on_failure:
      emit: cart.error
      data:
        reason: "Item not added"
      abort: true
      
  - id: checkout
    action: goto
    url: "{{vars.base_url}}/checkout"
    
  - id: screenshot
    action: screenshot
    path: "./output/{{timestamp}}_checkout.png"
    
  - id: extract_total
    action: extract
    selector: ".order-total"
    attribute: text
    store_as: order_total
    
# What to emit/return when workflow completes
outputs:
  - name: total
    value: "{{order_total}}"
  - name: screenshot
    value: "{{steps.screenshot.output}}"
    
on_complete:
  emit: checkout.completed
  data:
    total: "{{order_total}}"
    
on_error:
  emit: checkout.failed
  screenshot: true
```

### 2. Actions (Built-in)

| Action | Description | Key Params |
|--------|-------------|------------|
| `goto` | Navigate to URL | `url` |
| `click` | Click element | `selector`, `button` |
| `type` | Type text | `selector`, `text`, `clear` |
| `wait_for` | Wait for element/condition | `selector`, `condition`, `timeout` |
| `screenshot` | Capture screenshot | `path`, `full_page` |
| `extract` | Extract data from page | `selector`, `attribute`, `store_as` |
| `eval` | Run JavaScript | `script`, `store_as` |
| `tab.new` | Open new tab | `url` |
| `tab.switch` | Switch to tab | `index` or `title` |
| `tab.close` | Close tab | `index` |
| `emit` | Emit event | `event`, `data` |
| `call` | Call another workflow | `workflow`, `params`, `await` |
| `http` | Make HTTP request | `method`, `url`, `body` |
| `sleep` | Wait fixed time | `duration` |
| `condition` | Conditional branch | `if`, `then`, `else` |
| `loop` | Iterate | `items`, `as`, `steps` |

### 3. Triggers

```yaml
on:
  # REST API trigger - creates endpoint
  api:
    path: /trigger/my-workflow
    method: POST
    auth: bearer          # optional auth requirement
    
  # Cron schedule
  schedule: "*/5 * * * *"  # Every 5 minutes
  
  # Event from another workflow or external webhook
  event: user.signup
  
  # Webhook (external systems call in)
  webhook:
    path: /webhook/stripe
    secret: "{{env.STRIPE_SECRET}}"
    
  # File watcher
  watch:
    path: "./input/*.csv"
    events: [created, modified]
    
  # Manual only (no auto-trigger)
  manual: true
```

### 4. Events & Pipelines

Workflows communicate via events:

```yaml
# workflow-a.yaml
steps:
  - id: process
    action: extract
    selector: ".data"
    store_as: result
    
on_complete:
  emit: data.processed
  data:
    result: "{{result}}"
    
---
# workflow-b.yaml (triggered by workflow-a)
on:
  event: data.processed
  
steps:
  - id: use_data
    action: log
    message: "Got: {{trigger.data.result}}"
```

### 5. Multi-Instance & Tabs

```yaml
browser:
  instances: 3                    # Run 3 parallel browser instances
  data_dir: "./profiles/{{instance.id}}"
  
steps:
  - id: open_tabs
    action: loop
    items: ["https://a.com", "https://b.com", "https://c.com"]
    as: url
    steps:
      - action: tab.new
        url: "{{url}}"
        
  - id: parallel_extract
    action: loop
    items: [0, 1, 2]
    as: tab_index
    parallel: true              # Run in parallel
    steps:
      - action: tab.switch
        index: "{{tab_index}}"
      - action: extract
        selector: "h1"
        store_as: "title_{{tab_index}}"
```

## Architecture

```
automodus/
├── Cargo.toml
├── config/
│   └── automodus.toml         # Server configuration
├── workflows/                  # User-defined YAML workflows
│   └── examples/
├── src/
│   ├── main.rs                # Entry point
│   ├── lib.rs                 # Library exports
│   │
│   ├── core/                  # Core execution engine
│   │   ├── mod.rs
│   │   ├── engine.rs          # Workflow execution engine
│   │   ├── context.rs         # Execution context (vars, params)
│   │   ├── runtime.rs         # Browser instance management
│   │   └── events.rs          # Event bus
│   │
│   ├── workflow/              # Workflow definition & parsing
│   │   ├── mod.rs
│   │   ├── schema.rs          # YAML schema types
│   │   ├── parser.rs          # YAML parser
│   │   ├── validator.rs       # Workflow validation
│   │   └── loader.rs          # Load workflows from directory
│   │
│   ├── actions/               # Built-in actions
│   │   ├── mod.rs             # Action registry
│   │   ├── navigate.rs        # goto, back, forward, reload
│   │   ├── interact.rs        # click, type, select, hover
│   │   ├── wait.rs            # wait_for, sleep
│   │   ├── extract.rs         # extract, eval
│   │   ├── capture.rs         # screenshot, pdf
│   │   ├── tabs.rs            # tab.new, tab.switch, tab.close
│   │   ├── control.rs         # condition, loop, call
│   │   └── http.rs            # http requests
│   │
│   ├── triggers/              # Trigger handlers
│   │   ├── mod.rs
│   │   ├── api.rs             # REST API triggers
│   │   ├── schedule.rs        # Cron scheduler
│   │   ├── events.rs          # Event triggers
│   │   └── webhook.rs         # Webhook handlers
│   │
│   ├── browser/               # Browser automation (from existing)
│   │   ├── mod.rs
│   │   ├── instance.rs        # Browser instance management
│   │   ├── cdp.rs             # Chrome DevTools Protocol
│   │   ├── tabs.rs            # Tab management
│   │   └── session.rs         # Session/profile management
│   │
│   ├── api/                   # REST API server
│   │   ├── mod.rs
│   │   ├── routes.rs          # API routes
│   │   ├── workflows.rs       # /api/workflows endpoints
│   │   ├── triggers.rs        # Dynamic trigger routes
│   │   └── events.rs          # /api/events endpoints
│   │
│   └── utils/                 # Utilities
│       ├── mod.rs
│       ├── template.rs        # {{variable}} interpolation
│       └── logging.rs
│
└── tests/
    └── workflows/             # Test workflow YAMLs
```

## API Endpoints

```
POST   /api/workflows                    # Upload/create workflow
GET    /api/workflows                    # List all workflows
GET    /api/workflows/{name}             # Get workflow details
DELETE /api/workflows/{name}             # Delete workflow
POST   /api/workflows/{name}/run         # Run workflow manually
GET    /api/workflows/{name}/runs        # List workflow runs
GET    /api/workflows/{name}/runs/{id}   # Get run status/output

POST   /api/events                   # Emit event
GET    /api/events/stream            # SSE event stream

GET    /api/browser/instances        # List browser instances
POST   /api/browser/instances        # Create instance
DELETE /api/browser/instances/{id}   # Stop instance

# Dynamic routes from workflow triggers
POST   /workflows/checkout           # From workflow's on.api.path
POST   /webhook/stripe               # From workflow's on.webhook.path
```

## Template Syntax

```yaml
# Variables
"{{params.email}}"              # From trigger params
"{{vars.base_url}}"             # From workflow vars
"{{env.API_KEY}}"               # Environment variable
"{{steps.extract_data.output}}" # Previous step output
"{{instance.id}}"               # Current browser instance
"{{workflow.name}}"             # Current workflow name
"{{timestamp}}"                 # Current timestamp
"{{random.uuid}}"               # Random UUID

# Expressions
"{{params.count + 1}}"
"{{params.name | upper}}"
"{{params.items | join(', ')}}"
```

## Configuration

```toml
# config/automodus.toml
[server]
host = "0.0.0.0"
port = 8080

[browser]
executable = "/usr/bin/chromium"
default_headless = true
max_instances = 10
default_data_dir = "./data/browser"

[workflows]
directory = "./workflows"
watch = true                    # Hot-reload on changes

[events]
buffer_size = 1000
retention = "24h"

[auth]
enabled = true
secret_key = "your-secret"
```

## Example Workflows

### 1. Simple Screenshot Service

```yaml
name: screenshot-service
on:
  api:
    path: /screenshot
    method: POST

params:
  url:
    type: string
    required: true
  full_page:
    type: boolean
    default: false

steps:
  - action: goto
    url: "{{params.url}}"
  - action: wait_for
    selector: body
    timeout: 10s
  - action: screenshot
    full_page: "{{params.full_page}}"
    store_as: image

outputs:
  - name: screenshot
    value: "{{image}}"
    type: base64
```

### 2. Multi-Tab Data Aggregator

```yaml
name: price-aggregator
browser:
  instances: 1
  
on:
  schedule: "0 * * * *"  # Every hour

vars:
  sites:
    - url: "https://amazon.com/product/123"
      selector: "#price"
    - url: "https://ebay.com/item/456"  
      selector: ".price"
    - url: "https://walmart.com/ip/789"
      selector: "[data-price]"

steps:
  - id: open_sites
    action: loop
    items: "{{vars.sites}}"
    as: site
    steps:
      - action: tab.new
        url: "{{site.url}}"
        
  - id: extract_prices
    action: loop
    items: "{{vars.sites}}"
    as: site
    index_as: i
    steps:
      - action: tab.switch
        index: "{{i}}"
      - action: extract
        selector: "{{site.selector}}"
        store_as: "price_{{i}}"
        
  - action: emit
    event: prices.collected
    data:
      prices: ["{{price_0}}", "{{price_1}}", "{{price_2}}"]
```

### 3. Form Automation with Retry

```yaml  
name: form-submit
on:
  event: form.requested

params:
  form_data:
    type: object

steps:
  - action: goto
    url: "https://example.com/form"
    
  - action: loop
    items: "{{params.form_data | entries}}"
    as: field
    steps:
      - action: type
        selector: "[name='{{field.key}}']"
        text: "{{field.value}}"
        clear: true
        
  - id: submit
    action: click
    selector: "button[type=submit]"
    retry:
      attempts: 3
      delay: 2s
      
  - action: wait_for
    condition: url.contains("/success")
    timeout: 30s
    on_failure:
      - action: screenshot
        path: "./errors/{{timestamp}}.png"
      - abort: true
        error: "Form submission failed"

outputs:
  - name: success
    value: true
```

## Phase 1 MVP Scope

1. **Core**: YAML parser, execution engine, variable interpolation
2. **Actions**: goto, click, type, wait_for, screenshot, extract, sleep
3. **Triggers**: API endpoint, manual
4. **Browser**: Single instance management, basic tab support
5. **API**: Workflow CRUD, manual run, run status

## Future Phases

- Phase 2: Multi-instance parallel execution, event bus, scheduling
- Phase 3: Plugins/extensions, WASM actions, scripting
- Phase 4: Web UI, workflow designer, monitoring dashboard
