// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Multiboot 2 protocol implementation (FR-PROTO-003).
//!
//! Multiboot 2 uses a tag-based information structure instead of the
//! fixed-layout structure in Multiboot 1. The bootloader builds a
//! sequence of tags and passes them to the kernel.
//!
//! # References
//!
//! - [Multiboot 2 Specification](https://www.gnu.org/software/grub/manual/multiboot2/multiboot.html)

// Rust guideline compliant 2026-03-30

use alloc::vec::Vec;

#[cfg(test)]
use alloc::vec;

/// Multiboot 2 header magic.
pub const MULTIBOOT2_HEADER_MAGIC: u32 = 0xE85250D6;

/// Multiboot 2 bootloader magic passed in EAX.
pub const MULTIBOOT2_BOOTLOADER_MAGIC: u32 = 0x36D76289;

/// Header architecture: i386 protected mode.
pub const MULTIBOOT2_ARCHITECTURE_I386: u32 = 0;

/// Header architecture: MIPS.
pub const MULTIBOOT2_ARCHITECTURE_MIPS32: u32 = 4;

// =========================================================================
// Header tag types
// =========================================================================

pub const MULTIBOOT2_HEADER_TAG_END: u16 = 0;
pub const MULTIBOOT2_HEADER_TAG_INFORMATION_REQUEST: u16 = 1;
pub const MULTIBOOT2_HEADER_TAG_ADDRESS: u16 = 2;
pub const MULTIBOOT2_HEADER_TAG_ENTRY_ADDRESS: u16 = 3;
pub const MULTIBOOT2_HEADER_TAG_CONSOLE_FLAGS: u16 = 4;
pub const MULTIBOOT2_HEADER_TAG_FRAMEBUFFER: u16 = 5;
pub const MULTIBOOT2_HEADER_TAG_MODULE_ALIGN: u16 = 6;
pub const MULTIBOOT2_HEADER_TAG_EFI_BS: u16 = 7;
pub const MULTIBOOT2_HEADER_TAG_ENTRY_ADDRESS_EFI32: u16 = 8;
pub const MULTIBOOT2_HEADER_TAG_ENTRY_ADDRESS_EFI64: u16 = 9;
pub const MULTIBOOT2_HEADER_TAG_RELOCATABLE: u16 = 10;

// =========================================================================
// Boot information tag types
// =========================================================================

pub const MULTIBOOT2_TAG_TYPE_END: u32 = 0;
pub const MULTIBOOT2_TAG_TYPE_CMDLINE: u32 = 1;
pub const MULTIBOOT2_TAG_TYPE_BOOT_LOADER_NAME: u32 = 2;
pub const MULTIBOOT2_TAG_TYPE_MODULE: u32 = 3;
pub const MULTIBOOT2_TAG_TYPE_BASIC_MEMINFO: u32 = 4;
pub const MULTIBOOT2_TAG_TYPE_BOOTDEV: u32 = 5;
pub const MULTIBOOT2_TAG_TYPE_MMAP: u32 = 6;
pub const MULTIBOOT2_TAG_TYPE_VBE: u32 = 7;
pub const MULTIBOOT2_TAG_TYPE_FRAMEBUFFER: u32 = 8;
pub const MULTIBOOT2_TAG_TYPE_ELF_SECTIONS: u32 = 9;
pub const MULTIBOOT2_TAG_TYPE_APM: u32 = 10;
pub const MULTIBOOT2_TAG_TYPE_EFI32: u32 = 11;
pub const MULTIBOOT2_TAG_TYPE_EFI64: u32 = 12;
pub const MULTIBOOT2_TAG_TYPE_SMBIOS: u32 = 13;
pub const MULTIBOOT2_TAG_TYPE_ACPI_OLD: u32 = 14;
pub const MULTIBOOT2_TAG_TYPE_ACPI_NEW: u32 = 15;
pub const MULTIBOOT2_TAG_TYPE_NETWORK: u32 = 16;
pub const MULTIBOOT2_TAG_TYPE_EFI_MMAP: u32 = 17;
pub const MULTIBOOT2_TAG_TYPE_EFI_BS: u32 = 18;
pub const MULTIBOOT2_TAG_TYPE_EFI32_IH: u32 = 19;
pub const MULTIBOOT2_TAG_TYPE_EFI64_IH: u32 = 20;
pub const MULTIBOOT2_TAG_TYPE_LOAD_BASE_ADDR: u32 = 21;

