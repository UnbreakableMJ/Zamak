<!--
SPDX-License-Identifier: GPL-3.0-or-later
SPDX-FileCopyrightText: 2026 Mohamed Hammad
-->

# Contributing to ZAMAK

Thanks for your interest in ZAMAK — the Steelbore-ecosystem Rust
rewrite of the Limine bootloader. This document is the human
contributor on-ramp; `CLAUDE.md`, `AGENTS.md`, and `SKILL.md` cover
the machine / agent side.

## Ground rules

- **License:** GPL-3.0-or-later. By contributing you agree your work
  is licensed under GPL-3.0-or-later.
- **SPDX headers** at the top of every source file you create or
  substantially modify:
  ```rust
  // SPDX-License-Identifier: GPL-3.0-or-later
  // SPDX-FileCopyrightText: 2026 <Your Name>
  ```
- **Commit messages:** short imperative-mood subject (≤72 chars),
  blank line, then a body explaining the *why*. Reference the
  affected TODO.md item (e.g. `RG-3`, `SFRS-4`) in the subject when
  applicable.
- **Branching:** feature branches off `main`, squash-or-rebase merge.

## Development setup

```
git clone https://codeberg.org/steelbore/zamak
cd zamak/Zamak
RUSTFLAGS='' cargo build
RUSTFLAGS='' cargo test --workspace
RUSTFLAGS='' cargo clippy --lib -- -D warnings
cargo fmt --check
```

The `RUSTFLAGS=''` prefix is required because the shell environment
may carry `-C target-cpu=x86-64-v3`, which breaks cross-compilation
for RISC-V and LoongArch targets.

### Nightly toolchain (optional)

Miri and a handful of experiments need nightly:

```
rustup toolchain install nightly
rustup component add miri --toolchain nightly
cargo +nightly miri test -p zamak-core --lib
```

### QEMU smoke tests

```
./zamak-test/build-images.sh
RUSTFLAGS='' cargo run -p zamak-test -- --suite boot-smoke
```

Requires: `qemu-system-x86_64`, `qemu-system-aarch64`, `OVMF`,
`mtools`.

## Pull-request checklist

Before opening a PR:

- [ ] `cargo fmt` is clean.
- [ ] `cargo clippy --lib -- -D warnings` is clean across every crate.
- [ ] `cargo test --workspace` passes.
- [ ] New `unsafe` blocks carry a structured `// SAFETY:` contract
      (Preconditions / Postconditions / Clobbers / Worst-case).
- [ ] New assembly is inline (`global_asm!` / `asm!`) with
      `#[cfg(miri)]` stubs, symbolic operands, most-restrictive
      `options(...)`, and ≤ 20 instructions per block.
- [ ] New address arithmetic uses `checked_add` / `checked_sub` /
      `div_ceil` or an address newtype from `zamak_core::addr`.
- [ ] New CLI features honour SFRS §3–§11 (global flags,
      JSON envelope, exit-code map, structured errors).
- [ ] `CHANGELOG.md` has a new entry under `## [Unreleased]`.
- [ ] `TODO.md` has been updated if the change clears a tracked
      item.

## CLI-specific expectations

`zamak-cli` conforms to `SB-SFRS-STEELBORE-CLI v1.0.0`. New sub-
commands must:

1. Register a `CommandSpec` in `src/schema.rs`.
2. Live in their own module under `src/commands/`.
3. Return a JSON `Value` for `data` (no raw prints to stdout).
4. Emit diagnostics via `crate::output::emit_info` /
   `crate::output::emit_warn` (never stdout).
5. Honour `--dry-run` on writes.
6. Be covered by at least one unit test in `src/main.rs::tests` and
   one test per new flag.

Run `zamak describe --json` after your change and verify the new
command appears in the manifest.

## Code of Conduct

Be kind. Assume good faith. The Steelbore project values rigour
over speed and correctness over cleverness; feedback in that spirit
is always welcome.
