// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Per-arch page-table construction for ZAMAK UEFI (M4-1, M4-4, M6-1).
//!
//! Every sub-module exposes `build(boot_services, loaded_kernel) -> u64`
//! returning the **physical address of the root page table** that should
//! be installed by `handoff::jump_to_kernel` immediately after
//! `ExitBootServices`.
//!
//! On x86-64 the root is a PML4 written to CR3. On AArch64 it is the L0
//! table written to TTBR1_EL1. On RISC-V it is the Sv48 root page whose
//! PPN is encoded into SATP. On LoongArch it is the 4-level root written
//! to PGDH.
//!
//! All four builders share an identical shape: allocate a zeroed root
//! frame from UEFI `LOADER_DATA`, walk/install 4 KiB mappings for every
//! kernel PHDR, then map a direct HHDM covering all of physical memory
//! (huge pages where supported, 4 KiB fallback otherwise).
//!
//! The arch-neutral [`LoadedKernel`] struct is defined in `main.rs` and
//! re-imported here; we only need `phys_base`, `vaddr_start`, and `size`.

use uefi::prelude::*;
use uefi::table::boot::{AllocateType, MemoryType};

/// Common HHDM base used by every ZAMAK target (§FR-MM-002, §6.1).
/// Maps physical 0 to virtual `0xffff_8000_0000_0000`.
pub const HHDM_OFFSET: u64 = 0xffff_8000_0000_0000;

/// Arch-neutral 4 KiB frame allocation via UEFI `LOADER_DATA`.
/// Returns the base physical address of a fresh 4 KiB page, or `None`
/// if UEFI is out of memory. The page is NOT zeroed — callers that need
/// zeros (e.g. a new page-table root) must do that themselves.
pub fn alloc_4k_frame(boot_services: &BootServices) -> Option<u64> {
    boot_services
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1)
        .ok()
}

/// Walks the UEFI memory map and returns the highest physical byte
/// address reported by any entry. Used to size the HHDM.
pub fn max_phys_from_mmap(boot_services: &BootServices) -> u64 {
    let mmap_size = boot_services.memory_map_size();
    let buf_len = mmap_size
        .map_size
        .checked_add(1024)
        .expect("memory_map_size + 1024 overflowed usize");
    let mut buf = alloc::vec![0u8; buf_len];
    let mmap = boot_services
        .memory_map(&mut buf)
        .expect("Failed to get UEFI memory map");
    mmap.entries()
        .map(|d| {
            d.phys_start
                .checked_add(d.page_count.saturating_mul(4096))
                .unwrap_or(u64::MAX)
        })
        .max()
        .unwrap_or(4 * 1024 * 1024 * 1024)
}

// ---------------------------------------------------------------
// x86-64 — PML4 + 2 MiB HHDM via the `x86_64` crate's mapper API.
// ---------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
pub mod x86 {
    use super::*;
    use x86_64::structures::paging::{
        FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame,
        Size2MiB, Size4KiB,
    };
    use x86_64::{PhysAddr, VirtAddr};

