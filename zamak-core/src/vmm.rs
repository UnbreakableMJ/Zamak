// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Virtual Memory Manager (FR-MM-002).
//!
//! Architecture-agnostic planning for page-table construction: HHDM layout,
//! kernel PHDR placement, and framebuffer write-combining. Platform-specific
//! page-table materialization (x86 CR3, AArch64 TTBR, RISC-V satp) lives in
//! the corresponding `arch` module.

// Rust guideline compliant 2026-03-30

use alloc::vec::Vec;

/// 4 KiB standard page size.
pub const PAGE_SIZE: u64 = 4096;

/// 2 MiB huge page.
pub const HUGE_PAGE: u64 = 2 * 1024 * 1024;

/// 1 GiB gigantic page.
pub const GIGA_PAGE: u64 = 1 << 30;

/// Higher Half Direct Map base virtual address (Limine Protocol §HHDM).
pub const HHDM_VIRT_BASE: u64 = 0xFFFF_8000_0000_0000;

/// Kernel virtual base for the Limine Protocol (`kernel_address_request`).
pub const KERNEL_VIRT_BASE: u64 = 0xFFFF_FFFF_8000_0000;

/// Page caching policy (mapped to architecture-specific flag bits).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Normal writeback memory — fastest for RAM.
    WriteBack,
    /// Write-through — writes go to memory immediately.
    WriteThrough,
    /// Write-combining — weakly ordered, buffered writes. Used for framebuffers.
    WriteCombining,
    /// Uncacheable — used for MMIO.
    Uncacheable,
}

/// Page access permissions.
#[derive(Debug, Clone, Copy)]
pub struct Permissions {
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub user: bool,
}

impl Permissions {
    pub const KERNEL_CODE: Self = Self {
        readable: true,
        writable: false,
        executable: true,
        user: false,
    };
    pub const KERNEL_DATA: Self = Self {
        readable: true,
        writable: true,
        executable: false,
        user: false,
    };
    pub const KERNEL_RODATA: Self = Self {
        readable: true,
        writable: false,
        executable: false,
        user: false,
    };
    pub const MMIO: Self = Self {
        readable: true,
        writable: true,
        executable: false,
        user: false,
    };
    /// Coarse kernel-image mapping used by `zamak-uefi` during boot.
    ///
    /// Matches the x86-64 loader's historical behaviour of mapping the
    /// whole kernel range RWX at 4 KiB granule without per-PHDR
    /// subdivision. A future refactor can switch to per-PHDR
    /// `KERNEL_CODE` / `KERNEL_RODATA` / `KERNEL_DATA` mappings; until
    /// then this preset keeps the four arches byte-identical.
    pub const KERNEL_LOAD_AREA: Self = Self {
        readable: true,
        writable: true,
        executable: true,
        user: false,
    };
}

/// A VMM mapping directive: map a virtual range to a physical range.
#[derive(Debug, Clone, Copy)]
pub struct Mapping {
    pub virt_base: u64,
    pub phys_base: u64,
    pub length: u64,
    pub perms: Permissions,
    pub cache: CachePolicy,
}

impl Mapping {
    /// Number of pages this mapping covers at the given page size.
    pub fn page_count(&self, page_size: u64) -> u64 {
        self.length.div_ceil(page_size)
    }

    /// Returns `true` if this mapping can use a large-page size (2 MiB or 1 GiB).
    pub fn can_use_huge_pages(&self) -> bool {
        self.virt_base.is_multiple_of(HUGE_PAGE)
            && self.phys_base.is_multiple_of(HUGE_PAGE)
            && self.length >= HUGE_PAGE
    }

    pub fn can_use_giga_pages(&self) -> bool {
        self.virt_base.is_multiple_of(GIGA_PAGE)
            && self.phys_base.is_multiple_of(GIGA_PAGE)
            && self.length >= GIGA_PAGE
    }
}

// `u64::is_multiple_of` is provided by the standard library in Rust 1.87+ and
// is already used by `can_use_huge_pages`/`can_use_giga_pages` above.

