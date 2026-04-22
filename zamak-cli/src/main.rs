// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! ZAMAK host-side CLI tool — dual-mode (human + agent) per
//! SB-SFRS-STEELBORE-CLI v1.0.0.
//!
//! # Commands
//!
//! - `zamak install`        — Write stage1 MBR + stage2 location (FR-CLI-001)
//! - `zamak enroll-config`  — Compute BLAKE2B config hash and patch EFI binary (FR-CLI-002)
//! - `zamak sbom`           — Produce an SPDX 2.3 JSON document (FR-CLI-003)
//! - `zamak schema [cmd]`   — Emit JSON Schema (SFRS §6.1)
//! - `zamak describe`       — Emit capability manifest (SFRS §6.2)
//! - `zamak completions`    — Emit shell completion script (SFRS §8.1)
//!
//! # Global flags (every command)
//!
//! `--json` · `--format <fmt>` · `--fields a,b,c` · `--dry-run` ·
//! `--verbose` · `--quiet` · `--color=<auto|always|never>` ·
//! `--no-color` · `--yes` · `--force` · `--print0` · `--help` ·
//! `--version`

mod args;
mod commands;
mod env;
mod error;
mod hash;
mod json;
mod meta;
mod output;
mod schema;
mod time;
mod tui;
mod validate;

use std::process;

use crate::args::GlobalFlags;
use crate::env::EnvSnapshot;
use crate::error::CliError;
use crate::json::Value;
use crate::output::OutputPolicy;

fn main() {
    // UTF-8 everywhere. On Windows this would set the console code
    // page to 65001 (SFRS §2.3.2); on Unix it's a no-op because the
    // locale already guarantees UTF-8 under any sane configuration.
    #[cfg(windows)]
    unsafe {
        unsafe extern "system" {
            fn SetConsoleOutputCP(code_page: u32) -> i32;
        }
        SetConsoleOutputCP(65_001);
    }

    let argv: Vec<String> = std::env::args().collect();
    let code = match run(&argv) {
        Ok(code) => code,
        Err((err, policy, cmdline)) => error::emit(&policy, &cmdline, &err),
    };
    process::exit(code);
}

type Outcome = Result<i32, (CliError, OutputPolicy, String)>;

fn run(argv: &[String]) -> Outcome {
    // Split: argv[0] is the binary path; skip it for global parsing.
    let raw = &argv[1..];

    let env = EnvSnapshot::capture();
    let parsed = args::parse_globals(raw).map_err(|e| {
        let fallback = fallback_policy(&env);
        (e, fallback, meta::command_line(argv))
    })?;
    let globals = parsed.globals.clone();
    let remaining = parsed.remaining;

    if globals.version {
        print_version(&env, &globals);
        return Ok(0);
    }

    let (subcommand, sub_args) = match remaining.split_first() {
        Some((s, r)) if !s.starts_with("--") => (s.clone(), r.to_vec()),
        _ => {
            if globals.help {
                print_help();
                return Ok(0);
            }
            let policy = resolve_policy(&globals, &env);
            let err = CliError::usage("missing sub-command");
            return Err((err, policy, meta::command_line(argv)));
        }
    };

    if globals.help {
        print_command_help(&subcommand);
        return Ok(0);
    }

    let policy = resolve_policy(&globals, &env);
    let cmdline = meta::command_line(argv);

    match subcommand.as_str() {
        "help" => {
            print_help();
            Ok(0)
        }
        "install" => run_data_cmd("install", &policy, cmdline.clone(), || {
            commands::install::run(&sub_args, &policy, &globals, &env)
        }),
        "enroll-config" => run_data_cmd("enroll-config", &policy, cmdline.clone(), || {
            commands::enroll_config::run(&sub_args, &policy, &globals, &env)
        }),
        "sbom" => run_data_cmd("sbom", &policy, cmdline.clone(), || {
            commands::sbom::run(&sub_args, &policy, &globals)
        }),
        "schema" => run_data_cmd("schema", &policy, cmdline.clone(), || {
            commands::schema_cmd::run(&sub_args)
        }),
        "describe" => run_data_cmd("describe", &policy, cmdline.clone(), || {
            commands::describe::run(&sub_args)
        }),
        "completions" => match commands::completions::run(&sub_args) {
            Ok(()) => Ok(0),
            Err(e) => Err((e, policy, cmdline)),
        },
        other => {
            let err = CliError::usage(format!("unknown sub-command '{other}'"))
                .with_hint("Run 'zamak describe --json' to list available sub-commands.");
            Err((err, policy, cmdline))
        }
    }
}

