// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Minimal Limine-Protocol test kernel used by `zamak-test` for end-to-end
//! smoke tests (M1-16, M2-12, §8.2).
//!
//! The kernel:
//! 1. Declares a Limine base-revision marker and a framebuffer request so
//!    the bootloader knows it speaks the protocol.
//! 2. On entry, writes a known marker string to COM1 (0x3F8).
//! 3. Exits via the QEMU ISA debug-exit device on port 0x501.
//!
//! Build with:
//!     cargo build --release -p zamak-test-kernel \
//!         --target x86_64-unknown-none \
//!         -Z build-std=core,compiler_builtins
//!
//! The resulting ELF is then copied into the BIOS disk image or UEFI
//! ESP that `zamak-test` hands to QEMU.

#![no_std]
#![no_main]

use core::panic::PanicInfo;

/// Limine base revision (§PROTOCOL.md). Revision 3 is the current latest.
#[used]
#[link_section = ".limine_requests"]
static BASE_REVISION: [u64; 3] = [0xf9562b2d5c95a6c8, 0x6a7b384944536bdc, 3];

/// Start / end markers that the bootloader scans for.
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

/// A minimal framebuffer request. We don't need the response — just its
/// presence proves ZAMAK scanned and honoured the request list.
#[repr(C)]
struct FramebufferRequest {
    id: [u64; 4],
    revision: u64,
    response: *const (),
}

// SAFETY: The struct is only read by the bootloader before we run.
unsafe impl Sync for FramebufferRequest {}

#[used]
#[link_section = ".limine_requests"]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest {
    id: [
        0xc7b1dd30df4c8b88,
        0x0a82e883a194f07b,
        0x9d5827dcd881dd75,
        0xa3148604f6fab11b,
    ],
    revision: 0,
    response: core::ptr::null(),
};

/// QEMU ISA debug-exit I/O base used by zamak-test.
const DEBUG_EXIT_PORT: u16 = 0x501;
const DEBUG_EXIT_PASS: u8 = 0x31;
const DEBUG_EXIT_FAIL: u8 = 0x32;

/// COM1 serial port (QEMU routes this to -serial stdio).
const COM1_PORT: u16 = 0x3F8;

/// Writes a byte to an x86 I/O port.
///
/// # Safety
///
/// The caller must ensure that `port` addresses a device that exists and
/// is safe to write. For COM1 and the QEMU debug-exit device this is
/// always true inside a QEMU VM.
#[cfg(target_arch = "x86_64")]
unsafe fn outb(port: u16, value: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") value,
        options(nostack, nomem, preserves_flags),
    );
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn outb(_port: u16, _value: u8) {}

/// Writes a NUL-terminated byte string to COM1.
fn serial_write(s: &[u8]) {
    for &b in s {
        // SAFETY: COM1 (0x3F8) exists in every QEMU i440fx / q35 machine.
        unsafe { outb(COM1_PORT, b) };
    }
}

/// Kernel entry point per the Limine Protocol (§PROTOCOL.md).
///
/// Receives no arguments — the bootloader has already set up long mode,
/// paging, and a stack. We just log and exit.
#[no_mangle]
pub extern "C" fn _start() -> ! {
    serial_write(b"ZAMAK\n");
    serial_write(b"LIMINE_PROTOCOL_OK\n");

    // SAFETY: writing to the QEMU debug-exit device terminates the VM
    // with exit code (value << 1) | 1. zamak-test treats 0x63 as pass.
    unsafe { outb(DEBUG_EXIT_PORT, DEBUG_EXIT_PASS) };

    // If QEMU's debug-exit device is absent, spin so the harness times out
    // rather than triple-faulting.
    loop {
        core::hint::spin_loop();
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    serial_write(b"PANIC\n");
    // SAFETY: see `_start`; we're at the end of execution anyway.
    unsafe { outb(DEBUG_EXIT_PORT, DEBUG_EXIT_FAIL) };
    loop {
        core::hint::spin_loop();
    }
}
