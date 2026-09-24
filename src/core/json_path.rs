//! Shared JSON path traversal (dot notation with `.length` pseudo-property).
//!
//! Single implementation used by both the template engine and execution
//! context so `.length` semantics (final-segment only, Unicode char count
//! for strings) stay in lockstep.

use serde_json::Value;

/// Resolve a path like `foo.bar.0.baz` or `items.length` against `value`.
///
/// - Missing object keys / bad array indices → `None`
/// - `.length` on arrays → element count; on strings → Unicode scalar count
///   (not UTF-8 bytes); only valid as the **final** segment
pub fn get_json_path(value: &Value, path: &str) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = value;

    for (i, part) in parts.iter().enumerate() {
        let part = *part;
        let is_last = i + 1 == parts.len();
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            Value::Array(arr) => {
                if part == "length" {
                    return if is_last {
                        Some(Value::from(arr.len()))
                    } else {
                        None
                    };
                }
                let index: usize = part.parse().ok()?;
                current = arr.get(index)?;
            }
            Value::String(s) => {
                if part == "length" {
                    return if is_last {
                        Some(Value::from(s.chars().count()))
                    } else {
                        None
                    };
                }
                return None;
            }
            _ => return None,
        }
    }

    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn object_and_array_traversal() {
        let v = json!({"a": {"b": [{"c": 1}, {"c": 2}]}});
        assert_eq!(get_json_path(&v, "a.b.1.c"), Some(json!(2)));
        assert_eq!(get_json_path(&v, "a.b.9.c"), None);
        assert_eq!(get_json_path(&v, "a.missing"), None);
    }

    #[test]
    fn array_length_is_element_count() {
        let v = json!([1, 2, 3]);
        assert_eq!(get_json_path(&v, "length"), Some(json!(3)));
    }

    #[test]
    fn string_length_is_char_count_not_bytes() {
        // "日本語😀" = 4 chars, 13 UTF-8 bytes
        let v = json!("日本語😀");
        assert_eq!(get_json_path(&v, "length"), Some(json!(4)));
    }

    #[test]
    fn length_only_valid_as_final_segment() {
        let arr = json!([1, 2, 3]);
        assert_eq!(get_json_path(&arr, "length.foo"), None);
        let s = json!("hi");
        assert_eq!(get_json_path(&s, "length.foo"), None);
        // nested under object
        let obj = json!({"items": [1, 2]});
        assert_eq!(get_json_path(&obj, "items.length.foo"), None);
    }

    #[test]
    fn non_string_array_and_primitives() {
        let v = json!({"n": 42, "b": true, "z": null});
        assert_eq!(get_json_path(&v, "n"), Some(json!(42)));
        assert_eq!(get_json_path(&v, "b.length"), None);
        assert_eq!(get_json_path(&v, "z.foo"), None);
    }
}