/// A kernel program header that needs to be mapped.
#[derive(Debug, Clone, Copy)]
pub struct KernelPhdr {
    pub virt_addr: u64,
    pub phys_addr: u64,
    pub length: u64,
    pub perms: Permissions,
}

/// A framebuffer region requiring write-combining caching.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferRegion {
    pub phys_base: u64,
    pub length: u64,
}

/// A memory region descriptor from the PMM (just what VMM needs to map HHDM).
#[derive(Debug, Clone, Copy)]
pub struct HhdmRegion {
    pub phys_base: u64,
    pub length: u64,
}

/// Plans a full address-space layout: kernel PHDRs, HHDM, framebuffer.
///
/// Produces an ordered list of [`Mapping`]s that an arch-specific backend
/// materializes into page tables.
pub struct VmmPlan {
    pub mappings: Vec<Mapping>,
}

impl VmmPlan {
    /// Builds a plan from kernel PHDRs, HHDM regions, and framebuffers.
    pub fn build(
        kernel_phdrs: &[KernelPhdr],
        hhdm_regions: &[HhdmRegion],
        framebuffers: &[FramebufferRegion],
    ) -> Self {
        let mut mappings =
            Vec::with_capacity(kernel_phdrs.len() + hhdm_regions.len() + framebuffers.len());

        // Kernel PHDRs — at KERNEL_VIRT_BASE + phdr.virt_addr offset, preserved permissions.
        for phdr in kernel_phdrs {
            mappings.push(Mapping {
                virt_base: phdr.virt_addr,
                phys_base: phdr.phys_addr,
                length: phdr.length,
                perms: phdr.perms,
                cache: CachePolicy::WriteBack,
            });
        }

        // HHDM — map every physical region at HHDM_VIRT_BASE + phys.
        for region in hhdm_regions {
            mappings.push(Mapping {
                virt_base: HHDM_VIRT_BASE + region.phys_base,
                phys_base: region.phys_base,
                length: region.length,
                perms: Permissions::KERNEL_DATA,
                cache: CachePolicy::WriteBack,
            });
        }

        // Framebuffers — write-combining, HHDM-mapped.
        for fb in framebuffers {
            mappings.push(Mapping {
                virt_base: HHDM_VIRT_BASE + fb.phys_base,
                phys_base: fb.phys_base,
                length: fb.length,
                perms: Permissions::MMIO,
                cache: CachePolicy::WriteCombining,
            });
        }

        Self { mappings }
    }

    /// Returns the total bytes of virtual address space consumed.
    pub fn total_bytes(&self) -> u64 {
        self.mappings.iter().map(|m| m.length).sum()
    }

    /// Returns mappings that specifically need write-combining.
    pub fn write_combining_regions(&self) -> impl Iterator<Item = &Mapping> {
        self.mappings
            .iter()
            .filter(|m| m.cache == CachePolicy::WriteCombining)
    }
}

/// x86-64 PAT entry indices for mapping `CachePolicy` to hardware flags.
///
/// ZAMAK programs IA32_PAT so that PAT index 1 = WT, 2 = UC-, 3 = UC,
/// 4 = WB, 5 = WT, 6 = WC, 7 = UC.
pub mod x86_pat {
    use super::CachePolicy;

    /// Returns the (PCD, PWT, PAT) bit triple for a page-table entry on x86-64.
    ///
    /// With the PAT configured per the module docs, the triple selects the
    /// cache policy encoded by the `CachePolicy`.
    pub fn pte_flags(cache: CachePolicy) -> (bool, bool, bool) {
        match cache {
            // Entry 0: WB.
            CachePolicy::WriteBack => (false, false, false),
            // Entry 1: WT.
            CachePolicy::WriteThrough => (false, true, false),
            // Entry 6: WC (PAT=1, PCD=1, PWT=0).
            CachePolicy::WriteCombining => (true, false, true),
            // Entry 3: UC.
            CachePolicy::Uncacheable => (true, true, false),
        }
    }

