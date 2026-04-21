// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

#![no_std]

extern crate alloc;

/// Re-export `#[zamak_unsafe]` proc-macro for assembly boundary marking (§3.9).
pub use zamak_macros::zamak_unsafe;

pub mod addr;
pub mod arch;
pub mod blake2b;
pub mod chainload;
pub mod config;
pub mod config_discovery;
pub mod elf;
pub mod enrolled_hash;
pub mod ext2;
pub mod font;
pub mod fs;
pub mod gfx;
pub mod iso9660;
pub mod linux_boot;
pub mod multiboot;
pub mod multiboot2;
pub mod net;
pub mod pe;
pub mod pmm;
pub mod protocol;
pub mod rng;
pub mod theme_loader;
pub mod tui;
pub mod uri;
pub mod vmm;
pub mod wallpaper;
