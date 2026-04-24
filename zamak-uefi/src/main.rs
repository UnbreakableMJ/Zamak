// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

#![no_std]
#![no_main]

extern crate alloc;

mod handoff;
mod paging;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use log::{error, info};
use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::rng::Rng;
use uefi::table::boot::{AllocateType, MemoryType};

use zamak_core::config;
use zamak_core::elf;
use zamak_core::linux_boot;
use zamak_core::protocol;
use zamak_core::rng::KaslrRng;

/// Branch the post-ExitBootServices handoff between the Limine
/// Protocol path (Rust-written kernels that want our page tables
/// + scanned requests) and the Linux boot protocol path (BootParams
/// zero page + 64-bit entry under UEFI identity mapping).
enum KernelHandoff {
    /// Limine Protocol: `root_phys` is the PML4 to install; `entry`
    /// is the kernel's virtual entry.
    Limine { root_phys: u64, entry: u64 },
    /// Linux boot protocol: `boot_params_phys` is the physical
    /// address of the 4 KiB zero page to pass in RSI; `entry` is
    /// the 64-bit kernel entry physical address
    /// (`kernel_load_phys + 0x200`).
    Linux { boot_params_phys: u64, entry: u64 },
}

/// Translate a UEFI memory descriptor type to the E820 type code
/// the Linux kernel expects. See `arch/x86/boot/e820.c` in the
/// kernel for the canonical table.
fn uefi_mem_ty_to_e820(ty: uefi::table::boot::MemoryType) -> u32 {
    use uefi::table::boot::MemoryType as M;
    match ty {
        // Free memory that the kernel can reuse.
        M::CONVENTIONAL
        | M::LOADER_CODE
        | M::LOADER_DATA
        | M::BOOT_SERVICES_CODE
        | M::BOOT_SERVICES_DATA => 1,
        // Still-live after ExitBootServices — reserved for the firmware.
        M::RUNTIME_SERVICES_CODE | M::RUNTIME_SERVICES_DATA => 2,
        M::ACPI_RECLAIM => 3,
        M::ACPI_NON_VOLATILE => 4,
        M::UNUSABLE => 5,
        // Everything else (MMIO, PAL code, reserved, persistent mem) → reserved.
        _ => 2,
    }
}

