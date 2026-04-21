<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 Mohamed Hammad
-->

# ZAMAK Implementation Status — TODO

**Reference:** `ZAMAK_Bootloader_PRD_v1.3.docx.md` (SB-PRD-ZAMAK-001 v1.3.0)
**Generated:** 2026-04-21 (updated)
**Current milestone target:** M6 — LoongArch64 + Polish (due 2027-06-01). M1–M5 functionally complete; remaining gates are release-tagging, CI artifact confirmation, and rustc upstream LoongArch-UEFI support.

---

## Legend

- `[✓]` Done
- `[ ]` Not started
- `[~]` Partially implemented / non-compliant

---

## M0 — Architecture & Scaffolding (due 2026-03-15) — PAST DUE

| # | Status | Task |
|---|--------|------|
| M0-1 | `[✓]` | Cargo workspace root (`Cargo.toml`) with workspace members |
| M0-2 | `[✓]` | `zamak-bios` crate (BIOS stage3 entry + core boot logic) |
| M0-3 | `[✓]` | `zamak-uefi` crate (UEFI x86-64 entry) |
| M0-4 | `[✓]` | `zamak-core` shared library |
| M0-5 | `[✓]` | Rename `libzamak` -> `zamak-core` to match PRD §4.1 crate topology |
| M0-6 | `[✓]` | Rename `zamak-loader` -> `zamak-uefi` to match PRD §4.1 crate topology |
| M0-7 | `[✓]` | Create `zamak-proto` as a standalone `#![no_std]` crate (protocol types extracted from `zamak-core::protocol`) |
| M0-8 | `[✓]` | Create `zamak-stage1` as a proper Cargo crate (512-byte MBR, pure `global_asm!`, own linker script + target JSON) |
| M0-9 | `[✓]` | Create `zamak-decompressor` crate (stage2 decompressor using `miniz_oxide`, loaded at 0x70000) |
| M0-10 | `[✓]` | Create `zamak-cli` crate (host-side tool skeleton with subcommand stubs) |
| M0-11 | `[✓]` | Create `zamak-theme` crate (theme parser with Steelbore palette defaults) |
| M0-12 | `[✓]` | Create `zamak-test` crate (QEMU integration test harness with serial capture) |
| M0-13 | `[✓]` | Add `rustfmt.toml` project configuration |
| M0-14 | `[✓]` | Add `deny.toml` for `cargo-deny` (license + CVE checks) |
| M0-15 | `[✓]` | Configure CI/CD pipeline — `Zamak/.forgejo/workflows/{ci,release}.yml` with fmt/clippy/test/miri/deny/cross/qemu/sbom jobs |
| M0-16 | `[✓]` | Add `CHANGELOG.md` (Keep a Changelog format, ISO 8601 dates) |
| M0-17 | `[✓]` | SPDX headers on all source files |

---

## Steelbore Standard Compliance — §3

### §3.2 Inline Assembly — No Separate `.asm` Files (RESOLVED)

| # | Status | Task |
|---|--------|------|
| ASM-1 | `[✓]` | Convert `entry.asm` -> `global_asm!` in `zamak-bios/src/entry.rs` |
| ASM-2 | `[✓]` | Convert `mbr.asm` -> `global_asm!` in `zamak-bios/src/mbr.rs` |
| ASM-3 | `[✓]` | Convert `trampoline.asm` -> `global_asm!` in `zamak-bios/src/trampoline.rs` |
| ASM-4 | `[✓]` | Remove NASM dependency from `build.rs` — assembly now compiled via `global_asm!` |

### §3.9 Assembly Memory Safety Framework

