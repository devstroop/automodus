# Debug Mode Reference

Debug mode provides visibility into workflow execution for troubleshooting
selector issues, timing problems, and understanding automation behavior.
Everything below was verified against the code during the September 2026
docs-vs-code revision; the original implementation plan was moved to
[archive/plan-debug-capture.md](archive/plan-debug-capture.md).

## Current State

| Component | Status | Notes |
|-----------|--------|-------|
| `DebugConfig` struct | ✅ Implemented | `workflow/schema.rs` |
| `debug` field on Workflow | ✅ Implemented | `workflow/schema.rs` |
| `debug` field on Step | ✅ Implemented | `workflow/schema.rs` |
| CLI `--debug` flags | ✅ Implemented | `bin/automodus.rs` |
| Engine debug context | ✅ Implemented | `core/context.rs`, `core/engine.rs` |
| Screenshot capture | ✅ Implemented | `capture` modes wired in `core/engine.rs` |
| Delay / pause / highlight | ✅ Implemented | `ShellPauseHandler`, `WebSocketPauseHandler` |
| CDP console listener | ⚠️ Implemented, not surfaced | Listeners always start; buffers are never drained |
| CDP network listener | ⚠️ Implemented, not surfaced | Same as console listener |
| `level:` (info/debug/trace) | ⚠️ Schema-only | Stored/merged/echoed; log verbosity actually comes from `RUST_LOG` |
| `TraceLogger` (trace JSONL) | ❌ Not wired | Implemented in `utils/trace.rs` but never instantiated; `trace.jsonl` is not written |

## Configuration Levels

### 1. Workflow-Level Debug

```yaml
name: my_workflow
debug:
  enabled: true
  level: debug        # info | debug | trace
  capture: failure    # none | failure | before | after | all
  highlight: true     # Flash red border before interaction
  delay: 500          # ms delay between actions
  console: true       # Buffer console output (not yet surfaced)
  network: true       # Buffer network requests (not yet surfaced)
  
steps:
  - action: click
    selector: "text:Submit"
```

Or use a profile for common presets:

```yaml
name: my_workflow
debug:
  profile: verbose    # minimal | verbose | ci | demo
  
steps:
  - action: click
    selector: "text:Submit"
```

### 2. Step-Level Debug (Override)

```yaml
steps:
  - id: problematic_click
    action: click
    selector: "text:Submit"
    debug:
      enabled: true
      capture: all      # Capture before and after this step
      pause: true       # Prompt [c]ontinue/[s]kip/[a]bort before the step
```

### 3. CLI Override

```bash
# Enable debug for any workflow
automodus run workflow.yaml --debug

# Set the debug level (schema-only today; see Log Levels)
automodus run workflow.yaml --debug=trace

# With delay between actions (great for demos)
automodus run workflow.yaml --debug --delay=1000

# Capture screenshots for all steps
automodus run workflow.yaml --debug --capture=all

# Use a profile
automodus run workflow.yaml --debug --profile=ci
```

### 4. Environment Variable

Environment variables are honored on the `automodus run` path only (not by
the shell or API):

```bash
AUTOMODUS_DEBUG=true automodus run workflow.yaml
AUTOMODUS_DEBUG_LEVEL=trace automodus run workflow.yaml
AUTOMODUS_DEBUG_PROFILE=ci automodus run workflow.yaml
AUTOMODUS_DEBUG_DELAY=500 automodus run workflow.yaml
AUTOMODUS_DEBUG_CAPTURE=all automodus run workflow.yaml
```

## Debug Options Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | bool | false | Master switch for debug mode |
| `level` | string | info | **Schema-only today**: stored and echoed, but not consulted — log verbosity comes from `RUST_LOG` |
| `capture` | string | failure | Screenshot mode: `none`, `failure`, `before`, `after`, `all` (applies even when `enabled: false`) |
| `highlight` | bool | false | Flash element with red border before interaction |
| `delay` | int | 0 | Milliseconds to pause between actions |
| `pause` | bool | false | Wait for user input before continuing |
| `console` | bool | false | Buffer browser console output (captured, not yet surfaced — see Known Gaps) |
| `network` | bool | false | Buffer network requests (captured, not yet surfaced — see Known Gaps) |
| `profile` | string | - | Preset configuration: `minimal`, `verbose`, `ci`, `demo` |

