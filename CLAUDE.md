<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 Mohamed Hammad
-->

# CLAUDE.md — ZAMAK project context for Claude Code

This file is loaded automatically by Claude Code when working in
this repository. It captures the invariants a Claude Code session
needs to avoid re-discovering them from scratch.

## What this repository is

**ZAMAK** is a Rust rewrite of the Limine bootloader targeting BIOS
and UEFI on x86-64, AArch64, RISC-V 64, and LoongArch64. It is the
second project of the **Spacecraft Software** ecosystem. The canonical spec is
`../ZAMAK_Bootloader_PRD_v1.3.docx.md` (SS-PRD-ZAMAK-001 v1.3.0).
Implementation status lives in `../TODO.md`.

## Workspace layout

Multi-crate Cargo workspace rooted at `Zamak/Cargo.toml`:

| Crate | Purpose |
|---|---|
| `zamak-core` | Shared no-std library (protocol, config, VMM, arch) |
| `zamak-proto` | Standalone no-std protocol types |
| `zamak-bios` | BIOS stage-3 boot path |
| `zamak-uefi` | UEFI loader for all four arches |
| `zamak-stage1` | 512-byte MBR (pure `global_asm!`) |
| `zamak-decompressor` | Stage-2 decompressor (`miniz_oxide`) |
| `zamak-cli` | Host CLI (install, enroll-config, sbom, schema, describe, completions) |
| `zamak-theme` | TOML theme parser |
| `zamak-test` | QEMU integration-test harness |
| `zamak-macros` | Proc-macros (`#[zamak_unsafe]`) |

`zamak-test-kernel` and `zamak-linux-stub-kernel` are *excluded*
from the workspace (`Cargo.toml:22`) so their nightly +
`build-std` configs apply cleanly. Build explicitly with
`cargo +nightly build --manifest-path <crate>/Cargo.toml --release`.

## Build / test commands

Mirror of what CI runs (`.github/workflows/ci.yml`):

```sh
cargo fmt --all -- --check                                       # fmt
cargo clippy -p zamak-core -p zamak-proto -p zamak-theme \
             -p zamak-cli  -p zamak-macros -p zamak-test \
             --all-targets -- -D warnings                        # host-runnable crates only
cargo test  -p zamak-core --lib -p zamak-theme -p zamak-cli      # unit
cargo test  -p zamak-core --test proptests                       # property
cargo miri-test                                                  # alias → +nightly miri test -p zamak-core --lib
cargo deny check                                                 # licenses + advisories
cargo build -p zamak-uefi --target <T> \
            -Z build-std=core,alloc,compiler_builtins \
            -Z build-std-features=compiler-builtins-mem          # cross, T ∈ {x86_64-unknown-uefi,
                                                                 #                 aarch64-unknown-uefi,
                                                                 #                 riscv64gc-unknown-none-elf,
                                                                 #                 loongarch64-unknown-none}
./zamak-test/build-images.sh && \
  cargo run -p zamak-test -- --suite <S> --timeout 60            # S ∈ {boot-smoke, linux-bzimage, asm-verification}
cargo build --features tui -p zamak-cli                          # CLI with TUI explore mode
```

Freestanding crates (`zamak-uefi`, `zamak-bios`, `zamak-stage1`,
`zamak-decompressor`) are not in the clippy / unit-test sets; they
build under `cross` against their real targets.

### Local build prerequisites

- **`RUSTFLAGS=''` prefix** is required if your shell exports flags
  incompatible with the cross targets — `-C target-cpu=x86-64-v3`
  is the common culprit, and `-Clink-arg=-z -Clink-arg=pack-relative-relocs`
  also breaks the UEFI link step (`rust-lld` in PE-COFF mode mis-parses
  `-z` as an unknown PE option). CI sets `RUSTFLAGS: -D warnings`
  globally in the workflow env (`ci.yml:25`).
- **`cc` / `objcopy` / `ld` not on PATH** (e.g. plain login shell
  on NixOS, no `nix develop` / `nix-shell` active): wrap cargo
  invocations with
  `nix shell nixpkgs#gcc nixpkgs#binutils -c <cmd>`. The error
  surfaces as `error: linker 'cc' not found` from the
  `compiler_builtins` build script and blocks every `-Zbuild-std`
  build.
- **`mtools` + `sfdisk` + `objcopy`** are required by
  `zamak-test/build-images.sh` (BIOS disk image assembly). CI
  installs them via `apt-get install -y qemu-system-x86 ovmf mtools`;
  locally on NixOS they're already on the system PATH except for
  `objcopy` (`binutils`).

### CI jobs

`fmt`, `clippy`, `test` (3-target matrix: Linux x86-64, Linux
AArch64, macOS AArch64), `freebsd` (14.x via
`vmactions/freebsd-vm`), `miri`, `deny`, `cross` (4 UEFI/none
targets), `size-gate` (≤ 120 % of Limine v10.x baseline, §6.1),
`qemu-smoke` (`boot-smoke` + `linux-bzimage`), `asm-verification`,
and `sbom` (main-only, SPDX 2.3 via `zamak-cli sbom`).