| # | Status | Task |
|---|--------|------|
| ASM-5 | `[✓]` | Implement `#[zamak_unsafe]` proc-macro attribute for boundary-marking all `unsafe` blocks |
| ASM-6 | `[✓]` | Define newtype wrappers: `PageAlignedPhysAddr`, `PhysAddr`, `VirtAddr`, `TrampolineAddr`, `Cr3Value`, `MairValue`, `SatpValue` in `zamak-core::addr` (§3.9.3) |
| ASM-7 | `[✓]` | Wrap `asm!` in safe APIs: `zamak-core::arch::x86` module (inb, outb, pause, hlt, rdtsc, spin_wait); used in input.rs, main.rs, smp.rs (§3.9.2) |
| ASM-8 | `[✓]` | Add structured `// SAFETY:` contracts (Preconditions / Postconditions / Clobbers / Worst-case) to `zamak-bios` unsafe blocks (§3.9.6) |
| ASM-9 | `[✓]` | Add `const_assert!` field-offset and size validations for `#[repr(C)]` structs: `BiosRegs`, `DiskAddressPacket`, `E820Entry`, `MadtEntryHeader`, protocol types (§3.9.7) |
| ASM-10 | `[✓]` | Use `.pushsection`/`.popsection` in `global_asm!` for position-sensitive code (MBR, SMP trampoline) (§3.9.8) |
| ASM-11 | `[✓]` | Add `#[cfg(miri)]` stubs for all `asm!` blocks to enable Miri coverage (§3.9.10) |
| ASM-12 | `[✓]` | Enforce `asm!` block ≤ 20 instructions limit; split or justify any larger blocks (§3.9.1) |
| ASM-13 | `[✓]` | Use symbolic operands (`val = in(reg)`) in all `asm!` blocks: arch::x86, rng.rs, decompressor (§3.9.4) |
| ASM-14 | `[✓]` | Add most-restrictive `options(nostack, preserves_flags, nomem)` to all `asm!` blocks (§3.9.5) |

### §3.3 / §3.4 License & SPDX

| # | Status | Task |
|---|--------|------|
| LIC-1 | `[✓]` | Ensure SPDX header on every source file |
| LIC-2 | `[✓]` | SPDX SBOM generation in `zamak-cli sbom` + CI `sbom` job + release workflow upload |
| LIC-3 | `[✓]` | Validate SBOM with `spdx-tools` — `sbom` job now runs `pyspdxtools -i` on generated SBOM |

### §3.5 Rust Guidelines

| # | Status | Task |
|---|--------|------|
| RG-1 | `[✓]` | Doc-comments on public APIs — `# Safety` on all unsafe fns; `cargo doc --no-deps` clean (M6-4); `# Examples` on `addr` / `blake2b` headliners |
| RG-2 | `[✓]` | `zamak-cli` uses `Result<(), CliError>` throughout; `process::exit` only in `main` to translate errors to exit codes |
| RG-3 | `[✓]` | Address arithmetic: `checked_add` / `checked_sub` / `div_ceil` throughout `zamak-bios/src/main.rs` (LAPIC ID reg) and `zamak-uefi/src/main.rs` (ELF segment load, kernel page mapping, HHDM construction); plus address newtypes with `checked_*` helpers |
| RG-4 | `[✓]` | `cargo clippy --lib` — zero warnings across zamak-core/theme/cli/proto/macros |
| RG-5 | `[✓]` | `cargo fmt` — all workspace code formatted per project `rustfmt.toml` |

### §3.6 POSIX Compatibility

| # | Status | Task |
|---|--------|------|
| POSIX-1 | `[✓]` | `zamak-cli` — uses only `std::fs`/`std::io`; CI exercises Linux x86-64, Linux AArch64, macOS AArch64, and FreeBSD 14.x (new `freebsd` job in `.forgejo/workflows/ci.yml` via `vmactions/freebsd-vm@v1`) |
| POSIX-2 | `[✓]` | CI matrix — Linux x86-64, Linux AArch64, macOS AArch64, and FreeBSD 14.x all run `cargo test -p zamak-cli`; `release.yml` also produces a `zamak-freebsd-x86_64` release artifact |

---

## M1 — BIOS Boot x86-64 (due 2026-06-01)

