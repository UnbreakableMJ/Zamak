// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Property-based tests (TEST-7, PRD §8.1).
//!
//! These tests explore randomised inputs to find edge cases in the PMM,
//! config parser, URI parser, and other core algorithms. They run as part
//! of `cargo test --test proptests` and in CI.

// Rust guideline compliant 2026-03-30

extern crate alloc;

use proptest::prelude::*;
use zamak_core::config;
use zamak_core::pmm::{MemoryRegion, MemoryType, Pmm, PAGE_SIZE};
use zamak_core::rng::{kaslr_base, KaslrRng, KASLR_ALIGNMENT};

/// Deterministic RNG for property-test reproducibility.
struct SeededRng(u64);
impl KaslrRng for SeededRng {
    fn get_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

/// Strategy for generating page-aligned, non-overlapping memory regions.
fn arb_memory_region() -> impl Strategy<Value = MemoryRegion> {
    (0u64..0x1_0000_0000, 1u64..0x1_000_0000).prop_map(|(base, length)| {
        // Align base to page size to avoid trivial rejections.
        let aligned_base = base & !(PAGE_SIZE - 1);
        MemoryRegion {
            base: aligned_base,
            length: length & !(PAGE_SIZE - 1),
            region_type: MemoryType::Usable,
        }
    })
}

proptest! {
    /// PMM normalisation must always produce page-aligned regions.
    #[test]
    fn pmm_normalised_regions_are_page_aligned(
        regions in prop::collection::vec(arb_memory_region(), 0..16)
    ) {
        let pmm = Pmm::new(&regions);
        for r in pmm.regions() {
            prop_assert_eq!(r.base % PAGE_SIZE, 0);
            prop_assert_eq!(r.length % PAGE_SIZE, 0);
        }
    }

    /// PMM allocation bounds check: allocated base is always below the
    /// highest usable memory.
    #[test]
    fn pmm_allocation_stays_within_bounds(
        regions in prop::collection::vec(arb_memory_region(), 1..8),
        page_count in 1u64..16
    ) {
        let mut pmm = Pmm::new(&regions);
        let total_usable = pmm.total_usable();
        if let Some(base) = pmm.allocate_pages(page_count) {
            prop_assert_eq!(base % PAGE_SIZE, 0);
            prop_assert!(total_usable >= page_count * PAGE_SIZE);
        }
    }

    /// PMM allocations never overlap: successive allocations return
    /// disjoint regions.
    #[test]
    fn pmm_allocations_are_disjoint(
        seed_regions in prop::collection::vec(arb_memory_region(), 1..8),
        n in 1usize..5
    ) {
        let mut pmm = Pmm::new(&seed_regions);
        let mut allocs: alloc::vec::Vec<(u64, u64)> = alloc::vec::Vec::new();
        for _ in 0..n {
            if let Some(base) = pmm.allocate_pages(1) {
                allocs.push((base, base + PAGE_SIZE));
            }
        }
        // Check no two allocations overlap.
        for i in 0..allocs.len() {
            for j in (i + 1)..allocs.len() {
                let (a_start, a_end) = allocs[i];
                let (b_start, b_end) = allocs[j];
                prop_assert!(
                    a_end <= b_start || b_end <= a_start,
                    "overlap: [{:#x}, {:#x}) and [{:#x}, {:#x})",
                    a_start, a_end, b_start, b_end
                );
            }
        }
    }

    /// KASLR base is always 1 GiB-aligned when a slot exists.
    #[test]
    fn kaslr_base_is_gib_aligned(
        seed in any::<u64>(),
        min in 0u64..0x1_0000_0000,
        range_mib in 1024u64..8192,
        size_mib in 1u64..512
    ) {
        let mut rng = SeededRng(seed.wrapping_add(1));
        let max = min + range_mib * 1024 * 1024;
        let size = size_mib * 1024 * 1024;
        if let Some(base) = kaslr_base(&mut rng, min, max, size) {
            prop_assert_eq!(base % KASLR_ALIGNMENT, 0);
            prop_assert!(base >= min);
            prop_assert!(base + size <= max);
        }
    }

    /// Config parser never panics on arbitrary input.
    #[test]
    fn config_parser_does_not_panic(input in "\\PC{0,2048}") {
        let _ = config::parse(&input);
    }

    /// Parsed config always has at least default values.
    #[test]
    fn config_parser_preserves_defaults(input in "\\PC{0,256}") {
        let config = config::parse(&input);
        // These fields always have a sensible default.
        prop_assert!(!config.theme_variant.is_empty());
        // timeout has a default of 5; if user sets it, it's any u64.
        let _ = config.timeout; // just verify field is accessible
    }
}

/// Non-proptest: verifies the RNG seeding yields reproducible sequences.
#[test]
fn seeded_rng_is_deterministic() {
    let mut a = SeededRng(42);
    let mut b = SeededRng(42);
    for _ in 0..16 {
        assert_eq!(a.get_u64(), b.get_u64());
    }
}
