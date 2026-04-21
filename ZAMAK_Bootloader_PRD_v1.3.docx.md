  
**`PROJECT STEELBORE`**

**`ZAMAK Bootloader`**

`Product Requirements Document (PRD)`

| `Document ID:` | `SB-PRD-ZAMAK-001` |
| :---- | :---- |
| `Version:` | `1.3.0` |
| `Date:` | `2026-02-19` |
| `Author:` | `Mohamed Hammad` |
| `Status:` | `DRAFT` |
| `Classification:` | `Internal — Steelbore Contributors` |
| `License:` | `GPL-3.0-or-later` |
| `SPDX Identifier:` | `SPDX-License-Identifier: GPL-3.0-or-later` |
| `Copyright:` | `© 2026 Mohamed Hammad. All rights reserved.` |

*`Prepared by Mohamed Hammad`*

*`© 2026 Mohamed Hammad. All rights reserved.`*

*`Licensed under GPL-3.0-or-later.`*

*`All dates in this document use ISO 8601 format (YYYY-MM-DD).`*

**`Copyright and Legal Notice`**

`© 2026 Mohamed Hammad. All rights reserved.`

`This document and its contents are the intellectual property of Mohamed Hammad, authored as part of Project Steelbore. No part of this document may be reproduced, distributed, or transmitted in any form or by any means without the prior written permission of the author, except as permitted under the terms of the GNU General Public License v3.0 or later (GPL-3.0-or-later).`

`The ZAMAK software described herein is licensed under the GPL-3.0-or-later. A copy of the license is available at gnu.org/licenses/gpl-3.0.html.`

`SPDX document header for this file:`

SPDX-License-Identifier: GPL-3.0-or-later

SPDX-FileCopyrightText: 2026 Mohamed Hammad

`Trademarks: “Steelbore” and “ZAMAK” are trademarks of Mohamed Hammad. “Limine” is a trademark of its respective author(s). All other trademarks mentioned in this document are the property of their respective owners.`

`Disclaimer: This document is provided “as is” without warranty of any kind, express or implied. The information contained herein is subject to change without notice. The author shall not be liable for any errors or omissions in this document.`

# **`1. Executive Summary`**

`ZAMAK is a ground-up rewrite of the Limine bootloader in the Rust programming language. It is a sub-project of Project Steelbore, an initiative to deliver memory-safe, auditable, and high-performance system-level tooling under the GPL-3.0-or-later license. ZAMAK retains full protocol and feature parity with Limine while leveraging Rust’s type system and ownership model to eliminate entire classes of memory-safety vulnerabilities inherent in the existing C codebase.`

`The name “ZAMAK” is derived from the family of zinc–aluminium alloys used in precision die-casting—reflecting the project’s philosophy of producing a component that is strong, precise, and castable into any system mold.`

## **`1.1 Problem Statement`**

`Limine is an established multiprotocol bootloader written primarily in C (~92%) with minimal assembly. While it is well-architected and supports five CPU architectures, two firmware interfaces, and five boot protocols, its C foundation inherits systemic risks: buffer overflows in the config parser, use-after-free in the PMM, integer overflow in page-table arithmetic, and undefined behavior in firmware callback handling. These risk categories cannot be eliminated through code review alone; they require a language-level guarantee.`

## **`1.2 Proposed Solution`**

`ZAMAK replaces the entire C codebase with idiomatic,` \#\!\[no\_std\] `Rust while preserving the identical external interface: same configuration file format (`limine.conf`), same boot protocols, same Limine Protocol request/response ABI, same filesystem support, and same multi-architecture targeting. All architecture-specific assembly (boot sectors, trampolines, CPU mode transitions) is embedded directly in Rust source files via` core::arch::asm\! `/` global\_asm\! `macros, as mandated by the Steelbore standard. A dedicated Assembly Memory Safety framework (Section 3.9) ensures that these unavoidable` unsafe `assembly boundaries are as safe as mechanically possible.`

## **`1.3 Success Criteria`**

| `Criterion` | `Target` | `Measurement` |
| :---- | :---- | :---- |
| `Protocol parity` | `100% of Limine boot protocols supported` | `Protocol conformance test suite` |
| `Config parity` | `Byte-identical parsing of all valid limine.conf files` | `Differential fuzzing against Limine` |
| `Boot time` | `≤ 105% of Limine on equivalent hardware` | `Automated benchmark on reference platforms` |
| `Binary size` | `≤ 120% of Limine per-architecture` | `CI size-gate` |
| `Memory safety` | `Zero unsafe blocks outside #[zamak_unsafe] boundary` | `Miri + cargo-audit + manual audit` |
| `Asm safety` | `100% of asm! blocks wrapped in safe APIs` | `Audit + compile-time offset assertions` |
| `SPDX compliance` | `Machine-readable SBOM for every release` | `SPDX validation tool (spdx-tools)` |

# **`2. Scope and Boundaries`**

## **`2.1 In Scope`**

* `Complete reimplementation of Limine’s boot logic in Rust, covering BIOS three-stage chain and UEFI single-application paths.`

* `All five boot protocols: Limine Protocol, Linux Boot Protocol, Multiboot 1, Multiboot 2, and chainloading (EFI and BIOS).`

* `All five target architectures: x86 (IA-32), x86-64, AArch64, RISC-V 64, and LoongArch64.`

* `Configuration parser for limine.conf syntax (v9+ format) with macro expansion, BLAKE2B hash verification, and SMBIOS OEM String injection.`

* `Physical memory manager (PMM), virtual memory manager (VMM), ELF/PE loader, ACPI table discovery, SMP initialization.`

* `Interactive boot menu with Flanterm-compatible graphical terminal, live config editor with syntax highlighting and validation.`

* `Color theme system (configured separately per Steelbore standard).`

* `Assembly Memory Safety framework: safe Rust wrappers, newtype invariants, compile-time layout assertions, and structured SAFETY contracts for all asm! boundaries.`

