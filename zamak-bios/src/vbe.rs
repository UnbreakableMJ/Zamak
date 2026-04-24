// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

use crate::boot_bundle::VbeModeInfo;
use crate::{call_bios_int, BiosRegs};
use zamak_core::protocol::Framebuffer;

#[repr(C, packed)]
pub struct VbeInfoBlock {
    pub signature: [u8; 4], // "VESA"
    pub version: u16,
    pub oem_ptr: u32, // segment:offset
    pub capabilities: u32,
    pub video_mode_ptr: u32, // segment:offset
    pub total_memory: u16,   // in 64kb blocks
    pub oem_software_rev: u16,
    pub oem_vendor_name_ptr: u32,
    pub oem_product_name_ptr: u32,
    pub oem_product_rev_ptr: u32,
    pub reserved: [u8; 222],
    pub oem_data: [u8; 256],
}

// VbeModeInfo moved to `boot_bundle::VbeModeInfo` so the Path B
// real-mode orchestration can embed it in the BootDataBundle and
// the legacy vbe.rs path can still consume the same bytes.

pub fn find_and_set_vbe_mode(
    target_width: u16,
    target_height: u16,
    target_bpp: u8,
) -> Option<Framebuffer> {
    let info_ptr = 0x7000 as *mut VbeInfoBlock;
    let mode_ptr = 0x7200 as *mut VbeModeInfo;

    unsafe {
        core::ptr::write_bytes(info_ptr, 0, 1);
        core::ptr::copy_nonoverlapping(b"VBE2".as_ptr(), info_ptr as *mut u8, 4);
    }

    let mut regs = BiosRegs::default();
    regs.eax = 0x4F00;
    regs.edi = 0x7000;

    unsafe {
        call_bios_int(0x10, &mut regs);
    }

    if regs.eax != 0x004F {
        return None;
    }

    let info = unsafe { &*info_ptr };
    if &info.signature != b"VESA" {
        return None;
    }

    // Iterate modes
    let mut mode_idx_ptr = real_to_flat(info.video_mode_ptr) as *const u16;

    while unsafe { *mode_idx_ptr } != 0xFFFF {
        let mode = unsafe { *mode_idx_ptr };
        mode_idx_ptr = unsafe { mode_idx_ptr.offset(1) };

        let mut mregs = BiosRegs::default();
        mregs.eax = 0x4F01;
        mregs.ecx = mode as u32;
        mregs.edi = 0x7200;

        unsafe {
            call_bios_int(0x10, &mut mregs);
        }

        if mregs.eax != 0x004F {
            continue;
        }

        let minfo = unsafe { &*mode_ptr };

        // Check if mode matches
        // Attributes bit 7: Linear Framebuffer
        // Attributes bit 0: Mode supported by hardware
        if (minfo.attributes & 0x81) != 0x81 {
            continue;
        }

        if minfo.width == target_width && minfo.height == target_height && minfo.bpp == target_bpp {
            // Found it! Set mode
            let mut sregs = BiosRegs::default();
            sregs.eax = 0x4F02;
            sregs.ebx = (mode as u32) | 0x4000; // Bit 14 for LFB

            unsafe {
                call_bios_int(0x10, &mut sregs);
            }

            if sregs.eax == 0x004F {
                let mut fb = Framebuffer::default();
                fb.address = minfo.phys_base as u64;
                fb.width = minfo.width as u64;
                fb.height = minfo.height as u64;
                fb.pitch = minfo.pitch as u64;
                fb.bpp = minfo.bpp as u16;
                fb.memory_model = 1; // RGB
                fb.red_mask_size = minfo.red_mask_size;
                fb.red_mask_shift = minfo.red_field_position;
                fb.green_mask_size = minfo.green_mask_size;
                fb.green_mask_shift = minfo.green_field_position;
                fb.blue_mask_size = minfo.blue_mask_size;
                fb.blue_mask_shift = minfo.blue_field_position;

                return Some(fb);
            }
        }
    }

    None
}

fn real_to_flat(ptr: u32) -> u32 {
    let segment = (ptr >> 16) & 0xFFFF;
    let offset = ptr & 0xFFFF;
    (segment << 4) + offset
}

impl Default for VbeInfoBlock {
    fn default() -> Self {
        unsafe { core::mem::zeroed() }
    }
}
