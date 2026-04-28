<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- SPDX-FileCopyrightText: 2026 Mohamed Hammad -->

# Changelog

All notable changes to the ZAMAK bootloader will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

All dates use ISO 8601 format (YYYY-MM-DD).

## [Unreleased]

### Changed

- **M1-16 Path B**: BIOS boot path lifts every BIOS interrupt
  call (INT 13h disk read, INT 15h E820 walk, RSDP scan, MBR
  partition lookup, partition bulk-load) into real mode before
  CR0.PE in `rm_phaseb_orchestrate`; protected-mode `kmain`
  consumes a `BootDataBundle` at phys 0x1000 and never returns
  to real mode. The legacy `call_bios_int` 32→real→32
  trampoline that returned `AH=0x01` is gated behind the
  `legacy_trampoline` Cargo feature.
- New `zamak-core::ram_fat32` walker — concrete-type FAT32
  parser over a borrowed `&[u8]` (no trait objects, no
  packed-struct casts), with VFAT long-filename reassembly.
  Replaces the Path B BIOS path's filesystem layer; UEFI is
  unaffected.

### Fixed

- `zamak-bios::utils::{memset, memcpy}` no longer recurse —
  the previous Rust-level bodies lowered back into themselves
  via `core::ptr::{write_bytes, copy_nonoverlapping}` and hung
  on the first non-trivial call. They're now hand-written
  `rep stos` / `rep movs` blocks.
- `zamak-bios::paging::setup_paging` ignores non-USABLE e820
  entries when sizing the HHDM and caps the mapping at 16 GiB.
  QEMU's 1 TiB high-memory MMIO entry would otherwise have
  exhausted the 4 MiB bump heap on page-table allocation.

### Re-enabled

- `bios-boot-smoke` test in `zamak-test`'s `boot-smoke` suite,
  removed in db68d69 while M1-16 was incomplete. CI now boots
  the real BIOS chain (stage1 MBR + stage2 + FAT32 partition)
  end-to-end and asserts the `ZAMAK` / `LIMINE_PROTOCOL_OK`
  serial sentinels alongside the existing UEFI case.

## [0.8.5] - 2026-04-24