| # | Status | Task |
|---|--------|------|
| M1-1 | `[✓]` | BIOS E820 memory map enumeration (`zamak-bios::mmap`) |
| M1-2 | `[✓]` | FAT32 filesystem driver (`zamak-bios::fat32`) |
| M1-3 | `[✓]` | ELF loader with PIE + basic KASLR (`libzamak::elf`) |
| M1-4 | `[✓]` | Limine Protocol request scanning + response fulfillment |
| M1-5 | `[✓]` | VBE graphical framebuffer initialization (`zamak-bios::vbe`) |
| M1-6 | `[✓]` | Boot menu TUI + input handling (`libzamak::tui`) |
| M1-7 | `[✓]` | PSF2 font rendering (`libzamak::font`, `libzamak::gfx`) |
| M1-8 | `[✓]` | SMP AP bring-up via MADT + LAPIC IPI (`zamak-bios::smp`) |
| M1-9 | `[✓]` | ACPI RSDP discovery (BIOS scan in `main.rs`) |
| M1-10 | `[✓]` | BIOS stage1 MBR (`global_asm!` in `zamak-bios/src/mbr.rs`) |
| M1-11 | `[✓]` | Real→protected→long mode transition (`global_asm!` in `zamak-bios/src/entry.rs`, `trampoline.rs`) |
| M1-12 | `[✓]` | Implement `zamak-decompressor` (stage2 decompressor using `miniz_oxide`) |
| M1-13 | `[✓]` | KASLR: RDSEED → RDRAND → RDTSC fallback chain in `X86KaslrRng` with CPUID detection (§FR-MM-003) |
| M1-14 | `[✓]` | BLAKE2B hash implementation in `zamak-core::blake2b` (pure `no_std`, RFC 7693) for `#hash` URI suffix (§FR-CFG-003) |
| M1-15 | `[✓]` | ISO 9660 filesystem driver (`zamak-core::iso9660`) — read-only, supports path traversal, ECMA-119 |
| M1-16 | `[~]` | End-to-end BIOS Limine-Protocol kernel boot under QEMU — `zamak-test-kernel` (minimal Limine-Protocol kernel) builds; `zamak-test --suite boot-smoke` runs BIOS + UEFI cases; CI `qemu-smoke` job wires build-images.sh → QEMU → serial capture |

---

## M2 — UEFI Boot x86-64 (due 2026-08-01)

| # | Status | Task |
|---|--------|------|
| M2-1 | `[✓]` | UEFI loader entry point (`zamak-loader`) |
| M2-2 | `[✓]` | GOP framebuffer initialization |
| M2-3 | `[✓]` | UEFI memory map enumeration and Limine type mapping |
| M2-4 | `[✓]` | UEFI RSDP/ACPI config table lookup |
| M2-5 | `[✓]` | UEFI SMBIOS config table lookup |
| M2-6 | `[✓]` | UEFI SMP via `MpServices` protocol |
| M2-7 | `[✓]` | UEFI RNG for KASLR |
| M2-8 | `[✓]` | Module loading from UEFI filesystem |
| M2-9 | `[✓]` | VMM / HHDM mapping — full HHDM covering all physical memory via E820/UEFI memory map (§FR-MM-002) |
| M2-10 | `[✓]` | `ExitBootServices()` retry logic — handled by `uefi` crate v0.24 internally (§6.2) |
| M2-11 | `[✓]` | Linux Boot Protocol support — x86 bzImage setup header parsing, BootParams zero page, E820 population (§FR-PROTO-002) |
| M2-12 | `[~]` | End-to-end Linux bzImage boot under QEMU UEFI — `linux-bzimage` suite + `ZAMAK_LINUX_ESP` env in `zamak-test`; awaits real bzImage + UEFI initrd in CI |
| M2-13 | `[✓]` | Build and produce `BOOTX64.EFI` release artifact (`Makefile.uefi` with ESP image + QEMU target) |

---

## M3 — Config + Menu + Theme (due 2026-10-01)

