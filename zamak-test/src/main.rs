// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! ZAMAK QEMU integration test harness.
//!
//! Launches QEMU with the ZAMAK bootloader and a test kernel, captures
//! serial output, and checks for expected boot protocol responses.
//!
//! Usage:
//!   zamak-test --bios <image>       Run a single BIOS boot test
//!   zamak-test --uefi <esp.img>     Run a single UEFI boot test
//!   zamak-test --suite <name>       Run a named suite (see `SUITES`)
//!
//! Suites are declared in `SUITES` below. Each entry maps a suite name to
//! the list of test-case descriptors it expands to. The CI `qemu-smoke`
//! and `asm-verification` jobs both invoke this binary via `--suite`.

// Rust guideline compliant 2026-03-30

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Default timeout for a single boot test.
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);

/// Exit code from QEMU ISA debug exit device (port 0x501).
/// Test kernels write 0x31 (success) or 0x32 (failure) to this port.
const QEMU_EXIT_SUCCESS: i32 = 0x63; // (0x31 << 1) | 1
const QEMU_EXIT_FAILURE: i32 = 0x65; // (0x32 << 1) | 1

/// A single boot test case.
struct TestCase {
    name: &'static str,
    mode: BootMode,
    image_path: String,
    expected_serial: Vec<&'static str>,
}

/// Boot firmware mode.
#[derive(Clone, Copy)]
enum BootMode {
    Bios,
    Uefi,
}

/// Pre-canned test suites. Each entry resolves to one or more `TestCase`s
/// built from environment variables so the CI workflow can inject paths.
fn suites() -> Vec<(&'static str, Vec<TestCase>)> {
    vec![
        (
            "boot-smoke",
            vec![
                TestCase {
                    name: "bios-boot-smoke",
                    mode: BootMode::Bios,
                    image_path: env_path("ZAMAK_BIOS_IMAGE", "target/zamak-bios.img"),
                    expected_serial: vec!["ZAMAK", "LIMINE_PROTOCOL_OK"],
                },
                TestCase {
                    name: "uefi-boot-smoke",
                    mode: BootMode::Uefi,
                    image_path: env_path("ZAMAK_UEFI_ESP", "target/esp.img"),
                    expected_serial: vec!["ZAMAK", "LIMINE_PROTOCOL_OK"],
                },
            ],
        ),
        (
            "asm-verification",
            vec![TestCase {
                name: "asm-wrapper-state-check",
                mode: BootMode::Bios,
                image_path: env_path("ZAMAK_ASM_VERIFY_IMAGE", "target/asm-verify.img"),
                expected_serial: vec!["ASM_VERIFY_OK"],
            }],
        ),
        (
            "linux-bzimage",
            vec![TestCase {
                name: "uefi-linux-boot",
                mode: BootMode::Uefi,
                image_path: env_path("ZAMAK_LINUX_ESP", "target/linux-esp.img"),
                expected_serial: vec!["ZAMAK", "Linux version"],
            }],
        ),
    ]
}

fn env_path(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.to_string())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }

    match args[1].as_str() {
        "--help" | "-h" => {
            print_usage();
            return;
        }
        "--bios" | "--uefi" => run_single(&args),
        "--suite" => run_suite(&args),
        other => {
            eprintln!("zamak-test: unknown mode: {other}");
            print_usage();
            std::process::exit(2);
        }
    }
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  zamak-test --bios <image>");
    eprintln!("  zamak-test --uefi <esp.img>");
    eprintln!("  zamak-test --suite <name>");
    eprintln!();
    eprintln!("suites:");
    for (name, cases) in suites() {
        eprintln!("  {name:<20} ({} case{})", cases.len(), if cases.len() == 1 { "" } else { "s" });
    }
}

fn run_single(args: &[String]) {
    if args.len() < 3 {
        print_usage();
        std::process::exit(2);
    }
    let mode = match args[1].as_str() {
        "--bios" => BootMode::Bios,
        "--uefi" => BootMode::Uefi,
        _ => unreachable!(),
    };
    let test = TestCase {
        name: "boot-smoke",
        mode,
        image_path: args[2].clone(),
        expected_serial: vec!["ZAMAK"],
    };
    match run_test(&test) {
        TestResult::Pass => println!("[PASS] {}", test.name),
        TestResult::Fail(r) => {
            println!("[FAIL] {} — {r}", test.name);
            std::process::exit(1);
        }
        TestResult::Timeout => {
            println!("[TIMEOUT] {}", test.name);
            std::process::exit(1);
        }
    }
}

