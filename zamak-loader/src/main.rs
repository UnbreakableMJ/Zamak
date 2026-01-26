// SPDX-License-Identifier: GPL-3.0-or-later

#![no_std]
#![no_main]

extern crate alloc;

use uefi::prelude::*;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::media::file::{File, FileMode, FileAttribute, FileInfo};
use uefi::table::boot::{AllocateType, MemoryType};
use log::{info, error};
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use libzamak::config;
use libzamak::elf;
use libzamak::protocol;

use x86_64::{
    structures::paging::{
        PageTable, PageTableFlags, OffsetPageTable, Mapper, Size4KiB, FrameAllocator, PhysFrame, Page,
    },
    PhysAddr, VirtAddr,
};

struct UefiFrameAllocator<'a>(&'a BootServices);

unsafe impl FrameAllocator<Size4KiB> for UefiFrameAllocator<'_> {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let addr = self.0.allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1).ok()?;
        Some(PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

struct LoadedKernel {
    phys_base: u64,
    vaddr_start: u64,
    size: usize,
}

fn load_kernel_segments(boot_services: &BootServices, elf_info: &elf::ElfInfo, kernel_data: &[u8]) -> LoadedKernel {
    let mut min_vaddr = u64::MAX;
    let mut max_vaddr = 0;

    for segment in &elf_info.segments {
        if segment.vaddr < min_vaddr { min_vaddr = segment.vaddr; }
        let end = segment.vaddr + segment.mem_size as u64;
        if end > max_vaddr { max_vaddr = end; }
    }

    // Align to 4KiB
    let vaddr_start = min_vaddr & !0xfff;
    let vaddr_end = (max_vaddr + 0xfff) & !0xfff;
    let size = (vaddr_end - vaddr_start) as usize;
    let pages = size / 4096;

    info!("Kernel virtual range: {:#x} - {:#x} ({} bytes, {} pages)", vaddr_start, vaddr_end, size, pages);

    let phys_base = boot_services.allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .expect("Failed to allocate physical memory for kernel");
    
    // Clear memory (BSS)
    unsafe {
        core::ptr::write_bytes(phys_base as *mut u8, 0, size);
    }

    // Copy segments
    for segment in &elf_info.segments {
        let offset = (segment.vaddr - vaddr_start) as usize;
        let dest = (phys_base + offset as u64) as *mut u8;
        let src = &kernel_data[segment.offset..segment.offset + segment.file_size];
        
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dest, segment.file_size);
        }
        info!("  Loaded segment: vaddr {:#x} -> phys {:#x} ({} bytes)", segment.vaddr, phys_base + offset as u64, segment.file_size);
    }

    LoadedKernel { phys_base, vaddr_start, size }
}

fn setup_paging(boot_services: &BootServices, loaded_kernel: &LoadedKernel) -> PhysAddr {
    let mut allocator = UefiFrameAllocator(boot_services);
    
    // Allocate a new PML4 table
    let pml4_frame = allocator.allocate_frame().expect("Failed to allocate PML4 frame");
    let pml4_ptr = pml4_frame.start_address().as_u64() as *mut PageTable;
    
    unsafe {
        core::ptr::write_bytes(pml4_ptr, 0, 1);
    }
    
    // Create an OffsetPageTable with offset 0 because we assume UEFI has identity mapping for these addresses
    let mut mapper = unsafe { OffsetPageTable::new(&mut *pml4_ptr, VirtAddr::new(0)) };

    // 1. Map Kernel segments
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE; // Simple flags for now
    let start_page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(loaded_kernel.vaddr_start));
    let end_page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(loaded_kernel.vaddr_start + loaded_kernel.size as u64 - 1));

    for page in Page::range_inclusive(start_page, end_page) {
        let offset = page.start_address().as_u64() - loaded_kernel.vaddr_start;
        let frame = PhysFrame::containing_address(PhysAddr::new(loaded_kernel.phys_base + offset));
        unsafe {
            mapper.map_to(page, frame, flags, &mut allocator).expect("Failed to map kernel page").flush();
        }
    }

    // 2. Map HHDM (Higher Half Direct Map)
    // For simplicity, we map the first 4GB to 0xffff800000000000
    let hhdm_offset = 0xffff800000000000u64;
    let _hhdm_start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(hhdm_offset));
    
    // We'll map 4GB for now, using 2MB pages would be better but let's stay with 4K for UefiFrameAllocator simple use
    // Actually mapping 4GB with 4K pages will take MANY uefi allocations.
    // Let's just map the first 1GB for now.
    for i in 0..262144 { // 1GB / 4KB
        let phys_addr = i * 4096;
        let virt_addr = hhdm_offset + phys_addr;
        let page: Page<Size4KiB> = Page::containing_address(VirtAddr::new(virt_addr));
        let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
        unsafe {
            mapper.map_to(page, frame, flags, &mut allocator).expect("Failed to map HHDM page").ignore();
        }
    }

    pml4_frame.start_address()
}