| # | Status | Task |
|---|--------|------|
| M3-1 | `[✓]` | Config parser — Limine-compatible with `/`-delimited entries, sub-entries, `+` expand, `:` options, macros |
| M3-2 | `[✓]` | FR-CFG-002: Macro system (`${NAME}=value`, built-in `${ARCH}`, `${FW_TYPE}`, `${BOOT_DRIVE}`) |
| M3-3 | `[✓]` | FR-CFG-003: URI path resolution (`boot()`, `hdd(d:p)`, `odd(d:p)`, `guid(uuid)`, `fslabel(label)`, `tftp(ip)`) |
| M3-4 | `[✓]` | FR-CFG-003: `#hash` suffix BLAKE2B verification in URI resolver |
| M3-5 | `[✓]` | FR-CFG-004: SMBIOS Type 11 OEM String config injection (prefix `limine:config:`) |
| M3-6 | `[✓]` | FR-CFG-005: Full config search order — SMBIOS → UEFI app dir → standard paths |
| M3-7 | `[✓]` | FR-CFG-006: Config hash enrollment (`zamak enroll-config`); hash-lock disables editor |
| M3-8 | `[✓]` | FR-CFG-007: Load `zamak-theme.toml` from same partition as config — `theme_loader::resolve()` with `FileReader` trait, standard path search, default fallback |
| M3-9 | `[✓]` | Boot menu — entry selection, timeout, hierarchical tree with collapse/expand, BMP wallpaper (tiled / centered / stretched) via `draw_menu_with_wallpaper` (§FR-UI-001) |
| M3-10 | `[✓]` | Config editor — `MenuState::handle_editor_key` with F10-to-boot accelerator, `EditorValidator` callback producing `EditorDiagnostic` (Ok/Warning/Error), Esc-cancels, Backspace, error-gating on F10; 6 editor tests (§FR-UI-002) |
| M3-11 | `[✓]` | Implement `zamak-theme` crate — TOML parser, token groups (`surface`, `accent`, `palette`, `editor`, `branding`) |
| M3-12 | `[✓]` | Wire theme tokens into all TUI draw calls — `draw_menu` now accepts `&Theme`; all colors resolved through theme tokens |
| M3-13 | `[✓]` | Built-in default theme using Material Design Blue 800 primary (§3.1.2) |
| M3-14 | `[✓]` | `theme` and `theme_variant` global config options (§7.1) |
| M3-15 | `[✓]` | Config parser: byte-identical semantics to Limine v10.x (`limine.conf` format, not `zamak.conf`) (§FR-CFG-001) |

---

## M4 — Multi-Architecture (due 2027-01-15)

| # | Status | Task |
|---|--------|------|
| M4-1 | `[✓]` | AArch64 UEFI boot path — `paging::aarch64::build` wires `arch::aarch64::paging::PageTableBuilder` (L0–L3 4 KiB granule, MAIR-indexed cache policy, AP/UXN/PXN encoding) into `zamak-uefi::main`; `cargo check --target aarch64-unknown-uefi` is clean; 5 paging tests + full shared boot path |
| M4-2 | `[✓]` | AArch64 arch module: `arch::aarch64::mmu` — TTBR0/TTBR1 / MAIR_EL1 / TCR_EL1 / TLBI, STANDARD_MAIR constant (§3.2.1) |
| M4-3 | `[✓]` | AArch64 arch module: `arch::aarch64::psci` — SMC/HVC call wrapper, `cpu_on()` for SMP bring-up (§3.2.1) |
| M4-4 | `[✓]` | RISC-V 64 UEFI boot path — `paging::riscv64::build` wires `arch::riscv64::paging::PageTableBuilder` (Sv48 4-level, Svpbmt PBMT for device / framebuffer); `handoff::jump_to_kernel` writes SATP + sfence.vma; `cargo check --target riscv64gc-unknown-none-elf` is clean; 6 new paging tests |
| M4-5 | `[✓]` | RISC-V 64 arch module: `arch::riscv64::satp` — satp encode, csrw + sfence.vma, Sv39/48/57 modes (§3.2.1) |
| M4-6 | `[✓]` | RISC-V 64 arch module: `arch::riscv64::sbi` — ecall wrapper, HSM `hart_start`/`hart_stop`/`hart_status` (§3.2.1) |
| M4-7 | `[~]` | Build `BOOTAA64.EFI`, `BOOTRISCV64.EFI` — `release.yml` matrix includes both; first tagged release will publish |
| M4-8 | `[✓]` | RISC-V 64 and AArch64 in CI `cross` matrix job (`.forgejo/workflows/ci.yml`) |

---

## M5 — Feature Parity (due 2027-04-01)