/// Loads a Linux bzImage from `bzimage`, populates BootParams + E820
/// + cmdline (+ optional initrd), and returns a `KernelHandoff::Linux`
/// variant with stable physical addresses. All allocations go through
/// UEFI `LOADER_DATA` so they survive ExitBootServices.
fn load_linux_kernel(
    boot_services: &BootServices,
    bzimage: &[u8],
    cmdline: &str,
    initrd: Option<&[u8]>,
) -> KernelHandoff {
    use linux_boot::{
        kernel_load_address, kernel_offset, kernel_size, parse_setup_header, prepare_linux_boot,
        MemoryRegion,
    };

    let header = parse_setup_header(bzimage).expect("bzImage: invalid setup header");

    // Decide a physical load address. For relocatable kernels the
    // bootloader may pick any 2-MiB-aligned address; we always ask
    // UEFI for a fresh allocation so we don't collide with UEFI data.
    let body_size = kernel_size(&header, bzimage.len());
    let body_pages = (body_size + 0xFFF) / 0x1000;
    let kernel_load_phys = boot_services
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, body_pages)
        .expect("allocate bzImage kernel body");
    // SAFETY: UEFI-allocated pages, page-aligned, uniquely owned here;
    // we copy `body_size` bytes from the bzImage into them.
    unsafe {
        core::ptr::copy_nonoverlapping(
            bzimage[kernel_offset(&header)..].as_ptr(),
            kernel_load_phys as *mut u8,
            body_size,
        );
    }
    info!(
        "Linux kernel body: {} bytes loaded at phys {:#x} (pref {:#x}, load-addr {:#x})",
        body_size,
        kernel_load_phys,
        header.pref_address,
        kernel_load_address(&header)
    );

    // Command line: one 4-KiB page. Truncate (at NUL) if too long.
    let cmdline_phys = boot_services
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1)
        .expect("allocate cmdline page");
    let cmdline_bytes = cmdline.as_bytes();
    let cmdline_copy = core::cmp::min(cmdline_bytes.len(), 0xFFF);
    // SAFETY: freshly-allocated 4 KiB page, writable.
    unsafe {
        core::ptr::copy_nonoverlapping(
            cmdline_bytes.as_ptr(),
            cmdline_phys as *mut u8,
            cmdline_copy,
        );
        // Ensure NUL terminator.
        *(cmdline_phys as *mut u8).add(cmdline_copy) = 0;
    }

    // Optional initrd: copy into fresh UEFI-allocated pages.
    let initrd_info = initrd.map(|blob| {
        let pages = (blob.len() + 0xFFF) / 0x1000;
        let phys = boot_services
            .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
            .expect("allocate initrd");
        unsafe {
            core::ptr::copy_nonoverlapping(blob.as_ptr(), phys as *mut u8, blob.len());
        }
        info!(
            "Linux initrd: {} bytes loaded at phys {:#x}",
            blob.len(),
            phys
        );
        let size: u32 = blob.len().try_into().expect("initrd > 4 GiB not supported");
        (phys, size)
    });

    // Build an E820-style memory map from the UEFI memory map. Must be
    // done *before* ExitBootServices (we're still inside Boot Services
    // here); the resulting `Vec<MemoryRegion>` is a snapshot.
    let mmap_size = boot_services.memory_map_size();
    let mut mmap_buf = vec![0u8; mmap_size.map_size + 1024];
    let mmap = boot_services
        .memory_map(&mut mmap_buf)
        .expect("get UEFI memory map");
    let mut e820: Vec<MemoryRegion> = Vec::with_capacity(mmap.entries().count());
    for desc in mmap.entries() {
        e820.push(MemoryRegion {
            base: desc.phys_start,
            length: desc.page_count * 4096,
            typ: uefi_mem_ty_to_e820(desc.ty),
        });
    }
    info!("Linux: converted {} UEFI entries to E820", e820.len());

    let image = prepare_linux_boot(bzimage, kernel_load_phys, cmdline_phys, initrd_info, &e820)
        .expect("prepare_linux_boot failed");

    // The zero page needs to survive ExitBootServices at a stable
    // physical address; allocate a dedicated UEFI page and copy.
    let bp_phys = boot_services
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1)
        .expect("allocate BootParams zero page");
    // SAFETY: 4-KiB UEFI-allocated page, exactly matches BootParams size.
    unsafe {
        core::ptr::copy_nonoverlapping(image.boot_params.as_ptr(), bp_phys as *mut u8, 4096);
    }
    info!(
        "Linux BootParams at phys {:#x}, entry {:#x}",
        bp_phys, image.entry_point
    );

    KernelHandoff::Linux {
        boot_params_phys: bp_phys,
        entry: image.entry_point,
    }
}

/// UEFI input adapter used by the ZAMAK TUI. Arch-neutral.
struct UefiInput<'a> {
    stdin: &'a mut uefi::proto::console::text::Input,
    boot_services: &'a uefi::table::boot::BootServices,
}

impl<'a> zamak_core::tui::InputSource for UefiInput<'a> {
    fn read_key(&mut self) -> zamak_core::tui::Key {
        use uefi::proto::console::text::{Key, ScanCode};

        let event = unsafe { self.stdin.wait_for_key_event().unsafe_clone() };
        if self.boot_services.check_event(event).is_err() {
            return zamak_core::tui::Key::None;
        }

        if let Ok(Some(key)) = self.stdin.read_key() {
            match key {
                Key::Printable(c) => {
                    let char_code = char::from(c);
                    match char_code {
                        'k' => zamak_core::tui::Key::Char('k'),
                        'j' => zamak_core::tui::Key::Char('j'),
                        'i' => zamak_core::tui::Key::Char('i'),
                        '\r' | '\n' => zamak_core::tui::Key::Enter,
                        _ => zamak_core::tui::Key::Char(char_code),
                    }
                }
                Key::Special(ScanCode::UP) => zamak_core::tui::Key::Up,
                Key::Special(ScanCode::DOWN) => zamak_core::tui::Key::Down,
                Key::Special(ScanCode::ESCAPE) => zamak_core::tui::Key::Esc,
                _ => zamak_core::tui::Key::None,
            }
        } else {
            zamak_core::tui::Key::None
        }
    }
}

/// UEFI-backed randomness for KASLR. Arch-neutral.
struct UefiRng<'a> {
    boot_services: &'a BootServices,
}

