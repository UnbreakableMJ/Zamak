// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Multiboot 1 protocol implementation (FR-PROTO-003).
//!
//! Provides types and functions for loading Multiboot 1 compliant kernels.
//! The Multiboot 1 specification defines a header embedded in the kernel image
//! and an information structure passed to the kernel at boot.
//!
//! # References
//!
//! - [Multiboot 1 Specification](https://www.gnu.org/software/grub/manual/multiboot/multiboot.html)

// Rust guideline compliant 2026-03-30

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(test)]
use alloc::vec;

/// Multiboot 1 header magic number.
pub const MULTIBOOT_HEADER_MAGIC: u32 = 0x1BADB002;

/// Multiboot 1 bootloader magic passed in EAX to the kernel.
pub const MULTIBOOT_BOOTLOADER_MAGIC: u32 = 0x2BADB002;

// Header flag bits.

/// If set, align all boot modules on page (4 KiB) boundaries.
pub const MULTIBOOT_PAGE_ALIGN: u32 = 1 << 0;

/// If set, include memory information in the info structure.
pub const MULTIBOOT_MEMORY_INFO: u32 = 1 << 1;

/// If set, include video mode information.
pub const MULTIBOOT_VIDEO_MODE: u32 = 1 << 2;

/// If set, use the address fields in the header instead of ELF info.
pub const MULTIBOOT_AOUT_KLUDGE: u32 = 1 << 16;

// Info flag bits (set by the bootloader in `MultibootInfo::flags`).

/// `mem_lower` and `mem_upper` are valid.
pub const MULTIBOOT_INFO_MEMORY: u32 = 1 << 0;

/// `boot_device` is valid.
pub const MULTIBOOT_INFO_BOOTDEV: u32 = 1 << 1;

/// `cmdline` is valid.
pub const MULTIBOOT_INFO_CMDLINE: u32 = 1 << 2;

/// `mods_count` and `mods_addr` are valid.
pub const MULTIBOOT_INFO_MODS: u32 = 1 << 3;

/// a.out symbol table is valid (mutually exclusive with bit 5).
pub const MULTIBOOT_INFO_AOUT_SYMS: u32 = 1 << 4;

/// ELF section header table is valid (mutually exclusive with bit 4).
pub const MULTIBOOT_INFO_ELF_SHDR: u32 = 1 << 5;

/// `mmap_length` and `mmap_addr` are valid.
pub const MULTIBOOT_INFO_MEM_MAP: u32 = 1 << 6;

/// `drives_length` and `drives_addr` are valid.
pub const MULTIBOOT_INFO_DRIVE_INFO: u32 = 1 << 7;

/// `config_table` is valid.
pub const MULTIBOOT_INFO_CONFIG_TABLE: u32 = 1 << 8;

/// `boot_loader_name` is valid.
pub const MULTIBOOT_INFO_BOOT_LOADER_NAME: u32 = 1 << 9;

/// `apm_table` is valid.
pub const MULTIBOOT_INFO_APM_TABLE: u32 = 1 << 10;

/// `vbe_*` fields are valid.
pub const MULTIBOOT_INFO_VBE_INFO: u32 = 1 << 11;

/// `framebuffer_*` fields are valid.
pub const MULTIBOOT_INFO_FRAMEBUFFER_INFO: u32 = 1 << 12;

/// Multiboot 1 header found in the kernel image.
///
/// Must appear within the first 8192 bytes of the kernel, aligned to 4 bytes.
/// The checksum field ensures `magic + flags + checksum == 0` (mod 2^32).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MultibootHeader {
    pub magic: u32,
    pub flags: u32,
    pub checksum: u32,
    // Address fields (only valid when MULTIBOOT_AOUT_KLUDGE is set).
    pub header_addr: u32,
    pub load_addr: u32,
    pub load_end_addr: u32,
    pub bss_end_addr: u32,
    pub entry_addr: u32,
    // Video mode fields (only valid when MULTIBOOT_VIDEO_MODE is set).
    pub mode_type: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}