Bundles the M6-3 part 1 boot-phase instrumentation plus a
non-x86 UEFI build regression fix that v0.8.4's release
workflow surfaced (AArch64 / RISC-V / LoongArch cross builds
failed strict compilation because the Linux-Boot-Protocol
dispatch added in M2-12 wasn't gated to `target_arch =
"x86_64"`; the `cross` CI job had been masking it with
`|| true` since v0.6.x).

v0.8.4's GH Release page has the x86-64 assets only
(BOOTX64.EFI + CLI binaries + SBOM + SHA256SUMS). Use v0.8.5
for the full 4-arch UEFI asset set.

### Fixed

- **Non-x86 UEFI strict builds** (`zamak-uefi/src/main.rs`,
  `zamak-uefi/src/handoff.rs`) — gated `KernelHandoff::Linux`,
  `load_linux_kernel()`, `uefi_mem_ty_to_e820()`, and the
  `zamak_core::linux_boot` import behind
  `#[cfg(target_arch = "x86_64")]` so the compile_error-free
  build holds on all four UEFI targets. AArch64 Linux booting
  uses PE/COFF + EFI-stub (a different surface) and was never
  intended to go through the bzImage path.
- **CI `cross` job no longer swallows build errors**
  (`.github` + `.forgejo` workflow mirrors) — removed the
  `|| true` tolerance on `cargo build -p zamak-uefi`. With the
  Linux-path gating in place all four targets build clean
  under `-D warnings`; silent failure had previously let the
  v0.8.4 tag ship with broken non-x86 binaries.

### Added

- **M6-3 part 1 — boot-phase TSC instrumentation**. `zamak-uefi`
  now emits `ZAMAK_PHASE=<name> tsc=<u64>` lines on COM1 at six
  checkpoints (`uefi_entry`, `config_parsed`, `menu_finished`,
  `kernel_loaded`, `requests_fulfilled`, `pre_exit_boot_services`)
  plus one `ZAMAK_TSC_MHZ=<n>` discovered via CPUID 0x16
  (`unknown` when unavailable, as in QEMU). New
  `zamak-cli bench parse-serial [--tsc-mhz <mhz>] [<path>]`
  sub-command ingests a captured UEFI serial log and emits an
  SFRS envelope whose `data.phases[]` carries `{phase, tsc,
  delta_cycles[, delta_ns]}` per checkpoint. With this, the
  bare-metal perf leg of M6-3 is a one-shot: capture
  `-serial` on real hardware, feed through the parser, compare
  against the same run on Limine. Part 2 (actual hardware run)
  remains the user's responsibility. 7 new unit tests cover
  format matrix (explicit vs log-reported `tsc_mhz`,
  malformed/missing lines, empty input).

## [0.8.4] - 2026-04-24

Banks M2-12. No other functional change since v0.8.3.

### Added

- **M2-12 done** — end-to-end Linux bzImage boot smoke under QEMU
  UEFI. New `zamak-linux-stub-kernel` freestanding crate emits a
  spec-compliant protocol-2.15 bzImage (`HdrS` magic at 0x202,
  `setup_sects = 1`, 64-bit entry at `load_addr + 0x200`) whose
  kernel body prints `Linux version 0.0.0-zamak-stub` on COM1 and
  exits via `isa-debug-exit`. `zamak-core::linux_boot` gains
  `prepare_linux_boot()` — an orchestrator that walks
  `SetupHeader` + allocates BootParams + populates E820 from a
  caller-supplied `[MemoryRegion]`. `zamak-uefi` now dispatches
  on `entry.protocol`: `"linux"` routes through a new
  `load_linux_kernel()` that converts the UEFI memory map to E820,
  allocates stable physical pages for kernel body + cmdline +
  optional initrd + BootParams zero page, then hands off via
  `handoff::jump_to_linux_kernel` (new) which sets RSI and does
  a bare `jmp entry` without installing a ZAMAK PML4 (Linux uses
  UEFI's identity map and sets up its own paging). `build-images.sh`
  assembles `target/linux-esp.img` with the stub and a
  `PROTOCOL=linux` config; CI's `qemu-smoke` job runs the new
  `linux-bzimage` suite alongside `boot-smoke` and
  `asm-verification`, all three green.

## [0.8.3] - 2026-04-24

Third release-workflow patch. v0.8.2 produced 6/9 expected assets
on the GH Release page. Three artifacts (`BOOTRISCV64.EFI`,
`BOOTLOONGARCH64.EFI`, `zamak-freebsd-x86_64`) failed to upload
with silent `"No files were found"` warnings.

### Fixed

- **`build-artifacts` upload path** — was `*.efi`, but the two
  bare-metal targets (`riscv64gc-unknown-none-elf`,
  `loongarch64-unknown-none`) produce a plain ELF named
  `zamak-uefi` (no `.efi` extension). Added an explicit `bin:`
  matrix key per target — `zamak-uefi.efi` for the two UEFI
  targets, `zamak-uefi` for bare-metal — and point the upload
  at `target/<target>/release/<bin>` exactly.
- **`cli-freebsd` upload path** — was `/tmp/zamak-freebsd/zamak`,
  which is VM-internal and not synced back to the runner.
  `vmactions/freebsd-vm@v1` DOES sync the `target/` directory
  out (it lives inside the checkout), so upload directly from
  `target/release/zamak` and drop the no-op `cp` into `/tmp`.
- **All `upload-artifact@v4` steps** — added
  `if-no-files-found: error`. Future path drifts will fail the
  job instead of silently skipping the upload with a warning,
  so we don't release ship short again without noticing.

## [0.8.2] - 2026-04-24

Second release-workflow patch. v0.8.1 successfully published a GH
Release but only 4 of the expected 9 assets appeared: the
`flatten` step renamed all files to their inner basenames
(`zamak-uefi.efi` for every EFI target, `zamak` for every CLI
target), so `dist/BOOTX64.EFI/zamak-uefi.efi` and
`dist/BOOTAA64.EFI/zamak-uefi.efi` both became `flat/zamak-uefi.efi`
and overwrote each other during the move.

### Fixed

- **`release.yml` flatten/rename step** — now walks
  `dist/<upload-name>/` directories, picks the single inner file
  per matrix entry, and copies it to `flat/<upload-name>`. So
  `BOOTX64.EFI`, `BOOTAA64.EFI`, `BOOTRISCV64.EFI`,
  `BOOTLOONGARCH64.EFI`, `zamak-linux-x86_64`,
  `zamak-macos-aarch64`, `zamak-freebsd-x86_64` all appear as
  distinct release assets.

### Note

v0.8.1's GH Release page will stay on the repo for historical
reference but is incomplete (4/9 assets). Use v0.8.2 as the
effective "0.8" release point.

## [0.8.1] - 2026-04-24

Release-workflow patch release. No code changes; fixes
`.github/workflows/release.yml` so the tag actually publishes a
GitHub Release with assets. v0.8.0's tag was created but its
publish job was a `echo TODO:` placeholder, and two matrix jobs
(`bios-stage3`, `build-artifacts i686-unknown-uefi`) failed
because neither the BIOS binary nor 32-bit x86 UEFI compiles
from the default config — they had been listed in the release
matrix since v0.6.x despite never producing a usable artifact.

### Fixed

- **`release.yml` `publish` job** — replaced the stub
  `echo "TODO: upload …"` with a real `gh release create
  --generate-notes dist/* SHA256SUMS …spdx.json`. Added
  `permissions: contents: write` so the default `GITHUB_TOKEN`
  can create the release. Artifacts are now flattened out of
  `actions/download-artifact@v4`'s per-name subdirectories
  before upload, so asset filenames on the release page are
  `BOOTX64.EFI` / `zamak-linux-x86_64` / etc. rather than
  nested paths.
- **`release.yml` `build-artifacts` matrix** — dropped the
  `i686-unknown-uefi` → `BOOTIA32.EFI` entry.
  `zamak-uefi/src/paging.rs` `compile_error!`s for
  `target_arch = "x86"`; 32-bit UEFI paging was never
  implemented. Comment in the workflow points at M6 milestones
  for the add-back.
- **`release.yml` `bios-stage3` job** — removed entirely.
  `cargo build -p zamak-bios --release` against the host target
  produces a duplicate `_start` link error because zamak-bios
  is a freestanding binary with its own entry. The job needs
  the custom i686 target + `-Z build-std` (as
  `zamak-test/build-images.sh` already does), and can't produce
  a working binary until M1-16's `call_bios_int` trampoline is
  fixed. Comment in the workflow points at the re-add criteria.
- **`build-artifacts` build flags** — added
  `-Z build-std-features=compiler-builtins-mem` to match what
  `zamak-test/build-images.sh` passes; without it the memset /
  memcpy intrinsics aren't compiled into the freestanding
  builds (not an observed failure here, but consistency fix).

### Note on v0.8.0

The `v0.8.0` git tag remains in the repo but has no associated
GitHub Release page (publish was skipped). Use `v0.8.1` as the
effective "0.8" release point.

## [0.8.0] - 2026-04-24

CI-infrastructure consolidation release. Since v0.7.0, the CI
pipeline was ported from Forgejo to GitHub Actions with full
parity; a six-hour-hang regression in the `zamak-test` QEMU
harness was fixed (watchdog now owns the child via an mpsc
channel) and exposed four distinct loader bugs in the UEFI
x86-64 boot path, all fixed — `qemu-smoke` and
`asm-verification` are green end-to-end for the first time. The
BIOS boot chain is now fully scaffolded: `zamak-test/build-images.sh`
assembles a real MBR + stage2 + FAT32-partition disk, and the
remaining blocker (`call_bios_int` trampoline returning AH=0x01)
is isolated to a single instruction and documented in M1-16.

### Added

- **GitHub Actions CI pipeline** — complete port of
  `.forgejo/workflows/ci.yml` to `.github/workflows/ci.yml`. All
  16 jobs (fmt, clippy, test × 3 host targets, freebsd, miri,
  deny, cross × 4 bootloader targets, size-gate, qemu-smoke,
  asm-verification, sbom) pass on the Ubuntu / macOS / FreeBSD
  runners.
- **`zamak-test` `--timeout <seconds>` flag** — replaces the
  advisory-only `BOOT_TIMEOUT` constant that never fired. A
  watchdog thread now owns the QEMU child process via an
  `mpsc::channel`-based shutdown contract; `recv_timeout`
  guarantees the kill fires on schedule regardless of whether
  the guest emits serial output. Wired into CI with
  `--timeout 60` on both qemu-smoke and asm-verification.
- **`zamak-test` robust OVMF discovery** — harness probes
  `OVMF_CODE.fd` / `OVMF_CODE_4M.fd` / `OVMF_CODE_4M.ms.fd`
  under `$OVMF_DIR` and copies `OVMF_VARS.fd` to a writable
  temp file, so the UEFI suites run unchanged on Ubuntu 22.04,
  Ubuntu 24.04, and Nix.
- **BIOS boot chain scaffolding (partial M1-16)** —
  `zamak-test/build-images.sh` now assembles a real
  BIOS-bootable disk (MBR stage1 at LBA 0 + `zamak-bios` stage2
  at LBA 1 + FAT32 partition at LBA 4096 containing
  `zamak.conf` + `kernel.elf`). `zamak-stage1` gained a missing
  `build.rs` (without it rust-lld ignored `linker.ld` and
  objcopy produced an empty binary), direct COM1 serial output
  for progress tracing, an INT 13h extension presence check,
  and a multi-chunk read loop so stage2 binaries > 64 KiB don't
  exceed the real-mode segment boundary. `zamak-bios` gained
  serial checkpoints through `kmain`, an MBR partition-table
  scan, and feature-gated SMP modules (trampoline asm needs a
  position-independent rewrite — tracked in M1-16). The chain
  boots cleanly through `_start → protected-mode → kmain entry
  → Disk::new → first BIOS-I/O call`, where it currently hangs
  on `AH=0x01` from the `call_bios_int` round-trip; see
  `TODO.md` M1-16 for the two proposed paths forward.

### Fixed

- **UEFI x86-64 boot smoke — four masked loader bugs** in
  `zamak-uefi` that the watchdog hang had hidden:
  - PML4 did not identity-map low physical memory, so the
    instruction after `Cr3::write` in `handoff::jump_to_kernel`
    page-faulted → triple fault.
  - `KERNEL_PATH` lookup read from `entry.options` but the
    config parser writes directly to `entry.kernel_path`.
  - Kernel/module path separators weren't translated from
    Limine-style `/` to UEFI's `\`.
  - `zamak-core/src/assets/font.psf` was a 307-byte ASCII
    placeholder, not a PSF1 binary — `PsfFont::parse` returned
    `None` and the `.unwrap()` panicked before kernel load.
    Replaced with a minimal valid PSF1 (4-byte header + 4096
    bytes of blank glyphs).
- **`cargo-deny` v0.15+ schema migration** — `deny.toml`
  rewritten: dropped `vulnerability`/`yanked`/`notice`/`unlicensed`/`copyleft`
  (removed per EmbarkStudios/cargo-deny#611), added
  `GPL-3.0-or-later` to the allow list so the workspace crates
  themselves stop being rejected, and added `license =
  "GPL-3.0-or-later"` + `version = "0.7.0"` pins on every
  `path` dep so path-only deps no longer count as wildcard
  dependencies.
- **Miri UB in `elf::apply_relocations` + `gfx::put_pixel`** —
  unaligned u64/u32 writes through `*mut u8`-derived pointers
  are UB under Miri's symbolic alignment check (hardware
  tolerates them on x86/ARM); switched to `write_unaligned`.
- **Clippy regressions after the GH Actions port** — clippy
  scope narrowed to host-runnable crates (freestanding crates
  pull `uefi-services`, whose `#[panic_handler]` collides with
  Linux std's `panic_impl` → E0152). Multiple lint fixes across
  `zamak-core` (doc-list overindent, collapsible-if,
  manual-is-multiple-of, unnecessary-map-or), `zamak-cli` (10
  lints from `derivable_impls` to `result_large_err`),
  `wallpaper`/`proptests` (identity ops, inconsistent hex digit
  grouping). Latent `bad_asm_style` errors (`.intel_syntax` /
  `.att_syntax` directives are redundant on current nightly)
  fixed in `zamak-stage1/src/mbr.rs` and
  `zamak-bios/src/entry.rs`.
- **`zamak-cli` global `--version` shadowed sbom's
  `--version` arg** — sbom's flag renamed to `--release-version`;
  the global parser now stops consuming `--version`/`--help`
  after seeing a sub-command positional.

### Changed

- **Style sweep** — one-shot `cargo fmt --all` across the
  workspace.
- **TODO.md** — post-v0.7.0 flips for REL-1..7, M4-7, M6-2
  (v0.7.0 tag published their artifacts); TEST-4 and TEST-5
  flipped to `[✓]` for the UEFI x86-64 path; M1-16 note
  updated with concrete next-step for the `call_bios_int`
  trampoline.

## [0.7.0] - 2026-04-21

First cut of the dual-mode host CLI (SFRS v1.0.0), multi-arch UEFI
paging for AArch64 + RISC-V 64, and the coverage / Miri / FreeBSD
compliance bundle. M0–M5 functionally complete; M6-1 blocked on
rustc upstream `loongarch64-unknown-uefi` support; M6-3 awaits
bare-metal perf validation.

### Added

- **FreeBSD CI leg** (POSIX-1 / POSIX-2) — new `freebsd` job in
  `.forgejo/workflows/ci.yml` using `vmactions/freebsd-vm@v1`
  (FreeBSD 14.2 guest) runs `cargo test -p zamak-cli`. The release
  workflow also gains a `cli-freebsd` job that produces a
  `zamak-freebsd-x86_64` binary as part of every tagged release.
  The full host-CLI portability claim now spans Linux x86-64, Linux
  AArch64, macOS AArch64, and FreeBSD x86-64.

### Fixed

- **CI / release workflows** — removed stale `cd Zamak && ...`
  prefixes inherited from the pre-reorg subdirectory layout; every
  step now runs at the repo root. `sbom` and release workflows also
  updated to call `zamak-cli sbom --release-version ...` (renamed
  from the global-shadowing `--version` alias).

- **TEST-1 line coverage target met** — `cargo llvm-cov` now reports
  **80.52% line / 87.04% function coverage** on `zamak-core`. Added
  58 new unit tests across seven previously-untested modules:
  `elf` (4), `font` (7), `gfx` (7), `iso9660` (9), `linux_boot` (14),
  `protocol` (5), `wallpaper::draw` (4), plus six `rng` tests
  covering `X86KaslrRng`, `TimerJitterRng`, and `align_up`/`align_down`.
- **TEST-2 Miri runs clean** — `cargo +nightly miri test -p
  zamak-core --lib` reports 158 passed / 0 failed. `spin_wait`
  timing test re-gated `#[cfg(all(target_arch = "x86_64",
  not(miri)))]` because Miri's rdtsc stub is constant.
- **Limine v10.x differential fuzz target** (TEST-6) — new
  `fuzz/fuzz_targets/config_parser_differential.rs` fuzz harness that
  runs `zamak_core::config::parse` and a hand-rolled Limine v10.x
  reference model side by side, asserting depth-1 entry name order,
  global `timeout`, and `default_entry` match on every input the
  reference accepts. Accompanying cross-check suite in
  `zamak-core/tests/limine_reference_model.rs` (11 golden cases).
- **Multi-arch `setup_paging` ported to AArch64 and RISC-V 64** (M4-1, M4-4):
  - `zamak_core::arch::riscv64::paging::PageTableBuilder` — Sv48 4-level builder with Svpbmt PBMT encoding (PMA for cached RAM, NC for framebuffer, IO for MMIO); 6 new tests
  - `zamak_core::arch::loongarch64::paging::PageTableBuilder` — 4-level PGDH builder with MAT-encoded cache policy and PLV/NX/G encoding; 5 new tests
  - `zamak-uefi::paging` — arch-dispatching `build(boot_services, kernel) -> u64` that returns the per-arch root-table physical address (PML4 on x86, L0 on AArch64, Sv48 root on RISC-V, PGDH on LoongArch)
  - `zamak-uefi::main` refactored: deleted `stub_entry` and all `#[cfg(target_arch = "x86_64")]` gates on the shared boot path. Single arch-neutral entry point now dispatches through `paging::build` and `handoff::jump_to_kernel`. `cargo check --target aarch64-unknown-uefi` and `riscv64gc-unknown-none-elf` both clean.
  - `Permissions::KERNEL_LOAD_AREA` — RWX coarse preset for kernel-image mapping parity across arches until per-PHDR splitting is implemented
- **SFRS dual-mode CLI** — `zamak-cli` now conforms to `SB-SFRS-STEELBORE-CLI v1.0.0`:
  - Global flags: `--json`, `--format <human|json|jsonl|yaml|csv|explore>`, `--fields`, `--dry-run`, `--verbose`, `--quiet`, `--color`, `--no-color`, `--yes`, `--force`, `--print0`
  - JSON envelope `{metadata:{tool,version,command,timestamp}, data:...}` via `OutputPolicy::emit`
  - Structured error envelope on stderr (`error.code/exit_code/message/hint/timestamp/command/docs_url/io_kind`) with stable `UPPER_SNAKE_CASE` codes
  - Expanded exit-code map: 0/1/2/3/4/5 per SFRS §3.2
  - `zamak schema [<command>]` — JSON Schema Draft 2020-12 introspection
  - `zamak describe` — capability manifest (SFRS §6.2)
  - `zamak completions <bash|zsh|fish|nushell>` — shell completion scripts
  - Agent-env detection (`AI_AGENT` / `AGENT` / `CI` / `TERM=dumb`) forces json + no-color + no-TUI + no-prompts
  - Color policy: NO_COLOR / FORCE_COLOR / CLICOLOR / `--color` precedence per §4.4
  - Input hardening: path canonicalization + allow-list, control-byte rejection, numeric bounds, destructive-op confirmation
  - `--dry-run` on `install` / `enroll-config`; idempotent re-runs short-circuit
  - PowerShell-friendly errors (single-line JSON) per §8.3; Windows startup sets console CP 65001
  - UTF-8 without BOM throughout; ANSI escapes suppressed in every machine mode
- `--format explore` TUI (feature-gated behind Cargo feature `tui`, `ratatui` 0.29 + `crossterm` 0.28) with CUA + Vim keybindings, `/` filter, `s` sort, Enter detail, `e` export; graceful JSON fallback when feature disabled, under `AI_AGENT=1`, or on non-TTY
- Context files at `Zamak/` repo root: `CLAUDE.md`, `AGENTS.md`, `SKILL.md`, `CONTRIBUTING.md`
- Integration test matrix in `zamak-cli/tests/sfrs_conformance.rs` covering exit codes, TTY/non-TTY, `AI_AGENT=1`, `--dry-run`, control-char rejection, UTF-8 encoding, ANSI suppression, JSON Schema shape, `describe` enumeration
- Crate rename: `libzamak` -> `zamak-core`, `zamak-loader` -> `zamak-uefi` (PRD §4.1)
- Converted all `.asm` files to `global_asm!` in Rust source files (Steelbore §3.2)
- Added `rustfmt.toml` project formatting configuration
- Added `deny.toml` for `cargo-deny` license and CVE auditing
- Added `CHANGELOG.md` (Keep a Changelog format)
- Created `zamak-proto` standalone protocol types crate
- Created `zamak-theme` crate skeleton for theme file parsing
- Created `zamak-cli` crate skeleton for host tooling
- Created `zamak-decompressor` crate — BIOS stage2 gzip decompressor using `miniz_oxide`
- Added `zamak-core::addr` module with newtype wrappers: `PhysAddr`, `PageAlignedPhysAddr`, `VirtAddr`, `TrampolineAddr`, `Cr3Value`, `MairValue`, `SatpValue` (PRD §3.9.3)
- Added structured `// SAFETY:` contracts to all `unsafe` blocks in `zamak-bios` (PRD §3.9.6)
- Added `const_assert!` compile-time layout verifications for all `#[repr(C)]` structs: `BiosRegs`, `DiskAddressPacket`, `E820Entry`, `MadtEntryHeader`, protocol types (PRD §3.9.7)
- Used `.pushsection`/`.popsection` for MBR `global_asm!` block (PRD §3.9.8)
- Added `zamak-core::arch::x86` safe wrapper module (inb, outb, pause, hlt, rdtsc, spin_wait) to eliminate direct `asm!` in caller code (PRD §3.9.2)
- Created `zamak-stage1` crate — standalone 512-byte MBR boot sector binary (PRD §4.1)
- Implemented RDRAND/RDSEED KASLR fallback chain in `zamak-core::rng::X86KaslrRng` (FR-MM-003)
- Added `zamak-core::blake2b` — pure `no_std` BLAKE2B hash (RFC 7693) for config verification
- Added `zamak-core::iso9660` — read-only ISO 9660 filesystem driver for CD boot
- Added `options(nomem, nostack, preserves_flags)` to all `asm!` blocks (PRD §3.9.5)
- Added `#[cfg(miri)]` stubs for all `asm!` blocks in `arch.rs` and `rng.rs` (PRD §3.9.10)
- Added §3.9.1 justification comments to `global_asm!` blocks exceeding 20 instructions (entry.rs, trampoline.rs, mbr.rs)

- Full HHDM mapping covering all physical memory reported by E820/UEFI memory map (§FR-MM-002)
- Added `zamak-core::linux_boot` — x86 bzImage parser, setup header, BootParams zero page with E820 support (FR-PROTO-002)
- Added `Makefile.uefi` — builds `BOOTX64.EFI`, creates FAT32 ESP image, QEMU boot target
- Created `zamak-test` crate — QEMU integration test harness with serial capture and ISA debug exit
- Config parser: `${NAME}=value` macro definitions and `${ARCH}`, `${FW_TYPE}`, `${BOOT_DRIVE}` built-ins (FR-CFG-002)
- Added `zamak-core::uri` — Limine URI path parser with `#hash` BLAKE2B verification: `boot()`, `hdd(d:p)`, `odd(d:p)`, `guid(uuid)`, `fslabel(label)`, `tftp(ip)` (FR-CFG-003)
- Added `zamak-core::config_discovery` — SMBIOS Type 11 OEM String extraction and config search order (FR-CFG-004, FR-CFG-005)
- Implemented `zamak-theme` TOML parser with `Theme::from_toml()` — parses all five token groups with hex color values
- Created `zamak-macros` proc-macro crate with `#[zamak_unsafe]` attribute for assembly safety boundary marking (PRD §3.5, §3.9)
- Applied `#[zamak_unsafe]` to all `asm!` wrapper functions in `zamak-core::arch::x86`
- Implemented `zamak install` CLI command (FR-CLI-001): MBR validation, stage2 LBA/size patching at offsets 440/444, write to target device/image
- Implemented `zamak enroll-config` BLAKE2B-256 hash computation with standalone host implementation (FR-CLI-002, partial)
- Config hash enrollment: `enroll_config_hash()` and `verify_config_hash()` with constant-time comparison; editor auto-disabled when hash enrolled (FR-CFG-006)
- `theme` and `theme_variant` global config options parsed from config file (§7.1)
- `ThemeVariant` enum with `Dark`/`Light` support and `Theme::with_variant()` method
- `MenuState::editor_locked` field and `[CONFIG HASH ENROLLED]` lock indicator in TUI (FR-CFG-006)

- Config parser: Limine v10.x byte-identical semantics — `/`-delimited entries, `//`/`///` sub-entry nesting, `+` expand prefix, `:` option delimiter, `comment` local option (FR-CFG-001)
- Config parser: `quiet`, `serial`, `serial_baudrate`, `default_entry`, `verbose`, `hash_mismatch_panic` global options
- Added `zamak-core::multiboot` — Multiboot 1 protocol: header scanner, info structure builder, module/mmap types (FR-PROTO-003)
- Added `zamak-core::pe` — PE/COFF loader: PE32+ parser, section loading, DIR64/HIGHLOW/HIGH/LOW base relocation processing (§4.3)
- Added `zamak-core::multiboot2` — Multiboot 2 protocol: tag-based boot info builder, header parser with tag walking, memory map/module/framebuffer/ACPI tags (FR-PROTO-003)
- Added `zamak-core::pmm` — Physical Memory Manager: E820/UEFI memory map normalization, overlap resolution, page-alignment sanitization, top-down allocation, region marking (FR-MM-001)
- Added `zamak-core::vmm` — Virtual Memory Manager planning: `VmmPlan`, kernel PHDR / HHDM / framebuffer mappings, x86-64 PAT flag encoding, huge/giga page detection (FR-MM-002)
- Added `zamak-core::chainload` — EFI and BIOS chainload protocols with firmware-compatibility filter for menu entry hiding (FR-PROTO-004)
- Added `zamak-core::theme_loader` — `FileReader`-based theme resolution honoring `config.theme_path` with standard path fallback (FR-CFG-007)
- Added `zamak-core::tui::flatten_entries()` — hierarchical menu tree walker with `+` expand prefix + runtime expand/collapse state (M5-9 / FR-UI-001)
- KASLR: `kaslr_base()` with 1 GiB alignment per FR-MM-003 and `TimerJitterRng` for non-x86 timer-jitter entropy
- `zamak-theme::ThemeVariant` and `Theme::with_variant()` — dark/light variant switching; `Theme` wired through `draw_menu()` replacing hardcoded colors
- `zamak-cli sbom` — SPDX 2.3 JSON document generator with SHA-256 artifact checksums (FR-CLI-003)
- `zamak-cli` ISO 8601 timestamps via `iso8601_now()` / `log_info()` / `log_warn()` helpers (§3.7)
- Added `zamak-core::enrolled_hash` — `EnrolledHashSlot` with 16-byte signature + 32-byte BLAKE2B-256 slot; `find_slot()`, `patch_hash()`, `read_hash_at()` (FR-CFG-006)
- `zamak-uefi` embeds a `ZAMAK_ENROLLED_HASH` static so `zamak enroll-config` can locate and patch the slot in built EFI binaries
- `zamak enroll-config` now performs real EFI binary patching: signature scan, in-place hash write, file rewrite (FR-CLI-002)
- Added `zamak-core::arch::aarch64::mmu` — TTBR0/TTBR1/MAIR/TCR writes, `tlbi_all()`, `STANDARD_MAIR` constant (§3.2.1)
- Added `zamak-core::arch::aarch64::psci` — SMC/HVC call wrapper, `cpu_on()`, PSCI function IDs and return codes (§3.2.1)
- Added `zamak-core::arch::riscv64::satp` — `encode()`, `write_satp()` with sfence.vma, Sv39/48/57 mode constants (§3.2.1)
- Added `zamak-core::arch::riscv64::sbi` — `ecall` wrapper, HSM `hart_start`/`hart_stop`/`hart_status` (§3.2.1)
- Forgejo CI workflow (`.forgejo/workflows/ci.yml`) covering fmt, clippy, test, miri, deny, cross-compile (x86_64/aarch64/riscv64/loongarch64), QEMU smoke, SBOM
- Forgejo release workflow (`.forgejo/workflows/release.yml`) building all 5 architectures' `BOOT*.EFI`, BIOS stage3, CLI binaries, SHA256SUMS, and SPDX SBOM
- Workspace-wide `cargo fmt` pass applied (rustfmt 1.94 with project `rustfmt.toml`)
- Clippy-clean `cargo clippy --lib` across zamak-core / zamak-theme / zamak-cli / zamak-proto / zamak-macros (zero warnings)
- Per-asm-wrapper tests (12 new tests) — x86 `pause`/`rdtsc`/`spin_wait` exercised on host; AArch64 and RISC-V stubs verified to be side-effect-free and return NOT_SUPPORTED (TEST-4 / §3.9.9)
- `zamak-core/tests/proptests.rs` — 7 property tests covering PMM page-alignment, allocation disjointness, KASLR 1 GiB alignment, and config-parser panic safety (TEST-7 / §8.1)
- `.cargo/config.toml` with `MIRIFLAGS` + `cargo miri-test` alias for the Miri test suite (TEST-2 scaffolding)
- `zamak-core::enrolled_hash::PatchError` enum replaces the `Result<_, ()>` return type with a named error
- Added `zamak-core::arch::loongarch64` — CSR read/write, DMW encoding, IOCSR for SMP bring-up; stubs on non-LoongArch hosts (M6-1 / §3.2.1)
- Added `zamak-core::wallpaper` — BMP (24/32 bpp) parser with tiled / centered / stretched styles, `draw_menu_with_wallpaper()` integration (M3-9 / FR-UI-001)
- CI POSIX matrix (`test` job): Linux x86-64, Linux AArch64, macOS AArch64 (POSIX-2 / §6.4)
- CI: SPDX validation via `pyspdxtools` on the generated SBOM (LIC-3)
- CI: dedicated `asm-verification` job that runs host-safe arch wrapper tests + QEMU suite (CI-10)
- CI: functional `size-gate` job enforcing ≤120% of Limine v10.x baselines for every release artifact (CI-8, §6.1)
- Added `Zamak/fuzz/` — `cargo fuzz` harnesses for `config::parse`, `uri::parse_uri`, `multiboot::find_header`, `wallpaper::parse` (TEST-6 / §8.1)
- `zamak-core::addr::PhysAddr` / `VirtAddr` — explicit `checked_add` / `checked_sub` / `page_floor` / `page_ceil` / `wrapping_sub`; no `Add<u64>` by design, so every step is checked (RG-3 / §3.5)
- `zamak-cli` refactored to `Result<(), CliError>` — `main` is the only place that calls `process::exit`, and it stamps errors with an ISO 8601 `[WARN]` line (RG-2 / §3.5)
- `cargo doc --no-deps` is now clean across zamak-core / zamak-theme / zamak-proto / zamak-cli / zamak-macros (RG-1, M6-4 / §6.5)
- Added `zamak-uefi::handoff::jump_to_kernel` — arch-agnostic kernel hand-off dispatch with native implementations for x86-64 (CR3), AArch64 (MAIR/TCR/TTBR1/TLBI), RISC-V 64 (SATP Sv48 + sfence.vma), and LoongArch64 (PGDH + STLB flush) (M4-1, M4-4, M6-1)
- `zamak-uefi` now builds cleanly for all four target architectures: `x86_64-unknown-uefi`, `aarch64-unknown-uefi`, `riscv64gc-unknown-none-elf`, `loongarch64-unknown-none`
- Non-x86-64 UEFI targets get a stub `#[entry]` that returns `Status::UNSUPPORTED` with an informative log, so release artifacts build even while the full paging paths are ported
- Added `zamak-test-kernel` crate — a minimal Limine-Protocol kernel (~2.6 KiB ELF) that emits `ZAMAK` + `LIMINE_PROTOCOL_OK` to COM1 and exits via the QEMU ISA debug-exit device, enabling real end-to-end boot tests (M1-16 / M2-12)
- `zamak-test` now supports `--suite <name>` with three suites: `boot-smoke` (BIOS + UEFI), `asm-verification`, `linux-bzimage`; missing artefacts are skipped rather than failing
- `zamak-test/build-images.sh` — driver script that builds the test kernel, bootloader, and a 64 MiB FAT32 ESP with `BOOTX64.EFI` + `kernel.elf`
- CI `qemu-smoke` job now builds the test kernel + ESP image via the script and runs the `boot-smoke` suite under QEMU + OVMF (TEST-5)
- Added `zamak-asm-verify-kernel` — a second Limine-Protocol kernel in the `zamak-test-kernel` crate that exercises every host-safe asm wrapper (pause, rdtsc monotonicity, inb/outb round-trip) and emits `ASM_VERIFY_OK` on success (TEST-4 / §3.9.9)
- CI `asm-verification` job rebuilt end-to-end: builds the verify kernel, packages it into an ESP image, boots under QEMU + OVMF, and fails the run if the marker is absent (CI-10 flipped from partial to done)
- Added `zamak-core::arch::aarch64::paging::PageTableBuilder` — 4 KiB-granule L0-L3 page table constructor that consumes a `VmmPlan` and applies MAIR-indexed cache policies, AP/UXN/PXN permission bits, and inner-shareable attributes (M4-1 / §FR-MM-002)
- TUI editor F10-to-boot accelerator + pluggable `EditorValidator` callback producing `EditorDiagnostic` (Ok / Warning / Error); F10 is error-gated, Esc cancels and clears the buffer, Backspace shrinks + revalidates (M3-10 / §FR-UI-002)
- `MenuState::handle_editor_key` — 6 new tests cover F10 commit/refuse, Esc reset, Backspace, locked-editor guard, not-editing guard
- `zamak-uefi/src/main.rs` — `checked_add`/`checked_sub`/`div_ceil` across `load_kernel_segments`, `setup_paging` kernel page mapping, and HHDM construction (RG-3 / §3.5)
- `zamak-bios/src/main.rs` — `checked_add` for LAPIC ID register offset; previously an unchecked `lapic_addr + 0x20` (RG-3)
- `arch::loongarch64::csr_read` / `csr_write` upgraded to `const CSR: u32` generics because LoongArch `csrrd`/`csrwr` encode the register number in the instruction word

### Changed

- `zamak_theme::ThemeVariant::from_str` renamed to `parse` to avoid colliding with `std::str::FromStr::from_str` (which has a fallible signature)
- `zamak-core::vmm`: replaced custom `IsMultipleOf` trait with `u64::is_multiple_of` / `div_ceil` from the standard library

### Fixed

- Pre-existing `uefi` 0.24 API mismatch in `setup_paging` — `memory_map()` returns `MemoryMap<'_>` directly, not a `(key, iter)` tuple; this was blocking every `zamak-uefi` build
- `zamak-core::rng::detect_rng_support()` — work around LLVM's reserved `rbx` register by saving/restoring via `push`/`pop` around CPUID (was blocking all `zamak-core` tests)
- `zamak-core::pe::PeImage` / `SectionInfo` now derive `PartialEq`, `Eq` so tests can compare results
- Fixed packed struct field access in multiboot/multiboot2/config_discovery tests by copying to locals before asserting
- Chainload `parse()` signature changed from `Fn(&str) -> Option<&str>` to `FnMut(&str) -> Option<String>` to avoid lifetime issues with temporary `BTreeMap` values
- `protocol::scan_requests()` now explicitly types `i` as `usize` (was ambiguous numeric type)

### Changed

- Default theme primary accent changed from Steel Blue (#4B7EB0) to Material Design Blue 800 (#1565C0) per §3.1.2
- Default theme error color changed to Material Design Red 700 (#D32F2F)
- Config parser M3-1 upgraded from partial to full Limine-compatible implementation

- Removed NASM dependency from `build.rs` — all assembly now via `global_asm!`

## [0.6.9] - 2026-04-01

### Added

- BIOS boot path: Stage 1 MBR, Stage 2 entry, Stage 3 full bootloader
- UEFI boot path: x86-64 UEFI application with GOP, memory map, SMP
- Limine Protocol request scanning and response fulfillment
- FAT32 and ext2 filesystem drivers
- ELF64 loader with PIE and basic KASLR support
- Boot menu TUI with PSF2 font rendering
- SMP AP bring-up via MADT parsing and LAPIC IPI
- ACPI RSDP discovery
- VBE graphical framebuffer initialization (BIOS)
- Configuration file parser (`zamak.conf`)
