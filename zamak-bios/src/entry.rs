// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! BIOS Stage 2 entry point and CPU mode transition assembly.
//!
//! Contains the 16-bit real-mode entry, protected-mode initialization,
//! and long-mode transition (`enter_long_mode`). The legacy
//! `call_bios_int` 32→real→32 trampoline is gated behind the
//! `legacy_trampoline` feature (see M1-16 Path B — all BIOS I/O now
//! happens in real mode before CR0.PE). All assembly resides in
//! `global_asm!` per Steelbore Standard §3.2.

// Rust guideline compliant 2026-03-30

use core::arch::global_asm;

// SAFETY:
//   Preconditions:
//     - BIOS has loaded this code at the address specified by the linker
//       script (0x8000) and jumped here from Stage 1
//     - DL contains the BIOS boot drive number
//     - CPU is in 16-bit real mode with interrupts disabled
//   Postconditions:
//     - CPU is in 32-bit protected mode with a flat memory model
//     - `kmain` is called with the boot drive ID as its argument
//   Clobbers:
//     - All general-purpose registers (entry point, never returns to caller)
//   Worst-case on violation:
//     - Triple fault / immediate machine reset
// §3.9.1 justification: This global_asm! block contains the boot-path
// skeleton (_start, init_32, enter_long_mode, init_64) plus its GDT.
// These sections share labels and must be linked as a single contiguous
// unit starting at the linker-script ORIGIN (0x8000). Splitting them
// would require duplicating the GDT or introducing position-dependent
// relocations.
// NOTE: `global_asm!`'s default on x86 is Intel syntax — no
// `.intel_syntax`/`.att_syntax` directives here (they became
// `-D bad_asm_style` on modern rustc and CI runs with `-D warnings`).
global_asm!(
    // =========================================================================
    // 16-bit Real Mode Entry
    // =========================================================================
    ".section .entry, \"ax\"",
    ".code16",
    ".global _start",
    "_start:",
    "    cli",
    "    xor ax, ax",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "    mov sp, 0x8000",
    // Save the BIOS boot drive (DL) before the 'Z' breadcrumb's
    // `mov dx, 0x3F8` clobbers it. The orchestration in
    // `rm_phaseb_orchestrate` re-reads from this scratch location.
    "    mov byte ptr [0x0401], dl",
    // Print 'Z' to COM1 so we know stage2 reached real-mode entry.
    "    mov dx, 0x3F8",
    "    mov al, 'Z'",
    "    out dx, al",
    "",
    "    lgdt [gdt_descriptor]",
    "",
    "    sti",                             // BIOS services need interrupts on
    // M1-16 Path B: run the entire real-mode I/O phase and populate
    // the BootDataBundle at phys 0x1000 before CR0.PE. On return,
    // every BIOS-backed datum kmain needs (E820, MBR, partition
    // image, RSDP) is in the bundle; protected-mode kmain never
    // calls BIOS again.
    //
    // Use raw bytes for the call: LLVM's Intel-syntax `.code16`
    // backend emits `call <symbol>` as a 32-bit `calll` (5 bytes,
    // pushes a 4-byte EIP). The `.byte 0xE8 + .word rel16` form
    // forces a 16-bit near call (3 bytes, pushes 2-byte IP) so it
    // matches the 16-bit `ret` (`.byte 0xC3`) at the orchestration's
    // tail. Without this, every call/ret pair leaks 2 stack bytes
    // and SS:SP eventually corrupts the BIOS-handler frame.
    ".byte 0xE8",
    ".word rm_phaseb_orchestrate - . - 2",
    "    cli",                             // back to IF=0 for the PE switch
    "",
    "    mov eax, cr0",
    "    or  eax, 1",
    "    mov cr0, eax",
    "",
    "    ljmp 0x08, offset init_32",
    // =========================================================================
    // 32-bit Protected Mode Initialization
    // =========================================================================
    ".code32",
    "init_32:",
    // Hard-disable interrupts: in 32-bit PM with no IDT loaded, any
    // hardware IRQ (PIT timer at vector 0x08) triple-faults via the
    // missing-handler path. We only re-enable once the kernel sets
    // up its own IDT.
    "    cli",
    "    mov ax, 0x10",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov fs, ax",
    "    mov gs, ax",
    "    mov ss, ax",
    "    mov esp, 0x8000",               // stable PM stack, above bundle
    // Print 'P' to COM1 so we know the 16→32 mode switch succeeded.
    "    mov dx, 0x3F8",
    "    mov al, 'P'",
    "    out dx, al",
    "",
    "    .extern kmain",
    // Path B: hand kmain the physical address of the BootDataBundle
    // at 0x1000 that `rm_phaseb_orchestrate` populated before the
    // PE transition. kmain consumes nothing else from BIOS.
    "    push 0x1000",
    "    call kmain",
    "",
    ".Lhalt:",
    "    hlt",
    "    jmp .Lhalt",
    // =========================================================================
    // enter_long_mode — Enable PAE, set EFER.LME, load CR3, enable paging,
    //                    far-jump to 64-bit code segment.
    //
    // C ABI: void enter_long_mode(uint32_t pml4_phys, uint64_t entry_point)
    //        entry_point must be pre-stored at physical address 0x5FF0.
    // =========================================================================
    ".global enter_long_mode",
    "enter_long_mode:",
    "    mov eax, [esp + 4]", // pml4_phys
    "    mov cr3, eax",
    "",
    "    mov eax, cr4",
    "    or  eax, (1 << 5)", // PAE
    "    mov cr4, eax",
    "",
    "    mov ecx, 0xC0000080", // IA32_EFER MSR
    "    rdmsr",
    "    or  eax, (1 << 8)", // LME bit
    "    wrmsr",
    "",
    "    mov eax, cr0",
    "    or  eax, (1 << 31)", // PG bit
    "    mov cr0, eax",
    "",
    "    ljmp 0x28, offset init_64",
    // =========================================================================
    // 64-bit Long Mode Entry
    // =========================================================================
    ".code64",
    "init_64:",
    "    mov rbx, [0x5FF0]", // Entry point stored here by Rust
    "    jmp rbx",
    // =========================================================================
    // GDT — Global Descriptor Table
    // =========================================================================
    ".align 4",
    "gdt_start:",
    "    .quad 0x0000000000000000", // 0x00: Null descriptor
    "    .quad 0x00cf9a000000ffff", // 0x08: Code 32 (0..4G, P, R, E)
    "    .quad 0x00cf92000000ffff", // 0x10: Data 32 (0..4G, P, W)
    "    .quad 0x00009a000000ffff", // 0x18: Code 16 (Real-mode compatible)
    "    .quad 0x000092000000ffff", // 0x20: Data 16
    "    .quad 0x00af9a000000ffff", // 0x28: Code 64 (Long Mode)
    "gdt_end:",
    "",
    "gdt_descriptor:",
    "    .word gdt_end - gdt_start - 1",
    "    .long gdt_start",
);

