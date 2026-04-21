// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! PE/COFF loader for EFI chainloading (§4.3).
//!
//! Parses Portable Executable (PE32+) images used by UEFI applications.
//! Supports section loading, relocation processing, and entry point resolution.
//!
//! This module handles the subset of PE/COFF required for chainloading
//! EFI applications and loading PE-format kernels.

// Rust guideline compliant 2026-03-30

/// DOS MZ magic number.
pub const DOS_MAGIC: u16 = 0x5A4D;

/// PE signature magic.
pub const PE_MAGIC: u32 = 0x0000_4550; // "PE\0\0"

/// Machine types.
pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
pub const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;
pub const IMAGE_FILE_MACHINE_RISCV64: u16 = 0x5064;

/// Optional header magic values.
pub const PE32_MAGIC: u16 = 0x10B;
pub const PE32PLUS_MAGIC: u16 = 0x20B;

/// Section characteristics.
pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
pub const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;
pub const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
pub const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;

/// Base relocation types.
pub const IMAGE_REL_BASED_ABSOLUTE: u16 = 0;
pub const IMAGE_REL_BASED_HIGH: u16 = 1;
pub const IMAGE_REL_BASED_LOW: u16 = 2;
pub const IMAGE_REL_BASED_HIGHLOW: u16 = 3;
pub const IMAGE_REL_BASED_DIR64: u16 = 10;

/// DOS header — only the fields we need.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DosHeader {
    pub e_magic: u16,
    _reserved: [u8; 58],
    /// File offset to the PE signature.
    pub e_lfanew: u32,
}

/// COFF file header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct CoffHeader {
    pub machine: u16,
    pub number_of_sections: u16,
    pub time_date_stamp: u32,
    pub pointer_to_symbol_table: u32,
    pub number_of_symbols: u32,
    pub size_of_optional_header: u16,
    pub characteristics: u16,
}

/// PE32+ optional header (64-bit).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct OptionalHeader64 {
    pub magic: u16,
    pub major_linker_version: u8,
    pub minor_linker_version: u8,
    pub size_of_code: u32,
    pub size_of_initialized_data: u32,
    pub size_of_uninitialized_data: u32,
    pub address_of_entry_point: u32,
    pub base_of_code: u32,
    pub image_base: u64,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub major_os_version: u16,
    pub minor_os_version: u16,
    pub major_image_version: u16,
    pub minor_image_version: u16,
    pub major_subsystem_version: u16,
    pub minor_subsystem_version: u16,
    pub win32_version_value: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub checksum: u32,
    pub subsystem: u16,
    pub dll_characteristics: u16,
    pub size_of_stack_reserve: u64,
    pub size_of_stack_commit: u64,
    pub size_of_heap_reserve: u64,
    pub size_of_heap_commit: u64,
    pub loader_flags: u32,
    pub number_of_rva_and_sizes: u32,
}

/// Data directory entry.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DataDirectory {
    pub virtual_address: u32,
    pub size: u32,
}

/// Well-known data directory indices.
pub const IMAGE_DIRECTORY_ENTRY_BASERELOC: usize = 5;

/// Section header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SectionHeader {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: u32,
    pub pointer_to_relocations: u32,
    pub pointer_to_linenumbers: u32,
    pub number_of_relocations: u16,
    pub number_of_linenumbers: u16,
    pub characteristics: u32,
}

/// Base relocation block header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct BaseRelocationBlock {
    pub virtual_address: u32,
    pub size_of_block: u32,
}

/// Errors from PE parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeError {
    TooSmall,
    InvalidDosMagic,
    InvalidPeMagic,
    UnsupportedMachine,
    UnsupportedOptionalHeader,
    SectionOutOfBounds,
    RelocationOutOfBounds,
}

/// Parsed PE image metadata.
#[derive(Debug, PartialEq, Eq)]
pub struct PeImage {
    pub machine: u16,
    pub entry_point_rva: u32,
    pub image_base: u64,
    pub size_of_image: u32,
    pub section_alignment: u32,
    pub sections: alloc::vec::Vec<SectionInfo>,
    pub reloc_rva: u32,
    pub reloc_size: u32,
}