/// Multiboot 1 boot information structure passed to the kernel.
///
/// The bootloader allocates this structure and passes its physical address
/// in EBX when jumping to the kernel entry point.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MultibootInfo {
    pub flags: u32,
    pub mem_lower: u32,
    pub mem_upper: u32,
    pub boot_device: u32,
    pub cmdline: u32,
    pub mods_count: u32,
    pub mods_addr: u32,
    // Symbols: either a.out or ELF (union in C, we use ELF variant).
    pub syms: [u32; 4],
    pub mmap_length: u32,
    pub mmap_addr: u32,
    pub drives_length: u32,
    pub drives_addr: u32,
    pub config_table: u32,
    pub boot_loader_name: u32,
    pub apm_table: u32,
    // VBE fields.
    pub vbe_control_info: u32,
    pub vbe_mode_info: u32,
    pub vbe_mode: u16,
    pub vbe_interface_seg: u16,
    pub vbe_interface_off: u16,
    pub vbe_interface_len: u16,
    // Framebuffer fields.
    pub framebuffer_addr: u64,
    pub framebuffer_pitch: u32,
    pub framebuffer_width: u32,
    pub framebuffer_height: u32,
    pub framebuffer_bpp: u8,
    pub framebuffer_type: u8,
    // Color info (6 bytes, type-dependent).
    pub color_info: [u8; 6],
}

/// A Multiboot 1 module descriptor.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MultibootModule {
    /// Physical start address of the module.
    pub mod_start: u32,
    /// Physical end address of the module (exclusive).
    pub mod_end: u32,
    /// Physical address of a NUL-terminated ASCII string.
    pub string: u32,
    /// Reserved, must be zero.
    pub reserved: u32,
}

/// A Multiboot 1 memory map entry.
///
/// Note: the `size` field is *not* part of the entry itself; it precedes
/// each entry in the memory map and gives the size of the remaining fields.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MultibootMmapEntry {
    pub size: u32,
    pub addr: u64,
    pub len: u64,
    pub entry_type: u32,
}

/// Memory map entry types.
pub const MULTIBOOT_MEMORY_AVAILABLE: u32 = 1;
pub const MULTIBOOT_MEMORY_RESERVED: u32 = 2;
pub const MULTIBOOT_MEMORY_ACPI_RECLAIMABLE: u32 = 3;
pub const MULTIBOOT_MEMORY_NVS: u32 = 4;
pub const MULTIBOOT_MEMORY_BADRAM: u32 = 5;

/// Maximum number of bytes to search for the Multiboot header.
const MULTIBOOT_SEARCH_LIMIT: usize = 8192;

/// Scans a kernel image for the Multiboot 1 header.
///
/// The header must appear within the first 8192 bytes of the kernel,
/// aligned to a 4-byte boundary. Returns the byte offset of the header
/// if found.
pub fn find_header(kernel: &[u8]) -> Option<usize> {
    let search_end = kernel.len().min(MULTIBOOT_SEARCH_LIMIT);
    if search_end < 12 {
        return None;
    }

    let mut offset = 0;
    while offset + 12 <= search_end {
        let magic = u32::from_le_bytes(kernel[offset..offset + 4].try_into().ok()?);
        if magic == MULTIBOOT_HEADER_MAGIC {
            let flags = u32::from_le_bytes(kernel[offset + 4..offset + 8].try_into().ok()?);
            let checksum = u32::from_le_bytes(kernel[offset + 8..offset + 12].try_into().ok()?);
            // Verify: magic + flags + checksum must equal 0 (mod 2^32).
            if magic.wrapping_add(flags).wrapping_add(checksum) == 0 {
                return Some(offset);
            }
        }
        offset += 4; // 4-byte aligned search.
    }

    None
}

/// Parses the Multiboot 1 header from a kernel image at the given offset.
///
/// Returns `None` if there are not enough bytes for a full header.
pub fn parse_header(kernel: &[u8], offset: usize) -> Option<MultibootHeader> {
    let header_size = core::mem::size_of::<MultibootHeader>();
    if offset + header_size > kernel.len() {
        // If the kernel doesn't have the full extended header, parse the
        // mandatory 12 bytes (magic + flags + checksum) plus available fields.
        if offset + 12 > kernel.len() {
            return None;
        }
    }

    let available = kernel.len() - offset;
    let copy_len = available.min(header_size);
    let mut buf = [0u8; 48]; // size_of::<MultibootHeader>() == 48
    buf[..copy_len].copy_from_slice(&kernel[offset..offset + copy_len]);

    // SAFETY: MultibootHeader is #[repr(C, packed)] and all fields are
    // plain integers. Any bit pattern is valid.
    let header: MultibootHeader = unsafe { core::ptr::read_unaligned(buf.as_ptr().cast()) };

    Some(header)
}

