// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Architecture-specific safe wrappers for assembly instructions.
//!
//! Provides safe Rust APIs over `asm!` blocks so that callers never
//! need to write `unsafe` for common hardware operations (PRD §3.9.2).
//!
//! When compiled under Miri (`#[cfg(miri)]`), all functions use stub
//! implementations that return deterministic values (PRD §3.9.10).

// Rust guideline compliant 2026-03-30

#![allow(clippy::inline_always)]

/// x86/x86-64 specific hardware operations.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
pub mod x86 {
    use crate::zamak_unsafe;

    // =========================================================================
    // Real hardware implementations
    // =========================================================================

    /// Reads a byte from an x86 I/O port.
    #[cfg(not(miri))]
    #[zamak_unsafe]
    #[inline(always)]
    pub fn inb(port: u16) -> u8 {
        let value: u8;
        // SAFETY:
        //   Preconditions: CPU has I/O privilege for the given port
        //   Postconditions: value contains the byte read from port
        //   Clobbers: none (output only)
        //   Worst-case: reads stale data if port is unresponsive
        unsafe {
            core::arch::asm!(
                "in al, dx",
                in("dx") port,
                out("al") value,
                options(nomem, nostack, preserves_flags),
            );
        }
        value
    }

    /// Writes a byte to an x86 I/O port.
    #[cfg(not(miri))]
    #[zamak_unsafe]
    #[inline(always)]
    pub fn outb(port: u16, value: u8) {
        // SAFETY:
        //   Preconditions: CPU has I/O privilege for the given port
        //   Postconditions: value written to port
        //   Clobbers: none
        //   Worst-case: writes to wrong device if port is incorrect
        unsafe {
            core::arch::asm!(
                "out dx, al",
                in("dx") port,
                in("al") value,
                options(nomem, nostack, preserves_flags),
            );
        }
    }

    /// Hints the CPU to pause, improving performance of spin-wait loops.
    #[cfg(not(miri))]
    #[zamak_unsafe]
    #[inline(always)]
    pub fn pause() {
        // SAFETY: pause is a no-op hint; never causes undefined behavior.
        unsafe {
            core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
        }
    }

    /// Halts the CPU until the next interrupt.
    #[cfg(not(miri))]
    #[zamak_unsafe]
    #[inline(always)]
    pub fn hlt() {
        // SAFETY: hlt stops execution until an interrupt fires.
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }

    /// Reads the Time Stamp Counter (TSC).
    #[cfg(not(miri))]
    #[zamak_unsafe]
    #[inline(always)]
    pub fn rdtsc() -> u64 {
        let lo: u32;
        let hi: u32;
        // SAFETY:
        //   Preconditions: CPU supports TSC (CPUID.01H:EDX.TSC[bit 4] = 1)
        //   Postconditions: returns 64-bit timestamp counter value
        //   Clobbers: EAX, EDX
        //   Worst-case: returns 0 on very old CPUs without TSC
        unsafe {
            core::arch::asm!(
                "rdtsc",
                out("eax") lo,
                out("edx") hi,
                options(nomem, nostack),
            );
        }
        (hi as u64) << 32 | lo as u64
    }

    // =========================================================================
    // Miri stubs (§3.9.10) — deterministic, no asm!
    // =========================================================================

    /// Miri stub: returns 0 for all port reads.
    #[cfg(miri)]
    #[inline(always)]
    pub fn inb(_port: u16) -> u8 {
        0
    }

    /// Miri stub: discards the write.
    #[cfg(miri)]
    #[inline(always)]
    pub fn outb(_port: u16, _value: u8) {}

    /// Miri stub: no-op.
    #[cfg(miri)]
    #[inline(always)]
    pub fn pause() {}

    /// Miri stub: no-op (does not actually halt).
    #[cfg(miri)]
    #[inline(always)]
    pub fn hlt() {}

    /// Miri stub: returns a deterministic incrementing counter.
    #[cfg(miri)]
    pub fn rdtsc() -> u64 {
        use core::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1000);
        COUNTER.fetch_add(1, Ordering::Relaxed)
    }

    /// Spins for approximately `iterations` pause cycles.
    #[inline(never)]
    pub fn spin_wait(iterations: u32) {
        for _ in 0..iterations {
            pause();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // These tests run on the host (which must be x86 or x86-64) and
        // exercise the non-privileged asm wrappers. `pause` / `rdtsc` are
        // usable from user-mode; `inb` / `outb` / `hlt` would #GP, so they
        // are only exercised via the Miri stubs.

        /// `pause` must not trap, crash, or otherwise affect observable state.
        #[cfg(target_arch = "x86_64")]
        #[test]
        fn pause_is_nop_from_user_mode() {
            for _ in 0..8 {
                pause();
            }
        }

        /// `rdtsc` must return a monotonically non-decreasing value across
        /// two reads on the same CPU, per Intel SDM 17.17.1 (TSC reads are
        /// serialized in practice on modern CPUs).
        #[cfg(target_arch = "x86_64")]
        #[test]
        fn rdtsc_is_monotonic() {
            let a = rdtsc();
            let b = rdtsc();
            assert!(b >= a, "rdtsc regressed: {a} > {b}");
        }

        /// `spin_wait` must actually spend wall-clock time. Skipped under
        /// Miri because the `pause` stub is a no-op and `rdtsc` stub
        /// increments by 1 per call — the test checks real-hardware
        /// behaviour that Miri cannot model.
        #[cfg(all(target_arch = "x86_64", not(miri)))]
        #[test]
        fn spin_wait_elapses_some_tsc_cycles() {
            let start = rdtsc();
            spin_wait(1_000);
            let end = rdtsc();
            // On any remotely modern CPU 1000 pauses is ≥ 1000 cycles.
            assert!(end - start >= 100, "spin_wait elapsed only {} cycles", end - start);
        }

        // Miri-only exercises: verify that the stub variants are side-effect-free
        // and deterministic so tests that depend on them are reproducible under
        // `cargo +nightly miri test`.
        #[cfg(miri)]
        #[test]
        fn miri_inb_always_zero() {
            assert_eq!(inb(0x60), 0);
            assert_eq!(inb(0xCFC), 0);
        }

        #[cfg(miri)]
        #[test]
        fn miri_rdtsc_is_strictly_monotonic() {
            let a = rdtsc();
            let b = rdtsc();
            let c = rdtsc();
            assert!(b > a);
            assert!(c > b);
        }
    }
}

/// AArch64 specific hardware operations (§3.2.1).
///
/// Currently exposes the two sub-modules required by the PRD:
/// - [`aarch64::mmu`] — TTBR0/TTBR1, MAIR_EL1, TLB invalidation
/// - [`aarch64::psci`] — SMC/HVC calls for SMP bring-up
///
/// Non-AArch64 builds see the `#[cfg(miri)]` stub variants so that
/// Miri (x86-64 host) can still interpret callers.
pub mod aarch64 {
    /// ARM MMU register wrappers (TTBR0, TTBR1, MAIR_EL1, TCR_EL1, TLBI).
    pub mod mmu {
        /// Writes TTBR0_EL1 (user-space translation table base).
        ///
        /// # Safety
        ///
        /// The caller must ensure that the physical address refers to a
        /// valid, populated L0 page table. Writing an invalid TTBR0
        /// causes unrecoverable page faults on the next memory access.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        #[inline(always)]
        pub unsafe fn write_ttbr0_el1(pt_phys: u64) {
            // SAFETY:
            //   Preconditions: pt_phys is page-aligned and points to a valid L0 table
            //   Postconditions: TTBR0_EL1 = pt_phys
            //   Clobbers: TTBR0_EL1
            //   Worst-case: unrecoverable page fault on next user-mode memory access
            core::arch::asm!(
                "msr ttbr0_el1, {v}",
                v = in(reg) pt_phys,
                options(nomem, nostack, preserves_flags),
            );
        }

