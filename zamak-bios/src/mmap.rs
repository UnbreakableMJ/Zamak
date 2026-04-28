// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! BIOS E820 memory map enumeration.
//!
//! Calls INT 15h, AX=E820h repeatedly to build the physical memory map,
//! then converts E820 types to Limine Protocol memory map types.

// Rust guideline compliant 2026-03-30

use crate::boot_bundle::E820Entry;
use crate::{call_bios_int, BiosRegs};
use alloc::vec::Vec;
use zamak_core::protocol::{
    MemmapEntry, MEMMAP_ACPI_NVS, MEMMAP_ACPI_RECLAIMABLE, MEMMAP_BAD_MEMORY, MEMMAP_RESERVED,
    MEMMAP_USABLE,
};

/// SMAP signature returned in EAX on success ('SMAP' in little-endian).
const SMAP_MAGIC: u32 = 0x534D_4150;

/// Physical address used as the E820 entry buffer.
///
/// 0x5000 is in conventional memory, safely below the stage2 load address
/// and above the IVT/BDA region.
const E820_BUFFER_ADDR: u32 = 0x5000;

/// Enumerates the physical memory map using BIOS INT 15h, E820h.
pub fn get_memory_map() -> Vec<MemmapEntry> {
    let mut map = Vec::new();
    let mut regs = BiosRegs::default();
    let buffer = E820_BUFFER_ADDR as *mut E820Entry;

    regs.ebx = 0; // Continuation value — 0 starts enumeration.

    loop {
        regs.eax = 0xE820;
        regs.ecx = 24; // Size of E820 entry buffer.
        regs.edx = SMAP_MAGIC;
        regs.edi = E820_BUFFER_ADDR;

        // SAFETY:
        //   Preconditions: regs is valid; INT 15h E820 writes to ES:DI (= 0x5000)
        //   Postconditions: regs.eax = SMAP on success; entry written at buffer
        //   Clobbers: regs (output), memory at 0x5000..0x5018
        //   Worst-case: BIOS returns error (eax != SMAP) -> loop breaks
        unsafe {
            call_bios_int(0x15, &mut regs);
        }

        if regs.eax != SMAP_MAGIC {
            break;
        }

        // SAFETY: buffer points to E820_BUFFER_ADDR where INT 15h wrote the entry
        let entry = unsafe { &*buffer };
        let entry_type = match entry.typ {
            1 => MEMMAP_USABLE,
            2 => MEMMAP_RESERVED,
            3 => MEMMAP_ACPI_RECLAIMABLE,
            4 => MEMMAP_ACPI_NVS,
            5 => MEMMAP_BAD_MEMORY,
            _ => MEMMAP_RESERVED,
        };

        map.push(MemmapEntry {
            base: entry.base,
            length: entry.len,
            typ: entry_type,
        });

        if regs.ebx == 0 {
            break; // Last entry.
        }
    }

    map
}
// E820Entry moved to `boot_bundle::E820Entry` so both the legacy
// protected-mode path and the Path B real-mode orchestration share
// the same BIOS-layout record.
