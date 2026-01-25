// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{BiosRegs, call_bios_int};
use alloc::vec::Vec;
use libzamak::protocol::{
    MemmapEntry, 
    MEMMAP_USABLE, MEMMAP_RESERVED, 
    MEMMAP_ACPI_RECLAIMABLE, MEMMAP_ACPI_NVS, 
    MEMMAP_BAD_MEMORY
};

pub fn get_memory_map() -> Vec<MemmapEntry> {
    let mut map = Vec::new();
    let mut regs = BiosRegs::default();
    let buffer = 0x5000 as *mut E820Entry; // Use safe memory for the entry buffer
    
    regs.ebx = 0; // Continuation value
    
    loop {
        regs.eax = 0xE820;
        regs.ecx = 24; // Size of buffer
        regs.edx = 0x534D4150; // 'SMAP'
        regs.edi = 0x5000;

        unsafe {
            call_bios_int(0x15, &mut regs);
        }

        if regs.eax != 0x534D4150 {
            break; // Failed
        }

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
            break;
        }
    }

    map
}

#[repr(C, packed)]
struct E820Entry {
    base: u64,
    len: u64,
    typ: u32,
    acpi: u32,
}