impl<'a> zamak_core::rng::KaslrRng for UefiRng<'a> {
    fn get_u64(&mut self) -> u64 {
        if let Ok(handle) = self.boot_services.get_handle_for_protocol::<Rng>() {
            if let Ok(mut rng) = self.boot_services.open_protocol_exclusive::<Rng>(handle) {
                let mut buf = [0u8; 8];
                if rng.get_rng(None, &mut buf).is_ok() {
                    return u64::from_le_bytes(buf);
                }
            }
        }
        0
    }
}

/// Arch-neutral description of the in-memory kernel image. Consumed
/// by `paging::build` to produce the per-arch root page-table root.
pub struct LoadedKernel {
    pub phys_base: u64,
    pub vaddr_start: u64,
    pub size: usize,
}

fn load_kernel_segments(
    boot_services: &BootServices,
    elf_info: &elf::ElfInfo,
    kernel_data: &[u8],
) -> LoadedKernel {
    let mut min_vaddr = u64::MAX;
    let mut max_vaddr: u64 = 0;

    for segment in &elf_info.segments {
        if segment.vaddr < min_vaddr {
            min_vaddr = segment.vaddr;
        }
        let end = segment
            .vaddr
            .checked_add(segment.mem_size as u64)
            .expect("kernel segment vaddr+size overflowed u64");
        if end > max_vaddr {
            max_vaddr = end;
        }
    }

    let vaddr_start = min_vaddr & !0xfff;
    let vaddr_end = max_vaddr
        .checked_add(0xfff)
        .expect("kernel vaddr end overflowed when rounding up")
        & !0xfff;
    let size = vaddr_end
        .checked_sub(vaddr_start)
        .expect("kernel vaddr_end < vaddr_start") as usize;
    let pages = size / 4096;

    info!(
        "Kernel virtual range: {:#x} - {:#x} ({} bytes, {} pages)",
        vaddr_start, vaddr_end, size, pages
    );

    let phys_base = boot_services
        .allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages)
        .expect("Failed to allocate physical memory for kernel");

    // Zero the load area (BSS + padding).
    unsafe {
        core::ptr::write_bytes(phys_base as *mut u8, 0, size);
    }

    for segment in &elf_info.segments {
        let offset = segment
            .vaddr
            .checked_sub(vaddr_start)
            .expect("segment vaddr below kernel base") as usize;
        let dest_phys = phys_base
            .checked_add(offset as u64)
            .expect("phys_base + offset overflowed");
        let dest = dest_phys as *mut u8;
        let src = &kernel_data[segment.offset..segment.offset + segment.file_size];
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dest, segment.file_size);
        }
        info!(
            "  Loaded segment: vaddr {:#x} -> phys {:#x} ({} bytes)",
            segment.vaddr, dest_phys, segment.file_size
        );
    }

    LoadedKernel {
        phys_base,
        vaddr_start,
        size,
    }
}

