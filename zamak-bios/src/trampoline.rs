// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! SMP Application Processor (AP) startup trampoline.
//!
//! This code is copied to below 1 MiB at runtime and executed by each
//! AP after receiving a SIPI. It transitions the AP from 16-bit real
//! mode through 32-bit protected mode to 64-bit long mode, then parks
//! the AP waiting for a `goto_address`. All assembly resides in
//! `global_asm!` per Steelbore Standard §3.2.

// Rust guideline compliant 2026-03-30

use core::arch::global_asm;

// SAFETY:
//   Preconditions:
//     - This code block has been copied to a physical address below 1 MiB
//       (the address encoded in the SIPI vector)
//     - `trampoline_pml4_ptr` has been patched with the BSP's PML4 physical
//       address before sending the SIPI
//     - The GDT referenced by the trampoline is accessible at the copied
//       address
//   Postconditions:
//     - The AP is in 64-bit long mode, halted in a spin-wait loop
//     - The AP can be dispatched by writing a non-zero value to
//       its `goto_address` field
//   Clobbers:
//     - All general-purpose registers on the AP
//   Worst-case on violation:
//     - AP triple-faults; system hangs or resets

/// Linker symbols marking the trampoline boundaries.
/// Used by Rust code to compute the trampoline size and
/// copy it to the target physical address.
extern "C" {
    pub static trampoline_start: u8;
    pub static trampoline_end: u8;
}

/// Returns the size of the trampoline code block in bytes.
///
/// # Safety
///
/// This function reads linker-provided symbol addresses.
/// The symbols are always valid when the binary is linked correctly.
pub fn trampoline_size() -> usize {
    // SAFETY:
    //   Preconditions:
    //     - `trampoline_start` and `trampoline_end` are linker symbols
    //       defined in the global_asm! block below
    //   Postconditions:
    //     - Returns the byte distance between the two symbols
    //   Clobbers:
    //     - None (pure address arithmetic)
    //   Worst-case on violation:
    //     - Incorrect size leads to partial copy; AP triple-faults
    unsafe { &trampoline_end as *const u8 as usize - &trampoline_start as *const u8 as usize }
}

// §3.9.1 justification: The SMP trampoline (~22 instructions + GDTs + patchable
// fields) must be a single contiguous block because the BSP copies it wholesale
// to a physical address below 1 MiB before sending the SIPI. Splitting would
// break the relocation and GDT-relative addressing.
global_asm!(
    ".intel_syntax noprefix",
    ".pushsection .trampoline, \"ax\"",
    ".global trampoline_start",
    "trampoline_start:",
    // =========================================================================
    // 16-bit Real Mode — AP wakes up here after SIPI
    // =========================================================================
    ".code16",
    "    cli",
    "    xor ax, ax",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "",
    "    lgdt [trampoline_gdt_ptr]",
    "",
    // Enable Protected Mode
    "    mov eax, cr0",
    "    or  eax, 1",
    "    mov cr0, eax",
    "",
    "    ljmp 0x08, offset .Ltramp_pm_start",
    "",
    // =========================================================================
    // 32-bit Protected Mode
    // =========================================================================
    ".code32",
    ".Ltramp_pm_start:",
    "    mov ax, 0x10",
    "    mov ds, ax",
    "    mov es, ax",
    "    mov ss, ax",
    "",
    // Enable PAE
    "    mov eax, cr4",
    "    or  eax, (1 << 5)",
    "    mov cr4, eax",
    "",
    // Load page tables (PML4 address patched by BSP at runtime)
    "    mov eax, [trampoline_pml4_ptr]",
    "    mov cr3, eax",
    "",
    // Enable Long Mode via IA32_EFER MSR
    "    mov ecx, 0xC0000080",
    "    rdmsr",
    "    or  eax, (1 << 8)", // LME
    "    wrmsr",
    "",
    // Enable Paging
    "    mov eax, cr0",
    "    or  eax, (1 << 31)",
    "    mov cr0, eax",
    "",
    // Jump to 64-bit code
    "    lgdt [trampoline_gdt_ptr_long]",
    "    ljmp 0x08, offset .Ltramp_long_start",
    "",
    // =========================================================================
    // 64-bit Long Mode — AP parks here
    // =========================================================================
    ".code64",
    ".Ltramp_long_start:",
    "    hlt",
    "    jmp .Ltramp_long_start",
    // =========================================================================
    // GDTs for the trampoline (must be within the copied block)
    // =========================================================================
    ".align 8",
    "trampoline_gdt:",
    "    .quad 0x0000000000000000", // Null
    "    .quad 0x00cf9a000000ffff", // Code 32
    "    .quad 0x00cf92000000ffff", // Data 32
    "trampoline_gdt_ptr:",
    "    .word . - trampoline_gdt - 1",
    "    .long trampoline_gdt",
    "",
    "trampoline_gdt_long:",
    "    .quad 0x0000000000000000", // Null
    "    .quad 0x00af9a000000ffff", // Code 64
    "    .quad 0x00af92000000ffff", // Data 64
    "trampoline_gdt_ptr_long:",
    "    .word . - trampoline_gdt_long - 1",
    "    .quad trampoline_gdt_long",
    // =========================================================================
    // Patchable fields (written by BSP before sending SIPI)
    // =========================================================================
    ".align 8",
    ".global trampoline_pml4_ptr",
    "trampoline_pml4_ptr:",
    "    .long 0", // PML4 physical address (patched)
    ".global trampoline_entry_point",
    "trampoline_entry_point:",
    "    .quad 0", // Entry point (patched)
    ".global trampoline_stack_top",
    "trampoline_stack_top:",
    "    .quad 0", // Stack top (patched)
    ".global trampoline_end",
    "trampoline_end:",
    ".popsection",
    ".att_syntax prefix",
);
