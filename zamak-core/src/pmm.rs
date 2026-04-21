// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Physical Memory Manager (FR-MM-001).
//!
//! Normalizes firmware memory maps (E820 / UEFI GetMemoryMap) into the
//! unified Limine type system, performs overlap resolution, page-alignment
//! sanitization, and provides top-down allocation.
//!
//! # Design
//!
//! The PMM operates in two phases:
//! 1. **Normalization** — firmware memory map entries are sorted, overlaps
//!    resolved (higher-priority types win), and base/limit page-aligned.
//! 2. **Allocation** — pages are allocated top-down from the highest
//!    available region, which avoids fragmenting low memory needed by
//!    legacy hardware.

// Rust guideline compliant 2026-03-30

use alloc::vec::Vec;

/// Page size: 4 KiB.
pub const PAGE_SIZE: u64 = 4096;

/// Memory region types (Limine unified type system).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MemoryType {
    /// Usable RAM.
    Usable = 0,
    /// Reserved by firmware.
    Reserved = 1,
    /// ACPI reclaimable.
    AcpiReclaimable = 2,
    /// ACPI NVS (non-volatile storage).
    AcpiNvs = 3,
    /// Bad / defective RAM.
    BadMemory = 4,
    /// Bootloader reclaimable (used by ZAMAK, reclaimable after handoff).
    BootloaderReclaimable = 5,
    /// Kernel and modules.
    KernelAndModules = 6,
    /// Framebuffer memory.
    Framebuffer = 7,
}

impl MemoryType {
    /// Returns the priority of this type for overlap resolution.
    /// Higher values take precedence when two regions overlap.
    fn priority(self) -> u32 {
        match self {
            Self::Usable => 0,
            Self::BootloaderReclaimable => 1,
            Self::AcpiReclaimable => 2,
            Self::AcpiNvs => 3,
            Self::Reserved => 4,
            Self::Framebuffer => 5,
            Self::KernelAndModules => 6,
            Self::BadMemory => 7,
        }
    }

    /// Converts from E820 type values.
    pub fn from_e820(e820_type: u32) -> Self {
        match e820_type {
            1 => Self::Usable,
            3 => Self::AcpiReclaimable,
            4 => Self::AcpiNvs,
            5 => Self::BadMemory,
            _ => Self::Reserved,
        }
    }
}

/// A physical memory region.
#[derive(Debug, Clone, Copy)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub region_type: MemoryType,
}

impl MemoryRegion {
    /// End address (exclusive).
    pub fn end(&self) -> u64 {
        self.base.saturating_add(self.length)
    }
}

/// The physical memory manager state.
pub struct Pmm {
    /// Normalized, sorted, non-overlapping memory regions.
    regions: Vec<MemoryRegion>,
    /// Next allocation address (decrements on each allocation).
    alloc_top: u64,
}

impl Pmm {
    /// Creates a new PMM from raw firmware memory map entries.
    ///
    /// The entries are normalized: sorted by base address, overlaps resolved
    /// (higher-priority types win), and base/limit page-aligned.
    pub fn new(raw_entries: &[MemoryRegion]) -> Self {
        let regions = normalize(raw_entries);
        let alloc_top = regions
            .iter()
            .filter(|r| r.region_type == MemoryType::Usable)
            .map(|r| r.end())
            .max()
            .unwrap_or(0);

        Self { regions, alloc_top }
    }

    /// Returns the normalized memory map.
    pub fn regions(&self) -> &[MemoryRegion] {
        &self.regions
    }

    /// Total usable memory in bytes.
    pub fn total_usable(&self) -> u64 {
        self.regions
            .iter()
            .filter(|r| r.region_type == MemoryType::Usable)
            .map(|r| r.length)
            .sum()
    }