/// Parsed section info.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInfo {
    pub name: [u8; 8],
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_data_offset: u32,
    pub raw_data_size: u32,
    pub characteristics: u32,
}

impl SectionInfo {
    /// Returns the section name as a string (trimmed of NUL bytes).
    pub fn name_str(&self) -> &str {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(8);
        core::str::from_utf8(&self.name[..end]).unwrap_or("")
    }
}

/// Parses a PE/COFF image and returns its metadata.
pub fn parse(data: &[u8]) -> Result<PeImage, PeError> {
    if data.len() < core::mem::size_of::<DosHeader>() {
        return Err(PeError::TooSmall);
    }

    // Parse DOS header.
    let dos: DosHeader = read_struct(data, 0).ok_or(PeError::TooSmall)?;
    if dos.e_magic != DOS_MAGIC {
        return Err(PeError::InvalidDosMagic);
    }

    let pe_offset = dos.e_lfanew as usize;

    // Parse PE signature.
    if pe_offset + 4 > data.len() {
        return Err(PeError::TooSmall);
    }
    let pe_sig = u32::from_le_bytes(data[pe_offset..pe_offset + 4].try_into().unwrap());
    if pe_sig != PE_MAGIC {
        return Err(PeError::InvalidPeMagic);
    }

    // Parse COFF header.
    let coff_offset = pe_offset + 4;
    let coff: CoffHeader = read_struct(data, coff_offset).ok_or(PeError::TooSmall)?;

    match coff.machine {
        IMAGE_FILE_MACHINE_AMD64
        | IMAGE_FILE_MACHINE_I386
        | IMAGE_FILE_MACHINE_ARM64
        | IMAGE_FILE_MACHINE_RISCV64 => {}
        _ => return Err(PeError::UnsupportedMachine),
    }

    // Parse optional header (PE32+ only for now).
    let opt_offset = coff_offset + core::mem::size_of::<CoffHeader>();
    let opt: OptionalHeader64 = read_struct(data, opt_offset).ok_or(PeError::TooSmall)?;
    if opt.magic != PE32PLUS_MAGIC {
        return Err(PeError::UnsupportedOptionalHeader);
    }

    // Parse data directories to find base relocations.
    let dd_offset = opt_offset + core::mem::size_of::<OptionalHeader64>();
    let (reloc_rva, reloc_size) =
        if opt.number_of_rva_and_sizes as usize > IMAGE_DIRECTORY_ENTRY_BASERELOC {
            let reloc_dd_offset =
                dd_offset + IMAGE_DIRECTORY_ENTRY_BASERELOC * core::mem::size_of::<DataDirectory>();
            if let Some(dd) = read_struct::<DataDirectory>(data, reloc_dd_offset) {
                (dd.virtual_address, dd.size)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };

    // Parse section headers.
    let sections_offset = opt_offset + coff.size_of_optional_header as usize;
    let mut sections = alloc::vec::Vec::with_capacity(coff.number_of_sections as usize);
    for i in 0..coff.number_of_sections as usize {
        let sh_offset = sections_offset + i * core::mem::size_of::<SectionHeader>();
        let sh: SectionHeader = read_struct(data, sh_offset).ok_or(PeError::TooSmall)?;
        sections.push(SectionInfo {
            name: sh.name,
            virtual_address: sh.virtual_address,
            virtual_size: sh.virtual_size,
            raw_data_offset: sh.pointer_to_raw_data,
            raw_data_size: sh.size_of_raw_data,
            characteristics: sh.characteristics,
        });
    }

    Ok(PeImage {
        machine: coff.machine,
        entry_point_rva: opt.address_of_entry_point,
        image_base: opt.image_base,
        size_of_image: opt.size_of_image,
        section_alignment: opt.section_alignment,
        sections,
        reloc_rva,
        reloc_size,
    })
}

/// Applies base relocations to a loaded PE image.
///
/// `image` is the loaded image bytes at the actual load address.
/// `delta` is the difference between the actual load address and the
/// preferred `image_base`.
///
/// # Safety
///
/// The caller must ensure `image` points to a valid, writable memory region
/// containing the loaded PE sections, and that `reloc_data` contains valid
/// base relocation entries.
pub unsafe fn apply_relocations(
    image: &mut [u8],
    reloc_data: &[u8],
    delta: i64,
) -> Result<(), PeError> {
    if delta == 0 || reloc_data.is_empty() {
        return Ok(());
    }

    let mut offset = 0;
    while offset + 8 <= reloc_data.len() {
        let block: BaseRelocationBlock =
            read_struct(reloc_data, offset).ok_or(PeError::RelocationOutOfBounds)?;

        if block.size_of_block < 8 {
            break;
        }

        let entry_count = (block.size_of_block as usize - 8) / 2;
        let entries_start = offset + 8;

        for i in 0..entry_count {
            let entry_offset = entries_start + i * 2;
            if entry_offset + 2 > reloc_data.len() {
                break;
            }
            let entry = u16::from_le_bytes(
                reloc_data[entry_offset..entry_offset + 2]
                    .try_into()
                    .unwrap(),
            );
            let reloc_type = entry >> 12;
            let reloc_offset = (entry & 0x0FFF) as u32;
            let target = (block.virtual_address + reloc_offset) as usize;

            match reloc_type {
                IMAGE_REL_BASED_ABSOLUTE => {} // Padding, skip.
                IMAGE_REL_BASED_DIR64 => {
                    if target + 8 > image.len() {
                        return Err(PeError::RelocationOutOfBounds);
                    }
                    let val = u64::from_le_bytes(image[target..target + 8].try_into().unwrap());
                    let new_val = (val as i64).wrapping_add(delta) as u64;
                    image[target..target + 8].copy_from_slice(&new_val.to_le_bytes());
                }
                IMAGE_REL_BASED_HIGHLOW => {
                    if target + 4 > image.len() {
                        return Err(PeError::RelocationOutOfBounds);
                    }
                    let val = u32::from_le_bytes(image[target..target + 4].try_into().unwrap());
                    let new_val = (val as i64).wrapping_add(delta) as u32;
                    image[target..target + 4].copy_from_slice(&new_val.to_le_bytes());
                }
                IMAGE_REL_BASED_HIGH => {
                    if target + 2 > image.len() {
                        return Err(PeError::RelocationOutOfBounds);
                    }
                    let val = u16::from_le_bytes(image[target..target + 2].try_into().unwrap());
                    let new_val = ((val as i32) + (delta >> 16) as i32) as u16;
                    image[target..target + 2].copy_from_slice(&new_val.to_le_bytes());
                }
                IMAGE_REL_BASED_LOW => {
                    if target + 2 > image.len() {
                        return Err(PeError::RelocationOutOfBounds);
                    }
                    let val = u16::from_le_bytes(image[target..target + 2].try_into().unwrap());
                    let new_val = ((val as i32) + delta as i32) as u16;
                    image[target..target + 2].copy_from_slice(&new_val.to_le_bytes());
                }
                _ => {} // Unknown relocation type — skip.
            }
        }

        offset += block.size_of_block as usize;
    }

    Ok(())
}

/// Reads a packed struct from a byte slice at the given offset.
fn read_struct<T: Copy>(data: &[u8], offset: usize) -> Option<T> {
    let size = core::mem::size_of::<T>();
    if offset + size > data.len() {
        return None;
    }
    // SAFETY: All PE structs are #[repr(C, packed)] with plain integer fields.
    // Any bit pattern is valid. We use read_unaligned for packed structs.
    Some(unsafe { core::ptr::read_unaligned(data[offset..].as_ptr().cast()) })
}

// Compile-time layout verification (§3.9.7).
const _: () = {
    assert!(core::mem::size_of::<DosHeader>() == 64);
    assert!(core::mem::size_of::<CoffHeader>() == 20);
    assert!(core::mem::size_of::<SectionHeader>() == 40);
    assert!(core::mem::size_of::<DataDirectory>() == 8);
    assert!(core::mem::size_of::<BaseRelocationBlock>() == 8);
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal valid PE32+ image for testing.
    fn build_test_pe() -> alloc::vec::Vec<u8> {
        let mut pe = alloc::vec![0u8; 512];

        // DOS header.
        pe[0..2].copy_from_slice(&DOS_MAGIC.to_le_bytes());
        pe[60..64].copy_from_slice(&128u32.to_le_bytes()); // e_lfanew

        // PE signature at offset 128.
        pe[128..132].copy_from_slice(&PE_MAGIC.to_le_bytes());

        // COFF header at offset 132.
        pe[132..134].copy_from_slice(&IMAGE_FILE_MACHINE_AMD64.to_le_bytes()); // machine
        pe[134..136].copy_from_slice(&1u16.to_le_bytes()); // number_of_sections
        pe[148..150].copy_from_slice(&112u16.to_le_bytes()); // size_of_optional_header

        // Optional header at offset 152.
        pe[152..154].copy_from_slice(&PE32PLUS_MAGIC.to_le_bytes()); // magic
        pe[168..172].copy_from_slice(&0x1000u32.to_le_bytes()); // entry point RVA
        pe[176..184].copy_from_slice(&0x0040_0000u64.to_le_bytes()); // image_base
        pe[184..188].copy_from_slice(&0x1000u32.to_le_bytes()); // section_alignment
        pe[188..192].copy_from_slice(&0x200u32.to_le_bytes()); // file_alignment
        pe[208..212].copy_from_slice(&0x3000u32.to_le_bytes()); // size_of_image
        pe[212..216].copy_from_slice(&0x200u32.to_le_bytes()); // size_of_headers
        pe[236..240].copy_from_slice(&6u32.to_le_bytes()); // number_of_rva_and_sizes

        // Section header at offset 264 (152 + 112).
        pe[264..272].copy_from_slice(b".text\0\0\0"); // name
        pe[272..276].copy_from_slice(&0x100u32.to_le_bytes()); // virtual_size
        pe[276..280].copy_from_slice(&0x1000u32.to_le_bytes()); // virtual_address
        pe[280..284].copy_from_slice(&0x200u32.to_le_bytes()); // size_of_raw_data
        pe[284..288].copy_from_slice(&0x200u32.to_le_bytes()); // pointer_to_raw_data
        pe[300..304].copy_from_slice(
            &(IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE | IMAGE_SCN_MEM_READ).to_le_bytes(),
        ); // characteristics

        pe
    }

    #[test]
    fn parse_valid_pe() {
        let pe = build_test_pe();
        let image = parse(&pe).unwrap();
        assert_eq!(image.machine, IMAGE_FILE_MACHINE_AMD64);
        assert_eq!(image.entry_point_rva, 0x1000);
        assert_eq!(image.image_base, 0x0040_0000);
        assert_eq!(image.sections.len(), 1);
        assert_eq!(image.sections[0].name_str(), ".text");
    }

    #[test]
    fn parse_too_small() {
        assert_eq!(parse(&[0; 10]), Err(PeError::TooSmall));
    }

    #[test]
    fn parse_bad_dos_magic() {
        let mut pe = build_test_pe();
        pe[0] = 0xFF;
        assert_eq!(parse(&pe), Err(PeError::InvalidDosMagic));
    }

    #[test]
    fn parse_bad_pe_magic() {
        let mut pe = build_test_pe();
        pe[128] = 0xFF;
        assert_eq!(parse(&pe), Err(PeError::InvalidPeMagic));
    }

    #[test]
    fn section_name_str() {
        let si = SectionInfo {
            name: *b".reloc\0\0",
            virtual_address: 0,
            virtual_size: 0,
            raw_data_offset: 0,
            raw_data_size: 0,
            characteristics: 0,
        };
        assert_eq!(si.name_str(), ".reloc");
    }
}
