// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! BIOS → kmain handoff bundle (M1-16 Path B).
//!
//! All BIOS I/O in the Path B boot path happens in real mode **before**
//! CR0.PE is set. The real-mode orchestration in `_start` populates a
//! single `BootDataBundle` at physical address [`BOOT_BUNDLE_PHYS`],
//! and the 32-bit `kmain` then consumes nothing else from the BIOS —
//! it reads the bundle, parses the config, loads the ELF, and enters
//! long mode.
//!
//! The bundle lives in conventional memory (below 1 MiB) so the
//! real-mode code can write it without touching the unreal-mode
//! descriptor cache, and the bounce buffers carved out by the
//! real-mode FAT32 loader (phys `0x5000..0x7000`) do not overlap it.
//!
//! Layout invariants
//! -----------------
//! - `magic` is always `ZBDL_MAGIC` on a well-formed bundle. kmain
//!   bails immediately if the magic is absent — nothing else is
//!   trustworthy without it.
//! - `e820[..e820_count as usize]` is the valid prefix of the BIOS
//!   E820 memory map; remaining slots are zero-initialized.
//! - `config[..config_len as usize]` is the raw UTF-8-ish bytes of
//!   `zamak.conf`. Length is capped at `CONFIG_MAX_BYTES`.
//! - `kernel_phys` is a flat 32-bit-addressable physical address the
//!   kernel ELF was loaded to in real/unreal mode (typically
//!   `0x0100_0000` = 16 MiB). `kernel_len` is its size in bytes.
//! - `rsdp_phys` / `smbios_phys` are scan results from the BIOS
//!   vendor regions (0xE0000..0xFFFFF). Zero means "not found".

// Rust guideline compliant 2026-03-30

/// Physical address at which the real-mode orchestration writes the
/// `BootDataBundle`, and from which the 32-bit kmain reads it.
///
/// Chosen to sit above the IVT / BDA (0x0000..0x04FF) and below the
/// FAT32 bounce-buffer region (0x5000..0x6FFF) and the real-mode
/// stack / BIOS scratch zone (0x7000..0x7FFF) and stage2 load base
/// (0x8000).
pub const BOOT_BUNDLE_PHYS: usize = 0x0000_1000;

/// Magic stamped into `BootDataBundle::magic` by the real-mode
/// orchestration. ASCII "ZBDL" in little-endian byte order:
/// `b'Z' | b'B' << 8 | b'D' << 16 | b'L' << 24` → `0x4C44_425A`.
///
/// (The M1-16 Path B design doc listed this as `0x4C42_445A`, which is
/// actually "ZDBL" — the constant here is the value that decomposes
/// into the bytes the doc's English prose specifies.)
pub const ZBDL_MAGIC: u32 = 0x4C44_425A;

/// Maximum number of E820 entries the bundle can carry. The BIOS E820
/// spec doesn't strictly cap the count; 128 is comfortably more than
/// any real system we've observed (Linux uses 128 for its
/// `boot_params.e820_table` for the same reason).
pub const E820_MAX_ENTRIES: usize = 128;

/// Maximum `zamak.conf` size (4 KiB). Config files larger than this
/// are rejected by the real-mode loader before the bundle is finalized.
pub const CONFIG_MAX_BYTES: usize = 4096;

/// Raw BIOS E820 memory-map entry.
///
/// This is the 24-byte record the BIOS INT 15h, AX=E820h call emits,
/// with the optional ACPI attribute word kept so callers see exactly
/// what the BIOS returned. The protected-mode kmain converts this
/// into the Limine `MemmapEntry` shape when fulfilling the
/// `MEMMAP_ID` request.
#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
pub struct E820Entry {
    pub base: u64,
    pub len: u64,
    pub typ: u32,
    pub acpi: u32,
}

const _: () = {
    assert!(
        core::mem::size_of::<E820Entry>() == 24,
        "E820Entry must be 24 bytes"
    );
    assert!(core::mem::offset_of!(E820Entry, base) == 0);
    assert!(core::mem::offset_of!(E820Entry, len) == 8);
    assert!(core::mem::offset_of!(E820Entry, typ) == 16);
    assert!(core::mem::offset_of!(E820Entry, acpi) == 20);
};

