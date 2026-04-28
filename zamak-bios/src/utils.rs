// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Freestanding `mem*` intrinsics required by LLVM-generated calls.
//!
//! LLVM lowers slice/array initialization, large-struct moves, and
//! `core::ptr::{copy, write_bytes}` to `memcpy` / `memset` calls
//! whenever the size isn't a small constant. The `compiler-builtins`
//! crate ships fallback implementations, but on the i686-zamak custom
//! target the implementations LLVM picked up were *the very same
//! Rust functions LLVM had just lowered the calls to* — i.e. they
//! recursed into themselves and hung. Replacing them with hand-written
//! `rep stos` / `rep movs` blocks breaks the cycle (the inline asm
//! contains no high-level Rust the compiler could re-lower into a
//! mem-builtin call).

// Rust guideline compliant 2026-03-30

use core::arch::asm;

/// SAFETY:
///   Preconditions:
///     - `s..s.add(n)` is a valid writable region of length `n` bytes.
///     - DF is clear (Rust ABI invariant; cleared in `_start`).
///   Postconditions:
///     - Every byte in `s..s.add(n)` is set to `c as u8`.
///   Clobbers:
///     - EDI, ECX consumed by `rep stosb` semantics.
///   Worst-case on violation:
///     - Out-of-bounds write corrupts memory or triple-faults.
#[no_mangle]
pub unsafe extern "C" fn memset(s: *mut u8, c: i32, n: usize) -> *mut u8 {
    let dst = s;
    let val = c as u8;
    let count = n;
    asm!(
        "rep stosb",
        inout("edi") dst => _,
        inout("ecx") count => _,
        in("al") val,
        options(nostack, preserves_flags),
    );
    s
}

/// SAFETY: see [`memset`]. Source/destination must not overlap; for
/// overlapping moves use [`memmove`].
#[no_mangle]
pub unsafe extern "C" fn memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    let dst = dest;
    let count = n;
    // LLVM reserves `esi` for its own use, so we route the source
    // pointer through a free general-purpose register and move it
    // into ESI inside the asm block (saving/restoring around the
    // `rep movsb`).
    asm!(
        "push esi",
        "mov esi, {src}",
        "rep movsb",
        "pop esi",
        src = in(reg) src,
        inout("edi") dst => _,
        inout("ecx") count => _,
        options(preserves_flags),
    );
    dest
}

/// Byte-wise comparison; the loop's branches keep LLVM from lowering
/// it back into a `memcmp` libcall (which would recurse).
#[no_mangle]
pub unsafe extern "C" fn memcmp(s1: *const u8, s2: *const u8, n: usize) -> i32 {
    let mut i = 0;
    while i < n {
        let a = *s1.add(i);
        let b = *s2.add(i);
        if a != b {
            return a as i32 - b as i32;
        }
        i += 1;
    }
    0
}

/// Overlapping move. Copies forward when `dest < src` (the safe
/// direction), backward otherwise. Both directions go through `rep
/// movsb` with DF set/cleared explicitly so we don't depend on the
/// caller's flag state.
#[no_mangle]
pub unsafe extern "C" fn memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    if n == 0 {
        return dest;
    }
    if (dest as usize) < (src as usize) {
        let dst = dest;
        let count = n;
        asm!(
            "push esi",
            "mov esi, {src}",
            "cld",
            "rep movsb",
            "pop esi",
            src = in(reg) src,
            inout("edi") dst => _,
            inout("ecx") count => _,
            options(preserves_flags),
        );
    } else {
        let dst_end = dest.add(n - 1);
        let src_end = src.add(n - 1);
        let count = n;
        asm!(
            "push esi",
            "mov esi, {src_end}",
            "std",
            "rep movsb",
            "cld",
            "pop esi",
            src_end = in(reg) src_end,
            inout("edi") dst_end => _,
            inout("ecx") count => _,
        );
    }
    dest
}