* `Host utility (zamak CLI) for MBR installation and config hash enrollment.`

* `SPDX SBOM generation integrated into CI/CD.`

## **`2.2 Out of Scope`**

* `Filesystem support beyond FAT12/16/32 and ISO 9660 (matches Limine’s intentional design limitation).`

* `Network boot (PXE/HTTP) beyond TFTP (matches Limine).`

* `Graphical splash screens or animation (boot menu uses Material Design color tokens for theming only; no bitmapped UI widgets).`

* `Runtime kernel services (ZAMAK is a bootloader, not a kernel).`

* `GRUB compatibility or migration tooling.`

## **`2.3 Assumptions and Dependencies`**

1. `The Rust toolchain supports all five target architectures via existing tier-2/tier-3 targets, or custom target specifications will be provided.`

2. `The Limine Protocol specification (v3+) is stable and its ABI will not change during ZAMAK’s initial development cycle.`

3. `UEFI firmware on target platforms conforms to UEFI Specification 2.9 or later.`

4. `Contributors have read and accepted the Steelbore Contributor Agreement and the Pragmatic Rust Guidelines.`

# **`3. Steelbore Standard Compliance`**

`As a first-class Steelbore sub-project, ZAMAK must comply with every clause of the Steelbore standard. This section maps each requirement to the concrete design decisions in ZAMAK.`

## **`3.1 Color Theme Support (Configured Separately)`**

`ZAMAK’s boot menu terminal, editor, and branding elements must be fully themeable. Themes are defined in a separate file (`zamak-theme.toml`) that is never embedded in` limine.conf`. This separation ensures that visual identity can be managed independently from boot logic.`

### *`3.1.1 Theme File Format`*

zamak-theme.toml `uses TOML syntax and defines the following token groups:`

| `Token Group` | `Tokens` | `Description` |
| :---- | :---- | :---- |
| `surface` | `background, foreground, dim, bright` | `Base terminal surface colors` |
| `accent` | `primary, secondary, error, warning, success` | `Semantic accent colors for UI chrome` |
| `palette` | `ansi_0 through ansi_15` | `Full 16-color ANSI palette override` |
| `editor` | `key, colon, value, comment, invalid` | `Syntax-highlighting colors for config editor` |
| `branding` | `text_color, bar_color` | `Boot menu branding strip colors` |

`All color values are specified as 6-digit hexadecimal RGB strings (e.g.,` "50FA7B"`). The theme engine resolves tokens at boot time via a lookup table. A built-in default theme ships with Material Design Blue 800 as the primary accent.`

### *`3.1.2 Material Design Alignment`*

`Where applicable, the default theme and any Steelbore-provided themes use color values drawn from the Material Design color system (github.com/material-components). This applies to the boot menu’s accent bar, selection highlight, scrollbar tint, and editor syntax-highlighting palette. Since ZAMAK renders to a framebuffer terminal (not a web view), Material Design is used as a color-token reference, not as a component library.`

## **`3.2 Inline Assembly (No Separate .asm Files)`**

`The Steelbore standard mandates that all assembly code must reside within Rust source files. ZAMAK achieves this via Rust’s` core::arch::asm\! `macro for inline assembly and` core::arch::global\_asm\! `for module-level assembly blocks. The rationale is threefold: (a) the assembly is co-located with the Rust code that calls it, improving auditability; (b) the Rust compiler’s register allocator can participate in register selection for inline blocks; and (c) the build system does not need a separate assembler toolchain.`

### *`3.2.1 Assembly Modules in ZAMAK`*

| `Module` | `Arch` | `Assembly Purpose` |
| :---- | :---- | :---- |
| `zamak-stage1/src/mbr.rs` | `x86` | `512-byte MBR boot sector: BIOS INT 13h disk read, stage2 load` |
| `zamak-decompressor/src/entry.rs` | `x86` | `Real-mode → protected → long mode transitions` |
| `zamak-core/src/arch/x86_64/paging.rs` | `x86-64` | `CR3 load, TLB flush, CR4 manipulation` |
| `zamak-core/src/arch/x86_64/smp_trampoline.rs` | `x86-64` | `AP startup trampoline (real → long mode per-core)` |
| `zamak-core/src/arch/aarch64/mmu.rs` | `AArch64` | `TTBR0/TTBR1 load, MAIR_EL1 setup, TLB invalidation` |
| `zamak-core/src/arch/aarch64/psci.rs` | `AArch64` | `SMC/HVC calls for SMP bring-up` |
| `zamak-core/src/arch/riscv64/satp.rs` | `RISC-V` | `satp register write, sfence.vma` |
| `zamak-core/src/arch/riscv64/sbi.rs` | `RISC-V` | `SBI ecall wrappers for hart management` |

`Each module uses` \#\[cfg(target\_arch \= "...")\] `conditional compilation to ensure only the correct architecture’s assembly is included in a given build.`

## **`3.3 License: GPL-3.0-or-later`**

`Every source file in the ZAMAK repository must carry the following SPDX header as the first comment block:`

// SPDX-License-Identifier: GPL-3.0-or-later

// SPDX-FileCopyrightText: 2026 Mohamed Hammad

`The full license text is provided in` LICENSE `at the repository root. The copyright notice (`© 2026 Mohamed Hammad`) must appear in the` LICENSE `preamble, in every source file via the SPDX header above, and in the` README.md`. All third-party dependencies must be compatible with GPL-3.0-or-later; the CI pipeline includes a` cargo-deny `check that rejects any crate with an incompatible license.`

## **`3.4 SPDX (Software Package Data Exchange)`**

`ZAMAK produces a machine-readable Software Bill of Materials (SBOM) in SPDX 2.3 format for every release. The SBOM is generated by the CI/CD pipeline and includes:`

* `Package identity: name, version (SemVer), download URL, Git SHA.`

* `Copyright text: “© 2026 Mohamed Hammad” for all ZAMAK-authored files.`

* `License declared and concluded for every source file and dependency.`

