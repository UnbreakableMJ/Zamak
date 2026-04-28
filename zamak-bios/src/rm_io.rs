// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Real-mode I/O primitives for the M1-16 Path B boot flow.
//!
//! Every routine in this module is 16-bit asm in a single
//! `global_asm!` block linked into `.entry`, so the labels are
//! reachable by near calls from `_start` while the CPU is still in
//! real (or unreal) mode with CS = 0x0000. None of them are called
//! from Rust — the orchestration in `entry.rs::_start` fires them
//! directly via asm.
//!
//! Calling convention
//! ------------------
//! Each routine documents its own register-based ABI in a header
//! comment. None use cdecl: stack-frame marshalling from 16-bit asm
//! into a Rust `extern "C"` fn would require the compiler to emit
//! 16-bit-safe prologues, which rustc on the `i686-zamak` target
//! does not guarantee. Instead, callers load inputs into the named
//! registers and read outputs from the documented return registers
//! after a near `call`.
//!
//! Conventions common to every routine:
//! - CPU is assumed to be in 16-bit real mode (or unreal mode — same
//!   code works in both, because we only ever address data below
//!   1 MiB via segment:offset pairs) with DS = 0 and SS = 0 — the
//!   exact state `_start` leaves at the start of the real-mode
//!   phase.
//! - Interrupts are enabled (`sti` before each routine call,
//!   because BIOS services need interrupts on).
//! - Routines preserve SI, DI, BP, the segment registers DS / ES /
//!   FS / GS / SS, and the direction flag. Everything else (AX,
//!   BX, CX, DX, flags) may be clobbered.
//!
//! Scratch addresses (all below 1 MiB, all documented in
//! `boot_bundle.rs`'s module header):
//!   0x00700..0x00710   Disk Address Packet scratch for INT 13h
//!   0x05000..0x05200   VBE InfoBlock + ModeInfo scratch (Phase 5)
//!   0x05000..0x06FFF   FAT32 real-mode bounce-buffer zone
//!
//! Return-opcode note
//! ------------------
//! Every routine ends with `.byte 0xC3` instead of the `ret`
//! mnemonic. GAS in Intel-syntax `.code16` mode emits `ret` as
//! `66 c3` — the 32-bit `retd`, which pops 4 bytes from SP. A
//! real-mode caller only pushed 2 bytes for the return IP, so the
//! extra pop desynchronizes the stack and corrupts the next
//! operation. The raw `0xC3` byte is the unambiguous 16-bit near
//! ret.

// Rust guideline compliant 2026-04-25

use core::arch::global_asm;