| # | Status | Task |
|---|--------|------|
| M5-1 | `[✓]` | Multiboot 1 protocol (`zamak-core::multiboot`) — header scan, info struct builder, module/mmap types (§FR-PROTO-003) |
| M5-2 | `[✓]` | Multiboot 2 protocol — tag-based info builder, header parser, mmap/module/framebuffer/ACPI tags (§FR-PROTO-003) |
| M5-3 | `[✓]` | Chainloading — EFI applications: `chainload::ChainloadTarget::Efi`, path/image_path parsing, firmware compatibility filter (§FR-PROTO-004) |
| M5-4 | `[✓]` | Chainloading — BIOS boot sectors: `chainload::ChainloadTarget::Bios`, drive/partition/MBR-ID/GPT-GUID parsing, incompatible-entry hiding (§FR-PROTO-004) |
| M5-5 | `[✓]` | PE/COFF loader (`zamak-core::pe`) — PE32+ parser, section loader, base relocation processing (§4.3) |
| M5-6 | `[✓]` | Full VMM with HHDM, kernel PHDRs, framebuffer write-combining — `vmm::VmmPlan`, x86 PAT flag encoding, huge/giga page detection (§FR-MM-002) |
| M5-7 | `[✓]` | Full PMM with overlap resolution, page-alignment sanitization, top-down allocation (§FR-MM-001) |
| M5-8 | `[✓]` | KASLR: 1 GiB alignment via `kaslr_base()`; RDSEED/RDRAND/RDTSC chain + `TimerJitterRng` fallback for non-x86 (§FR-MM-003) |
| M5-9 | `[✓]` | Boot menu: hierarchical directory expansion/collapse — `flatten_entries()` walks tree honoring `expanded` state and `+` prefix (§FR-UI-001) |

---

## M6 — LoongArch64 + Polish (due 2027-06-01)

| # | Status | Task |
|---|--------|------|
| M6-1 | `[~]` | LoongArch64 UEFI boot path — `arch::loongarch64::paging::PageTableBuilder` (4-level PGDH, MAT-encoded cache policy, PLV/NX encoding) + `paging::loongarch64::build` + `handoff::jump_to_kernel` (CRMD clear IE, PGDH, STLB flush) all implemented and compile for `loongarch64-unknown-none`; 5 new paging tests pass. Full `cargo check --target loongarch64-unknown-uefi` blocked on rustc upstream — the target does not yet exist (`uefi-services` uses the `efiapi` ABI which is unsupported on `loongarch64-unknown-none`) |
| M6-2 | `[~]` | `BOOTLOONGARCH64.EFI` — `release.yml` matrix entry for `loongarch64-unknown-none`; first tagged release will publish |
| M6-3 | `[~]` | Performance tuning — LTO + `codegen-units = 1` + `panic = abort` in release profile; CI `size-gate` enforces ≤120% size target; cold-boot timing baseline needs real hardware to validate |
| M6-4 | `[✓]` | Full rustdoc — zero warnings on `cargo doc --no-deps` for zamak-core / theme / proto / cli / macros |

---

## Host CLI (`zamak-cli`) — §5.5

| # | Status | Task |
|---|--------|------|
| CLI-1 | `[✓]` | Create `zamak-cli` crate (skeleton with subcommand stubs) |
| CLI-2 | `[✓]` | FR-CLI-001: `zamak install` — write stage1 MBR sector, record stage2 location |
| CLI-3 | `[✓]` | FR-CLI-002: `zamak enroll-config` — compute BLAKE2B-256 hash, scan EFI binary for `ZAMAK_CFG_HASH` signature, patch hash slot in place |
| CLI-4 | `[✓]` | FR-CLI-003: `zamak sbom` — SPDX 2.3 JSON document with creationInfo, packages, relationships; SHA-256 artifact checksums |
| CLI-5 | `[✓]` | ISO 8601 timestamps in all CLI log output — `iso8601_now()` / `log_info()` / `log_warn()` helpers (§3.7) |
| CLI-6 | `[✓]` | POSIX-compatible — CLI uses only `std::fs`, `std::io`, forward-slash paths throughout; no platform-specific APIs (§3.6) |

