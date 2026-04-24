// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! BIOS disk I/O via INT 13h Extended Read.
//!
//! Uses a bounce buffer at 0x2000 (below 1 MiB) because BIOS INT 13h
//! requires real-mode-addressable memory for the data buffer.

// Rust guideline compliant 2026-03-30

use crate::{call_bios_int, BiosRegs};

/// Bounce buffer physical address for BIOS disk reads.
///
/// Must be below 1 MiB and not overlap with other low-memory structures
/// (IVT at 0x0, BDA at 0x400, E820 buffer at 0x5000, DAP at 0x6000,
/// real-mode stack at 0x7000, stage2 entry at 0x8000).
const BOUNCE_BUFFER_ADDR: u32 = 0x2000;

/// Disk Address Packet physical address for INT 13h.
const DAP_ADDR: u32 = 0x6000;

/// Maximum sectors per bounce-buffer read (4096 / 512 = 8).
const MAX_SECTORS_PER_READ: usize = 8;

#[derive(Clone, Copy)]
pub struct Disk {
    drive_id: u8,
}

/// INT 13h Extended Read Disk Address Packet (DAP).
#[repr(C, packed)]
struct DiskAddressPacket {
    size: u8,
    reserved: u8,
    count: u16,
    offset: u16,
    segment: u16,
    lba: u64,
}

// §3.9.7: Compile-time layout verification for INT 13h DAP struct.
const _: () = {
    assert!(
        core::mem::size_of::<DiskAddressPacket>() == 16,
        "DAP must be 16 bytes"
    );
    assert!(core::mem::offset_of!(DiskAddressPacket, size) == 0);
    assert!(core::mem::offset_of!(DiskAddressPacket, count) == 2);
    assert!(core::mem::offset_of!(DiskAddressPacket, offset) == 4);
    assert!(core::mem::offset_of!(DiskAddressPacket, segment) == 6);
    assert!(core::mem::offset_of!(DiskAddressPacket, lba) == 8);
};

impl Disk {
    pub fn new(drive_id: u8) -> Self {
        Self { drive_id }
    }

    /// Reads sectors using INT 13h Extended Read (AH=42h).
    ///
    /// # Safety
    ///
    /// `buffer_addr` must be a valid real-mode-addressable physical address
    /// with enough space for `count * 512` bytes.
    pub unsafe fn read_sectors_internal(
        &self,
        lba: u64,
        count: u16,
        buffer_addr: u32,
    ) -> Result<(), u8> {
        let dap = DiskAddressPacket {
            size: 0x10,
            reserved: 0,
            count,
            offset: (buffer_addr & 0xFFFF) as u16,
            segment: ((buffer_addr >> 4) & 0xF000) as u16,
            lba,
        };

        // SAFETY:
        //   Preconditions: DAP_ADDR (0x6000) is free conventional memory
        //   Postconditions: DAP structure written at 0x6000 for INT 13h
        //   Clobbers: memory at 0x6000..0x6010
        //   Worst-case: overwrites data if 0x6000 is in use
        let dap_ptr = DAP_ADDR as *mut DiskAddressPacket;
        unsafe {
            *dap_ptr = dap;
        }

        let mut regs = BiosRegs::default();
        regs.eax = 0x4200; // Extended Read.
        regs.edx = self.drive_id as u32;
        regs.esi = DAP_ADDR; // DS:SI -> DAP.

        // Serial breadcrumbs: '[' before the 32→real→32 mode round-trip,
        // ']' after. If we see '[' but not ']' during M1-16 bring-up the
        // `call_bios_int` trampoline is the culprit.
        crate::mark(b'[');
        // SAFETY:
        //   Preconditions: DAP at 0x6000 is valid; drive_id is the BIOS boot drive
        //   Postconditions: sectors read to buffer_addr; regs updated with status
        //   Clobbers: regs, memory at buffer_addr..buffer_addr+(count*512)
        //   Worst-case: BIOS returns error code in AH if read fails
        unsafe {
            call_bios_int(0x13, &mut regs);
        }
        crate::mark(b']');
        let ah = ((regs.eax >> 8) & 0xFF) as u8;
        if ah != 0 {
            // Emit two hex digits of AH to serial so the bring-up log
            // shows which BIOS error fired instead of a silent panic.
            let hi = ah >> 4;
            let lo = ah & 0x0F;
            crate::mark(b'!');
            crate::mark(if hi < 10 { b'0' + hi } else { b'A' + (hi - 10) });
            crate::mark(if lo < 10 { b'0' + lo } else { b'A' + (lo - 10) });
            return Err(ah);
        }
        crate::mark(b'.'); // successful read
        Ok(())
    }
}

use zamak_core::fs::{BlockDevice, Error};

impl BlockDevice for Disk {
    fn read_sectors(
        &self,
        start_sector: u64,
        count: usize,
        buffer: &mut [u8],
    ) -> Result<(), Error> {
        let bounce_buffer = BOUNCE_BUFFER_ADDR as *mut u8;
        let mut sectors_read = 0;
        let mut current_lba = start_sector;

        while sectors_read < count {
            let chunk = core::cmp::min(count - sectors_read, MAX_SECTORS_PER_READ);

            // SAFETY:
            //   Preconditions: BOUNCE_BUFFER_ADDR is free; chunk <= 8 (fits in 4 KiB)
            //   Postconditions: data at bounce buffer copied to caller's buffer
            //   Clobbers: memory at BOUNCE_BUFFER_ADDR..+chunk*512, buffer slice
            //   Worst-case: read_sectors_internal fails -> IoError returned
            unsafe {
                if self
                    .read_sectors_internal(current_lba, chunk as u16, BOUNCE_BUFFER_ADDR)
                    .is_err()
                {
                    return Err(Error::IoError);
                }
                let dest_ptr = buffer.as_mut_ptr().add(sectors_read * 512);
                core::ptr::copy_nonoverlapping(bounce_buffer, dest_ptr, chunk * 512);
            }

            sectors_read += chunk;
            current_lba += chunk as u64;
        }

        Ok(())
    }
}
