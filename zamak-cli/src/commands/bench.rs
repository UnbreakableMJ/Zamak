// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! `zamak bench parse-serial` — host-side consumer for the
//! TSC-based boot-phase instrumentation `zamak-uefi` emits to
//! COM1 (M6-3 part 1).
//!
//! Reads a UEFI serial log (file path or stdin) and emits an
//! SFRS-canonical JSON envelope whose `data` payload is:
//!
//! ```json
//! {
//!   "tsc_mhz": 2400,
//!   "phases": [
//!     {"phase": "uefi_entry",        "tsc": 1000000, "delta_cycles": 0},
//!     {"phase": "config_parsed",     "tsc": 1100000, "delta_cycles": 100000,
//!      "delta_ns": 41666.67},
//!     ...
//!   ]
//! }
//! ```
//!
//! `delta_ns` is emitted only when a TSC frequency is known —
//! either the log contained a `ZAMAK_TSC_MHZ=<n>` line or the
//! caller passed `--tsc-mhz <n>` on the CLI (explicit wins over
//! log-reported).

// Rust guideline compliant 2026-03-30

use std::fs;
use std::io::{self, Read as _};

use crate::error::CliError;
use crate::json::{obj, Value};

/// Entry point wired into `main.rs`' sub-command match.
pub fn run(args: &[String]) -> Result<Value, CliError> {
    let sub = args
        .first()
        .ok_or_else(|| CliError::usage("bench: missing sub-verb (expected 'parse-serial')"))?;
    match sub.as_str() {
        "parse-serial" => run_parse_serial(&args[1..]),
        other => Err(CliError::usage(format!(
            "bench: unknown sub-verb '{other}' (expected 'parse-serial')"
        ))),
    }
}

/// `zamak bench parse-serial [--tsc-mhz <n>] [<path>]`.
fn run_parse_serial(args: &[String]) -> Result<Value, CliError> {
    let mut tsc_mhz_override: Option<u64> = None;
    let mut path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--tsc-mhz" => {
                let v = args
                    .get(i + 1)
                    .ok_or_else(|| CliError::usage("--tsc-mhz requires a value (MHz integer)"))?;
                let n: u64 = v
                    .parse()
                    .map_err(|_| CliError::usage(format!("--tsc-mhz: invalid MHz value '{v}'")))?;
                if n == 0 {
                    return Err(CliError::usage("--tsc-mhz must be > 0"));
                }
                tsc_mhz_override = Some(n);
                i += 2;
            }
            "--help" | "-h" => {
                return Err(CliError::usage(
                    "usage: zamak bench parse-serial [--tsc-mhz <mhz>] [<path>]",
                ));
            }
            arg if arg.starts_with("--") => {
                return Err(CliError::usage(format!(
                    "bench parse-serial: unknown flag '{arg}'"
                )));
            }
            _ => {
                if path.is_some() {
                    return Err(CliError::usage(
                        "bench parse-serial: only one positional path allowed",
                    ));
                }
                path = Some(args[i].clone());
                i += 1;
            }
        }
    }

    let input = match path {
        Some(p) => fs::read_to_string(&p).map_err(|e| {
            CliError::new(
                crate::error::ErrorCode::NotFound,
                format!("bench parse-serial: cannot read '{p}': {e}"),
            )
        })?,
        None => {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).map_err(|e| {
                CliError::new(
                    crate::error::ErrorCode::General,
                    format!("bench parse-serial: stdin read failed: {e}"),
                )
            })?;
            buf
        }
    };

    Ok(parse_serial_to_value(&input, tsc_mhz_override))
}