---

## SFRS — Dual-Mode CLI (SB-SFRS-STEELBORE-CLI v1.0.0) — zamak-cli

| # | Status | Task |
|---|--------|------|
| SFRS-1 | `[✓]` | §3.1 / §4.1: global `--json` flag + `--format <json\|jsonl\|yaml\|csv\|explore>` with TTY auto-detect (json default when piped, under `AI_AGENT=1`, or `CI=true`) |
| SFRS-2 | `[✓]` | §3.2: full exit-code map (0 success / 1 general / 2 usage / 3 not-found / 4 permission / 5 conflict + reserved 6 rate-limited) via `error::ErrorCode` |
| SFRS-3 | `[✓]` | §3.3: `--dry-run` on `install` and `enroll-config`; idempotent re-runs short-circuit (byte-identical MBR → no-op; existing hash already enrolled → no-op) |
| SFRS-4 | `[✓]` | §3.4 / §6.1: `zamak schema [<command>]` emits JSON Schema Draft 2020-12 (input, output, exit codes, examples) — single source of truth in `schema.rs` |
| SFRS-5 | `[✓]` | §3.4 / §6.2: `zamak describe` emits capability manifest (commands, supports_json, supports_dry_run, idempotent, destructive, formats, mcp_available, tui_feature) |
| SFRS-6 | `[✓]` | §3.5 / §4.3: structured JSON error envelope on stderr (`error.code`/`exit_code`/`message`/`hint`/`timestamp`/`command`/`docs_url`/`io_kind`) with stable `UPPER_SNAKE_CASE` codes via `error::emit` |
| SFRS-7 | `[✓]` | §3.6: stdout=data / stderr=diagnostics split; `--fields a,b,c` projection via `Value::project`; `--format jsonl` streams each data row as one line |
| SFRS-8 | `[✓]` | §3.7: noun-verb hierarchy with shared globals (`--json`, `--format`, `--fields`, `--dry-run`, `--verbose`, `--quiet`, `--color`, `--no-color`, `--yes`, `--force`, `--print0`); aliases hidden from `describe` / `schema` output |
| SFRS-9 | `[✓]` | §4.2 / §4.4: Steelbore six-token palette via 24-bit ANSI (`output::Palette`); NO_COLOR / FORCE_COLOR / CLICOLOR / `--color={auto,always,never}` precedence; ANSI suppressed in every machine mode |
| SFRS-10 | `[✓]` | §4.3: top-level JSON envelope `{metadata:{tool,version,command,timestamp}, data:...}` produced by `OutputPolicy::emit`; snake_case canonical; ISO 8601 strings; JSON null for missing values |
| SFRS-11 | `[✓]` | §5: `--format explore` TUI via `ratatui` 0.29 + `crossterm` 0.28 behind Cargo feature `tui`; alt-screen; CUA + Vim keybinds (`↑↓`/`jk`, `/` filter, `s` sort, Enter detail, `e` export); TTY + non-agent guard with JSON fallback. Feature build verified clean (`cargo check --features tui`); all 95 zamak-cli tests still pass with the feature enabled |
| SFRS-12 | `[✓]` | §6.3: `CLAUDE.md`, `AGENTS.md`, `SKILL.md`, `CONTRIBUTING.md` at `Zamak/` repo root per Steelbore context-file format |
| SFRS-13 | `[✓]` | §7.2: `validate` module — path canonicalization + allow-list (`safe_path`), control-byte rejection (`reject_control_chars`), numeric bounds (`check_bounds`), `--yes`/`--force` required for destructive ops in non-TTY (`confirm_destructive`) |
| SFRS-14 | `[✓]` | §8.1 / §8.4: POSIX text records; `--print0` / `-0` global flag wired; `zamak completions <shell>` sub-command emits bash / zsh / fish / nushell scripts |
| SFRS-15 | `[✓]` | §8.3: PowerShell-friendly JSON — single document (not NDJSON unless explicit), single-line stderr errors (`to_compact`), UTF-8 without BOM, Windows startup sets console CP 65001 via `SetConsoleOutputCP` |
| SFRS-16 | `[✓]` | §9.1: agent-env detection — `AI_AGENT` / `AGENT` / `CI` / `TERM=dumb` force json + no-color + no-TUI + no-prompts; captured once in `EnvSnapshot::capture` |
| SFRS-17 | `[✓]` | §11.1: integration test matrix in `zamak-cli/tests/sfrs_conformance.rs` — exit codes, TTY/non-TTY, `AI_AGENT=1`, `--dry-run`, control-char rejection, UTF-8 encoding, ANSI suppression, JSON Schema shape, describe enumeration |

