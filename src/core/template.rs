//! Template Engine
//!
//! Handles {{variable}} interpolation in flow definitions.

use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

use super::ExecutionContext;

static TEMPLATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([^}]+)\}\}").unwrap());

/// Template engine for variable interpolation
pub struct TemplateEngine;

impl TemplateEngine {
    /// Render a template string with context values
    ///
    /// Supports an optional filter after `|`:
    ///   `{{params.message | json}}` → JSON-encoded literal (safe inside JS)
    pub fn render(template: &str, ctx: &ExecutionContext) -> String {
        TEMPLATE_RE
            .replace_all(template, |caps: &regex::Captures| {
                let raw = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                let (key, filter) = match raw.split_once('|') {
                    Some((k, f)) => (k.trim(), f.trim()),
                    None => (raw, ""),
                };

                if filter == "json" {
                    // Missing paths become null so optional params are valid JS/JSON
                    let value = Self::resolve(key, ctx).unwrap_or(Value::Null);
                    return serde_json::to_string(&value).unwrap_or_else(|_| "null".into());
                }

                if !filter.is_empty() {
                    // Unknown filter: leave the original token untouched
                    return format!("{{{{{}}}}}", raw);
                }

                Self::resolve(key, ctx)
                    .map(|v| value_to_string(&v))
                    .unwrap_or_else(|| format!("{{{{{}}}}}", key))
            })
            .to_string()
    }

    /// Render a YAML value, interpolating any template strings
    pub fn render_yaml(value: &serde_yaml::Value, ctx: &ExecutionContext) -> serde_yaml::Value {
        match value {
            serde_yaml::Value::String(s) => serde_yaml::Value::String(Self::render(s, ctx)),
            serde_yaml::Value::Sequence(arr) => {
                serde_yaml::Value::Sequence(arr.iter().map(|v| Self::render_yaml(v, ctx)).collect())
            }
            serde_yaml::Value::Mapping(map) => {
                let mut new_map = serde_yaml::Mapping::new();
                for (k, v) in map {
                    let new_key = if let serde_yaml::Value::String(s) = k {
                        serde_yaml::Value::String(Self::render(s, ctx))
                    } else {
                        k.clone()
                    };
                    new_map.insert(new_key, Self::render_yaml(v, ctx));
                }
                serde_yaml::Value::Mapping(new_map)
            }
            _ => value.clone(),
        }
    }

    /// Render a HashMap of YAML values
    pub fn render_params(
        params: &HashMap<String, serde_yaml::Value>,
        ctx: &ExecutionContext,
    ) -> HashMap<String, serde_yaml::Value> {
        params
            .iter()
            .map(|(k, v)| (k.clone(), Self::render_yaml(v, ctx)))
            .collect()
    }

    /// Resolve a reference key to a value
    fn resolve(key: &str, ctx: &ExecutionContext) -> Option<Value> {
        // Handle nested references: params.foo.bar
        let parts: Vec<&str> = key.splitn(2, '.').collect();

        match parts.first()? {
            &"params" => {
                if parts.len() > 1 {
                    resolve_path(&ctx.params, parts[1])
                } else {
                    Some(Value::Object(
                        ctx.params
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    ))
                }
            }
            &"vars" => {
                if parts.len() > 1 {
                    resolve_path(&ctx.vars, parts[1])
                } else {
                    Some(Value::Object(
                        ctx.vars
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    ))
                }
            }
            &"store" => {
                if parts.len() > 1 {
                    resolve_path(&ctx.store, parts[1])
                } else {
                    Some(Value::Object(
                        ctx.store
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    ))
                }
            }
            &"steps" => {
                if parts.len() > 1 {
                    let step_parts: Vec<&str> = parts[1].splitn(2, '.').collect();
                    let step_id = step_parts[0];
                    if let Some(output) = ctx.step_outputs.get(step_id) {
                        if step_parts.len() > 1 {
                            resolve_json_path(output, step_parts[1])
                        } else {
                            Some(output.clone())
                        }
                    } else {
                        None
                    }
                } else {
                    Some(Value::Object(
                        ctx.step_outputs
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect(),
                    ))
                }
            }
            &"env" => {
                if parts.len() > 1 {
                    ctx.env.get(parts[1]).map(|s| Value::String(s.clone()))
                } else {
                    Some(Value::Object(
                        ctx.env
                            .iter()
                            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                            .collect(),
                    ))
                }
            }
            &"instance" => match parts.get(1) {
                Some(&"id") => Some(Value::String(ctx.instance_id.clone())),
                _ => Some(Value::Object(
                    [("id".to_string(), Value::String(ctx.instance_id.clone()))]
                        .into_iter()
                        .collect(),
                )),
            },
            &"timestamp" => Some(Value::String(chrono::Utc::now().to_rfc3339())),
            &"workflow" => match parts.get(1) {
                Some(&"name") => Some(Value::String(ctx.workflow_name.clone())),
                Some(&"id") => Some(Value::String(ctx.workflow_id.clone())),
                _ => Some(Value::Object(
                    [
                        ("name".to_string(), Value::String(ctx.workflow_name.clone())),
                        ("id".to_string(), Value::String(ctx.workflow_id.clone())),
                    ]
                    .into_iter()
                    .collect(),
                )),
            },
            // Direct lookup in store as fallback
            _ => ctx.store.get(key).cloned(),
        }
    }
}

