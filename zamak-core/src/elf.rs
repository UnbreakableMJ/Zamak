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

        *target_ptr = value;
    }
}
