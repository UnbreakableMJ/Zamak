// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Linux x86 Boot Protocol implementation (FR-PROTO-002).
//!
//! Parses the x86 bzImage `setup_header` and provides the `BootParams`
//! structure needed to hand off to a Linux kernel. Supports boot
//! protocol versions ≥ 2.06.
//!
//! Reference: <https://www.kernel.org/doc/html/latest/arch/x86/boot.html>

// Rust guideline compliant 2026-03-30

use core::fmt;

/// Minimum supported boot protocol version (2.06).
///
/// Version 2.06 added `cmdline_size` and is the baseline for modern
/// kernels. Versions below this are too old to support reliably.
const MIN_BOOT_PROTOCOL_VERSION: u16 = 0x0206;

/// Expected magic number at offset 0x202 in a bzImage: "HdrS".
const HDRS_MAGIC: u32 = 0x5372_6448;

/// Default kernel load address when `relocatable_kernel` is not set.
const DEFAULT_KERNEL_LOAD_ADDR: u64 = 0x0010_0000; // 1 MiB

/// Bootloader type ID for "other" (0xFF = undefined/other).
const BOOTLOADER_TYPE: u8 = 0xFF;

/// Boot protocol version reported by the bootloader.
const BOOTLOADER_VERSION: u8 = 0;

/// Errors that can occur when parsing a bzImage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBootError {
    /// Image too small to contain a valid setup header.
    ImageTooSmall,
    /// Missing "HdrS" magic at offset 0x202.
    InvalidMagic,
    /// Boot protocol version is below the minimum supported (2.06).
    UnsupportedVersion(u16),
    /// The setup_sects field is zero (corrupted header).
    InvalidSetupSects,
    /// The protected-mode kernel is empty.
    EmptyKernel,
}

impl fmt::Display for LinuxBootError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageTooSmall => write!(f, "bzImage too small for setup header"),
            Self::InvalidMagic => write!(f, "missing HdrS magic at offset 0x202"),
            Self::UnsupportedVersion(v) => {
                write!(f, "boot protocol {v:#06x} below minimum 0x0206")
            }
            Self::InvalidSetupSects => write!(f, "setup_sects is zero"),
            Self::EmptyKernel => write!(f, "protected-mode kernel payload is empty"),
        }
    }
}

/// Parsed setup header from a bzImage.
///
/// Contains the fields the bootloader needs to load and launch the
/// kernel. Field offsets reference the Linux boot protocol spec.
#[derive(Debug, Clone)]
pub struct SetupHeader {
    /// Boot protocol version (e.g., 0x020F for 2.15).
    pub version: u16,
    /// Size of the real-mode code in 512-byte sectors (offset 0x1F1).
    pub setup_sects: u8,
    /// Kernel load flags (offset 0x211).
    pub loadflags: u8,
    /// Size of the protected-mode kernel in 16-byte paragraphs (offset 0x1F4, protocol < 2.04)
    /// or the full 32-bit `syssize` at offset 0x1F4.
    pub syssize: u32,
    /// Preferred load address for the protected-mode kernel (offset 0x258, protocol ≥ 2.06).
    pub pref_address: u64,
    /// Whether the kernel is relocatable (offset 0x234, protocol ≥ 2.05).
    pub relocatable: bool,
    /// Alignment required for the kernel load address (offset 0x230, protocol ≥ 2.05).
    pub kernel_alignment: u32,
    /// Minimum kernel load address (offset 0x238, protocol ≥ 2.10).
    pub min_alignment: u32,
    /// Maximum size of the command line (offset 0x238, protocol ≥ 2.06).
    pub cmdline_size: u32,
    /// Size of the real-mode setup code + header in bytes.
    pub setup_size: usize,
    /// Initramfs size limit (offset 0x23C, protocol ≥ 2.06).
    pub initrd_addr_max: u32,
}

/// The `LOADED_HIGH` flag in `loadflags` — kernel must be loaded at 1 MiB+.
const LOADED_HIGH: u8 = 0x01;