/// Memory map entry types.
pub const MULTIBOOT2_MEMORY_AVAILABLE: u32 = 1;
pub const MULTIBOOT2_MEMORY_RESERVED: u32 = 2;
pub const MULTIBOOT2_MEMORY_ACPI_RECLAIMABLE: u32 = 3;
pub const MULTIBOOT2_MEMORY_NVS: u32 = 4;
pub const MULTIBOOT2_MEMORY_BADRAM: u32 = 5;

/// Multiboot 2 header (fixed portion).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Multiboot2Header {
    pub magic: u32,
    pub architecture: u32,
    pub header_length: u32,
    pub checksum: u32,
}

/// Generic header tag (common prefix for all header tags).
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct HeaderTag {
    pub tag_type: u16,
    pub flags: u16,
    pub size: u32,
}

/// Generic boot information tag header.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct TagHeader {
    pub tag_type: u32,
    pub size: u32,
}

/// Maximum bytes to search for the Multiboot 2 header.
const MULTIBOOT2_SEARCH_LIMIT: usize = 32768;

/// Scans a kernel image for the Multiboot 2 header.
///
/// The header must appear within the first 32768 bytes, aligned to 8 bytes.
/// Returns the byte offset if found.
pub fn find_header(kernel: &[u8]) -> Option<usize> {
    let search_end = kernel.len().min(MULTIBOOT2_SEARCH_LIMIT);
    if search_end < 16 {
        return None;
    }

    let mut offset = 0;
    while offset + 16 <= search_end {
        let magic = u32::from_le_bytes(kernel[offset..offset + 4].try_into().ok()?);
        if magic == MULTIBOOT2_HEADER_MAGIC {
            let arch = u32::from_le_bytes(kernel[offset + 4..offset + 8].try_into().ok()?);
            let length = u32::from_le_bytes(kernel[offset + 8..offset + 12].try_into().ok()?);
            let checksum = u32::from_le_bytes(kernel[offset + 12..offset + 16].try_into().ok()?);
            // Verify: magic + architecture + header_length + checksum == 0 (mod 2^32).
            if magic
                .wrapping_add(arch)
                .wrapping_add(length)
                .wrapping_add(checksum)
                == 0
            {
                return Some(offset);
            }
        }
        offset += 8; // 8-byte aligned search.
    }

    None
}

/// Parsed Multiboot 2 header with its tag requests.
#[derive(Debug)]
pub struct ParsedHeader {
    pub architecture: u32,
    pub header_length: u32,
    /// Requested information tag types (from INFORMATION_REQUEST header tags).
    pub requested_tags: Vec<u32>,
    /// Whether the kernel requests a framebuffer.
    pub framebuffer_requested: bool,
    pub preferred_width: u32,
    pub preferred_height: u32,
    pub preferred_bpp: u32,
    /// Whether modules should be page-aligned.
    pub module_align: bool,
    /// Whether to keep EFI boot services.
    pub efi_bs: bool,
    /// Entry address override (if present).
    pub entry_address: Option<u32>,
    /// EFI 64-bit entry address (if present).
    pub entry_address_efi64: Option<u32>,
}