* `Relationship graph: ZAMAK → dependency crates → transitive crates.`

* `File checksums (SHA-256) for the final binary artifacts.`

* `SPDX document creation info: tool identity, creation timestamp (ISO 8601).`

`The SBOM file is named` zamak-\<version\>.spdx.json `and is published alongside the release binaries. Validation is performed using` spdx-tools `(spdx.dev).`

## **`3.5 Rust Guidelines`**

`All ZAMAK code must conform to the Pragmatic Rust Guidelines as forked by the Steelbore project (github.com/UnbreakableMJ/rust-guidelines), which extends Microsoft’s Pragmatic Rust Guidelines. Key mandates include:`

* `All public APIs must be documented with doc-comments including examples.`

* `No unhandled panics in library code; use Result<T, E> propagation.`

* `Unsafe code is confined to clearly-marked #[zamak_unsafe] boundaries with SAFETY comments.`

* `All integer arithmetic on addresses and sizes must use checked_* or wrapping_* methods.`

* `Clippy must pass at the warn level with no allowed lints suppressed globally.`

* `Code formatting via rustfmt with the project’s .rustfmt.toml configuration.`

## **`3.6 POSIX Compatibility`**

`ZAMAK’s host-side tooling (the` zamak `CLI for MBR installation, config hash enrollment, and SBOM generation) must build and run on any POSIX-compliant system. This means:`

* `No Windows-only APIs in the host tool; filesystem paths use forward slashes internally.`

* `Build system (Cargo + build.rs) uses only POSIX shell commands for scripting hooks.`

* `The CI matrix includes Linux (x86-64, AArch64), macOS (AArch64), and FreeBSD (x86-64).`

`Note: the bootloader firmware binary itself is a freestanding (`\#\!\[no\_std\]`,` \#\!\[no\_main\]`) artifact that runs on bare metal and is not POSIX-related. This requirement applies strictly to the host-side components.`

## **`3.7 ISO 8601 Date Format`**

`Every timestamp produced or consumed by ZAMAK must conform to ISO 8601. This includes:`

* `SPDX SBOM creation timestamps: YYYY-MM-DDThh:mm:ssZ.`

* `Git tag annotations and CHANGELOG entries: YYYY-MM-DD.`

* `Config editor display (if date/time is shown): YYYY-MM-DD hh:mm UTC.`

* `Log output from the zamak CLI: ISO 8601 with timezone offset.`

`No ambiguous date formats (e.g., MM/DD/YYYY or DD.MM.YYYY) shall appear in any ZAMAK output, documentation, or source comment.`

## **`3.8 Material Design (Where Applicable)`**

`Material Design (github.com/material-components) is referenced in two specific contexts within ZAMAK:`

5. `Color tokens: the default theme palette is derived from Material Design’s color system (Blue 800 primary, Grey 900 on-surface, Red 700 error, etc.).`

6. `If ZAMAK’s host CLI ever ships a TUI (terminal UI) mode for interactive configuration, it should follow Material Design’s layout and typography principles adapted for text mode.`

`Material Design component libraries (Flutter, MDC-Web) are not applicable to a bare-metal bootloader and are not used directly. The standard is interpreted as a design-language reference.`

## **`3.9 Assembly Code Memory Safety`**

`The Steelbore standard requires all assembly to live inside Rust source files (Section 3.2). This companion section defines the mandatory techniques for making those assembly boundaries as memory-safe as mechanically possible. Assembly code cannot be verified by the Rust compiler; these requirements compensate by shrinking the unverifiable surface area, enforcing invariants at the type level, and validating behavior at test time.`

### *`3.9.1 Principle: Minimize Assembly Surface Area`*

`The most effective safety technique is to write as little assembly as possible. Every instruction expressible in Rust is an instruction the compiler can verify. Assembly modules shall contain only instructions with no Rust equivalent: privileged register writes (`CR3`,` satp`,` TTBR1\_EL1`), CPU mode transitions (real → protected → long), and special instructions (`wrmsr`,` invlpg`,` ecall`,` smc`). All surrounding logic—computing values, validating inputs, handling errors—must remain in Rust.`

**`Metric:`** `each` asm\! `block shall not exceed 20 instructions. Blocks exceeding this limit must be justified in a design review and split if possible.`

### *`3.9.2 Requirement: Safe Rust Wrappers Over Unsafe Assembly`*

`Every` asm\! `/` global\_asm\! `block must be wrapped in a function whose public signature is safe. The` unsafe `block is internal to the wrapper. Callers never interact with` asm\! `directly.`

`The wrapper function must validate all preconditions before entering the` unsafe `block. Example pattern:`

pub fn load\_page\_table(addr: PageAlignedPhysAddr) {

    // Precondition enforced by the PageAlignedPhysAddr type

    unsafe {

        asm\!("mov cr3, {}", in(reg) addr.as\_u64(),

             options(nostack, preserves\_flags));

    }

}

`Callers use` load\_page\_table()`—a safe function—and never touch` asm\! `themselves. This pattern is mandatory for every assembly boundary in ZAMAK.`

### *`3.9.3 Requirement: Newtype Wrappers for Hardware Constraints`*

`Raw integer types (`u64`,` usize`) must never be passed directly to assembly wrappers for addresses, register values, or hardware-constrained quantities. Instead, ZAMAK defines newtype wrappers that enforce invariants at construction time:`

| `Newtype` | `Inner` | `Invariant Enforced at Construction` |
| :---- | :---- | :---- |
| `PageAlignedPhysAddr` | `u64` | `addr & 0xFFF == 0 (4 KiB alignment)` |
| `PhysAddr` | `u64` | `addr < MAX_PHYS_ADDR for the architecture` |
| `VirtAddr` | `u64` | `Canonical form (sign-extended bits 48/57+)` |
| `TrampolineAddr` | `u64` | `addr < 0x100000 (below 1 MiB for real-mode APs)` |
| `Cr3Value` | `u64` | `Bits 0–11 are valid flags; bits 12+ are page-aligned` |
| `MairValue` | `u64` | `All 8 attribute fields are valid MAIR encodings` |
| `SatpValue` | `u64` | `Mode field matches a supported Sv mode (39/48/57)` |

