# Debug Mode Design

This document outlines the debug mode implementation for automodus workflows.

## Current State

| Component | Status | Notes |
|-----------|--------|-------|
| `DebugConfig` struct | ✅ Implemented | `workflow/schema.rs` |
| `debug` field on Workflow | ✅ Implemented | `workflow/schema.rs` |
| `debug` field on Step | ✅ Implemented | `workflow/schema.rs` |
| CLI `--debug` flags | ✅ Implemented | `bin/automodus.rs` |
| Engine debug context | ✅ Implemented | `core/context.rs`, `core/engine.rs` |
| CDP console listener | ✅ Implemented | Native CDP `EventConsoleApiCalled` via `start_console_listener()` |
| CDP network listener | ✅ Implemented | Native CDP `RequestWillBeSent`/`ResponseReceived` via `start_network_listener()` |
| Element highlighting | ✅ Implemented | `core/engine.rs` |
| Screenshot capture | ✅ Implemented | Wired to debug modes |

## Overview

Debug mode provides visibility into workflow execution for troubleshooting selector issues, timing problems, and understanding automation behavior.

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
  console: true       # Capture browser console output
  network: true       # Capture XHR/fetch requests
  
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
      pause: true       # Wait for Enter key before continuing
```

### 3. CLI Override

```bash
# Enable debug for any workflow
automodus run workflow.yaml --debug

# With specific level (trace includes selector resolution logs)
automodus run workflow.yaml --debug=trace

# With delay between actions (great for demos)
automodus run workflow.yaml --debug --delay=1000

# Capture screenshots for all steps
automodus run workflow.yaml --debug --capture=all

# Use a profile
automodus run workflow.yaml --debug --profile=ci
```

### 4. Environment Variable

```bash
AUTOMODUS_DEBUG=true automodus shell
AUTOMODUS_DEBUG_LEVEL=trace automodus shell
AUTOMODUS_DEBUG_PROFILE=ci automodus shell
```

## Debug Options Reference

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enabled` | bool | false | Master switch for debug mode |
| `level` | string | info | Log verbosity: `info`, `debug`, `trace` |
| `capture` | string | failure | Screenshot mode: `none`, `failure`, `before`, `after`, `all` |
| `highlight` | bool | false | Flash element with red border before interaction |
| `delay` | int | 0 | Milliseconds to pause between actions |
| `pause` | bool | false | Wait for user input before continuing |
| `console` | bool | false | Capture browser console.log/warn/error |
| `network` | bool | false | Capture XHR/fetch requests and responses |
| `profile` | string | - | Preset configuration: `minimal`, `verbose`, `ci`, `demo` |

### Profiles

Profiles provide preset configurations for common scenarios:

| Profile | What it sets |
|---------|--------------|
| `minimal` | `capture: failure` |
| `verbose` | `level: trace`, `capture: all`, `console: true`, `network: true` |
| `ci` | `capture: failure`, `console: true`, `network: true` |
| `demo` | `highlight: true`, `delay: 1000` |

Profile settings can be overridden by explicit options:

```yaml
debug:
  profile: ci
  capture: all    # Override ci's capture: failure
```

### Capture Modes

| Mode | Description |
|------|-------------|
| `none` | No screenshots captured |
| `failure` | Capture only when a step fails (default) |
| `before` | Capture before each action |
| `after` | Capture after each action |
| `all` | Capture before and after each action |

### Log Levels

| Level | Includes |
|-------|----------|
| `info` | Step start/complete, errors |
| `debug` | + Action parameters, timing, variable state |
| `trace` | + Selector resolution, element details, injected JS |

## Debug Output

### Selector Resolution (level: trace)

When `level: trace`, selector details are logged automatically:

```
[TRACE] Step: click_login
  Selector: text:Log in with phone number
  Parsed: TextExact("Log in with phone number")
  Found: <div role="button" tabindex="0">Log in with phone number</div>
  Element: { tag: "DIV", role: "button", text: "Log in with phone number", clickable: true }
```

### Browser Console (console: true)

Captures browser console output with timestamps:

