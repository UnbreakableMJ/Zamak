// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Input-hardening helpers (SFRS §7).
//!
//! Agent-originated arguments are untrusted: they may carry
//! path-traversal attempts, embedded control characters, absurd
//! numeric values, or shell metacharacters received from an upstream
//! indirect-prompt-injection source. All CLI entry points MUST run
//! their inputs through the helpers defined here before acting.

use std::path::{Path, PathBuf};

use crate::error::CliError;

/// Rejects any string that contains control bytes other than `\n`
/// (0x0A) and `\r` (0x0D). Agents frequently smuggle prompt-injection
/// payloads via NUL or escape bytes; text arguments to a CLI should
/// never legitimately contain them.
pub fn reject_control_chars(label: &str, s: &str) -> Result<(), CliError> {
    for (i, b) in s.bytes().enumerate() {
        if b == 0x0A || b == 0x0D {
            continue;
        }
        if b < 0x20 || b == 0x7F {
            return Err(CliError::invalid(format!(
                "{label}: rejected control byte {b:#04x} at position {i}"
            ))
            .with_hint(
                "Argument contains a control character (NUL, ESC, etc). \
                 Re-run with a cleaned value or pass the content via a file.",
            ));
        }
    }
    Ok(())
}

/// Bounds-checks an integer argument. Returns `CliError::invalid` if
/// the value is out of the `[min, max]` closed range. Uses the SFRS
/// `INVALID_ARGUMENT` code (exit 2).
pub fn check_bounds<T>(label: &str, value: T, min: T, max: T) -> Result<(), CliError>
where
    T: PartialOrd + std::fmt::Display,
{
    if value < min || value > max {
        return Err(CliError::invalid(format!(
            "{label}: value {value} out of range [{min}, {max}]"
        )));
    }
    Ok(())
}

/// Canonicalises a path and confirms it resolves inside one of the
/// `allowed` roots. An empty `allowed` list means "no restriction"
/// (trust the caller) — used for subcommands that legitimately
/// operate on block devices like `/dev/sda`.
#[allow(dead_code)] // called only by the test matrix and future restricted commands
pub fn safe_path(label: &str, path: &str, allowed: &[&Path]) -> Result<PathBuf, CliError> {
    reject_control_chars(label, path)?;
    // Quick syntactic traversal check BEFORE canonicalising, so the
    // error message is specific even when the target doesn't exist.
    if path.contains("/../") || path.contains("\\..\\") || path.ends_with("/..") {
        return Err(CliError::permission(format!(
            "{label}: rejected path-traversal component in '{path}'"
        )));
    }
    let candidate = PathBuf::from(path);
    if allowed.is_empty() {
        return Ok(candidate);
    }
    // Canonicalise parents so non-existent targets (e.g. the EFI
    // binary about to be written) still resolve.
    let parent = candidate
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let canon = parent
        .canonicalize()
        .map_err(|e| CliError::from_io(&format!("{label}: canonicalize '{path}'"), e))?;
    let ok = allowed.iter().any(|root| canon.starts_with(root));
    if !ok {
        return Err(CliError::permission(format!(
            "{label}: '{path}' resolves outside the allowed roots"
        )));
    }
    Ok(candidate)
}

/// Asks the user to confirm a destructive operation. In non-TTY
/// (agent / CI / piped) contexts this defaults to DENY unless
/// `--yes` / `--force` was passed (SFRS §7.2 item 20).
pub fn confirm_destructive(
    label: &str,
    stdin_is_tty: bool,
    yes: bool,
    force: bool,
) -> Result<(), CliError> {
    if yes || force {
        return Ok(());
    }
    if !stdin_is_tty {
        return Err(CliError::usage(format!(
            "{label}: destructive operation requires --yes or --force when stdin is not a TTY"
        ))
        .with_hint(format!(
            "Re-run with '--yes' after verifying the target: zamak {label} --yes"
        )));
    }
    // TTY path: prompt on stderr, read one line from stdin.
    use std::io::{self, Write as _};
    eprint!("{label}: type 'yes' to proceed: ");
    let _ = io::stderr().flush();
    let mut line = String::new();
    io::stdin()
        .read_line(&mut line)
        .map_err(|e| CliError::from_io("confirm prompt", e))?;
    if line.trim().eq_ignore_ascii_case("yes") {
        Ok(())
    } else {
        Err(CliError::usage(format!("{label}: declined by user")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_and_cr_are_allowed() {
        reject_control_chars("msg", "hello\nworld\rend").unwrap();
    }

    #[test]
    fn nul_byte_is_rejected() {
        let s = "a\0b";
        let err = reject_control_chars("msg", s).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn esc_byte_is_rejected() {
        let err = reject_control_chars("msg", "a\x1bb").unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn del_byte_is_rejected() {
        let err = reject_control_chars("msg", "a\x7fb").unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn bounds_accept_inclusive() {
        check_bounds("n", 5u64, 1, 10).unwrap();
        check_bounds("n", 1u64, 1, 10).unwrap();
        check_bounds("n", 10u64, 1, 10).unwrap();
    }

    #[test]
    fn bounds_reject_out_of_range() {
        let err = check_bounds("n", 11u64, 1, 10).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::InvalidArgument);
    }

    #[test]
    fn path_traversal_blocked() {
        let err = safe_path("cfg", "/tmp/../etc/passwd", &[]).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::PermissionDenied);
    }

    #[test]
    fn relative_traversal_blocked() {
        let err = safe_path("cfg", "a/../b", &[]).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::PermissionDenied);
    }

    #[test]
    fn allowed_roots_permit_inside() {
        // `/tmp` is canonical enough for this test on every POSIX box.
        let root = Path::new("/tmp");
        let _ = safe_path("cfg", "/tmp/zamak-test.txt", &[root]).unwrap();
    }

    #[test]
    fn destructive_non_tty_requires_yes() {
        let err = confirm_destructive("install", false, false, false).unwrap_err();
        assert_eq!(err.code, crate::error::ErrorCode::UsageError);
    }

    #[test]
    fn destructive_with_yes_passes() {
        confirm_destructive("install", false, true, false).unwrap();
        confirm_destructive("install", false, false, true).unwrap();
    }
}