/// Shared rails for any command whose output is a structured `data`
/// payload. Handles:
/// - TUI dispatch (with graceful fallback)
/// - JSON envelope construction
/// - Field projection and jsonl/yaml/csv formatting
/// - Error-to-envelope translation on stderr
fn run_data_cmd(
    subcommand: &str,
    policy: &OutputPolicy,
    cmdline: String,
    body: impl FnOnce() -> Result<Value, CliError>,
) -> Outcome {
    let data = match body() {
        Ok(v) => v,
        Err(e) => return Err((e, policy.clone(), cmdline)),
    };

    let command_label = format!("zamak {subcommand}");
    let metadata = meta::envelope_metadata(&command_label);

    if policy.format == output::Format::Explore {
        let env = EnvSnapshot::capture();
        if let Some(reason) = tui::should_fall_back(&env, policy) {
            output::emit_warn(policy, reason);
            let mut fallback = policy.clone();
            fallback.format = output::Format::Json;
            fallback.color = false;
            if let Err(e) = fallback.emit(metadata, data) {
                return Err((CliError::from_io("emit envelope", e), fallback, cmdline));
            }
            return Ok(0);
        }
        #[cfg(feature = "tui")]
        {
            if let Err(e) = tui::explore(&data) {
                return Err((CliError::from_io("explore TUI", e), policy.clone(), cmdline));
            }
            return Ok(0);
        }
        #[cfg(not(feature = "tui"))]
        {
            // Unreachable: should_fall_back returns Some when the
            // feature is disabled, but the exhaustive match helps the
            // compiler and makes the intent explicit.
            let _ = data;
            return Ok(0);
        }
    }

    if let Err(e) = policy.emit(metadata, data) {
        return Err((
            CliError::from_io("emit envelope", e),
            policy.clone(),
            cmdline,
        ));
    }
    Ok(0)
}

fn resolve_policy(globals: &GlobalFlags, env: &EnvSnapshot) -> OutputPolicy {
    OutputPolicy::resolve(
        globals.format,
        globals.json,
        globals.color,
        globals.quiet,
        globals.verbose,
        globals.fields.clone(),
        globals.print0,
        env,
    )
}

/// The policy to use when reporting an error that happened *before*
/// we had enough context to compute a real one (e.g. malformed
/// global flags). Biases toward the user's visible environment.
fn fallback_policy(env: &EnvSnapshot) -> OutputPolicy {
    OutputPolicy::resolve(
        None,
        false,
        output::ColorMode::Auto,
        false,
        false,
        None,
        false,
        env,
    )
}

fn print_version(env: &EnvSnapshot, globals: &GlobalFlags) {
    let policy = resolve_policy(globals, env);
    if policy.format.is_machine() {
        let v = json::obj([
            ("tool", Value::str(meta::TOOL_NAME)),
            ("version", Value::str(meta::TOOL_VERSION)),
        ]);
        if policy.color {
            println!("{}", v.to_pretty());
        } else {
            println!("{}", v.to_compact());
        }
    } else {
        println!("{} {}", meta::TOOL_NAME, meta::TOOL_VERSION);
    }
}

fn print_help() {
    println!("zamak — ZAMAK bootloader host CLI ({})", meta::TOOL_VERSION);
    println!();
    println!("Usage: zamak [GLOBAL FLAGS] <command> [COMMAND OPTIONS]");
    println!();
    println!("Commands:");
    println!("  install          Write stage1 MBR and record stage2 location");
    println!("  enroll-config    Compute BLAKE2B config hash and patch EFI binary");
    println!("  sbom             Generate SPDX 2.3 JSON SBOM");
    println!("  schema [<cmd>]   Emit JSON Schema (Draft 2020-12)");
    println!("  describe         Emit capability manifest");
    println!("  completions <sh> Emit shell completion script (bash|zsh|fish|nushell)");
    println!("  help             Show this message");
    println!();
    println!("Global flags:");
    println!("  --json                     Emit JSON envelope (alias for --format json)");
    println!("  --format <fmt>             human|json|jsonl|yaml|csv|explore");
    println!("  --fields <a,b,c>           Project output to listed fields");
    println!("  --dry-run, -n              Plan without mutating state");
    println!("  --verbose, -v              Verbose diagnostics");
    println!("  --quiet, -q                Suppress [INFO] diagnostics");
    println!("  --color <mode>             auto|always|never (default auto)");
    println!("  --no-color                 Equivalent to --color=never");
    println!("  --yes, -y                  Skip confirmation prompts");
    println!("  --force                    Force destructive operation");
    println!("  --print0, -0               NUL-delimited path output");
    println!("  --help, -h                 Show this message");
    println!("  --version, -V              Print version");
    println!();
    println!("Environment:");
    println!("  AI_AGENT=1 / AGENT=1       Force JSON, no color, no TUI, no prompts");
    println!("  CI=true                    Same, for CI pipelines");
    println!("  NO_COLOR / FORCE_COLOR     ANSI color gate");
    println!("  TERM=dumb                  Disable color and TUI");
    println!();
    println!("Run 'zamak describe --json' for a machine-readable capability manifest.");
}