    /// UEFI-backed frame allocator that the `x86_64` mapper API wants.
    struct UefiFrameAlloc<'a>(&'a BootServices);
    unsafe impl FrameAllocator<Size4KiB> for UefiFrameAlloc<'_> {
        fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
            let addr = super::alloc_4k_frame(self.0)?;
            Some(PhysFrame::containing_address(PhysAddr::new(addr)))
        }
    }

    pub fn build(boot_services: &BootServices, kernel: &crate::LoadedKernel) -> u64 {
        let mut alloc = UefiFrameAlloc(boot_services);

        let pml4_frame = <UefiFrameAlloc<'_> as FrameAllocator<Size4KiB>>::allocate_frame(&mut alloc)
            .expect("allocate PML4 frame");
        let pml4_ptr = pml4_frame.start_address().as_u64() as *mut PageTable;
        // SAFETY: frame is UEFI-allocated, page-aligned, owned here.
        unsafe {
            core::ptr::write_bytes(pml4_ptr, 0, 1);
        }
        let mut mapper = unsafe { OffsetPageTable::new(&mut *pml4_ptr, VirtAddr::new(0)) };

        // Map kernel PHDR range at 4 KiB granule.
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        let start_page: Page<Size4KiB> =
            Page::containing_address(VirtAddr::new(kernel.vaddr_start));
        let end_vaddr = kernel
            .vaddr_start
            .checked_add(kernel.size as u64)
            .and_then(|e| e.checked_sub(1))
            .expect("kernel vaddr range overflowed u64");
        let end_page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(end_vaddr));
        for page in Page::range_inclusive(start_page, end_page) {
            let offset = page
                .start_address()
                .as_u64()
                .checked_sub(kernel.vaddr_start)
                .expect("kernel page below vaddr_start");
            let frame_phys = kernel
                .phys_base
                .checked_add(offset)
                .expect("phys_base + offset overflowed");
            let frame = PhysFrame::containing_address(PhysAddr::new(frame_phys));
            unsafe {
                mapper
                    .map_to(page, frame, flags, &mut alloc)
                    .expect("map kernel page")
                    .flush();
            }
        }

        // Map HHDM covering all physical memory with 2 MiB huge pages.
        let max_phys = super::max_phys_from_mmap(boot_services);
        let huge: u64 = 2 * 1024 * 1024;
        let num_huge = max_phys.div_ceil(huge);
        let hhdm_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        for i in 0..num_huge {
            let pa = i.checked_mul(huge).expect("HHDM i*huge overflowed");
            let va = super::HHDM_OFFSET
                .checked_add(pa)
                .expect("HHDM va overflowed");
            let page: Page<Size2MiB> = Page::containing_address(VirtAddr::new(va));
            let frame = PhysFrame::containing_address(PhysAddr::new(pa));
            unsafe {
                mapper
                    .map_to(page, frame, hhdm_flags, &mut alloc)
                    .expect("map HHDM huge page")
                    .ignore();
            }
        }

        pml4_frame.start_address().as_u64()
    }
}

// ---------------------------------------------------------------
// AArch64 — 48-bit VA, L0/L1/L2/L3, 4 KiB granule via core builder.
// ---------------------------------------------------------------

#[cfg(target_arch = "aarch64")]
pub mod aarch64 {
    use super::*;
    use zamak_core::arch::aarch64::paging as arch_paging;
    use zamak_core::vmm::{CachePolicy, Mapping, Permissions, VmmPlan};

    struct FrameAlloc<'a>(&'a BootServices);
    impl arch_paging::FrameAllocator for FrameAlloc<'_> {
        fn alloc_frame(&mut self) -> Option<u64> {
            super::alloc_4k_frame(self.0)
        }
    }

    pub fn build(boot_services: &BootServices, kernel: &crate::LoadedKernel) -> u64 {
        let alloc = FrameAlloc(boot_services);
        // Because UEFI is identity-mapped at boot, any physical frame
        // can be accessed via its physical address as a writable slice.
        let phys_to_table = |pa: u64| -> &'static mut [u64; arch_paging::ENTRIES_PER_TABLE] {
            unsafe { &mut *(pa as *mut [u64; arch_paging::ENTRIES_PER_TABLE]) }
        };
        let mut builder = arch_paging::PageTableBuilder::new(alloc, phys_to_table)
            .expect("allocate aarch64 L0 root");

        let plan = build_plan(kernel, boot_services);
        builder.apply(&plan).expect("aarch64 page-table apply");
        builder.root()
    }

    fn build_plan(kernel: &crate::LoadedKernel, boot_services: &BootServices) -> VmmPlan {
        let mut mappings = alloc::vec::Vec::new();
        mappings.push(Mapping {
            virt_base: kernel.vaddr_start,
            phys_base: kernel.phys_base,
            length: kernel.size as u64,
            perms: Permissions::KERNEL_LOAD_AREA,
            cache: CachePolicy::WriteBack,
        });
        let max_phys = super::max_phys_from_mmap(boot_services);
        mappings.push(Mapping {
            virt_base: super::HHDM_OFFSET,
            phys_base: 0,
            length: max_phys,
            perms: Permissions::KERNEL_DATA,
            cache: CachePolicy::WriteBack,
        });
        VmmPlan { mappings }
    }
}

// ---------------------------------------------------------------
// RISC-V 64 — Sv48, 4-level via core builder.
// ---------------------------------------------------------------

#[cfg(target_arch = "riscv64")]
pub mod riscv64 {
    use super::*;
    use zamak_core::arch::riscv64::paging as arch_paging;
    use zamak_core::vmm::{CachePolicy, Mapping, Permissions, VmmPlan};

