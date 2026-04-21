<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 Mohamed Hammad
-->

# AGENTS.md — ZAMAK agent context (Codex, Cursor, generic)

Complements `CLAUDE.md`. Where `CLAUDE.md` is Claude-Code-specific,
this file is the neutral agent-onboarding document consumed by
OpenAI Codex CLI, Cursor, Aider, and similar tools.

## Project identity

- **Name:** ZAMAK
- **Type:** Rust bootloader (BIOS + UEFI, x86-64 / AArch64 /
  RISC-V 64 / LoongArch64)
- **Status:** active development; reference PRD v1.3.0
- **Organisation:** Steelbore
- **License:** GPL-3.0-or-later on every source file

## Coding conventions

- Rust edition 2021, stable toolchain for production crates.
- Microsoft Pragmatic Rust Guidelines apply (see `rust-guidelines`
  skill in Claude Code, or the equivalent house style).
- `#![no_std]` for every bootloader crate. `std` is permitted only
  in `zamak-cli` and `zamak-test`.
- `#[repr(C)]` on every struct that crosses the firmware / kernel
  boundary, plus `const_assert!` on its size/offsets.
- Panic policy: `panic = "abort"` in release; `panic!` in library
  code is a bug unless explicitly justified.
- Error handling: internal functions return `Result`. `process::exit`
  is called only from `main`.

## Test patterns

- Unit tests live in `#[cfg(test)] mod tests` at the bottom of each
  source file, or in a sibling `tests/` directory when they need a
  test fixture.
- `proptest` is used for address-arithmetic and parser invariants;
  `cargo test --test proptests -p zamak-core` runs the suite.
- QEMU-driven boot tests live in `zamak-test`; the entry point is
  `zamak-test --suite <name>`. Suites in scope today: `boot-smoke`,
  `asm-verification`, `linux-bzimage`.
- Miri: `cargo +nightly miri test -p zamak-core --lib`. Any `asm!`
  or raw-pointer shim has a `#[cfg(miri)]` stub that mocks the
  hardware interaction.

## Forbidden patterns

- Separate `.asm` files (use `global_asm!` / `asm!`).
- `unsafe` blocks without a structured `// SAFETY:` comment
  (Preconditions / Postconditions / Clobbers / Worst-case).
- Naked arithmetic on addresses. Use `checked_add`, `checked_sub`,
  `div_ceil`, or the newtype wrappers in `zamak_core::addr`.
- Panicking in `no_std` code during normal flow.
- Shell-specific idioms in the host CLI output. Default text output
  must be POSIX-parseable with `grep`/`awk`/`cut`/`sed`.
- External dependencies with non-GPL-3.0-compatible licenses
  (`cargo deny check` enforces this in CI).
- `--local-time` flags or locale-dependent time formatting.

## CLI conformance

Every host CLI binary (`zamak-cli` today, future ones the same)
must conform to `SB-SFRS-STEELBORE-CLI v1.0.0`:

- `--json` / `--format` / structured JSON error envelope on stderr
- `schema` and `describe` sub-commands
- Exit codes 0/1/2/3/4/5 as documented in the SFRS
- `--dry-run` on every write command, idempotent re-runs
- `AI_AGENT` / `AGENT` / `CI` env detection
- `NO_COLOR` / `FORCE_COLOR` / `CLICOLOR` precedence
- ISO 8601 UTC timestamps, UTF-8 without BOM

`zamak describe --json` returns the authoritative capability
manifest at runtime — consult it instead of hard-coding flag lists.

## Repository layout checkpoint

```
Zamak/
├── Cargo.toml              # workspace root
├── CHANGELOG.md            # Keep-a-Changelog + ISO 8601 dates
├── CLAUDE.md               # Claude-Code-specific context
├── AGENTS.md               # (this file)
├── SKILL.md                # Steelbore skill manifest
├── CONTRIBUTING.md         # human-contributor guide
├── zamak-core/             # shared no-std library
├── zamak-cli/              # host CLI (dual-mode)
├── zamak-bios/             # BIOS stage-3
├── zamak-uefi/             # UEFI loader (all arches)
├── ...
└── target/                 # build output (gitignored)
```

## Safety-critical areas

Changes in these files must run the full test suite (`cargo test
--workspace` + `zamak-test --suite asm-verification`) before merging:

- `zamak-core/src/arch/**` — arch-specific low-level helpers
- `zamak-bios/src/entry.rs`, `mbr.rs`, `trampoline.rs` — inline asm
- `zamak-uefi/src/main.rs`, `handoff.rs` — kernel hand-off
- `zamak-core/src/protocol.rs` — Limine Protocol ABI

## Useful entrypoints

- `zamak describe --json` — live capability manifest
- `zamak schema install` — install-command JSON Schema
- `cargo xtask` — currently unused; reserved for future task runner