### Profiles

Profiles provide preset configurations for common scenarios:

| Profile | What it sets |
|---------|--------------|
| `minimal` | `capture: failure` |
| `verbose` | `level: trace`†, `capture: all`, `console: true`†, `network: true`† |
| `ci` | `capture: failure`, `console: true`†, `network: true`† |
| `demo` | `highlight: true`, `delay: 1000` |

Profile settings can be overridden by explicit options:

```yaml
debug:
  profile: ci
  capture: all    # Override ci's capture: failure
```

† currently inert (see Known Gaps in [SHELL.md](SHELL.md)).

> **Caveat:** a workflow-level `profile:` is currently applied only for
> sub-workflows invoked with `call:`. On the `automodus run` path, pass the
> preset on the CLI instead (`--profile=ci`), which also enables debug mode.

### Capture Modes

| Mode | Description |
|------|-------------|
| `none` | No screenshots captured |
| `failure` | Capture only when a step fails (default) |
| `before` | Capture before each action |
| `after` | Capture after each action |
| `all` | Capture before and after each action |

The mode is evaluated independently of `enabled:` — with defaults, a failed
step produces a screenshot in `data/debug/` even with debug off. Set
`capture: none` to disable that.

### Log Levels

The `level` option (`info` | `debug` | `trace`) is parsed, merged, and
printed with the "Debug mode enabled" banner, but it does not currently
change log output. Console/network entries are emitted at tracing `debug`
level regardless, so actual verbosity is controlled by `RUST_LOG` (e.g.
`RUST_LOG=debug`).

## Debug Output

### Trace Output (level: trace)

⚠️ Not wired yet: `TraceLogger` (`src/utils/trace.rs`) would append JSONL
events to `data/debug/trace.jsonl`, but it is never instantiated at runtime,
so no trace file is produced. The shell `trace <file>` command only prints a
"Running with trace logging..." banner and executes the workflow normally.

### Browser Console (console: true)

The CDP console listener (`start_console_listener()`) buffers every
`console.*` call as `{timestamp, level, message}` and emits each entry at
tracing `debug` level (shown here in its `ConsoleEntry::format()` shape):

```
[CONSOLE] 14:32:01.123 LOG: App initialized
[CONSOLE] 14:32:01.456 WARN: Deprecated API used
[CONSOLE] 14:32:02.789 ERROR: Failed to load resource: 404
```

The `console:` flag itself does not yet gate or surface these buffers (see
Known Gaps).

### Network Requests (network: true)

The CDP network listener (`start_network_listener()`) correlates
request/response pairs into `{method, url, status, duration_ms}` entries and
emits them at tracing `debug` level:

```
[NETWORK] GET https://api.example.com/user → 200 (45ms)
[NETWORK] POST https://api.example.com/login → 401 (120ms)
```

Request/response bodies are **not** captured, and the buffers are not
drained anywhere yet.

### Element Highlighting

When `highlight: true`, before interacting:
1. Element gets a red border (`outline: 3px solid red`, removed ~300 ms later)
2. Page scrolls the element into view
3. ~200 ms pause so the highlight is visible
4. Action executed

### Capture Naming

Screenshots are saved to `data/debug/` as:

```
data/debug/
└── <YYYYMMDD_HHMMSS>_<workflow>_<step>_<phase>.png   # phase: before | after | failure
```

### Cleanup

Debug files are pruned by `src/utils/debug.rs` (`cleanup_debug_dir`), not by
a `DebugCapture` helper. Default `CleanupPolicy`: **7 days** max age, **100
files** kept (newest first), no default size limit.

```bash
# Shell
debug clean

# HTTP
POST /api/debug/cleanup
→ { "success": true, "files_removed": 3, "bytes_freed": 104857,
    "files_remaining": 97 }
```

