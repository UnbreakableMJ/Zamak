// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Cross-checks the hand-rolled Limine v10.x reference parser used by
//! the `config_parser_differential` fuzz target against
//! `zamak_core::config::parse` on a curated corpus of golden inputs.
//!
//! This is not fuzzing — it's a smoke test that the reference model
//! itself stays valid when zamak's parser is touched. The real
//! differential campaign runs under `cargo +nightly fuzz run
//! config_parser_differential` in CI.

// Reference model mirrored from fuzz_targets/config_parser_differential.rs.
// Keep this in sync with that file by copy-paste so the two remain
// independently implemented relative to zamak's parser.

#[derive(Debug, Default, PartialEq, Eq)]
struct ReferenceConfig {
    entries: Vec<String>,
    timeout: Option<u64>,
    default_entry: Option<usize>,
}

fn parse_reference(input: &str) -> Option<ReferenceConfig> {
    let mut cfg = ReferenceConfig::default();
    let mut in_entry_scope = false;

    for raw in input.split('\n') {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if line.starts_with('/') {
            let depth = line.bytes().take_while(|&b| b == b'/').count();
            if depth > line.len() {
                return None;
            }
            let rest = &line[depth..];
            let rest = rest.strip_prefix('+').unwrap_or(rest);
            let name = rest.split(':').next().unwrap_or("").trim();
            if name.is_empty() || name.bytes().any(|b| b < 0x20 && b != b'\t') {
                return None;
            }
            if depth == 1 {
                cfg.entries.push(name.to_string());
                in_entry_scope = true;
            } else if depth == 2 || depth == 3 {
                in_entry_scope = true;
            } else {
                return None;
            }
            continue;
        }

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
            continue;
        }
    }
    Some(cfg)
}

fn assert_same(input: &str) {
    let Some(reference) = parse_reference(input) else {
        panic!("reference model rejected golden input:\n{input}");
    };
    let zamak = zamak_core::config::parse(input);
    let zamak_names: Vec<String> =
        zamak.entries.iter().map(|e| e.name.clone()).collect();
    assert_eq!(
        zamak_names, reference.entries,
        "entry-name differential on input:\n{input}"
    );
    if let Some(t) = reference.timeout {
        assert_eq!(zamak.timeout, t, "timeout differential on:\n{input}");
    }
    if let Some(d) = reference.default_entry {
        assert_eq!(
            zamak.default_entry, d,
            "default_entry differential on:\n{input}"
        );
    }
}

#[test]
fn empty_input_parses_to_no_entries() {
    let zamak = zamak_core::config::parse("");
    let reference = parse_reference("").unwrap();
    assert!(zamak.entries.is_empty());
    assert!(reference.entries.is_empty());
}

#[test]
fn one_entry_matches() {
    assert_same("/Linux\n");
}

#[test]
fn multiple_entries_match_in_order() {
    assert_same("/First\n/Second\n/Third\n");
}

#[test]
fn comments_do_not_create_entries() {
    assert_same("# pure comment\n/Real\n# trailing\n");
}

#[test]
fn blank_lines_are_ignored() {
    assert_same("\n\n/A\n\n\n/B\n\n");
}

#[test]
fn top_level_timeout_is_captured() {
    assert_same("timeout=10\n/Linux\n");
}

#[test]
fn default_entry_is_captured() {
    assert_same("default_entry=2\n/A\n/B\n/C\n");
}

#[test]
fn expand_prefix_preserves_entry_name() {
    // `/+Directory` opens a directory entry with the `+` stripped.
    assert_same("/+Distros\n");
}

#[test]
fn crlf_line_endings_work() {
    assert_same("/Foo\r\n/Bar\r\n");
}

#[test]
fn reference_rejects_depth_four() {
    // `////Name` is a depth-4 header which the clean-subset doesn't
    // accept. The reference model must return `None` so the fuzz
    // harness skips the comparison.
    assert!(parse_reference("////TooDeep\n").is_none());
}

#[test]
fn reference_rejects_empty_entry_name() {
    assert!(parse_reference("/\n").is_none());
}
