// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Output-mode selection, JSON envelope, and color policy.
//!
//! Implements SFRS §4 (mode architecture) and §4.4 (color rules):
//!
//! - Human mode → pretty text + optional ANSI color
//! - JSON / JSONL / YAML / CSV / Explore → machine-readable
//! - Precedence: explicit flag > env > TTY > fallback
//! - `NO_COLOR` / `FORCE_COLOR` / `CLICOLOR` / `--color` chain
//! - JSON envelope: `{metadata: {...}, data: ...}` (SFRS §4.3)

use std::io::Write;

use crate::env::EnvSnapshot;
use crate::json::{obj, Value};

/// Machine- or human-readable output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// TTY-friendly human text (default when stdout is a TTY).
    Human,
    /// Single well-formed JSON document (default when piped).
    Json,
    /// Newline-delimited JSON (one object per line).
    JsonL,
    /// YAML 1.2.
    Yaml,
    /// CSV (RFC 4180).
    Csv,
    /// Interactive TUI browser (requires TTY + `tui` feature).
    Explore,
}

impl Format {
    /// Canonical short name as it appears in `--format <...>`.
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Human => "human",
            Format::Json => "json",
            Format::JsonL => "jsonl",
            Format::Yaml => "yaml",
            Format::Csv => "csv",
            Format::Explore => "explore",
        }
    }

    /// Parses the `<fmt>` token from `--format <fmt>`. `-E` is an
    /// accepted alias for `explore` (see SFRS §5.1).
    pub fn parse(s: &str) -> Option<Format> {
        match s {
            "human" => Some(Format::Human),
            "json" => Some(Format::Json),
            "jsonl" | "ndjson" => Some(Format::JsonL),
            "yaml" | "yml" => Some(Format::Yaml),
            "csv" => Some(Format::Csv),
            "explore" | "E" => Some(Format::Explore),
            _ => None,
        }
    }

    /// True for formats that carry structured machine data.
    pub fn is_machine(self) -> bool {
        !matches!(self, Format::Human | Format::Explore)
    }
}

/// How color policy was resolved. Follows SFRS §4.4 precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn parse(s: &str) -> Option<ColorMode> {
        match s {
            "auto" => Some(ColorMode::Auto),
            "always" => Some(ColorMode::Always),
            "never" => Some(ColorMode::Never),
            _ => None,
        }
    }
}

/// A resolved decision about how this invocation should emit output.
/// Computed once from flags + env snapshot; the rest of the program
/// never re-reads environment variables.
#[derive(Debug, Clone)]
#[allow(dead_code)] // verbose + print0: surface reserved for future per-command logic
pub struct OutputPolicy {
    pub format: Format,
    pub color: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub fields: Option<Vec<String>>,
    pub print0: bool,
}

impl OutputPolicy {
    /// Resolves `--format`, `--json`, `--color`, `NO_COLOR`,
    /// `FORCE_COLOR`, `CLICOLOR`, `AI_AGENT`, `CI`, `TERM=dumb`, and
    /// TTY state into a single policy.
    pub fn resolve(
        explicit_format: Option<Format>,
        explicit_json: bool,
        color: ColorMode,
        quiet: bool,
        verbose: bool,
        fields: Option<Vec<String>>,
        print0: bool,
        env: &EnvSnapshot,
    ) -> OutputPolicy {
        let format = resolve_format(explicit_format, explicit_json, env);
        let color = resolve_color(color, format, env);
        OutputPolicy {
            format,
            color,
            quiet,
            verbose,
            fields,
            print0,
        }
    }

    /// Applies `--fields a,b,c` projection to a `data` value if the
    /// user passed `--fields`. Otherwise returns the value unchanged.
    pub fn project(&self, data: Value) -> Value {
        match &self.fields {
            Some(fs) => data.project(fs),
            None => data,
        }
    }