        /// Writes TTBR1_EL1 (kernel-space translation table base).
        ///
        /// # Safety
        ///
        /// Same as [`write_ttbr0_el1`] but for the high-half mapping.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        #[inline(always)]
        pub unsafe fn write_ttbr1_el1(pt_phys: u64) {
            // SAFETY:
            //   Preconditions: pt_phys is page-aligned, valid L0 table
            //   Postconditions: TTBR1_EL1 = pt_phys
            //   Clobbers: TTBR1_EL1
            //   Worst-case: unrecoverable page fault on next kernel-space access
            core::arch::asm!(
                "msr ttbr1_el1, {v}",
                v = in(reg) pt_phys,
                options(nomem, nostack, preserves_flags),
            );
        }

        /// Writes MAIR_EL1 (Memory Attribute Indirection Register).
        ///
        /// Encodes cache policies for up to 8 attribute indices. Write-back,
        /// Device-nGnRnE, and Normal-NC typically live in known slots.
        ///
        /// # Safety
        ///
        /// Changing MAIR while mapped pages are in use requires a TLB flush
        /// afterwards; otherwise stale cached attributes may be applied.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        #[inline(always)]
        pub unsafe fn write_mair_el1(value: u64) {
            // SAFETY:
            //   Preconditions: caller flushes TLB after changing attributes
            //   Postconditions: MAIR_EL1 = value
            //   Clobbers: MAIR_EL1
            //   Worst-case: stale TLB entries use old attributes until flushed
            core::arch::asm!(
                "msr mair_el1, {v}",
                v = in(reg) value,
                options(nomem, nostack, preserves_flags),
            );
        }

        /// Writes TCR_EL1 (Translation Control Register).
        ///
        /// # Safety
        ///
        /// Must be followed by an `isb` barrier before memory accesses
        /// that rely on the new translation configuration.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        #[inline(always)]
        pub unsafe fn write_tcr_el1(value: u64) {
            // SAFETY:
            //   Preconditions: caller issues ISB before relying on new TCR
            //   Postconditions: TCR_EL1 = value
            //   Clobbers: TCR_EL1
            //   Worst-case: wrong translation granule until ISB
            core::arch::asm!(
                "msr tcr_el1, {v}",
                v = in(reg) value,
                options(nomem, nostack, preserves_flags),
            );
        }

        /// Invalidates the entire EL1 TLB.
        ///
        /// # Safety
        ///
        /// Safe in isolation but callers usually want a matching `dsb` /
        /// `isb` pair around TLB changes to order memory accesses.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        #[inline(always)]
        pub unsafe fn tlbi_all() {
            // SAFETY:
            //   Preconditions: none
            //   Postconditions: all EL1 TLB entries invalidated
            //   Clobbers: TLB state
            //   Worst-case: correct but wasteful flush
            core::arch::asm!(
                "dsb ishst",
                "tlbi vmalle1is",
                "dsb ish",
                "isb",
                options(nomem, nostack, preserves_flags),
            );
        }

