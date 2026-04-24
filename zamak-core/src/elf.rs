// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

use alloc::vec::Vec;
use goblin::elf::{program_header::PT_LOAD, reloc::R_X86_64_RELATIVE, Elf};

#[derive(Debug)]
pub struct ElfInfo {
    pub entry: u64,
    pub segments: Vec<ElfSegment>,
    pub relocations: Vec<Relocation>,
    pub is_pie: bool,
}

#[derive(Debug)]
pub struct ElfSegment {
    pub paddr: u64,
    pub vaddr: u64,
    pub mem_size: usize,
    pub file_size: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Relocation {
    pub offset: u64,
    pub addend: i64,
}

pub fn parse_elf(bytes: &[u8]) -> Result<ElfInfo, &'static str> {
    let elf = Elf::parse(bytes).map_err(|_| "Failed to parse ELF")?;

    if !elf.is_64 {
        return Err("Only 64-bit ELF kernels are supported");
    }

    let is_pie = elf.header.e_type == goblin::elf::header::ET_DYN;

    let mut segments = Vec::new();
    for ph in &elf.program_headers {
        if ph.p_type == PT_LOAD {
            segments.push(ElfSegment {
                paddr: ph.p_paddr,
                vaddr: ph.p_vaddr,
                mem_size: ph.p_memsz as usize,
                file_size: ph.p_filesz as usize,
                offset: ph.p_offset as usize,
            });
        }
    }

    let mut relocations = Vec::new();
    for reloc in &elf.dynrelas {
        if reloc.r_type == R_X86_64_RELATIVE {
            // R_X86_64_RELATIVE = 8.
            // It means B + A.
            relocations.push(Relocation {
                offset: reloc.r_offset,
                addend: reloc.r_addend.unwrap_or(0),
            });
        }
    }

    Ok(ElfInfo {
        entry: elf.entry,
        segments,
        relocations,
        is_pie,
    })
}

