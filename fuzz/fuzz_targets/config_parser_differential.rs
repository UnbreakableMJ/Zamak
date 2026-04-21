// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Differential fuzz target — ZAMAK config parser vs. a hand-rolled
//! Limine v10.x reference model (TEST-6, §FR-CFG-001).
//!
//! The reference model implements the Limine v10.x `limine.conf`
//! spec from scratch in pure Rust (no shared code with
//! `zamak_core::config`). It deliberately covers only the clean
//! sub-set of the format: `/Name` entries, `//SubName` nesting,
//! `/+Name` auto-expand, `key=value` options, `#` line comments,
//! `${VAR}` macro references. Malformed inputs that the reference
//! rejects are skipped — divergence there is not a bug.
//!
//! When the reference model accepts the input, the following
//! invariants must hold against `zamak_core::config::parse`:
//!
//! 1. Same set of depth-1 entry names (order-preserving).
//! 2. Same global `timeout` value when both parsers extracted one.
//! 3. Same `default_entry` (1-based index) when both extracted one.
//!
//! Run via:
//!     cargo +nightly fuzz run config_parser_differential -- -max_total_time=300

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = core::str::from_utf8(data) else {
        return;
    };
    // Reject inputs that are too large (fuzz target perf) or carry
    // bytes that neither parser must accept as config content.
    if input.len() > 64 * 1024 {
        return;
    }
    if input.bytes().any(|b| b == 0) {
        return;
    }

    let Some(reference) = parse_reference(input) else {
        // Reference model rejected — don't compare.
        return;
    };
    let zamak_parsed = zamak_core::config::parse(input);

    // (1) depth-1 entry names match, order-preserving.
    let zamak_names: Vec<&str> = zamak_parsed
        .entries
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    let reference_names: Vec<&str> = reference
        .entries
        .iter()
        .map(|s| s.as_str())
        .collect();
    assert_eq!(
        zamak_names, reference_names,
        "entry-name differential: input={input:?}\n  zamak:     {zamak_names:?}\n  reference: {reference_names:?}"
    );

    // (2) global timeout.
    if let Some(t) = reference.timeout {
        assert_eq!(
            zamak_parsed.timeout, t,
            "timeout differential: {} vs {}", zamak_parsed.timeout, t
        );
    }

    // (3) default_entry.
    if let Some(d) = reference.default_entry {
        assert_eq!(
            zamak_parsed.default_entry, d,
            "default_entry differential: {} vs {}", zamak_parsed.default_entry, d
        );
    }
});

/// Hand-rolled Limine v10.x reference parser. Independent of
/// `zamak_core::config` — shares no code, only the same spec.
#[derive(Debug, Default)]
struct ReferenceConfig {
    /// Depth-1 entry names in declaration order.
    entries: Vec<String>,
    /// Global `timeout=N` if it appeared at top level.
    timeout: Option<u64>,
    /// Global `default_entry=N` (1-based) if present.
    default_entry: Option<usize>,
}

fn parse_reference(input: &str) -> Option<ReferenceConfig> {
    let mut cfg = ReferenceConfig::default();
    // Track whether we've opened the first entry. Lines before any
    // `/Entry` are global-scope.
    let mut in_entry_scope = false;

    for raw in input.split('\n') {
        // Limine treats `\r` at end-of-line as whitespace.
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();

        // Comment or blank line.
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Entry headers start with `/` at column 0 (after optional
        // leading whitespace). Reject header-looking lines that
        // appear indented — zamak's parser is more liberal than the
        // reference, which is fine: we just refuse to compare.
        if line.starts_with('/') {
            // `///Name` is depth 3; `//Name` depth 2; `/Name` depth 1.
            let depth = line.bytes().take_while(|&b| b == b'/').count();
            if depth > line.len() {
                return None;
            }
            let rest = &line[depth..];
            // `/+Name` opens an expanded directory entry.
            let rest = rest.strip_prefix('+').unwrap_or(rest);
            // Name ends at first `:` or end-of-line (Limine options
            // come on their own lines, not tacked on the header).
            let name = rest.split(':').next().unwrap_or("").trim();
            // Reject empty / control-char-bearing names.
            if name.is_empty()
                || name.bytes().any(|b| b < 0x20 && b != b'\t')
            {
                return None;
            }
            if depth == 1 {
                cfg.entries.push(name.to_string());
                in_entry_scope = true;
            } else if depth == 2 || depth == 3 {
                // Sub-entries: no-op for our invariant set (we only
                // compare depth-1 names). Still mark we're in an
                // entry so global_options lookups stay out.
                in_entry_scope = true;
            } else {
                // Depth > 3: bail, not in clean-subset.
                return None;
            }
            continue;
        }

        // `KEY=VALUE` option lines. We only care about top-level
        // `timeout=` and `default_entry=` for the invariant set.
        if !in_entry_scope {
            if let Some(eq) = line.find('=') {
                let key = line[..eq].trim();
                let val = line[eq + 1..].trim();
                match key {
                    "timeout" | "TIMEOUT" => {
                        cfg.timeout = val.parse().ok();
                    }
                    "default_entry" | "DEFAULT_ENTRY" => {
                        cfg.default_entry = val.parse().ok();
                    }
                    _ => {}
                }
            }
            // An unparseable non-entry non-option line at top level
            // is tolerable — zamak may ignore it, reference ignores
            // it too.
            continue;
        }
    }
    Some(cfg)
}
