---
name: zamak
description: ZAMAK bootloader host CLI — install stage1 MBR + stage2, enroll config BLAKE2B hashes into UEFI binaries, generate SPDX 2.3 SBOMs. Dual-mode (human + agent) per SB-SFRS-STEELBORE-CLI v1.0.0.
license: GPL-3.0-or-later
project: Steelbore
component: Zamak
version: 0.1.0
entry_points:
  - command: zamak install
    description: Write stage1 MBR and record stage2 location (FR-CLI-001)
    destructive: true
    idempotent: true
    supports_dry_run: true
  - command: zamak enroll-config
    description: Compute BLAKE2B-256 config hash and patch EFI binary (FR-CLI-002)
    destructive: true
    idempotent: true
    supports_dry_run: true
  - command: zamak sbom
    description: Generate SPDX 2.3 JSON SBOM (FR-CLI-003)
    destructive: false
    idempotent: true
  - command: zamak schema
    description: Emit JSON Schema Draft 2020-12 for tool or single command
    destructive: false
    idempotent: true
  - command: zamak describe
    description: Emit capability manifest
    destructive: false
    idempotent: true
  - command: zamak completions <shell>
    description: Emit shell completion script for bash / zsh / fish / nushell
    destructive: false
    idempotent: true
global_flags:
  - "--json"
  - "--format"
  - "--fields"
  - "--dry-run"
  - "--verbose"
  - "--quiet"
  - "--color"
  - "--no-color"
  - "--yes"
  - "--force"
  - "--print0"
output_formats: [human, json, jsonl, yaml, csv, explore]
exit_codes:
  0: SUCCESS
  1: GENERAL_ERROR
  2: USAGE_ERROR
  3: NOT_FOUND
  4: PERMISSION_DENIED
  5: CONFLICT
agent_env_vars:
  AI_AGENT: Force JSON + no color + no TUI + no prompts
  AGENT: Same as AI_AGENT
  CI: Same (for CI pipelines)
  NO_COLOR: Disable ANSI color
  FORCE_COLOR: Force ANSI color (overrides NO_COLOR)
  TERM: "dumb disables color + TUI"
---

# ZAMAK CLI — agent skill

## Overview

`zamak` is the host-side companion CLI for the ZAMAK Rust
bootloader. It provides three safety-critical operations against
boot media and release artifacts:

1. **Install** — writes a 512-byte stage1 MBR to a target device,
   writes stage2 starting at an LBA of your choice, and patches the
   stage2 location into the MBR's patchable fields.
2. **Enroll config** — computes a BLAKE2B-256 hash of a
   `zamak.conf`, locates the `ZAMAK_CFG_HASH` slot signature in an
   EFI binary, and patches the hash in place. This activates
   hash-lock mode (editor-disabled) at boot.
3. **SBOM** — produces an SPDX 2.3 JSON document for a release,
   optionally including SHA-256 checksums for listed artifacts.

Plus three introspection commands — `schema`, `describe`,
`completions` — that expose the tool's own capabilities for agents
and shells.

## When to use this skill

- You are automating ZAMAK release workflows (CI, bring-your-own
  provisioning pipelines, Forgejo actions).
- You are writing a skill that needs to invoke `zamak` sub-commands
  and consume their structured output.
- You are debugging a ZAMAK boot problem and want machine-readable
  output about what the installer saw on disk.

## Key patterns

```nu
# List all sub-commands with their destructive / idempotent flags.
zamak describe --json | from json | get data.commands
```

```bash
# Dry-run the install step and preview the JSON action plan.
zamak install --mbr mbr.bin --stage2 stage2.bin --target disk.img \
    --dry-run --json | jq .data
```

```powershell
# PowerShell 7+
$sbom = zamak sbom --version 0.7.0 --json dist/BOOTX64.EFI | ConvertFrom-Json
$sbom.data.spdxVersion
```

```fish
# Fetch the install command's JSON Schema (usable as an LLM
# function-calling tool definition).
zamak schema install --json | jq .
```

## Constraints

- `install` and `enroll-config` require `--yes` or `--force` when
  stdin is not a TTY. Running under an agent means non-TTY — always
  pass `--yes` after your own confirmation logic.
- `--format explore` requires a TTY **and** the `tui` Cargo feature.
  Under `AI_AGENT=1` or when stdout is a pipe, the command falls
  back to JSON with a warning on stderr.
- Errors in JSON mode always land on stderr as a single-line JSON
  object matching the SFRS §3.5 envelope.
- The `sbom` command, with `--output <path>`, writes the SPDX
  document verbatim (no envelope) to the file; the stdout stream
  still carries the metadata/data envelope.

## Reference

- `../ZAMAK_Bootloader_PRD_v1.3.docx.md` — full product spec
- `../TODO.md` — live implementation status
- `../steelbore-dual-mode-cli-sfrs.docx` — CLI framework SFRS v1.0.0
- `zamak describe --json` — live capability manifest (authoritative
  at runtime)
