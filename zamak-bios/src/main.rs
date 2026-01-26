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
pub mod smp;
pub mod input;

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
use alloc::string::String;

fn fulfill_requests(
    mmap: &[protocol::MemmapEntry], 
    fb: Option<protocol::Framebuffer>, 
    kernel_file: Option<*const protocol::File>,
    modules: &[protocol::File],
    rsdp: Option<u64>,
    smp: Option<protocol::SmpResponse>,
    requests: &[*mut protocol::RawRequest]
) {
    for &req_ptr in requests {
        let req = unsafe { &mut *req_ptr };
        
        match req.id {
            protocol::BOOTLOADER_INFO_ID => {
                let response = Box::leak(Box::new(protocol::BootloaderInfoResponse {
                    name: Box::leak(Box::new("Zamak-Bios\0")).as_ptr() as u64,
                    version: Box::leak(Box::new("0.6.9\0")).as_ptr() as u64,
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
            protocol::RSDP_ID => {
                if let Some(addr) = rsdp {
                    let response = Box::leak(Box::new(protocol::RsdpResponse {
                        revision: 0,
                        address: addr,
                    }));
                    req.response = response as *mut _ as u64;
                }
            }
            protocol::KERNEL_FILE_ID => {
                if let Some(kf) = kernel_file {
                    let response = Box::leak(Box::new(protocol::KernelFileResponse {
                        revision: 0,
                        kernel_file: kf as u64,
                    }));
                    req.response = response as *mut _ as u64;
                }
            }
            protocol::MODULE_ID => {
                if !modules.is_empty() {
                    let mut file_ptrs = Vec::new();
                    for m in modules {
                        file_ptrs.push(Box::leak(Box::new(*m)) as *const _);
                    }
                    let file_list = Box::leak(file_ptrs.into_boxed_slice());
                    let response = Box::leak(Box::new(protocol::ModuleResponse {
                        revision: 0,
                        module_count: file_list.len() as u64,
                        modules: file_list.as_ptr() as u64,
                    }));
                    req.response = response as *mut _ as u64;
                }
            }
            protocol::SMP_ID => {
                if let Some(s) = smp {
                    let response = Box::leak(Box::new(s));
                    req.response = response as *mut _ as u64;
                }
            }
            _ => {}
        }
    }
}

use libzamak::rng::KaslrRng;

pub struct BiosRng;

impl KaslrRng for BiosRng {
    fn get_u64(&mut self) -> u64 {
        unsafe { core::arch::x86::_rdtsc() }
    }
}

#[no_mangle]
pub extern "C" fn kmain(drive_id: u8) -> ! {
    // 2. Initialize Disk
    let mut disk = Disk::new(drive_id);
    let mut disk_ext2 = disk.clone(); 
    
    // 3. Mount Filesystem
    // We try FAT32 first, then EXT2
    use libzamak::fs::FileSystem;
    use libzamak::ext2::Ext2;
    use crate::fat32::Fat32;

    let mut fs_fat: Option<Fat32> = None;
    let mut fs_ext2: Option<Ext2> = None;
    
    // Probe FAT32
    if let Ok(f) = Fat32::parse(&mut disk, 0) {
        fs_fat = Some(f);
    } 
    // If not FAT32, probe EXT2
    else if let Ok(f) = Ext2::mount(&mut disk_ext2, 0) {
        fs_ext2 = Some(f);
    } else {
        panic!("No supported filesystem found on boot partition");
    }

    let fs: &dyn FileSystem = if let Some(ref f) = fs_fat {
        f
    } else {
        fs_ext2.as_ref().unwrap()
    };
    
    // Read Config
    let mut config_file_buf = vec![0u8; 4096];
    let config_entry = fs.find_file("zamak.conf").expect("Missing zamak.conf");
    fs.read_file(&config_entry, &mut config_file_buf).expect("Failed to read config");
    
    let config_size = config_entry.size as usize;
    // Simple parser
    let config_str = core::str::from_utf8(&config_file_buf[..config_size]).unwrap_or("");
    let config = libzamak::config::parse(config_str);

    // 4. Initialize Graphics (VBE) for TUI
    let mut fb_opt = vbe::find_and_set_vbe_mode(1024, 768, 32); 
    if fb_opt.is_none() {
         fb_opt = vbe::find_and_set_vbe_mode(800, 600, 32);
    }
    
    let mut selected_idx = 0;
    
    // Initialize Logging (Serial)
    // Serial init not available yet, skipping.
    
    // Check for Network Boot (Stub)
    // Real PXE detection would check for !PXE structure in memory (0x0000-0xFFFF)
    // or generic Int 18h behavior. For now we assume Disk boot unless specified.
    // log::info!("Network Boot: Not Supported (Stub)");
    if let Some(mut fb) = fb_opt {

        // TUI Loop
        use libzamak::tui::{MenuState, draw_menu, Key, InputSource};
        use libzamak::font::{PsfFont, DEFAULT_FONT};
        use libzamak::gfx::Canvas;
        use crate::input::BiosInput;

        let font = PsfFont::parse(DEFAULT_FONT).unwrap();
        let mut canvas = Canvas::new(&mut fb);
        let mut input = BiosInput;
        
        let mut state = MenuState::new(config.timeout); 
        let mut time_remaining = config.timeout * 10;
        
        loop {
            // Draw
            draw_menu(&mut canvas, &font, &config, &state, time_remaining);
            
            // Poll Input
            let key = input.read_key();
            
            // Handle Input
            match key {
                Key::Up | Key::Char('k') => {
                    if state.selected_idx > 0 { state.selected_idx -= 1; }
                    time_remaining = 0; // Stop timeout
                },
                Key::Down | Key::Char('j') => {
                    if state.selected_idx < config.entries.len() - 1 { state.selected_idx += 1; }
                    time_remaining = 0;
                },
                Key::Edit | Key::Char('i') => {
                    state.editing = !state.editing;
                    time_remaining = 0;
                    if state.editing {
                        // Populate buffer with current cmdline
                        if let Some(entry) = config.entries.get(state.selected_idx) {
                             state.edit_buffer = String::from(&entry.cmdline);
                        }
                    }
                },
                // Basic char input for editing
                Key::Char(c) if state.editing => {
                    if c == '\n' { 
                         // handled by Enter match below
                    } else {
                        state.edit_buffer.push(c);
                    }
                },
                Key::Esc => {
                    if state.editing {
                        state.editing = false;
                    }
                },
                Key::Enter => {
                    if state.editing {
                        state.editing = false;
                        // Commit?
                    } else {
                        selected_idx = state.selected_idx;
                        break;
                    }
                },
                _ => {}
            }
            
            // Timeout logic
            if time_remaining > 0 {
                // simple wait
                for _ in 0..5000000 { unsafe { core::arch::asm!("pause"); } }
                time_remaining -= 1;
                if time_remaining == 0 {
                    break; 
                }
            } else {
                 // Fast poll UI
                 for _ in 0..100000 { unsafe { core::arch::asm!("pause"); } }
            }
        }
    }
    
    // Load Kernel
    let selected_entry = &config.entries[selected_idx];
    let kernel_path = &selected_entry.kernel_path;

    // Load Kernel File
    let kernel_entry = fs.find_file(kernel_path).expect("Kernel not found");
    let mut kernel_buf = vec![0u8; kernel_entry.size as usize];
    fs.read_file(&kernel_entry, &mut kernel_buf).expect("Failed to read kernel");

    // Load Modules
    let loaded_modules = Vec::new();
    if !selected_entry.modules.is_empty() {
        for mod_cfg in &selected_entry.modules {
             let mut _m_buf = vec![0u8; 0]; // Simplified module load
             // In real impl we would load it
             // Placeholder for now as we don't have modules
             let _ = mod_cfg;
        }
    }
    
    // Parse ELF
    // Parse ELF
    let current_video_mode = fb_opt; // Pass the active VBE mode
    let mut info = libzamak::elf::parse_elf(&kernel_buf).expect("Invalid ELF kernel");

    // Gather Memory Map
    let mmap_entries = get_memory_map();
    
    let mut kernel_vaddr_start = 0xffffffff80000000;
    
    if info.is_pie {
        let mut rng = BiosRng;
        // Limit randomness to avoid mapping conflicts or OOM
        // 0 to 256 * 2MB = 512MB variance
        let offset = (rng.get_u64() % 256) * 0x200000;
        kernel_vaddr_start += offset;
        
        unsafe {
            libzamak::elf::apply_relocations(
                kernel_buf.as_mut_ptr(),
                kernel_vaddr_start,
                &info.relocations
            );
        }
        
        // Adjust entry point if it's relative
        info.entry = kernel_vaddr_start + info.entry;
    }
    
    let kernel_size = kernel_buf.len();

    // Prepare ACPI/RSDP
    let rsdp = find_rsdp();

    // SMP Discovery and Startup
    let mut smp_response = None;
    if let Some(rsdp_addr) = rsdp {
        let (lapic_addr, cpus) = smp::parse_madt(rsdp_addr);
        let pml4 = paging::setup_paging(info.segments[0].paddr, kernel_vaddr_start, kernel_size);
        let smp_list = smp::start_aps(lapic_addr, &cpus, pml4.as_u64());
        
        let mut smp_info_ptrs = Vec::new();
        for info in smp_list {
            smp_info_ptrs.push(Box::leak(Box::new(info)) as *const protocol::SmpInfo);
        }
        let smp_ptr = Box::leak(smp_info_ptrs.into_boxed_slice());

        smp_response = Some(protocol::SmpResponse {
            revision: 0,
            flags: 0,
            bsp_lapic_id: unsafe { *((lapic_addr + 0x20) as *const u32) >> 24 } as u32,
            cpu_count: smp_ptr.len() as u64,
            cpus: smp_ptr.as_ptr() as u64,
        });
    }

    // Prepare Kernel File
    let kf_data = Box::leak(kernel_buf.into_boxed_slice());
    let kf = Box::leak(Box::new(protocol::File {
        revision: 0,
        address: kf_data.as_ptr() as u64,
        size: kf_data.len() as u64,
        path: Box::leak(Box::new(String::from(kernel_path))).as_ptr() as u64, 
        cmdline: Box::leak(Box::new(String::from(&selected_entry.cmdline))).as_ptr() as u64, 
        ..Default::default()
    }));

    // Scan and fulfill requests in the LOADED kernel
    let mut all_requests = Vec::new();
    for seg in &info.segments {
        let seg_ptr = seg.paddr as *const u8;
        let seg_slice = unsafe { core::slice::from_raw_parts(seg_ptr, seg.mem_size as usize) };
        let mut reqs = protocol::scan_requests(seg_slice);
        all_requests.append(&mut reqs);
    }
    
    fulfill_requests(&mmap_entries, current_video_mode, Some(kf), &loaded_modules, rsdp, smp_response, &all_requests);

    // Setup Paging
    let pml4 = paging::setup_paging(info.segments[0].paddr, kernel_vaddr_start, kernel_size);
    
    // Enter Long Mode
    unsafe { enter_long_mode(pml4.as_u64() as u32, info.entry); }
    
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

pub fn find_rsdp() -> Option<u64> {
    // Search 0xE0000 to 0xFFFFF
    let start = 0xE0000 as *const u8;
    for i in (0..0x20000).step_by(16) {
        let ptr = unsafe { start.add(i) };
        let slice = unsafe { core::slice::from_raw_parts(ptr, 8) };
        if slice == b"RSD PTR " {
            return Some(ptr as u64);
        }
    }
    None
}