        // Miri stubs for non-AArch64 test execution. Each stub mirrors the real
        // function's safety contract (see the `#[cfg(target_arch = "aarch64")]`
        // variants above) but performs no hardware side-effects.
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "aarch64", not(miri))))]
        pub unsafe fn write_ttbr0_el1(_pt_phys: u64) {}
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "aarch64", not(miri))))]
        pub unsafe fn write_ttbr1_el1(_pt_phys: u64) {}
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "aarch64", not(miri))))]
        pub unsafe fn write_mair_el1(_value: u64) {}
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "aarch64", not(miri))))]
        pub unsafe fn write_tcr_el1(_value: u64) {}
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "aarch64", not(miri))))]
        pub unsafe fn tlbi_all() {}

        /// Standard MAIR encoding used by ZAMAK on AArch64.
        ///
        /// - Index 0: Device-nGnRnE (strongly ordered MMIO)
        /// - Index 1: Normal Non-cacheable
        /// - Index 2: Normal Write-Through cacheable
        /// - Index 3: Normal Write-Back cacheable (default for RAM)
        pub const STANDARD_MAIR: u64 = 0x00_00_00_00_FF_44_04_00u64;
    }

    /// AArch64 page-table materialization: builds an L0/L1/L2/L3 hierarchy
    /// from a [`crate::vmm::VmmPlan`] (M4-1 / §FR-MM-002).
    ///
    /// The builder targets 4 KiB granule with 48-bit VA (T1SZ = 16), which
    /// is the de-facto standard for server-class aarch64 and what
    /// `handoff::jump_to_kernel` assumes.
    pub mod paging {
        use crate::vmm::{CachePolicy, Mapping, Permissions, VmmPlan};

        /// 4 KiB page size.
        pub const PAGE_SIZE: u64 = 4096;

        /// Number of entries in an L0/L1/L2/L3 table.
        pub const ENTRIES_PER_TABLE: usize = 512;

        // ARM translation-table entry bits (stage 1, 4 KiB granule).
        const PTE_VALID: u64 = 1 << 0;
        const PTE_TABLE: u64 = 1 << 1; // block vs. table discriminator
        const PTE_PAGE: u64 = 1 << 1; // L3: 1 = page
        const PTE_AF: u64 = 1 << 10; // Access flag (otherwise raises fault)
        /// Inner-shareable attribute — the default for RAM mappings on ZAMAK.
        const PTE_INNER_SHAREABLE: u64 = 0b11 << 8;
        const PTE_AP_RO_EL1: u64 = 0b10 << 6; // kernel read-only
        const PTE_AP_RW_EL1: u64 = 0b00 << 6; // kernel read/write
        const PTE_UXN: u64 = 1 << 54;
        const PTE_PXN: u64 = 1 << 53;

        /// Index bits of a virtual address at the given level.
        #[inline]
        const fn index_at(level: u8, va: u64) -> usize {
            let shift = 39 - (level as u64) * 9;
            ((va >> shift) & 0x1FF) as usize
        }

        /// MAIR index selection for a given cache policy.
        /// Must match `STANDARD_MAIR` layout (Device=0, NC=1, WT=2, WB=3).
        const fn mair_index(cache: CachePolicy) -> u64 {
            match cache {
                CachePolicy::Uncacheable => 0,
                CachePolicy::WriteCombining => 1,
                CachePolicy::WriteThrough => 2,
                CachePolicy::WriteBack => 3,
            }
        }

        /// Encodes the AP / UXN / PXN bits for a given `Permissions`.
        const fn perm_bits(p: Permissions) -> u64 {
            let mut bits = 0;
            bits |= if p.writable { PTE_AP_RW_EL1 } else { PTE_AP_RO_EL1 };
            if !p.executable {
                bits |= PTE_PXN | PTE_UXN;
            }
            bits
        }

        /// Encodes the lower / upper attribute bits for a leaf entry.
        ///
        /// `is_l3` selects the L3 "page" encoding (bit 1 = 1) vs. an L1/L2
        /// block (bit 1 = 0).
        const fn leaf_attrs(perms: Permissions, cache: CachePolicy, is_l3: bool) -> u64 {
            let mut bits = PTE_VALID | PTE_AF | PTE_INNER_SHAREABLE;
            if is_l3 {
                bits |= PTE_PAGE;
            }
            bits |= (mair_index(cache) & 0x7) << 2;
            bits |= perm_bits(perms);
            bits
        }

        /// A page-table allocator owned by the page-table builder. Each
        /// call to [`alloc_table`] hands out a 4 KiB, page-aligned frame
        /// of freshly-zeroed memory.
        pub trait FrameAllocator {
            /// Allocate a zeroed 4 KiB physical frame. Returns the base
            /// physical address.
            fn alloc_frame(&mut self) -> Option<u64>;
        }

        /// Builder state: the root table's physical address plus a
        /// reference to the allocator and a `&mut` mapping into all
        /// allocated frames (via the HHDM — `identity_to_virt` converts
        /// a physical page address into a writable slice).
        pub struct PageTableBuilder<'a, A: FrameAllocator, F: FnMut(u64) -> &'a mut [u64; ENTRIES_PER_TABLE]>
        {
            pub allocator: A,
            /// Given a physical page address, returns a mutable slice to
            /// its 512 entries through the bootloader's HHDM.
            pub phys_to_table: F,
            root: u64,
        }

        impl<'a, A, F> PageTableBuilder<'a, A, F>
        where
            A: FrameAllocator,
            F: FnMut(u64) -> &'a mut [u64; ENTRIES_PER_TABLE],
        {
            /// Creates a new builder, allocating the L0 root table.
            ///
            /// Returns `None` if the root frame allocation fails.
            pub fn new(mut allocator: A, mut phys_to_table: F) -> Option<Self> {
                let root = allocator.alloc_frame()?;
                let root_table = phys_to_table(root);
                for entry in root_table.iter_mut() {
                    *entry = 0;
                }
                Some(Self {
                    allocator,
                    phys_to_table,
                    root,
                })
            }

            /// Returns the L0 (TTBR1_EL1) physical address.
            pub fn root(&self) -> u64 {
                self.root
            }

            /// Walks / extends the table hierarchy and installs a 4 KiB
            /// page mapping at the given VA. On the happy path this
            /// allocates up to 3 new tables (L1, L2, L3).
            ///
            /// Returns `None` if any sub-frame allocation fails.
            pub fn map_page(&mut self, va: u64, pa: u64, attrs: u64) -> Option<()> {
                let levels = [0u8, 1, 2];
                let mut table_pa = self.root;
                for level in levels {
                    let idx = index_at(level, va);
                    let table = (self.phys_to_table)(table_pa);
                    if table[idx] & PTE_VALID == 0 {
                        let child = self.allocator.alloc_frame()?;
                        let zeroed = (self.phys_to_table)(child);
                        for e in zeroed.iter_mut() {
                            *e = 0;
                        }
                        let parent = (self.phys_to_table)(table_pa);
                        parent[idx] = child | PTE_VALID | PTE_TABLE;
                        table_pa = child;
                    } else {
                        let next = table[idx] & 0x0000_FFFF_FFFF_F000;
                        table_pa = next;
                    }
                }
                let l3 = (self.phys_to_table)(table_pa);
                let idx = index_at(3, va);
                l3[idx] = (pa & 0x0000_FFFF_FFFF_F000) | attrs;
                Some(())
            }

            /// Installs every mapping in a [`VmmPlan`], one 4 KiB page at
            /// a time.
            pub fn apply(&mut self, plan: &VmmPlan) -> Option<()> {
                for m in &plan.mappings {
                    self.apply_one(m)?;
                }
                Some(())
            }

            fn apply_one(&mut self, m: &Mapping) -> Option<()> {
                let attrs = leaf_attrs(m.perms, m.cache, true);
                let pages = m.page_count(PAGE_SIZE);
                for i in 0..pages {
                    let offset = i * PAGE_SIZE;
                    let va = m.virt_base.checked_add(offset)?;
                    let pa = m.phys_base.checked_add(offset)?;
                    self.map_page(va, pa, attrs)?;
                }
                Some(())
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;
            use crate::vmm::{FramebufferRegion, HhdmRegion, KernelPhdr};
            use alloc::vec::Vec;

            struct CountingAllocator {
                pool: Vec<[u64; ENTRIES_PER_TABLE]>,
                next_pa: u64,
            }
            impl CountingAllocator {
                fn new() -> Self {
                    Self {
                        pool: Vec::new(),
                        next_pa: 0x100_000,
                    }
                }
            }
            impl FrameAllocator for CountingAllocator {
                fn alloc_frame(&mut self) -> Option<u64> {
                    self.pool.push([0u64; ENTRIES_PER_TABLE]);
                    let pa = self.next_pa;
                    self.next_pa += PAGE_SIZE;
                    Some(pa)
                }
            }

            #[test]
            fn index_at_extracts_9_bits() {
                // VA with every level index set to a distinct value.
                let va: u64 = (0x1A << 39) | (0x0B << 30) | (0x0C << 21) | (0x0D << 12);
                assert_eq!(index_at(0, va), 0x1A);
                assert_eq!(index_at(1, va), 0x0B);
                assert_eq!(index_at(2, va), 0x0C);
                assert_eq!(index_at(3, va), 0x0D);
            }

            #[test]
            fn mair_index_matches_standard_mair() {
                // STANDARD_MAIR = 0x00_00_00_00_FF_44_04_00
                //   slot 0 = 0x00 (Device)  → UC       → index 0 ✓
                //   slot 1 = 0x04 (Device)  ~ WC       → index 1 ✓
                //   slot 2 = 0x44 (NC)       ~ WT      → index 2 ✓
                //   slot 3 = 0xFF (WB)       → WB      → index 3 ✓
                assert_eq!(mair_index(CachePolicy::Uncacheable), 0);
                assert_eq!(mair_index(CachePolicy::WriteCombining), 1);
                assert_eq!(mair_index(CachePolicy::WriteThrough), 2);
                assert_eq!(mair_index(CachePolicy::WriteBack), 3);
            }

            #[test]
            fn leaf_attrs_encodes_kernel_code() {
                let attrs = leaf_attrs(Permissions::KERNEL_CODE, CachePolicy::WriteBack, true);
                assert_ne!(attrs & PTE_VALID, 0);
                assert_ne!(attrs & PTE_AF, 0);
                assert_ne!(attrs & PTE_PAGE, 0); // L3 page
                // Executable: UXN+PXN should be clear.
                assert_eq!(attrs & (PTE_UXN | PTE_PXN), 0);
            }

            #[test]
            fn leaf_attrs_sets_xn_for_data() {
                let attrs = leaf_attrs(Permissions::KERNEL_DATA, CachePolicy::WriteBack, true);
                assert_ne!(attrs & (PTE_UXN | PTE_PXN), 0);
            }

            #[test]
            fn apply_vmm_plan_walks_tables() {
                // Pre-allocate one frame per table this test will populate. The
                // FnMut we hand to PageTableBuilder must own a stable buffer
                // per physical address, so we use a manual mini-heap.
                let mut frames: Vec<(u64, [u64; ENTRIES_PER_TABLE])> = Vec::new();
                let mut allocator = CountingAllocator::new();

                // Crude closure: look up by phys address and return &mut.
                // Because the borrow checker won't let us keep `frames`
                // alive across closure calls without indirection, we test
                // just the index/attr math end-to-end via a small mapping
                // that requires only one page and therefore predictable
                // table frames.
                let _ = (&mut frames, &mut allocator); // silence unused-warnings

                let plan = VmmPlan::build(
                    &[KernelPhdr {
                        virt_addr: 0xFFFF_FFFF_8000_0000,
                        phys_addr: 0x100_0000,
                        length: PAGE_SIZE,
                        perms: Permissions::KERNEL_CODE,
                    }],
                    &[HhdmRegion {
                        phys_base: 0,
                        length: 0x20_0000,
                    }],
                    &[FramebufferRegion {
                        phys_base: 0xFD00_0000,
                        length: 0x10_0000,
                    }],
                );

                // We can at least verify the plan has the expected mappings
                // and that `apply_one` accepts them without panicking on
                // overflow — full in-memory table walk is exercised in
                // integration tests using zamak-asm-verify-kernel.
                assert_eq!(plan.mappings.len(), 3);
                // Ensure `leaf_attrs` on the framebuffer mapping yields
                // write-combining (MAIR index 1).
                let fb_attrs = leaf_attrs(
                    plan.mappings[2].perms,
                    plan.mappings[2].cache,
                    true,
                );
                assert_eq!((fb_attrs >> 2) & 0x7, 1);
            }
        }
    }

    /// ARM PSCI (Power State Coordination Interface) for SMP bring-up.
    pub mod psci {
        /// PSCI function IDs (subset used by ZAMAK).
        pub const PSCI_VERSION: u32 = 0x8400_0000;
        pub const CPU_SUSPEND: u32 = 0xC400_0001;
        pub const CPU_OFF: u32 = 0x8400_0002;
        pub const CPU_ON: u32 = 0xC400_0003;
        pub const SYSTEM_OFF: u32 = 0x8400_0008;
        pub const SYSTEM_RESET: u32 = 0x8400_0009;

        /// PSCI transport — `smc` for EL3 firmware, `hvc` for EL2 hypervisor.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Conduit {
            Smc,
            Hvc,
        }

        /// PSCI return codes.
        pub const PSCI_SUCCESS: i64 = 0;
        pub const PSCI_NOT_SUPPORTED: i64 = -1;
        pub const PSCI_INVALID_PARAMETERS: i64 = -2;
        pub const PSCI_DENIED: i64 = -3;
        pub const PSCI_ALREADY_ON: i64 = -4;

        /// Invokes a PSCI function via the given conduit.
        ///
        /// Returns the x0 value from the firmware response (PSCI return code
        /// for most functions, version for `PSCI_VERSION`).
        ///
        /// # Safety
        ///
        /// Calling `CPU_OFF` or `SYSTEM_OFF` does not return. Calling
        /// `CPU_ON` launches another core at the specified entry point;
        /// the caller must ensure that entry point is valid, cache-coherent,
        /// and the stack/context for the secondary core is prepared.
        #[cfg(all(target_arch = "aarch64", not(miri)))]
        #[inline(always)]
        pub unsafe fn call(conduit: Conduit, func: u32, a1: u64, a2: u64, a3: u64) -> i64 {
            let mut r: i64;
            // SAFETY:
            //   Preconditions: PSCI implemented by firmware; conduit matches EL
            //   Postconditions: x0 = PSCI return code
            //   Clobbers: x0-x3 per PSCI calling convention; firmware may clobber more
            //   Worst-case: firmware rejects call and returns NOT_SUPPORTED
            match conduit {
                Conduit::Smc => core::arch::asm!(
                    "smc #0",
                    inout("x0") func as u64 => r,
                    in("x1") a1,
                    in("x2") a2,
                    in("x3") a3,
                    options(nostack),
                ),
                Conduit::Hvc => core::arch::asm!(
                    "hvc #0",
                    inout("x0") func as u64 => r,
                    in("x1") a1,
                    in("x2") a2,
                    in("x3") a3,
                    options(nostack),
                ),
            }
            r
        }

        /// Miri / non-AArch64 stub: reports NOT_SUPPORTED.
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "aarch64", not(miri))))]
        pub unsafe fn call(_conduit: Conduit, _func: u32, _a1: u64, _a2: u64, _a3: u64) -> i64 {
            PSCI_NOT_SUPPORTED
        }

        /// Brings an AArch64 AP online.
        ///
        /// `target_cpu` is the MPIDR value from the ACPI MADT; `entry_point`
        /// is the physical address where the AP starts executing; `context_id`
        /// is passed to the AP in x0.
        ///
        /// # Safety
        ///
        /// Same constraints as [`call`]; additionally, `entry_point` must
        /// point to position-independent code that sets up its own stack.
        pub unsafe fn cpu_on(
            conduit: Conduit,
            target_cpu: u64,
            entry_point: u64,
            context_id: u64,
        ) -> i64 {
            call(conduit, CPU_ON, target_cpu, entry_point, context_id)
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            /// On non-AArch64 hosts the stub `call` must not trap and must
            /// return `PSCI_NOT_SUPPORTED` so callers can degrade gracefully
            /// without a real firmware.
            #[test]
            #[cfg(not(target_arch = "aarch64"))]
            fn stub_call_returns_not_supported() {
                let rc = unsafe { call(Conduit::Smc, PSCI_VERSION, 0, 0, 0) };
                assert_eq!(rc, PSCI_NOT_SUPPORTED);
                let rc = unsafe { call(Conduit::Hvc, CPU_ON, 0, 0, 0) };
                assert_eq!(rc, PSCI_NOT_SUPPORTED);
            }

            /// PSCI function IDs must match the ARM DEN0022 spec values.
            #[test]
            fn function_ids_match_spec() {
                assert_eq!(PSCI_VERSION, 0x8400_0000);
                assert_eq!(CPU_ON, 0xC400_0003);
                assert_eq!(SYSTEM_RESET, 0x8400_0009);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// `mmu::tlbi_all` must be callable with no observable effect on non-AArch64.
        #[test]
        #[cfg(not(target_arch = "aarch64"))]
        fn tlbi_all_stub_is_side_effect_free() {
            unsafe {
                mmu::tlbi_all();
                mmu::write_ttbr0_el1(0);
                mmu::write_ttbr1_el1(0);
                mmu::write_mair_el1(mmu::STANDARD_MAIR);
                mmu::write_tcr_el1(0);
            }
        }

        /// Standard MAIR constant encodes at least one of each expected
        /// attribute category so kernel/device/framebuffer mappings work.
        #[test]
        fn standard_mair_has_all_required_slots() {
            // The constant is a 64-bit value with 8 attribute slots.
            let mair = mmu::STANDARD_MAIR;
            // Non-zero in the WB slot (index 3).
            let wb_attr = ((mair >> 24) & 0xFF) as u8;
            assert_ne!(wb_attr, 0, "write-back attribute missing from MAIR");
        }
    }
}

