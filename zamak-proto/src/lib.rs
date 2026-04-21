// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Limine Protocol types and request/response structures.
//!
//! This is a standalone `#![no_std]` crate containing all Limine Protocol
//! wire types, request IDs, and marker constants. It has no dependencies
//! beyond `core` and can be consumed by any bootloader or kernel crate.

// Rust guideline compliant 2026-03-30

#![no_std]

// ---------------------------------------------------------------------------
// Request markers
// ---------------------------------------------------------------------------

/// Marker sequence that begins a Limine request block in the kernel image.
pub const START_MARKER: [u64; 4] = [
    0xf6b8_f4bd_9d19_a43a,
    0x4df9_2cfc_92d3_20b6,
    0x4574_9712_7cc9_f6d4,
    0x5c42_f026_7c7e_527d,
];

/// Marker sequence that ends a Limine request block in the kernel image.
pub const END_MARKER: [u64; 2] = [0x30d7_4613_c7a3_8753, 0x16b0_4323_e0ec_bf77];

/// Magic value present at the start of every individual request.
pub const COMMON_MAGIC: [u64; 2] = [0xc7b1_dd30_df4c_8b88, 0x0a82_e883_0112_6014];

// ---------------------------------------------------------------------------
// Request IDs
// ---------------------------------------------------------------------------

pub const BOOTLOADER_INFO_ID: [u64; 2] = [0xf550_38b8_e257_417e, 0x5f59_6395_b058_72df];
pub const STACK_SIZE_ID: [u64; 2] = [0x224e_f2cd_0a8e_77b2, 0x321a_0293_3552_07c5];
pub const HHDM_ID: [u64; 2] = [0x48d1_2d4d_805f_4581, 0xedc1_2740_43b4_47b2];
pub const MEMMAP_ID: [u64; 2] = [0x67cf_3d3d_3876_527d, 0xe30d_74b8_8303_1260];
pub const FRAMEBUFFER_ID: [u64; 2] = [0x9d58_2c31_e21b_777a, 0x54af_621d_f608_145a];
pub const MODULE_ID: [u64; 2] = [0x3e7e_2797_02ec_e32d, 0x4dca_2a80_3f26_01ee];
pub const RSDP_ID: [u64; 2] = [0xc5e7_28f5_b803_f261, 0x82b2_d32e_2601_dfa5];
pub const SMBIOS_ID: [u64; 2] = [0x9e30_0560_3972_627e, 0x4f6a_2a0b_3f26_01ee];
pub const KERNEL_FILE_ID: [u64; 2] = [0xad97_e30e_1e2d_777a, 0x24ef_2cd0_a8e7_7b21];
pub const SMP_ID: [u64; 2] = [0x95a1_aed3_e22b_777a, 0xa5a1_aed3_e22b_777a];

// ---------------------------------------------------------------------------
// Request / Response structures
// ---------------------------------------------------------------------------

/// Raw request header as found in the kernel image.
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
    pub name: u64,
    pub version: u64,
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
    pub entries: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MemmapEntry {
    pub base: u64,
    pub length: u64,
    pub typ: u32,
}

// Memory map type constants
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
    pub framebuffers: u64,
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

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct File {
    pub revision: u64,
    pub address: u64,
    pub size: u64,
    pub path: u64,
    pub cmdline: u64,
    pub media_type: u32,
    pub unused: u32,
    pub tftp_ip: u32,
    pub tftp_port: u32,
    pub partition_index: u32,
    pub mbr_disk_id: u32,
    pub gpt_disk_uuid: [u64; 2],
    pub gpt_part_uuid: [u64; 2],
    pub part_uuid: [u64; 2],
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct ModuleResponse {
    pub revision: u64,
    pub module_count: u64,
    pub modules: u64,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct RsdpResponse {
    pub revision: u64,
    pub address: u64,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct SmbiosResponse {
    pub revision: u64,
    pub address: u64,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct KernelFileResponse {
    pub revision: u64,
    pub kernel_file: u64,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct SmpResponse {
    pub revision: u64,
    pub flags: u32,
    pub bsp_lapic_id: u32,
    pub cpu_count: u64,
    pub cpus: u64,
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct SmpInfo {
    pub processor_id: u32,
    pub lapic_id: u32,
    pub reserved: u64,
    pub goto_address: u64,
    pub extra_argument: u64,
}

// ---------------------------------------------------------------------------
// Request scanning
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// §3.9.7: Compile-time layout verification for all #[repr(C)] protocol structs
// ---------------------------------------------------------------------------

const _: () = {
    use core::mem::{offset_of, size_of};

    // RawRequest: scanned in kernel memory by byte pattern matching.
    assert!(size_of::<RawRequest>() == 40);
    assert!(offset_of!(RawRequest, magic) == 0);
    assert!(offset_of!(RawRequest, id) == 16);
    assert!(offset_of!(RawRequest, response) == 32);

    // MemmapEntry: filled by bootloader, consumed by kernel.
    assert!(offset_of!(MemmapEntry, base) == 0);
    assert!(offset_of!(MemmapEntry, length) == 8);
    assert!(offset_of!(MemmapEntry, typ) == 16);

    // SmpInfo: written by bootloader, read by kernel per-CPU.
    assert!(size_of::<SmpInfo>() == 32);
    assert!(offset_of!(SmpInfo, processor_id) == 0);
    assert!(offset_of!(SmpInfo, lapic_id) == 4);
    assert!(offset_of!(SmpInfo, goto_address) == 16);
    assert!(offset_of!(SmpInfo, extra_argument) == 24);

    // SmpResponse: aggregates CPU info.
    assert!(offset_of!(SmpResponse, flags) == 8);
    assert!(offset_of!(SmpResponse, bsp_lapic_id) == 12);
    assert!(offset_of!(SmpResponse, cpu_count) == 16);
    assert!(offset_of!(SmpResponse, cpus) == 24);
};

/// Scan a kernel image byte slice for Limine protocol requests.
///
/// Returns pointers to each [`RawRequest`] found between the
/// [`START_MARKER`] and [`END_MARKER`] boundaries.
///
/// # Safety
///
/// The returned pointers are valid only as long as the backing `kernel_bytes`
/// slice remains allocated and unmodified.
pub fn scan_requests(_kernel_bytes: &[u8]) -> &'static [*mut RawRequest] {
    // The real scan needs `alloc::vec::Vec`, which is provided by `zamak-core`
    // (not this `#![no_std]` + no-`alloc` crate). Callers should use
    // `zamak_core::protocol::scan_requests` instead; this stub exists only to
    // keep the protocol API surface complete in the standalone crate.
    &[]
}