/// VESA VBE mode-info record, written by real-mode INT 10h, AX=4F01h
/// for the selected mode and surfaced to kmain via `BootDataBundle`.
///
/// Only the fields relevant to framebuffer hand-off are consulted;
/// the rest are preserved verbatim so any post-boot VBE introspection
/// tools see the original BIOS-returned bytes.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct VbeModeInfo {
    pub attributes: u16,
    pub window_a: u8,
    pub window_b: u8,
    pub granularity: u16,
    pub window_size: u16,
    pub segment_a: u16,
    pub segment_b: u16,
    pub win_func_ptr: u32,
    pub pitch: u16,
    pub width: u16,
    pub height: u16,
    pub w_char: u8,
    pub y_char: u8,
    pub planes: u8,
    pub bpp: u8,
    pub banks: u8,
    pub memory_model: u8,
    pub bank_size: u8,
    pub image_pages: u8,
    pub reserved0: u8,
    pub red_mask_size: u8,
    pub red_field_position: u8,
    pub green_mask_size: u8,
    pub green_field_position: u8,
    pub blue_mask_size: u8,
    pub blue_field_position: u8,
    pub rsvd_mask_size: u8,
    pub rsvd_field_position: u8,
    pub direct_color_mode_info: u8,
    pub phys_base: u32,
    pub reserved1: u32,
    pub reserved2: u16,
    pub lin_bytes_per_scan_line: u16,
    pub b_num_images: u8,
    pub l_num_images: u8,
    pub l_red_mask_size: u8,
    pub l_red_field_position: u8,
    pub l_green_mask_size: u8,
    pub l_green_field_position: u8,
    pub l_blue_mask_size: u8,
    pub l_blue_field_position: u8,
    pub l_rsvd_mask_size: u8,
    pub l_rsvd_field_position: u8,
    pub max_pixel_clock: u32,
    pub reserved3: [u8; 190],
}

impl Default for VbeModeInfo {
    fn default() -> Self {
        // SAFETY: VbeModeInfo is a `#[repr(C, packed)]` POD record
        // whose all-zero bit pattern is a valid (empty) mode-info
        // block — `attributes == 0` signals "mode not set" to
        // consumers.
        unsafe { core::mem::zeroed() }
    }
}

const _: () = {
    assert!(
        core::mem::size_of::<VbeModeInfo>() == 256,
        "VbeModeInfo must be 256 bytes (VBE 3.0 spec)"
    );
};

/// Handoff record populated by real-mode orchestration in `_start`
/// and consumed by the 32-bit `kmain`.
///
/// Placed at physical address [`BOOT_BUNDLE_PHYS`]. `magic` must be
/// [`ZBDL_MAGIC`] for kmain to proceed; every other field is only
/// meaningful once the magic has been validated.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct BootDataBundle {
    /// [`ZBDL_MAGIC`] — kmain panics if this doesn't match.
    pub magic: u32,
    /// BIOS boot drive number from DL at `_start`.
    pub boot_drive: u8,
    /// Pad to a 4-byte boundary before `e820_count`.
    pub _pad0: [u8; 3],
    /// Number of valid entries in `e820`.
    pub e820_count: u32,
    pub e820: [E820Entry; E820_MAX_ENTRIES],
    /// Starting LBA of the MBR partition the kernel lives on.
    pub partition_lba: u32,
    /// MBR partition type byte (0x0B / 0x0C for FAT32, 0x83 for
    /// Linux). Present for kmain diagnostics; Path B only mounts
    /// FAT32 regardless.
    pub partition_type: u8,
    pub _pad1: [u8; 3],
    /// Physical address of a pre-loaded copy of the boot partition's
    /// leading bytes. Real-mode orchestration bulk-reads the
    /// partition through `rm_load_chunk` into this buffer; kmain
    /// then parses FAT32 against it in protected mode without
    /// needing another BIOS call.
    pub partition_image_phys: u64,
    /// Length in bytes of the pre-loaded partition image.
    pub partition_image_len: u64,
    pub vbe_info: VbeModeInfo,
    /// Length of the config bytes in `config`. `0` means "no config
    /// file found" — kmain panics in that case.
    pub config_len: u32,
    pub config: [u8; CONFIG_MAX_BYTES],
    /// Physical address of the BIOS RSDP (or 0 if not found).
    pub rsdp_phys: u64,
    /// Physical address of the SMBIOS anchor (or 0 if not found).
    pub smbios_phys: u64,
    /// Physical address the kernel ELF was loaded to (typically
    /// `0x0100_0000`).
    pub kernel_phys: u64,
    /// Length of the kernel ELF in bytes.
    pub kernel_len: u64,
}