`Construction via` ::new() `returns` Result\<Self, InvalidHwValue\>`, making invalid hardware values unrepresentable. The` unsafe `block inside the assembly wrapper does not need a runtime assert because the type already guarantees the invariant.`

### *`3.9.4 Requirement: Symbolic Operands via asm! (Prefer Over Hardcoded Registers)`*

`The` asm\! `macro’s operand system allows the Rust compiler’s register allocator to participate in register selection. ZAMAK requires that all` asm\! `blocks use symbolic operands wherever possible:`

// CORRECT: compiler tracks the register and validates the type

asm\!("mov cr3, {val}", val \= in(reg) phys\_addr,

     options(nostack, preserves\_flags));

// PROHIBITED: hardcoded register, compiler learns nothing

asm\!("mov cr3, rax", in("rax") phys\_addr);

`Hardcoded register names are only permitted when the ISA requires a specific register (e.g.,` ecx `for` wrmsr `on x86, or` x0`–`x3 `for PSCI` smc `on AArch64).`

### *`3.9.5 Requirement: asm! Options on Every Block`*

`Every` asm\! `invocation must include the most restrictive applicable` options(...) `flags. These communicate constraints to the compiler and reduce the risk of miscompilation around the assembly block:`

| `Option` | `Meaning` | `When to Apply` |
| :---- | :---- | :---- |
| `nostack` | `Assembly does not touch the stack` | `Almost always; omit only for blocks that push/pop` |
| `preserves_flags` | `Does not modify condition flags (EFLAGS/NZCV)` | `Most register read/write operations` |
| `nomem` | `Does not read or write memory` | `Pure register operations (CPUID, read MSR)` |
| `readonly` | `Reads memory but does not write` | `Memory reads without side effects` |
| `noreturn` | `Control never returns to Rust` | `Final jump to kernel entry point` |

`Every option that can truthfully be added must be added. Omitting an applicable option is a code-review finding.`

### *`3.9.6 Requirement: Structured SAFETY Contracts`*

`The Steelbore standard requires` // SAFETY: `comments on all` unsafe `blocks. For assembly boundaries, ZAMAK elevates this to a structured contract format with four mandatory sections:`

// SAFETY:

//   Preconditions:

//     \- \`stack\_top\` is a valid, mapped, writable address

//     \- \`stack\_top\` is 16-byte aligned (SysV ABI)

//     \- \`entry\` points to a valid kernel entry function

//   Postconditions:

//     \- Control never returns (noreturn)

//   Clobbers:

//     \- All general-purpose registers

//   Worst-case on violation:

//     \- Triple fault / immediate machine reset

`The four sections (Preconditions, Postconditions, Clobbers, Worst-case on violation) are mandatory. During code review, the reviewer verifies that calling code establishes every precondition—not that the assembly itself is “correct” in isolation.`

### *`3.9.7 Requirement: Compile-Time Layout Assertions for Shared Structures`*

`When assembly must access memory through a shared data structure (e.g., the SMP trampoline data block), the structure must be defined as a` \#\[repr(C)\] `Rust struct, and all field offsets used in assembly must be validated at compile time:`

\#\[repr(C, align(4096))\]

struct TrampolineData {

    page\_table:  u64,   // offset 0

    stack\_top:   u64,   // offset 8

    entry\_point: u64,   // offset 16

    cpu\_id:      u32,   // offset 24

}

// Compile-time assertions — build fails if struct layout changes

const\_assert\!(offset\_of\!(TrampolineData, page\_table) \== 0);

const\_assert\!(offset\_of\!(TrampolineData, stack\_top) \== 8);

const\_assert\!(offset\_of\!(TrampolineData, entry\_point) \== 16);

const\_assert\!(offset\_of\!(TrampolineData, cpu\_id) \== 24);

const\_assert\!(size\_of::\<TrampolineData\>() \== 4096);

`If a contributor reorders the struct fields, the build breaks instantly instead of silently corrupting memory at boot time. This pattern is mandatory for every structure accessed by assembly.`

### *`3.9.8 Requirement: Linker-Section Isolation for Position-Sensitive Code`*

`Code that must reside at a specific physical address (stage1 MBR, SMP trampoline) must be placed in dedicated linker sections via` global\_asm\! `with` .pushsection `/` .popsection`. The size and placement must be queryable from Rust:`

global\_asm\!(

    ".pushsection .trampoline, \\"ax\\"",

    ".code16",

    "trampoline\_start:",

    // ... real-mode AP startup code ...

    "trampoline\_end:",

    ".popsection",

);

extern "C" { static trampoline\_start: u8; static trampoline\_end: u8; }

fn trampoline\_size() \-\> usize {

    unsafe { \&trampoline\_end as \*const u8 as usize

           \- \&trampoline\_start as \*const u8 as usize }

}

`Rust code copies exactly` trampoline\_size() `bytes to the <1 MiB target address—no hardcoded magic sizes. If the assembly grows or shrinks, the Rust side adapts automatically.`

### *`3.9.9 Requirement: Post-Assembly Hardware-State Verification in Tests`*

`The QEMU-based test harness (`zamak-test`) must include integration tests that exercise every assembly wrapper and verify the resulting hardware state from Rust. Examples:`

| `Assembly Operation` | `Post-Verification Test` |
| :---- | :---- |
| `load_page_table(pt)` | `read_cr3() & !0xFFF == pt.as_u64()` |
| `write_mair(val)` | `read_mair() == val.as_u64()` |
| `set_satp(mode, ppn)` | `read_satp() == expected_satp_encoding` |
| `smp_wake_ap(cpu_id)` | `AP increments a shared atomic counter within timeout` |
| `enable_sse()` | `CR4 bit 9 (OSFXSR) is set; CR0 bit 2 (EM) is clear` |