/// Parses the setup header from a bzImage buffer.
///
/// Returns the parsed header or an error if the image is invalid.
pub fn parse_setup_header(bzimage: &[u8]) -> Result<SetupHeader, LinuxBootError> {
    if bzimage.len() < 0x260 {
        return Err(LinuxBootError::ImageTooSmall);
    }

    // Check "HdrS" magic at offset 0x202.
    let magic = u32::from_le_bytes([
        bzimage[0x202],
        bzimage[0x203],
        bzimage[0x204],
        bzimage[0x205],
    ]);
    if magic != HDRS_MAGIC {
        return Err(LinuxBootError::InvalidMagic);
    }

    // Boot protocol version at offset 0x206.
    let version = u16::from_le_bytes([bzimage[0x206], bzimage[0x207]]);
    if version < MIN_BOOT_PROTOCOL_VERSION {
        return Err(LinuxBootError::UnsupportedVersion(version));
    }

    // setup_sects at offset 0x1F1.
    let setup_sects = if bzimage[0x1F1] == 0 {
        4
    } else {
        bzimage[0x1F1]
    };

    // syssize at offset 0x1F4 (32-bit LE).
    let syssize = u32::from_le_bytes([
        bzimage[0x1F4],
        bzimage[0x1F5],
        bzimage[0x1F6],
        bzimage[0x1F7],
    ]);

    // loadflags at offset 0x211.
    let loadflags = bzimage[0x211];

    // kernel_alignment at offset 0x230 (protocol ≥ 2.05).
    let kernel_alignment = u32::from_le_bytes([
        bzimage[0x230],
        bzimage[0x231],
        bzimage[0x232],
        bzimage[0x233],
    ]);

    // relocatable_kernel at offset 0x234 (protocol ≥ 2.05).
    let relocatable = bzimage[0x234] != 0;

    // min_alignment is a shift count at offset 0x235 (protocol ≥ 2.10).
    // Actual alignment = 1 << min_alignment.
    let min_alignment = if version >= 0x020A {
        1u32 << bzimage[0x235]
    } else {
        kernel_alignment
    };

    // cmdline_size at offset 0x238 (protocol ≥ 2.06).
    let cmdline_size = u32::from_le_bytes([
        bzimage[0x238],
        bzimage[0x239],
        bzimage[0x23A],
        bzimage[0x23B],
    ]);

    // initrd_addr_max at offset 0x22C (protocol ≥ 2.03).
    let initrd_addr_max = u32::from_le_bytes([
        bzimage[0x22C],
        bzimage[0x22D],
        bzimage[0x22E],
        bzimage[0x22F],
    ]);

    // pref_address at offset 0x258 (protocol ≥ 2.10).
    let pref_address = if version >= 0x020A && bzimage.len() >= 0x260 {
        u64::from_le_bytes([
            bzimage[0x258],
            bzimage[0x259],
            bzimage[0x25A],
            bzimage[0x25B],
            bzimage[0x25C],
            bzimage[0x25D],
            bzimage[0x25E],
            bzimage[0x25F],
        ])
    } else {
        DEFAULT_KERNEL_LOAD_ADDR
    };

    // The setup code occupies (1 + setup_sects) * 512 bytes at the start.
    let setup_size = (1 + setup_sects as usize) * 512;

    if bzimage.len() <= setup_size {
        return Err(LinuxBootError::EmptyKernel);
    }

    Ok(SetupHeader {
        version,
        setup_sects,
        loadflags,
        syssize,
        pref_address,
        relocatable,
        kernel_alignment,
        min_alignment,
        cmdline_size,
        setup_size,
        initrd_addr_max,
    })
}

/// Returns the byte offset where the protected-mode kernel starts in the bzImage.
pub fn kernel_offset(header: &SetupHeader) -> usize {
    header.setup_size
}

/// Returns the size of the protected-mode kernel in bytes.
pub fn kernel_size(header: &SetupHeader, bzimage_len: usize) -> usize {
    bzimage_len - header.setup_size
}

/// Returns the load address for the protected-mode kernel.
///
/// If the kernel is relocatable, returns `pref_address`. Otherwise
/// returns the default 1 MiB load address (or 0x10000 if `LOADED_HIGH`
/// is not set, which we reject for modern kernels).
pub fn kernel_load_address(header: &SetupHeader) -> u64 {
    if header.loadflags & LOADED_HIGH != 0 {
        if header.relocatable {
            header.pref_address
        } else {
            DEFAULT_KERNEL_LOAD_ADDR
        }
    } else {
        // Legacy real-mode kernel — not supported.
        DEFAULT_KERNEL_LOAD_ADDR
    }
}

/// x86 boot_params / zero page (4096 bytes).
///
/// Uses a raw byte array with accessor methods to guarantee correct
/// field offsets. The Linux kernel reads specific byte offsets from
/// this page, so structural alignment must match the spec exactly.
///
/// Reference: <https://www.kernel.org/doc/html/latest/arch/x86/zero-page.html>
#[repr(C, align(4096))]
pub struct BootParams {
    data: [u8; 4096],
}

