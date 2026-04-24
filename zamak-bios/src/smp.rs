// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! SMP (Symmetric Multi-Processing) AP bring-up for the BIOS boot path.
//!
//! Parses the ACPI MADT to discover Local APICs, then sends INIT/SIPI
//! IPIs to start application processors using the trampoline code.

// Rust guideline compliant 2026-03-30

use crate::trampoline;
use crate::utils;
use alloc::boxed::Box;
use alloc::vec::Vec;
use zamak_core::protocol;

#[repr(C, packed)]
struct MadtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: [u32; 1],
    creator_revision: u32,
    lapic_addr: u32,
    flags: u32,
}

#[repr(C, packed)]
struct MadtEntryHeader {
    entry_type: u8,
    length: u8,
}

// §3.9.7: Compile-time layout verification for ACPI MADT structures.
const _: () = {
    assert!(
        core::mem::size_of::<MadtEntryHeader>() == 2,
        "MadtEntryHeader must be 2 bytes"
    );
    assert!(core::mem::offset_of!(MadtEntryHeader, entry_type) == 0);
    assert!(core::mem::offset_of!(MadtEntryHeader, length) == 1);
};

pub struct Cpu {
    pub processor_id: u8,
    pub lapic_id: u8,
    pub flags: u32,
}

/// Parses the ACPI MADT table to discover CPUs and the LAPIC base address.
///
/// Walks RSDP -> RSDT -> MADT to enumerate all Local APIC entries.
pub fn parse_madt(rsdp_addr: u64) -> (u32, Vec<Cpu>) {
    let mut cpus = Vec::new();
    let mut lapic_addr = 0;

    let rsdp = rsdp_addr as *const u8;

    // SAFETY:
    //   Preconditions: rsdp_addr points to a valid RSDP structure found by BIOS scan
    //   Postconditions: rsdt_addr contains the RSDT physical address from RSDP offset 16
    //   Clobbers: none
    //   Worst-case: reads garbage if rsdp_addr is invalid -> RSDT lookup fails
    let rsdt_addr = unsafe { *(rsdp.add(16) as *const u32) };
    let rsdt = rsdt_addr as *const u8;

    // SAFETY: rsdt points to ACPI RSDT; length at offset 4 per ACPI spec
    let rsdt_len = unsafe { *(rsdt.add(4) as *const u32) };

    let entries_count = (rsdt_len - 36) / 4;
    // SAFETY: RSDT entries start at offset 36; each is a 4-byte physical address
    let entries = unsafe { rsdt.add(36) as *const u32 };

    for i in 0..entries_count {
        // SAFETY: i < entries_count; entries array is within RSDT bounds
        let entry_addr = unsafe { *entries.add(i as usize) };
        let header = entry_addr as *const u8;
        // SAFETY: ACPI table header starts with 4-byte signature
        let sig = unsafe { core::slice::from_raw_parts(header, 4) };

        if sig == b"APIC" {
            let madt = header as *const MadtHeader;
            // SAFETY: madt points to valid MADT; lapic_addr at fixed offset in MadtHeader
            lapic_addr = unsafe { (*madt).lapic_addr };

            let mut offset = 44; // After fixed MADT header (including flags)
                                 // SAFETY: length field in MADT header
            let madt_len = unsafe { (*madt).length };

            while offset < madt_len {
                // SAFETY: offset < madt_len; entry header is 2 bytes (type + length)
                let entry = unsafe { header.add(offset as usize) as *const MadtEntryHeader };
                let entry_type = unsafe { (*entry).entry_type };
                let entry_len = unsafe { (*entry).length };

                // Type 0 = Processor Local APIC entry.
                if entry_type == 0 {
                    // SAFETY: Local APIC entry layout: +2 = proc_id, +3 = lapic_id, +4 = flags
                    let proc_id = unsafe { *header.add(offset as usize + 2) };
                    let lapic_id = unsafe { *header.add(offset as usize + 3) };
                    let flags = unsafe { *(header.add(offset as usize + 4) as *const u32) };

                    cpus.push(Cpu {
                        processor_id: proc_id,
                        lapic_id,
                        flags,
                    });
                }

                offset += entry_len as u32;
                if entry_len == 0 {
                    break;
                }
            }
            break;
        }
    }

    (lapic_addr, cpus)
}

