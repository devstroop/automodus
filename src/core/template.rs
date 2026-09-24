//! Template Engine
//!
//! Handles {{variable}} interpolation in flow definitions.

use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;
use tracing::warn;

use super::json_path::get_json_path;
use super::ExecutionContext;

static TEMPLATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([^}]+)\}\}").unwrap());

/// Strict single-template form: exactly one `{{ path | json }}` with no extra
/// `{{`/`}}` delimiters inside (rejects mixed strings like `{{a}} and {{b | json}}`).
/// Matched against the *untrimmed* string so padded values fall through.
static SINGLE_JSON_TEMPLATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\{\{\s*([^|{}]+?)\s*\|\s*json\s*\}\}$").unwrap());

/// Path roots accepted by `resolve`, with dotted segments allowing any
/// characters `resolve` can look up (hyphens, digits, etc.) except delimiters.
static VALID_PATH_ROOT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(params|vars|store|steps|env|instance|timestamp|workflow)(\.[^|{}\s]+)*$")
        .unwrap()
});

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
    ///
    /// A string that is exactly one unpadded `{{path | json}}` template is
    /// returned as a typed YAML scalar (number/bool/null/array/object, or the
    /// bare string value) so types survive into HTTP bodies and `call` params.
    /// Anything else (mixed templates, padding, missing paths, round-trip
    /// failure) falls through to [`Self::render`].
    pub fn render_yaml(value: &serde_yaml::Value, ctx: &ExecutionContext) -> serde_yaml::Value {
        match value {
            serde_yaml::Value::String(s) => {
                // Match untrimmed: padded `"  {{…}}  "` must keep surrounding spaces
                if let Some(caps) = SINGLE_JSON_TEMPLATE.captures(s) {
                    let key = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                    if VALID_PATH_ROOT.is_match(key) {
                        match Self::resolve(key, ctx) {
                            // Missing path: do NOT invent null — fall through so
                            // typos stay visible (render's | json → "null" string)
                            None => {}
                            // Strings: bare value (no JSON re-quote → no double-encoding)
                            Some(Value::String(st)) => return serde_yaml::Value::String(st),
                            // Non-string: JSON round-trip to keep number/bool/null type
                            Some(v) => match serde_json::to_string(&v)
                                .and_then(|json| serde_json::from_str::<serde_yaml::Value>(&json))
                            {
                                Ok(parsed) => return parsed,
                                Err(e) => {
                                    warn!(
                                        key,
                                        error = %e,
                                        "template: JSON→YAML round-trip failed; falling back to string render"
                                    );
                                }
                            },
                        }
                    }
                }
                serde_yaml::Value::String(Self::render(s, ctx))
            }
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
/// (delegates to shared [`get_json_path`] so semantics match execution context)
fn resolve_json_path(value: &Value, path: &str) -> Option<Value> {
    get_json_path(value, path)
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

    #[test]
    fn test_render_yaml_json_filter_preserves_number_type() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("user_id".into(), Value::Number(7.into()));

        let yaml = serde_yaml::Value::String("{{params.user_id | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_u64(), Some(7));
    }

    #[test]
    fn test_render_yaml_json_filter_preserves_bool_and_null() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("flag".into(), Value::Bool(true));

        let flag = TemplateEngine::render_yaml(
            &serde_yaml::Value::String("{{params.flag | json}}".into()),
            &ctx,
        );
        assert_eq!(flag.as_bool(), Some(true));

        // Missing path falls through to render's | json → string "null" (visible, not YAML null)
        let missing = TemplateEngine::render_yaml(
            &serde_yaml::Value::String("{{params.nope | json}}".into()),
            &ctx,
        );
        assert_eq!(missing.as_str(), Some("null"));
    }

    #[test]
    fn test_render_yaml_embedded_json_filter_stays_string() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("name".into(), Value::String("Ada".into()));

        let yaml = serde_yaml::Value::String("hello {{params.name | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_str(), Some(r#"hello "Ada""#));
    }

    #[test]
    fn test_render_yaml_exact_string_json_is_bare() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("name".into(), Value::String("Ada".into()));

        let yaml = serde_yaml::Value::String("{{params.name | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        // Bare string — no JSON re-quote (avoids double-encoding in JSON bodies)
        assert_eq!(rendered.as_str(), Some("Ada"));
    }

    #[test]
    fn test_render_yaml_padded_template_not_coerced() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("n".into(), Value::Number(7.into()));

        let yaml = serde_yaml::Value::String("  {{params.n | json}}  ".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        // Surrounding spaces preserved; not silently typed to 7
        assert_eq!(rendered.as_str(), Some("  7  "));
    }

    #[test]
    fn test_render_yaml_hyphenated_key_preserves_type() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("user-id".into(), Value::Number(42.into()));

        let yaml = serde_yaml::Value::String("{{params.user-id | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_u64(), Some(42));
    }

    #[test]
    fn test_render_yaml_mixed_templates_not_coerced_to_null() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.params.insert("a".into(), Value::String("X".into()));
        ctx.params.insert("b".into(), Value::Number(2.into()));

        let yaml =
            serde_yaml::Value::String("{{params.a}} and {{params.b | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        // Must interpolate both; must NOT become Null via bogus single-template match
        assert_eq!(rendered.as_str(), Some("X and 2"));
    }

    #[test]
    fn test_render_yaml_invalid_key_falls_back_not_null() {
        let ctx = ExecutionContext::new("test", "instance-1");

        // Junk key (contains `}}`) — not a valid path; should not become YAML null
        let yaml =
            serde_yaml::Value::String("{{params.a}} and {{params.b | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert!(!rendered.is_null());

        // Empty key after filter — also not typed-null
        let yaml = serde_yaml::Value::String("{{ | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert!(!rendered.is_null());
    }

    #[test]
    fn test_render_yaml_missing_path_is_string_null_not_yaml_null() {
        let ctx = ExecutionContext::new("test", "instance-1");

        let yaml = serde_yaml::Value::String("{{params.nope | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        // Falls through to render: JSON null encoded as string "null"
        assert_eq!(rendered.as_str(), Some("null"));
        assert!(!rendered.is_null());
    }

    #[test]
    fn test_render_array_length_path() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        let arr = serde_json::json!([{"id": 1}, {"id": 2}, {"id": 3}]);
        ctx.store.insert("items".into(), arr);

        let yaml = serde_yaml::Value::String("count={{store.items.length}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_str(), Some("count=3"));
    }

    #[test]
    fn test_render_string_length_path() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.store.insert("s".into(), serde_json::json!("hello"));

        let yaml = serde_yaml::Value::String("{{store.s.length}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_str(), Some("5"));
    }

    #[test]
    fn test_render_string_length_is_char_count_not_bytes() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        // "日本語😀" = 4 chars, 13 UTF-8 bytes — length must report chars
        ctx.store.insert("s".into(), serde_json::json!("日本語😀"));

        let yaml = serde_yaml::Value::String("{{store.s.length}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_str(), Some("4"));
    }

    #[test]
    fn test_length_with_trailing_segment_is_none() {
        let mut ctx = ExecutionContext::new("test", "instance-1");
        ctx.store.insert("items".into(), serde_json::json!([1, 2, 3]));
        ctx.store.insert("s".into(), serde_json::json!("hi"));

        // length only valid as final segment — trailing .foo must not yield a number.
        // Without | json, missing paths preserve the token (typo visibility).
        let yaml = serde_yaml::Value::String("{{store.items.length.foo}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_str(), Some("{{store.items.length.foo}}"));

        // With | json, missing → string "null" (never "3")
        let yaml = serde_yaml::Value::String("{{store.items.length.foo | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_str(), Some("null"));

        let yaml = serde_yaml::Value::String("{{store.s.length.foo | json}}".into());
        let rendered = TemplateEngine::render_yaml(&yaml, &ctx);
        assert_eq!(rendered.as_str(), Some("null"));
    }
}