/// Parses Multiboot 2 header tags starting at the given offset.
pub fn parse_header(kernel: &[u8], offset: usize) -> Option<ParsedHeader> {
    if offset + 16 > kernel.len() {
        return None;
    }

    let architecture = u32::from_le_bytes(kernel[offset + 4..offset + 8].try_into().ok()?);
    let header_length = u32::from_le_bytes(kernel[offset + 8..offset + 12].try_into().ok()?);

    let mut parsed = ParsedHeader {
        architecture,
        header_length,
        requested_tags: Vec::new(),
        framebuffer_requested: false,
        preferred_width: 0,
        preferred_height: 0,
        preferred_bpp: 0,
        module_align: false,
        efi_bs: false,
        entry_address: None,
        entry_address_efi64: None,
    };

    // Walk header tags (starting after the fixed 16-byte header).
    let tags_start = offset + 16;
    let tags_end = offset + header_length as usize;
    let mut pos = tags_start;

    while pos + 8 <= tags_end && pos + 8 <= kernel.len() {
        let tag_type = u16::from_le_bytes(kernel[pos..pos + 2].try_into().ok()?);
        let _flags = u16::from_le_bytes(kernel[pos + 2..pos + 4].try_into().ok()?);
        let tag_size = u32::from_le_bytes(kernel[pos + 4..pos + 8].try_into().ok()?);

        if tag_type == MULTIBOOT2_HEADER_TAG_END {
            break;
        }

        match tag_type {
            MULTIBOOT2_HEADER_TAG_INFORMATION_REQUEST => {
                // Each u32 after the 8-byte header is a requested tag type.
                let count = (tag_size as usize - 8) / 4;
                for i in 0..count {
                    let req_offset = pos + 8 + i * 4;
                    if req_offset + 4 <= kernel.len() {
                        let req =
                            u32::from_le_bytes(kernel[req_offset..req_offset + 4].try_into().ok()?);
                        parsed.requested_tags.push(req);
                    }
                }
            }
            MULTIBOOT2_HEADER_TAG_FRAMEBUFFER => {
                parsed.framebuffer_requested = true;
                if pos + 20 <= kernel.len() {
                    parsed.preferred_width =
                        u32::from_le_bytes(kernel[pos + 8..pos + 12].try_into().ok()?);
                    parsed.preferred_height =
                        u32::from_le_bytes(kernel[pos + 12..pos + 16].try_into().ok()?);
                    parsed.preferred_bpp =
                        u32::from_le_bytes(kernel[pos + 16..pos + 20].try_into().ok()?);
                }
            }
            MULTIBOOT2_HEADER_TAG_MODULE_ALIGN => {
                parsed.module_align = true;
            }
            MULTIBOOT2_HEADER_TAG_EFI_BS => {
                parsed.efi_bs = true;
            }
            MULTIBOOT2_HEADER_TAG_ENTRY_ADDRESS if pos + 12 <= kernel.len() => {
                parsed.entry_address = Some(u32::from_le_bytes(
                    kernel[pos + 8..pos + 12].try_into().ok()?,
                ));
            }
            MULTIBOOT2_HEADER_TAG_ENTRY_ADDRESS_EFI64 if pos + 12 <= kernel.len() => {
                parsed.entry_address_efi64 = Some(u32::from_le_bytes(
                    kernel[pos + 8..pos + 12].try_into().ok()?,
                ));
            }
            _ => {}
        }

        // Advance to next tag (8-byte aligned).
        let advance = ((tag_size as usize) + 7) & !7;
        pos += advance;
    }

    Some(parsed)
}

/// Builder for the Multiboot 2 boot information structure.
///
/// Tags are appended sequentially. The builder handles alignment
/// and the terminating end tag automatically.
pub struct BootInfoBuilder {
    buf: Vec<u8>,
}

