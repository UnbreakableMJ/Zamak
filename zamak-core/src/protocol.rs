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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a kernel-image byte slice containing: leading noise,
    /// START_MARKER, one request whose first 16 bytes are
    /// COMMON_MAGIC, END_MARKER, trailing noise.
    fn mk_image_with_one_request() -> alloc::vec::Vec<u8> {
        let start = unsafe {
            core::slice::from_raw_parts(START_MARKER.as_ptr().cast::<u8>(), 32)
        };
        let end = unsafe {
            core::slice::from_raw_parts(END_MARKER.as_ptr().cast::<u8>(), 16)
        };
        let magic = unsafe {
            core::slice::from_raw_parts(COMMON_MAGIC.as_ptr().cast::<u8>(), 16)
        };

        let mut img = alloc::vec::Vec::new();
        img.extend_from_slice(&[0xAAu8; 64]); // noise
        img.extend_from_slice(start);         // start marker
        img.extend_from_slice(magic);         // common magic
        img.extend_from_slice(&[0u8; 16]);    // request id continuation
        img.extend_from_slice(end);           // end marker
        img.extend_from_slice(&[0xFFu8; 64]); // noise
        img
    }

    #[test]
    fn scan_requests_finds_request_between_markers() {
        let img = mk_image_with_one_request();
        let reqs = scan_requests(&img);
        assert_eq!(reqs.len(), 1, "expected 1 request, got {}", reqs.len());
    }

    #[test]
    fn scan_requests_returns_empty_on_empty_slice() {
        let reqs = scan_requests(&[]);
        assert!(reqs.is_empty());
    }

    #[test]
    fn scan_requests_returns_empty_when_no_start_marker() {
        let reqs = scan_requests(&[0u8; 512]);
        assert!(reqs.is_empty());
    }

    #[test]
    fn scan_requests_stops_at_end_marker() {
        // Same shape as mk_image_with_one_request but with NO common
        // magic between the markers — should return zero requests.
        let start = unsafe {
            core::slice::from_raw_parts(START_MARKER.as_ptr().cast::<u8>(), 32)
        };
        let end = unsafe {
            core::slice::from_raw_parts(END_MARKER.as_ptr().cast::<u8>(), 16)
        };
        let mut img = alloc::vec::Vec::new();
        img.extend_from_slice(&[0u8; 32]);
        img.extend_from_slice(start);
        img.extend_from_slice(&[0u8; 32]); // padding, not a magic
        img.extend_from_slice(end);
        let reqs = scan_requests(&img);
        assert!(reqs.is_empty());
    }

    #[test]
    fn scan_requests_ignores_truncated_tail() {
        // Truncate to less than 32 bytes — function must not panic.
        let reqs = scan_requests(&[0u8; 8]);
        assert!(reqs.is_empty());
    }
}
