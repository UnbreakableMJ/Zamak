// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Minimal JSON encoder (no external deps).
//!
//! Produces SFRS-compliant output: a single, complete JSON document,
//! UTF-8 without BOM, snake_case property names, no trailing commas,
//! no ANSI codes, no JSON comments. Pretty-printing uses 2-space
//! indent for TTY consumption; compact form is used otherwise
//! (PowerShell `ConvertFrom-Json` and Nushell `from json` both
//! accept pretty JSON, so pretty is the safe default on TTY only).

use std::fmt::Write as _;

/// A JSON value in the Steelbore canonical form.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Float + future-expansion variants
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    /// Property order is preserved (stable, reproducible output).
    Object(Vec<(String, Value)>),
}

impl Value {
    /// Convenience: builds a `Value::Str` from anything `Display`able.
    pub fn str<S: Into<String>>(s: S) -> Self {
        Value::Str(s.into())
    }

    /// Appends a key/value pair to an object. Panics if `self` is not
    /// `Value::Object`.
    #[allow(dead_code)]
    pub fn insert<K: Into<String>>(&mut self, k: K, v: Value) {
        match self {
            Value::Object(o) => o.push((k.into(), v)),
            _ => panic!("Value::insert on non-object"),
        }
    }

    /// Emits compact JSON (no whitespace). Suitable for pipes, CI,
    /// and `--format jsonl` (one object per line).
    pub fn to_compact(&self) -> String {
        let mut s = String::new();
        write_compact(self, &mut s);
        s
    }

    /// Emits pretty JSON with 2-space indent. Suitable for TTY.
    pub fn to_pretty(&self) -> String {
        let mut s = String::new();
        write_pretty(self, &mut s, 0);
        s
    }

    /// Filters `Value::Object` down to the named fields, recursively.
    /// Unknown fields are silently dropped. Non-object values are
    /// returned as-is. Implements `--fields a,b,c` projection
    /// (SFRS §3.6).
    pub fn project(&self, fields: &[String]) -> Self {
        match self {
            Value::Object(props) => Value::Object(
                props
                    .iter()
                    .filter(|(k, _)| fields.iter().any(|f| f == k))
                    .cloned()
                    .collect(),
            ),
            Value::Array(items) => Value::Array(items.iter().map(|v| v.project(fields)).collect()),
            other => other.clone(),
        }
    }
}

fn write_compact(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::UInt(u) => {
            let _ = write!(out, "{u}");
        }
        Value::Float(f) => {
            if !f.is_finite() {
                out.push_str("null");
            } else {
                let _ = write!(out, "{f}");
            }
        }
        Value::Str(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(v, out);
            }
            out.push(']');
        }
        Value::Object(props) => {
            out.push('{');
            for (i, (k, v)) in props.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_compact(v, out);
            }
            out.push('}');
        }
    }
}

fn write_pretty(v: &Value, out: &mut String, depth: usize) {
    match v {
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Object(props) if props.is_empty() => out.push_str("{}"),
        Value::Array(items) => {
            out.push('[');
            out.push('\n');
            for (i, v) in items.iter().enumerate() {
                indent(out, depth + 1);
                write_pretty(v, out, depth + 1);
                if i + 1 < items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push(']');
        }
        Value::Object(props) => {
            out.push('{');
            out.push('\n');
            for (i, (k, v)) in props.iter().enumerate() {
                indent(out, depth + 1);
                write_string(out, k);
                out.push_str(": ");
                write_pretty(v, out, depth + 1);
                if i + 1 < props.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            indent(out, depth);
            out.push('}');
        }
        other => write_compact(other, out),
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Builds an object value from a slice of `(key, Value)` pairs.
/// Convenience for eliminating boilerplate at call sites.
pub fn obj<I: IntoIterator<Item = (&'static str, Value)>>(pairs: I) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_bool_numbers() {
        assert_eq!(Value::Null.to_compact(), "null");
        assert_eq!(Value::Bool(true).to_compact(), "true");
        assert_eq!(Value::Bool(false).to_compact(), "false");
        assert_eq!(Value::Int(-42).to_compact(), "-42");
        assert_eq!(Value::UInt(42).to_compact(), "42");
    }

    #[test]
    fn string_escapes_required_characters() {
        let v = Value::str("a\"b\\c\nd\te");
        assert_eq!(v.to_compact(), r#""a\"b\\c\nd\te""#);
    }

    #[test]
    fn string_escapes_control_bytes_as_unicode() {
        let v = Value::str("x\x01y");
        assert_eq!(v.to_compact(), "\"x\\u0001y\"");
    }

    #[test]
    fn compact_object_is_single_line() {
        let o = obj([("a", Value::Int(1)), ("b", Value::str("hi"))]);
        assert_eq!(o.to_compact(), r#"{"a":1,"b":"hi"}"#);
    }

    #[test]
    fn pretty_object_has_indent() {
        let o = obj([("a", Value::Int(1))]);
        let s = o.to_pretty();
        assert!(s.starts_with("{\n  \"a\": 1"));
        assert!(s.ends_with("}"));
    }

    #[test]
    fn empty_collections_collapse() {
        assert_eq!(Value::Object(vec![]).to_pretty(), "{}");
        assert_eq!(Value::Array(vec![]).to_pretty(), "[]");
    }

    #[test]
    fn field_projection_keeps_only_listed() {
        let o = obj([
            ("keep", Value::Int(1)),
            ("drop", Value::Int(2)),
            ("also_keep", Value::Int(3)),
        ]);
        let kept = o.project(&["keep".into(), "also_keep".into()]);
        assert_eq!(
            kept.to_compact(),
            r#"{"keep":1,"also_keep":3}"#
        );
    }

    #[test]
    fn projection_recurses_into_arrays() {
        let arr = Value::Array(vec![
            obj([("k", Value::Int(1)), ("v", Value::Int(2))]),
            obj([("k", Value::Int(3)), ("v", Value::Int(4))]),
        ]);
        let kept = arr.project(&["k".into()]);
        assert_eq!(kept.to_compact(), r#"[{"k":1},{"k":3}]"#);
    }

    #[test]
    fn non_finite_floats_serialize_as_null() {
        assert_eq!(Value::Float(f64::NAN).to_compact(), "null");
        assert_eq!(Value::Float(f64::INFINITY).to_compact(), "null");
    }
}