/// Information needed to build the Multiboot info structure.
pub struct MultibootBootInfo {
    pub mem_lower_kb: u32,
    pub mem_upper_kb: u32,
    pub cmdline: String,
    pub boot_loader_name: String,
    pub modules: Vec<ModuleInfo>,
    pub mmap: Vec<MmapRegion>,
    pub framebuffer: Option<FramebufferInfo>,
}

/// A module to pass to the Multiboot kernel.
pub struct ModuleInfo {
    pub start: u32,
    pub end: u32,
    pub cmdline: String,
}

/// A memory map region.
#[derive(Debug, Clone, Copy)]
pub struct MmapRegion {
    pub addr: u64,
    pub len: u64,
    pub region_type: u32,
}

/// Framebuffer information.
#[derive(Debug, Clone, Copy)]
pub struct FramebufferInfo {
    pub addr: u64,
    pub pitch: u32,
    pub width: u32,
    pub height: u32,
    pub bpp: u8,
}

/// Builds a `MultibootInfo` structure from the given boot information.
///
/// The caller must allocate physical memory for the info structure, the
/// command line string, boot loader name, module array, and memory map.
/// This function populates the fields and returns the info structure with
/// flags set appropriately.
pub fn build_info(boot: &MultibootBootInfo) -> MultibootInfo {
    let mut info = MultibootInfo {
        flags: 0,
        mem_lower: 0,
        mem_upper: 0,
        boot_device: 0,
        cmdline: 0,
        mods_count: 0,
        mods_addr: 0,
        syms: [0; 4],
        mmap_length: 0,
        mmap_addr: 0,
        drives_length: 0,
        drives_addr: 0,
        config_table: 0,
        boot_loader_name: 0,
        apm_table: 0,
        vbe_control_info: 0,
        vbe_mode_info: 0,
        vbe_mode: 0,
        vbe_interface_seg: 0,
        vbe_interface_off: 0,
        vbe_interface_len: 0,
        framebuffer_addr: 0,
        framebuffer_pitch: 0,
        framebuffer_width: 0,
        framebuffer_height: 0,
        framebuffer_bpp: 0,
        framebuffer_type: 0,
        color_info: [0; 6],
    };

    // Memory info.
    info.flags |= MULTIBOOT_INFO_MEMORY;
    info.mem_lower = boot.mem_lower_kb;
    info.mem_upper = boot.mem_upper_kb;

    // Modules.
    if !boot.modules.is_empty() {
        info.flags |= MULTIBOOT_INFO_MODS;
        info.mods_count = boot.modules.len() as u32;
        // mods_addr must be set by the caller after allocating the module array.
    }

    // Memory map.
    if !boot.mmap.is_empty() {
        info.flags |= MULTIBOOT_INFO_MEM_MAP;
        // mmap_addr and mmap_length set by caller after allocating entries.
    }

    // Boot loader name.
    info.flags |= MULTIBOOT_INFO_BOOT_LOADER_NAME;
    // boot_loader_name pointer set by caller.

    // Command line.
    if !boot.cmdline.is_empty() {
        info.flags |= MULTIBOOT_INFO_CMDLINE;
        // cmdline pointer set by caller.
    }

    // Framebuffer.
    if let Some(fb) = boot.framebuffer {
        info.flags |= MULTIBOOT_INFO_FRAMEBUFFER_INFO;
        info.framebuffer_addr = fb.addr;
        info.framebuffer_pitch = fb.pitch;
        info.framebuffer_width = fb.width;
        info.framebuffer_height = fb.height;
        info.framebuffer_bpp = fb.bpp;
        info.framebuffer_type = 1; // RGB color.
    }

    info
}

