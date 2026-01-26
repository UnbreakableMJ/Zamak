// SPDX-License-Identifier: GPL-3.0-or-later

use std::process::Command;
use std::env;
use std::path::Path;

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();
    
    // Check if nasm is available via nix-shell if not in path
    let nasm_cmd = if Command::new("nasm").arg("--version").status().is_ok() {
        "nasm"
    } else {
        // This is a bit of a hack for the environment
        "nasm" 
    };

    println!("cargo:rerun-if-changed=src/entry.asm");
    
    let status = Command::new(nasm_cmd)
        .args(&["-f", "elf32", "src/entry.asm", "-o"])
        .arg(&format!("{}/entry.o", out_dir))
        .status();

    if let Err(e) = status {
        panic!("Failed to run nasm: {}", e);
    }
    
    if !status.unwrap().success() {
        panic!("nasm failed to compile entry.asm");
    }

    // Create a static library so cargo can find it
    let status = Command::new("ar")
        .args(&["crus", "libentry.a", "entry.o"])
        .current_dir(Path::new(&out_dir))
        .status()
        .expect("Failed to run ar");

    if !status.success() {
        panic!("ar failed to create libentry.a");
    }

    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=entry");
    
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{}/linker.ld", manifest_dir);
}
