// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

#![no_std]
#![no_main]

extern crate alloc;

pub mod allocator;
pub mod boot_bundle;
pub mod entry;
pub mod fat32;
pub mod input;
pub mod mbr;
pub mod paging;
pub mod ram_disk;
pub mod rm_io;
// The trampoline-dependent BIOS-interrupt callers are only present
// when the `legacy_trampoline` feature is enabled. M1-16 Path B moves
// all INT 13h / 15h / 10h calls into real-mode asm before CR0.PE, so
// the default build no longer needs these modules.
#[cfg(feature = "legacy_trampoline")]
pub mod disk;
#[cfg(feature = "legacy_trampoline")]
pub mod mmap;
#[cfg(feature = "legacy_trampoline")]
pub mod vbe;
// SMP bring-up and its AP trampoline are temporarily disabled in the
// BIOS boot path. The trampoline's real-mode asm references in-section
// labels via 16-bit absolute relocations, which rust-lld cannot resolve
// once the `.trampoline` section lands beyond 64 KiB in the final ELF.
// Fixing this needs a position-independent rewrite (or a linker-script
// VMA override) and is out of M1-16 scope. The boot-smoke test kernel
// does not consume a Limine SMP response.
#[cfg(feature = "smp")]
pub mod smp;
#[cfg(feature = "smp")]
pub mod trampoline;
pub mod utils;

use core::panic::PanicInfo;
#[cfg(feature = "legacy_trampoline")]
use disk::Disk;
#[cfg(feature = "legacy_trampoline")]
#[allow(unused_imports)]
use fat32::Fat32;
#[cfg(feature = "legacy_trampoline")]
use mmap::get_memory_map;

#[cfg(feature = "legacy_trampoline")]
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

#[cfg(feature = "legacy_trampoline")]
extern "C" {
    fn call_bios_int(int_no: u8, regs: *mut BiosRegs);
}

extern "C" {
    // Used by the legacy kmain body and, from Phase 6 onwards, by the
    // Path B kmain after it consumes the BootDataBundle. Kept
    // unconditional so the symbol is always present in entry.rs-land.
    #[allow(dead_code)]
    fn enter_long_mode(pml4_phys: u32, entry_point: u64);
}

// §3.9.7: Compile-time layout verification for structs accessed by assembly.
#[cfg(feature = "legacy_trampoline")]
const _: () = {
    assert!(
        core::mem::size_of::<BiosRegs>() == 24,
        "BiosRegs must be 24 bytes"
    );
    assert!(
        core::mem::offset_of!(BiosRegs, eax) == 0,
        "BiosRegs.eax at offset 0"
    );
    assert!(
        core::mem::offset_of!(BiosRegs, ebx) == 4,
        "BiosRegs.ebx at offset 4"
    );
    assert!(
        core::mem::offset_of!(BiosRegs, ecx) == 8,
        "BiosRegs.ecx at offset 8"
    );
    assert!(
        core::mem::offset_of!(BiosRegs, edx) == 12,
        "BiosRegs.edx at offset 12"
    );
    assert!(
        core::mem::offset_of!(BiosRegs, esi) == 16,
        "BiosRegs.esi at offset 16"
    );
    assert!(
        core::mem::offset_of!(BiosRegs, edi) == 20,
        "BiosRegs.edi at offset 20"
    );
};

use alloc::boxed::Box;
#[cfg(feature = "legacy_trampoline")]
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use zamak_core::protocol;