`These tests run under QEMU on every CI build. A failing post-verification test is treated as a P0 (release-blocking) defect.`

### *`3.9.10 Requirement: Miri Coverage for All Non-Assembly Code`*

`Miri (Rust’s undefined-behavior detector) cannot interpret inline assembly. ZAMAK’s architecture compensates by structuring code so that assembly wrappers are thin—the bulk of logic (address computation, structure packing, validation) is in pure Rust that Miri can fully check. The CI pipeline runs` cargo \+nightly miri test `on the` zamak-core `crate with all` asm\! `blocks stubbed out via a` \#\[cfg(miri)\] `feature flag that replaces them with no-ops or mock returns.`

**`Coverage target:`** `Miri must execute ≥ 90% of lines in` zamak-core `(excluding arch-specific modules that are` asm\!`-only).`

### *`3.9.11 Summary: Assembly Safety Layers`*

`The following table summarizes the defense-in-depth strategy, from outermost (cheapest) to innermost (most expensive):`

| `Layer` | `Technique` | `What It Catches` |
| :---- | :---- | :---- |
| `L0 — Design` | `Minimize asm! surface (≤ 20 instructions)` | `Eliminates risk by not writing assembly at all` |
| `L1 — Type system` | `Newtype wrappers (PageAlignedPhysAddr, etc.)` | `Invalid values caught at construction time` |
| `L2 — Compiler` | `Symbolic asm! operands + options()` | `Register conflicts, incorrect clobbers, reordering bugs` |
| `L3 — Static analysis` | `const_assert! on struct offsets + sizes` | `Layout mismatches between Rust structs and asm offsets` |
| `L4 — Code review` | `Structured SAFETY contracts (4 sections)` | `Precondition violations, missing clobber declarations` |
| `L5 — Dynamic analysis` | `Miri on non-asm code (≥ 90% coverage)` | `UB in address math, aliasing violations, uninit reads` |
| `L6 — Integration test` | `Post-asm hardware verification under QEMU` | `Incorrect register writes, failed mode transitions` |
| `L7 — Fuzzing` | `Differential fuzzing of config parser + PMM` | `Edge cases in the Rust logic surrounding asm boundaries` |

`No single layer is sufficient. Together, they reduce the “trusted computing base” of unverified code to the minimum possible number of machine instructions, and verify everything around those instructions with Rust’s full power.`

# **`4. System Architecture`**

## **`4.1 Crate Topology`**

`ZAMAK is structured as a Cargo workspace with the following crates:`

| `Crate` | `Type` | `Description` |
| :---- | :---- | :---- |
| `zamak-stage1` | `[[bin]] (x86 only)` | `512-byte MBR boot sector. Pure global_asm!. Loads stage2.` |
| `zamak-decompressor` | `[[bin]] (x86 only)` | `BIOS stage2 decompressor. Inflates zamak-bios.sys.` |
| `zamak-core` | `#![no_std] lib` | `Shared bootloader logic: config, PMM, VMM, ELF loader, ACPI, SMP, terminal, menu.` |
| `zamak-bios` | `[[bin]]` | `BIOS stage3 entry point. Links zamak-core.` |
| `zamak-uefi` | `[[bin]]` | `UEFI application entry point. Links zamak-core.` |
| `zamak-proto` | `#![no_std] lib` | `Limine Protocol types and request/response structures. Standalone crate.` |
| `zamak-cli` | `[[bin]]` | `Host-side CLI tool. MBR install, hash enrollment, SBOM gen. POSIX-compatible.` |
| `zamak-theme` | `#![no_std] lib` | `Theme file parser and color-token resolver. Consumed by zamak-core.` |
| `zamak-test` | `integration test` | `Boot conformance test harness. Runs under QEMU with serial capture.` |

## **`4.2 Boot Paths`**

### *`4.2.1 BIOS Boot Path (x86 Only)`*

`The BIOS boot path mirrors Limine’s three-stage chain, reimplemented in Rust:`

7. `Stage 1 (zamak-stage1): 512-byte MBR sector. Written entirely in global_asm!. Loaded by BIOS at 0x7C00. Reads stage2 from disk via INT 13h. Jumps to stage2.`

8. `Stage 2 (zamak-decompressor): Decompresses the zamak-bios.sys blob (miniz_oxide, a pure-Rust replacement for tinf) to a fixed physical address. Transitions from real mode → protected mode → long mode.`

9. `Stage 3 (zamak-bios + zamak-core): Full bootloader logic. Enumerates disks, initializes memory map (INT 15h E820), discovers config, renders menu, loads kernel.`

### *`4.2.2 UEFI Boot Path (All Architectures)`*

`The UEFI path is a single PE/COFF application (`BOOTX64.EFI`,` BOOTAA64.EFI`,` BOOTRISCV64.EFI`,` BOOTLOONGARCH64.EFI`) built from zamak-uefi + zamak-core. It uses UEFI Boot Services for disk I/O, memory map, and GOP framebuffer, then calls` ExitBootServices() `before jumping to the kernel.`

## **`4.3 Feature Map`**