    /// Emits a `data` payload in the negotiated format to stdout.
    /// Progress indicators and diagnostics must never pass through
    /// this method (SFRS §3.6).
    pub fn emit(&self, metadata: Value, data: Value) -> std::io::Result<()> {
        let data = self.project(data);
        let envelope = obj([("metadata", metadata), ("data", data)]);
        let stdout = std::io::stdout();
        let mut out = stdout.lock();
        match self.format {
            Format::Json => {
                if self.color {
                    // Per SFRS §4.4: ANSI codes MUST be suppressed in
                    // all machine modes. Pretty-print for TTY but
                    // without color sequences.
                    writeln!(out, "{}", envelope.to_pretty())
                } else {
                    writeln!(out, "{}", envelope.to_compact())
                }
            }
            Format::JsonL => {
                // Each top-level record of the data array becomes one
                // line. Metadata is dropped in JSONL per streaming
                // convention — consumers can run `schema` to learn
                // the shape. Non-array `data` is emitted as a single
                // line.
                match envelope {
                    Value::Object(mut props) => {
                        // Find the "data" field
                        let data = props
                            .iter()
                            .position(|(k, _)| k == "data")
                            .map(|i| props.swap_remove(i).1);
                        match data {
                            Some(Value::Array(items)) => {
                                for item in items {
                                    writeln!(out, "{}", item.to_compact())?;
                                }
                                Ok(())
                            }
                            Some(v) => writeln!(out, "{}", v.to_compact()),
                            None => Ok(()),
                        }
                    }
                    v => writeln!(out, "{}", v.to_compact()),
                }
            }
            Format::Yaml => {
                // Minimal YAML 1.2: Nushell / PowerShell / jq already
                // consume JSON; YAML output is best-effort and
                // produced by direct serialization of the envelope.
                let s = to_yaml(&envelope, 0);
                write!(out, "{s}")
            }
            Format::Csv => {
                // Produces RFC 4180 CSV from the `data` array if it
                // is homogeneous; otherwise falls back to a single
                // "data" column containing compact JSON.
                to_csv(&envelope, &mut out)
            }
            Format::Human => {
                // Human mode: the command is responsible for its own
                // rendering. This path is a safety fallback —
                // dump the envelope as pretty JSON.
                if self.color {
                    writeln!(out, "{}", envelope.to_pretty())
                } else {
                    writeln!(out, "{}", envelope.to_pretty())
                }
            }
            Format::Explore => {
                // Explore is handled by the TUI module before reaching
                // here; if we get here, the TUI feature is off and we
                // fall back to JSON with a warning (already emitted).
                writeln!(out, "{}", envelope.to_pretty())
            }
        }
    }
}

/// Resolves the effective format from flags + env (SFRS §4.1).
fn resolve_format(explicit: Option<Format>, explicit_json: bool, env: &EnvSnapshot) -> Format {
    // 1. Explicit flag wins.
    if let Some(f) = explicit {
        return f;
    }
    if explicit_json {
        return Format::Json;
    }
    // 2. Agent / CI env forces JSON.
    if env.agent_mode || env.ci_mode {
        return Format::Json;
    }
    // 3. TTY detection.
    if env.stdout_is_tty {
        Format::Human
    } else {
        Format::Json
    }
}

/// Resolves the effective color decision (SFRS §4.4).
fn resolve_color(mode: ColorMode, format: Format, env: &EnvSnapshot) -> bool {
    // Machine modes suppress ANSI unconditionally (§4.4).
    if format.is_machine() {
        return false;
    }
    // Explicit --color flag (highest precedence after machine-mode lock).
    match mode {
        ColorMode::Never => return false,
        ColorMode::Always => return true,
        ColorMode::Auto => {}
    }
    // Env chain — FORCE_COLOR outranks NO_COLOR per SFRS §4.4.
    if env.force_color {
        return true;
    }
    if env.no_color {
        return false;
    }
    if env.cli_color_off {
        return false;
    }
    if env.term_is_dumb {
        return false;
    }
    env.stdout_is_tty
}

/// Steelbore six-token palette as ANSI 24-bit escapes (§3.1 brand).
/// No-op when `color` is false.
pub struct Palette {
    pub color: bool,
}

#[allow(dead_code)] // palette surface kept complete for future use
impl Palette {
    pub const VOID_NAVY: (u8, u8, u8) = (0x0B, 0x12, 0x22);
    pub const STEEL_BLUE: (u8, u8, u8) = (0x4B, 0x7E, 0xB0);
    pub const MOLTEN_AMBER: (u8, u8, u8) = (0xD9, 0x8E, 0x32);
    pub const RADIUM_GREEN: (u8, u8, u8) = (0x50, 0xFA, 0x7B);
    pub const RED_OXIDE: (u8, u8, u8) = (0xFF, 0x5C, 0x5C);
    pub const LIQUID_COOLANT: (u8, u8, u8) = (0x8B, 0xE9, 0xFD);