**Out of scope / explicitly skipped:**

- §3.8 / §10 MCP server surface — only required when a tool exposes >10 sub-commands. `zamak-cli` has 3, so MCP is not mandated. Add back if/when sub-command count exceeds 10.
- TUI keybindings beyond the §5.2 required set.

---

## Testing — §8

| # | Status | Task |
|---|--------|------|
| TEST-1 | `[✓]` | Unit tests for `zamak-core` — 215 lib tests + 7 proptests; `cargo llvm-cov -p zamak-core --lib --summary-only` reports **80.52% line coverage / 87.04% function coverage** (target ≥80% met). Added tests for `elf`, `font`, `gfx`, `iso9660`, `linux_boot`, `protocol`, `rng`, `wallpaper::draw` |
| TEST-2 | `[✓]` | Miri — nightly `miri` component installed; `cargo +nightly miri test -p zamak-core --lib` runs clean (**158 passed, 0 failed**). The `spin_wait` test is gated `#[cfg(all(target_arch = "x86_64", not(miri)))]` because Miri's rdtsc stub is constant; all other `asm!` blocks have `#[cfg(miri)]` side-effect-free stubs |
| TEST-3 | `[✓]` | `zamak-test` QEMU integration test harness with serial capture + ISA debug exit — crate scaffolded, wired into CI `qemu-smoke` job |
| TEST-4 | `[~]` | Post-assembly hardware state verification — 12 host-safe tests + dedicated `zamak-asm-verify-kernel` (Limine-Protocol test kernel that runs every wrapper and emits `ASM_VERIFY_OK`) wired into CI `asm-verification` job |
| TEST-5 | `[~]` | Boot conformance — `qemu-smoke` CI job builds `zamak-test-kernel` + ESP image, boots via OVMF, captures `LIMINE_PROTOCOL_OK` through serial; full protocol × arch matrix pending multi-arch artifacts |
| TEST-6 | `[✓]` | Fuzz harnesses — `fuzz/fuzz_targets/{config_parser,uri_parser,multiboot_header,bmp_parser,config_parser_differential}.rs` via `cargo fuzz`. Differential target compares `zamak_core::config::parse` against a hand-rolled Limine v10.x reference model (clean-subset spec); 11 golden-corpus cross-checks pass in `zamak-core/tests/limine_reference_model.rs`. Full C-linked Limine `config.c` differential is a future extension |
| TEST-7 | `[✓]` | `proptest`-based property tests (`tests/proptests.rs`) — PMM normalisation/allocation/disjointness, KASLR alignment, config-parser panic safety |

---

## CI/CD Pipeline — §8.2

| # | Status | Task |
|---|--------|------|
| CI-1 | `[✓]` | Trigger: every push to `main` and every pull request — `on: push/pull_request` in ci.yml |
| CI-2 | `[✓]` | `cargo fmt --check` — `fmt` job |
| CI-3 | `[✓]` | `cargo clippy -- -D warnings` — `clippy` job |
| CI-4 | `[✓]` | `cargo test` across host crates — `test` job |
| CI-5 | `[✓]` | `cargo +nightly miri test` on `zamak-core` — `miri` job |
| CI-6 | `[✓]` | `cargo deny check` (license + CVE audit) — `deny` job |
| CI-7 | `[✓]` | Cross-compile all five architectures — `cross` matrix job |
| CI-8 | `[✓]` | Binary size gate — `size-gate` job compares each artifact against a Limine v10.x baseline and fails at >120% per §6.1 |
| CI-9 | `[✓]` | QEMU smoke tests — `qemu-smoke` job via `zamak-test` harness |
| CI-10 | `[✓]` | `asm-verification` CI job — builds `zamak-asm-verify-kernel`, packages it into an ESP image, boots under QEMU + OVMF, fails the build unless `ASM_VERIFY_OK` appears on serial |
| CI-11 | `[✓]` | SPDX SBOM generation — `sbom` job invokes `zamak-cli sbom` |
| CI-12 | `[✓]` | Publish artifacts + SBOM on tagged release — `release.yml` workflow |