/// Pure string-in → Value-out. Exposed for unit tests.
pub(crate) fn parse_serial_to_value(log: &str, tsc_mhz_override: Option<u64>) -> Value {
    let mut phases: Vec<(String, u64)> = Vec::new();
    let mut tsc_mhz_from_log: Option<u64> = None;
    for line in log.lines() {
        if let Some((name, tsc)) = parse_phase_line(line) {
            phases.push((name, tsc));
        } else if let Some(mhz) = parse_tsc_mhz_line(line) {
            tsc_mhz_from_log = Some(mhz);
        }
    }

    let tsc_mhz_used = tsc_mhz_override.or(tsc_mhz_from_log);

    let mut phase_values: Vec<Value> = Vec::with_capacity(phases.len());
    let mut prev: Option<u64> = None;
    for (name, tsc) in &phases {
        let delta_cycles = prev.map_or(0u64, |p| tsc.saturating_sub(p));
        let mut entry = obj([
            ("phase", Value::str(name)),
            ("tsc", Value::UInt(*tsc)),
            ("delta_cycles", Value::UInt(delta_cycles)),
        ]);
        if let Some(mhz) = tsc_mhz_used {
            // cycles × (1 / MHz) = cycles × (1 / (cycles/μs)) = μs
            // multiply by 1000 for ns.
            let ns = (delta_cycles as f64) * 1000.0 / (mhz as f64);
            entry.insert("delta_ns", Value::Float(ns));
        }
        phase_values.push(entry);
        prev = Some(*tsc);
    }

    let mut out = obj([("phases", Value::Array(phase_values))]);
    if let Some(mhz) = tsc_mhz_used {
        out.insert("tsc_mhz", Value::UInt(mhz));
    }
    out
}

/// Finds `ZAMAK_PHASE=<name> tsc=<u64>` anywhere in the line.
/// Accepts log-framed lines (e.g. the `[ INFO]: file@line:` prefix
/// that `uefi_services`' logger adds).
fn parse_phase_line(line: &str) -> Option<(String, u64)> {
    let after_tag = line.split_once("ZAMAK_PHASE=")?.1;
    // Phase name goes up to the next whitespace.
    let (name, rest) = after_tag.split_once(char::is_whitespace)?;
    let tsc_part = rest.split_once("tsc=")?.1;
    // TSC digits: parse until whitespace / end.
    let digits: String = tsc_part
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let tsc: u64 = digits.parse().ok()?;
    Some((name.to_string(), tsc))
}