```
[CONSOLE] 14:32:01.123 LOG: App initialized
[CONSOLE] 14:32:01.456 WARN: Deprecated API used
[CONSOLE] 14:32:02.789 ERROR: Failed to load resource: 404
```

### Network Requests (network: true)

Captures XHR/fetch requests:

```
[NETWORK] GET https://api.example.com/user → 200 (45ms)
[NETWORK] POST https://api.example.com/login → 401 (120ms)
  Request: { "email": "***", "password": "***" }
  Response: { "error": "Invalid credentials" }
```

### Element Highlighting

When `highlight: true`, before clicking:
1. Element gets a red border (`outline: 3px solid red`)
2. Brief pause (200ms)
3. Border removed
4. Click executed

### Capture Naming

Screenshots saved to `data/debug/`:
```
data/debug/
├── workflow_step1_before.png
├── workflow_step1_after.png
├── workflow_step2_failure.png
└── ...
```

### Screenshot Management

```rust
/// Screenshot directory management
const DEBUG_DIR: &str = "data/debug";
const MAX_DEBUG_FILES: usize = 100;  // Auto-cleanup oldest
const MAX_DEBUG_SIZE_MB: u64 = 500;  // Total size limit

impl DebugCapture {
    /// Ensure debug directory exists
    pub fn init() -> Result<PathBuf> {
        let path = PathBuf::from(DEBUG_DIR);
        std::fs::create_dir_all(&path)?;
        Self::cleanup_if_needed(&path)?;
        Ok(path)
    }
    
    /// Remove oldest files if limits exceeded
    fn cleanup_if_needed(path: &Path) -> Result<()> {
        let mut files: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "png"))
            .collect();
        
        // Sort by modification time (oldest first)
        files.sort_by_key(|f| f.metadata().and_then(|m| m.modified()).ok());
        
        // Delete oldest files if count exceeds limit
        while files.len() > MAX_DEBUG_FILES {
            if let Some(oldest) = files.first() {
                std::fs::remove_file(oldest.path())?;
                files.remove(0);
            }
        }
        
        // Check total size and delete oldest if needed
        let total_mb: u64 = files.iter()
            .filter_map(|f| f.metadata().ok())
            .map(|m| m.len())
            .sum::<u64>() / (1024 * 1024);
        
        while total_mb > MAX_DEBUG_SIZE_MB && !files.is_empty() {
            if let Some(oldest) = files.first() {
                std::fs::remove_file(oldest.path())?;
                files.remove(0);
            }
        }
        
        Ok(())
    }
    
    /// Generate screenshot filename
    pub fn filename(workflow: &str, step: usize, phase: &str) -> String {
        format!("{}_{}_step{}_{}.png", 
            Utc::now().format("%Y%m%d_%H%M%S"),
            workflow,
            step,
            phase  // "before", "after", "failure"
        )
    }
}
```

### Output Destination

Debug output goes to multiple destinations based on type:

| Output Type | Destination | Format |
|-------------|-------------|--------|
| Log messages | stdout + log file | Human-readable |
| Screenshots | `data/debug/*.png` | PNG |
| Console/Network | stdout + execution result | JSON in API, human in shell |
| Trace data | `data/debug/trace.jsonl` | JSON Lines (when `level: trace`) |

```rust
/// Debug output configuration
pub struct DebugOutput {
    /// Write to stdout (default: true for shell, false for API)
    pub stdout: bool,
    /// Write to log file
    pub log_file: Option<PathBuf>,
    /// Include in execution result (for API)
    pub include_in_result: bool,
}
```

### Pause Interaction

When `pause: true`, behavior differs by interface:

**Shell:**
```
[PAUSED] Step 3: click "text:Submit"
Press Enter to continue, 's' to skip, 'q' to quit: _
```

**API (via WebSocket):**
```json
{ "type": "execution.paused", "id": "exec-123", "step": 3, "action": "click" }
```

Client sends to resume:
```json
{ "type": "execution.continue", "id": "exec-123" }
// or
{ "type": "execution.skip", "id": "exec-123" }
// or  
{ "type": "execution.abort", "id": "exec-123" }
```

