// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::vec::Vec;
use goblin::elf::{Elf, program_header::PT_LOAD};

#[derive(Debug)]
pub struct ElfInfo {
    pub entry: u64,
    pub segments: Vec<ElfSegment>,
}

#[derive(Debug)]
pub struct ElfSegment {
    pub paddr: u64,
    pub vaddr: u64,
    pub mem_size: usize,
    pub file_size: usize,
    pub offset: usize,
}

pub fn parse_elf(bytes: &[u8]) -> Result<ElfInfo, &'static str> {
    let elf = Elf::parse(bytes).map_err(|_| "Failed to parse ELF")?;
    
    if !elf.is_64 {
        return Err("Only 64-bit ELF kernels are supported");
    }

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

    Ok(ElfInfo {
        entry: elf.entry,
        segments,
    })
}
