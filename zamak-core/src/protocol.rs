// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Limine Protocol types — re-exported from [`zamak_proto`].
//!
//! This module re-exports all protocol types from the standalone
//! `zamak-proto` crate and adds the `scan_requests` function that
//! requires `alloc`.

// Rust guideline compliant 2026-03-30

pub use zamak_proto::*;

use alloc::vec::Vec;

/// Scan a kernel image byte slice for Limine protocol requests.
///
/// Returns mutable pointers to each [`RawRequest`] found between the
/// start and end marker boundaries.
pub fn scan_requests(kernel_bytes: &[u8]) -> Vec<*mut RawRequest> {
    let mut requests = Vec::new();

    // SAFETY:
    //   Preconditions:
    //     - START_MARKER, END_MARKER, COMMON_MAGIC are valid static arrays
    //   Postconditions:
    //     - Returns byte-slice views of the marker arrays for comparison
    //   Clobbers:
    //     - None
    //   Worst-case on violation:
    //     - Incorrect marker comparison; requests not found
    let start_bytes =
        unsafe { core::slice::from_raw_parts(START_MARKER.as_ptr().cast::<u8>(), 32) };
    let end_bytes = unsafe { core::slice::from_raw_parts(END_MARKER.as_ptr().cast::<u8>(), 16) };
    let common_magic_bytes =
        unsafe { core::slice::from_raw_parts(COMMON_MAGIC.as_ptr().cast::<u8>(), 16) };

    let mut i: usize = 0;
    while i
        .checked_add(32)
        .is_some_and(|end| end <= kernel_bytes.len())
    {
        if kernel_bytes[i..i + 32] == *start_bytes {
            i += 32;
            while i
                .checked_add(32)
                .is_some_and(|end| end <= kernel_bytes.len())
            {
                if kernel_bytes[i..i + 16] == *end_bytes {
                    return requests;
                }

                if kernel_bytes[i..i + 16] == *common_magic_bytes {
                    requests.push(kernel_bytes[i..].as_ptr() as *mut RawRequest);
                }
                i += 8;
            }
        }
        i += 8;
    }

    requests
}
