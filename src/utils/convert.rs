//! Type Conversion Utilities
//!
//! Helpers for converting between different data formats.

use serde_json::Value;

/// Convert YAML value to JSON value
///
/// This is commonly needed when workflow definitions use serde_yaml::Value
/// but runtime operations use serde_json::Value.
///
/// # Example
///
/// ```
/// use automodus::utils::convert::yaml_to_json;
///
/// let yaml = serde_yaml::Value::String("hello".to_string());
/// let json = yaml_to_json(&yaml);
/// assert_eq!(json, serde_json::Value::String("hello".to_string()));
/// ```
pub fn yaml_to_json(yaml: &serde_yaml::Value) -> Value {
    match yaml {
        serde_yaml::Value::Null => Value::Null,
        serde_yaml::Value::Bool(b) => Value::Bool(*b),
        serde_yaml::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(serde_json::Number::from_f64(f).unwrap_or(serde_json::Number::from(0)))
            } else {
                Value::Null
            }
        }
        serde_yaml::Value::String(s) => Value::String(s.clone()),
        serde_yaml::Value::Sequence(arr) => Value::Array(arr.iter().map(yaml_to_json).collect()),
        serde_yaml::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in map {
                if let serde_yaml::Value::String(key) = k {
                    obj.insert(key.clone(), yaml_to_json(v));
                }
            }
            Value::Object(obj)
        }
        serde_yaml::Value::Tagged(tagged) => yaml_to_json(&tagged.value),
    }
}

/// Convert JSON value to YAML value
pub fn json_to_yaml(json: &Value) -> serde_yaml::Value {
    match json {
        Value::Null => serde_yaml::Value::Null,
        Value::Bool(b) => serde_yaml::Value::Bool(*b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_yaml::Value::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                serde_yaml::Value::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_yaml::Value::Number(f.into())
            } else {
                serde_yaml::Value::Null
            }
        }
        Value::String(s) => serde_yaml::Value::String(s.clone()),
        Value::Array(arr) => {
            serde_yaml::Value::Sequence(arr.iter().map(json_to_yaml).collect())
        }
        Value::Object(obj) => {
            let mut map = serde_yaml::Mapping::new();
            for (k, v) in obj {
                map.insert(serde_yaml::Value::String(k.clone()), json_to_yaml(v));
            }
            serde_yaml::Value::Mapping(map)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_to_json_primitives() {
        assert_eq!(yaml_to_json(&serde_yaml::Value::Null), Value::Null);
        assert_eq!(
            yaml_to_json(&serde_yaml::Value::Bool(true)),
            Value::Bool(true)
        );
        assert_eq!(
            yaml_to_json(&serde_yaml::Value::String("test".into())),
            Value::String("test".into())
        );
    }

    #[test]
    fn test_yaml_to_json_number() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("42").unwrap();
        let json = yaml_to_json(&yaml);
        assert_eq!(json, Value::Number(42.into()));
    }

    #[test]
    fn test_yaml_to_json_array() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("[1, 2, 3]").unwrap();
        let json = yaml_to_json(&yaml);
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_yaml_to_json_object() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("name: test\nvalue: 42").unwrap();
        let json = yaml_to_json(&yaml);
        assert!(json.is_object());
        assert_eq!(json["name"], "test");
        assert_eq!(json["value"], 42);
    }

    #[test]
    fn test_roundtrip() {
        let original: serde_yaml::Value =
            serde_yaml::from_str("name: test\nitems: [1, 2, 3]").unwrap();
        let json = yaml_to_json(&original);
        let back = json_to_yaml(&json);
        assert_eq!(yaml_to_json(&back), json);
    }
}