/// Applies relocations to the loaded kernel (in-place).
///
/// # Arguments
/// * `phys_base` - The pointer to the physical memory where the kernel is loaded.
/// * `virt_base` - The virtual base address where the kernel will be mapped.
/// * `relocations` - The list of relocations to apply.
///
/// # Safety
///
/// `phys_base` must point to at least `max(reloc.offset) + 8` bytes of writable
/// memory that holds the loaded kernel image. Each relocation's `offset` must
/// fit within that region; callers typically obtain relocations from the same
/// ELF parser that determines the image size.
pub unsafe fn apply_relocations(phys_base: *mut u8, virt_base: u64, relocations: &[Relocation]) {
    for reloc in relocations {
        // Target is at phys_base + offset
        let target_ptr = phys_base.add(reloc.offset as usize) as *mut u64;

        // Value is virt_base + addend
        // We use wrapping_add because addend is signed (though usually positive for local symbols,
        // relative relocations often have 0 addend or relative to section).
        // For R_X86_64_RELATIVE, the value should be Base + Addend.
        let value = virt_base.wrapping_add(reloc.addend as u64);

        // Use unaligned write: the function's contract (phys_base: *mut u8)
        // does not require 8-byte alignment, and Rust `*ptr = value` on
        // *mut u64 is UB when the address isn't 8-aligned even though
        // x86-64/aarch64 tolerate it in hardware.
        target_ptr.write_unaligned(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid 64-bit ET_EXEC ELF with one PT_LOAD
    /// program header. Used to exercise `parse_elf` without pulling
    /// in a real toolchain.
    fn mk_minimal_elf64(entry: u64, pie: bool) -> Vec<u8> {
        // ELF64 header is 64 bytes; one program header is 56 bytes.
        let mut img = Vec::with_capacity(64 + 56);
        // e_ident.
        img.extend_from_slice(&[
            0x7F, b'E', b'L', b'F', // magic
            2,    // EI_CLASS = ELFCLASS64
            1,    // EI_DATA  = ELFDATA2LSB (little-endian)
            1,    // EI_VERSION = EV_CURRENT
            0,    // EI_OSABI = SYSV
            0,    // EI_ABIVERSION
            0, 0, 0, 0, 0, 0, 0, // padding (7 bytes)
        ]);
        // e_type: ET_EXEC (2) or ET_DYN (3 = PIE).
        img.extend_from_slice(&(if pie { 3u16 } else { 2u16 }).to_le_bytes());
        // e_machine: EM_X86_64 = 62.
        img.extend_from_slice(&62u16.to_le_bytes());
        // e_version = 1.
        img.extend_from_slice(&1u32.to_le_bytes());
        // e_entry.
        img.extend_from_slice(&entry.to_le_bytes());
        // e_phoff = 64 (right after ehdr).
        img.extend_from_slice(&64u64.to_le_bytes());
        // e_shoff = 0.
        img.extend_from_slice(&0u64.to_le_bytes());
        // e_flags.
        img.extend_from_slice(&0u32.to_le_bytes());
        // e_ehsize = 64.
        img.extend_from_slice(&64u16.to_le_bytes());
        // e_phentsize = 56.
        img.extend_from_slice(&56u16.to_le_bytes());
        // e_phnum = 1.
        img.extend_from_slice(&1u16.to_le_bytes());
        // e_shentsize / e_shnum / e_shstrndx = 0.
        img.extend_from_slice(&[0u8; 6]);

        // One PT_LOAD program header.
        img.extend_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
        img.extend_from_slice(&5u32.to_le_bytes()); // p_flags = R|X
        img.extend_from_slice(&0x1000u64.to_le_bytes()); // p_offset
        img.extend_from_slice(&0xFFFFFFFF80000000u64.to_le_bytes()); // p_vaddr
        img.extend_from_slice(&0x200000u64.to_le_bytes()); // p_paddr
        img.extend_from_slice(&0x1000u64.to_le_bytes()); // p_filesz
        img.extend_from_slice(&0x2000u64.to_le_bytes()); // p_memsz (> filesz → BSS)
        img.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align

        debug_assert_eq!(img.len(), 64 + 56);
        img
    }

    #[test]
    fn parse_rejects_non_elf() {
        let err = parse_elf(&[0, 1, 2, 3, 4]).unwrap_err();
        assert!(err.contains("ELF"));
    }

    #[test]
    fn parse_rejects_empty() {
        parse_elf(&[]).unwrap_err();
    }

    #[test]
    fn parse_minimal_elf64_exec_extracts_entry_and_segment() {
        let bytes = mk_minimal_elf64(0xFFFFFFFF80001000, false);
        let info = parse_elf(&bytes).expect("minimal ELF must parse");
        assert_eq!(info.entry, 0xFFFFFFFF80001000);
        assert!(!info.is_pie);
        assert_eq!(info.segments.len(), 1);
        let seg = &info.segments[0];
        assert_eq!(seg.vaddr, 0xFFFFFFFF80000000);
        assert_eq!(seg.paddr, 0x200000);
        assert_eq!(seg.file_size, 0x1000);
        assert_eq!(seg.mem_size, 0x2000);
        assert_eq!(seg.offset, 0x1000);
        assert!(info.relocations.is_empty());
    }

    #[test]
    fn parse_minimal_elf64_pie_sets_is_pie() {
        let bytes = mk_minimal_elf64(0x1000, true);
        let info = parse_elf(&bytes).expect("PIE ELF must parse");
        assert!(info.is_pie);
    }

    #[test]
    fn apply_relocations_writes_virt_plus_addend() {
        // Allocate 64 bytes of heap; write a relocation at offset 8
        // and verify the 8-byte slot there holds virt_base + addend.
        let mut buf = alloc::vec![0u8; 64];
        let virt_base: u64 = 0xFFFFFFFF80000000;
        let relocs = [
            Relocation {
                offset: 8,
                addend: 0x1234,
            },
            Relocation {
                offset: 32,
                addend: -16,
            },
        ];
        unsafe {
            apply_relocations(buf.as_mut_ptr(), virt_base, &relocs);
        }
        let slot_a = u64::from_le_bytes(buf[8..16].try_into().unwrap());
        let slot_b = u64::from_le_bytes(buf[32..40].try_into().unwrap());
        assert_eq!(slot_a, virt_base.wrapping_add(0x1234));
        assert_eq!(slot_b, virt_base.wrapping_add((-16i64) as u64));
    }
}
