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
//! Optional: `--timeout <seconds>` overrides the per-test budget.
//!
//! Suites are declared in `SUITES` below. Each entry maps a suite name to
//! the list of test-case descriptors it expands to. The CI `qemu-smoke`
//! and `asm-verification` jobs both invoke this binary via `--suite`.

// Rust guideline compliant 2026-03-30

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

/// Default per-test wall-clock budget when `--timeout` is not given.
///
/// A watchdog thread (see `run_test`) kills QEMU after this duration if
/// the expected serial sentinels have not been observed. Without a
/// watchdog, a silent QEMU (missing or malformed disk image) can hang
/// `reader.lines()` indefinitely — that's the bug this constant used to
/// be advisory-only for.
const DEFAULT_BOOT_TIMEOUT: Duration = Duration::from_secs(30);

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
                // bios-boot-smoke is deferred until M1-16 produces a
                // real BIOS boot chain (stage1 MBR + stage2 + kernel
                // partition). Today the "BIOS image" is a copy of the
                // UEFI ESP, which BIOS firmware cannot boot — there's
                // no MBR at LBA 0. Re-add this case once build-images.sh
                // stamps the real stage1.
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
                // The asm-verify image produced by build-images.sh is a
                // UEFI ESP (BOOTX64.EFI + zamak.conf pointing at the
                // asm-verify kernel). M1-16's full BIOS boot chain isn't
                // built yet, so run the verify kernel through the UEFI
                // path for now — the asm wrappers under test are
                // arch-level, not firmware-specific.
                mode: BootMode::Uefi,
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
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("zamak-test: {e}");
            print_usage();
            std::process::exit(2);
        }
    };

    match parsed.mode {
        Mode::Help => print_usage(),
        Mode::Single { boot, image } => run_single(boot, image, parsed.timeout),
        Mode::Suite { name } => run_suite(&name, parsed.timeout),
    }
}

struct ParsedArgs {
    mode: Mode,
    timeout: Duration,
}

enum Mode {
    Help,
    Single { boot: BootMode, image: String },
    Suite { name: String },
}