/// Finds `ZAMAK_TSC_MHZ=<n>` anywhere in the line (ignores
/// `ZAMAK_TSC_MHZ=unknown`).
fn parse_tsc_mhz_line(line: &str) -> Option<u64> {
    let rest = line.split_once("ZAMAK_TSC_MHZ=")?.1;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LOG: &str = "\
[ INFO]: zamak-uefi/src/main.rs@565: ZAMAK_TSC_MHZ=2400
[ INFO]: zamak-uefi/src/main.rs@542: ZAMAK_PHASE=uefi_entry tsc=1000000
[ INFO]: Zamak starting up (0.8.4)...
[ INFO]: zamak-uefi/src/main.rs@542: ZAMAK_PHASE=config_parsed tsc=1120000
[ INFO]: zamak-uefi/src/main.rs@542: ZAMAK_PHASE=pre_exit_boot_services tsc=1240000
";

    fn phase_at(v: &Value, idx: usize) -> &Vec<(String, Value)> {
        let Value::Object(root) = v else {
            panic!("expected object")
        };
        let phases = &root.iter().find(|(k, _)| k == "phases").unwrap().1;
        let Value::Array(items) = phases else {
            panic!("expected array")
        };
        let Value::Object(entry) = &items[idx] else {
            panic!("expected object")
        };
        entry
    }

    fn field<'a>(entry: &'a [(String, Value)], k: &str) -> Option<&'a Value> {
        entry.iter().find(|(key, _)| key == k).map(|(_, v)| v)
    }

    #[test]
    fn parses_phases_in_order() {
        let v = parse_serial_to_value(SAMPLE_LOG, None);
        let p0 = phase_at(&v, 0);
        assert!(matches!(field(p0, "phase"), Some(Value::Str(s)) if s == "uefi_entry"));
        assert!(matches!(field(p0, "tsc"), Some(Value::UInt(1000000))));
        assert!(matches!(field(p0, "delta_cycles"), Some(Value::UInt(0))));

        let p1 = phase_at(&v, 1);
        assert!(matches!(field(p1, "phase"), Some(Value::Str(s)) if s == "config_parsed"));
        assert!(matches!(
            field(p1, "delta_cycles"),
            Some(Value::UInt(120000))
        ));
    }

    #[test]
    fn log_tsc_mhz_enables_delta_ns() {
        let v = parse_serial_to_value(SAMPLE_LOG, None);
        let p1 = phase_at(&v, 1);
        let Some(Value::Float(ns)) = field(p1, "delta_ns") else {
            panic!("expected delta_ns float");
        };
        // 120000 cycles at 2400 MHz = 50_000 ns exactly.
        assert!((ns - 50000.0).abs() < 0.01);
    }

    #[test]
    fn explicit_tsc_mhz_overrides_log() {
        let v = parse_serial_to_value(SAMPLE_LOG, Some(1200));
        let p1 = phase_at(&v, 1);
        let Some(Value::Float(ns)) = field(p1, "delta_ns") else {
            panic!("expected delta_ns float");
        };
        // 120000 / 1200 MHz = 100 μs = 100_000 ns.
        assert!((ns - 100000.0).abs() < 0.01);
    }

    #[test]
    fn missing_tsc_mhz_omits_delta_ns() {
        let log = "\
[ INFO]: ZAMAK_PHASE=uefi_entry tsc=10
[ INFO]: ZAMAK_PHASE=config_parsed tsc=20
";
        let v = parse_serial_to_value(log, None);
        let p1 = phase_at(&v, 1);
        assert!(field(p1, "delta_ns").is_none());
        assert!(matches!(field(p1, "delta_cycles"), Some(Value::UInt(10))));
        // Envelope must NOT advertise a tsc_mhz if we don't know one.
        let Value::Object(root) = &v else { panic!() };
        assert!(!root.iter().any(|(k, _)| k == "tsc_mhz"));
    }

    #[test]
    fn unknown_tsc_mhz_line_is_ignored() {
        let log = "\
[ INFO]: ZAMAK_TSC_MHZ=unknown
[ INFO]: ZAMAK_PHASE=uefi_entry tsc=10
[ INFO]: ZAMAK_PHASE=config_parsed tsc=20
";
        let v = parse_serial_to_value(log, None);
        let Value::Object(root) = &v else { panic!() };
        // No tsc_mhz field, no delta_ns on phases.
        assert!(!root.iter().any(|(k, _)| k == "tsc_mhz"));
        assert!(field(phase_at(&v, 1), "delta_ns").is_none());
    }

    #[test]
    fn empty_input_produces_empty_phases() {
        let v = parse_serial_to_value("", Some(2400));
        let Value::Object(root) = &v else { panic!() };
        // tsc_mhz still present because explicit.
        assert!(matches!(
            root.iter().find(|(k, _)| k == "tsc_mhz").map(|(_, v)| v),
            Some(Value::UInt(2400)),
        ));
        let phases = &root.iter().find(|(k, _)| k == "phases").unwrap().1;
        assert!(matches!(phases, Value::Array(a) if a.is_empty()));
    }

    #[test]
    fn malformed_phase_line_is_skipped() {
        let log = "\
[ INFO]: ZAMAK_PHASE=good tsc=10
[ INFO]: ZAMAK_PHASE=bad tsc=not-a-number
[ INFO]: ZAMAK_PHASE=other_good tsc=30
";
        let v = parse_serial_to_value(log, None);
        let Value::Object(root) = &v else { panic!() };
        let phases = &root.iter().find(|(k, _)| k == "phases").unwrap().1;
        let Value::Array(items) = phases else {
            panic!()
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(
            field(phase_at(&v, 0), "phase"),
            Some(Value::Str(s)) if s == "good",
        ));
        assert!(matches!(
            field(phase_at(&v, 1), "phase"),
            Some(Value::Str(s)) if s == "other_good",
        ));
    }
}