// SAFETY:
//   Preconditions:
//     - CPU is in 16-bit real mode or unreal mode (the BIOS service
//       doesn't care which, as long as CS is a real-mode code
//       segment and the descriptor caches for DS/ES are compatible).
//     - DS = 0, ES = 0, SS = 0, SP points at a real-mode stack
//       (0x7000 per Stage 2 convention).
//     - For each routine, the register-argument contract at its
//       header comment is met by the caller.
//   Postconditions:
//     - Each routine returns to the caller via 16-bit near `ret`.
//     - Results land in the documented return registers; callee-saved
//       registers are restored.
//   Clobbers:
//     - Per-routine: AX, BX, CX, DX, and flags at minimum.
//   Worst-case on violation:
//     - BIOS returns an error code in AH / AX (caller must check).
//     - A bad DAP or bad segment can triple-fault — but this is no
//       worse than the legacy `call_bios_int` trampoline we're
//       replacing.
// §3.9.1 justification: Each routine is a narrow wrapper around one
// BIOS entry point (INT 13h AH=42h / INT 15h AX=E820h / INT 10h
// AX=4F00h-02h). They're co-located in one `global_asm!` so they
// share the `.code16` mode directive and so the assembler emits them
// contiguously in `.entry` right after `_start`.
global_asm!(
    ".section .entry, \"ax\"",
    ".code16",
    // =======================================================================
    // rm_disk_read_ext
    //
    // Issue INT 13h, AH=42h (Extended Read) using a Disk Address Packet
    // that has already been written to memory by the caller.
    //
    // In:
    //   DL = BIOS drive number (0x80 for first HDD)
    //   SI = offset of a 16-byte Disk Address Packet (DS:SI)
    // Out:
    //   AL = AH from BIOS after INT 13h (0x00 on success, non-zero =
    //        BIOS error code). AH itself is zeroed on return.
    //   CF = set on error (mirrors BIOS semantics; callers normally
    //        just check AL).
    // Clobbers: AX, flags.
    // =======================================================================
    ".global rm_disk_read_ext",
    "rm_disk_read_ext:",
    "    mov ah, 0x42",
    "    int 0x13",
    "    mov al, ah", // move status into AL for caller
    "    xor ah, ah",
    "    .byte 0xC3", // 16-bit near ret — see module header
    // =======================================================================
    // rm_e820_next
    //
    // Fetch one E820 entry via INT 15h, AX=E820h.
    //
    // In:
    //   EBX = continuation value (0 to start enumeration; the value
    //         returned in EBX from the previous call to iterate)
    //   DI  = offset of a 24-byte output buffer (ES:DI)
    // Out:
    //   EAX = SMAP magic (0x534D4150) on success, anything else on error
    //   EBX = next continuation value (0 after the final entry)
    //   ECX = number of bytes the BIOS wrote into [ES:DI] (usually 20
    //         or 24)
    //   CF  = set on error
    // Clobbers: EAX, EBX, ECX, EDX, flags.
    // =======================================================================
    ".global rm_e820_next",
    "rm_e820_next:",
    "    mov eax, 0xE820",
    "    mov edx, 0x534D4150", // 'SMAP'
    "    mov ecx, 24",
    "    int 0x15",
    "    .byte 0xC3", // 16-bit near ret — see module header
    // =======================================================================
    // rm_vbe_info
    //
    // Fetch the VBE controller info block via INT 10h, AX=4F00h.
    //
    // In:
    //   DI = offset of a 512-byte VbeInfoBlock (ES:DI). Caller must
    //        pre-stamp the first 4 bytes with "VBE2" so the BIOS
    //        returns the extended 512-byte block rather than the
    //        legacy 256-byte form.
    // Out:
    //   AX = 0x004F on success, anything else on error.
    // Clobbers: AX, flags.
    // =======================================================================
    ".global rm_vbe_info",
    "rm_vbe_info:",
    "    mov ax, 0x4F00",
    "    int 0x10",
    "    .byte 0xC3", // 16-bit near ret — see module header
    // =======================================================================
    // rm_vbe_mode_info
    //
    // Fetch a VBE mode-info record via INT 10h, AX=4F01h.
    //
    // In:
    //   CX = mode number (as reported in the InfoBlock's video_mode list)
    //   DI = offset of a 256-byte VbeModeInfo buffer (ES:DI)
    // Out:
    //   AX = 0x004F on success, anything else on error.
    // Clobbers: AX, flags.
    // =======================================================================
    ".global rm_vbe_mode_info",
    "rm_vbe_mode_info:",
    "    mov ax, 0x4F01",
    "    int 0x10",
    "    .byte 0xC3", // 16-bit near ret — see module header
    // =======================================================================
    // rm_vbe_set_mode
    //
    // Activate a VBE mode via INT 10h, AX=4F02h.
    //
    // In:
    //   BX = mode number | 0x4000 (bit 14 = LFB = linear
    //        framebuffer; bit 15 = don't clear display memory —
    //        we leave that bit clear so the BIOS zeroes it).
    // Out:
    //   AX = 0x004F on success, anything else on error.
    // Clobbers: AX, flags.
    // =======================================================================
    ".global rm_vbe_set_mode",
    "rm_vbe_set_mode:",
    "    mov ax, 0x4F02",
    "    int 0x10",
    "    .byte 0xC3", // 16-bit near ret — see module header
    // =======================================================================
    // rm_outb_com1
    //
    // Write a single byte to COM1 (0x3F8). Used for boot-phase
    // breadcrumbs from the real-mode orchestration in `_start`.
    //
    // In:
    //   AL = byte to emit
    // Out:
    //   nothing (AL preserved, which is why the `mov dx, 0x3F8 ;
    //   out dx, al` idiom here is done in-place without touching AL).
    // Clobbers: DX, flags.
    // =======================================================================
    ".global rm_outb_com1",
    "rm_outb_com1:",
    "    mov dx, 0x3F8",
    "    out dx, al",
    "    .byte 0xC3", // 16-bit near ret — see module header
    // =======================================================================
    // rm_unreal_enter
    //
    // Transition into unreal mode so FS retains a 4 GiB flat descriptor
    // cache after PE is cleared. DS / ES / SS / CS stay real-mode, so
    // subsequent INT 13h / 15h / 10h calls continue to work against
    // their BIOS-friendly segment:offset pairs. Only the FS prefix
    // (and, transiently, ES during `rm_memcpy_to_high`) crosses the
    // 1 MiB boundary.
    //
    // Uses the existing GDT from entry.rs: selector 0x10 = flat 32-bit
    // data (base=0, limit=4 GiB, G=1, D/B=1). No new descriptor table
    // is required.
    //
    // In:  nothing.
    // Out: FS descriptor cache populated with a flat 32-bit data segment.
    // Clobbers: AX, BX, EAX, flags.
    // =======================================================================
    ".global rm_unreal_enter",
    "rm_unreal_enter:",
    "    cli",
    "    lgdt [gdt_descriptor]",
    "    mov eax, cr0",
    "    or  eax, 1",
    "    mov cr0, eax", // PE on
    "    mov bx, 0x10",
    "    mov fs, bx", // FS cache ← flat 32-bit data descriptor
    "    and eax, 0xFFFFFFFE",
    "    mov cr0, eax", // PE off (FS cache persists)
    "    sti",
    "    .byte 0xC3",
    // =======================================================================
    // rm_memcpy_to_high
    //
    // Copy `ECX` bytes from low-memory source `[DS:ESI]` (DS = 0
    // assumed by caller) to a 32-bit linear destination `EDI` that may
    // live above the 1 MiB boundary.
    //
    // ES is flipped to the flat 32-bit descriptor for the duration of
    // the copy (via a transient PE-on/PE-off round trip), then popped
    // back to its saved real-mode value so the next BIOS call sees the
    // ES the caller expected.
    //
    // In:
    //   ESI = source linear address (must be < 64 KiB if DS == 0, or
    //         below the 1 MiB mark — we only ever call this with a
    //         bounce buffer in low memory).
    //   EDI = destination linear address (any 32-bit address; usually
    //         >= 0x0100_0000 for the kernel load buffer).
    //   ECX = byte count.
    // Out:
    //   ESI, EDI, ECX consumed per `rep movsb` semantics.
    // Clobbers: AX, BX, EAX, flags, DF (cleared by `cld`).
    // =======================================================================
    ".global rm_memcpy_to_high",
    "rm_memcpy_to_high:",
    "    push es",
    "    cli",
    "    mov eax, cr0",
    "    or  eax, 1",
    "    mov cr0, eax", // PE on — required to populate ES cache
    "    mov bx, 0x10",
    "    mov es, bx", // ES cache ← flat 32-bit data
    "    and eax, 0xFFFFFFFE",
    "    mov cr0, eax", // PE off (ES cache persists)
    "    cld",
    // `addr32 rep movsb` = 0x67 0xF3 0xA4. Forces 32-bit address size
    // so the CPU uses ESI / EDI / ECX instead of SI / DI / CX. GAS's
    // Intel-syntax `addr32` prefix may not parse here, so emit raw.
    "    .byte 0x67, 0xF3, 0xA4",
    "    pop es", // restore caller's real-mode ES value +
    // 64-KiB limit cache (via real-mode seg load)
    "    sti",
    "    .byte 0xC3",
    // =======================================================================
    // rm_load_chunk
    //
    // Load up to 16 contiguous sectors starting at LBA `EBX` into a
    // 32-bit destination `EDI` via the bounce buffer at phys 0x5000.
    //
    // The outer loop (sector-by-sector advancement, count tracking,
    // error propagation) lives in the `_start` orchestration — this
    // routine handles one chunk and leaves register state unchanged
    // for registers the caller needs to track across iterations
    // (EBX/ECX/EDI are consumed as inputs, not preserved here; the
    // caller recomputes them between chunks).
    //
    // In:
    //   DL  = BIOS drive number
    //   EBX = LBA (low 32 bits; the DAP's high-32 is zeroed)
    //   AX  = sector count (1..=16)
    //   EDI = destination linear address
    // Out:
    //   AL  = 0x00 on success, BIOS AH code on failure.
    // Clobbers: AX, CX, DX, ESI, flags, DF, ES (restored).
    // =======================================================================
    // =======================================================================
    // rm_phaseb_orchestrate
    //
    // Run the entire real-mode I/O phase and populate the
    // `BootDataBundle` at phys 0x1000.
    //
    // Called exactly once from `_start` while the CPU is in 16-bit
    // real mode with DS/ES/SS = 0 and SP = 0x8000. On return the
    // bundle is fully populated, `ZBDL_MAGIC` is stamped last, and
    // the caller may proceed with the CR0.PE transition to
    // protected mode.
    //
    // Scratch layout (phys):
    //   0x00400..0x004FF   orchestration loop state (boot drive, E820
    //                      continuation/DI/count, partition-load
    //                      cursors)
    //   0x00500..0x006FF   MBR sector scratch (1 sector = 512 B)
    //   0x00700..0x0071F   INT 13h DAP + rm_load_chunk byte-count slot
    //   0x01000..0x02D47   BootDataBundle
    //   0x05000..0x06FFF   FAT32 bounce buffer (rm_load_chunk / rm_memcpy_to_high)
    //
    // In:  DL = BIOS boot drive (as handed to `_start` from Stage 1).
    // Out: Bundle at 0x1000 populated. Magic stamped last.
    //      On failure: emits '?' on COM1 and halts forever — Stage 2
    //      can't return usefully without disk I/O.
    // =======================================================================
    ".global rm_phaseb_orchestrate",
    "rm_phaseb_orchestrate:",
    // Boot drive is already saved at [0x0401] by `_start` before its
    // 'Z' breadcrumb's `mov dx, 0x3F8` clobbers DL.
    "    cld",
    // Zero the bundle region so fields we don't write stay 0 for kmain.
    "    xor ax, ax",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov di, 0x1000",
    "    mov cx, 0x1E00", // 7680 bytes covers the bundle
    "    xor al, al",
    "    rep stosb",
    // Enter unreal mode so FS has a flat cache (rm_memcpy_to_high
    // does its own ES flip, but unreal_enter also exercises the GDT
    // and gives us a known CR0 starting state).
    ".byte 0xE8",
    ".word rm_unreal_enter - . - 2",
    "    mov al, 'U'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    "    mov al, [0x0401]",
    "    mov byte ptr [0x1004], al",
    // ---- E820 walk ----
    "    mov al, 'E'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    "    mov dword ptr [0x0410], 0",
    "    mov word ptr  [0x0414], 0x100C",
    "    mov dword ptr [0x0418], 0",
    ".Lpb_e820_loop:",
    "    mov ebx, [0x0418]",
    "    mov di, [0x0414]",
    "    xor ax, ax",
    "    mov es, ax",
    ".byte 0xE8",
    ".word rm_e820_next - . - 2",
    "    cmp eax, 0x534D4150",
    "    jne .Lpb_e820_done",
    "    mov [0x0418], ebx",
    "    add word ptr [0x0414], 24",
    "    inc dword ptr [0x0410]",
    "    test ebx, ebx",
    "    jz .Lpb_e820_done",
    "    cmp dword ptr [0x0410], 128",
    "    jb .Lpb_e820_loop",
    ".Lpb_e820_done:",
    "    mov eax, [0x0410]",
    "    mov dword ptr [0x1008], eax",
    // ---- MBR read ----
    ".Lpb_mbr_first:",
    "    mov al, 'M'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    // Build DAP via .word stores (bypasses any subtle word-immediate
    // encoding quirks GAS+LLVM might have for `mov word ptr [imm], imm`).
    "    xor ax, ax",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov di, 0x0700",
    "    mov ax, 0x0010",
    "    stosw", // size + reserved
    "    mov ax, 1",
    "    stosw", // count
    "    mov ax, 0x0500",
    "    stosw", // offset = 0x0500
    "    mov ax, 0",
    "    stosw", // segment = 0 → phys 0x0500
    "    xor ax, ax",
    "    stosw", // LBA[0..1] = 0 (MBR)
    "    stosw", // LBA[2..3]
    "    stosw", // LBA[4..5]
    "    stosw", // LBA[6..7]
    "    mov dl, [0x0401]",
    "    mov si, 0x0700",
    "    mov ah, 0x42",
    "    int 0x13",
    "    mov al, ah",
    "    jc  .Lpb_fail",
    // ---- Scan MBR partition table at 0x06BE..0x06FD (4 × 16 bytes) ----
    // (Sector loaded at phys 0x0500; partition table is at MBR offset
    //  446 = 0x1BE; phys = 0x0500 + 0x1BE = 0x06BE.)
    "    mov bx, 0x06BE",
    "    mov cx, 4",
    ".Lpb_part_scan:",
    "    mov al, [bx + 4]", // partition type byte
    "    cmp al, 0x0B",
    "    je .Lpb_part_found",
    "    cmp al, 0x0C",
    "    je .Lpb_part_found",
    "    cmp al, 0x83",
    "    je .Lpb_part_found",
    "    add bx, 16",
    "    loop .Lpb_part_scan",
    "    jmp .Lpb_fail", // no FAT32/Linux partition
    ".Lpb_part_found:",
    "    mov eax, [bx + 8]", // partition LBA (u32, offset 8 in entry)
    "    mov dword ptr [0x1C0C], eax", // bundle.partition_lba
    "    mov al, [bx + 4]",
    "    mov byte ptr [0x1C10], al", // bundle.partition_type
    // ---- Bulk-load partition into phys 0x0200_0000 (32 MiB), cap 8 MiB ----
    "    mov al, 'L'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    // Loop state at 0x0420:
    //   [0x0420] u32 remaining_sectors (8 MiB / 512 = 0x4000)
    //   [0x0424] u32 current LBA
    //   [0x0428] u32 dest_phys cursor
    "    mov eax, [0x1C0C]",
    "    mov [0x0424], eax",
    "    mov dword ptr [0x0428], 0x02000000",
    "    mov dword ptr [0x0420], 0x4000",
    ".Lpb_load_loop:",
    "    mov eax, [0x0420]",
    "    test eax, eax",
    "    jz .Lpb_load_done",
    "    cmp eax, 16",
    "    jbe .Lpb_have_chunk",
    "    mov eax, 16",
    ".Lpb_have_chunk:",
    "    mov dl, [0x0401]",
    "    mov ebx, [0x0424]",
    "    mov edi, [0x0428]",
    "    push eax", // stash chunk count across rm_load_chunk
    ".byte 0xE8",
    ".word rm_load_chunk - . - 2",
    "    pop ebx", // chunk sectors (reuse ebx)
    "    test al, al",
    "    jnz .Lpb_fail",
    "    mov ecx, ebx",
    "    shl ecx, 9",        // chunk bytes
    "    add [0x0428], ecx", // dest += bytes
    "    add [0x0424], ebx", // LBA += chunk
    "    sub [0x0420], ebx", // remaining -= chunk
    "    jmp .Lpb_load_loop",
    ".Lpb_load_done:",
    "    mov al, 'l'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    "    mov dword ptr [0x1C14], 0x02000000", // partition_image_phys low
    "    mov dword ptr [0x1C18], 0",          // partition_image_phys high
    "    mov dword ptr [0x1C1C], 0x00800000", // partition_image_len low (8 MiB)
    "    mov dword ptr [0x1C20], 0",          // partition_image_len high
    // ---- RSDP scan 0xE0000..0xFFFF0 for \"RSD PTR \" ----
    "    mov al, 'R'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    "    mov ebx, 0xE0000",
    ".Lpb_rsdp_loop:",
    "    cmp dword ptr [ebx + 0], 0x20445352", // \"RSD \" LE
    "    jne .Lpb_rsdp_next",
    "    cmp dword ptr [ebx + 4], 0x20525450", // \"PTR \" LE
    "    jne .Lpb_rsdp_next",
    "    mov dword ptr [0x2D28], ebx", // bundle.rsdp_phys low
    "    mov dword ptr [0x2D2C], 0",   // bundle.rsdp_phys high
    "    jmp .Lpb_rsdp_done",
    ".Lpb_rsdp_next:",
    "    add ebx, 16", // RSDP is on 16-byte boundary
    "    cmp ebx, 0xFFFF0",
    "    jb .Lpb_rsdp_loop",
    ".Lpb_rsdp_done:",
    // ---- SMBIOS / VBE: skipped in MVP; bundle fields stay 0. ----
    // ---- Stamp ZBDL_MAGIC last so kmain can detect partial init ----
    "    mov dword ptr [0x1000], 0x4C44425A", // ZBDL_MAGIC
    "    mov al, 'k'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    "    .byte 0xC3", // 16-bit near ret
    ".Lpb_fail:",
    // Emit the AL we're panicking on as two hex digits so bring-up
    // logs distinguish BIOS error code from parse-time 0xFF.
    "    mov [0x0430], al",
    "    mov al, '?'",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    "    mov al, [0x0430]",
    "    mov bl, al",
    "    shr al, 4",
    "    and al, 0x0F",
    "    cmp al, 10",
    "    jb .Lpb_fail_hi_dec",
    "    add al, 'A' - 10",
    "    jmp .Lpb_fail_hi_emit",
    ".Lpb_fail_hi_dec:",
    "    add al, '0'",
    ".Lpb_fail_hi_emit:",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    "    mov al, bl",
    "    and al, 0x0F",
    "    cmp al, 10",
    "    jb .Lpb_fail_lo_dec",
    "    add al, 'A' - 10",
    "    jmp .Lpb_fail_lo_emit",
    ".Lpb_fail_lo_dec:",
    "    add al, '0'",
    ".Lpb_fail_lo_emit:",
    ".byte 0xE8",
    ".word rm_outb_com1 - . - 2",
    ".Lpb_halt:",
    "    hlt",
    "    jmp .Lpb_halt",
    ".global rm_load_chunk",
    "rm_load_chunk:",
    "    push bp",
    "    mov bp, sp",
    // Stash inputs at DAP / near-scratch so the BIOS call clobbering
    // general registers doesn't lose them.
    "    mov word ptr [0x0700], 0x0010", // DAP size + reserved
    "    mov word ptr [0x0702], ax",     // count
    "    mov word ptr [0x0704], 0x5000", // buffer offset (bounce)
    "    mov word ptr [0x0706], 0x0000", // buffer segment
    "    mov dword ptr [0x0708], ebx",   // LBA low32
    "    mov dword ptr [0x070C], 0",     // LBA high32
    // Save the chunk byte count (AX × 512) in the scratch dword at
    // 0x0710 so we can recover it after the BIOS call blows away AX.
    "    movzx ecx, ax",
    "    shl ecx, 9",
    "    mov dword ptr [0x0710], ecx",
    // Issue the disk read. SI = DAP offset, DL still holds the drive.
    "    mov si, 0x0700",
    ".byte 0xE8",
    ".word rm_disk_read_ext - . - 2",
    "    test al, al",
    "    jnz .Lrlc_err",
    // Success: memcpy bounce → high dest.
    "    mov ecx, [0x0710]",
    "    mov esi, 0x5000",
    ".byte 0xE8",
    ".word rm_memcpy_to_high - . - 2",
    "    xor al, al",
    "    pop bp",
    "    .byte 0xC3",
    ".Lrlc_err:",
    // rm_disk_read_ext already set AL to the BIOS error code.
    "    pop bp",
    "    .byte 0xC3",
);