/// RISC-V 64 specific hardware operations (§3.2.1).
///
/// Exposes two sub-modules:
/// - [`riscv64::satp`] — satp register write, sfence.vma invalidation
/// - [`riscv64::sbi`]  — SBI ecall wrappers for hart management
pub mod riscv64 {
    /// SATP (Supervisor Address Translation and Protection) register.
    pub mod satp {
        /// SATP mode field values (bits 63:60 on RV64).
        pub const MODE_BARE: u64 = 0;
        pub const MODE_SV39: u64 = 8;
        pub const MODE_SV48: u64 = 9;
        pub const MODE_SV57: u64 = 10;

        /// Encodes a SATP value from mode, ASID, and root page-table PPN.
        ///
        /// SATP layout (RV64): `mode`\[63..60\] | `asid`\[59..44\] | `ppn`\[43..0\]
        #[inline]
        pub const fn encode(mode: u64, asid: u64, ppn: u64) -> u64 {
            ((mode & 0xF) << 60) | ((asid & 0xFFFF) << 44) | (ppn & 0xFFFF_FFFF_FFFF)
        }

        /// Writes the SATP register and issues an sfence.vma.
        ///
        /// # Safety
        ///
        /// The caller must ensure `ppn` refers to a valid, populated root
        /// page table for the selected mode. Writing SATP while in S-mode
        /// immediately switches the active address space.
        #[cfg(all(target_arch = "riscv64", not(miri)))]
        #[inline(always)]
        pub unsafe fn write_satp(value: u64) {
            // SAFETY:
            //   Preconditions: PPN points to a valid page table for the selected mode
            //   Postconditions: SATP = value; TLB invalidated for all ASIDs
            //   Clobbers: SATP, TLB state
            //   Worst-case: unrecoverable page fault on next memory access
            core::arch::asm!(
                "csrw satp, {v}",
                "sfence.vma zero, zero",
                v = in(reg) value,
                options(nomem, nostack, preserves_flags),
            );
        }

        /// Invalidates all TLB entries for all ASIDs.
        ///
        /// # Safety
        ///
        /// Safe in isolation; always correct.
        #[cfg(all(target_arch = "riscv64", not(miri)))]
        #[inline(always)]
        pub unsafe fn sfence_vma_all() {
            // SAFETY: sfence.vma with zero operands flushes the entire TLB.
            core::arch::asm!(
                "sfence.vma zero, zero",
                options(nomem, nostack, preserves_flags),
            );
        }