// =========================================================================
// call_bios_int — legacy 32→16→real→16→32 trampoline.
//
// Gated behind the `legacy_trampoline` feature. The default M1-16 Path B
// build does not include this — all BIOS I/O now happens in real mode
// before CR0.PE, and protected-mode code consumes pre-populated buffers
// from the BootDataBundle at phys 0x01000.
//
// C ABI: void call_bios_int(uint8_t int_no, BiosRegs *regs)
// =========================================================================
#[cfg(feature = "legacy_trampoline")]
global_asm!(
    ".section .entry, \"ax\"",
    ".code32",
    ".global call_bios_int",
    "call_bios_int:",
    "    push ebp",
    "    mov  ebp, esp",
    "    pusha",
    "",
    "    mov [esp_save_ptr], esp",
    "",
    "    ljmp 0x18, offset .Lpm16",
    "",
    ".code16",
    ".Lpm16:",
    "    mov ax, 0x20",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "",
    "    mov eax, cr0",
    "    and eax, 0xFFFFFFFE", // Clear PE bit
    "    mov cr0, eax",
    "",
    "    ljmp 0x00, offset .Lrm",
    "",
    ".Lrm:",
    "    xor ax, ax",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "    mov sp, 0x7000",
    "",
    "    mov eax, [ebp + 12]", // regs pointer
    "    mov edi, eax",
    "",
    "    mov eax, [edi + 0]",        // eax
    "    mov ebx, [edi + 4]",        // ebx
    "    mov ecx, [edi + 8]",        // ecx
    "    mov edx, [edi + 12]",       // edx
    "    mov esi, [edi + 16]",       // esi
    "    push dword ptr [edi + 20]", // push edi temporarily
    "",
    "    mov al, [ebp + 8]",      // int_no
    "    mov [.Lint_op + 1], al", // Self-modifying: patch interrupt number
    "",
    "    pop edi",
    "",
    ".Lint_op:",
    "    int 0", // Patched at runtime
    "",
    "    push edi",
    "    mov edi, [ebp + 12]",
    "    mov [edi + 0], eax",
    "    mov [edi + 4], ebx",
    "    mov [edi + 8], ecx",
    "    mov [edi + 12], edx",
    "    mov [edi + 16], esi",
    "    pop eax",
    "    mov [edi + 20], eax",
    "",
    "    mov eax, cr0",
    "    or  eax, 1",
    "    mov cr0, eax",
    "",
    "    ljmp 0x08, offset .Lpm32",
    "",
    ".code32",
    ".Lpm32:",
    "    mov ax, 0x10",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "",
    "    mov esp, [esp_save_ptr]",
    "    popa",
    "    pop ebp",
    "    ret",
    ".section .data",
    ".align 4",
    "esp_save_ptr:",
    "    .long 0",
);