#[allow(dead_code)]
fn fulfill_requests(
    mmap: &[protocol::MemmapEntry],
    fb: Option<protocol::Framebuffer>,
    kernel_file: Option<*const protocol::File>,
    modules: &[protocol::File],
    rsdp: Option<u64>,
    smp: Option<protocol::SmpResponse>,
    requests: &[*mut protocol::RawRequest],
) {
    for &req_ptr in requests {
        // SAFETY: req_ptr was found by scan_requests in kernel memory; it points
        //         to a valid RawRequest with the Limine magic header intact.
        let req = unsafe { &mut *req_ptr };

        match req.id {
            protocol::BOOTLOADER_INFO_ID => {
                let response = Box::leak(Box::new(protocol::BootloaderInfoResponse {
                    name: Box::leak(Box::new("Zamak-Bios\0")).as_ptr() as u64,
                    version: Box::leak(Box::new(concat!(env!("CARGO_PKG_VERSION"), "\0"))).as_ptr()
                        as u64,
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
                    let fb_list: &mut [*const protocol::Framebuffer] =
                        Box::leak(vec![fb_ptr as *const _].into_boxed_slice());
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

#[cfg(feature = "legacy_trampoline")]
use zamak_core::arch::x86 as arch;
#[cfg(feature = "legacy_trampoline")]
use zamak_core::rng::{KaslrRng, X86KaslrRng};

/// Writes a single ASCII byte to COM1 (0x3F8). Used as a boot-progress
/// checkpoint marker — the test harness doesn't consume these, they're
/// for humans reading QEMU serial logs during M1-16 bring-up.
#[inline(always)]
fn mark(b: u8) {
    // SAFETY:
    //   Preconditions: COM1 (0x3F8) exists in every QEMU PC machine.
    //   Postconditions: byte appears on -serial stdio output.
    //   Clobbers: DX, AL (temps).
    //   Worst-case: spurious byte appears on serial.
    unsafe {
        core::arch::asm!(
            "mov dx, 0x3F8",
            "out dx, al",
            in("al") b,
            out("dx") _,
            options(nostack, nomem, preserves_flags),
        );
    }
}

/// Path B kmain: consumes the `BootDataBundle` populated by the
/// real-mode orchestration in `_start`. This Phase 6 framework
/// validates the magic and converts the E820 map into the Limine
/// shape; the FAT32-parse + ELF load + Limine fulfillment + long-mode
/// entry land in a follow-up commit.
#[cfg(not(feature = "legacy_trampoline"))]
#[no_mangle]
pub extern "C" fn kmain(bundle_phys: u32) -> ! {
    use crate::boot_bundle::{BootDataBundle, ZBDL_MAGIC};
    use zamak_core::protocol::{
        MemmapEntry, MEMMAP_ACPI_NVS, MEMMAP_ACPI_RECLAIMABLE, MEMMAP_BAD_MEMORY,
        MEMMAP_RESERVED, MEMMAP_USABLE,
    };

    // SAFETY: hard-disable interrupts. We have no IDT, so any IRQ
    // (e.g. the PIT timer at vector 0x08) would triple-fault.
    unsafe {
        core::arch::asm!("cli", options(nomem, nostack, preserves_flags));
    }

    mark(b'K');

    // SAFETY: real-mode orchestration writes the bundle at this fixed
    // address before the PE transition and stamps the magic last.
    let bundle: &'static BootDataBundle =
        unsafe { &*(bundle_phys as *const BootDataBundle) };
    let magic = bundle.magic;
    assert!(
        magic == ZBDL_MAGIC,
        "kmain: BootDataBundle magic mismatch"
    );
    mark(b'B');

    // ---- E820 → Limine memmap entries ----
    let e820_count = bundle.e820_count as usize;
    let mut mmap_entries: Vec<MemmapEntry> = Vec::with_capacity(e820_count);
    for i in 0..e820_count {
        let entry = bundle.e820[i];
        let base = entry.base;
        let len = entry.len;
        let typ = entry.typ;
        let limine_typ = match typ {
            1 => MEMMAP_USABLE,
            2 => MEMMAP_RESERVED,
            3 => MEMMAP_ACPI_RECLAIMABLE,
            4 => MEMMAP_ACPI_NVS,
            5 => MEMMAP_BAD_MEMORY,
            _ => MEMMAP_RESERVED,
        };
        mmap_entries.push(MemmapEntry { base, length: len, typ: limine_typ });
    }
    mark(b'E');
    let _ = mmap_entries;
    mark(b'.');
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack, preserves_flags));
        }
    }
}

#[cfg(feature = "legacy_trampoline")]
#[no_mangle]
pub extern "C" fn kmain(drive_id: u8) -> ! {
    mark(b'M'); // kmain entry

    // 2. Initialize Disk
    let mut disk = Disk::new(drive_id);
    let mut disk_ext2 = disk.clone();
    mark(b'D'); // Disk::new returned

    // 3. Read the MBR partition table to find a FAT32 / EXT2 partition.
    //    An unpartitioned disk would have the filesystem at LBA 0, but
    //    `zamak-test/build-images.sh` stamps a proper MBR partition
    //    table via `sfdisk`, so we follow that pointer.
    use zamak_core::fs::BlockDevice;
    let mut mbr_buf = [0u8; 512];
    BlockDevice::read_sectors(&disk, 0, 1, &mut mbr_buf).expect("Failed to read MBR sector");
    mark(b'm'); // MBR read
    let mut part_lba: u64 = 0;
    for i in 0..4 {
        let base = 446 + i * 16;
        let type_byte = mbr_buf[base + 4];
        // 0x01=FAT12, 0x04=FAT16<32MB, 0x06=FAT16, 0x0B=FAT32/CHS,
        // 0x0C=FAT32/LBA, 0x83=Linux (EXT*).
        if matches!(type_byte, 0x01 | 0x04 | 0x06 | 0x0B | 0x0C | 0x83) {
            let lba = u32::from_le_bytes([
                mbr_buf[base + 8],
                mbr_buf[base + 9],
                mbr_buf[base + 10],
                mbr_buf[base + 11],
            ]);
            if lba != 0 {
                part_lba = lba as u64;
                break;
            }
        }
    }
    if part_lba == 0 {
        panic!("No FAT32/EXT2 partition entry in MBR");
    }
    mark(b'P'); // Partition LBA resolved

    // 4. Mount Filesystem
    // We try FAT32 first, then EXT2
    use crate::fat32::Fat32;
    use zamak_core::ext2::Ext2;
    use zamak_core::fs::FileSystem;

    let mut fs_fat: Option<Fat32> = None;
    let mut fs_ext2: Option<Ext2> = None;

    mark(b'F'); // About to probe filesystems
                // Probe FAT32
    if let Ok(f) = Fat32::parse(&mut disk, part_lba) {
        fs_fat = Some(f);
    }
    // If not FAT32, probe EXT2
    else if let Ok(f) = Ext2::mount(&mut disk_ext2, part_lba) {
        fs_ext2 = Some(f);
    } else {
        panic!("No supported filesystem found on boot partition");
    }
    mark(b'f'); // Filesystem mounted

    let fs: &dyn FileSystem = if let Some(ref f) = fs_fat {
        f
    } else {
        fs_ext2.as_ref().unwrap()
    };

    // Read Config
    let mut config_file_buf = vec![0u8; 4096];
    let config_entry = fs.find_file("zamak.conf").expect("Missing zamak.conf");
    fs.read_file(&config_entry, &mut config_file_buf)
        .expect("Failed to read config");

    let config_size = config_entry.size as usize;
    // Simple parser
    let config_str = core::str::from_utf8(&config_file_buf[..config_size]).unwrap_or("");
    let config = zamak_core::config::parse(config_str);
    mark(b'C'); // Config parsed

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
        use crate::input::BiosInput;
        use zamak_core::font::{PsfFont, DEFAULT_FONT};
        use zamak_core::gfx::Canvas;
        use zamak_core::tui::{draw_menu, InputSource, Key, MenuState};
        use zamak_theme::{Theme, ThemeVariant};

        let font = PsfFont::parse(DEFAULT_FONT).unwrap();
        let mut canvas = Canvas::new(&mut fb);
        let mut input = BiosInput;

        let theme_variant = ThemeVariant::parse(&config.theme_variant);
        let theme = Theme::default().with_variant(theme_variant);

        let mut state = if config.config_hash.is_some() {
            MenuState::new_locked(config.timeout)
        } else {
            MenuState::new(config.timeout)
        };
        let mut time_remaining = config.timeout * 10;

        loop {
            // Draw
            draw_menu(&mut canvas, &font, &config, &state, &theme, time_remaining);

            // Poll Input
            let key = input.read_key();

            // Handle Input
            match key {
                Key::Up | Key::Char('k') => {
                    if state.selected_idx > 0 {
                        state.selected_idx -= 1;
                    }
                    time_remaining = 0; // Stop timeout
                }
                Key::Down | Key::Char('j') => {
                    if state.selected_idx < config.entries.len() - 1 {
                        state.selected_idx += 1;
                    }
                    time_remaining = 0;
                }
                Key::Edit | Key::Char('i') => {
                    state.editing = !state.editing;
                    time_remaining = 0;
                    if state.editing {
                        // Populate buffer with current cmdline
                        if let Some(entry) = config.entries.get(state.selected_idx) {
                            state.edit_buffer = String::from(&entry.cmdline);
                        }
                    }
                }
                // Basic char input for editing
                Key::Char(c) if state.editing => {
                    if c == '\n' {
                        // handled by Enter match below
                    } else {
                        state.edit_buffer.push(c);
                    }
                }
                Key::Esc => {
                    if state.editing {
                        state.editing = false;
                    }
                }
                Key::Enter => {
                    if state.editing {
                        state.editing = false;
                        // Commit?
                    } else {
                        selected_idx = state.selected_idx;
                        break;
                    }
                }
                _ => {}
            }

            // Timeout logic
            if time_remaining > 0 {
                // simple wait
                arch::spin_wait(5_000_000);
                time_remaining -= 1;
                if time_remaining == 0 {
                    break;
                }
            } else {
                // Fast poll UI
                arch::spin_wait(100_000);
            }
        }
    }

    // Load Kernel
    let selected_entry = &config.entries[selected_idx];
    let kernel_path = &selected_entry.kernel_path;

    // Load Kernel File
    let kernel_entry = fs.find_file(kernel_path).expect("Kernel not found");
    let mut kernel_buf = vec![0u8; kernel_entry.size as usize];
    fs.read_file(&kernel_entry, &mut kernel_buf)
        .expect("Failed to read kernel");

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
    let mut info = zamak_core::elf::parse_elf(&kernel_buf).expect("Invalid ELF kernel");

    // Gather Memory Map
    let mmap_entries = get_memory_map();

    let mut kernel_vaddr_start = 0xffffffff80000000;

    if info.is_pie {
        let mut rng = X86KaslrRng::new();
        // Limit randomness to avoid mapping conflicts or OOM
        // 0 to 256 * 2MB = 512MB variance
        let offset = (rng.get_u64() % 256) * 0x200000;
        kernel_vaddr_start += offset;

        // SAFETY:
        //   Preconditions: kernel_buf contains a valid PIE ELF with relocations
        //   Postconditions: all relocations adjusted to kernel_vaddr_start base
        //   Clobbers: kernel_buf contents (in-place patching)
        //   Worst-case: corrupted kernel if relocations are invalid
        unsafe {
            zamak_core::elf::apply_relocations(
                kernel_buf.as_mut_ptr(),
                kernel_vaddr_start,
                &info.relocations,
            );
        }

        // Adjust entry point if it's relative
        info.entry = kernel_vaddr_start + info.entry;
    }

    let kernel_size = kernel_buf.len();

    // Prepare ACPI/RSDP
    let rsdp = find_rsdp();

    // SMP Discovery and Startup — see module-level comment on the
    // `smp`/`trampoline` mod declarations: trampoline asm needs a
    // position-independent rewrite (tracked separately). The Limine
    // Protocol treats the SMP response as optional, so leaving it
    // `None` is a conforming bootloader behavior.
    #[cfg(feature = "smp")]
    let smp_response = smp_bringup(rsdp, &info, kernel_vaddr_start, kernel_size, &mmap_entries);
    #[cfg(not(feature = "smp"))]
    let _ = rsdp;
    #[cfg(not(feature = "smp"))]
    let smp_response: Option<protocol::SmpResponse> = None;

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
        // SAFETY: seg.paddr was set by the ELF loader; seg.mem_size is the segment size.
        let seg_slice = unsafe { core::slice::from_raw_parts(seg_ptr, seg.mem_size as usize) };
        let mut reqs = protocol::scan_requests(seg_slice);
        all_requests.append(&mut reqs);
    }

    fulfill_requests(
        &mmap_entries,
        current_video_mode,
        Some(kf),
        &loaded_modules,
        rsdp,
        smp_response,
        &all_requests,
    );

    // Setup Paging
    let pml4 = paging::setup_paging(
        info.segments[0].paddr,
        kernel_vaddr_start,
        kernel_size,
        &mmap_entries,
    );

    // Enter Long Mode
    // SAFETY:
    //   Preconditions: pml4 is a valid PML4 page table; info.entry is the kernel entry point
    //   Postconditions: CPU transitions to 64-bit long mode and jumps to kernel; never returns
    //   Clobbers: all registers (mode transition)
    //   Worst-case: triple fault if page tables are invalid or entry point is wrong
    unsafe {
        enter_long_mode(pml4.as_u64() as u32, info.entry);
    }

    loop {}
}

#[allow(dead_code)]
fn println(vga: *mut u8, line: isize, msg: &str, color: u8) {
    for (i, &byte) in msg.as_bytes().iter().enumerate() {
        // SAFETY: vga points to 0xB8000 VGA text buffer (80x25x2 = 4000 bytes);
        //         line*80+i must be within bounds for this to be valid.
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
        // SAFETY: BIOS RSDP resides in 0xE0000..0xFFFFF; reading 8 bytes for signature.
        let ptr = unsafe { start.add(i) };
        let slice = unsafe { core::slice::from_raw_parts(ptr, 8) };
        if slice == b"RSD PTR " {
            return Some(ptr as u64);
        }
    }
    None
}