    pub fn paint(&self, (r, g, b): (u8, u8, u8), s: &str) -> String {
        if !self.color {
            return s.to_string();
        }
        format!("\x1b[38;2;{r};{g};{b}m{s}\x1b[0m")
    }
}

/// Writes a structured info-level diagnostic to stderr.
/// Never touches stdout (SFRS §3.6, §4.2).
pub fn emit_info(policy: &OutputPolicy, msg: &str) {
    if policy.quiet {
        return;
    }
    match policy.format {
        Format::Json | Format::JsonL | Format::Yaml | Format::Csv => {
            let v = obj([
                ("level", Value::str("info")),
                ("timestamp", Value::str(crate::time::iso8601_now())),
                ("message", Value::str(msg)),
            ]);
            eprintln!("{}", v.to_compact());
        }
        _ => {
            let p = Palette {
                color: policy.color,
            };
            let ts = crate::time::iso8601_now();
            let tag = p.paint(Palette::STEEL_BLUE, "[INFO]");
            eprintln!("{ts} {tag} {msg}");
        }
    }
}

/// Writes a structured warning to stderr.
pub fn emit_warn(policy: &OutputPolicy, msg: &str) {
    match policy.format {
        Format::Json | Format::JsonL | Format::Yaml | Format::Csv => {
            let v = obj([
                ("level", Value::str("warn")),
                ("timestamp", Value::str(crate::time::iso8601_now())),
                ("message", Value::str(msg)),
            ]);
            eprintln!("{}", v.to_compact());
        }
        _ => {
            let p = Palette {
                color: policy.color,
            };
            let ts = crate::time::iso8601_now();
            let tag = p.paint(Palette::MOLTEN_AMBER, "[WARN]");
            eprintln!("{ts} {tag} {msg}");
        }
    }
}

/// Minimal YAML 1.2 emitter. Only handles the Steelbore envelope
/// shape (objects + arrays of primitives / nested objects). Not a
/// general-purpose YAML encoder.
fn to_yaml(v: &Value, depth: usize) -> String {
    let mut out = String::new();
    match v {
        Value::Object(props) => {
            for (k, val) in props {
                for _ in 0..depth {
                    out.push(' ');
                    out.push(' ');
                }
                out.push_str(k);
                out.push(':');
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        out.push('\n');
                        out.push_str(&to_yaml(val, depth + 1));
                    }
                    _ => {
                        out.push(' ');
                        out.push_str(&scalar_yaml(val));
                        out.push('\n');
                    }
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                for _ in 0..depth {
                    out.push(' ');
                    out.push(' ');
                }
                out.push_str("- ");
                match item {
                    Value::Object(_) | Value::Array(_) => {
                        out.push('\n');
                        out.push_str(&to_yaml(item, depth + 1));
                    }
                    _ => {
                        out.push_str(&scalar_yaml(item));
                        out.push('\n');
                    }
                }
            }
        }
        other => {
            out.push_str(&scalar_yaml(other));
            out.push('\n');
        }
    }
    out
}

fn scalar_yaml(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => {
            if f.is_finite() {
                f.to_string()
            } else {
                "null".to_string()
            }
        }
        Value::Str(s) => {
            // Always quote strings for safety; YAML has too many
            // unquoted-scalar edge cases.
            format!("\"{}\"", yaml_escape(s))
        }
        // Should not be reached — handled inline above.
        Value::Array(_) | Value::Object(_) => "[]".to_string(),
    }
}

fn yaml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// RFC 4180 CSV emitter. Works when `data` is an array of homogeneous
/// objects (the common `list` case). Falls back to a single-column
/// form for any other shape.
fn to_csv<W: Write>(envelope: &Value, out: &mut W) -> std::io::Result<()> {
    let data = match envelope {
        Value::Object(props) => props
            .iter()
            .find(|(k, _)| k == "data")
            .map(|(_, v)| v.clone())
            .unwrap_or(Value::Null),
        _ => return writeln!(out, "{}", envelope.to_compact()),
    };
    match data {
        Value::Array(ref rows) if !rows.is_empty() => {
            // Gather header from the first object row.
            let headers: Vec<String> = match &rows[0] {
                Value::Object(props) => props.iter().map(|(k, _)| k.clone()).collect(),
                _ => return writeln!(out, "data\n{}", data.to_compact()),
            };
            // Header line.
            writeln!(out, "{}", headers.join(","))?;
            for row in rows {
                let cells: Vec<String> = match row {
                    Value::Object(props) => headers
                        .iter()
                        .map(|h| {
                            props
                                .iter()
                                .find(|(k, _)| k == h)
                                .map(|(_, v)| csv_cell(v))
                                .unwrap_or_default()
                        })
                        .collect(),
                    other => vec![csv_cell(other)],
                };
                writeln!(out, "{}", cells.join(","))?;
            }
            Ok(())
        }
        ref other => writeln!(out, "data\n{}", csv_cell(other)),
    }
}

