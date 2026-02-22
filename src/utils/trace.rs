//! Trace Logging
//!
//! JSONL trace output for debug mode.

use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;
use tracing::warn;

/// Trace logger for JSONL output
pub struct TraceLogger {
    writer: Option<Mutex<BufWriter<File>>>,
    enabled: bool,
}

impl TraceLogger {
    /// Create a new trace logger
    pub fn new(enabled: bool, output_dir: &Path) -> Self {
        if !enabled {
            return Self {
                writer: None,
                enabled: false,
            };
        }

        // Create trace file
        let trace_file = output_dir.join("trace.jsonl");
        
        if let Err(e) = std::fs::create_dir_all(output_dir) {
            warn!("Failed to create trace output directory: {}", e);
            return Self {
                writer: None,
                enabled: false,
            };
        }

        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&trace_file)
        {
            Ok(file) => {
                tracing::info!(path = %trace_file.display(), "Trace logging enabled");
                Self {
                    writer: Some(Mutex::new(BufWriter::new(file))),
                    enabled: true,
                }
            }
            Err(e) => {
                warn!("Failed to create trace file: {}", e);
                Self {
                    writer: None,
                    enabled: false,
                }
            }
        }
    }

    /// Create a disabled trace logger
    pub fn disabled() -> Self {
        Self {
            writer: None,
            enabled: false,
        }
    }

    /// Check if trace logging is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Log an event
    pub fn log(&self, event: TraceEvent) {
        if let Some(writer) = &self.writer {
            let entry = json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "event": event.event_type,
                "workflow": event.workflow,
                "step": event.step,
                "data": event.data,
            });

            if let Ok(mut w) = writer.lock() {
                if let Err(e) = writeln!(w, "{}", entry.to_string()) {
                    warn!("Failed to write trace log: {}", e);
                }
                let _ = w.flush();
            }
        }
    }

    /// Log selector resolution
    pub fn log_selector(&self, workflow: &str, step: usize, selector: &str, found: bool) {
        self.log(TraceEvent {
            event_type: "selector.resolved".to_string(),
            workflow: workflow.to_string(),
            step,
            data: json!({
                "selector": selector,
                "found": found,
            }),
        });
    }

    /// Log element information
    pub fn log_element(&self, workflow: &str, step: usize, selector: &str, info: &ElementInfo) {
        self.log(TraceEvent {
            event_type: "element.info".to_string(),
            workflow: workflow.to_string(),
            step,
            data: json!({
                "selector": selector,
                "tag": info.tag,
                "role": info.role,
                "text": info.text,
                "attributes": info.attributes,
            }),
        });
    }

    /// Log JavaScript injection
    pub fn log_js(&self, workflow: &str, step: usize, script: &str, result: Option<&serde_json::Value>) {
        self.log(TraceEvent {
            event_type: "js.executed".to_string(),
            workflow: workflow.to_string(),
            step,
            data: json!({
                "script": script,
                "result": result,
            }),
        });
    }

    /// Log action execution
    pub fn log_action(&self, workflow: &str, step: usize, action: &str, params: &serde_json::Value) {
        self.log(TraceEvent {
            event_type: "action.execute".to_string(),
            workflow: workflow.to_string(),
            step,
            data: json!({
                "action": action,
                "params": params,
            }),
        });
    }

    /// Log action result
    pub fn log_action_result(
        &self,
        workflow: &str,
        step: usize,
        action: &str,
        success: bool,
        error: Option<&str>,
    ) {
        self.log(TraceEvent {
            event_type: "action.result".to_string(),
            workflow: workflow.to_string(),
            step,
            data: json!({
                "action": action,
                "success": success,
                "error": error,
            }),
        });
    }

    /// Log workflow start
    pub fn log_workflow_start(&self, workflow: &str, params: &serde_json::Value) {
        self.log(TraceEvent {
            event_type: "workflow.start".to_string(),
            workflow: workflow.to_string(),
            step: 0,
            data: json!({
                "params": params,
            }),
        });
    }

    /// Log workflow end
    pub fn log_workflow_end(&self, workflow: &str, success: bool, duration_ms: i64) {
        self.log(TraceEvent {
            event_type: "workflow.end".to_string(),
            workflow: workflow.to_string(),
            step: 0,
            data: json!({
                "success": success,
                "duration_ms": duration_ms,
            }),
        });
    }
}

/// Trace event data
pub struct TraceEvent {
    /// Event type
    pub event_type: String,
    /// Workflow name
    pub workflow: String,
    /// Step index
    pub step: usize,
    /// Event data
    pub data: serde_json::Value,
}

/// Element information for trace logging
#[derive(Debug, Default)]
pub struct ElementInfo {
    /// HTML tag name
    pub tag: String,
    /// ARIA role
    pub role: Option<String>,
    /// Text content
    pub text: Option<String>,
    /// Element attributes
    pub attributes: std::collections::HashMap<String, String>,
}