fn read_file(file: &mut uefi::proto::media::file::RegularFile) -> Vec<u8> {
    let mut i_buffer = [0u8; 256];
    let info: &FileInfo = file.get_info(&mut i_buffer).expect("Failed to get file info");
    let size = info.file_size() as usize;

    let mut data = vec![0; size];
    file.read(&mut data).expect("Failed to read file");
    data
}

fn get_config_table_addr(system_table: &SystemTable<Boot>, guid: uefi::Guid) -> Option<u64> {
    for entry in system_table.config_table() {
        if entry.guid == guid {
            return Some(entry.address as u64);
        }
    }
    None
}

fn fulfill_requests(
    system_table: &SystemTable<Boot>, 
    kernel_file: Option<*const protocol::File>,
    modules: &[protocol::File],
    requests: &[*mut protocol::RawRequest]
) {
    for &req_ptr in requests {
        let req = unsafe { &mut *req_ptr };
        info!("Processing request: ID {:x} {:x}", req.id[0], req.id[1]);

        match req.id {
            protocol::BOOTLOADER_INFO_ID => {
                let response = Box::leak(Box::new(protocol::BootloaderInfoResponse {
                    name: Box::leak(Box::new("Zamak\0")).as_ptr() as u64,
                    version: Box::leak(Box::new("0.5.0\0")).as_ptr() as u64,
                }));
                req.response = response as *mut _ as u64;
                info!("  -> Fulfilled BOOTLOADER_INFO");
            }
            protocol::HHDM_ID => {
                let response = Box::leak(Box::new(protocol::HhdmResponse {
                    revision: 0,
                    offset: 0xffff800000000000u64,
                }));
                req.response = response as *mut _ as u64;
                info!("  -> Fulfilled HHDM");
            }
            protocol::MEMMAP_ID => {
                let boot_services = system_table.boot_services();
                let mmap_size = boot_services.memory_map_size();
                let mut mmap_buffer = vec![0u8; mmap_size.map_size + 1024]; // Extra space for changes
                let mmap = boot_services.memory_map(&mut mmap_buffer)
                    .expect("Failed to get memory map");
                let mmap_iter = mmap.entries();

                let mut entries = Vec::new();
                for desc in mmap_iter {
                    let typ = match desc.ty {
                        uefi::table::boot::MemoryType::CONVENTIONAL => protocol::MEMMAP_USABLE,
                        uefi::table::boot::MemoryType::LOADER_CODE | uefi::table::boot::MemoryType::LOADER_DATA => protocol::MEMMAP_BOOTLOADER_RECLAIMABLE,
                        uefi::table::boot::MemoryType::ACPI_RECLAIM => protocol::MEMMAP_ACPI_RECLAIMABLE,
                        uefi::table::boot::MemoryType::ACPI_NON_VOLATILE => protocol::MEMMAP_ACPI_NVS,
                        uefi::table::boot::MemoryType::UNUSABLE => protocol::MEMMAP_BAD_MEMORY,
                        _ => protocol::MEMMAP_RESERVED,
                    };
                    entries.push(protocol::MemmapEntry {
                        base: desc.phys_start,
                        length: desc.page_count * 4096,
                        typ,
                    });
                }

                let entries_ptr = Box::leak(entries.into_boxed_slice());
                let response = Box::leak(Box::new(protocol::MemmapResponse {
                    revision: 0,
                    entry_count: entries_ptr.len() as u64,
                    entries: entries_ptr.as_ptr() as u64,
                }));
                req.response = response as *mut _ as u64;
                info!("  -> Fulfilled MEMMAP ({} entries)", entries_ptr.len());
            }
            protocol::FRAMEBUFFER_ID => {
                let boot_services = system_table.boot_services();
                if let Ok(gop_handle) = boot_services.get_handle_for_protocol::<GraphicsOutput>() {
                    let mut gop = boot_services.open_protocol_exclusive::<GraphicsOutput>(gop_handle)
                        .expect("Failed to open GOP");
                    
                    let mode_info = gop.current_mode_info();
                    let (width, height) = mode_info.resolution();
                    let mut fb_ptr = gop.frame_buffer();
                    
                    let fb = Box::leak(Box::new(protocol::Framebuffer {
                        address: fb_ptr.as_mut_ptr() as u64,
                        width: width as u64,
                        height: height as u64,
                        pitch: (mode_info.stride() * 4) as u64, // Assume 32bpp for now
                        bpp: 32,
                        memory_model: 1, // RGB
                        red_mask_size: 8,
                        red_mask_shift: 0,
                        green_mask_size: 8,
                        green_mask_shift: 8,
                        blue_mask_size: 8,
                        blue_mask_shift: 16,
                        unused: [0; 7],
                        edid_size: 0,
                        edid: 0,
                    }));

                    let fb_list = Box::leak(vec![fb as *const _].into_boxed_slice());
                    let response = Box::leak(Box::new(protocol::FramebufferResponse {
                        revision: 0,
                        framebuffer_count: 1,
                        framebuffers: fb_list.as_ptr() as u64,
                    }));
                    req.response = response as *mut _ as u64;
                    info!("  -> Fulfilled FRAMEBUFFER ({}x{})", width, height);
                } else {
                    error!("  -> Failed to locate GOP for FRAMEBUFFER request");
                }
            }
            protocol::RSDP_ID => {
                let acpi_2_guid = uefi::table::cfg::ACPI2_GUID;
                let acpi_1_guid = uefi::table::cfg::ACPI_GUID;
                let addr = get_config_table_addr(system_table, acpi_2_guid)
                    .or_else(|| get_config_table_addr(system_table, acpi_1_guid));
                
                if let Some(a) = addr {
                    let response = Box::leak(Box::new(protocol::RsdpResponse {
                        revision: 0,
                        address: a,
                    }));
                    req.response = response as *mut _ as u64;
                    info!("  -> Fulfilled RSDP");
                }
            }
            protocol::SMBIOS_ID => {
                let smbios_3_guid = uefi::table::cfg::SMBIOS3_GUID;
                let smbios_guid = uefi::table::cfg::SMBIOS_GUID;
                let addr = get_config_table_addr(system_table, smbios_3_guid)
                    .or_else(|| get_config_table_addr(system_table, smbios_guid));
                
                if let Some(a) = addr {
                    let response = Box::leak(Box::new(protocol::SmbiosResponse {
                        revision: 0,
                        address: a,
                    }));
                    req.response = response as *mut _ as u64;
                    info!("  -> Fulfilled SMBIOS");
                }
            }
            protocol::KERNEL_FILE_ID => {
                if let Some(kf) = kernel_file {
                    let response = Box::leak(Box::new(protocol::KernelFileResponse {
                        revision: 0,
                        kernel_file: kf as u64,
                    }));
                    req.response = response as *mut _ as u64;
                    info!("  -> Fulfilled KERNEL_FILE");
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
                    info!("  -> Fulfilled MODULES ({})", file_list.len());
                }
            }
            _ => {
                info!("  -> Unknown or unhandled request");
            }
        }
    }
}

