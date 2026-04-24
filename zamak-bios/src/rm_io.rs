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
    "    mov al, ah",           // move status into AL for caller
    "    xor ah, ah",
    "    .byte 0xC3",  // 16-bit near ret — see module header
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
    "    mov edx, 0x534D4150",  // 'SMAP'
    "    mov ecx, 24",
    "    int 0x15",
    "    .byte 0xC3",  // 16-bit near ret — see module header
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
    "    .byte 0xC3",  // 16-bit near ret — see module header
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
    "    .byte 0xC3",  // 16-bit near ret — see module header
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
    "    .byte 0xC3",  // 16-bit near ret — see module header
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
    "    .byte 0xC3",  // 16-bit near ret — see module header
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
    "    mov cr0, eax",          // PE on
    "    mov bx, 0x10",
    "    mov fs, bx",             // FS cache ← flat 32-bit data descriptor
    "    and eax, 0xFFFFFFFE",
    "    mov cr0, eax",           // PE off (FS cache persists)
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
    "    mov cr0, eax",          // PE on — required to populate ES cache
    "    mov bx, 0x10",
    "    mov es, bx",             // ES cache ← flat 32-bit data
    "    and eax, 0xFFFFFFFE",
    "    mov cr0, eax",           // PE off (ES cache persists)
    "    cld",
    // `addr32 rep movsb` = 0x67 0xF3 0xA4. Forces 32-bit address size
    // so the CPU uses ESI / EDI / ECX instead of SI / DI / CX. GAS's
    // Intel-syntax `addr32` prefix may not parse here, so emit raw.
    "    .byte 0x67, 0xF3, 0xA4",
    "    pop es",                 // restore caller's real-mode ES value +
                                   // 64-KiB limit cache (via real-mode seg load)
    "    sti",
    "    .byte 0xC3",
);