| `Feature` | `Limine Source` | `ZAMAK Crate` | `Parity` |
| :---- | :---- | :---- | :---- |
| `Config parser + macros` | `common/lib/config.c` | `zamak-core::config` | `Full` |
| `Boot menu + editor` | `common/menu.c` | `zamak-core::menu` | `Full` |
| `Flanterm terminal` | `common/lib/flanterm/` | `zamak-core::terminal` | `Full` |
| `PMM (E820 / UEFI GetMemoryMap)` | `common/mm/pmm.s2.c` | `zamak-core::mm::pmm` | `Full` |
| `VMM (page tables, HHDM)` | `common/mm/vmm.c` | `zamak-core::mm::vmm` | `Full` |
| `ELF loader` | `common/lib/elf.c` | `zamak-core::loader::elf` | `Full` |
| `PE loader` | `common/lib/pe.c` | `zamak-core::loader::pe` | `Full` |
| `ACPI discovery` | `common/lib/acpi.c` | `zamak-core::acpi` | `Full` |
| `SMP bring-up` | `common/sys/smp.c` | `zamak-core::smp` | `Full` |
| `Limine Protocol` | `common/protos/limine.c` | `zamak-core::proto::limine` | `Full` |
| `Linux Protocol` | `common/protos/linux.c` | `zamak-core::proto::linux` | `Full` |
| `Multiboot 1/2` | `common/protos/multiboot*.c` | `zamak-core::proto::multiboot` | `Full` |
| `FAT12/16/32 driver` | `common/fs/fat32.c` | `zamak-core::fs::fat` | `Full` |
| `ISO 9660 driver` | `common/fs/iso9660.c` | `zamak-core::fs::iso9660` | `Full` |
| `URI path resolution` | `common/lib/uri.c` | `zamak-core::config::uri` | `Full` |
| `Color theme engine` | `N/A (new)` | `zamak-theme` | `New` |
| `Asm safety framework` | `N/A (new)` | `zamak-core::arch::safety` | `New` |
| `SPDX SBOM generation` | `N/A (new)` | `zamak-cli::sbom` | `New` |

# **`5. Functional Requirements`**

## **`5.1 Configuration System`**

### *`FR-CFG-001: Config File Parsing`*

`ZAMAK must parse` limine.conf `files with byte-identical semantics to Limine v10.x. This includes all three syntactic elements (`\# `comments,` /`-prefixed menu entries with depth encoding,` key: value `assignments), case-insensitive option names, and whitespace trimming.`

### *`FR-CFG-002: Macro System`*

`ZAMAK must support macro definitions (`${NAME}=value`) and single-pass expansion, including the two built-in macros` ${ARCH} `and` ${FW\_TYPE} `with identical value strings.`

### *`FR-CFG-003: Path Resolution`*

`ZAMAK must resolve all Limine URI path types:` boot()`,` hdd(d:p)`,` odd(d:p)`,` guid(uuid)`,` fslabel(label)`, and` tftp(ip)`, including BLAKE2B hash verification via the` \#hash `suffix.`

### *`FR-CFG-004: SMBIOS Configuration`*

`ZAMAK must accept configuration from SMBIOS Type 11 OEM Strings prefixed with` limine:config:`.`

### *`FR-CFG-005: File Discovery`*

`ZAMAK must search for the configuration file in the same order as Limine: SMBIOS → UEFI app directory → standard paths.`

### *`FR-CFG-006: Secure Boot Hash Verification`*

`ZAMAK must support config hash enrollment via` zamak enroll-config`, verify BLAKE2B hash at boot, panic on mismatch, and disable the editor when enrolled.`

### *`FR-CFG-007: Theme File Loading`*

`ZAMAK must load` zamak-theme.toml `from the same partition as` limine.conf`. Missing/malformed themes fall back to built-in defaults with a warning.`

## **`5.2 Boot Protocols`**

### *`FR-PROTO-001: Limine Protocol`*

`Full Limine Protocol implementation with all request/response types and base revisions (0–4+).`

### *`FR-PROTO-002: Linux Boot Protocol`*

`Load Linux kernels via x86 bzImage and ARM64 Image header, passing command line and initramfs modules.`

### *`FR-PROTO-003: Multiboot 1 and 2`*

`Load Multiboot 1/2 compliant kernels with correct info structures and module passing.`

### *`FR-PROTO-004: Chainloading`*

`Chainload EFI applications (UEFI) and BIOS boot sectors (BIOS) with automatic hiding of incompatible entries.`

## **`5.3 Memory Management`**

### *`FR-MM-001: Physical Memory Manager`*

`Normalize firmware memory maps into unified Limine type system, perform overlap resolution, page-alignment sanitization, and provide top-down allocation.`

### *`FR-MM-002: Virtual Memory Manager`*

`Construct architecture-appropriate page tables with HHDM mappings, kernel PHDRs, and framebuffer write-combining.`

### *`FR-MM-003: KASLR`*

`Implement KASLR using RDRAND/RDSEED (x86), firmware RNG, or timer-jitter fallback, with 1 GB alignment.`

## **`5.4 User Interface`**

### *`FR-UI-001: Boot Menu`*

`Render hierarchical boot menu supporting entry selection, directory expansion/collapse, timeout countdown, and wallpaper display.`

### *`FR-UI-002: Config Editor`*

`Interactive config editor with real-time syntax highlighting and validation, F10 to boot, Escape to cancel.`

### *`FR-UI-003: Theming`*

`All visual elements use color tokens from` zamak-theme.toml`; overridable without recompiling.`

## **`5.5 Host Tooling`**

### *`FR-CLI-001: MBR Installation`*

`The` zamak install `command writes the stage1 MBR sector and records stage2 location.`

### *`FR-CLI-002: Config Hash Enrollment`*

`The` zamak enroll-config `command computes BLAKE2B hash and patches it into the EFI binary.`

### *`FR-CLI-003: SBOM Generation`*

`The` zamak sbom `command produces a valid SPDX 2.3 JSON document.`

## **`5.6 Assembly Safety (Functional)`**

### *`FR-ASM-001: Safe Wrapper Mandate`*

`Every` asm\! `block must be enclosed in a function with a safe public signature. No module outside` zamak-core::arch `may contain` asm\! `invocations.`

### *`FR-ASM-002: Newtype Hardware Values`*

`All values passed to assembly wrappers must use newtype wrappers as defined in Section 3.9.3. Raw integer types are prohibited at the assembly boundary.`

### *`FR-ASM-003: Compile-Time Layout Verification`*

`Every` \#\[repr(C)\] `struct accessed by assembly must have` const\_assert\! `validations for all field offsets and the total struct size.`

### *`FR-ASM-004: Post-Assembly Verification Tests`*