    /// Allocates `count` contiguous physical pages (top-down).
    ///
    /// Returns the base physical address of the allocated region,
    /// or `None` if no suitable region is found.
    pub fn allocate_pages(&mut self, count: u64) -> Option<u64> {
        let size = count.checked_mul(PAGE_SIZE)?;
        if size == 0 {
            return None;
        }

        // Search usable regions top-down.
        // Find the highest usable region that fits below alloc_top.
        let mut best: Option<(u64, usize)> = None; // (alloc_base, region_index)

        for (i, region) in self.regions.iter().enumerate() {
            if region.region_type != MemoryType::Usable {
                continue;
            }

            let region_end = region.end().min(self.alloc_top);
            if region_end < size || region_end - size < region.base {
                continue;
            }

            let alloc_base = align_down(region_end - size, PAGE_SIZE);
            if alloc_base < region.base {
                continue;
            }

            match best {
                Some((prev_base, _)) if alloc_base <= prev_base => {}
                _ => best = Some((alloc_base, i)),
            }
        }

        let (alloc_base, region_idx) = best?;

        // Split the region: the allocated portion becomes BootloaderReclaimable.
        let region = self.regions[region_idx];
        let alloc_end = alloc_base + size;

        // Remove the original region and insert up to 3 replacement regions.
        self.regions.remove(region_idx);
        let mut insert_idx = region_idx;

        // Part before the allocation.
        if alloc_base > region.base {
            self.regions.insert(
                insert_idx,
                MemoryRegion {
                    base: region.base,
                    length: alloc_base - region.base,
                    region_type: MemoryType::Usable,
                },
            );
            insert_idx += 1;
        }

        // The allocated region itself.
        self.regions.insert(
            insert_idx,
            MemoryRegion {
                base: alloc_base,
                length: size,
                region_type: MemoryType::BootloaderReclaimable,
            },
        );
        insert_idx += 1;

        // Part after the allocation.
        if alloc_end < region.end() {
            self.regions.insert(
                insert_idx,
                MemoryRegion {
                    base: alloc_end,
                    length: region.end() - alloc_end,
                    region_type: MemoryType::Usable,
                },
            );
        }

        // Move alloc_top down to avoid re-scanning already-allocated space.
        self.alloc_top = alloc_base;

        Some(alloc_base)
    }

    /// Marks a physical address range as a specific type.
    ///
    /// Used to mark regions for kernel, modules, framebuffer, etc.
    pub fn mark_region(&mut self, base: u64, length: u64, region_type: MemoryType) {
        let mark_end = base.saturating_add(length);
        let mut new_regions = Vec::with_capacity(self.regions.len() + 2);

        for region in &self.regions {
            let r_end = region.end();

            // No overlap — keep as-is.
            if mark_end <= region.base || base >= r_end {
                new_regions.push(*region);
                continue;
            }

            // Part before the marked range.
            if region.base < base {
                new_regions.push(MemoryRegion {
                    base: region.base,
                    length: base - region.base,
                    region_type: region.region_type,
                });
            }

            // The marked region (only the overlapping portion).
            let overlap_start = base.max(region.base);
            let overlap_end = mark_end.min(r_end);
            new_regions.push(MemoryRegion {
                base: overlap_start,
                length: overlap_end - overlap_start,
                region_type,
            });

            // Part after the marked range.
            if r_end > mark_end {
                new_regions.push(MemoryRegion {
                    base: mark_end,
                    length: r_end - mark_end,
                    region_type: region.region_type,
                });
            }
        }

        self.regions = new_regions;
    }
}

/// Normalizes raw firmware memory map entries.
///
/// 1. Sort by base address.
/// 2. Resolve overlaps (higher-priority type wins).
/// 3. Page-align: base rounded up, end rounded down.
/// 4. Merge adjacent regions of the same type.
/// 5. Remove zero-length regions.
fn normalize(raw: &[MemoryRegion]) -> Vec<MemoryRegion> {
    if raw.is_empty() {
        return Vec::new();
    }

    // Sort by base address.
    let mut entries: Vec<MemoryRegion> = raw.to_vec();
    entries.sort_by_key(|e| e.base);

    // Page-align entries.
    for entry in &mut entries {
        let aligned_base = align_up(entry.base, PAGE_SIZE);
        let aligned_end = align_down(entry.end(), PAGE_SIZE);
        if aligned_end <= aligned_base {
            entry.length = 0;
        } else {
            entry.base = aligned_base;
            entry.length = aligned_end - aligned_base;
        }
    }

    // Remove zero-length entries.
    entries.retain(|e| e.length > 0);

    // Resolve overlaps: sweep-line approach.
    // For simplicity, use a split-and-override approach.
    let mut resolved: Vec<MemoryRegion> = Vec::with_capacity(entries.len() * 2);
    for entry in &entries {
        insert_with_overlap(&mut resolved, *entry);
    }

    // Merge adjacent regions of the same type.
    merge_adjacent(&mut resolved);

    resolved
}