    struct FrameAlloc<'a>(&'a BootServices);
    impl arch_paging::FrameAllocator for FrameAlloc<'_> {
        fn alloc_frame(&mut self) -> Option<u64> {
            super::alloc_4k_frame(self.0)
        }
    }

    pub fn build(boot_services: &BootServices, kernel: &crate::LoadedKernel) -> u64 {
        let alloc = FrameAlloc(boot_services);
        let phys_to_table = |pa: u64| -> &'static mut [u64; arch_paging::ENTRIES_PER_TABLE] {
            unsafe { &mut *(pa as *mut [u64; arch_paging::ENTRIES_PER_TABLE]) }
        };
        let mut builder = arch_paging::PageTableBuilder::new(alloc, phys_to_table)
            .expect("allocate riscv64 Sv48 root");

        let plan = build_plan(kernel, boot_services);
        builder.apply(&plan).expect("riscv64 page-table apply");
        builder.root()
    }

    fn build_plan(kernel: &crate::LoadedKernel, boot_services: &BootServices) -> VmmPlan {
        let mut mappings = alloc::vec::Vec::new();
        mappings.push(Mapping {
            virt_base: kernel.vaddr_start,
            phys_base: kernel.phys_base,
            length: kernel.size as u64,
            perms: Permissions::KERNEL_LOAD_AREA,
            cache: CachePolicy::WriteBack,
        });
        let max_phys = super::max_phys_from_mmap(boot_services);
        mappings.push(Mapping {
            virt_base: super::HHDM_OFFSET,
            phys_base: 0,
            length: max_phys,
            perms: Permissions::KERNEL_DATA,
            cache: CachePolicy::WriteBack,
        });
        VmmPlan { mappings }
    }
}

// ---------------------------------------------------------------
// LoongArch64 — 4-level PGDH via core builder.
// ---------------------------------------------------------------

#[cfg(target_arch = "loongarch64")]
pub mod loongarch64 {
    use super::*;
    use zamak_core::arch::loongarch64::paging as arch_paging;
    use zamak_core::vmm::{CachePolicy, Mapping, Permissions, VmmPlan};

    struct FrameAlloc<'a>(&'a BootServices);
    impl arch_paging::FrameAllocator for FrameAlloc<'_> {
        fn alloc_frame(&mut self) -> Option<u64> {
            super::alloc_4k_frame(self.0)
        }
    }

    pub fn build(boot_services: &BootServices, kernel: &crate::LoadedKernel) -> u64 {
        let alloc = FrameAlloc(boot_services);
        let phys_to_table = |pa: u64| -> &'static mut [u64; arch_paging::ENTRIES_PER_TABLE] {
            unsafe { &mut *(pa as *mut [u64; arch_paging::ENTRIES_PER_TABLE]) }
        };
        let mut builder = arch_paging::PageTableBuilder::new(alloc, phys_to_table)
            .expect("allocate loongarch64 PGDH root");

        let plan = build_plan(kernel, boot_services);
        builder.apply(&plan).expect("loongarch64 page-table apply");
        builder.root()
    }

    fn build_plan(kernel: &crate::LoadedKernel, boot_services: &BootServices) -> VmmPlan {
        let mut mappings = alloc::vec::Vec::new();
        mappings.push(Mapping {
            virt_base: kernel.vaddr_start,
            phys_base: kernel.phys_base,
            length: kernel.size as u64,
            perms: Permissions::KERNEL_LOAD_AREA,
            cache: CachePolicy::WriteBack,
        });
        let max_phys = super::max_phys_from_mmap(boot_services);
        mappings.push(Mapping {
            virt_base: super::HHDM_OFFSET,
            phys_base: 0,
            length: max_phys,
            perms: Permissions::KERNEL_DATA,
            cache: CachePolicy::WriteBack,
        });
        VmmPlan { mappings }
    }
}

// ---------------------------------------------------------------
// Unified dispatch.
// ---------------------------------------------------------------

/// Build page tables for the current target arch and return the root
/// physical address (value to hand to `handoff::jump_to_kernel`).
#[inline]
pub fn build(boot_services: &BootServices, kernel: &crate::LoadedKernel) -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        return x86::build(boot_services, kernel);
    }
    #[cfg(target_arch = "aarch64")]
    {
        return aarch64::build(boot_services, kernel);
    }
    #[cfg(target_arch = "riscv64")]
    {
        return riscv64::build(boot_services, kernel);
    }
    #[cfg(target_arch = "loongarch64")]
    {
        return loongarch64::build(boot_services, kernel);
    }
    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "riscv64",
        target_arch = "loongarch64",
    )))]
    compile_error!("zamak-uefi: unsupported target_arch");
}