impl Default for BootInfoBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl BootInfoBuilder {
    /// Creates a new builder. The first 8 bytes are reserved for the
    /// total_size and reserved fields of the boot info header.
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(4096);
        // total_size placeholder (patched in finish()).
        buf.extend_from_slice(&0u32.to_le_bytes());
        // reserved.
        buf.extend_from_slice(&0u32.to_le_bytes());
        Self { buf }
    }

    /// Aligns the buffer to 8 bytes.
    fn align8(&mut self) {
        while !self.buf.len().is_multiple_of(8) {
            self.buf.push(0);
        }
    }

    /// Adds a command line tag.
    pub fn add_cmdline(&mut self, cmdline: &str) {
        self.align8();
        let string_bytes = cmdline.as_bytes();
        let size = 8 + string_bytes.len() as u32 + 1; // +1 for NUL
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_CMDLINE.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(string_bytes);
        self.buf.push(0); // NUL terminator.
    }

    /// Adds a boot loader name tag.
    pub fn add_boot_loader_name(&mut self, name: &str) {
        self.align8();
        let string_bytes = name.as_bytes();
        let size = 8 + string_bytes.len() as u32 + 1;
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_BOOT_LOADER_NAME.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(string_bytes);
        self.buf.push(0);
    }

    /// Adds a basic memory info tag.
    pub fn add_basic_meminfo(&mut self, mem_lower_kb: u32, mem_upper_kb: u32) {
        self.align8();
        let size: u32 = 16;
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_BASIC_MEMINFO.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(&mem_lower_kb.to_le_bytes());
        self.buf.extend_from_slice(&mem_upper_kb.to_le_bytes());
    }

    /// Adds a module tag.
    pub fn add_module(&mut self, mod_start: u32, mod_end: u32, cmdline: &str) {
        self.align8();
        let string_bytes = cmdline.as_bytes();
        let size = 16 + string_bytes.len() as u32 + 1;
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_MODULE.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(&mod_start.to_le_bytes());
        self.buf.extend_from_slice(&mod_end.to_le_bytes());
        self.buf.extend_from_slice(string_bytes);
        self.buf.push(0);
    }

    /// Adds a memory map tag.
    pub fn add_mmap(&mut self, entries: &[MmapEntry]) {
        self.align8();
        let entry_size: u32 = 24; // size + addr(8) + len(8) + type(4) + reserved(4)
        let size = 16 + entries.len() as u32 * entry_size;
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_MMAP.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(&entry_size.to_le_bytes());
        self.buf.extend_from_slice(&0u32.to_le_bytes()); // entry_version

        for entry in entries {
            self.buf.extend_from_slice(&entry.addr.to_le_bytes());
            self.buf.extend_from_slice(&entry.len.to_le_bytes());
            self.buf.extend_from_slice(&entry.entry_type.to_le_bytes());
            self.buf.extend_from_slice(&0u32.to_le_bytes()); // reserved
        }
    }

    /// Adds a framebuffer tag.
    pub fn add_framebuffer(&mut self, addr: u64, pitch: u32, width: u32, height: u32, bpp: u8) {
        self.align8();
        let size: u32 = 32;
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_FRAMEBUFFER.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(&addr.to_le_bytes());
        self.buf.extend_from_slice(&pitch.to_le_bytes());
        self.buf.extend_from_slice(&width.to_le_bytes());
        self.buf.extend_from_slice(&height.to_le_bytes());
        self.buf.push(bpp);
        self.buf.push(1); // framebuffer_type: RGB
        self.buf.extend_from_slice(&0u16.to_le_bytes()); // reserved
                                                         // Color info (8 bytes for RGB type).
        self.buf.extend_from_slice(&[8, 0, 8, 8, 8, 16, 0, 0]); // r/g/b size and position
    }

    /// Adds an ACPI RSDP tag (old, 20-byte RSDP).
    pub fn add_acpi_old(&mut self, rsdp: &[u8]) {
        self.align8();
        let size = 8 + rsdp.len() as u32;
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_ACPI_OLD.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(rsdp);
    }

    /// Adds an ACPI XSDP tag (new, 36-byte XSDP).
    pub fn add_acpi_new(&mut self, xsdp: &[u8]) {
        self.align8();
        let size = 8 + xsdp.len() as u32;
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_ACPI_NEW.to_le_bytes());
        self.buf.extend_from_slice(&size.to_le_bytes());
        self.buf.extend_from_slice(xsdp);
    }

    /// Finalizes the boot information structure by appending the end tag
    /// and patching the total size. Returns the completed byte buffer.
    pub fn finish(mut self) -> Vec<u8> {
        // End tag.
        self.align8();
        self.buf
            .extend_from_slice(&MULTIBOOT2_TAG_TYPE_END.to_le_bytes());
        self.buf.extend_from_slice(&8u32.to_le_bytes());

        // Patch total_size at offset 0.
        let total = self.buf.len() as u32;
        self.buf[0..4].copy_from_slice(&total.to_le_bytes());

        self.buf
    }
}

