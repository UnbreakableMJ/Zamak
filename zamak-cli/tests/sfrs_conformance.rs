// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! SFRS §11.1 compliance matrix — integration-level tests that
//! exercise the `zamak` binary end-to-end.
//!
//! Every test invokes the compiled `zamak` binary via `std::process::
//! Command` and asserts SFRS-mandated behaviour:
//!
//! - Exit-code correctness (§3.2 / §11.1 Exit-Code Tests)
//! - TTY / non-TTY mode switching (§4.1 / §11.1)
//! - AI_AGENT=1 forces JSON + no color (§9.1 / §11.1)
//! - JSON Schema validation (§6.1 / §11.1)
//! - Structured error envelope on stderr (§3.5)
//! - UTF-8 without BOM (§2.3.2 / §11.1)
//! - `NO_COLOR` suppresses ANSI codes (§4.4)
//! - `--fields` projection (§3.6)
//! - `--dry-run` on every write command (§3.3)

use std::process::{Command, Stdio};

/// Resolve the compiled `zamak` binary. Cargo exposes it as
/// `CARGO_BIN_EXE_zamak` at test-time.
fn zamak() -> Command {
    let path = env!("CARGO_BIN_EXE_zamak");
    let mut cmd = Command::new(path);
    // Keep tests agent-mode by default so the binary takes its
    // machine-readable paths and stays off the TTY path even when
    // run under a dev's shell.
    cmd.env("AI_AGENT", "1");
    cmd.env("NO_COLOR", "1");
    cmd.stdin(Stdio::null());
    cmd
}

/// Runs the binary and returns (exit_code, stdout_bytes, stderr_bytes).
fn run(args: &[&str]) -> (i32, Vec<u8>, Vec<u8>) {
    let out = zamak()
        .args(args)
        .output()
        .expect("failed to invoke zamak");
    (out.status.code().unwrap_or(-1), out.stdout, out.stderr)
}

fn stdout_str(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).expect("stdout must be valid UTF-8")
}

// ------------- exit-code table -------------

#[test]
fn exit_0_on_describe() {
    let (code, _, _) = run(&["describe"]);
    assert_eq!(code, 0);
}

#[test]
fn exit_2_on_bad_global_flag() {
    let (code, _, _) = run(&["--format", "xml"]);
    assert_eq!(code, 2, "--format xml should be USAGE_ERROR (2)");
}

#[test]
fn exit_2_on_unknown_subcommand() {
    let (code, _, _) = run(&["bogus"]);
    assert_eq!(code, 2);
}

#[test]
fn exit_2_on_missing_required_arg() {
    let (code, _, _) = run(&["install", "--target", "/tmp/x.img"]);
    assert_eq!(code, 2, "install without --mbr should be USAGE_ERROR");
}

#[test]
fn exit_3_on_schema_unknown_command() {
    let (code, _, _) = run(&["schema", "bogus"]);
    assert_eq!(code, 3, "NOT_FOUND when schema target doesn't exist");
}

// ------------- AI_AGENT forces JSON on TTY inheritance -------------

