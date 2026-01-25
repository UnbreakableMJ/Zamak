// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::vec::Vec;

pub const START_MARKER: [u64; 4] = [
    0xf6b8f4bd9d19a43a, 0x4df92cfc92d320b6,
    0x457497127cc9f6d4, 0x5c42f0267c7e527d
];

pub const END_MARKER: [u64; 2] = [
    0x30d74613c7a38753, 0x16b04323e0ecbf77
];

pub const COMMON_MAGIC: [u64; 2] = [
    0xc7b1dd30df4c8b88, 0x0a82e88301126014
];

// Request IDs for common features
pub const BOOTLOADER_INFO_ID: [u64; 2] = [0xf55038b8e257417e, 0x5f596395b05872df];
pub const STACK_SIZE_ID: [u64; 2] = [0x224ef2cd0a8e77b2, 0x321a0293355207c5];
pub const HHDM_ID: [u64; 2] = [0x48d12d4d805f4581, 0xedc1274043b447b2];
pub const MEMMAP_ID: [u64; 2] = [0x67cf3d3d3876527d, 0xe30d74b883031260];
pub const FRAMEBUFFER_ID: [u64; 2] = [0x9d582c31e21b777a, 0x54af621df608145a];

#[repr(C)]
#[derive(Debug)]
pub struct RawRequest {
    pub magic: [u64; 2],
    pub id: [u64; 2],
    pub response: u64,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct BootloaderInfoResponse {
    pub name: u64,    // Pointer to string
    pub version: u64, // Pointer to string
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct HhdmResponse {
    pub revision: u64,
    pub offset: u64,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct MemmapResponse {
    pub revision: u64,
    pub entry_count: u64,
    pub entries: u64, // Pointer to array of MemmapEntry
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemmapEntry {
    pub base: u64,
    pub length: u64,
    pub typ: u32,
}

pub const MEMMAP_USABLE: u32 = 0;
pub const MEMMAP_RESERVED: u32 = 1;
pub const MEMMAP_ACPI_RECLAIMABLE: u32 = 2;
pub const MEMMAP_ACPI_NVS: u32 = 3;
pub const MEMMAP_BAD_MEMORY: u32 = 4;
pub const MEMMAP_BOOTLOADER_RECLAIMABLE: u32 = 5;
pub const MEMMAP_KERNEL_AND_MODULES: u32 = 6;
pub const MEMMAP_FRAMEBUFFER: u32 = 7;

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct FramebufferResponse {
    pub revision: u64,
    pub framebuffer_count: u64,
    pub framebuffers: u64, // Pointer to array of pointers to Framebuffer
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct Framebuffer {
    pub address: u64,
    pub width: u64,
    pub height: u64,
    pub pitch: u64,
    pub bpp: u16,
    pub memory_model: u8,
    pub red_mask_size: u8,
    pub red_mask_shift: u8,
    pub green_mask_size: u8,
    pub green_mask_shift: u8,
    pub blue_mask_size: u8,
    pub blue_mask_shift: u8,
    pub unused: [u8; 7],
    pub edid_size: u64,
    pub edid: u64,
}

pub fn scan_requests(kernel_bytes: &[u8]) -> Vec<*mut RawRequest> {
    let mut requests = Vec::new();
    
    // Convert markers to byte slices for comparison
    let start_bytes = unsafe { core::slice::from_raw_parts(START_MARKER.as_ptr() as *const u8, 32) };
    let end_bytes = unsafe { core::slice::from_raw_parts(END_MARKER.as_ptr() as *const u8, 16) };
    let common_magic_bytes = unsafe { core::slice::from_raw_parts(COMMON_MAGIC.as_ptr() as *const u8, 16) };

    let mut i = 0;
    while i + 32 <= kernel_bytes.len() {
        if &kernel_bytes[i..i+32] == start_bytes {
            i += 32;
            while i + 32 <= kernel_bytes.len() {
                if &kernel_bytes[i..i+16] == end_bytes {
                    return requests;
                }

                if &kernel_bytes[i..i+16] == common_magic_bytes {
                    requests.push(&kernel_bytes[i] as *const u8 as *mut RawRequest);
                }
                i += 8;
            }
        }
        i += 8;
    }

    requests
}