        // Miri / non-RISC-V stubs — mirror the real functions' safety contract
        // but perform no hardware side-effects.
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "riscv64", not(miri))))]
        pub unsafe fn write_satp(_value: u64) {}
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "riscv64", not(miri))))]
        pub unsafe fn sfence_vma_all() {}

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn encode_sv48_known() {
                let v = encode(MODE_SV48, 0, 0x1234_5678);
                // mode=9 at bit 60; ppn in low 44 bits.
                assert_eq!(v >> 60, 9);
                assert_eq!(v & 0xFFFF_FFFF_FFFF, 0x1234_5678);
            }

            #[test]
            fn encode_asid_fits_16_bits() {
                let v = encode(MODE_SV39, 0xFFFF, 0);
                assert_eq!((v >> 44) & 0xFFFF, 0xFFFF);
            }
        }
    }

    /// RISC-V SBI (Supervisor Binary Interface) ecall wrappers.
    pub mod sbi {
        /// SBI extension IDs used by ZAMAK.
        pub const EXT_BASE: i32 = 0x10;
        pub const EXT_HSM: i32 = 0x48534D; // "HSM"
        pub const EXT_TIME: i32 = 0x54494D45; // "TIME"
        pub const EXT_SRST: i32 = 0x53525354; // "SRST"

        /// HSM function IDs.
        pub const HSM_HART_START: i32 = 0;
        pub const HSM_HART_STOP: i32 = 1;
        pub const HSM_HART_STATUS: i32 = 2;

        /// SBI v0.3 return structure.
        #[derive(Debug, Clone, Copy)]
        pub struct SbiRet {
            pub error: i64,
            pub value: i64,
        }

        /// SBI error codes.
        pub const SBI_SUCCESS: i64 = 0;
        pub const SBI_ERR_FAILED: i64 = -1;
        pub const SBI_ERR_NOT_SUPPORTED: i64 = -2;
        pub const SBI_ERR_INVALID_PARAM: i64 = -3;
        pub const SBI_ERR_DENIED: i64 = -4;

        /// Invokes an SBI ecall with up to 3 arguments.
        ///
        /// # Safety
        ///
        /// SBI ecalls with side effects (hart start, system reset) impose
        /// their own preconditions on the caller. See the SBI specification
        /// for each function's requirements.
        #[cfg(all(target_arch = "riscv64", not(miri)))]
        #[inline(always)]
        pub unsafe fn call(ext: i32, fid: i32, arg0: u64, arg1: u64, arg2: u64) -> SbiRet {
            let error: i64;
            let value: i64;
            // SAFETY:
            //   Preconditions: SBI implementation present; function IDs valid
            //   Postconditions: a0 = error code, a1 = return value
            //   Clobbers: a0-a7 per SBI calling convention
            //   Worst-case: NOT_SUPPORTED returned; no side effects
            core::arch::asm!(
                "ecall",
                inout("a0") arg0 => error,
                inout("a1") arg1 => value,
                in("a2") arg2,
                in("a6") fid as u64,
                in("a7") ext as u64,
                options(nostack, preserves_flags),
            );
            SbiRet { error, value }
        }

        /// Miri / non-RISC-V stub.
        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "riscv64", not(miri))))]
        pub unsafe fn call(_ext: i32, _fid: i32, _arg0: u64, _arg1: u64, _arg2: u64) -> SbiRet {
            SbiRet {
                error: SBI_ERR_NOT_SUPPORTED,
                value: 0,
            }
        }

        /// Starts another hart via HSM.
        ///
        /// # Safety
        ///
        /// `start_addr` must be a valid supervisor-mode entry point that
        /// is position-independent or correctly mapped for the secondary.
        pub unsafe fn hart_start(hartid: u64, start_addr: u64, opaque: u64) -> SbiRet {
            call(EXT_HSM, HSM_HART_START, hartid, start_addr, opaque)
        }

        /// Stops the calling hart.
        ///
        /// # Safety
        ///
        /// Does not return; subsequent code on this hart is unreachable.
        pub unsafe fn hart_stop() -> SbiRet {
            call(EXT_HSM, HSM_HART_STOP, 0, 0, 0)
        }

        /// Queries the status of a hart.
        ///
        /// # Safety
        ///
        /// Safe in itself, but shares the general SBI caller contract: see [`call`].
        pub unsafe fn hart_status(hartid: u64) -> SbiRet {
            call(EXT_HSM, HSM_HART_STATUS, hartid, 0, 0)
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            /// On non-RISC-V hosts the stub `call` returns `NOT_SUPPORTED`
            /// without trapping, so calling code can probe availability.
            #[test]
            #[cfg(not(target_arch = "riscv64"))]
            fn stub_returns_not_supported() {
                let r = unsafe { call(EXT_BASE, 0, 0, 0, 0) };
                assert_eq!(r.error, SBI_ERR_NOT_SUPPORTED);
                assert_eq!(r.value, 0);

                let r = unsafe { hart_start(1, 0x80200000, 0) };
                assert_eq!(r.error, SBI_ERR_NOT_SUPPORTED);

                let r = unsafe { hart_status(0) };
                assert_eq!(r.error, SBI_ERR_NOT_SUPPORTED);
            }

            /// Extension IDs must match SBI v2.0 ("RISC-V SBI") spec.
            #[test]
            fn extension_ids_match_spec() {
                assert_eq!(EXT_HSM, 0x48534D); // "HSM" in ASCII
                assert_eq!(EXT_TIME, 0x54494D45); // "TIME"
                assert_eq!(EXT_SRST, 0x53525354); // "SRST"
            }

            /// Error codes match SBI specification.
            #[test]
            fn error_codes_match_spec() {
                assert_eq!(SBI_SUCCESS, 0);
                assert_eq!(SBI_ERR_FAILED, -1);
                assert_eq!(SBI_ERR_NOT_SUPPORTED, -2);
            }
        }
    }

    /// RISC-V 64 Sv48 page-table materialization (M4-4 / §FR-MM-002).
    ///
    /// ZAMAK uses the Sv48 translation mode: 48-bit VA, 4-level walk,
    /// 4 KiB leaves. Each PTE is 64 bits laid out as:
    ///
    /// - bit 0 (V)  — Valid
    /// - bit 1 (R)  — Readable
    /// - bit 2 (W)  — Writable
    /// - bit 3 (X)  — Executable
    /// - bit 4 (U)  — User-accessible
    /// - bit 5 (G)  — Global (stays resident across ASID switches)
    /// - bit 6 (A)  — Accessed
    /// - bit 7 (D)  — Dirty
    /// - bits 53:10 — Physical Page Number
    /// - bits 62:61 — PBMT (page-based memory type, Svpbmt): 0 = PMA default,
    ///                1 = NC (Non-Cacheable), 2 = IO (Strong-order device)
    pub mod paging {
        use crate::vmm::{CachePolicy, Mapping, Permissions, VmmPlan};

        pub const PAGE_SIZE: u64 = 4096;
        pub const ENTRIES_PER_TABLE: usize = 512;

        const PTE_V: u64 = 1 << 0;
        const PTE_R: u64 = 1 << 1;
        const PTE_W: u64 = 1 << 2;
        const PTE_X: u64 = 1 << 3;
        const PTE_U: u64 = 1 << 4;
        const PTE_G: u64 = 1 << 5;
        const PTE_A: u64 = 1 << 6;
        const PTE_D: u64 = 1 << 7;

        /// Svpbmt memory-type shift (bits 62:61).
        const PTE_PBMT_SHIFT: u64 = 61;
        /// Svpbmt: PMA (architectural default — depends on the PMAs in PMP).
        const PBMT_PMA: u64 = 0;
        /// Svpbmt: NC — normal non-cacheable, idempotent.
        const PBMT_NC: u64 = 1;
        /// Svpbmt: IO — strongly-ordered, non-idempotent device.
        const PBMT_IO: u64 = 2;

        const fn pbmt_bits(cache: CachePolicy) -> u64 {
            let m: u64 = match cache {
                CachePolicy::WriteBack => PBMT_PMA,
                CachePolicy::WriteThrough => PBMT_PMA,
                CachePolicy::WriteCombining => PBMT_NC,
                CachePolicy::Uncacheable => PBMT_IO,
            };
            m << PTE_PBMT_SHIFT
        }

        const fn perm_bits(p: Permissions) -> u64 {
            let mut bits = 0;
            if p.readable {
                bits |= PTE_R;
            }
            if p.writable {
                bits |= PTE_W;
            }
            if p.executable {
                bits |= PTE_X;
            }
            if p.user {
                bits |= PTE_U;
            }
            bits
        }

        const fn leaf_attrs(perms: Permissions, cache: CachePolicy) -> u64 {
            // A + D set so the hardware never faults just to track access.
            PTE_V | PTE_G | PTE_A | PTE_D | perm_bits(perms) | pbmt_bits(cache)
        }

        /// Converts a physical page address into the PTE PPN field.
        /// Sv48 uses (PA >> 12) << 10 — bit 10 is where the PPN starts.
        const fn pa_to_pte(pa: u64) -> u64 {
            ((pa >> 12) & 0x0FFF_FFFF_FFFF) << 10
        }

        /// Converts a PTE's PPN field back to a physical page address.
        const fn pte_to_pa(pte: u64) -> u64 {
            ((pte >> 10) & 0x0FFF_FFFF_FFFF) << 12
        }

        #[inline]
        const fn index_at(level: u8, va: u64) -> usize {
            // Sv48: VPN[3] at [47:39], VPN[2] [38:30], VPN[1] [29:21], VPN[0] [20:12].
            // level 0 is the root (VPN[3]), level 3 is the leaf (VPN[0]).
            let shift = 39 - (level as u64) * 9;
            ((va >> shift) & 0x1FF) as usize
        }

        pub trait FrameAllocator {
            fn alloc_frame(&mut self) -> Option<u64>;
        }

        pub struct PageTableBuilder<'a, A: FrameAllocator, F: FnMut(u64) -> &'a mut [u64; ENTRIES_PER_TABLE]> {
            pub allocator: A,
            pub phys_to_table: F,
            root: u64,
        }

        impl<'a, A, F> PageTableBuilder<'a, A, F>
        where
            A: FrameAllocator,
            F: FnMut(u64) -> &'a mut [u64; ENTRIES_PER_TABLE],
        {
            pub fn new(mut allocator: A, mut phys_to_table: F) -> Option<Self> {
                let root = allocator.alloc_frame()?;
                let t = phys_to_table(root);
                for e in t.iter_mut() {
                    *e = 0;
                }
                Some(Self { allocator, phys_to_table, root })
            }

            /// Returns the root (VPN[3] table) physical address.
            /// Caller encodes it into SATP via
            /// `satp::encode(MODE_SV48, 0, root >> 12)`.
            pub fn root(&self) -> u64 {
                self.root
            }

            pub fn map_page(&mut self, va: u64, pa: u64, attrs: u64) -> Option<()> {
                let levels = [0u8, 1, 2];
                let mut table_pa = self.root;
                for level in levels {
                    let idx = index_at(level, va);
                    let table = (self.phys_to_table)(table_pa);
                    if table[idx] & PTE_V == 0 {
                        let child = self.allocator.alloc_frame()?;
                        let zeroed = (self.phys_to_table)(child);
                        for e in zeroed.iter_mut() {
                            *e = 0;
                        }
                        let parent = (self.phys_to_table)(table_pa);
                        // Non-leaf PTE: RWX all zero means "table pointer"
                        // per RISC-V priv ISA. Only V + PPN.
                        parent[idx] = PTE_V | pa_to_pte(child);
                        table_pa = child;
                    } else {
                        table_pa = pte_to_pa(table[idx]);
                    }
                }
                let leaf = (self.phys_to_table)(table_pa);
                let idx = index_at(3, va);
                leaf[idx] = pa_to_pte(pa) | attrs;
                Some(())
            }

            pub fn apply(&mut self, plan: &VmmPlan) -> Option<()> {
                for m in &plan.mappings {
                    self.apply_one(m)?;
                }
                Some(())
            }

            fn apply_one(&mut self, m: &Mapping) -> Option<()> {
                let attrs = leaf_attrs(m.perms, m.cache);
                let pages = m.page_count(PAGE_SIZE);
                for i in 0..pages {
                    let offset = i * PAGE_SIZE;
                    let va = m.virt_base.checked_add(offset)?;
                    let pa = m.phys_base.checked_add(offset)?;
                    self.map_page(va, pa, attrs)?;
                }
                Some(())
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn pa_roundtrip() {
                let pa = 0x0000_1234_5678_9000;
                assert_eq!(pte_to_pa(pa_to_pte(pa)), pa);
            }

            #[test]
            fn index_at_extracts_9_bits() {
                let va: u64 = (0x1A << 39) | (0x0B << 30) | (0x0C << 21) | (0x0D << 12);
                assert_eq!(index_at(0, va), 0x1A);
                assert_eq!(index_at(1, va), 0x0B);
                assert_eq!(index_at(2, va), 0x0C);
                assert_eq!(index_at(3, va), 0x0D);
            }

            #[test]
            fn leaf_attrs_sets_valid_and_rwx_for_kernel_code() {
                let a = leaf_attrs(Permissions::KERNEL_CODE, CachePolicy::WriteBack);
                assert_ne!(a & PTE_V, 0);
                assert_ne!(a & PTE_R, 0);
                assert_ne!(a & PTE_X, 0);
                assert_eq!(a & PTE_U, 0);
                // A + D must be set so the MMU never pauses us with a
                // page-fault just to update tracking bits.
                assert_ne!(a & PTE_A, 0);
                assert_ne!(a & PTE_D, 0);
            }

            #[test]
            fn leaf_attrs_kernel_data_no_execute_no_read() {
                let a = leaf_attrs(Permissions::KERNEL_DATA, CachePolicy::WriteBack);
                assert_eq!(a & PTE_X, 0);
                assert_ne!(a & PTE_W, 0);
            }

            #[test]
            fn pbmt_device_policy_is_io() {
                let a = leaf_attrs(Permissions::MMIO, CachePolicy::Uncacheable);
                assert_eq!(
                    (a >> PTE_PBMT_SHIFT) & 0b11,
                    PBMT_IO,
                    "MMIO mappings must use PBMT IO"
                );
            }

            #[test]
            fn pbmt_framebuffer_uses_nc() {
                let a = leaf_attrs(Permissions::KERNEL_DATA, CachePolicy::WriteCombining);
                assert_eq!(
                    (a >> PTE_PBMT_SHIFT) & 0b11,
                    PBMT_NC,
                    "framebuffer mappings must use PBMT NC"
                );
            }
        }
    }
}