/// Inserts a region into the list, resolving overlaps by priority.
fn insert_with_overlap(regions: &mut Vec<MemoryRegion>, new: MemoryRegion) {
    let new_end = new.end();
    let mut to_add: Vec<MemoryRegion> = Vec::new();
    let mut i = 0;

    while i < regions.len() {
        let existing = regions[i];
        let ex_end = existing.end();

        // No overlap.
        if new_end <= existing.base || new.base >= ex_end {
            i += 1;
            continue;
        }

        // Overlap detected. Compare priorities.
        if new.region_type.priority() >= existing.region_type.priority() {
            // New type wins in the overlap region. Split existing around new.
            regions.remove(i);

            // Part of existing before new.
            if existing.base < new.base {
                regions.insert(
                    i,
                    MemoryRegion {
                        base: existing.base,
                        length: new.base - existing.base,
                        region_type: existing.region_type,
                    },
                );
                i += 1;
            }

            // Part of existing after new.
            if ex_end > new_end {
                regions.insert(
                    i,
                    MemoryRegion {
                        base: new_end,
                        length: ex_end - new_end,
                        region_type: existing.region_type,
                    },
                );
            }
        } else {
            // Existing type wins. Trim new around existing.
            if new.base < existing.base {
                to_add.push(MemoryRegion {
                    base: new.base,
                    length: existing.base - new.base,
                    region_type: new.region_type,
                });
            }
            if new_end > ex_end {
                // Continue with the remainder.
                // We'll handle this by adjusting new and continuing.
                to_add.push(MemoryRegion {
                    base: ex_end,
                    length: new_end - ex_end,
                    region_type: new.region_type,
                });
            }
            // The overlapping portion stays as existing type.
            // Insert partial pieces and return.
            for piece in to_add {
                if piece.length > 0 {
                    insert_sorted(regions, piece);
                }
            }
            return;
        }
    }

    // No remaining overlap — insert the new region.
    insert_sorted(regions, new);
    for piece in to_add {
        if piece.length > 0 {
            insert_sorted(regions, piece);
        }
    }
}

/// Inserts a region in sorted order by base address.
fn insert_sorted(regions: &mut Vec<MemoryRegion>, region: MemoryRegion) {
    let pos = regions
        .iter()
        .position(|r| r.base > region.base)
        .unwrap_or(regions.len());
    regions.insert(pos, region);
}

/// Merges adjacent regions of the same type.
fn merge_adjacent(regions: &mut Vec<MemoryRegion>) {
    if regions.len() < 2 {
        return;
    }

    let mut i = 0;
    while i + 1 < regions.len() {
        if regions[i].region_type == regions[i + 1].region_type
            && regions[i].end() == regions[i + 1].base
        {
            regions[i].length += regions[i + 1].length;
            regions.remove(i + 1);
        } else {
            i += 1;
        }
    }
}

/// Aligns a value up to the given alignment.
const fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