// Compile-time layout verification (§3.9.7).
const _: () = {
    assert!(core::mem::size_of::<MultibootHeader>() == 48);
    assert!(core::mem::size_of::<MultibootModule>() == 16);
    assert!(core::mem::size_of::<MultibootMmapEntry>() == 24);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_header_valid() {
        // Build a minimal Multiboot header: magic + flags + checksum.
        let flags: u32 = MULTIBOOT_MEMORY_INFO;
        let checksum = 0u32.wrapping_sub(MULTIBOOT_HEADER_MAGIC.wrapping_add(flags));
        let mut kernel = [0u8; 8192];
        kernel[0..4].copy_from_slice(&MULTIBOOT_HEADER_MAGIC.to_le_bytes());
        kernel[4..8].copy_from_slice(&flags.to_le_bytes());
        kernel[8..12].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(find_header(&kernel), Some(0));
    }

    #[test]
    fn find_header_at_offset() {
        let flags: u32 = 0;
        let checksum = 0u32.wrapping_sub(MULTIBOOT_HEADER_MAGIC);
        let mut kernel = [0u8; 8192];
        // Place header at offset 256 (4-byte aligned).
        kernel[256..260].copy_from_slice(&MULTIBOOT_HEADER_MAGIC.to_le_bytes());
        kernel[260..264].copy_from_slice(&flags.to_le_bytes());
        kernel[264..268].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(find_header(&kernel), Some(256));
    }

    #[test]
    fn find_header_bad_checksum() {
        let mut kernel = [0u8; 8192];
        kernel[0..4].copy_from_slice(&MULTIBOOT_HEADER_MAGIC.to_le_bytes());
        kernel[4..8].copy_from_slice(&0u32.to_le_bytes());
        kernel[8..12].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        assert_eq!(find_header(&kernel), None);
    }

    #[test]
    fn find_header_too_small() {
        let kernel = [0u8; 8];
        assert_eq!(find_header(&kernel), None);
    }

    #[test]
    fn parse_header_basic() {
        let flags = MULTIBOOT_MEMORY_INFO | MULTIBOOT_PAGE_ALIGN;
        let checksum = 0u32.wrapping_sub(MULTIBOOT_HEADER_MAGIC.wrapping_add(flags));
        let mut kernel = [0u8; 64];
        kernel[0..4].copy_from_slice(&MULTIBOOT_HEADER_MAGIC.to_le_bytes());
        kernel[4..8].copy_from_slice(&flags.to_le_bytes());
        kernel[8..12].copy_from_slice(&checksum.to_le_bytes());
        let hdr = parse_header(&kernel, 0).unwrap();
        // Copy packed fields into locals before asserting.
        let (magic, hdr_flags, hdr_checksum) = (hdr.magic, hdr.flags, hdr.checksum);
        assert_eq!(magic, MULTIBOOT_HEADER_MAGIC);
        assert_eq!(hdr_flags, flags);
        assert_eq!(hdr_checksum, checksum);
    }

    #[test]
    fn bootloader_magic_value() {
        assert_eq!(MULTIBOOT_BOOTLOADER_MAGIC, 0x2BADB002);
    }

    #[test]
    fn build_info_basic() {
        let boot = MultibootBootInfo {
            mem_lower_kb: 640,
            mem_upper_kb: 130048,
            cmdline: String::from("root=/dev/sda1"),
            boot_loader_name: String::from("ZAMAK 0.6.9"),
            modules: Vec::new(),
            mmap: vec![
                MmapRegion {
                    addr: 0,
                    len: 0xA0000,
                    region_type: MULTIBOOT_MEMORY_AVAILABLE,
                },
                MmapRegion {
                    addr: 0x100000,
                    len: 0x7F00000,
                    region_type: MULTIBOOT_MEMORY_AVAILABLE,
                },
            ],
            framebuffer: None,
        };
        let info = build_info(&boot);
        // Copy packed fields into locals before asserting.
        let flags = info.flags;
        let mem_lower = info.mem_lower;
        let mem_upper = info.mem_upper;
        assert_ne!(flags & MULTIBOOT_INFO_MEMORY, 0);
        assert_ne!(flags & MULTIBOOT_INFO_CMDLINE, 0);
        assert_ne!(flags & MULTIBOOT_INFO_MEM_MAP, 0);
        assert_ne!(flags & MULTIBOOT_INFO_BOOT_LOADER_NAME, 0);
        assert_eq!(flags & MULTIBOOT_INFO_MODS, 0); // No modules.
        assert_eq!(mem_lower, 640);
        assert_eq!(mem_upper, 130048);
    }
}