/// Starts application processors by sending INIT + SIPI IPIs.
///
/// Copies the trampoline code to 0x1000 (within the 1 MiB real-mode
/// limit), patches the PML4 address, and sends IPIs via the LAPIC.
pub fn start_aps(lapic_addr: u32, cpus: &[Cpu], pml4: u64) -> Vec<protocol::SmpInfo> {
    let mut smp_infos = Vec::new();

    // SAFETY:
    //   Preconditions: lapic_addr is the LAPIC MMIO base from MADT
    //   Postconditions: reads BSP LAPIC ID from LAPIC ID register (offset 0x20)
    //   Clobbers: none
    //   Worst-case: reads wrong LAPIC ID if lapic_addr is invalid
    let bsp_lapic_id = unsafe { *((lapic_addr + 0x20) as *const u32) >> 24 } as u8;

    // Copy trampoline to below 1 MiB and patch PML4 address. The
    // trampoline lives in the `.trampoline` linker section (see
    // `trampoline.rs` `global_asm!`); its start/end are linker symbols.
    // SAFETY:
    //   Preconditions: `trampoline_start`/`trampoline_end` are linker
    //     symbols defined in the `.trampoline` global_asm! block;
    //     0x1000 is free memory below 1 MiB (conventional memory).
    //   Postconditions: trampoline code is at 0x1000; PML4 patched at 0x1500.
    //   Clobbers: memory at 0x1000..0x1000+len and at 0x1500.
    //   Worst-case: overwrites data if 0x1000 region is in use.
    let tramp_ptr = unsafe { &trampoline::trampoline_start as *const u8 };
    let tramp_len = trampoline::trampoline_size();
    unsafe {
        utils::memcpy(0x1000 as *mut u8, tramp_ptr, tramp_len);
        *((0x1000 + 0x500) as *mut u32) = pml4 as u32;
    }

    for (i, cpu) in cpus.iter().enumerate() {
        if cpu.lapic_id == bsp_lapic_id {
            smp_infos.push(protocol::SmpInfo {
                processor_id: i as u32,
                lapic_id: cpu.lapic_id as u32,
                ..Default::default()
            });
            continue;
        }

        // Allocate 16 KiB stack for the AP.
        let stack = alloc::vec![0u8; 16384];
        let stack_ptr = Box::leak(stack.into_boxed_slice());
        let _stack_top = stack_ptr.as_ptr() as u64 + 16384;

        // Send INIT IPI.
        send_ipi(lapic_addr, cpu.lapic_id, 0x0000_4500);

        // Delay ~10 ms (busy-wait).
        zamak_core::arch::x86::spin_wait(1_000_000);

        // Send SIPI — vector 0x01 = trampoline at 0x1000.
        send_ipi(lapic_addr, cpu.lapic_id, 0x0000_4600 | 0x01);

        smp_infos.push(protocol::SmpInfo {
            processor_id: i as u32,
            lapic_id: cpu.lapic_id as u32,
            ..Default::default()
        });
    }

    smp_infos
}

/// Sends an Inter-Processor Interrupt via the LAPIC ICR registers.
fn send_ipi(lapic_addr: u32, target_lapic_id: u8, command: u32) {
    let icr_low = (lapic_addr + 0x300) as *mut u32;
    let icr_high = (lapic_addr + 0x310) as *mut u32;

    // SAFETY:
    //   Preconditions: lapic_addr is valid LAPIC MMIO base; ICR at +0x300/+0x310
    //   Postconditions: IPI sent to target_lapic_id with given command
    //   Clobbers: LAPIC ICR registers
    //   Worst-case: sends IPI to wrong CPU if lapic_addr is wrong
    unsafe {
        // Wait for previous IPI to complete (delivery status bit 12).
        while (*icr_low & (1 << 12)) != 0 {
            zamak_core::arch::x86::pause();
        }
        *icr_high = (target_lapic_id as u32) << 24;
        *icr_low = command;
    }
}