/// Resolve a path like "foo.bar.baz" in a HashMap
fn resolve_path(map: &HashMap<String, Value>, path: &str) -> Option<Value> {
    let parts: Vec<&str> = path.splitn(2, '.').collect();
    let value = map.get(parts[0])?;

    if parts.len() > 1 {
        resolve_json_path(value, parts[1])
    } else {
        Some(value.clone())
    }
}

/// Resolve a path like "foo.bar.0.baz" in a JSON value
fn resolve_json_path(value: &Value, path: &str) -> Option<Value> {
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

/// Convert a JSON value to a string for interpolation
fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        Value::Array(_) | Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_interpolation() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params
            .insert("name".into(), Value::String("World".into()));

        let result = TemplateEngine::render("Hello {{params.name}}!", &ctx);
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_multiple_interpolations() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params
            .insert("first".into(), Value::String("John".into()));
        ctx.params
            .insert("last".into(), Value::String("Doe".into()));

        let result = TemplateEngine::render("{{params.first}} {{params.last}}", &ctx);
        assert_eq!(result, "John Doe");
    }

    #[test]
    fn test_missing_value_preserved() {
        let ctx = ExecutionContext::new("test", "instance-1");

        let result = TemplateEngine::render("Hello {{params.missing}}!", &ctx);
        assert_eq!(result, "Hello {{params.missing}}!");
    }

    #[test]
    fn test_store_values() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.store
            .insert("result".into(), Value::String("success".into()));

        let result = TemplateEngine::render("Status: {{store.result}}", &ctx);
        assert_eq!(result, "Status: success");
    }

    #[test]
    fn test_env_values() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.env.insert("MY_VAR".into(), "test_value".into());

        let result = TemplateEngine::render("Env: {{env.MY_VAR}}", &ctx);
        assert_eq!(result, "Env: test_value");
    }

    #[test]
    fn test_instance_id() {
        let ctx = ExecutionContext::new("test", "instance-1");

        let result = TemplateEngine::render("Instance: {{instance.id}}", &ctx);
        assert_eq!(result, "Instance: instance-1");
    }

    #[test]
    fn test_json_filter_escapes_quotes() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params
            .insert("message".into(), Value::String("It's a \"test\"\nline".into()));

        let result = TemplateEngine::render("var m = {{params.message | json}};", &ctx);
        assert_eq!(result, r#"var m = "It's a \"test\"\nline";"#);
    }

    #[test]
    fn test_json_filter_missing_is_null() {
        let ctx = ExecutionContext::new("test", "instance-1");

        let result = TemplateEngine::render("var m = {{params.missing | json}};", &ctx);
        assert_eq!(result, "var m = null;");
    }

    #[test]
    fn test_json_filter_bool_number() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("n".into(), Value::Number(42.into()));
        ctx.params.insert("b".into(), Value::Bool(true));

        let result = TemplateEngine::render("{{params.n | json}} {{params.b | json}}", &ctx);
        assert_eq!(result, "42 true");
    }

    #[test]
    fn test_unknown_filter_preserved() {
        let ctx = ExecutionContext::new("test", "instance-1");

        let result = TemplateEngine::render("{{params.x | upper}}", &ctx);
        assert_eq!(result, "{{params.x | upper}}");
    }
}