fn run_suite(args: &[String]) {
    if args.len() < 3 {
        eprintln!("zamak-test --suite: missing suite name");
        std::process::exit(2);
    }
    let wanted = &args[2];
    let all = suites();
    let (_name, cases) = match all.iter().find(|(n, _)| *n == wanted.as_str()) {
        Some(s) => s,
        None => {
            eprintln!("zamak-test: unknown suite '{wanted}'");
            print_usage();
            std::process::exit(2);
        }
    };

    let mut fails = 0;
    for test in cases {
        // Skip tests whose image file is missing — CI may not have every
        // artifact built yet (e.g. pre-tagged-release runs lack a bzImage).
        if !std::path::Path::new(&test.image_path).exists() {
            println!("[SKIP] {} — image {} not found", test.name, test.image_path);
            continue;
        }
        match run_test(test) {
            TestResult::Pass => println!("[PASS] {}", test.name),
            TestResult::Fail(r) => {
                println!("[FAIL] {} — {r}", test.name);
                fails += 1;
            }
            TestResult::Timeout => {
                println!("[TIMEOUT] {}", test.name);
                fails += 1;
            }
        }
    }
    if fails > 0 {
        std::process::exit(1);
    }
}

enum TestResult {
    Pass,
    Fail(String),
    Timeout,
}

fn run_test(test: &TestCase) -> TestResult {
    let mut cmd = build_qemu_command(test);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return TestResult::Fail(format!("failed to spawn QEMU: {e}")),
    };

    let stdout = child.stdout.take().expect("stdout captured");
    let reader = BufReader::new(stdout);
    let start = Instant::now();
    let mut matched = vec![false; test.expected_serial.len()];

    for line in reader.lines() {
        if start.elapsed() > BOOT_TIMEOUT {
            let _ = child.kill();
            return TestResult::Timeout;
        }

        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        for (i, pattern) in test.expected_serial.iter().enumerate() {
            if line.contains(pattern) {
                matched[i] = true;
            }
        }

        if matched.iter().all(|&m| m) {
            let _ = child.kill();
            return TestResult::Pass;
        }
    }

    let status = child.wait().ok();
    if let Some(status) = status {
        if let Some(code) = status.code() {
            if code == QEMU_EXIT_SUCCESS {
                return TestResult::Pass;
            }
            if code == QEMU_EXIT_FAILURE {
                return TestResult::Fail("test kernel reported failure".into());
            }
        }
    }

    if matched.iter().all(|&m| m) {
        TestResult::Pass
    } else {
        let missing: Vec<_> = test
            .expected_serial
            .iter()
            .zip(matched.iter())
            .filter(|(_, &m)| !m)
            .map(|(p, _)| *p)
            .collect();
        TestResult::Fail(format!("missing serial output: {missing:?}"))
    }
}

fn build_qemu_command(test: &TestCase) -> Command {
    let mut cmd = Command::new("qemu-system-x86_64");

    // Common flags: no display, serial to stdout, ISA debug exit device.
    cmd.args(["-display", "none"]);
    cmd.args(["-serial", "stdio"]);
    cmd.args(["-device", "isa-debug-exit,iobase=0x501,iosize=0x04"]);
    cmd.args(["-m", "256M"]);
    cmd.args(["-no-reboot"]);

    match test.mode {
        BootMode::Bios => {
            cmd.args(["-drive", &format!("format=raw,file={}", test.image_path)]);
        }
        BootMode::Uefi => {
            // Assumes OVMF is available at the standard path.
            let ovmf_dir =
                std::env::var("OVMF_DIR").unwrap_or_else(|_| "/usr/share/OVMF".into());
            cmd.args([
                "-drive",
                &format!("if=pflash,format=raw,readonly=on,file={ovmf_dir}/OVMF_CODE.fd"),
            ]);
            cmd.args([
                "-drive",
                &format!("if=pflash,format=raw,file={ovmf_dir}/OVMF_VARS.fd"),
            ]);
            cmd.args(["-drive", &format!("format=raw,file={}", test.image_path)]);
        }
    }

    cmd
}
