// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Page table setup for the BIOS boot path.
//!
//! Builds a 4-level x86-64 page table hierarchy using 2 MiB huge pages:
//! - Identity maps the first 1 GiB (PML4[0])
//! - Maps the kernel at 0xFFFF_FFFF_8000_0000 (PML4[511], PDPT[510])
//! - Maps full HHDM at 0xFFFF_8000_0000_0000 covering all physical memory (§FR-MM-002)

// Rust guideline compliant 2026-03-30

use alloc::alloc::{alloc, Layout};
use x86_64::PhysAddr;
use zamak_core::protocol::{MemmapEntry, MEMMAP_USABLE};

/// Page table entry flags: Present + Writable.
const PTE_PRESENT_WRITABLE: u64 = 0x3;

/// Page table entry flags: Present + Writable + Huge (2 MiB).
const PTE_HUGE: u64 = 0x83;

/// Number of 2 MiB pages to identity-map (512 = 1 GiB).
const IDENTITY_MAP_PAGES: isize = 512;

/// Number of 2 MiB pages for the kernel mapping (64 = 128 MiB).
const KERNEL_MAP_PAGES: isize = 64;

/// 2 MiB huge page size in bytes.
const HUGE_PAGE_SIZE: u64 = 2 * 1024 * 1024;

/// 1 GiB in bytes (one PDPT entry covers this with 512 PD entries of 2 MiB each).
const GIB: u64 = 1024 * 1024 * 1024;

/// HHDM virtual base address (Limine protocol standard).
const HHDM_VIRT_BASE: u64 = 0xFFFF_8000_0000_0000;

/// PML4 index for the HHDM base address (bits 47:39).
const HHDM_PML4_START: isize = ((HHDM_VIRT_BASE >> 39) & 0x1FF) as isize; // 256

/// Hard ceiling for the HHDM mapping. QEMU reports a high reserved
/// MMIO region around 1 TiB which would force allocating ~1024
/// PD-level pages — exhausting our 4 MiB bump heap. The HHDM only
/// needs to reach RAM, not MMIO; capping at 16 GiB keeps the page
/// table footprint under 80 KiB while comfortably covering every
/// realistic boot-smoke configuration.
const HHDM_MAX_BYTES: u64 = 16 * GIB;

/// Computes the highest *usable* physical address from the memory
/// map. Reserved / ACPI / MMIO entries are ignored so a high MMIO
/// hole doesn't blow up the HHDM page-table allocation. The result is
/// clamped to [`HHDM_MAX_BYTES`] for the same reason.
fn max_physical_address(mmap: &[MemmapEntry]) -> u64 {
    let mut max: u64 = 0;
    for e in mmap {
        if e.typ != MEMMAP_USABLE {
            continue;
        }
        let end = e.base.wrapping_add(e.length);
        if end > max {
            max = end;
        }
    }
    if max == 0 {
        return GIB;
    }
    if max > HHDM_MAX_BYTES {
        return HHDM_MAX_BYTES;
    }
    max
}