/// Hand-rolled parser. Accepts `--timeout <secs>` anywhere relative to
/// the mode flag; rejects any other leading token.
fn parse_args(argv: &[String]) -> Result<ParsedArgs, String> {
    let mut timeout = DEFAULT_BOOT_TIMEOUT;
    let mut mode: Option<Mode> = None;
    let mut i = 1;

    while i < argv.len() {
        let arg = argv[i].as_str();
        match arg {
            "--help" | "-h" => {
                return Ok(ParsedArgs {
                    mode: Mode::Help,
                    timeout,
                });
            }
            "--timeout" => {
                let val = argv
                    .get(i + 1)
                    .ok_or_else(|| "--timeout requires a seconds value".to_string())?;
                let secs: u64 = val
                    .parse()
                    .map_err(|_| format!("--timeout: invalid seconds value '{val}'"))?;
                if secs == 0 {
                    return Err("--timeout: must be greater than 0".into());
                }
                timeout = Duration::from_secs(secs);
                i += 2;
            }
            "--bios" | "--uefi" => {
                let boot = if arg == "--bios" {
                    BootMode::Bios
                } else {
                    BootMode::Uefi
                };
                let image = argv
                    .get(i + 1)
                    .ok_or_else(|| format!("{arg} requires an image path"))?
                    .clone();
                if mode.is_some() {
                    return Err("only one mode flag allowed".into());
                }
                mode = Some(Mode::Single { boot, image });
                i += 2;
            }
            "--suite" => {
                let name = argv
                    .get(i + 1)
                    .ok_or_else(|| "--suite requires a suite name".to_string())?
                    .clone();
                if mode.is_some() {
                    return Err("only one mode flag allowed".into());
                }
                mode = Some(Mode::Suite { name });
                i += 2;
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let mode = mode.ok_or_else(|| "missing mode flag".to_string())?;
    Ok(ParsedArgs { mode, timeout })
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  zamak-test [--timeout <seconds>] --bios <image>");
    eprintln!("  zamak-test [--timeout <seconds>] --uefi <esp.img>");
    eprintln!("  zamak-test [--timeout <seconds>] --suite <name>");
    eprintln!();
    eprintln!(
        "  --timeout <seconds>   Per-test wall-clock budget (default: {}s).",
        DEFAULT_BOOT_TIMEOUT.as_secs()
    );
    eprintln!();
    eprintln!("suites:");
    for (name, cases) in suites() {
        eprintln!(
            "  {name:<20} ({} case{})",
            cases.len(),
            if cases.len() == 1 { "" } else { "s" }
        );
    }
}

fn run_single(boot: BootMode, image: String, timeout: Duration) {
    let test = TestCase {
        name: "boot-smoke",
        mode: boot,
        image_path: image,
        expected_serial: vec!["ZAMAK"],
    };
    match run_test(&test, timeout) {
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

fn run_suite(wanted: &str, timeout: Duration) {
    let all = suites();
    let (_name, cases) = match all.iter().find(|(n, _)| *n == wanted) {
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
        match run_test(test, timeout) {
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

fn run_test(test: &TestCase, timeout: Duration) -> TestResult {
    let mut cmd = build_qemu_command(test);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return TestResult::Fail(format!("failed to spawn QEMU: {e}")),
    };

    let stdout = child.stdout.take().expect("stdout captured");
    let (done_tx, watchdog) = start_watchdog(child, timeout);

    let reader = BufReader::new(stdout);
    let mut matched = vec![false; test.expected_serial.len()];

    for line in reader.lines() {
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
            break;
        }
    }

    // Signal the watchdog either that we're finished reading, or — if
    // we broke out due to an IO error — that it should reap the child
    // regardless. Dropping the sender works equally well (it lands as
    // a Disconnected on the watchdog's side).
    let _ = done_tx.send(());
    let (timed_out, status) = watchdog.join().expect("watchdog join");

    if timed_out {
        return TestResult::Timeout;
    }

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

/// Spawn a watchdog thread that owns `child`. The caller uses the
/// returned `Sender<()>` to signal "done reading"; dropping it without
/// sending works too. The thread's result is `(timed_out, status)`.
///
/// If the watchdog's `recv_timeout` fires before the caller signals,
/// it kills `child` and reports `timed_out = true`. Either way it
/// reaps the child before returning the exit status.
fn start_watchdog(
    mut child: Child,
    timeout: Duration,
) -> (Sender<()>, JoinHandle<(bool, Option<ExitStatus>)>) {
    let (tx, rx) = mpsc::channel::<()>();
    let handle = std::thread::spawn(move || {
        let timed_out = matches!(
            rx.recv_timeout(timeout),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        if timed_out {
            let _ = child.kill();
        }
        let status = child.wait().ok();
        (timed_out, status)
    });
    (tx, handle)
}

/// Locate the OVMF CODE image. Ubuntu 22.04 ships
/// `/usr/share/OVMF/OVMF_CODE.fd`; 24.04+ renamed it to
/// `OVMF_CODE_4M.fd` (and sometimes `.ms.fd` for Secure Boot).
/// Nix puts them under `$out/FV/`. Caller can override via `OVMF_DIR`.
fn find_ovmf_code(ovmf_dir: &str) -> Option<String> {
    for name in [
        "OVMF_CODE.fd",
        "OVMF_CODE_4M.fd",
        "OVMF_CODE_4M.ms.fd",
        "OVMF.fd",
    ] {
        let p = format!("{ovmf_dir}/{name}");
        if std::path::Path::new(&p).is_file() {
            return Some(p);
        }
    }
    None
}

/// Find the OVMF VARS template and return a path to a fresh WRITABLE
/// copy under `/tmp`. Distro packages keep the canonical VARS file
/// read-only so QEMU's pflash writes fail; we always work on a copy.
fn writable_ovmf_vars(ovmf_dir: &str) -> Option<String> {
    let src = ["OVMF_VARS.fd", "OVMF_VARS_4M.fd", "OVMF_VARS_4M.ms.fd"]
        .iter()
        .map(|n| format!("{ovmf_dir}/{n}"))
        .find(|p| std::path::Path::new(p).is_file())?;

    let dst = format!("/tmp/zamak-test-ovmf-vars-{}.fd", std::process::id());
    std::fs::copy(&src, &dst).ok()?;
    // The destination inherits mode from the source on Linux; make
    // sure owner has write permission regardless of how the distro
    // packaged the template.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        if let Ok(meta) = std::fs::metadata(&dst) {
            let mut perms = meta.permissions();
            perms.set_mode(0o600);
            let _ = std::fs::set_permissions(&dst, perms);
        }
    }
    Some(dst)
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
            let ovmf_dir = std::env::var("OVMF_DIR").unwrap_or_else(|_| "/usr/share/OVMF".into());
            let code = find_ovmf_code(&ovmf_dir).unwrap_or_else(|| {
                panic!("zamak-test: could not find OVMF_CODE.fd (or _4M variants) under {ovmf_dir}")
            });
            let vars = writable_ovmf_vars(&ovmf_dir).unwrap_or_else(|| {
                panic!(
                    "zamak-test: could not find/copy OVMF_VARS.fd (or _4M variants) from {ovmf_dir}"
                )
            });
            cmd.args([
                "-drive",
                &format!("if=pflash,format=raw,readonly=on,file={code}"),
            ]);
            cmd.args(["-drive", &format!("if=pflash,format=raw,file={vars}")]);
            cmd.args(["-drive", &format!("format=raw,file={}", test.image_path)]);
        }
    }

    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// A silent, long-running child stands in for a hung QEMU: produces
    /// no stdout, never exits on its own. The watchdog must kill it.
    #[test]
    fn watchdog_kills_silent_child_and_sets_flag() {
        let child = Command::new("sleep")
            .arg("60")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sleep");

        let start = Instant::now();
        let (_tx, handle) = start_watchdog(child, Duration::from_secs(1));
        let (timed_out, status) = handle.join().expect("watchdog join");
        let elapsed = start.elapsed();

        assert!(
            elapsed < Duration::from_secs(3),
            "watchdog should reap child within 3s, took {elapsed:?}"
        );
        assert!(timed_out, "timed_out must be true after watchdog fires");
        assert!(
            status.map(|s| !s.success()).unwrap_or(true),
            "killed child must not report success"
        );
    }

    /// When the caller signals "done" before the budget elapses the
    /// watchdog must NOT flag a timeout.
    #[test]
    fn watchdog_does_not_flag_when_done_signalled() {
        let child = Command::new("true")
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn true");

        let (tx, handle) = start_watchdog(child, Duration::from_secs(30));
        // Give the child a moment to exit on its own, then signal.
        std::thread::sleep(Duration::from_millis(100));
        let _ = tx.send(());
        let (timed_out, _status) = handle.join().expect("watchdog join");
        assert!(!timed_out, "timed_out must be false when caller signals");
    }

    #[test]
    fn parse_args_accepts_timeout_before_mode() {
        let argv: Vec<String> = ["zamak-test", "--timeout", "5", "--suite", "boot-smoke"]
            .into_iter()
            .map(String::from)
            .collect();
        let parsed = parse_args(&argv).expect("parse ok");
        assert_eq!(parsed.timeout, Duration::from_secs(5));
        match parsed.mode {
            Mode::Suite { name } => assert_eq!(name, "boot-smoke"),
            _ => panic!("expected Suite mode"),
        }
    }

    #[test]
    fn parse_args_accepts_timeout_after_mode() {
        let argv: Vec<String> = ["zamak-test", "--suite", "boot-smoke", "--timeout", "7"]
            .into_iter()
            .map(String::from)
            .collect();
        let parsed = parse_args(&argv).expect("parse ok");
        assert_eq!(parsed.timeout, Duration::from_secs(7));
    }

    #[test]
    fn parse_args_defaults_to_constant_when_missing() {
        let argv: Vec<String> = ["zamak-test", "--suite", "boot-smoke"]
            .into_iter()
            .map(String::from)
            .collect();
        let parsed = parse_args(&argv).expect("parse ok");
        assert_eq!(parsed.timeout, DEFAULT_BOOT_TIMEOUT);
    }

    #[test]
    fn parse_args_rejects_zero_timeout() {
        let argv: Vec<String> = ["zamak-test", "--timeout", "0", "--suite", "boot-smoke"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_rejects_non_numeric_timeout() {
        let argv: Vec<String> = ["zamak-test", "--timeout", "abc", "--suite", "boot-smoke"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn parse_args_rejects_duplicate_mode() {
        let argv: Vec<String> = ["zamak-test", "--suite", "boot-smoke", "--bios", "foo.img"]
            .into_iter()
            .map(String::from)
            .collect();
        assert!(parse_args(&argv).is_err());
    }
}