const _: () = {
    assert!(
        core::mem::size_of::<BootDataBundle>() < 8192,
        "BootDataBundle must fit comfortably below the 0x5000 bounce \
         buffer (bundle starts at 0x1000)"
    );
    assert!(core::mem::offset_of!(BootDataBundle, magic) == 0);
    // `e820` begins at offset 12 (magic@0 + u32, boot_drive@4 + u8
    // + 3-byte pad, e820_count@8 + u32 = 12). Real-mode asm writes
    // into `[BOOT_BUNDLE_PHYS + 12]` for E820 entries.
    assert!(core::mem::offset_of!(BootDataBundle, e820) == 12);
    // `kernel_phys` and `kernel_len` are the last 16 bytes — the
    // real-mode orchestration writes them last so kmain sees a
    // fully-initialized bundle only if every step succeeded.
    assert!(
        core::mem::offset_of!(BootDataBundle, kernel_len)
            + core::mem::size_of::<u64>()
            == core::mem::size_of::<BootDataBundle>()
    );
};

/// Reads the live `BootDataBundle` at [`BOOT_BUNDLE_PHYS`].
///
/// # Safety
///
/// Caller must guarantee:
/// - CPU is in 32-bit protected mode with flat DS/ES (i.e. past the
///   `init_32` transition), so the fixed physical address is also
///   the linear address.
/// - The real-mode orchestration has finished populating the bundle
///   (i.e. this is being called from `kmain` or later).
/// - No concurrent writer (there isn't one — stage 2 is
///   single-threaded and SMP APs haven't started yet).
#[inline]
#[allow(dead_code)] // wired up in Phase 6
pub unsafe fn bundle<'a>() -> &'a BootDataBundle {
    // SAFETY: see fn safety contract above.
    unsafe { &*(BOOT_BUNDLE_PHYS as *const BootDataBundle) }
}

/// Mutable accessor used by the real-mode orchestration when filling
/// in the bundle. In protected-mode kmain, use [`bundle`] instead.
///
/// # Safety
///
/// Same as [`bundle`]; additionally, caller must ensure no reader
/// observes a partially-written bundle across writes (real-mode
/// orchestration writes `magic` last, by design, so the discipline
/// is: all other fields first, `magic` last).
#[inline]
#[allow(dead_code)] // wired up in Phase 5
pub unsafe fn bundle_mut<'a>() -> &'a mut BootDataBundle {
    // SAFETY: see fn safety contract above.
    unsafe { &mut *(BOOT_BUNDLE_PHYS as *mut BootDataBundle) }
}

// zamak-bios is a no_std / no_main binary crate with a custom
// `panic_impl`, so it can't host a `#[cfg(test)]` module the way a
// library crate can. The checks below are evaluated at compile time
// and cover the same invariants the plan called out for Phase 1
// ("bundle layout, magic round-trip"): layout via `offset_of` and
// `size_of`, magic round-trip via explicit LE-byte decomposition.
const _: () = {
    let bytes = ZBDL_MAGIC.to_le_bytes();
    assert!(bytes[0] == b'Z', "ZBDL_MAGIC byte 0 must be 'Z'");
    assert!(bytes[1] == b'B', "ZBDL_MAGIC byte 1 must be 'B'");
    assert!(bytes[2] == b'D', "ZBDL_MAGIC byte 2 must be 'D'");
    assert!(bytes[3] == b'L', "ZBDL_MAGIC byte 3 must be 'L'");
    assert!(
        core::mem::size_of::<[E820Entry; E820_MAX_ENTRIES]>()
            == 24 * E820_MAX_ENTRIES,
        "E820 table must be exactly 24 bytes per entry (no tail padding)"
    );
    // Leave plenty of headroom below the 0x4000 mark where the FAT32
    // bounce buffer begins (0x5000 minus overlap safety).
    assert!(
        core::mem::size_of::<BootDataBundle>() < 0x3000,
        "BootDataBundle must stay under 12 KiB to fit below 0x4000"
    );
};
