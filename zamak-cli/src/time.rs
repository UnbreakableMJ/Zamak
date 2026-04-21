// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! ISO 8601 / UTC time helpers — no external deps.
//!
//! Steelbore Standard §2.3.1: timestamps MUST be ISO 8601 with
//! explicit `Z` suffix. `--local-time` is explicitly prohibited.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current UTC time as `YYYY-MM-DDThh:mm:ssZ`.
pub fn iso8601_now() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_iso8601(dur.as_secs())
}

/// Formats a Unix timestamp (seconds since epoch) as
/// `YYYY-MM-DDThh:mm:ssZ`.
pub fn format_iso8601(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let hh = tod / 3600;
    let mm = (tod % 3600) / 60;
    let ss = tod % 60;

    // Howard Hinnant's days_from_civil inverse.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!("{year:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_zero_is_unix_epoch() {
        assert_eq!(format_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn y2k_is_correct() {
        assert_eq!(format_iso8601(946_684_800), "2000-01-01T00:00:00Z");
    }

    #[test]
    fn leap_day_2024() {
        assert_eq!(format_iso8601(1_709_210_096), "2024-02-29T12:34:56Z");
    }

    #[test]
    fn now_ends_with_z() {
        let s = iso8601_now();
        assert!(s.ends_with('Z'), "{s} missing trailing Z");
        assert_eq!(s.len(), 20, "{s} wrong length");
    }
}
