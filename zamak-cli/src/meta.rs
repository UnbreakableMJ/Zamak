// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Tool metadata constants and envelope-metadata builder.

use crate::json::{obj, Value};

pub const TOOL_NAME: &str = "zamak";
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const DOCS_BASE: &str = "https://steelbore.dev/zamak/docs";

/// Builds the `metadata` portion of the JSON envelope per SFRS §4.3.
pub fn envelope_metadata(command: &str) -> Value {
    obj([
        ("tool", Value::str(TOOL_NAME)),
        ("version", Value::str(TOOL_VERSION)),
        ("command", Value::str(command)),
        ("timestamp", Value::str(crate::time::iso8601_now())),
    ])
}

/// Reconstructs the invocation string for the error envelope
/// `command` field (SFRS §3.5). Joins argv with single spaces,
/// quoting any arg that contains whitespace.
pub fn command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.chars().any(|c| c.is_whitespace() || c == '"') {
                format!("{:?}", a)
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_contains_required_fields() {
        let m = envelope_metadata("zamak sbom");
        let s = m.to_compact();
        assert!(s.contains(r#""tool":"zamak""#));
        assert!(s.contains(r#""command":"zamak sbom""#));
        assert!(s.contains(r#""version""#));
        assert!(s.contains(r#""timestamp""#));
    }

    #[test]
    fn command_line_quotes_whitespace() {
        let argv = vec!["zamak".into(), "install".into(), "with space".into()];
        let s = command_line(&argv);
        assert!(s.contains('"'), "whitespace arg not quoted: {s}");
    }
}
