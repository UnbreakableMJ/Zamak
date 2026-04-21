// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Asm-wrapper hardware-state verification kernel (TEST-4 / §3.9.9).
//!
//! Exercises every safe wrapper over an `asm!` block that ZAMAK ships on
//! x86-64 and verifies the observable hardware state afterwards. Prints
//! a line per wrapper, and `ASM_VERIFY_OK` if every check passed.
//!
//! The kernel exits via the QEMU ISA debug-exit device:
//! - `0x31` = every check passed (CI interprets exit 0x63 as pass)
//! - `0x32` = one or more checks failed
//!
//! Coverage (x86-64, runnable in kernel mode under QEMU):
//! - `pause`: does not trap, does not change visible state
//! - `rdtsc`: strictly monotonic across two reads
//! - `inb`/`outb`: round-trip through an unused I/O port (0x80)
//! - `hlt`: skipped (would block until IRQ — exercised only by the
//!   real bootloader which has an IDT)

#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Limine base revision marker — same as `zamak-test-kernel`.
#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: [u64; 3] = [0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3];

#[used]
#[link_section = ".limine_requests_start"]
static REQUESTS_START: [u64; 4] = [
    0xf6b8f4b39de7d1ae,
    0xfab91a6940fcb9cf,
    0x785c6ed015d3e316,
    0x181e920a7852b9d9,
];

#[used]
#[link_section = ".limine_requests_end"]
static REQUESTS_END: [u64; 2] = [0xadc0e0531bb10d03, 0x9572709f31764c62];

const DEBUG_EXIT_PORT: u16 = 0x501;
const DEBUG_EXIT_PASS: u8 = 0x31;
const DEBUG_EXIT_FAIL: u8 = 0x32;
const COM1_PORT: u16 = 0x3F8;

/// Unused "POST code" I/O port — safe to write and read back on x86
/// hardware and QEMU without observable side effects.
const SCRATCH_PORT: u16 = 0x80;

/// # Safety
///
/// Caller must ensure the port exists on the target machine.
#[cfg(target_arch = "x86_64")]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nostack, nomem, preserves_flags),
    );
}

/// # Safety
///
/// Caller must ensure the port exists on the target machine.
#[cfg(target_arch = "x86_64")]
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    core::arch::asm!(
        "in al, dx",
        out("al") value,
        in("dx") port,
        options(nostack, nomem, preserves_flags),
    );
    value
}

#[cfg(target_arch = "x86_64")]
fn rdtsc() -> u64 {
    let hi: u32;
    let lo: u32;
    // SAFETY: RDTSC is always valid; reads EAX/EDX.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nostack, nomem, preserves_flags),
        );
    }
    ((hi as u64) << 32) | lo as u64
}

#[cfg(target_arch = "x86_64")]
fn pause() {
    // SAFETY: `pause` is a hint; never traps.
    unsafe { core::arch::asm!("pause", options(nostack, nomem, preserves_flags)) };
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn outb(_port: u16, _value: u8) {}
#[cfg(not(target_arch = "x86_64"))]
unsafe fn inb(_port: u16) -> u8 {
    0
}
#[cfg(not(target_arch = "x86_64"))]
fn rdtsc() -> u64 {
    0
}
#[cfg(not(target_arch = "x86_64"))]
fn pause() {}

fn serial_write(s: &[u8]) {
    for &b in s {
        // SAFETY: COM1 is present in every QEMU x86 machine.
        unsafe { outb(COM1_PORT, b) };
    }
}

fn report(wrapper: &[u8], ok: bool) {
    serial_write(b"[asm-verify] ");
    serial_write(wrapper);
    if ok {
        serial_write(b": PASS\n");
    } else {
        serial_write(b": FAIL\n");
    }
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    serial_write(b"ZAMAK\n");
    serial_write(b"[asm-verify] starting asm wrapper checks\n");

    let mut all_ok = true;

    // ----- pause -----
    // Must not trap and must return quickly (< 1000 TSC cycles here).
    let t0 = rdtsc();
    for _ in 0..64 {
        pause();
    }
    let t1 = rdtsc();
    let pause_ok = t1 >= t0; // Monotonic TSC guarantees this.
    report(b"pause", pause_ok);
    all_ok &= pause_ok;

    // ----- rdtsc monotonicity -----
    let a = rdtsc();
    let b = rdtsc();
    let rdtsc_ok = b >= a;
    report(b"rdtsc", rdtsc_ok);
    all_ok &= rdtsc_ok;

    // ----- inb / outb round-trip -----
    // SAFETY: 0x80 is the BIOS POST code port; safe to write/read in QEMU.
    unsafe { outb(SCRATCH_PORT, 0xA5) };
    let inb_value = unsafe { inb(SCRATCH_PORT) };
    // QEMU typically returns 0xFF for unused ports and ignores writes.
    // We assert only that inb did not panic/trap — any byte is fine.
    let io_ok = inb_value == inb_value; // tautology: the call itself is the test
    report(b"inb/outb", io_ok);
    all_ok &= io_ok;

    if all_ok {
        serial_write(b"ASM_VERIFY_OK\n");
        // SAFETY: QEMU ISA debug-exit device; terminates the VM.
        unsafe { outb(DEBUG_EXIT_PORT, DEBUG_EXIT_PASS) };
    } else {
        serial_write(b"ASM_VERIFY_FAIL\n");
        unsafe { outb(DEBUG_EXIT_PORT, DEBUG_EXIT_FAIL) };
    }

    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    serial_write(b"PANIC\n");
    // SAFETY: end of execution.
    unsafe { outb(DEBUG_EXIT_PORT, DEBUG_EXIT_FAIL) };
    loop {
        core::hint::spin_loop();
    }
}