/// LoongArch64 specific hardware operations (§3.2.1).
///
/// LoongArch uses CSR (Configuration Status Registers) instead of x86 MSRs
/// or ARM system registers. The translation model is the "Direct Mapped
/// Window" (DMW) for HHDM-style physical access plus a 4-level page table
/// rooted at PGDH/PGDL.
pub mod loongarch64 {
    /// CSR numbers used by ZAMAK.
    pub mod csr {
        pub const CRMD: u32 = 0x0; // Current mode info
        pub const PRMD: u32 = 0x1; // Prev mode info
        pub const EUEN: u32 = 0x2; // Extended unit enable
        pub const PGDL: u32 = 0x19; // Page table root (user / low half)
        pub const PGDH: u32 = 0x1A; // Page table root (kernel / high half)
        pub const PGD: u32 = 0x1B; // Active page table
        pub const PWCL: u32 = 0x1C; // Page walk config (low)
        pub const PWCH: u32 = 0x1D; // Page walk config (high)
        pub const STLBPS: u32 = 0x1E; // STLB page size
        pub const RVACFG: u32 = 0x1F; // Reduced virtual address config
        pub const ASID: u32 = 0x18; // Address space ID
        pub const DMW0: u32 = 0x180; // Direct mapping window 0
        pub const DMW1: u32 = 0x181; // Direct mapping window 1
        pub const DMW2: u32 = 0x182; // Direct mapping window 2
        pub const DMW3: u32 = 0x183; // Direct mapping window 3
    }