/// Sets up 4-level page tables for the transition to long mode.
///
/// The `mmap` parameter is used to determine how much physical memory
/// to cover in the HHDM region. All memory reported by E820 will be
/// accessible through the HHDM at `0xFFFF_8000_0000_0000 + phys_addr`.
///
/// Returns the physical address of the PML4 table, suitable for
/// loading into CR3.
pub fn setup_paging(
    kernel_phys_base: u64,
    _kernel_vaddr_start: u64,
    _kernel_size: usize,
    mmap: &[MemmapEntry],
) -> PhysAddr {
    let pml4 = allocate_page();

    // 1. Identity map first 1 GiB (PML4[0]).
    let pdpt_ident = allocate_page();
    // SAFETY:
    //   Preconditions: pml4 is a valid, zeroed, 4 KiB-aligned page from allocate_page()
    //   Postconditions: PML4[0] points to pdpt_ident with Present+Writable
    //   Clobbers: memory at pml4[0]
    //   Worst-case: invalid page table entry causes page fault on first memory access
    unsafe {
        *pml4.offset(0) = (pdpt_ident as u64) | PTE_PRESENT_WRITABLE;
    }

    let pd_ident = allocate_page();
    // SAFETY: pdpt_ident is valid from allocate_page(); sets PDPT[0] -> pd_ident
    unsafe {
        *pdpt_ident.offset(0) = (pd_ident as u64) | PTE_PRESENT_WRITABLE;
    }

    for i in 0..IDENTITY_MAP_PAGES {
        let addr = i as u64 * HUGE_PAGE_SIZE;
        // SAFETY: pd_ident has 512 entries; i < 512; maps 2 MiB huge page
        unsafe {
            *pd_ident.offset(i) = addr | PTE_HUGE;
        }
    }

    // 2. Map kernel at 0xFFFF_FFFF_8000_0000 (PML4[511], PDPT[510]).
    let pdpt_kernel = allocate_page();
    // SAFETY: pml4 valid; PML4[511] -> kernel PDPT
    unsafe {
        *pml4.offset(511) = (pdpt_kernel as u64) | PTE_PRESENT_WRITABLE;
    }

    let pd_kernel = allocate_page();
    // SAFETY: pdpt_kernel valid; PDPT[510] -> kernel PD
    unsafe {
        *pdpt_kernel.offset(510) = (pd_kernel as u64) | PTE_PRESENT_WRITABLE;
    }

    for i in 0..KERNEL_MAP_PAGES {
        let phys = kernel_phys_base + (i as u64 * HUGE_PAGE_SIZE);
        // SAFETY: pd_kernel has 512 entries; i < 64; maps kernel 2 MiB pages
        unsafe {
            *pd_kernel.offset(i) = phys | PTE_HUGE;
        }
    }

    // 3. Map HHDM at 0xFFFF_8000_0000_0000 covering all physical memory (§FR-MM-002).
    //
    // Each PML4 entry covers 512 GiB. Each PDPT entry covers 1 GiB.
    // Each PD entry covers 2 MiB. We use 2 MiB huge pages throughout.
    //
    // Layout: PML4[256..] -> PDPT[0..N] -> PD[0..512] -> 2 MiB huge pages.
    let max_phys = max_physical_address(mmap);
    let total_gib = max_phys.wrapping_add(GIB - 1) / GIB; // Round up to GiB.
    let total_pdpt_entries = total_gib as isize;

    // Each PML4 entry covers 512 PDPT entries (512 GiB).
    let pml4_entries_needed = ((total_pdpt_entries as isize) + 511) / 512;

    // Allocate and wire PML4 -> PDPT -> PD levels.
    let mut pdpt_index = 0isize;
    for pml4_i in 0..pml4_entries_needed {
        let pdpt = allocate_page();
        let pml4_slot = HHDM_PML4_START + pml4_i;
        // SAFETY: pml4_slot is within valid PML4 range (256..511);
        //         pdpt is a valid zeroed page
        unsafe {
            *pml4.offset(pml4_slot) = (pdpt as u64) | PTE_PRESENT_WRITABLE;
        }

        let pdpt_count_this = core::cmp::min(512, total_pdpt_entries - (pml4_i * 512));
        for pdpt_j in 0..pdpt_count_this {
            let pd = allocate_page();
            // SAFETY: pdpt_j < 512; pd is a valid zeroed page
            unsafe {
                *pdpt.offset(pdpt_j) = (pd as u64) | PTE_PRESENT_WRITABLE;
            }

            // Fill PD with 2 MiB huge page mappings.
            let base_phys = pdpt_index as u64 * GIB;
            let pages_in_pd = if base_phys + GIB <= max_phys {
                512isize
            } else {
                // Partial last GiB: only map up to max_phys.
                ((max_phys - base_phys + HUGE_PAGE_SIZE - 1) / HUGE_PAGE_SIZE) as isize
            };

            for pd_k in 0..pages_in_pd {
                let phys = base_phys + pd_k as u64 * HUGE_PAGE_SIZE;
                // SAFETY: pd_k < 512; maps a 2 MiB huge page at the correct physical address
                unsafe {
                    *pd.offset(pd_k) = phys | PTE_HUGE;
                }
            }

            pdpt_index += 1;
        }
    }

    PhysAddr::new(pml4 as u64)
}

/// Allocates a zeroed, 4 KiB-aligned page for use as a page table.
fn allocate_page() -> *mut u64 {
    let layout = Layout::from_size_align(4096, 4096).expect("page layout is valid");

    // SAFETY:
    //   Preconditions: layout is non-zero size with valid alignment
    //   Postconditions: returns a 4 KiB-aligned, allocated block (or null)
    //   Clobbers: none
    //   Worst-case: null pointer if allocator is exhausted (handled below)
    let ptr = unsafe { alloc(layout) as *mut u64 };
    if ptr.is_null() {
        panic!("out of memory during paging setup");
    }

    // SAFETY: ptr is non-null and points to 4096 bytes; zeroing 512 u64s = 4096 bytes
    unsafe {
        core::ptr::write_bytes(ptr, 0, 512);
    }
    ptr
}