### Output Destination

| Output Type | Destination | Format |
|-------------|-------------|--------|
| Log messages | stdout (and daemon log when attached) | Human-readable (or JSON with `LOG_JSON=1`) |
| Screenshots | `data/debug/*.png` | PNG |
| Console/Network entries | Adapter buffers + tracing `debug` logs | Not included in API responses (see Known Gaps) |
| Trace data | `data/debug/trace.jsonl` | Not written yet — `TraceLogger` is not wired |

### Pause Interaction

When `pause: true`, behavior differs by interface:

**Shell / CLI (`ShellPauseHandler`):**
```
⏸  Paused at step 3 (click) selector=text:Submit in 'my_workflow'
   [c]ontinue  [s]kip  [a]bort
```
(Empty input continues; stdin errors continue.)

**API (via WebSocket):**
```json
{ "version": 1, "type": "execution.paused", "id": "exec-123", "step": 3 }
```

Client sends to resume:
```json
{ "type": "continue", "id": "exec-123" }
// or
{ "type": "skip", "id": "exec-123" }
// or
{ "type": "abort", "id": "exec-123" }
```

**API (without WebSocket):** the execution waits indefinitely for a WS
command — there is no timeout and no HTTP fallback. If the pause channel is
dropped (e.g. cancellation), the engine continues.

## Precedence

Merge order on the `automodus run` path (later wins; `merge()` gives
`Some` values precedence):

1. Environment variables (`AUTOMODUS_DEBUG*`) — lowest
2. Workflow-level `debug:` section
3. CLI flags (`--debug`, `--debug=<level>`, `--delay=`, `--capture=`,
   `--profile=`, `--highlight`, `--pause`, `--console`, `--network`) — highest

Step-level `debug:` then overrides the resolved workflow config per step
(`resolve_step_debug()`), with defaults applied last via `resolve()`.
Profile defaults are expanded where `with_profile()` is called: CLI
`--profile=` always, workflow `debug.profile:` only for `call:`
sub-workflows (see the caveat above).

## Implementation History

The original implementation plan (schema sketches, engine-integration notes,
and the phased checklist) was moved to
[archive/plan-debug-capture.md](archive/plan-debug-capture.md) during the
September 2026 docs-vs-code revision; it is kept for historical context only.

## Example Use Cases

Profiles are most reliable via the CLI on the `run` path (workflow-level
`profile:` currently only applies to `call:` sub-workflows):

### Debugging Selector Issues

```bash
automodus run workflow.yaml --profile=verbose
```

Output helps identify:
- What elements were found (via logs at `RUST_LOG=debug`)
- Why the wrong element was selected
- Current page state via screenshots (`capture: all`)

### Visual Debugging (Demo Mode)

```bash
automodus run workflow.yaml --profile=demo
```

Each action:
1. Highlights target element
2. Waits 1 second
3. Executes action

Great for demos and understanding workflow flow.

### CI/CD Failure Investigation

```bash
automodus run workflow.yaml --profile=ci
```

Automatically captures:
- Screenshot when any step fails (`capture: failure`)

Console/network listeners also run, but their buffers are not surfaced yet
(see Known Gaps in [SHELL.md](SHELL.md)).

### API Debugging

```yaml
debug:
  enabled: true
  network: true
  console: true

steps:
  - action: click
    selector: "text:Login"
```

`network:` / `console:` entries are buffered in the browser adapter and
emitted at tracing `debug` level, which helps inspect request URLs, status
codes, and page `console.*` output while iterating on a workflow.

## Related

- [SHELL.md](SHELL.md) - Shell & API reference
- [DESIGN.md](DESIGN.md) - Workflow design and syntax
- [ARCHITECTURE.md](ARCHITECTURE.md) - System internals
- [archive/plan-debug-capture.md](archive/plan-debug-capture.md) - Historical plan
- [CONTRIBUTING.md](../CONTRIBUTING.md) - Development guidelines
- [CONTRIBUTING.md](../CONTRIBUTING.md) - Development guidelines