/// Aligns a value down to the given alignment.
const fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_basic() {
        let raw = [
            MemoryRegion {
                base: 0,
                length: 0xA0000,
                region_type: MemoryType::Usable,
            },
            MemoryRegion {
                base: 0xA0000,
                length: 0x60000,
                region_type: MemoryType::Reserved,
            },
            MemoryRegion {
                base: 0x100000,
                length: 0x7F00000,
                region_type: MemoryType::Usable,
            },
        ];
        let pmm = Pmm::new(&raw);
        assert!(pmm.total_usable() > 0);
        assert!(pmm.regions().len() >= 2);
    }

    #[test]
    fn page_alignment() {
        // Region not page-aligned: base=0x123, length=0x5000.
        let raw = [MemoryRegion {
            base: 0x123,
            length: 0x5000,
            region_type: MemoryType::Usable,
        }];
        let regions = normalize(&raw);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].base, PAGE_SIZE); // Rounded up to 0x1000.
        assert_eq!(regions[0].base % PAGE_SIZE, 0);
        assert_eq!(regions[0].end() % PAGE_SIZE, 0);
    }

    #[test]
    fn overlap_resolution() {
        // Two regions overlap: usable [0, 0x10000) and reserved [0x5000, 0x8000).
        let raw = [
            MemoryRegion {
                base: 0,
                length: 0x10000,
                region_type: MemoryType::Usable,
            },
            MemoryRegion {
                base: 0x5000,
                length: 0x3000,
                region_type: MemoryType::Reserved,
            },
        ];
        let regions = normalize(&raw);

        // Should have 3 regions: usable, reserved, usable.
        assert!(regions.len() >= 2);
        // The reserved region should be present in the overlap zone.
        let reserved = regions
            .iter()
            .find(|r| r.region_type == MemoryType::Reserved);
        assert!(reserved.is_some());
        let reserved = reserved.unwrap();
        assert_eq!(reserved.base, 0x5000);
        assert_eq!(reserved.length, 0x3000);
    }

    #[test]
    fn top_down_allocation() {
        let raw = [MemoryRegion {
            base: 0x100000,
            length: 0x100000,
            region_type: MemoryType::Usable,
        }];
        let mut pmm = Pmm::new(&raw);

        // Allocate 1 page — should come from the top.
        let addr = pmm.allocate_pages(1).unwrap();
        assert_eq!(addr, 0x100000 + 0x100000 - PAGE_SIZE); // Top of region.
        assert_eq!(addr % PAGE_SIZE, 0);

        // Allocate another page.
        let addr2 = pmm.allocate_pages(1).unwrap();
        assert!(addr2 < addr);
        assert_eq!(addr2 % PAGE_SIZE, 0);
    }

    #[test]
    fn allocate_too_large() {
        let raw = [MemoryRegion {
            base: 0x100000,
            length: PAGE_SIZE * 2,
            region_type: MemoryType::Usable,
        }];
        let mut pmm = Pmm::new(&raw);
        assert!(pmm.allocate_pages(3).is_none());
    }

    #[test]
    fn allocate_zero_pages() {
        let raw = [MemoryRegion {
            base: 0x100000,
            length: PAGE_SIZE * 10,
            region_type: MemoryType::Usable,
        }];
        let mut pmm = Pmm::new(&raw);
        assert!(pmm.allocate_pages(0).is_none());
    }

    #[test]
    fn mark_region() {
        let raw = [MemoryRegion {
            base: 0x100000,
            length: 0x100000,
            region_type: MemoryType::Usable,
        }];
        let mut pmm = Pmm::new(&raw);
        pmm.mark_region(0x150000, 0x10000, MemoryType::KernelAndModules);

        let kernel_region = pmm
            .regions()
            .iter()
            .find(|r| r.region_type == MemoryType::KernelAndModules);
        assert!(kernel_region.is_some());
        let kr = kernel_region.unwrap();
        assert_eq!(kr.base, 0x150000);
        assert_eq!(kr.length, 0x10000);
    }

    #[test]
    fn e820_type_conversion() {
        assert_eq!(MemoryType::from_e820(1), MemoryType::Usable);
        assert_eq!(MemoryType::from_e820(2), MemoryType::Reserved);
        assert_eq!(MemoryType::from_e820(3), MemoryType::AcpiReclaimable);
        assert_eq!(MemoryType::from_e820(4), MemoryType::AcpiNvs);
        assert_eq!(MemoryType::from_e820(5), MemoryType::BadMemory);
        assert_eq!(MemoryType::from_e820(99), MemoryType::Reserved);
    }

    #[test]
    fn merge_adjacent_regions() {
        let raw = [
            MemoryRegion {
                base: 0x0000,
                length: 0x1000,
                region_type: MemoryType::Usable,
            },
            MemoryRegion {
                base: 0x1000,
                length: 0x1000,
                region_type: MemoryType::Usable,
            },
            MemoryRegion {
                base: 0x2000,
                length: 0x1000,
                region_type: MemoryType::Usable,
            },
        ];
        let regions = normalize(&raw);
        // All three should be merged into one.
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].base, 0);
        assert_eq!(regions[0].length, 0x3000);
    }

    #[test]
    fn empty_memory_map() {
        let pmm = Pmm::new(&[]);
        assert_eq!(pmm.total_usable(), 0);
        assert!(pmm.regions().is_empty());
    }
}
