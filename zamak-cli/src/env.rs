// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Agent / CI / TTY / terminal-capability detection.
//!
//! Implements SFRS §9.1 (standardized env vars) and §4.1 (mode
//! detection precedence). Pure `std`, no external deps. POSIX-first:
//! `isatty` is called on Unix; Windows falls back to
//! "not a TTY" to bias toward machine mode.

use std::env;

/// Snapshot of all environment-driven decisions, computed once at
/// startup so the rest of the program sees a consistent view.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // stderr_is_tty: surface reserved for progress/spinner logic
pub struct EnvSnapshot {
    pub stdout_is_tty: bool,
    pub stderr_is_tty: bool,
    pub stdin_is_tty: bool,
    pub agent_mode: bool,
    pub ci_mode: bool,
    pub term_is_dumb: bool,
    pub no_color: bool,
    pub force_color: bool,
    pub cli_color_off: bool,
}

impl EnvSnapshot {
    /// Reads the environment and `isatty` state into a snapshot.
    pub fn capture() -> Self {
        Self {
            stdout_is_tty: is_tty(Fd::Stdout),
            stderr_is_tty: is_tty(Fd::Stderr),
            stdin_is_tty: is_tty(Fd::Stdin),
            agent_mode: truthy("AI_AGENT") || truthy("AGENT"),
            ci_mode: truthy("CI"),
            term_is_dumb: env::var("TERM").map(|t| t == "dumb").unwrap_or(false),
            no_color: env::var("NO_COLOR").map(|s| !s.is_empty()).unwrap_or(false),
            force_color: env::var("FORCE_COLOR")
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            cli_color_off: env::var("CLICOLOR").map(|s| s == "0").unwrap_or(false),
        }
    }

    /// Non-interactive = piped stdout OR agent OR CI OR TERM=dumb.
    #[allow(dead_code)]
    pub fn is_non_interactive(&self) -> bool {
        !self.stdout_is_tty || self.agent_mode || self.ci_mode || self.term_is_dumb
    }

    /// Agent-like invocation: any signal that a non-human is driving.
    #[allow(dead_code)]
    pub fn is_agent_like(&self) -> bool {
        self.agent_mode || self.ci_mode || !self.stdout_is_tty || self.term_is_dumb
    }
}

fn truthy(var: &str) -> bool {
    match env::var(var) {
        Ok(v) => {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        }
        Err(_) => false,
    }
}

#[derive(Clone, Copy)]
enum Fd {
    Stdin,
    Stdout,
    Stderr,
}

#[cfg(unix)]
fn is_tty(fd: Fd) -> bool {
    // SAFETY: isatty(3) is a read-only query on a file descriptor.
    // Preconditions: none (any int is accepted; returns 0 on invalid fd).
    // Postconditions: returns 1 iff fd refers to a terminal; errno is
    // clobbered on failure but we ignore it.
    unsafe extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    let fd = match fd {
        Fd::Stdin => 0,
        Fd::Stdout => 1,
        Fd::Stderr => 2,
    };
    unsafe { isatty(fd) == 1 }
}

#[cfg(not(unix))]
fn is_tty(_fd: Fd) -> bool {
    // SFRS §2.3.3: POSIX-first. Non-POSIX platforms bias toward
    // machine mode (return false) so that agents get JSON by default.
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_is_consistent() {
        let s = EnvSnapshot::capture();
        // If stdout is not a TTY, we must consider it non-interactive.
        if !s.stdout_is_tty {
            assert!(s.is_non_interactive());
        }
        // Agent-like implies non-interactive.
        if s.agent_mode || s.ci_mode {
            assert!(s.is_agent_like());
        }
    }

    #[test]
    fn truthy_accepts_canonical_forms() {
        unsafe { env::set_var("ZAMAK_TEST_TRUTHY", "1") };
        assert!(truthy("ZAMAK_TEST_TRUTHY"));
        unsafe { env::set_var("ZAMAK_TEST_TRUTHY", "true") };
        assert!(truthy("ZAMAK_TEST_TRUTHY"));
        unsafe { env::set_var("ZAMAK_TEST_TRUTHY", "claude-code") };
        assert!(truthy("ZAMAK_TEST_TRUTHY"));
        unsafe { env::set_var("ZAMAK_TEST_TRUTHY", "0") };
        assert!(!truthy("ZAMAK_TEST_TRUTHY"));
        unsafe { env::set_var("ZAMAK_TEST_TRUTHY", "false") };
        assert!(!truthy("ZAMAK_TEST_TRUTHY"));
        unsafe { env::set_var("ZAMAK_TEST_TRUTHY", "") };
        assert!(!truthy("ZAMAK_TEST_TRUTHY"));
        unsafe { env::remove_var("ZAMAK_TEST_TRUTHY") };
        assert!(!truthy("ZAMAK_TEST_TRUTHY"));
    }
}