fn read_file(file: &mut uefi::proto::media::file::RegularFile) -> Vec<u8> {
    let mut i_buffer = [0u8; 256];
    let info: &FileInfo = file
        .get_info(&mut i_buffer)
        .expect("Failed to get file info");
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
    smp: Option<protocol::SmpResponse>,
    requests: &[*mut protocol::RawRequest],
) {
    for &req_ptr in requests {
        let req = unsafe { &mut *req_ptr };
        info!("Processing request: ID {:x} {:x}", req.id[0], req.id[1]);

        match req.id {
            protocol::BOOTLOADER_INFO_ID => {
                let response = Box::leak(Box::new(protocol::BootloaderInfoResponse {
                    name: Box::leak(Box::new("Zamak\0")).as_ptr() as u64,
                    version: Box::leak(Box::new(concat!(env!("CARGO_PKG_VERSION"), "\0"))).as_ptr()
                        as u64,
                }));
                req.response = response as *mut _ as u64;
                info!("  -> Fulfilled BOOTLOADER_INFO");
            }
            protocol::HHDM_ID => {
                let response = Box::leak(Box::new(protocol::HhdmResponse {
                    revision: 0,
                    offset: paging::HHDM_OFFSET,
                }));
                req.response = response as *mut _ as u64;
                info!("  -> Fulfilled HHDM");
            }
            protocol::MEMMAP_ID => {
                let boot_services = system_table.boot_services();
                let mmap_size = boot_services.memory_map_size();
                let mut mmap_buffer = vec![0u8; mmap_size.map_size + 1024];
                let mmap = boot_services
                    .memory_map(&mut mmap_buffer)
                    .expect("Failed to get memory map");
                let mmap_iter = mmap.entries();

                let mut entries = Vec::new();
                for desc in mmap_iter {
                    let typ = match desc.ty {
                        uefi::table::boot::MemoryType::CONVENTIONAL => protocol::MEMMAP_USABLE,
                        uefi::table::boot::MemoryType::LOADER_CODE
                        | uefi::table::boot::MemoryType::LOADER_DATA => {
                            protocol::MEMMAP_BOOTLOADER_RECLAIMABLE
                        }
                        uefi::table::boot::MemoryType::ACPI_RECLAIM => {
                            protocol::MEMMAP_ACPI_RECLAIMABLE
                        }
                        uefi::table::boot::MemoryType::ACPI_NON_VOLATILE => {
                            protocol::MEMMAP_ACPI_NVS
                        }
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
                    let mut gop = boot_services
                        .open_protocol_exclusive::<GraphicsOutput>(gop_handle)
                        .expect("Failed to open GOP");
                    let mode_info = gop.current_mode_info();
                    let (width, height) = mode_info.resolution();
                    let mut fb_ptr = gop.frame_buffer();

                    let fb = Box::leak(Box::new(protocol::Framebuffer {
                        address: fb_ptr.as_mut_ptr() as u64,
                        width: width as u64,
                        height: height as u64,
                        pitch: (mode_info.stride() * 4) as u64,
                        bpp: 32,
                        memory_model: 1,
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
            protocol::SMP_ID => {
                if let Some(s) = smp {
                    let response = Box::leak(Box::new(s));
                    req.response = response as *mut _ as u64;
                    info!("  -> Fulfilled SMP ({} CPUs)", s.cpu_count);
                }
            }
            _ => {
                info!("  -> Unknown or unhandled request");
            }
        }
    }
}

/// Enrolled config hash slot embedded in the binary (FR-CFG-006).
///
/// `zamak enroll-config` locates this slot by scanning for its 16-byte
/// signature and overwrites the 32 bytes that follow with a BLAKE2B-256
/// hash of the user's config file. At boot, [`zamak_core::enrolled_hash`]
/// reads this slot; a non-zero hash locks the config editor.
#[used]
#[no_mangle]
pub static ZAMAK_ENROLLED_HASH: zamak_core::enrolled_hash::EnrolledHashSlot =
    zamak_core::enrolled_hash::EnrolledHashSlot::empty();

/// M6-3 instrumentation: emit one `ZAMAK_PHASE=<name> tsc=<u64>` line
/// per checkpoint. Consumed by `zamak-cli bench parse-serial` off a
/// captured UEFI serial log to compute phase deltas (cycles / ns).
///
/// `info!` already routes through `uefi_services`' logger to the UEFI
/// console + COM1 (OVMF / real hardware with serial-redirect), so no
/// extra plumbing is needed — the mark is observable alongside normal
/// `[ INFO ]` lines.
#[cfg(target_arch = "x86_64")]
fn mark_phase(phase: &str) {
    let tsc = zamak_core::arch::x86::rdtsc();
    info!("ZAMAK_PHASE={phase} tsc={tsc}");
}

#[cfg(not(target_arch = "x86_64"))]
fn mark_phase(_phase: &str) {
    // Non-x86 UEFI targets don't have `rdtsc`; skip silently. AArch64
    // etc. can land an equivalent generic-timer read in a follow-up.
}

/// Emit the TSC frequency (in MHz) at boot start, when discoverable.
///
/// Uses CPUID leaf 0x16 (Processor Frequency Information), available
/// on Skylake-era and newer Intel / AMD parts. Emits
/// `ZAMAK_TSC_MHZ=<n>` when EAX reports a non-zero base frequency, or
/// `ZAMAK_TSC_MHZ=unknown` otherwise (caller of `bench parse-serial`
/// can then pass `--tsc-mhz <n>` manually based on the host SKU).
#[cfg(target_arch = "x86_64")]
fn emit_tsc_freq() {
    // CPUID.16H — EAX=base MHz, EBX=max MHz, ECX=bus MHz. Leaf is
    // reported in the maximum-basic-leaf (EAX=0) as at least 0x16 on
    // parts that support it. Check the maximum first.
    let (max_leaf, _, _, _) = cpuid(0);
    if max_leaf < 0x16 {
        info!("ZAMAK_TSC_MHZ=unknown");
        return;
    }
    let (base_mhz, _max_mhz, _bus_mhz, _) = cpuid(0x16);
    if base_mhz == 0 {
        info!("ZAMAK_TSC_MHZ=unknown");
    } else {
        info!("ZAMAK_TSC_MHZ={base_mhz}");
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn emit_tsc_freq() {}

#[cfg(target_arch = "x86_64")]
fn cpuid(leaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY:
    //   Preconditions: `leaf` is a valid CPUID leaf number.
    //   Postconditions: returns the four CPUID output registers.
    //   Clobbers: EAX, EBX, ECX, EDX.
    //   Worst-case: junk values for unsupported leaves; the caller
    //     gates on `max_leaf` before trusting non-basic leaves.
    // rbx is reserved by LLVM in x86-64 PIC; save/restore manually.
    // ecx: sub-leaf (0 for leaves we use), clobbered on return.
    // SAFETY: See function-level block.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx:e}, ebx",
            "pop rbx",
            inout("eax") leaf => eax,
            ebx = out(reg) ebx,
            inout("ecx") 0u32 => ecx,
            out("edx") edx,
            options(nostack, preserves_flags),
        );
    }
    (eax, ebx, ecx, edx)
}

#[entry]
fn main(image_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi_services::init(&mut system_table).unwrap();
    emit_tsc_freq();
    mark_phase("uefi_entry");
    info!("Zamak starting up ({})...", env!("CARGO_PKG_VERSION"));

    let mut boot_data: Option<KernelHandoff> = None;

    {
        let boot_services = system_table.boot_services();

        let loaded_image = boot_services
            .open_protocol_exclusive::<LoadedImage>(image_handle)
            .expect("Failed to open LoadedImage protocol");
        let device_handle = loaded_image.device();

        let mut fs = boot_services
            .open_protocol_exclusive::<SimpleFileSystem>(device_handle)
            .expect("Failed to open SimpleFileSystem protocol");

        let mut root = fs.open_volume().expect("Failed to open volume");

        let config_paths = [cstr16!("\\zamak.conf"), cstr16!("\\boot\\zamak.conf")];

        for path in config_paths {
            if let Ok(file_handle) = root.open(path, FileMode::Read, FileAttribute::empty()) {
                info!("Found configuration file at: {:?}", path);

                let mut file = match file_handle.into_type().expect("file type") {
                    uefi::proto::media::file::FileType::Regular(f) => f,
                    _ => continue,
                };

                let config_data = read_file(&mut file);
                let config_str = core::str::from_utf8(&config_data).expect("Config is not UTF-8");
                let config = config::parse(config_str);
                mark_phase("config_parsed");

                // Detect network-boot context.
                use uefi::proto::network::snp::SimpleNetwork;
                use uefi::table::boot::SearchType;
                use uefi::Identify;
                let is_network_boot = boot_services
                    .locate_handle_buffer(SearchType::ByProtocol(&SimpleNetwork::GUID))
                    .map(|h| !h.is_empty())
                    .unwrap_or(false);
                if is_network_boot {
                    log::info!("Boot Source: Network (Protocol Present)");
                } else {
                    log::info!("Boot Source: Disk / Local Media");
                }

                // Framebuffer for TUI.
                let gop_handle = boot_services
                    .get_handle_for_protocol::<GraphicsOutput>()
                    .expect("Graphics Output Protocol support is required");
                let mut gop = boot_services
                    .open_protocol_exclusive::<GraphicsOutput>(gop_handle)
                    .expect("Failed to open GOP");

                let mut selected_idx = 0;

                {
                    use zamak_core::font::{PsfFont, DEFAULT_FONT};
                    use zamak_core::gfx::Canvas;
                    use zamak_core::tui::{draw_menu, InputSource, MenuState};
                    use zamak_theme::{Theme, ThemeVariant};

                    let mode_info = gop.current_mode_info();
                    let mut fb_struct = protocol::Framebuffer {
                        address: gop.frame_buffer().as_mut_ptr() as u64,
                        width: mode_info.resolution().0 as u64,
                        height: mode_info.resolution().1 as u64,
                        pitch: mode_info.stride() as u64 * 4,
                        bpp: 32,
                        red_mask_size: 8,
                        red_mask_shift: 16,
                        green_mask_size: 8,
                        green_mask_shift: 8,
                        blue_mask_size: 8,
                        blue_mask_shift: 0,
                        ..Default::default()
                    };
                    match mode_info.pixel_format() {
                        uefi::proto::console::gop::PixelFormat::Rgb => {
                            fb_struct.red_mask_shift = 0;
                            fb_struct.blue_mask_shift = 16;
                        }
                        uefi::proto::console::gop::PixelFormat::Bgr => {
                            fb_struct.red_mask_shift = 16;
                            fb_struct.blue_mask_shift = 0;
                        }
                        _ => {}
                    }

                    let font = PsfFont::parse(DEFAULT_FONT).unwrap();
                    let mut canvas = Canvas::new(&mut fb_struct);

                    let st_ptr =
                        (&system_table as *const SystemTable<Boot>) as *mut SystemTable<Boot>;
                    let stdin = unsafe { (*st_ptr).stdin() };
                    let mut input = UefiInput {
                        stdin,
                        boot_services,
                    };

                    let theme_variant = ThemeVariant::parse(&config.theme_variant);
                    let theme = Theme::default().with_variant(theme_variant);

                    let mut state = if config.config_hash.is_some() {
                        MenuState::new_locked(config.timeout)
                    } else {
                        MenuState::new(config.timeout)
                    };
                    let mut time_remaining = config.timeout * 10;

                    loop {
                        draw_menu(&mut canvas, &font, &config, &state, &theme, time_remaining);
                        let key = input.read_key();
                        if let zamak_core::tui::Key::None = key {
                            boot_services.stall(100_000);
                            if time_remaining > 0 {
                                time_remaining -= 1;
                            }
                            if time_remaining == 0 {
                                break;
                            }
                        } else {
                            time_remaining = 0;
                        }

                        match key {
                            zamak_core::tui::Key::Up => {
                                if state.selected_idx > 0 {
                                    state.selected_idx -= 1;
                                }
                            }
                            zamak_core::tui::Key::Down => {
                                if state.selected_idx < config.entries.len() - 1 {
                                    state.selected_idx += 1;
                                }
                            }
                            zamak_core::tui::Key::Char('k') => {
                                if state.selected_idx > 0 {
                                    state.selected_idx -= 1;
                                }
                            }
                            zamak_core::tui::Key::Char('j') => {
                                if state.selected_idx < config.entries.len() - 1 {
                                    state.selected_idx += 1;
                                }
                            }
                            zamak_core::tui::Key::Edit | zamak_core::tui::Key::Char('i') => {
                                state.editing = !state.editing;
                                if state.editing {
                                    state.edit_buffer = alloc::string::String::from(
                                        &config.entries[state.selected_idx].cmdline,
                                    );
                                }
                            }
                            zamak_core::tui::Key::Char(c) if state.editing => {
                                state.edit_buffer.push(c);
                            }
                            zamak_core::tui::Key::Esc => {
                                if state.editing {
                                    state.editing = false;
                                }
                            }
                            zamak_core::tui::Key::Enter => {
                                if state.editing {
                                    state.editing = false;
                                } else {
                                    selected_idx = state.selected_idx;
                                    break;
                                }
                            }
                            _ => {}
                        }
                    }
                    mark_phase("menu_finished");
                }

                if let Some(entry) = config.entries.get(selected_idx) {
                    info!(
                        "Booting entry: {} (PROTOCOL={})",
                        entry.name, entry.protocol
                    );
                    // The config parser stores KERNEL_PATH/PATH directly on
                    // `entry.kernel_path`; only unrecognised keys spill into
                    // `entry.options`. Fall back to the options map so any
                    // pre-parser configs (or future renames) still boot.
                    let kernel_path_str: &str = if !entry.kernel_path.is_empty() {
                        entry.kernel_path.as_str()
                    } else {
                        entry
                            .options
                            .get("KERNEL_PATH")
                            .or(entry.options.get("PATH"))
                            .map(|s| s.as_str())
                            .expect("No kernel path specified")
                    };

                    // UEFI SimpleFileSystem paths use backslash separators.
                    // Limine-style configs use forward slashes, so translate
                    // in place while copying into the UTF-16 buffer.
                    let mut path_buf = [0u16; 256];
                    let mut i = 0;
                    for c in kernel_path_str.chars() {
                        path_buf[i] = if c == '/' { '\\' as u16 } else { c as u16 };
                        i += 1;
                    }
                    let u_path = uefi::CStr16::from_u16_with_nul(&path_buf[..i + 1])
                        .expect("Failed to create CStr16");

                    if let Ok(k_handle) = root.open(u_path, FileMode::Read, FileAttribute::empty())
                    {
                        let mut k_file = match k_handle.into_type().expect("file type") {
                            uefi::proto::media::file::FileType::Regular(f) => f,
                            _ => panic!("Kernel is a directory"),
                        };
                        let kernel_data = read_file(&mut k_file);
                        mark_phase("kernel_loaded");

                        // Dispatch on the entry's protocol. Linux uses its
                        // own boot flow (BootParams + RSI + 64-bit jump);
                        // everything else goes through the ELF / Limine
                        // path.
                        if entry.protocol.eq_ignore_ascii_case("linux") {
                            // Optional initrd: first module, if any.
                            let initrd_blob: Option<Vec<u8>> =
                                if let Some(mod_cfg) = entry.modules.first() {
                                    info!("Loading initrd: {}", mod_cfg.path);
                                    let mut mod_path_buf = [0u16; 256];
                                    let mut mi = 0;
                                    for c in mod_cfg.path.chars() {
                                        mod_path_buf[mi] =
                                            if c == '/' { '\\' as u16 } else { c as u16 };
                                        mi += 1;
                                    }
                                    let u_mod_path =
                                        uefi::CStr16::from_u16_with_nul(&mod_path_buf[..mi + 1])
                                            .expect("CStr16 for initrd");
                                    let mh = root
                                        .open(u_mod_path, FileMode::Read, FileAttribute::empty())
                                        .expect("open initrd");
                                    let mut mf = match mh.into_type().expect("initrd file type") {
                                        uefi::proto::media::file::FileType::Regular(f) => f,
                                        _ => panic!("initrd is a directory"),
                                    };
                                    Some(read_file(&mut mf))
                                } else {
                                    None
                                };

                            let handoff = load_linux_kernel(
                                boot_services,
                                &kernel_data,
                                &entry.cmdline,
                                initrd_blob.as_deref(),
                            );
                            mark_phase("requests_fulfilled");
                            boot_data = Some(handoff);
                            k_file.close();
                            file.close();
                            break;
                        }

                        let mut elf_info =
                            elf::parse_elf(&kernel_data).expect("Failed to parse ELF");

                        let mut loaded_kernel =
                            load_kernel_segments(boot_services, &elf_info, &kernel_data);

                        if elf_info.is_pie {
                            let mut rng = UefiRng { boot_services };
                            let random_val = rng.get_u64();
                            let base = 0xffffffff80000000;
                            let offset = (random_val % 512) * 0x200000;
                            loaded_kernel.vaddr_start = base + offset;
                            unsafe {
                                zamak_core::elf::apply_relocations(
                                    loaded_kernel.phys_base as *mut u8,
                                    loaded_kernel.vaddr_start,
                                    &elf_info.relocations,
                                );
                            }
                            elf_info.entry = loaded_kernel.vaddr_start + elf_info.entry;
                        }

                        let relocated_ptr = loaded_kernel.phys_base as *const u8;
                        let relocated_slice = unsafe {
                            core::slice::from_raw_parts(relocated_ptr, loaded_kernel.size)
                        };
                        let relocated_requests = protocol::scan_requests(relocated_slice);

                        let mut loaded_modules = Vec::new();
                        for mod_cfg in &entry.modules {
                            info!("Loading module: {}", mod_cfg.path);
                            let mut mod_path_buf = [0u16; 256];
                            let mut mi = 0;
                            for c in mod_cfg.path.chars() {
                                mod_path_buf[mi] = if c == '/' { '\\' as u16 } else { c as u16 };
                                mi += 1;
                            }
                            let u_mod_path =
                                uefi::CStr16::from_u16_with_nul(&mod_path_buf[..mi + 1])
                                    .expect("CStr16 for module");
                            if let Ok(m_handle) =
                                root.open(u_mod_path, FileMode::Read, FileAttribute::empty())
                            {
                                let mut m_file = match m_handle.into_type().expect("file type") {
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

                        // SMP discovery — only MpServices is x86-centric
                        // enough to warrant gating. The rest of SMP info
                        // is arch-neutral protocol fields.
                        #[allow(unused_mut)]
                        let mut smp_response = None;
                        #[cfg(target_arch = "x86_64")]
                        if let Ok(mp_handle) = boot_services
                            .get_handle_for_protocol::<uefi::proto::pi::mp::MpServices>()
                        {
                            if let Ok(mp) = boot_services
                                .open_protocol_exclusive::<uefi::proto::pi::mp::MpServices>(
                                    mp_handle,
                                )
                            {
                                let count = mp
                                    .get_number_of_processors()
                                    .expect("Failed to get CPU count");
                                let total_cpus = count.total;
                                info!(
                                    "UEFI SMP: {} total CPUs, {} enabled",
                                    total_cpus, count.enabled
                                );

                                let mut smp_infos = Vec::new();
                                let mut bsp_lapic_id = 0;
                                for i in 0..total_cpus {
                                    let info =
                                        mp.get_processor_info(i).expect("Failed to get CPU info");
                                    if info.is_bsp() {
                                        bsp_lapic_id = info.location.package as u32;
                                    }
                                    smp_infos.push(protocol::SmpInfo {
                                        processor_id: i as u32,
                                        lapic_id: info.location.package as u32,
                                        ..Default::default()
                                    });
                                }

                                let mut smp_info_ptrs = Vec::new();
                                for info in smp_infos {
                                    smp_info_ptrs.push(
                                        Box::leak(Box::new(info)) as *const protocol::SmpInfo,
                                    );
                                }
                                let smp_ptr = Box::leak(smp_info_ptrs.into_boxed_slice());
                                smp_response = Some(protocol::SmpResponse {
                                    revision: 0,
                                    flags: 0,
                                    bsp_lapic_id,
                                    cpu_count: smp_ptr.len() as u64,
                                    cpus: smp_ptr.as_ptr() as u64,
                                });
                            }
                        }

                        fulfill_requests(
                            &system_table,
                            Some(kf),
                            &loaded_modules,
                            smp_response,
                            &relocated_requests,
                        );
                        mark_phase("requests_fulfilled");

                        // Build per-arch page tables. Dispatches to
                        // x86_64 / aarch64 / riscv64 / loongarch64 in
                        // the `paging` module.
                        let root_phys = paging::build(boot_services, &loaded_kernel);
                        boot_data = Some(KernelHandoff::Limine {
                            root_phys,
                            entry: elf_info.entry,
                        });

                        k_file.close();
                    }
                }
                file.close();
                break;
            }
        }
    }

    if let Some(handoff_kind) = boot_data {
        match handoff_kind {
            KernelHandoff::Limine { root_phys, entry } => {
                info!(
                    "Exiting boot services and jumping to Limine kernel at {:#x}",
                    entry
                );
                mark_phase("pre_exit_boot_services");
                let (_st, _mmap_iter) = system_table.exit_boot_services();
                // SAFETY: ExitBootServices has succeeded; the caller built the
                // page tables rooted at `root_phys` earlier and placed the
                // kernel at `entry`. This call never returns.
                unsafe {
                    handoff::jump_to_kernel(root_phys, entry);
                }
            }
            KernelHandoff::Linux {
                boot_params_phys,
                entry,
            } => {
                info!(
                    "Exiting boot services and jumping to Linux kernel at {:#x} (RSI={:#x})",
                    entry, boot_params_phys
                );
                mark_phase("pre_exit_boot_services");
                let (_st, _mmap_iter) = system_table.exit_boot_services();
                // SAFETY: ExitBootServices has succeeded; the caller built the
                // BootParams zero page at `boot_params_phys` and populated E820
                // from the UEFI memory map. This call never returns; Linux sets
                // up its own page tables under UEFI's identity mapping (which
                // `paging::x86::build` extended to cover low physical memory in
                // v0.8.0).
                unsafe {
                    handoff::jump_to_linux_kernel(boot_params_phys, entry);
                }
            }
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