#[test]
fn agent_mode_emits_json_envelope_on_stdout() {
    let (code, out, _) = run(&["describe"]);
    assert_eq!(code, 0);
    let s = stdout_str(&out);
    assert!(
        s.contains(r#""metadata""#) && s.contains(r#""data""#),
        "agent mode must emit envelope; got: {s}"
    );
}

#[test]
fn agent_mode_error_goes_to_stderr_as_json() {
    let (code, _out, err) = run(&["bogus"]);
    assert_eq!(code, 2);
    let e = stdout_str(&err);
    assert!(
        e.contains(r#""error""#) && e.contains(r#""code":"USAGE_ERROR""#),
        "structured error envelope missing on stderr: {e}"
    );
}

// ------------- JSON schema shape -------------

#[test]
fn full_schema_references_2020_12_dialect() {
    let (code, out, _) = run(&["schema"]);
    assert_eq!(code, 0);
    let s = stdout_str(&out);
    assert!(s.contains("https://json-schema.org/draft/2020-12/schema"));
}

#[test]
fn single_command_schema_has_exit_code_map() {
    let (code, out, _) = run(&["schema", "install"]);
    assert_eq!(code, 0);
    let s = stdout_str(&out);
    assert!(s.contains(r#""0":"SUCCESS""#));
    assert!(s.contains(r#""3":"NOT_FOUND""#));
    assert!(s.contains(r#""destructive":true"#));
    assert!(s.contains(r#""supports_dry_run":true"#));
}

// ------------- no-color + UTF-8 + BOM -------------

#[test]
fn json_output_is_valid_utf8_without_bom() {
    let (code, out, _) = run(&["describe"]);
    assert_eq!(code, 0);
    assert!(
        !out.starts_with(&[0xEF, 0xBB, 0xBF]),
        "stdout must not start with UTF-8 BOM"
    );
    std::str::from_utf8(&out).expect("stdout must be valid UTF-8");
}

#[test]
fn json_output_contains_no_ansi_escapes() {
    let (_, out, _) = run(&["describe"]);
    for (i, b) in out.iter().enumerate() {
        assert_ne!(*b, 0x1B, "stdout byte {i} is ESC (0x1B)");
    }
}

// ------------- --fields projection -------------

#[test]
fn fields_projection_limits_data_keys() {
    let (code, out, _) = run(&["describe", "--fields", "tool,version"]);
    assert_eq!(code, 0);
    let s = stdout_str(&out);
    assert!(s.contains(r#""tool""#));
    assert!(s.contains(r#""version""#));
    assert!(
        !s.contains(r#""commands""#),
        "commands field should have been projected away: {s}"
    );
}

// ------------- jsonl streaming -------------

#[test]
fn jsonl_format_emits_lines() {
    let (code, out, _) = run(&["--format", "jsonl", "describe"]);
    assert_eq!(code, 0);
    let s = stdout_str(&out);
    // describe's data is a single object, so jsonl emits the object
    // itself on one line.
    assert_eq!(s.trim_end().lines().count(), 1, "got: {s}");
    // Each line must be independently parseable JSON.
    for line in s.trim_end().lines() {
        assert!(line.trim_start().starts_with('{'));
    }
}

// ------------- dry-run idempotency -------------

#[test]
fn dry_run_flag_is_honoured_by_install() {
    // Point at nonexistent paths — the binary still returns NOT_FOUND
    // because it needs to read the MBR, but the --dry-run flag must
    // not be rejected as unknown.
    let (code, _out, err) = run(&[
        "install",
        "--mbr",
        "/nonexistent-zamak-test.bin",
        "--stage2",
        "/nonexistent-zamak-test.bin",
        "--target",
        "/tmp/zamak-test-tgt.img",
        "--dry-run",
    ]);
    assert_eq!(code, 3, "expected NOT_FOUND for missing MBR; got {code}");
    // Verify that the error is the file-not-found, not a usage error.
    let e = stdout_str(&err);
    assert!(
        e.contains(r#""code":"NOT_FOUND""#),
        "expected structured NOT_FOUND error: {e}"
    );
}

// ------------- control-char rejection (§7.2) -------------

#[test]
fn control_char_in_version_arg_is_rejected() {
    let (code, _out, err) = run(&["sbom", "--version", "1.0\x01"]);
    assert_eq!(code, 2);
    let e = stdout_str(&err);
    assert!(
        e.contains(r#""code":"INVALID_ARGUMENT""#),
        "expected INVALID_ARGUMENT: {e}"
    );
}

// ------------- completions sub-command (§8.1) -------------

#[test]
fn completions_bash_prints_script() {
    let (code, out, _) = run(&["completions", "bash"]);
    assert_eq!(code, 0);
    let s = stdout_str(&out);
    assert!(s.contains("_zamak()"));
    assert!(s.contains("complete -F _zamak zamak"));
}

#[test]
fn completions_unknown_shell_is_invalid() {
    let (code, _, err) = run(&["completions", "xonsh"]);
    assert_eq!(code, 2);
    let e = stdout_str(&err);
    assert!(e.contains(r#""code":"INVALID_ARGUMENT""#));
}

// ------------- describe manifest contract (§6.2) -------------

#[test]
fn describe_enumerates_every_shipped_command() {
    let (_, out, _) = run(&["describe"]);
    let s = stdout_str(&out);
    for cmd in ["install", "enroll-config", "sbom", "schema", "describe", "completions"] {
        assert!(
            s.contains(&format!(r#""name":"{cmd}""#)),
            "describe missing '{cmd}': {s}"
        );
    }
}

#[test]
fn describe_reports_global_flag_surface() {
    let (_, out, _) = run(&["describe"]);
    let s = stdout_str(&out);
    for flag in [
        "--json",
        "--format",
        "--fields",
        "--dry-run",
        "--verbose",
        "--quiet",
        "--color",
        "--no-color",
        "--yes",
        "--force",
        "--print0",
    ] {
        assert!(s.contains(flag), "global flag '{flag}' not advertised: {s}");
    }
}

// ------------- version contract -------------

#[test]
fn version_flag_emits_json_under_agent() {
    let (code, out, _) = run(&["--version"]);
    assert_eq!(code, 0);
    let s = stdout_str(&out);
    assert!(
        s.contains(r#""tool""#) && s.contains(r#""version""#),
        "agent-mode version output must be JSON: {s}"
    );
}
