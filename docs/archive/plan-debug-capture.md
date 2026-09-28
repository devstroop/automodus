# Archive: Debug Capture Implementation Plan

> **Historical document.** This plan was part of `docs/DEBUG.md` and was
> extracted here during the September 2026 docs-vs-code revision. It describes
> work as *planned* at the time; several snippets were sketches rather than
> the final code. Kept for historical context only.
>
> For the current, verified description of debug mode, see
> [`DEBUG.md`](../DEBUG.md). The schema described below now lives in
> `src/workflow/schema.rs`; the capture/limit constants and `DebugCapture`
> helper from the old sketch were never implemented as written (cleanup is
> `src/utils/debug.rs` via `debug clean` / `POST /api/debug/cleanup`).

## Outcome

**Shipped**

- `DebugConfig` / `LogLevel` / `CaptureMode` / `DebugProfile` in
  `src/workflow/schema.rs`, with `resolve()` / `with_profile()` / `merge()`
- `debug:` fields on workflow and step, CLI flags, engine delay/pause/capture
  wiring, highlight JS injection, CDP console + network listeners,
  `TraceLogger` (`data/debug/trace.jsonl`), debug-dir cleanup

**Not shipped / diverged from this plan**

- `DebugCapture` helper struct with `MAX_DEBUG_FILES`/`MAX_DEBUG_SIZE_MB`
  limits (replaced by `CleanupPolicy`: 7 days / 100 files defaults)
- `DebugOutput` struct and `trace.jsonl` gated exactly as sketched
- `console:` / `network:` workflow flags gating anything: the listeners always
  start, and their buffers are never drained or surfaced
- Workflow-level `debug.profile` on the `automodus run` path (profiles are
  applied for CLI `--profile=` and for sub-workflows via `call:`, but the main
  run path skips `with_profile()`)

---

# Implementation Notes

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

# Implementation Phases

> **Recommended:** Start with Phase 1 (Schema) - it's low-risk and provides immediate value while the daemon architecture is planned.

### Phase 1: Schema ✅
1. ~~Add `DebugConfig` struct to `workflow/schema.rs`~~
2. ~~Add `debug` field to `Workflow` and `Step`~~
3. ~~Add CLI argument parsing for `--debug`, `--delay`, `--capture`, `--profile`~~
4. ~~Verify YAML parsing works (backwards compatible - field is optional)~~

### Phase 2: Engine Integration ✅
1. ~~Add `debug: DebugConfig` to `ExecutionContext`~~
2. ~~Merge workflow + step + CLI debug configs with precedence~~
3. ~~Add delay between steps when `delay > 0`~~
4. ~~Add screenshot capture on step failure when `capture != none`~~
5. ~~Wire `highlight` to browser adapter (JS injection)~~

### Phase 3: CDP Listeners ✅
1. ~~Add console listener to browser adapter~~ — `start_console_listener()` via `EventConsoleApiCalled`
2. ~~Add network request listener to browser adapter~~ — `start_network_listener()` via `EventRequestWillBeSent`/`EventResponseReceived`
3. ~~Store captured data in `ExecutionContext`~~
4. ~~Include in `WorkflowResult` when debug enabled~~

### Phase 4: Pause & Trace ✅
1. ~~Implement pause for shell (stdin prompt)~~ — `ShellPauseHandler` wired into CLI + standalone shell
2. ~~Implement pause for API (WebSocket command)~~ — `WebSocketPauseHandler` in `state.rs`, wired in `handlers.rs`
3. ~~Add trace-level JSONL output~~ — `TraceLogger` in `utils/trace.rs`
4. ~~Add debug directory cleanup~~