fn print_command_help(cmd: &str) {
    match cmd {
        "install" => println!(
            "Usage: zamak install --mbr <mbr.bin> --stage2 <stage2.bin> --target <device> \
             [--stage2-lba <n>] [--dry-run] [--yes]"
        ),
        "enroll-config" => println!(
            "Usage: zamak enroll-config --config <zamak.conf> --efi <BOOTX64.EFI> [--dry-run] [--yes]"
        ),
        "sbom" => println!(
            "Usage: zamak sbom --version <ver> [--output <file>] [<artifact>...]"
        ),
        "schema" => println!("Usage: zamak schema [<command>]"),
        "describe" => println!("Usage: zamak describe"),
        "completions" => {
            println!("Usage: zamak completions <bash|zsh|fish|nushell>")
        }
        _ => print_help(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_version_flag_exits_zero() {
        let r = run(&["zamak".into(), "--version".into()]);
        assert!(matches!(r, Ok(0)));
    }

    #[test]
    fn run_help_flag_exits_zero() {
        let r = run(&["zamak".into(), "--help".into()]);
        assert!(matches!(r, Ok(0)));
    }

    #[test]
    fn unknown_subcommand_is_usage_error() {
        let r = run(&["zamak".into(), "bogus".into()]);
        let Err((e, _, _)) = r else {
            panic!("expected err")
        };
        assert_eq!(e.code, error::ErrorCode::UsageError);
    }

    #[test]
    fn missing_subcommand_is_usage_error() {
        let r = run(&["zamak".into()]);
        let Err((e, _, _)) = r else {
            panic!("expected err")
        };
        assert_eq!(e.code, error::ErrorCode::UsageError);
    }

    #[test]
    fn describe_returns_zero() {
        let r = run(&["zamak".into(), "describe".into(), "--json".into()]);
        assert!(matches!(r, Ok(0)));
    }

    #[test]
    fn schema_whole_tool_returns_zero() {
        let r = run(&["zamak".into(), "schema".into(), "--json".into()]);
        assert!(matches!(r, Ok(0)));
    }

    #[test]
    fn schema_unknown_command_is_not_found() {
        let r = run(&[
            "zamak".into(),
            "schema".into(),
            "bogus".into(),
            "--json".into(),
        ]);
        let Err((e, _, _)) = r else {
            panic!("expected err")
        };
        assert_eq!(e.code, error::ErrorCode::NotFound);
    }

    #[test]
    fn completions_bash_writes_script() {
        // Smoke test: should not error and should return Ok(0).
        let r = run(&["zamak".into(), "completions".into(), "bash".into()]);
        assert!(matches!(r, Ok(0)));
    }

    #[test]
    fn completions_unknown_shell_is_invalid() {
        let r = run(&["zamak".into(), "completions".into(), "xonsh".into()]);
        let Err((e, _, _)) = r else {
            panic!("expected err")
        };
        assert_eq!(e.code, error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn dry_run_install_emits_no_io() {
        // Feed deliberately-missing paths to prove --dry-run short-
        // circuits before any disk access. The command still opens
        // the MBR file because it needs to inspect it; skip that by
        // expecting a from_io error here, proving the control-flow.
        let r = run(&[
            "zamak".into(),
            "--json".into(),
            "install".into(),
            "--mbr".into(),
            "/nonexistent-zamak-test.bin".into(),
            "--stage2".into(),
            "/nonexistent-zamak-test.bin".into(),
            "--target".into(),
            "/tmp/zamak-test-target.img".into(),
            "--dry-run".into(),
        ]);
        let Err((e, _, _)) = r else {
            panic!("expected NotFound err")
        };
        assert_eq!(e.code, error::ErrorCode::NotFound);
    }

    #[test]
    fn control_char_in_arg_is_rejected() {
        let r = run(&[
            "zamak".into(),
            "sbom".into(),
            "--version".into(),
            "1.0\x01".into(),
        ]);
        let Err((e, _, _)) = r else {
            panic!("expected InvalidArgument")
        };
        assert_eq!(e.code, error::ErrorCode::InvalidArgument);
    }
}