fn csv_cell(v: &Value) -> String {
    let raw = match v {
        Value::Str(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_compact(),
    };
    let needs_quote = raw
        .chars()
        .any(|c| c == ',' || c == '"' || c == '\n' || c == '\r');
    if needs_quote {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_env() -> EnvSnapshot {
        EnvSnapshot {
            stdout_is_tty: true,
            stderr_is_tty: true,
            stdin_is_tty: true,
            agent_mode: false,
            ci_mode: false,
            term_is_dumb: false,
            no_color: false,
            force_color: false,
            cli_color_off: false,
        }
    }

    #[test]
    fn tty_defaults_to_human() {
        let p = OutputPolicy::resolve(
            None,
            false,
            ColorMode::Auto,
            false,
            false,
            None,
            false,
            &base_env(),
        );
        assert_eq!(p.format, Format::Human);
        assert!(p.color);
    }

    #[test]
    fn pipe_defaults_to_json() {
        let mut e = base_env();
        e.stdout_is_tty = false;
        let p = OutputPolicy::resolve(None, false, ColorMode::Auto, false, false, None, false, &e);
        assert_eq!(p.format, Format::Json);
        assert!(!p.color);
    }

    #[test]
    fn agent_forces_json_no_color() {
        let mut e = base_env();
        e.agent_mode = true;
        let p = OutputPolicy::resolve(
            None,
            false,
            ColorMode::Always,
            false,
            false,
            None,
            false,
            &e,
        );
        assert_eq!(p.format, Format::Json);
        assert!(!p.color, "machine modes suppress ANSI unconditionally");
    }

    #[test]
    fn no_color_env_overrides_auto() {
        let mut e = base_env();
        e.no_color = true;
        let p = OutputPolicy::resolve(None, false, ColorMode::Auto, false, false, None, false, &e);
        assert!(!p.color);
    }

    #[test]
    fn force_color_beats_no_color() {
        let mut e = base_env();
        e.no_color = true;
        e.force_color = true;
        let p = OutputPolicy::resolve(None, false, ColorMode::Auto, false, false, None, false, &e);
        assert!(p.color, "FORCE_COLOR overrides NO_COLOR (§4.4)");
    }

    #[test]
    fn explicit_json_beats_tty_default() {
        let p = OutputPolicy::resolve(
            None,
            true,
            ColorMode::Auto,
            false,
            false,
            None,
            false,
            &base_env(),
        );
        assert_eq!(p.format, Format::Json);
    }

    #[test]
    fn explicit_format_beats_everything() {
        let mut e = base_env();
        e.agent_mode = true;
        let p = OutputPolicy::resolve(
            Some(Format::Yaml),
            false,
            ColorMode::Auto,
            false,
            false,
            None,
            false,
            &e,
        );
        assert_eq!(p.format, Format::Yaml);
    }

    #[test]
    fn csv_list_of_objects() {
        use std::io::Cursor;
        let env = obj([
            ("metadata", Value::Object(vec![])),
            (
                "data",
                Value::Array(vec![
                    obj([("name", Value::str("a")), ("n", Value::Int(1))]),
                    obj([("name", Value::str("b")), ("n", Value::Int(2))]),
                ]),
            ),
        ]);
        let mut buf = Cursor::new(Vec::new());
        to_csv(&env, &mut buf).unwrap();
        let s = String::from_utf8(buf.into_inner()).unwrap();
        assert_eq!(s, "name,n\na,1\nb,2\n");
    }

    #[test]
    fn csv_quotes_commas() {
        let cell = csv_cell(&Value::str("a,b"));
        assert_eq!(cell, "\"a,b\"");
    }

    #[test]
    fn format_parse_accepts_aliases() {
        assert_eq!(Format::parse("ndjson"), Some(Format::JsonL));
        assert_eq!(Format::parse("E"), Some(Format::Explore));
        assert_eq!(Format::parse("yml"), Some(Format::Yaml));
        assert_eq!(Format::parse("bogus"), None);
    }
}