`Every assembly wrapper must have a corresponding integration test in` zamak-test `that exercises the wrapper under QEMU and verifies the resulting hardware state.`

# **`6. Non-Functional Requirements`**

## **`6.1 Performance`**

| `Metric` | `Requirement` | `Rationale` |
| :---- | :---- | :---- |
| `Cold boot to menu` | `≤ 105% of Limine` | `Rust zero-cost abstractions should not add measurable overhead` |
| `Menu-to-kernel-entry` | `≤ 110% of Limine` | `Page table construction is compute-bound; bounds checks are minor` |
| `Binary size (x86-64 UEFI)` | `≤ 120% of Limine` | `Monomorphization may increase size; LTO + size opt mitigates` |
| `Peak RAM during boot` | `≤ 110% of Limine` | `Allocator-free #![no_std] core has no hidden heap overhead` |

## **`6.2 Reliability`**

* `ZAMAK must not panic on any valid limine.conf input.`

* `Human-readable error messages on panic (never a bare register dump).`

* `Memory map sanitization handles overlapping, unaligned, and zero-length entries without crashing.`

* `UEFI ExitBootServices() retry logic handles the documented memory-map-change race condition.`

## **`6.3 Security`**

* `All unsafe code annotated with structured // SAFETY: contracts (Section 3.9.6).`

* `No raw pointer arithmetic outside arch/ and mm/ modules.`

* `Config hash enrollment disables the editor, preventing runtime modification.`

* `cargo-deny in CI rejects dependencies with known CVEs or GPL-incompatible licenses.`

* `All dependencies pinned in Cargo.lock; dependabot enabled for security advisories.`

## **`6.4 Portability`**

* `Host CLI compiles on Linux, macOS, FreeBSD, and any POSIX-compliant OS with a Rust toolchain.`

* `Bootloader binaries cross-compiled from any host to any target architecture.`

* `No platform-specific #[cfg] in zamak-core except for target_arch.`

## **`6.5 Maintainability`**

* `Minimum 80% line coverage for zamak-core (cargo-llvm-cov); 90% Miri coverage for non-asm code.`

* `All public types and functions documented with rustdoc; cargo doc produces zero warnings.`

* `Semantic Versioning for all published crates.`

* `CHANGELOG.md follows Keep a Changelog format with ISO 8601 dates.`

# **`7. Configuration Reference`**

`ZAMAK supports 100% of Limine’s configuration options plus the new theme-related options.`

## **`7.1 New Global Options`**

| `Option` | `Values` | `Default` | `Description` |
| :---- | :---- | :---- | :---- |
| `theme` | `Path to .toml file` | `(built-in)` | `Path to zamak-theme.toml. Uses Limine URI syntax.` |
| `theme_variant` | `light / dark` | `dark` | `Selects the active variant from the theme file.` |

## **`7.2 Theme File Schema (zamak-theme.toml)`**

`See Section 3.1.1 for the full token-group table. Example:`

\[surface\]

background \= "000027"

foreground \= "D98E32"

dim \= "A06A20"

bright \= "F5C87A"

\[accent\]

primary \= "4B7EB0"

secondary \= "50FA7B"

error \= "D32F2F"

warning \= "F57F17"

success \= "2E7D32"

\[editor\]

key \= "8BE9FD"

colon \= "50FA7B"

value \= "D98E32"

comment \= "A06A20"

invalid \= "D32F2F"

# **`8. Testing Strategy`**

## **`8.1 Test Levels`**

| `Level` | `Scope` | `Tooling` | `Coverage Target` |
| :---- | :---- | :---- | :---- |
| `Unit tests` | `Individual functions and modules` | `cargo test, proptest` | `≥ 80% line coverage` |
| `Miri` | `Non-asm code UB detection` | `cargo +nightly miri test` | `≥ 90% of zamak-core lines` |
| `Integration tests` | `Cross-module interaction` | `cargo test --test` | `All critical paths` |
| `Boot conformance` | `End-to-end boot per arch` | `zamak-test + QEMU serial` | `All protocols × all archs` |
| `Asm verification` | `Post-asm hardware state` | `zamak-test + QEMU` | `Every asm! wrapper` |
| `Differential fuzzing` | `Config parser vs. Limine` | `cargo-fuzz + libFuzzer` | `72h continuous per release` |
| `Performance` | `Boot time + binary size` | `hyperfine + CI size gate` | `Per-commit on main` |

## **`8.2 CI/CD Pipeline`**

10. `Trigger: every push to main and every pull request.`

11. `cargo fmt --check (fail on unformatted code).`

12. `cargo clippy -- -D warnings (fail on any Clippy warning).`

13. `cargo test across all crates (unit + integration).`

14. `cargo +nightly miri test on zamak-core with asm stubs.`

15. `cargo deny check (license compatibility and CVE audit).`

16. `Cross-compile ZAMAK binaries for all five architectures.`

17. `Binary size gate: fail if > 120% of Limine baseline.`

18. `Boot conformance: QEMU smoke test for x86-64 BIOS, x86-64 UEFI, AArch64 UEFI.`

19. `Assembly verification tests under QEMU.`

20. `SPDX SBOM generation and validation via spdx-tools.`

21. `Publish artifacts and SBOM to release if tagged.`

# **`9. Release and Versioning`**

## **`9.1 Versioning Scheme`**

`ZAMAK uses Semantic Versioning 2.0.0 (SemVer). MAJOR increments on breaking protocol/ABI changes, MINOR on backward-compatible features, PATCH on bug fixes.`

## **`9.2 Release Artifacts`**

