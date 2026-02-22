//! Execution Context
//!
//! Manages state during workflow execution.

use serde_json::Value;
use std::collections::HashMap;

/// Execution context for a workflow run
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Workflow name
    pub workflow_name: String,

    /// Workflow execution ID (for tracking)
    pub workflow_id: String,

    /// Browser instance ID
    pub instance_id: String,

    /// Current tab index
    pub tab_index: usize,

    /// Workflow variables (from workflow definition)
    pub vars: HashMap<String, Value>,

    /// Trigger parameters (from API call, event, etc.)
    pub params: HashMap<String, Value>,

    /// Stored values from step outputs
    pub store: HashMap<String, Value>,

    /// Step outputs (keyed by step id)
    pub step_outputs: HashMap<String, Value>,

    /// Events emitted during execution
    pub events: Vec<(String, Value)>,

    /// Current step index
    pub step_index: usize,

    /// Execution start time
    pub started_at: chrono::DateTime<chrono::Utc>,

    /// Environment variables (from system)
    pub env: HashMap<String, String>,
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(workflow_name: impl Into<String>, instance_id: impl Into<String>) -> Self {
        Self {
            workflow_name: workflow_name.into(),
            workflow_id: uuid::Uuid::new_v4().to_string(),
            instance_id: instance_id.into(),
            tab_index: 0,
            vars: HashMap::new(),
            params: HashMap::new(),
            store: HashMap::new(),
            step_outputs: HashMap::new(),
            events: Vec::new(),
            step_index: 0,
            started_at: chrono::Utc::now(),
            env: std::env::vars().collect(),
        }
    }

    /// Set workflow variables
    pub fn with_vars(mut self, vars: HashMap<String, Value>) -> Self {
        self.vars = vars;
        self
    }

    /// Set trigger parameters
    pub fn with_params(mut self, params: HashMap<String, Value>) -> Self {
        self.params = params;
        self
    }

    /// Store a value
    pub fn store_value(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        self.store.insert(key.clone(), value.clone());
        // Also add to step outputs if we have a current step id
        if self.step_index > 0 {
            self.step_outputs
                .insert(format!("step_{}", self.step_index), value);
        }
    }

    /// Store a step output by ID
    pub fn store_step_output(&mut self, step_id: impl Into<String>, value: Value) {
        self.step_outputs.insert(step_id.into(), value);
    }

    /// Get a variable value
    pub fn get_var(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    /// Get a parameter value
    pub fn get_param(&self, name: &str) -> Option<&Value> {
        self.params.get(name)
    }

    /// Get a stored value
    pub fn get_stored(&self, name: &str) -> Option<&Value> {
        self.store.get(name)
    }

    /// Get a step output by ID
    pub fn get_step_output(&self, step_id: &str) -> Option<&Value> {
        self.step_outputs.get(step_id)
    }

    /// Get an environment variable
    pub fn get_env(&self, name: &str) -> Option<&String> {
        self.env.get(name)
    }

    /// Emit an event
    pub fn emit_event(&mut self, name: impl Into<String>, data: Value) {
        self.events.push((name.into(), data));
    }

    /// Advance to next step
    pub fn next_step(&mut self) {
        self.step_index += 1;
    }

    /// Jump to a specific step index
    pub fn goto_step(&mut self, index: usize) {
        self.step_index = index;
    }

    /// Convert to ActionContext for action execution
    pub fn to_action_context(&self, step_id: Option<&str>) -> crate::actions::ActionContext {
        crate::actions::ActionContext {
            workflow_name: self.workflow_name.clone(),
            step_index: self.step_index,
            step_id: step_id.map(String::from),
            vars: self.vars.clone(),
            params: self.params.clone(),
            store: self.store.clone(),
            instance_id: self.instance_id.clone(),
            tab_index: self.tab_index,
        }
    }

    /// Get execution duration
    pub fn duration(&self) -> chrono::Duration {
        chrono::Utc::now() - self.started_at
    }

    /// Build final output from output definitions
    pub fn build_output(&self, output_defs: &HashMap<String, serde_yaml::Value>) -> Value {
        let mut result = serde_json::Map::new();

        for (key, value) in output_defs {
            let resolved = self.resolve_yaml_value(value);
            result.insert(key.clone(), resolved);
        }

        Value::Object(result)
    }

    /// Resolve a YAML value, replacing template references
    fn resolve_yaml_value(&self, value: &serde_yaml::Value) -> Value {
        match value {
            serde_yaml::Value::String(s) => {
                // Check for template syntax
                if s.starts_with("{{") && s.ends_with("}}") {
                    let key = s.trim_start_matches("{{").trim_end_matches("}}").trim();
                    self.resolve_reference(key).unwrap_or(Value::Null)
                } else {
                    Value::String(s.clone())
                }
            }
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Number(i.into())
                } else if let Some(f) = n.as_f64() {
                    Value::Number(
                        serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)),
                    )
                } else {
                    Value::Null
                }
            }
            serde_yaml::Value::Bool(b) => Value::Bool(*b),
            serde_yaml::Value::Null => Value::Null,
            serde_yaml::Value::Sequence(arr) => {
                Value::Array(arr.iter().map(|v| self.resolve_yaml_value(v)).collect())
            }
            serde_yaml::Value::Mapping(map) => {
                let mut obj = serde_json::Map::new();
                for (k, v) in map {
                    if let serde_yaml::Value::String(key) = k {
                        obj.insert(key.clone(), self.resolve_yaml_value(v));
                    }
                }
                Value::Object(obj)
            }
            _ => Value::Null,
        }
    }

    /// Resolve a reference like "params.x", "vars.y", "store.z", "steps.id.output"
    fn resolve_reference(&self, key: &str) -> Option<Value> {
        if let Some(name) = key.strip_prefix("params.") {
            // Handle nested access: params.foo.bar
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            let value = self.get_param(parts[0])?;
            if parts.len() > 1 {
                get_nested_value(value, parts[1])
            } else {
                Some(value.clone())
            }
        } else if let Some(name) = key.strip_prefix("vars.") {
            // Handle nested access: vars.foo.bar
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            let value = self.get_var(parts[0])?;
            if parts.len() > 1 {
                get_nested_value(value, parts[1])
            } else {
                Some(value.clone())
            }
        } else if let Some(name) = key.strip_prefix("store.") {
            // Handle nested access: store.result.status
            let parts: Vec<&str> = name.splitn(2, '.').collect();
            let value = self.get_stored(parts[0])?;
            if parts.len() > 1 {
                get_nested_value(value, parts[1])
            } else {
                Some(value.clone())
            }
        } else if let Some(rest) = key.strip_prefix("steps.") {
            // Format: steps.step_id.output or steps.step_id.output.field
            let parts: Vec<&str> = rest.splitn(2, '.').collect();
            if let Some(step_output) = self.get_step_output(parts[0]) {
                if parts.len() > 1 {
                    // Get nested field
                    get_nested_value(step_output, parts[1])
                } else {
                    Some(step_output.clone())
                }
            } else {
                None
            }
        } else if let Some(name) = key.strip_prefix("env.") {
            self.get_env(name).map(|s| Value::String(s.clone()))
        } else if key == "instance.id" {
            Some(Value::String(self.instance_id.clone()))
        } else if key == "timestamp" {
            Some(Value::String(chrono::Utc::now().to_rfc3339()))
        } else {
            // Try store as default
            self.get_stored(key).cloned()
        }
    }
}

/// Get a nested value from a JSON value using dot notation
fn get_nested_value(value: &Value, path: &str) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for part in parts {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            Value::Array(arr) => {
                let index: usize = part.parse().ok()?;
                current = arr.get(index)?;
            }
            _ => return None,
        }
    }

    Some(current.clone())
}
