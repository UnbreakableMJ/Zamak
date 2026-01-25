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
pub mod vbe;

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

use libzamak::protocol;
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::vec;

fn fulfill_requests(mmap: &[protocol::MemmapEntry], fb: Option<protocol::Framebuffer>, requests: &[*mut protocol::RawRequest]) {
    for &req_ptr in requests {
        let req = unsafe { &mut *req_ptr };
        
        match req.id {
            protocol::BOOTLOADER_INFO_ID => {
                let response = Box::leak(Box::new(protocol::BootloaderInfoResponse {
                    name: Box::leak(Box::new("Zamak-Bios\0")).as_ptr() as u64,
                    version: Box::leak(Box::new("0.3.0\0")).as_ptr() as u64,
                }));
                req.response = response as *mut _ as u64;
            }
            protocol::HHDM_ID => {
                let response = Box::leak(Box::new(protocol::HhdmResponse {
                    revision: 0,
                    offset: 0xffff800000000000u64,
                }));
                req.response = response as *mut _ as u64;
            }
            protocol::MEMMAP_ID => {
                let entries_ptr = Box::leak(mmap.to_vec().into_boxed_slice());
                let response = Box::leak(Box::new(protocol::MemmapResponse {
                    revision: 0,
                    entry_count: entries_ptr.len() as u64,
                    entries: entries_ptr.as_ptr() as u64,
                }));
                req.response = response as *mut _ as u64;
            }
            protocol::FRAMEBUFFER_ID => {
                if let Some(framebuf) = fb {
                    let fb_ptr = Box::leak(Box::new(framebuf));
                    let fb_list: &mut [*const protocol::Framebuffer] = Box::leak(vec![fb_ptr as *const _].into_boxed_slice());
                    let response = Box::leak(Box::new(protocol::FramebufferResponse {
                        revision: 0,
                        framebuffer_count: 1,
                        framebuffers: fb_list.as_ptr() as u64,
                    }));
                    req.response = response as *mut _ as u64;
                }
            }
            _ => {}
        }
    }
}

#[no_mangle]
pub extern "C" fn kmain(drive_id: u8) -> ! {
    // Try to set VBE mode early
    // We try 1024x768x32 first
    let vbe_fb = vbe::find_and_set_vbe_mode(1024, 768, 32);
    
    if vbe_fb.is_none() {
        // Fallback to text mode if VBE fails
        let mut regs = BiosRegs::default();
        regs.eax = 0x0003; 
        unsafe { call_bios_int(0x10, &mut regs); }
    }

    let vga_buffer = 0xb8000 as *mut u8;
    if vbe_fb.is_none() {
        println(vga_buffer, 0, "Zamak BIOS Stage 2 Loading...", 0x0f);
    }
    
    let mmap_entries = get_memory_map();
    if vbe_fb.is_none() {
        let count = mmap_entries.len();
        let mut msg = [0u8; 32];
        let mut pos = 0;
        for &b in b"Memory Map entries: " { msg[pos] = b; pos += 1; }
        msg[pos] = b'0' + (count / 10) as u8; pos += 1;
        msg[pos] = b'0' + (count % 10) as u8; pos += 1;
        if let Ok(s) = core::str::from_utf8(&msg[..pos]) {
            println(vga_buffer, 1, s, 0x07);
        }
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
                        
                        // Load Kernel ELF
                        if let Ok(kernel_file) = fs.find_path(kernel_path) {
                            let mut kernel_buf = alloc::vec![0u8; kernel_file.file_size as usize];
                            if fs.read_file(&kernel_file, kernel_buf.as_mut_ptr()).is_ok() {
                                if let Ok(info) = libzamak::elf::parse_elf(&kernel_buf) {
                                    
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
                                    
                                    // Scan and fulfill requests in the LOADED kernel
                                    // The requests are in the physical memory we just copied to
                                    // We need to scan the segments
                                    let mut all_requests = Vec::new();
                                    for seg in &info.segments {
                                        let seg_ptr = seg.paddr as *const u8;
                                        let seg_slice = core::slice::from_raw_parts(seg_ptr, seg.mem_size as usize);
                                        let mut reqs = protocol::scan_requests(seg_slice);
                                        all_requests.append(&mut reqs);
                                    }
                                    
                                    fulfill_requests(&mmap_entries, vbe_fb, &all_requests);

                                    // Setup Paging
                                    let pml4 = paging::setup_paging(info.segments[0].paddr, kernel_vaddr_start, kernel_size);
                                    
                                    // Enter Long Mode
                                    *(0x5FF0 as *mut u64) = info.entry;
                                    enter_long_mode(pml4.as_u64() as u32, info.entry);
                                }
                            }
                        }
                    }
                }
            }
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