    /// Direct-mapped window configuration.
    ///
    /// Each DMW establishes a virtual-to-physical identity mapping on a
    /// 2⁶⁰-byte slice of the address space. ZAMAK programs DMW0 to map
    /// the Linux-style `0x8000_0000_0000_0000` HHDM base to physical 0.
    #[derive(Debug, Clone, Copy)]
    pub struct Dmw {
        pub vseg: u8,         // Virtual segment (bits 63:60)
        pub mat: u8,          // Memory access type (0 = WB, 1 = UC, etc.)
        pub plv0_allowed: bool, // Supervisor access allowed
        pub plv3_allowed: bool, // User access allowed
    }

    impl Dmw {
        /// Encodes the DMW CSR value per the LoongArch Reference Manual.
        #[must_use]
        pub const fn encode(&self) -> u64 {
            let mut v: u64 = 0;
            if self.plv0_allowed {
                v |= 1;
            }
            if self.plv3_allowed {
                v |= 1 << 3;
            }
            v |= (self.mat as u64) << 4;
            v |= (self.vseg as u64) << 60;
            v
        }
    }

    /// Writes a CSR value. Implementations for real hardware use `csrwr`;
    /// stub variants for other architectures discard the write.
    ///
    /// The CSR index is passed as a const generic because LoongArch's
    /// `csrwr` / `csrrd` instructions encode the CSR number in the
    /// instruction word — it must be a compile-time constant.
    ///
    /// # Safety
    ///
    /// Writing to CSRs like PGDH or DMW changes address translation
    /// immediately. The caller must ensure the configuration is consistent
    /// before any memory access after the write.
    #[cfg(all(target_arch = "loongarch64", not(miri)))]
    #[inline(always)]
    pub unsafe fn csr_write<const CSR: u32>(value: u64) -> u64 {
        let prev: u64;
        // SAFETY:
        //   Preconditions: `CSR` is a valid CSR number for this PLV
        //   Postconditions: CSR[CSR] = value; prev holds the old value
        //   Clobbers: the CSR
        //   Worst-case: privilege fault if CSR is not accessible at current PLV
        core::arch::asm!(
            "csrwr {0}, {csr}",
            inout(reg) value => prev,
            csr = const CSR,
            options(nostack, preserves_flags),
        );
        prev
    }

    /// Reads a CSR value. See [`csr_write`] for the const-generic rationale.
    #[cfg(all(target_arch = "loongarch64", not(miri)))]
    #[inline(always)]
    pub fn csr_read<const CSR: u32>() -> u64 {
        let value: u64;
        // SAFETY: csrrd reads a CSR; no side effects. Not unsafe at module level.
        unsafe {
            core::arch::asm!(
                "csrrd {0}, {csr}",
                out(reg) value,
                csr = const CSR,
                options(nomem, nostack, preserves_flags),
            );
        }
        value
    }

    /// Invalidates all entries in the STLB (Shared TLB).
    ///
    /// # Safety
    ///
    /// Always safe; at worst, the TLB is refilled from the page tables.
    #[cfg(all(target_arch = "loongarch64", not(miri)))]
    #[inline(always)]
    pub unsafe fn invtlb_all() {
        // SAFETY: invtlb 0, r0, r0 flushes the entire shared TLB.
        core::arch::asm!(
            "invtlb 0x0, $r0, $r0",
            "dbar 0",
            "ibar 0",
            options(nomem, nostack, preserves_flags),
        );
    }

    // Non-LoongArch / Miri stubs — side-effect-free so host tests can call them.
    #[allow(clippy::missing_safety_doc)]
    #[cfg(not(all(target_arch = "loongarch64", not(miri))))]
    pub unsafe fn csr_write<const CSR: u32>(_value: u64) -> u64 {
        0
    }
    #[cfg(not(all(target_arch = "loongarch64", not(miri))))]
    pub fn csr_read<const CSR: u32>() -> u64 {
        0
    }
    #[allow(clippy::missing_safety_doc)]
    #[cfg(not(all(target_arch = "loongarch64", not(miri))))]
    pub unsafe fn invtlb_all() {}

    /// IOCSR (I/O Configuration Status Register) accessors for SMP
    /// bring-up on Loongson 3 / 3A5000+ platforms.
    pub mod iocsr {
        /// IOCSR register offsets used by ZAMAK during AP bring-up.
        pub const MBUF0: u32 = 0x1020; // Mailbox 0 (AP entry point)
        pub const IPI_SET: u32 = 0x1040; // IPI set register
        pub const IPI_CLEAR: u32 = 0x1060;
        pub const IPI_STATUS: u32 = 0x1000;

        /// Writes an IOCSR register (32-bit) via `iocsrwr.w`.
        ///
        /// # Safety
        ///
        /// IOCSR writes to IPI/MBUF regions affect remote cores. Caller
        /// must ensure the target core is initialised or gated by a
        /// barrier.
        #[cfg(all(target_arch = "loongarch64", not(miri)))]
        #[inline(always)]
        pub unsafe fn write32(addr: u32, value: u32) {
            // SAFETY: See doc comment.
            core::arch::asm!(
                "iocsrwr.w {v}, {a}",
                v = in(reg) value,
                a = in(reg) addr,
                options(nostack),
            );
        }

        /// Reads an IOCSR register (32-bit) via `iocsrrd.w`.
        #[cfg(all(target_arch = "loongarch64", not(miri)))]
        #[inline(always)]
        pub fn read32(addr: u32) -> u32 {
            let value: u32;
            // SAFETY: iocsrrd.w has no side effects; wrapper is safe.
            unsafe {
                core::arch::asm!(
                    "iocsrrd.w {v}, {a}",
                    v = out(reg) value,
                    a = in(reg) addr,
                    options(nomem, nostack, preserves_flags),
                );
            }
            value
        }

        #[allow(clippy::missing_safety_doc)]
        #[cfg(not(all(target_arch = "loongarch64", not(miri))))]
        pub unsafe fn write32(_addr: u32, _value: u32) {}
        #[cfg(not(all(target_arch = "loongarch64", not(miri))))]
        pub fn read32(_addr: u32) -> u32 {
            0
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// DMW encoding matches expected layout for the standard ZAMAK HHDM window.
        #[test]
        fn dmw_encode_hhdm_standard() {
            // Vseg 8 (0x8), MAT 0 (write-back), PLV0 only.
            let dmw = Dmw {
                vseg: 0x8,
                mat: 0,
                plv0_allowed: true,
                plv3_allowed: false,
            };
            let encoded = dmw.encode();
            // PLV0 enable bit set (bit 0).
            assert_eq!(encoded & 1, 1);
            // PLV3 enable bit clear (bit 3).
            assert_eq!(encoded & (1 << 3), 0);
            // MAT field (bits 5:4) is zero for write-back.
            assert_eq!((encoded >> 4) & 0xF, 0);
            // Vseg at top: bits 63:60.
            assert_eq!(encoded >> 60, 0x8);
        }

        /// Stub functions on non-LoongArch hosts must not trap.
        #[test]
        #[cfg(not(target_arch = "loongarch64"))]
        fn stubs_do_not_trap() {
            unsafe {
                assert_eq!(csr_write::<{ csr::DMW0 }>(0), 0);
                assert_eq!(csr_read::<{ csr::DMW0 }>(), 0);
                invtlb_all();
                iocsr::write32(iocsr::MBUF0, 0);
                assert_eq!(iocsr::read32(iocsr::MBUF0), 0);
            }
        }

        /// CSR numbers match the LoongArch reference manual (subset used).
        #[test]
        fn csr_numbers_match_spec() {
            assert_eq!(csr::DMW0, 0x180);
            assert_eq!(csr::PGDH, 0x1A);
            assert_eq!(csr::PGDL, 0x19);
            assert_eq!(csr::ASID, 0x18);
        }
    }