#[entry]
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();
    info!("Zamak starting up...");

    let mut boot_data = None;

    {
        let boot_services = system_table.boot_services();

        // Get the handle of the device we were loaded from
        let loaded_image = boot_services
            .open_protocol_exclusive::<LoadedImage>(image_handle)
            .expect("Failed to open LoadedImage protocol");
        let device_handle = loaded_image.device();

        // Open the filesystem on that device
        let mut fs = boot_services
            .open_protocol_exclusive::<SimpleFileSystem>(device_handle)
            .expect("Failed to open SimpleFileSystem protocol");

        let mut root = fs.open_volume().expect("Failed to open volume");

        // Try to find a configuration file
        let config_paths = [
            cstr16!("\\zamak.conf"),
            cstr16!("\\boot\\zamak.conf"),
        ];
        
        for path in config_paths {
            if let Ok(file_handle) = root.open(path, FileMode::Read, FileAttribute::empty()) {
                info!("Found configuration file at: {:?}", path);
                
                let mut file = match file_handle.into_type().expect("Failed to get file type") {
                    uefi::proto::media::file::FileType::Regular(f) => f,
                    _ => continue,
                };

                let config_data = read_file(&mut file);
                let config_str = core::str::from_utf8(&config_data).expect("Config is not UTF-8");
                let config = config::parse(config_str);
                
                if let Some(entry) = config.entries.first() {
                    info!("Booting entry: {}", entry.name);
                    let kernel_path_str = entry.options.get("KERNEL_PATH")
                        .or(entry.options.get("PATH"))
                        .expect("No kernel path specified");

                    // Convert &str to CStr16
                    let mut path_buf = [0u16; 256];
                    let mut i = 0;
                    for c in kernel_path_str.chars() {
                        path_buf[i] = c as u16;
                        i += 1;
                    }
                    let u_path = uefi::CStr16::from_u16_with_nul(&path_buf[..i+1]).expect("Failed to create CStr16");

                    if let Ok(k_handle) = root.open(u_path, FileMode::Read, FileAttribute::empty()) {
                        let mut k_file = match k_handle.into_type().expect("Failed to get file type") {
                            uefi::proto::media::file::FileType::Regular(f) => f,
                            _ => panic!("Kernel is a directory"),
                        };
                        let kernel_data = read_file(&mut k_file);
                        let elf_info = elf::parse_elf(&kernel_data).expect("Failed to parse ELF");
                        
                        let loaded_kernel = load_kernel_segments(boot_services, &elf_info, &kernel_data);
                        let relocated_ptr = loaded_kernel.phys_base as *const u8;
                        let relocated_slice = unsafe { core::slice::from_raw_parts(relocated_ptr, loaded_kernel.size) };
                        let relocated_requests = protocol::scan_requests(relocated_slice);
                        
                        let mut loaded_modules = Vec::new();
                        for mod_cfg in &entry.modules {
                            info!("Loading module: {}", mod_cfg.path);
                            
                            // Convert &str to CStr16
                            let mut mod_path_buf = [0u16; 256];
                            let mut mi = 0;
                            for c in mod_cfg.path.chars() {
                                mod_path_buf[mi] = c as u16;
                                mi += 1;
                            }
                            let u_mod_path = uefi::CStr16::from_u16_with_nul(&mod_path_buf[..mi+1]).expect("Failed to create CStr16 for module");

                            if let Ok(m_handle) = root.open(u_mod_path, FileMode::Read, FileAttribute::empty()) {
                                let mut m_file = match m_handle.into_type().expect("Failed to get file type") {
                                    uefi::proto::media::file::FileType::Regular(f) => f,
                                    _ => continue,
                                };
                                let m_data = read_file(&mut m_file);
                                let m_leaked = Box::leak(m_data.into_boxed_slice());
                                loaded_modules.push(protocol::File {
                                    revision: 0,
                                    address: m_leaked.as_ptr() as u64,
                                    size: m_leaked.len() as u64,
                                    ..Default::default()
                                });
                                m_file.close();
                            } else {
                                error!("Failed to open module: {}", mod_cfg.path);
                            }
                        }

                        let kf_data = Box::leak(kernel_data.into_boxed_slice());
                        let kf = Box::leak(Box::new(protocol::File {
                            revision: 0,
                            address: kf_data.as_ptr() as u64,
                            size: kf_data.len() as u64,
                            ..Default::default()
                        }));

                        fulfill_requests(&system_table, Some(kf), &loaded_modules, &relocated_requests);

                        let pml4_phys = setup_paging(boot_services, &loaded_kernel);
                        boot_data = Some((pml4_phys, elf_info.entry));
                        
                        // Close kernel file (m_file already closed in loop)
                        k_file.close();
                    }
                }
                file.close();
                break;
            }
        }
    }

    if let Some((pml4_phys, entry_point)) = boot_data {
        info!("Exiting boot services and jumping to kernel at {:#x}", entry_point);
        
        let (_st, _mmap_iter) = system_table.exit_boot_services();

        unsafe {
            core::arch::asm!("cli");
            x86_64::registers::control::Cr3::write(
                PhysFrame::containing_address(pml4_phys),
                x86_64::registers::control::Cr3Flags::empty()
            );

            let entry_ptr: extern "C" fn() -> ! = core::mem::transmute(entry_point);
            entry_ptr();
        }
    }

    info!("Press any key to exit...");
    loop {
        if let Ok(Some(_)) = system_table.stdin().read_key() {
            break;
        }
    }

    Status::SUCCESS
}
