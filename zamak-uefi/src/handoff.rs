// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Architecture-specific kernel hand-off for the UEFI boot path (M4-1, M4-4, M6-1).
//!
//! Each architecture has the same contract:
//!
//! 1. Disable interrupts.
//! 2. Switch to the page tables ZAMAK built.
//! 3. Invalidate the TLB.
//! 4. Jump to the kernel entry point.
//!
//! The actual instructions differ per-arch: x86-64 writes `CR3` and `jmp`,
//! AArch64 writes `TTBR1_EL1` + `MAIR_EL1` + `TCR_EL1` + `TLBI VMALLE1IS`,
//! RISC-V 64 writes `satp` + `sfence.vma zero, zero`, LoongArch writes PGDH
//! and invalidates the STLB.

// Rust guideline compliant 2026-03-30

#![allow(clippy::missing_safety_doc)]

/// Root-table physical base for the page-table hierarchy.
///
/// The meaning varies per-arch:
/// - x86-64: PML4 physical address (goes into CR3).
/// - AArch64: L0 physical address (goes into TTBR1_EL1).
/// - RISC-V 64: root PPN encoded in SATP.
/// - LoongArch64: PGDH physical address.
pub type RootTableAddr = u64;

/// Hands off control to the kernel.
///
/// Must be called only after `ExitBootServices` has succeeded.
///
/// # Safety
///
/// - Interrupts must already be disabled or safe to disable here.
/// - `root_table` must reference a valid, fully-populated page-table
///   hierarchy that maps the kernel's virtual address range and the
///   current instruction pointer.
/// - `entry_point` must be a valid kernel entry address reachable
///   through the new translation.
/// - Does not return.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub unsafe fn jump_to_kernel(root_table: RootTableAddr, entry_point: u64) -> ! {
    use x86_64::{structures::paging::PhysFrame, PhysAddr};

    // SAFETY: Disable interrupts before switching CR3. Any pending IRQs would
    // be handled through the old IDT, which may no longer be mapped.
    core::arch::asm!("cli", options(nomem, nostack));

    x86_64::registers::control::Cr3::write(
        PhysFrame::containing_address(PhysAddr::new(root_table)),
        x86_64::registers::control::Cr3Flags::empty(),
    );

    let entry_ptr: extern "C" fn() -> ! = core::mem::transmute(entry_point);
    entry_ptr();
}

/// AArch64 kernel hand-off: disable interrupts, program MAIR/TCR/TTBR,
/// flush TLB, and jump to the kernel.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
pub unsafe fn jump_to_kernel(root_table: RootTableAddr, entry_point: u64) -> ! {
    use zamak_core::arch::aarch64::mmu;

    // SAFETY: `msr daifset, #0xF` masks all interrupt sources (D, A, I, F).
    core::arch::asm!(
        "msr daifset, #0xF",
        options(nomem, nostack, preserves_flags)
    );

    mmu::write_mair_el1(mmu::STANDARD_MAIR);

    // TCR_EL1 — 48-bit VA, 4 KiB granule, inner+outer WB cacheable, non-shareable.
    // IPS = 0b101 (48-bit PA), TG1 = 0b10 (4 KiB granule for TTBR1_EL1).
    const TCR_VALUE: u64 = (0b101 << 32)            // IPS = 48-bit PA
        | (0b10 << 30)                             // TG1 = 4 KiB
        | (0b11 << 28)                             // SH1 = inner shareable
        | (0b01 << 26)                             // ORGN1 = WB WA
        | (0b01 << 24)                             // IRGN1 = WB WA
        | (16 << 16)                               // T1SZ = 64-48 = 16
        | (16); // T0SZ = 16
    mmu::write_tcr_el1(TCR_VALUE);

    mmu::write_ttbr1_el1(root_table);

    core::arch::asm!("isb", options(nomem, nostack, preserves_flags));
    mmu::tlbi_all();

    let entry_ptr: extern "C" fn() -> ! = core::mem::transmute(entry_point);
    entry_ptr();
}

/// RISC-V 64 kernel hand-off: mask interrupts, program SATP, sfence.vma, jump.
#[cfg(target_arch = "riscv64")]
#[inline(always)]
pub unsafe fn jump_to_kernel(root_table: RootTableAddr, entry_point: u64) -> ! {
    use zamak_core::arch::riscv64::satp;

    // SAFETY: Clear the sstatus.SIE bit to mask supervisor interrupts.
    core::arch::asm!("csrci sstatus, 2", options(nomem, nostack, preserves_flags));

    // Encode SATP for Sv48 with ASID 0 and root PPN = root_table >> 12.
    let satp_value = satp::encode(satp::MODE_SV48, 0, root_table >> 12);
    satp::write_satp(satp_value);

    let entry_ptr: extern "C" fn() -> ! = core::mem::transmute(entry_point);
    entry_ptr();
}

/// LoongArch64 kernel hand-off: clear CRMD.IE, program PGDH, flush TLB, jump.
#[cfg(target_arch = "loongarch64")]
#[inline(always)]
pub unsafe fn jump_to_kernel(root_table: RootTableAddr, entry_point: u64) -> ! {
    use zamak_core::arch::loongarch64::{csr, csr_read, csr_write, invtlb_all};

    // SAFETY: CRMD bit 2 (IE) = 0 disables global interrupts.
    let crmd = csr_read::<{ csr::CRMD }>();
    csr_write::<{ csr::CRMD }>(crmd & !(1 << 2));

    csr_write::<{ csr::PGDH }>(root_table);
    core::arch::asm!("ibar 0", options(nomem, nostack, preserves_flags));
    invtlb_all();

    let entry_ptr: extern "C" fn() -> ! = core::mem::transmute(entry_point);
    entry_ptr();
}

/// Fallback for architectures that don't have a real hand-off yet (build only).
#[cfg(not(any(
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "riscv64",
    target_arch = "loongarch64"
)))]
pub unsafe fn jump_to_kernel(_root_table: RootTableAddr, _entry_point: u64) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
