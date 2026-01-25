// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{BiosRegs, call_bios_int};

pub struct Disk {
    drive_id: u8,
}

#[repr(C, packed)]
struct DiskAddressPacket {
    size: u8,
    reserved: u8,
    count: u16,
    offset: u16,
    segment: u16,
    lba: u64,
}

impl Disk {
    pub fn new(drive_id: u8) -> Self {
        Self { drive_id }
    }

    pub unsafe fn read_sectors(&self, lba: u64, count: u16, buffer_addr: u32) -> Result<(), u8> {
        let dap = DiskAddressPacket {
            size: 0x10,
            reserved: 0,
            count,
            offset: (buffer_addr & 0xFFFF) as u16,
            segment: ((buffer_addr >> 4) & 0xF000) as u16, // This is wrong for protected mode buffer addressing, but BIOS needs segment:offset
            lba,
        };
        
        // Wait, BIOS int 0x13/42h needs the DAP in memory and its pointer in SI.
        // We are in protected mode. Our bridge transitions to real mode.
        // The DAP must be in Real Mode accessible memory (<1MB).
        // 0x7000 is our RM stack. Let's use 0x6000 for DAP.
        
        let dap_ptr = 0x6000 as *mut DiskAddressPacket;
        *dap_ptr = dap;

        let mut regs = BiosRegs::default();
        regs.eax = 0x4200; // Extended Read
        regs.edx = self.drive_id as u32;
        regs.esi = 0x6000; // DS:SI points to DAP. Bridge sets DS=0.

        call_bios_int(0x13, &mut regs);

        if (regs.eax >> 8) & 0xFF != 0 {
            return Err((regs.eax >> 8) as u8);
        }

        Ok(())
    }
}
