// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Minimal bump allocator for the stage2 decompressor.
//!
//! Provides a [`GlobalAlloc`] implementation backed by a fixed-size
//! static buffer. The decompressor runs once and never frees memory,
//! making a bump allocator the simplest and fastest choice.
//!
//! The heap is placed in BSS (zero-initialized by the entry assembly).

// Rust guideline compliant 2026-03-30

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

/// Heap size for the decompressor.
///
/// 256 KiB is sufficient for `miniz_oxide` internal buffers plus the
/// decompressed output `Vec`. The typical compressed stage3 is under
/// 128 KiB decompressed.
const HEAP_SIZE: usize = 256 * 1024;

/// Alignment guarantee for all allocations.
const MAX_ALIGN: usize = 16;

#[repr(C, align(16))]
struct HeapBuffer {
    data: [u8; HEAP_SIZE],
}

static mut HEAP: HeapBuffer = HeapBuffer {
    data: [0; HEAP_SIZE],
};

/// Current allocation offset within the heap buffer.
static OFFSET: AtomicUsize = AtomicUsize::new(0);

/// Initializes the bump allocator.
///
/// Must be called once before any allocation. Resets the offset to
/// zero (idempotent if BSS is already zeroed).
pub fn init() {
    OFFSET.store(0, Ordering::Release);
}

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(MAX_ALIGN);
        let size = layout.size();

        loop {
            let current = OFFSET.load(Ordering::Acquire);
            // Align up.
            let aligned = (current + align - 1) & !(align - 1);
            let new_offset = match aligned.checked_add(size) {
                Some(v) => v,
                None => return core::ptr::null_mut(),
            };

            if new_offset > HEAP_SIZE {
                return core::ptr::null_mut();
            }

            if OFFSET
                .compare_exchange_weak(current, new_offset, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return unsafe { HEAP.data.as_mut_ptr().add(aligned) };
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator: deallocation is a no-op.
        // The decompressor runs once and jumps to stage3.
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;
