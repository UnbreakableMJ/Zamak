<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- SPDX-FileCopyrightText: 2026 Mohamed Hammad -->

# ZAMAK Fuzz Harnesses (PRD §8.1, TEST-6)

Libfuzzer-based harnesses for the parsers that accept external input.

## Prerequisites

```sh
cargo install cargo-fuzz
rustup install nightly
```

## Running a target

```sh
cd Zamak/fuzz
cargo +nightly fuzz run config_parser
cargo +nightly fuzz run uri_parser
cargo +nightly fuzz run multiboot_header
cargo +nightly fuzz run bmp_parser
```

Targets:

| Target | Module fuzzed |
|---|---|
| `config_parser` | `zamak_core::config::parse` — Limine-compatible config |
| `uri_parser` | `zamak_core::uri::parse_uri` — `boot()`/`hdd()`/`fslabel()`/`tftp()` URIs |
| `multiboot_header` | `zamak_core::multiboot::find_header` + `parse_header` |
| `bmp_parser` | `zamak_core::wallpaper::parse` — BMP wallpaper decoder |

## PRD requirement

> Differential fuzzing of config parser against Limine (72 h continuous per release)
> — §8.1

The `config_parser` harness feeds the same input to both the ZAMAK and Limine
parsers and asserts identical outputs. Integration with a Limine reference
build is tracked in TEST-6.
