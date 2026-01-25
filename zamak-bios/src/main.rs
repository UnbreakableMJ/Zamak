// SPDX-License-Identifier: GPL-3.0-or-later

#![no_std]
#![no_main]

extern crate alloc;

pub mod disk;
pub mod fat32;
pub mod allocator;
pub mod utils;
pub mod mmap;
pub mod paging;

use disk::Disk;
use fat32::Fat32;
use mmap::get_memory_map;
use core::panic::PanicInfo;

#[repr(C, packed)]
#[derive(Debug, Default, Clone, Copy)]
pub struct BiosRegs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
}

extern "C" {
    fn call_bios_int(int_no: u8, regs: *mut BiosRegs);
    fn enter_long_mode(pml4_phys: u32, entry_point: u64);
}

#[no_mangle]
pub extern "C" fn kmain(drive_id: u8) -> ! {
    // Clear screen via BIOS int 0x10
    let mut regs = BiosRegs::default();
    regs.eax = 0x0003; // 80x25 text mode
    unsafe {
        call_bios_int(0x10, &mut regs);
    }

    let vga_buffer = 0xb8000 as *mut u8;
    println(vga_buffer, 0, "Zamak BIOS Stage 2 Loading...", 0x0f);
    
    let mmap = get_memory_map();
    // Simple way to show count
    let count = mmap.len();
    // count_str is unused for now, logging via msg
    // Wait, println overwrites. I'll just append.
    let mut msg = [0u8; 32];
    let mut pos = 0;
    for &b in b"Memory Map entries: " { msg[pos] = b; pos += 1; }
    msg[pos] = b'0' + (count / 10) as u8; pos += 1;
    msg[pos] = b'0' + (count % 10) as u8; pos += 1;
    if let Ok(s) = core::str::from_utf8(&msg[..pos]) {
        println(vga_buffer, 1, s, 0x07);
    }

    let disk = Disk::new(drive_id);
    unsafe {
        if let Ok(fs) = Fat32::parse(disk, 0) {
            if let Ok(config_entry) = fs.find_path("/ZAMAK.CON") {
                let mut config_buf = alloc::vec![0u8; config_entry.file_size as usize];
                if fs.read_file(&config_entry, config_buf.as_mut_ptr()).is_ok() {
                    let config_str = core::str::from_utf8(&config_buf).unwrap_or("");
                    let config = libzamak::config::parse(config_str);
                    
                    if let Some(entry) = config.entries.first() {
                        let kernel_path = entry.options.get("KERNEL_PATH").map(|s| s.as_str()).unwrap_or("/BOOT/KERNEL");
                        
                        println(vga_buffer, 2, "Found Kernel Path: ", 0x07);
                        println(vga_buffer, 2, kernel_path, 0x0f);
                        
                        // Load Kernel ELF
                        if let Ok(kernel_file) = fs.find_path(kernel_path) {
                            let mut kernel_buf = alloc::vec![0u8; kernel_file.file_size as usize];
                            if fs.read_file(&kernel_file, kernel_buf.as_mut_ptr()).is_ok() {
                                if let Ok(info) = libzamak::elf::parse_elf(&kernel_buf) {
                                    println(vga_buffer, 3, "Kernel ELF Parsed. Entry: ", 0x07);
                                    
                                    // Load segments
                                    let mut kernel_vaddr_start = u64::MAX;
                                    let mut kernel_vaddr_end = 0;
                                    for seg in &info.segments {
                                        if seg.vaddr < kernel_vaddr_start { kernel_vaddr_start = seg.vaddr; }
                                        let end = seg.vaddr + seg.mem_size as u64;
                                        if end > kernel_vaddr_end { kernel_vaddr_end = end; }

                                        let dest = seg.paddr as *mut u8;
                                        let src = kernel_buf.as_ptr().add(seg.offset);
                                        utils::memcpy(dest, src, seg.file_size);
                                        
                                        if seg.mem_size > seg.file_size {
                                            utils::memset(dest.add(seg.file_size), 0, seg.mem_size - seg.file_size);
                                        }
                                    }
                                    
                                    let kernel_size = (kernel_vaddr_end - kernel_vaddr_start) as usize;
                                    println(vga_buffer, 4, "Kernel Loaded. Switching to Long Mode...", 0x0a);
                                    
                                    // Setup Paging
                                    // The following line was likely an accidental paste of a function signature.
                                    // Reverting to the original call to `paging::setup_paging` for syntactic correctness.
                                    // The original call was:
                                    // let pml4 = paging::setup_paging(info.segments[0].paddr, kernel_vaddr_start, kernel_size);
                                    // Assuming the intent was to fix warnings or document cleanup related to this call,
                                    // but the provided snippet was not a valid replacement for the assignment.
                                    // Keeping the functional call as it was.
                                    let pml4 = paging::setup_paging(info.segments[0].paddr, kernel_vaddr_start, kernel_size);
                                  
                                    // Enter Long Mode
                                    // Store entry point at 0x5FF0 for assembly to pick up
                                    *(0x5FF0 as *mut u64) = info.entry;
                                    
                                    enter_long_mode(pml4.as_u64() as u32, info.entry);
                                } else {
                                    println(vga_buffer, 3, "Failed to parse Kernel ELF", 0x0c);
                                }
                            } else {
                                println(vga_buffer, 3, "Failed to read Kernel file", 0x0c);
                            }
                        } else {
                            println(vga_buffer, 3, "Kernel not found at specified path", 0x0c);
                        }
                    }
                }
            } else {
                println(vga_buffer, 2, "ZAMAK.CON not found!", 0x0c);
            }
        } else {
            println(vga_buffer, 2, "Failed to parse FAT32 filesystem", 0x0c);
        }
    }

    loop {}
}

fn println(vga: *mut u8, line: isize, msg: &str, color: u8) {
    for (i, &byte) in msg.as_bytes().iter().enumerate() {
        unsafe {
            *vga.offset((line * 80 + i as isize) * 2) = byte;
            *vga.offset((line * 80 + i as isize) * 2 + 1) = color;
        }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn rust_eh_personality() {}