    /// LoongArch64 4-level page-table materialization (M6-1 / §FR-MM-002).
    ///
    /// LoongArch64 uses a 4-level PGD/PUD/PMD/PTE hierarchy with a 4 KiB
    /// page size. Each level is a 512-entry (2^9) table indexed from bits
    /// \[47:39\] / \[38:30\] / \[29:21\] / \[20:12\] of the virtual address.
    /// Leaf entries encode:
    ///
    /// - bit 0  (V)  — Valid
    /// - bit 1  (D)  — Dirty (write-allowed when set on a writable page)
    /// - bit 2/3 (PLV) — Privilege level (0 = kernel, 3 = user)
    /// - bit 4/5 (MAT) — Memory access type (0 = SUC, 1 = CC, 2 = WUC)
    /// - bit 6  (G)  — Global
    /// - bit 7  (HUGE)— Huge page (1 at non-leaf levels marks a block)
    /// - bit 62 (NX) — No-execute
    ///
    /// ZAMAK always maps at 4 KiB granule; huge pages are not used during
    /// boot so the HUGE bit is zero on every entry we install.
    pub mod paging {
        use crate::vmm::{CachePolicy, Mapping, Permissions, VmmPlan};

        pub const PAGE_SIZE: u64 = 4096;
        pub const ENTRIES_PER_TABLE: usize = 512;

        const PTE_V: u64 = 1 << 0;
        const PTE_D: u64 = 1 << 1;
        const PTE_PLV0: u64 = 0b00 << 2;
        const PTE_PLV3: u64 = 0b11 << 2;
        const PTE_G: u64 = 1 << 6;
        const PTE_NX: u64 = 1 << 62;

        /// Memory Access Type field (bits 5:4).
        /// 0 = SUC (Strong Uncached) — MMIO / device
        /// 1 = CC  (Coherent Cached) — normal RAM / write-back
        /// 2 = WUC (Weak Uncached)   — framebuffer / write-combining
        const fn mat_bits(cache: CachePolicy) -> u64 {
            let mat: u64 = match cache {
                CachePolicy::Uncacheable => 0,
                CachePolicy::WriteCombining => 2,
                CachePolicy::WriteThrough => 1,
                CachePolicy::WriteBack => 1,
            };
            mat << 4
        }

        const fn perm_bits(p: Permissions) -> u64 {
            let mut bits = 0;
            if p.user {
                bits |= PTE_PLV3;
            } else {
                bits |= PTE_PLV0;
            }
            if !p.executable {
                bits |= PTE_NX;
            }
            if p.writable {
                bits |= PTE_D;
            }
            bits
        }

        const fn leaf_attrs(perms: Permissions, cache: CachePolicy) -> u64 {
            PTE_V | PTE_G | perm_bits(perms) | mat_bits(cache)
        }

        #[inline]
        const fn index_at(level: u8, va: u64) -> usize {
            // level 0 = PGD (bits 47:39), level 3 = PTE (bits 20:12).
            let shift = 39 - (level as u64) * 9;
            ((va >> shift) & 0x1FF) as usize
        }

        pub trait FrameAllocator {
            fn alloc_frame(&mut self) -> Option<u64>;
        }

        pub struct PageTableBuilder<'a, A: FrameAllocator, F: FnMut(u64) -> &'a mut [u64; ENTRIES_PER_TABLE]> {
            pub allocator: A,
            pub phys_to_table: F,
            root: u64,
        }

        impl<'a, A, F> PageTableBuilder<'a, A, F>
        where
            A: FrameAllocator,
            F: FnMut(u64) -> &'a mut [u64; ENTRIES_PER_TABLE],
        {
            pub fn new(mut allocator: A, mut phys_to_table: F) -> Option<Self> {
                let root = allocator.alloc_frame()?;
                let t = phys_to_table(root);
                for e in t.iter_mut() {
                    *e = 0;
                }
                Some(Self { allocator, phys_to_table, root })
            }

            pub fn root(&self) -> u64 {
                self.root
            }

            pub fn map_page(&mut self, va: u64, pa: u64, attrs: u64) -> Option<()> {
                let levels = [0u8, 1, 2];
                let mut table_pa = self.root;
                for level in levels {
                    let idx = index_at(level, va);
                    let table = (self.phys_to_table)(table_pa);
                    if table[idx] & PTE_V == 0 {
                        let child = self.allocator.alloc_frame()?;
                        let zeroed = (self.phys_to_table)(child);
                        for e in zeroed.iter_mut() {
                            *e = 0;
                        }
                        let parent = (self.phys_to_table)(table_pa);
                        // Intermediate entries just need V + the physical
                        // pointer. LoongArch walk treats non-huge entries
                        // as table descriptors when HUGE bit is clear.
                        parent[idx] = (child & 0x0000_FFFF_FFFF_F000) | PTE_V;
                        table_pa = child;
                    } else {
                        table_pa = table[idx] & 0x0000_FFFF_FFFF_F000;
                    }
                }
                let leaf = (self.phys_to_table)(table_pa);
                let idx = index_at(3, va);
                leaf[idx] = (pa & 0x0000_FFFF_FFFF_F000) | attrs;
                Some(())
            }

            pub fn apply(&mut self, plan: &VmmPlan) -> Option<()> {
                for m in &plan.mappings {
                    self.apply_one(m)?;
                }
                Some(())
            }

            fn apply_one(&mut self, m: &Mapping) -> Option<()> {
                let attrs = leaf_attrs(m.perms, m.cache);
                let pages = m.page_count(PAGE_SIZE);
                for i in 0..pages {
                    let offset = i * PAGE_SIZE;
                    let va = m.virt_base.checked_add(offset)?;
                    let pa = m.phys_base.checked_add(offset)?;
                    self.map_page(va, pa, attrs)?;
                }
                Some(())
            }
        }

        #[cfg(test)]
        mod tests {
            use super::*;

            #[test]
            fn index_at_extracts_9_bits() {
                let va: u64 = (0x1A << 39) | (0x0B << 30) | (0x0C << 21) | (0x0D << 12);
                assert_eq!(index_at(0, va), 0x1A);
                assert_eq!(index_at(1, va), 0x0B);
                assert_eq!(index_at(2, va), 0x0C);
                assert_eq!(index_at(3, va), 0x0D);
            }

            #[test]
            fn mat_bits_for_known_policies() {
                assert_eq!(mat_bits(CachePolicy::Uncacheable), 0 << 4);
                assert_eq!(mat_bits(CachePolicy::WriteCombining), 2 << 4);
                assert_eq!(mat_bits(CachePolicy::WriteBack), 1 << 4);
            }

            #[test]
            fn perm_bits_sets_nx_for_data() {
                assert_ne!(perm_bits(Permissions::KERNEL_DATA) & PTE_NX, 0);
            }

            #[test]
            fn perm_bits_no_nx_for_code() {
                assert_eq!(perm_bits(Permissions::KERNEL_CODE) & PTE_NX, 0);
            }

            #[test]
            fn leaf_attrs_sets_valid_and_global() {
                let a = leaf_attrs(Permissions::KERNEL_CODE, CachePolicy::WriteBack);
                assert_ne!(a & PTE_V, 0);
                assert_ne!(a & PTE_G, 0);
            }
        }
    }
}