**API (without WebSocket):**
If no WebSocket connected, `pause: true` is ignored with warning in response:
```json
{
  "success": true,
  "warnings": ["pause ignored: no WebSocket connection for step 3"]
}
```

## Precedence

From highest to lowest:
1. Step-level `debug:` options
2. CLI flags (`--debug`, `--delay`, `--capture`, `--profile`)
3. Workflow-level `debug:` section (explicit options override profile)
4. Profile defaults
5. Environment variables
6. Default values

## Implementation Notes

### Schema Changes (Priority: Do First)

Add to `workflow/schema.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Debug configuration for workflow execution
/// 
/// Uses Option<T> for fields to distinguish "not set" from "set to default".
/// This enables proper merge semantics where only explicitly-set fields override.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DebugConfig {
    /// Master switch for debug mode (None = inherit from parent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Log verbosity level (None = inherit from parent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<LogLevel>,
    /// Screenshot capture mode (None = inherit from parent)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture: Option<CaptureMode>,
    /// Flash element with red border before interaction (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<bool>,
    /// Milliseconds to pause between actions (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<u64>,
    /// Wait for user input before continuing (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause: Option<bool>,
    /// Capture browser console output (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub console: Option<bool>,
    /// Capture network requests (None = inherit)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<bool>,
    /// Preset configuration profile
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<DebugProfile>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    #[default]
    Info,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    None,
    #[default]
    Failure,
    Before,
    After,
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DebugProfile {
    Minimal,
    Verbose,
    Ci,
    Demo,
}

impl DebugConfig {
    /// Resolve final values (apply defaults for None fields)
    pub fn resolve(&self) -> ResolvedDebugConfig {
        ResolvedDebugConfig {
            enabled: self.enabled.unwrap_or(false),
            level: self.level.unwrap_or_default(),
            capture: self.capture.unwrap_or_default(),
            highlight: self.highlight.unwrap_or(false),
            delay: self.delay.unwrap_or(0),
            pause: self.pause.unwrap_or(false),
            console: self.console.unwrap_or(false),
            network: self.network.unwrap_or(false),
        }
    }
    
    /// Apply profile defaults, then merge explicit options
    pub fn with_profile(mut self) -> Self {
        if let Some(profile) = self.profile {
            let defaults = match profile {
                DebugProfile::Minimal => DebugConfig {
                    capture: Some(CaptureMode::Failure),
                    ..Default::default()
                },
                DebugProfile::Verbose => DebugConfig {
                    level: Some(LogLevel::Trace),
                    capture: Some(CaptureMode::All),
                    console: Some(true),
                    network: Some(true),
                    ..Default::default()
                },
                DebugProfile::Ci => DebugConfig {
                    capture: Some(CaptureMode::Failure),
                    console: Some(true),
                    network: Some(true),
                    ..Default::default()
                },
                DebugProfile::Demo => DebugConfig {
                    highlight: Some(true),
                    delay: Some(1000),
                    ..Default::default()
                },
            };
            // Profile sets defaults, explicit fields override
            self = defaults.merge(&self);
        }
        self
    }
    
    /// Merge with another config (other's Some values take precedence)
    pub fn merge(&self, other: &DebugConfig) -> Self {
        Self {
            enabled: other.enabled.or(self.enabled),
            level: other.level.or(self.level),
            capture: other.capture.or(self.capture),
            highlight: other.highlight.or(self.highlight),
            delay: other.delay.or(self.delay),
            pause: other.pause.or(self.pause),
            console: other.console.or(self.console),
            network: other.network.or(self.network),
            profile: other.profile.or(self.profile),
        }
    }
}

/// Resolved debug config with concrete values (no Options)
#[derive(Debug, Clone)]
pub struct ResolvedDebugConfig {
    pub enabled: bool,
    pub level: LogLevel,
    pub capture: CaptureMode,
    pub highlight: bool,
    pub delay: u64,
    pub pause: bool,
    pub console: bool,
    pub network: bool,
}

fn default_level() -> LogLevel { LogLevel::Info }
fn default_capture() -> CaptureMode { CaptureMode::Failure }
```

