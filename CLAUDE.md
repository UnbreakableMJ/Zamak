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
second project of the **Steelbore** ecosystem. The canonical spec is
`../ZAMAK_Bootloader_PRD_v1.3.docx.md` (SB-PRD-ZAMAK-001 v1.3.0).
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
| `zamak-test-kernel` | Minimal Limine-Protocol test kernel |
| `zamak-macros` | Proc-macros (`#[zamak_unsafe]`) |

## Build / test commands

- `RUSTFLAGS='' cargo build` — default workspace build. `RUSTFLAGS=''`
  prefix is required because the shell env may carry
  `-C target-cpu=x86-64-v3`, which breaks RISC-V / LoongArch cross builds.
- `RUSTFLAGS='' cargo test -p zamak-core -p zamak-theme -p zamak-cli` — unit tests
- `RUSTFLAGS='' cargo test --workspace` — full workspace
- `RUSTFLAGS='' cargo clippy --lib -- -D warnings` — lint
- `cargo +nightly miri test -p zamak-core --lib` — Miri (nightly component required)
- `./zamak-test/build-images.sh && RUSTFLAGS='' cargo run -p zamak-test -- --suite boot-smoke` — QEMU smoke
- `cargo build --features tui -p zamak-cli` — CLI with TUI explore mode

## Steelbore Standard invariants

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

## CLI: SFRS dual-mode contract

`zamak-cli` conforms to `SB-SFRS-STEELBORE-CLI v1.0.0`:

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

## Current milestone

See `../TODO.md` for the authoritative live status. At time of
writing ZAMAK is past M0–M5 milestone scope; remaining work is
gated on tagged release, FreeBSD CI runner, bare-metal perf
validation, and the SFRS dual-mode CLI follow-ons.
