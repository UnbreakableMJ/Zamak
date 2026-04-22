// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Global CLI flag parser.
//!
//! Shared flags across every Steelbore CLI (SFRS §3.7):
//! `--json`, `--format <fmt>`, `--fields a,b,c`, `--dry-run`,
//! `--verbose`, `--quiet`, `--color=<auto|always|never>`,
//! `--no-color`, `--yes`, `--force`, `--print0` / `-0`.
//!
//! The parser is intentionally minimal (no `clap` dep) so that
//! `zamak-cli` stays a single-binary POSIX tool with no runtime
//! dependencies beyond `std`. It extracts global flags from the
//! argument list and returns the remainder for the sub-command
//! parser to consume.

use crate::error::CliError;
use crate::output::{ColorMode, Format};

/// Parsed global flags. Downstream code consumes this via
/// `OutputPolicy::resolve`.
#[derive(Debug, Default, Clone)]
pub struct GlobalFlags {
    pub format: Option<Format>,
    pub json: bool,
    pub fields: Option<Vec<String>>,
    pub dry_run: bool,
    pub verbose: bool,
    pub quiet: bool,
    pub color: ColorMode,
    pub yes: bool,
    pub force: bool,
    pub print0: bool,
    pub help: bool,
    pub version: bool,
}

impl Default for ColorMode {
    fn default() -> Self {
        ColorMode::Auto
    }
}

/// Result of `parse_globals`: the populated `GlobalFlags` plus the
/// leftover args (sub-command name + its own args).
#[derive(Debug)]
pub struct ParsedArgs {
    pub globals: GlobalFlags,
    pub remaining: Vec<String>,
}

/// Pulls every recognised global flag out of `args`, leaving
/// sub-command positional args and sub-command-specific options
/// behind. Unknown `--` tokens are preserved (the sub-command
/// parser decides whether they're errors).
pub fn parse_globals(args: &[String]) -> Result<ParsedArgs, CliError> {
    let mut globals = GlobalFlags::default();
    let mut remaining: Vec<String> = Vec::with_capacity(args.len());
    let mut it = args.iter();
    // `--version`/`-V` and `--help`/`-h` are tool-level globals ONLY
    // until we've seen the sub-command positional. After that, they
    // belong to the sub-command and must NOT be consumed here (e.g.
    // `zamak sbom --version 0.7.0` — `--version` is the sbom arg).
    let mut saw_subcommand = false;

    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => globals.json = true,
            "--format" => {
                let v = it
                    .next()
                    .ok_or_else(|| CliError::usage("--format requires a value"))?;
                globals.format =
                    Some(Format::parse(v).ok_or_else(|| {
                        CliError::usage(format!("--format: unknown value '{v}'"))
                    })?);
            }
            s if s.starts_with("--format=") => {
                let v = &s["--format=".len()..];
                globals.format =
                    Some(Format::parse(v).ok_or_else(|| {
                        CliError::usage(format!("--format: unknown value '{v}'"))
                    })?);
            }
            "-E" => globals.format = Some(Format::Explore),
            "--fields" => {
                let v = it
                    .next()
                    .ok_or_else(|| CliError::usage("--fields requires a value"))?;
                globals.fields = Some(parse_field_list(v));
            }
            s if s.starts_with("--fields=") => {
                globals.fields = Some(parse_field_list(&s["--fields=".len()..]));
            }
            "--dry-run" | "-n" => globals.dry_run = true,
            "--verbose" | "-v" => globals.verbose = true,
            "--quiet" | "-q" => globals.quiet = true,
            "--color" => {
                let v = it
                    .next()
                    .ok_or_else(|| CliError::usage("--color requires a value"))?;
                globals.color = ColorMode::parse(v)
                    .ok_or_else(|| CliError::usage(format!("--color: unknown value '{v}'")))?;
            }
            s if s.starts_with("--color=") => {
                let v = &s["--color=".len()..];
                globals.color = ColorMode::parse(v)
                    .ok_or_else(|| CliError::usage(format!("--color: unknown value '{v}'")))?;
            }
            "--no-color" => globals.color = ColorMode::Never,
            "--yes" | "-y" => globals.yes = true,
            "--force" => globals.force = true,
            "--print0" | "-0" => globals.print0 = true,
            "--help" | "-h" | "help" if !saw_subcommand => globals.help = true,
            "--version" | "-V" if !saw_subcommand => globals.version = true,
            "--" => {
                // Everything after `--` goes to the sub-command
                // verbatim (POSIX double-dash convention).
                remaining.push("--".to_string());
                for rest in it.by_ref() {
                    remaining.push(rest.clone());
                }
                break;
            }
            _ => {
                if !a.starts_with('-') {
                    saw_subcommand = true;
                }
                remaining.push(a.clone());
            }
        }
    }

    Ok(ParsedArgs { globals, remaining })
}

fn parse_field_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_args_defaults() {
        let p = parse_globals(&[]).unwrap();
        assert!(!p.globals.json);
        assert!(p.remaining.is_empty());
    }

    #[test]
    fn json_flag_sets_bool() {
        let p = parse_globals(&args(&["--json", "sbom"])).unwrap();
        assert!(p.globals.json);
        assert_eq!(p.remaining, vec!["sbom"]);
    }

    #[test]
    fn format_with_space_and_equals() {
        let p = parse_globals(&args(&["--format", "yaml", "install"])).unwrap();
        assert_eq!(p.globals.format, Some(Format::Yaml));
        let p = parse_globals(&args(&["--format=json", "install"])).unwrap();
        assert_eq!(p.globals.format, Some(Format::Json));
    }

    #[test]
    fn fields_csv() {
        let p = parse_globals(&args(&["--fields", "a, b ,c", "sbom"])).unwrap();
        assert_eq!(
            p.globals.fields,
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
    }

    #[test]
    fn dry_run_short_and_long() {
        assert!(
            parse_globals(&args(&["--dry-run"]))
                .unwrap()
                .globals
                .dry_run
        );
        assert!(parse_globals(&args(&["-n"])).unwrap().globals.dry_run);
    }

    #[test]
    fn double_dash_stops_parsing() {
        let p = parse_globals(&args(&["--json", "--", "--not-a-flag"])).unwrap();
        assert!(p.globals.json);
        assert_eq!(p.remaining, vec!["--", "--not-a-flag"]);
    }

    #[test]
    fn explore_short_alias() {
        let p = parse_globals(&args(&["-E", "sbom"])).unwrap();
        assert_eq!(p.globals.format, Some(Format::Explore));
    }

    #[test]
    fn no_color_sets_never() {
        let p = parse_globals(&args(&["--no-color", "install"])).unwrap();
        assert_eq!(p.globals.color, ColorMode::Never);
    }

    #[test]
    fn unknown_format_is_usage_error() {
        let err = parse_globals(&args(&["--format", "xml"])).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::UsageError);
    }

    #[test]
    fn print0_short_and_long() {
        assert!(parse_globals(&args(&["-0"])).unwrap().globals.print0);
        assert!(parse_globals(&args(&["--print0"])).unwrap().globals.print0);
    }

    #[test]
    fn subcommand_passthrough_preserves_order() {
        let p = parse_globals(&args(&[
            "install", "--mbr", "a.bin", "--json", "--target", "t",
        ]))
        .unwrap();
        assert!(p.globals.json);
        assert_eq!(
            p.remaining,
            vec!["install", "--mbr", "a.bin", "--target", "t"]
        );
    }
}