---

## Release Artifacts — §9.2

| # | Status | Task |
|---|--------|------|
| REL-1 | `[~]` | `zamak-bios.sys` — wired into `release.yml` via `bios-stage3` job; first tagged release will publish |
| REL-2 | `[~]` | `BOOTX64.EFI` — `build-artifacts` matrix target `x86_64-unknown-uefi` |
| REL-3 | `[~]` | `BOOTIA32.EFI` — `build-artifacts` matrix target `i686-unknown-uefi` |
| REL-4 | `[~]` | `BOOTAA64.EFI` — `build-artifacts` matrix target `aarch64-unknown-uefi` |
| REL-5 | `[~]` | `BOOTRISCV64.EFI` — `build-artifacts` matrix target `riscv64gc-unknown-none-elf` |
| REL-6 | `[~]` | `BOOTLOONGARCH64.EFI` — `build-artifacts` matrix target `loongarch64-unknown-none` |
| REL-7 | `[~]` | `zamak` CLI binary — `cli-binaries` matrix job (Linux x86-64, macOS AArch64) + new `cli-freebsd` job (FreeBSD 14.x via `vmactions/freebsd-vm@v1`); first tagged release will publish all three |
| REL-8 | `[✓]` | `zamak-<ver>.spdx.json` — generated by `publish` job via `zamak-cli sbom` |
| REL-9 | `[✓]` | `SHA256SUMS` — produced in `publish` job via `sha256sum` over `dist/` |
| REL-10 | `[✓]` | `CHANGELOG.md` — Keep a Changelog format, ISO 8601 dates in `Zamak/CHANGELOG.md` |

---

## Summary

| Category | Done | Partial | Not Started |
|----------|------|---------|-------------|
| M0 Scaffolding | 17 | 0 | 0 |
| Asm Compliance §3.2 | 4 | 0 | 0 |
| Asm Safety §3.9 | 10 | 0 | 0 |
| License / SPDX | 3 | 0 | 0 |
| Rust Guidelines | 5 | 0 | 0 |
| POSIX | 2 | 0 | 0 |
| M1 BIOS Boot | 15 | 1 | 0 |
| M2 UEFI Boot | 12 | 1 | 0 |
| M3 Config/Menu/Theme | 15 | 0 | 0 |
| M4 Multi-arch | 6 | 2 | 0 |
| M5 Feature Parity | 9 | 0 | 0 |
| M6 LoongArch64 | 1 | 3 | 0 |
| Host CLI | 6 | 0 | 0 |
| SFRS Dual-Mode CLI | 17 | 0 | 0 |
| Testing | 5 | 2 | 0 |
| CI/CD | 12 | 0 | 0 |
| Release Artifacts | 3 | 7 | 0 |
| **Total** | **143** | **15** | **0** |

**No items are fully not-started.** Every remaining item is `[~]` partial, waiting on:

- **Tagged release** — flips REL-1..7, M4-7, M6-2 once `release.yml` runs on a pushed tag (9 items)
- **CI artifact confirmation** — `boot-smoke` on `zamak-test-kernel` (M1-16), Linux bzImage UEFI smoke (M2-12), full protocol × arch matrix (TEST-5), asm verification run (TEST-4) — 4 items
- **LoongArch UEFI target** — M6-1 blocked on rustc upstream (`loongarch64-unknown-uefi` target does not yet exist; `uefi-services`' `efiapi` ABI unsupported on `loongarch64-unknown-none`). Paging builder + handoff code are implemented and compile for `loongarch64-unknown-none`; will flip to `[✓]` when rustc lands the UEFI target
- **Real hardware perf baseline** — M6-3 cold-boot timing requires bare-metal measurement
