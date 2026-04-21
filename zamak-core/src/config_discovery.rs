// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Configuration discovery and search order (FR-CFG-004, FR-CFG-005).
//!
//! Implements the config search priority:
//! 1. SMBIOS Type 11 OEM Strings (prefixed with `limine:config:`)
//! 2. UEFI application directory (same partition as the bootloader)
//! 3. Standard paths on the boot volume
//!
//! Also provides SMBIOS Type 11 OEM String extraction for injecting
//! config fragments from firmware.

// Rust guideline compliant 2026-03-30

use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[cfg(test)]
use alloc::vec;

/// Prefix that identifies a Limine config line in SMBIOS OEM Strings.
const SMBIOS_CONFIG_PREFIX: &str = "limine:config:";

/// Standard config file search paths, checked in order.
pub const STANDARD_PATHS: &[&str] = &[
    "/zamak.conf",
    "/limine.conf",
    "/boot/zamak.conf",
    "/boot/limine.conf",
    "/boot/limine/limine.conf",
    "/EFI/BOOT/zamak.conf",
    "/EFI/BOOT/limine.conf",
];

/// SMBIOS entry point signature: "_SM_" (32-bit). Used by platform code that
/// locates the SMBIOS table before passing its contents here.
pub const SMBIOS_32_ANCHOR: &[u8; 4] = b"_SM_";

/// SMBIOS entry point signature: "_SM3_" (64-bit). Used by platform code that
/// locates the SMBIOS 3.x table before passing its contents here.
pub const SMBIOS_64_ANCHOR: &[u8; 5] = b"_SM3_";

/// SMBIOS Type 11 structure header type value.
const SMBIOS_TYPE_OEM_STRINGS: u8 = 11;

/// Extracts config lines from SMBIOS Type 11 OEM Strings.
///
/// Scans the SMBIOS structure table for Type 11 entries and returns
/// all strings that start with `limine:config:`, with the prefix stripped.
///
/// The `smbios_table` parameter should point to the SMBIOS structure table
/// data (not the entry point). `table_len` is the total byte length.
pub fn extract_smbios_config(smbios_table: &[u8]) -> Vec<String> {
    let mut config_lines = Vec::new();
    let mut offset = 0;

    while offset + 4 <= smbios_table.len() {
        let struct_type = smbios_table[offset];
        let struct_len = smbios_table[offset + 1] as usize;

        if struct_len < 4 {
            break; // Invalid structure length.
        }

        // End-of-table marker (Type 127).
        if struct_type == 127 {
            break;
        }

        // The formatted area ends at offset + struct_len.
        // The unformatted string area follows immediately after.
        let string_area_start = offset + struct_len;

        // Parse the string area: NUL-terminated strings, double-NUL ends the section.
        let strings = parse_smbios_strings(&smbios_table[string_area_start..]);

        if struct_type == SMBIOS_TYPE_OEM_STRINGS {
            for s in &strings {
                if let Some(config_line) = s.strip_prefix(SMBIOS_CONFIG_PREFIX) {
                    config_lines.push(config_line.to_string());
                }
            }
        }

        // Advance past the formatted area + string area (including the double-NUL).
        offset = string_area_start + strings_section_len(&smbios_table[string_area_start..]);
    }

    config_lines
}

/// Parses NUL-terminated strings from an SMBIOS unformatted area.
fn parse_smbios_strings(data: &[u8]) -> Vec<String> {
    let mut strings = Vec::new();
    let mut start = 0;

    for i in 0..data.len() {
        if data[i] == 0 {
            if i == start {
                // Double NUL — end of string area.
                break;
            }
            if let Ok(s) = core::str::from_utf8(&data[start..i]) {
                strings.push(s.to_string());
            }
            start = i + 1;
        }
    }

    strings
}

/// Returns the byte length of the SMBIOS string section (including the double-NUL terminator).
fn strings_section_len(data: &[u8]) -> usize {
    for i in 0..data.len().saturating_sub(1) {
        if data[i] == 0 && data[i + 1] == 0 {
            return i + 2;
        }
    }
    // No double-NUL found — treat the rest as the section.
    data.len()
}

/// Combines SMBIOS config lines (if any) with file-based config.
///
/// SMBIOS lines are prepended to the file config, allowing firmware
/// to inject global options or override settings.
pub fn merge_smbios_config(smbios_lines: &[String], file_config: &str) -> String {
    if smbios_lines.is_empty() {
        return file_config.to_string();
    }

    let mut merged = String::new();
    merged.push_str("# SMBIOS OEM String injected config\n");
    for line in smbios_lines {
        merged.push_str(line);
        merged.push('\n');
    }
    merged.push('\n');
    merged.push_str(file_config);
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_smbios_oem_strings() {
        // Simulate a Type 11 structure:
        // Header: type=11, length=5, handle=0x0000, count=2
        // Strings: "limine:config:TIMEOUT=10\0" "other string\0\0"
        let mut data = Vec::new();
        // Formatted area: type(1) + length(1) + handle(2) + count(1) = 5 bytes.
        data.push(11); // Type 11
        data.push(5); // Length
        data.extend_from_slice(&[0x00, 0x00]); // Handle
        data.push(2); // String count

        // Unformatted string area:
        data.extend_from_slice(b"limine:config:TIMEOUT=10\0");
        data.extend_from_slice(b"other string\0");
        data.push(0); // Double-NUL terminator.

        // End-of-table marker.
        data.push(127); // Type 127
        data.push(4); // Length
        data.extend_from_slice(&[0xFF, 0xFF]); // Handle
        data.push(0); // Double-NUL.
        data.push(0);

        let lines = extract_smbios_config(&data);
        assert_eq!(lines, vec!["TIMEOUT=10"]);
    }

    #[test]
    fn merge_empty_smbios() {
        let merged = merge_smbios_config(&[], "TIMEOUT=5");
        assert_eq!(merged, "TIMEOUT=5");
    }

    #[test]
    fn merge_with_smbios() {
        let smbios = vec!["TIMEOUT=10".to_string()];
        let merged = merge_smbios_config(&smbios, ":My Entry\nPROTOCOL=limine");
        assert!(merged.contains("TIMEOUT=10"));
        assert!(merged.contains(":My Entry"));
    }
}
