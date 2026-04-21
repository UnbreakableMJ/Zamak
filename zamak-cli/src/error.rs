// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Structured errors and exit-code mapping per SFRS §3.2 / §3.5.
//!
//! The error envelope emitted on stderr in machine modes:
//!
//! ```json
//! { "error": {
//!     "code": "NOT_FOUND",
//!     "exit_code": 3,
//!     "message": "Repository 'foo' does not exist",
//!     "hint": "Run 'zamak describe --json' to list commands",
//!     "timestamp": "2026-04-21T12:00:00Z",
//!     "command": "zamak install --target /dev/sda",
//!     "docs_url": "https://steelbore.dev/zamak/errors#not_found"
//! }}
//! ```

use crate::json::{obj, Value};
use crate::output::{Format, OutputPolicy};

/// Canonical UPPER_SNAKE_CASE error codes (SFRS §3.5). Stable across
/// minor versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // RateLimited / InternalError reserved for future network/internal paths
pub enum ErrorCode {
    Success,
    General,
    UsageError,
    NotFound,
    PermissionDenied,
    Conflict,
    InvalidArgument,
    RateLimited,
    InternalError,
}

impl ErrorCode {
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorCode::Success => 0,
            ErrorCode::General => 1,
            ErrorCode::UsageError => 2,
            ErrorCode::NotFound => 3,
            ErrorCode::PermissionDenied => 4,
            ErrorCode::Conflict => 5,
            ErrorCode::InvalidArgument => 2,
            ErrorCode::RateLimited => 6,
            ErrorCode::InternalError => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Success => "SUCCESS",
            ErrorCode::General => "GENERAL_ERROR",
            ErrorCode::UsageError => "USAGE_ERROR",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::Conflict => "CONFLICT",
            ErrorCode::InvalidArgument => "INVALID_ARGUMENT",
            ErrorCode::RateLimited => "RATE_LIMITED",
            ErrorCode::InternalError => "INTERNAL_ERROR",
        }
    }
}

/// A structured error with the fields needed to produce the §3.5
/// envelope. `hint` is the tips-thinking field — it MUST contain the
/// exact command syntax needed to resolve the problem.
#[derive(Debug, Clone)]
pub struct CliError {
    pub code: ErrorCode,
    pub message: String,
    pub hint: Option<String>,
    pub docs_url: Option<String>,
    /// Optional `std::io::Error` kind name for diagnostics.
    pub io_kind: Option<String>,
}

#[allow(dead_code)] // permission/conflict/general/with_docs: reserved for future commands
impl CliError {
    pub fn new<S: Into<String>>(code: ErrorCode, message: S) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            docs_url: None,
            io_kind: None,
        }
    }

    pub fn usage<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCode::UsageError, message)
            .with_hint("Run 'zamak --help' or 'zamak describe --json' for usage.")
    }

    pub fn not_found<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn invalid<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    pub fn permission<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCode::PermissionDenied, message)
    }

    pub fn conflict<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn general<S: Into<String>>(message: S) -> Self {
        Self::new(ErrorCode::General, message)
    }

    pub fn with_hint<S: Into<String>>(mut self, hint: S) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_docs<S: Into<String>>(mut self, url: S) -> Self {
        self.docs_url = Some(url.into());
        self
    }

    pub fn from_io(context: &str, err: std::io::Error) -> Self {
        let code = match err.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            std::io::ErrorKind::AlreadyExists => ErrorCode::Conflict,
            _ => ErrorCode::General,
        };
        let mut e = Self::new(code, format!("{context}: {err}"));
        e.io_kind = Some(format!("{:?}", err.kind()));
        e
    }
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

/// Emits the error to stderr in the right shape for the negotiated
/// format, and returns the exit code to use.
pub fn emit(policy: &OutputPolicy, command: &str, err: &CliError) -> i32 {
    let ts = crate::time::iso8601_now();
    match policy.format {
        Format::Json | Format::JsonL | Format::Yaml | Format::Csv => {
            let mut inner = vec![
                ("code".to_string(), Value::str(err.code.as_str())),
                ("exit_code".to_string(), Value::Int(err.code.exit_code() as i64)),
                ("message".to_string(), Value::str(&err.message)),
                ("timestamp".to_string(), Value::str(ts)),
                ("command".to_string(), Value::str(command)),
            ];
            if let Some(h) = &err.hint {
                inner.push(("hint".to_string(), Value::str(h)));
            }
            if let Some(u) = &err.docs_url {
                inner.push(("docs_url".to_string(), Value::str(u)));
            }
            if let Some(k) = &err.io_kind {
                inner.push(("io_kind".to_string(), Value::str(k)));
            }
            let envelope = obj([("error", Value::Object(inner))]);
            // SFRS §8.3: PowerShell parses each stderr line as a
            // separate ErrorRecord — emit one line, never pretty.
            eprintln!("{}", envelope.to_compact());
        }
        _ => {
            // Human-readable.
            let p = crate::output::Palette { color: policy.color };
            let tag = p.paint(crate::output::Palette::RED_OXIDE, "[ERROR]");
            eprintln!("{ts} {tag} {}: {}", err.code.as_str(), err.message);
            if let Some(h) = &err.hint {
                let hint_tag = p.paint(crate::output::Palette::LIQUID_COOLANT, "hint:");
                eprintln!("       {hint_tag} {h}");
            }
            if let Some(u) = &err.docs_url {
                let docs_tag = p.paint(crate::output::Palette::STEEL_BLUE, "docs:");
                eprintln!("       {docs_tag} {u}");
            }
        }
    }
    err.code.exit_code()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_matches_sfrs_table() {
        assert_eq!(ErrorCode::Success.exit_code(), 0);
        assert_eq!(ErrorCode::General.exit_code(), 1);
        assert_eq!(ErrorCode::UsageError.exit_code(), 2);
        assert_eq!(ErrorCode::NotFound.exit_code(), 3);
        assert_eq!(ErrorCode::PermissionDenied.exit_code(), 4);
        assert_eq!(ErrorCode::Conflict.exit_code(), 5);
    }

    #[test]
    fn io_error_maps_kind() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "nope");
        let err = CliError::from_io("opening", io);
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(err.message.contains("nope"));
        assert_eq!(err.io_kind.as_deref(), Some("NotFound"));
    }

    #[test]
    fn usage_includes_hint() {
        let err = CliError::usage("missing --foo");
        assert!(err.hint.is_some());
        assert_eq!(err.code, ErrorCode::UsageError);
    }

    #[test]
    fn error_codes_are_upper_snake() {
        for c in [
            ErrorCode::Success,
            ErrorCode::NotFound,
            ErrorCode::PermissionDenied,
            ErrorCode::Conflict,
            ErrorCode::InvalidArgument,
            ErrorCode::RateLimited,
            ErrorCode::InternalError,
        ] {
            let s = c.as_str();
            assert!(
                s.chars().all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit()),
                "{s} is not UPPER_SNAKE_CASE"
            );
        }
    }
}