/// Well-known byte offsets within the boot_params / zero page.
mod offsets {
    /// type_of_loader (1 byte).
    pub const TYPE_OF_LOADER: usize = 0x210;
    /// loadflags (1 byte).
    pub const LOADFLAGS: usize = 0x211;
    /// code32_start (4 bytes LE) — 32-bit entry point of protected-mode kernel.
    pub const CODE32_START: usize = 0x214;
    /// ramdisk_image (4 bytes LE) — physical address of initramfs.
    pub const RAMDISK_IMAGE: usize = 0x218;
    /// ramdisk_size (4 bytes LE) — size of initramfs in bytes.
    pub const RAMDISK_SIZE: usize = 0x21C;
    /// cmd_line_ptr (4 bytes LE) — physical address of NUL-terminated command line.
    pub const CMD_LINE_PTR: usize = 0x228;
    /// E820 entry count at offset 0x1E8.
    pub const E820_ENTRIES: usize = 0x1E8;
    /// E820 table starts at offset 0x2D0, each entry is 20 bytes.
    pub const E820_TABLE: usize = 0x2D0;
    /// Maximum E820 entries that fit in the zero page.
    pub const E820_MAX: usize = 128;
}

impl BootParams {
    /// Creates a zeroed `BootParams`.
    pub fn zeroed() -> Self {
        Self { data: [0u8; 4096] }
    }

    /// Populates the boot_params from the raw bzImage setup code.
    ///
    /// Copies the setup header (first `header.setup_size` bytes, capped
    /// at 4096) into this structure, then patches the `type_of_loader`
    /// field to identify ZAMAK as the bootloader.
    pub fn populate_from_bzimage(&mut self, bzimage: &[u8], header: &SetupHeader) {
        let copy_len = core::cmp::min(header.setup_size, 4096);
        self.data[..copy_len].copy_from_slice(&bzimage[..copy_len]);

        // Identify ZAMAK as the bootloader (type 0xFF = "other").
        self.data[offsets::TYPE_OF_LOADER] = BOOTLOADER_TYPE | (BOOTLOADER_VERSION << 4);

        // Ensure LOADED_HIGH and CAN_USE_HEAP are set.
        self.data[offsets::LOADFLAGS] |= LOADED_HIGH | 0x80; // 0x80 = CAN_USE_HEAP
    }

    /// Sets the command line pointer (physical address of NUL-terminated string).
    pub fn set_cmdline(&mut self, phys_addr: u32) {
        self.write_u32(offsets::CMD_LINE_PTR, phys_addr);
    }

    /// Sets the initramfs location and size.
    pub fn set_initrd(&mut self, phys_addr: u32, size: u32) {
        self.write_u32(offsets::RAMDISK_IMAGE, phys_addr);
        self.write_u32(offsets::RAMDISK_SIZE, size);
    }

    /// Sets the 32-bit entry point for the protected-mode kernel.
    pub fn set_code32_start(&mut self, addr: u32) {
        self.write_u32(offsets::CODE32_START, addr);
    }

    /// Adds an E820 memory map entry to the zero page.
    ///
    /// Returns `false` if the table is full (128 entries max).
    pub fn add_e820_entry(&mut self, base: u64, size: u64, typ: u32) -> bool {
        let count = self.data[offsets::E820_ENTRIES] as usize;
        if count >= offsets::E820_MAX {
            return false;
        }

        let entry_offset = offsets::E820_TABLE + count * 20;
        self.write_u64(entry_offset, base);
        self.write_u64(entry_offset + 8, size);
        self.write_u32(entry_offset + 16, typ);
        self.data[offsets::E820_ENTRIES] = (count + 1) as u8;
        true
    }

    /// Returns a pointer to the raw data, suitable for passing to the kernel.
    pub fn as_ptr(&self) -> *const u8 {
        self.data.as_ptr()
    }

    /// Returns a mutable pointer to the raw data.
    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        self.data.as_mut_ptr()
    }

    fn write_u32(&mut self, offset: usize, val: u32) {
        self.data[offset..offset + 4].copy_from_slice(&val.to_le_bytes());
    }

    fn write_u64(&mut self, offset: usize, val: u64) {
        self.data[offset..offset + 8].copy_from_slice(&val.to_le_bytes());
    }
}

// §3.9.7: Compile-time layout verification.
const _: () = {
    assert!(
        core::mem::size_of::<BootParams>() == 4096,
        "BootParams must be exactly 4096 bytes (one page)"
    );
};
