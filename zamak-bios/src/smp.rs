// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::boxed::Box;
use alloc::vec::Vec;
use crate::utils;
use libzamak::protocol;

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

pub struct Cpu {
    pub processor_id: u8,
    pub lapic_id: u8,
    pub flags: u32,
}

pub fn parse_madt(rsdp_addr: u64) -> (u32, Vec<Cpu>) {
    let mut cpus = Vec::new();
    let mut lapic_addr = 0;

    // Minimal RSDP/XSDT parsing
    // For BIOS, we assume RSDP v1 and RSDT
    let rsdp = rsdp_addr as *const u8;
    let rsdt_addr = unsafe { *(rsdp.add(16) as *const u32) };
    let rsdt = rsdt_addr as *const u8;
    let rsdt_len = unsafe { *(rsdt.add(4) as *const u32) };
    
    let entries_count = (rsdt_len - 36) / 4;
    let entries = unsafe { rsdt.add(36) as *const u32 };
    
    for i in 0..entries_count {
        let entry_addr = unsafe { *entries.add(i as usize) };
        let header = entry_addr as *const u8;
        let sig = unsafe { core::slice::from_raw_parts(header, 4) };
        
        if sig == b"APIC" {
            let madt = header as *const MadtHeader;
            lapic_addr = unsafe { (*madt).lapic_addr };
            
            let mut offset = 44; // After flags
            let madt_len = unsafe { (*madt).length };
            
            while offset < madt_len {
                let entry = unsafe { header.add(offset as usize) as *const MadtEntryHeader };
                let entry_type = unsafe { (*entry).entry_type };
                let entry_len = unsafe { (*entry).length };
                
                if entry_type == 0 { // Processor Local APIC
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
                if entry_len == 0 { break; }
            }
            break;
        }
    }
    
    (lapic_addr, cpus)
}

pub fn start_aps(lapic_addr: u32, cpus: &[Cpu], pml4: u64) -> Vec<protocol::SmpInfo> {
    let mut smp_infos = Vec::new();
    let bsp_lapic_id = unsafe { *((lapic_addr + 0x20) as *const u32) >> 24 } as u8;

    // Load Trampoline
    let trampoline_bytes = include_bytes!("../../trampoline.bin");
    unsafe {
        utils::memcpy(0x1000 as *mut u8, trampoline_bytes.as_ptr(), trampoline_bytes.len());
        // Patch trampoline with PML4 at offset 0x500
        *((0x1000 + 0x500) as *mut u32) = pml4 as u32;
    }

    // Allocate an array of SmpInfo pointers that the trampoline might need 
    // (Though usually kernel does this)
    
    for (i, cpu) in cpus.iter().enumerate() {
        if cpu.lapic_id == bsp_lapic_id {
            smp_infos.push(protocol::SmpInfo {
                processor_id: i as u32,
                lapic_id: cpu.lapic_id as u32,
                ..Default::default()
            });
            continue;
        }

        // Allocate stack for AP
        let stack = alloc::vec![0u8; 16384]; // 16KB stack
        let stack_ptr = Box::leak(stack.into_boxed_slice());
        let _stack_top = stack_ptr.as_ptr() as u64 + 16384;

        // In a real implementation, we'd need to pass the stack_top to each AP individually
        // Since they all use the same trampoline bin, we can use a "boot_info" struct at 0x1000 + 0x600 
        // that's unique for each SIPI, or just let the kernel handle it later (Limine style)
        
        // Send INIT IPI
        send_ipi(lapic_addr, cpu.lapic_id, 0x0000_4500);
        
        // Wait 10ms
        for _ in 0..1000000 { unsafe { core::arch::asm!("pause"); } }
        
        // Send SIPI
        send_ipi(lapic_addr, cpu.lapic_id, 0x0000_4600 | 0x01); 
        
        smp_infos.push(protocol::SmpInfo {
            processor_id: i as u32,
            lapic_id: cpu.lapic_id as u32,
            ..Default::default()
        });
    }

    smp_infos
}

fn send_ipi(lapic_addr: u32, target_lapic_id: u8, command: u32) {
    let icr_low = (lapic_addr + 0x300) as *mut u32;
    let icr_high = (lapic_addr + 0x310) as *mut u32;
    
    unsafe {
        while (*icr_low & (1 << 12)) != 0 { core::arch::asm!("pause"); }
        *icr_high = (target_lapic_id as u32) << 24;
        *icr_low = command;
    }
}