Add to `Workflow` struct:
```rust
pub struct Workflow {
    // ... existing fields ...
    
    /// Debug configuration for this workflow
    #[serde(default)]
    pub debug: DebugConfig,
}
```

Add to `Step` struct:
```rust
pub struct Step {
    // ... existing fields ...
    
    /// Step-level debug overrides
    #[serde(default)]
    pub debug: Option<DebugConfig>,
}
```

### Engine Integration

1. Parse `debug` section in workflow loader
2. Apply profile defaults, then merge explicit options
3. Merge CLI args with workflow config
4. Pass `DebugConfig` to action execution context
5. Actions check debug config before/after execution
6. Add highlight JS injection in browser adapter
7. Setup CDP listeners for console/network when enabled

### Console Capture Implementation

```rust
// Setup CDP console listener
page.event_listener::<ConsoleAPICalledEvent>()
    .for_each(|event| {
        let level = event.type_;
        let msg = event.args.iter()
            .map(|a| a.value.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        debug_log!("[CONSOLE] {} {}: {}", timestamp(), level, msg);
    });
```

### Network Capture Implementation

```rust
// Setup CDP network listeners  
page.event_listener::<ResponseReceivedEvent>()
    .for_each(|event| {
        let req = event.response;
        debug_log!("[NETWORK] {} {} → {} ({}ms)", 
            req.method, req.url, req.status, req.timing);
    });
```

### Highlight Implementation

```javascript
// Injected before click when highlight: true
(function highlightElement(el) {
  const original = el.style.outline;
  el.style.outline = '3px solid red';
  el.style.outlineOffset = '2px';
  setTimeout(() => {
    el.style.outline = original;
  }, 200);
})(targetElement);
```

## Example Use Cases

### Debugging Selector Issues

```yaml
debug:
  profile: verbose
  
steps:
  - action: click
    selector: "text:Submit"
```

Output helps identify:
- What elements were found
- Why the wrong element was selected
- Current page state via screenshot

### Visual Debugging (Demo Mode)

```yaml
debug:
  profile: demo
```

Each action:
1. Highlights target element
2. Waits 1 second
3. Executes action

Great for demos and understanding workflow flow.

### CI/CD Failure Investigation

```yaml
debug:
  profile: ci
```

Automatically captures:
- Screenshot when any step fails
- Browser console errors
- Failed network requests

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

Captures all API calls made during the workflow, helping debug:
- Authentication failures
- API errors
- Timing issues with async requests

## Implementation Phases

> **Recommended:** Start with Phase 1 (Schema) - it's low-risk and provides immediate value while the daemon architecture is planned.

### Phase 1: Schema (Priority - Do First, ~1-2 days)
1. Add `DebugConfig` struct to `workflow/schema.rs`
2. Add `debug` field to `Workflow` and `Step`
3. Add CLI argument parsing for `--debug`, `--delay`, `--capture`, `--profile`
4. Verify YAML parsing works (backwards compatible - field is optional)

### Phase 2: Engine Integration
1. Add `debug: DebugConfig` to `ExecutionContext`
2. Merge workflow + step + CLI debug configs with precedence
3. Add delay between steps when `delay > 0`
4. Add screenshot capture on step failure when `capture != none`
5. Wire `highlight` to browser adapter (JS injection)

### Phase 3: CDP Listeners
1. Add console listener to browser adapter
2. Add network request listener to browser adapter
3. Store captured data in `ExecutionContext`
4. Include in `WorkflowResult` when debug enabled

### Phase 4: Pause & Trace
1. Implement pause for shell (stdin prompt)
2. Implement pause for API (WebSocket command)
3. Add trace-level JSONL output
4. Add debug directory cleanup

## Related

- [SHELL.md](SHELL.md) - Shell & API design
- [DESIGN.md](../DESIGN.md) - Overall architecture
- [CONTRIBUTING.md](../CONTRIBUTING.md) - Development guidelines