/// A memory map entry for the Multiboot 2 mmap tag.
#[derive(Debug, Clone, Copy)]
pub struct MmapEntry {
    pub addr: u64,
    pub len: u64,
    pub entry_type: u32,
}

// Compile-time layout verification (§3.9.7).
const _: () = {
    assert!(core::mem::size_of::<Multiboot2Header>() == 16);
    assert!(core::mem::size_of::<HeaderTag>() == 8);
    assert!(core::mem::size_of::<TagHeader>() == 8);
};

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_header(arch: u32, tags: &[u8]) -> Vec<u8> {
        let header_length = 16 + tags.len() as u32 + 8; // +8 for end tag
        let checksum = 0u32
            .wrapping_sub(MULTIBOOT2_HEADER_MAGIC)
            .wrapping_sub(arch)
            .wrapping_sub(header_length);
        let mut buf = Vec::new();
        buf.extend_from_slice(&MULTIBOOT2_HEADER_MAGIC.to_le_bytes());
        buf.extend_from_slice(&arch.to_le_bytes());
        buf.extend_from_slice(&header_length.to_le_bytes());
        buf.extend_from_slice(&checksum.to_le_bytes());
        buf.extend_from_slice(tags);
        // End tag.
        buf.extend_from_slice(&MULTIBOOT2_HEADER_TAG_END.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes()); // flags
        buf.extend_from_slice(&8u32.to_le_bytes()); // size
                                                    // Pad to 32K so find_header works.
        buf.resize(32768, 0);
        buf
    }

    #[test]
    fn find_header_valid() {
        let kernel = build_test_header(MULTIBOOT2_ARCHITECTURE_I386, &[]);
        assert_eq!(find_header(&kernel), Some(0));
    }

    #[test]
    fn find_header_not_found() {
        let kernel = vec![0u8; 32768];
        assert_eq!(find_header(&kernel), None);
    }

    #[test]
    fn parse_header_empty_tags() {
        let kernel = build_test_header(MULTIBOOT2_ARCHITECTURE_I386, &[]);
        let parsed = parse_header(&kernel, 0).unwrap();
        assert_eq!(parsed.architecture, MULTIBOOT2_ARCHITECTURE_I386);
        assert!(parsed.requested_tags.is_empty());
        assert!(!parsed.framebuffer_requested);
    }

    #[test]
    fn boot_info_builder_basic() {
        let mut builder = BootInfoBuilder::new();
        builder.add_boot_loader_name("ZAMAK 0.6.9");
        builder.add_cmdline("root=/dev/sda1");
        builder.add_basic_meminfo(640, 130048);
        builder.add_mmap(&[MmapEntry {
            addr: 0,
            len: 0xA0000,
            entry_type: MULTIBOOT2_MEMORY_AVAILABLE,
        }]);
        let info = builder.finish();

        // Check total_size is patched.
        let total_size = u32::from_le_bytes(info[0..4].try_into().unwrap());
        assert_eq!(total_size as usize, info.len());

        // Last 8 bytes should be the end tag.
        let end_type = u32::from_le_bytes(info[info.len() - 8..info.len() - 4].try_into().unwrap());
        let end_size = u32::from_le_bytes(info[info.len() - 4..].try_into().unwrap());
        assert_eq!(end_type, MULTIBOOT2_TAG_TYPE_END);
        assert_eq!(end_size, 8);
    }

    #[test]
    fn bootloader_magic_value() {
        assert_eq!(MULTIBOOT2_BOOTLOADER_MAGIC, 0x36D76289);
    }
}
