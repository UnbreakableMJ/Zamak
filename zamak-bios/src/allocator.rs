// SPDX-License-Identifier: GPL-3.0-or-later

use core::alloc::{GlobalAlloc, Layout};
use core::ptr::null_mut;
use core::sync::atomic::{AtomicUsize, Ordering};

pub struct BumpAllocator {
    heap_start: usize,
    heap_size: usize,
    next: AtomicUsize,
}

impl BumpAllocator {
    pub const fn new(heap_start: usize, heap_size: usize) -> Self {
        Self {
            heap_start,
            heap_size,
            next: AtomicUsize::new(0),
        }
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();
        
        loop {
            let current = self.next.load(Ordering::Relaxed);
            let start = (self.heap_start + current + align - 1) & !(align - 1);
            let end = start + size;
            
            if end > self.heap_start + self.heap_size {
                return null_mut();
            }
            
            if self.next.compare_exchange(current, end - self.heap_start, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                return start as *mut u8;
            }
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator doesn't deallocate
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator::new(0x100000, 0x400000); // 4MB heap at 1MB
