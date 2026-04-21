// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Fuzzes the Multiboot 1 header scanner and parser.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(offset) = zamak_core::multiboot::find_header(data) {
        let _ = zamak_core::multiboot::parse_header(data, offset);
    }
});