## Spacecraft Software Standard invariants

These are **non-negotiable**. Violating any one of them is a blocking
defect.

- **ISO 8601 UTC everywhere.** All timestamps end with `Z`. No
  local time. No `--local-time` flag. Duration values use ISO 8601
  duration format (e.g. `PT1H30M`).
- **UTF-8 without BOM.** Output is always UTF-8; on Windows the CLI
  sets console CP 65001 at startup.
- **POSIX-first design.** No Bash-isms / Nushell-isms / PowerShell-isms
  in default output.
- **Metric and 24-hour.** No AM/PM, no imperial units.
- **GPL-3.0-or-later + SPDX headers** on every source file.
- **Inline assembly only** (`global_asm!` / `asm!`). No `.asm` files.
  Every `asm!` block ≤ 20 instructions, uses symbolic operands, sets
  most-restrictive `options(...)` tags, carries a structured
  `// SAFETY:` contract (Preconditions / Postconditions / Clobbers /
  Worst-case), and ships a `#[cfg(miri)]` stub.
- **Newtype address wrappers** (`PhysAddr`, `VirtAddr`, `Cr3Value`,
  `MairValue`, `SatpValue`) and `checked_add`/`checked_sub`/`div_ceil`
  on all address arithmetic.
- **Binary-size budget.** Each bootloader artifact (`BOOTX64.EFI`,
  `BOOTAA64.EFI`, `BOOTRISCV64.EFI`, `zamak-bios.sys`) must stay
  ≤ 120 % of the Limine v10.x baseline. CI `size-gate` enforces
  this; baselines live inline in `.github/workflows/ci.yml`.

## CLI: SFRS dual-mode contract

`zamak-cli` conforms to `SS-SFRS-SPACECRAFT-SOFTWARE-CLI v1.0.0`:

- **Global flags** (every sub-command): `--json`, `--format <fmt>`,
  `--fields`, `--dry-run`, `--verbose`, `--quiet`, `--color`,
  `--no-color`, `--yes`, `--force`, `--print0`, `--help`,
  `--version`.
- **Output formats**: `human` (TTY default), `json` (default when
  piped / `AI_AGENT=1` / `CI=true`), `jsonl`, `yaml`, `csv`,
  `explore` (TUI, feature-gated).
- **Envelope**: `{metadata: {tool, version, command, timestamp}, data: ...}`.
- **Exit codes**: 0 success / 1 general / 2 usage / 3 not-found /
  4 permission / 5 conflict.
- **Structured errors on stderr** in machine mode
  (`error.code/exit_code/message/hint/timestamp/command/docs_url`).
- **Agent detection**: `AI_AGENT` / `AGENT` / `CI` / `TERM=dumb`
  force JSON + no-color + no-TUI + no-prompts.
- **Self-description**: `zamak schema` + `zamak describe` provide
  JSON-Schema-driven introspection.

## Conventions Claude Code should maintain

- Prefer editing existing files over creating new ones.
- Default to NO comments — well-named identifiers carry meaning.
  Comments are reserved for the WHY of non-obvious constraints.
- Default to NO trailing summaries; the user reads diffs.
- In-tree tests only; no integration tests that require host state
  (use `zamak-test` harness for those).
- Keep commits small and scoped — one SFRS-N or M-N item per commit
  when possible.
- Every user-facing change updates `CHANGELOG.md` and `TODO.md` in
  the *same* commit (per `CONTRIBUTING.md`). Releases roll
  `[Unreleased]` → `## [vX.Y.Z] - YYYY-MM-DD`.

## Companion docs

- `AGENTS.md` — safety-critical crates (`arch`, `entry`, `mbr`,
  `trampoline`, `handoff`, `protocol`) and the
  `zamak describe --json` live capability manifest.
- `SKILL.md` — agent-facing CLI patterns (`--dry-run --json`
  preview, non-TTY → `--yes` / `--force`, `--format explore` TUI
  fallback).
- `CONTRIBUTING.md` — PR checklist, unsafe-block contract format,
  and the `CHANGELOG.md` + `TODO.md` co-update rule above.

## Current milestone

See `../TODO.md` for the authoritative live status. v0.9.0 closed
M1-16 (BIOS Path B → end-to-end Limine boot) and the FreeBSD CI
runner (now via `vmactions/freebsd-vm@v1` — `ci.yml:89`). The two
known partials gating v1.0 are:

- **M6-1 LoongArch UEFI** — blocked on rustc upstream
  (`loongarch64-unknown-uefi` target not yet stable).
- **M6-3 Part 2** — bare-metal perf baseline against Limine v10.x.
  Part 1 (TSC instrumentation) shipped in v0.8.5; the hardware
  capture is the open work. Staging tree lives at
  `dist/perf-baseline/` (gitignored). The
  `zamak-test-kernel` emits a `KERNEL_ENTRY tsc=<u64>` line on
  every Limine-Protocol entry so ZAMAK and Limine can be compared
  with the same kernel, same physical TSC.
