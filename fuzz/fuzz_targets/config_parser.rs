// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Fuzzes `zamak-core::config::parse` with arbitrary input.
//!
//! Goal: the parser must never panic, abort, or produce out-of-bounds
//! accesses no matter what the kernel sees on disk (TEST-6, §8.1).
//!
//! Run via:
//!     cargo +nightly fuzz run config_parser -- -max_total_time=60

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = core::str::from_utf8(data) {
        let _ = zamak_core::config::parse(s);
    }
});