    /// Packs the three bits into the standard x86-64 PTE positions.
    ///
    /// - PWT (bit 3)
    /// - PCD (bit 4)
    /// - PAT (bit 7 for 4 KiB pages; bit 12 for 2 MiB/1 GiB pages).
    pub fn pte_bits(cache: CachePolicy, is_huge: bool) -> u64 {
        let (pcd, pwt, pat) = pte_flags(cache);
        let mut bits: u64 = 0;
        if pwt {
            bits |= 1 << 3;
        }
        if pcd {
            bits |= 1 << 4;
        }
        if pat {
            bits |= if is_huge { 1 << 12 } else { 1 << 7 };
        }
        bits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_plan_includes_all_regions() {
        let phdrs = [KernelPhdr {
            virt_addr: KERNEL_VIRT_BASE,
            phys_addr: 0x100_0000,
            length: 0x10_0000,
            perms: Permissions::KERNEL_CODE,
        }];
        let hhdm = [HhdmRegion {
            phys_base: 0,
            length: 0x8000_0000,
        }];
        let fb = [FramebufferRegion {
            phys_base: 0xFD00_0000,
            length: 0x80_0000,
        }];

        let plan = VmmPlan::build(&phdrs, &hhdm, &fb);
        assert_eq!(plan.mappings.len(), 3);
        assert_eq!(plan.total_bytes(), 0x10_0000 + 0x8000_0000 + 0x80_0000);
    }

    #[test]
    fn framebuffer_uses_write_combining() {
        let fb = [FramebufferRegion {
            phys_base: 0xFD00_0000,
            length: 0x80_0000,
        }];
        let plan = VmmPlan::build(&[], &[], &fb);
        let wc: Vec<_> = plan.write_combining_regions().collect();
        assert_eq!(wc.len(), 1);
        assert_eq!(wc[0].cache, CachePolicy::WriteCombining);
    }

    #[test]
    fn huge_page_alignment_detection() {
        let huge_aligned = Mapping {
            virt_base: HHDM_VIRT_BASE,
            phys_base: 0,
            length: HUGE_PAGE * 2,
            perms: Permissions::KERNEL_DATA,
            cache: CachePolicy::WriteBack,
        };
        assert!(huge_aligned.can_use_huge_pages());

        let misaligned = Mapping {
            virt_base: HHDM_VIRT_BASE + 0x1000,
            phys_base: 0x1000,
            length: HUGE_PAGE * 2,
            perms: Permissions::KERNEL_DATA,
            cache: CachePolicy::WriteBack,
        };
        assert!(!misaligned.can_use_huge_pages());
    }

    #[test]
    fn giga_page_alignment_detection() {
        let gib_aligned = Mapping {
            virt_base: HHDM_VIRT_BASE,
            phys_base: 0,
            length: GIGA_PAGE * 2,
            perms: Permissions::KERNEL_DATA,
            cache: CachePolicy::WriteBack,
        };
        assert!(gib_aligned.can_use_giga_pages());
    }

    #[test]
    fn x86_pat_writeback() {
        let (pcd, pwt, pat) = x86_pat::pte_flags(CachePolicy::WriteBack);
        assert!(!pcd && !pwt && !pat);
    }

    #[test]
    fn x86_pat_write_combining_bits() {
        // WC: PCD=1, PWT=0, PAT=1.
        let (pcd, pwt, pat) = x86_pat::pte_flags(CachePolicy::WriteCombining);
        assert!(pcd);
        assert!(!pwt);
        assert!(pat);
    }

    #[test]
    fn x86_pat_huge_page_pat_bit_position() {
        // For huge pages, PAT bit is at position 12; for 4K pages, at position 7.
        let bits_4k = x86_pat::pte_bits(CachePolicy::WriteCombining, false);
        let bits_huge = x86_pat::pte_bits(CachePolicy::WriteCombining, true);
        assert_eq!(bits_4k & (1 << 7), 1 << 7);
        assert_eq!(bits_huge & (1 << 12), 1 << 12);
    }

    #[test]
    fn page_count_rounds_up() {
        let m = Mapping {
            virt_base: 0,
            phys_base: 0,
            length: 0x1001,
            perms: Permissions::KERNEL_DATA,
            cache: CachePolicy::WriteBack,
        };
        assert_eq!(m.page_count(PAGE_SIZE), 2);
    }
}