| `Artifact` | `Description` |
| :---- | :---- |
| `zamak-bios.sys` | `BIOS stage3 binary (x86/x86-64)` |
| `BOOTX64.EFI` | `UEFI application for x86-64` |
| `BOOTIA32.EFI` | `UEFI application for IA-32` |
| `BOOTAA64.EFI` | `UEFI application for AArch64` |
| `BOOTRISCV64.EFI` | `UEFI application for RISC-V 64` |
| `BOOTLOONGARCH64.EFI` | `UEFI application for LoongArch64` |
| `zamak (CLI binary)` | `Host tool for Linux, macOS, FreeBSD (per-platform)` |
| `zamak-<ver>.spdx.json` | `SPDX 2.3 SBOM for the release` |
| `SHA256SUMS` | `Checksums for all artifacts` |
| `CHANGELOG.md` | `Human-readable release notes (ISO 8601 dates)` |

## **`9.3 Milestone Schedule`**

| `Milestone` | `Target Date` | `Deliverables` |
| :---- | :---- | :---- |
| `M0: Architecture & PRD` | `2026-03-15` | `This PRD, crate skeleton, CI pipeline, compliance checklist` |
| `M1: BIOS Boot (x86-64)` | `2026-06-01` | `Stage1–Stage3 boots Limine Protocol kernel on QEMU` |
| `M2: UEFI Boot (x86-64)` | `2026-08-01` | `UEFI app boots Linux and Limine Protocol kernels` |
| `M3: Config + Menu + Theme` | `2026-10-01` | `Full config parser, menu, editor, theme engine` |
| `M4: Multi-arch` | `2027-01-15` | `AArch64 and RISC-V 64 UEFI boot paths functional` |
| `M5: Feature parity` | `2027-04-01` | `All protocols, KASLR, SMP, ACPI, Multiboot, chainload` |
| `M6: LoongArch64 + polish` | `2027-06-01` | `LoongArch64, performance tuning, documentation` |
| `v1.0.0 release` | `2027-07-01` | `First stable release with full SPDX SBOM` |

# **`10. Glossary`**

| `Term` | `Definition` |
| :---- | :---- |
| `ABI` | `Application Binary Interface. The low-level calling convention between components.` |
| `AP` | `Application Processor. Any CPU core other than the BSP.` |
| `BSP` | `Bootstrap Processor. The first CPU core that executes at power-on.` |
| `E820` | `BIOS interrupt (INT 15h, AX=E820h) enumerating the physical memory map.` |
| `HHDM` | `Higher Half Direct Map. Contiguous virtual mapping of all physical memory.` |
| `KASLR` | `Kernel Address Space Layout Randomization.` |
| `MBR` | `Master Boot Record. First 512-byte sector containing bootstrap code.` |
| `MAIR` | `Memory Attribute Indirection Register (AArch64). Defines caching policies.` |
| `PAT` | `Page Attribute Table (x86). Controls per-page memory caching behavior.` |
| `PMM` | `Physical Memory Manager. Tracks and allocates physical memory pages.` |
| `PSCI` | `Power State Coordination Interface (ARM). Firmware CPU management.` |
| `RSDP` | `Root System Description Pointer. Entry point to ACPI tables.` |
| `SBOM` | `Software Bill of Materials. Machine-readable component inventory.` |
| `SBI` | `Supervisor Binary Interface (RISC-V). Firmware interface for supervisor mode.` |
| `SMP` | `Symmetric Multi-Processing. Using multiple CPU cores.` |
| `SPDX` | `Software Package Data Exchange. Open SBOM standard.` |
| `TTBR` | `Translation Table Base Register (AArch64). Page table root pointer.` |
| `VMM` | `Virtual Memory Manager. Constructs and manages page tables.` |

# **`11. References`**

| `ID` | `Title` | `URL / Location` |
| :---- | :---- | :---- |
| `REF-01` | `Limine Bootloader Source Repository` | `https://codeberg.org/Limine/Limine.git` |
| `REF-02` | `Limine Boot Protocol Specification` | `PROTOCOL.md in Limine trunk` |
| `REF-03` | `Limine Configuration Reference` | `CONFIG.md in Limine trunk` |
| `REF-04` | `SPDX Specification` | `https://spdx.dev/` |
| `REF-05` | `Pragmatic Rust Guidelines (Steelbore fork)` | `github.com/UnbreakableMJ/rust-guidelines` |
| `REF-06` | `Material Design Components` | `github.com/material-components` |
| `REF-07` | `UEFI Specification 2.10` | `https://uefi.org/specs/UEFI/2.10/` |
| `REF-08` | `Multiboot 1 Specification` | `GNU GRUB documentation` |
| `REF-09` | `Multiboot 2 Specification` | `GNU GRUB documentation` |
| `REF-10` | `ISO 8601:2019 Date and Time Format` | `https://www.iso.org/iso-8601` |
| `REF-11` | `GNU GPL Version 3` | `https://www.gnu.org/licenses/gpl-3.0.html` |
| `REF-12` | `Rust Inline Assembly Reference` | `https://doc.rust-lang.org/reference/inline-assembly.html` |

# **`12. Revision History`**

| `Version` | `Date` | `Author` | `Changes` |
| :---- | :---- | :---- | :---- |
| `1.0.0` | `2026-02-18` | `Mohamed Hammad` | `Initial PRD. Sections 1–11 covering architecture, protocols, config, testing, and release plan.` |
| `1.1.0` | `2026-02-19` | `Mohamed Hammad` | `Added Section 3.9 (Assembly Code Memory Safety) with 11 subsections. Added FR-ASM-001 through FR-ASM-004. Applied Steelbore theme to document formatting.` |
| `1.2.0` | `2026-02-19` | `Mohamed Hammad` | `Updated authorship to Mohamed Hammad. Added copyright and legal notices throughout: cover page, copyright page, SPDX headers, footer, license section, and end-of-document notice. Added Section 12 (Revision History).` |
| `1.3.0` | `2026-03-09` | `Mohamed Hammad` | `Changed page size from US Letter to ISO A4 (210 × 297 mm). Forced page background color to Void Navy (#000027). Rescaled all table widths proportionally to fit A4 content area.` |

*`— End of Document —`*

`© 2026 Mohamed Hammad. All rights reserved.`

`Licensed under GPL-3.0-or-later as part of Project Steelbore.`