<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- SPDX-FileCopyrightText: 2026 Mohamed Hammad -->

# M1-16 Path B — rearchitect `zamak-bios` to do BIOS I/O in real mode

**Status:** Design doc, not yet implemented. Supersedes Path A
(debug the existing `call_bios_int` trampoline) per the decision
recorded in TODO.md's M1-16 entry. Delete this file when M1-16
flips `[✓]`.

**Reference source tree:** commit `dfc42f0` (v0.8.5).

## 1. Problem recap

`zamak-bios` today:

1. Stage1 (`zamak-stage1`) loads stage2 (zamak-bios binary) at
   physical 0x8000 in 16-bit real mode and jumps there.
2. `zamak-bios/src/entry.rs::_start` enables protected mode via
   CR0.PE and `ljmp 0x08, init_32`.
3. `init_32` loads 32-bit data selectors and calls Rust
   `kmain(drive_id: u8)`.
4. `kmain` (protected mode) needs BIOS services (INT 13h for
   disk, INT 15h for E820, INT 10h for VBE). Today it gets them
   via a 32→16→real→16→32-bit trampoline
   `call_bios_int(int_no, regs)` defined as a `global_asm!` block
   in `entry.rs` (~40 instructions, CR0 dance + GDT reload +
   segment-cache flush + `int 0xNN` + reverse).

**Observed defect:** Post the mode round-trip, `INT 13h AH=0x42`
returns `AH=0x01` (invalid function). The same call works when
issued by stage1 directly in real mode. Trampoline is leaving
some BIOS-required state wrong — most likely candidates: real-mode
IVT not restored, descriptor caches for real-mode segments still
holding 32-bit attributes, A20 / PIC mask drift, or IDTR
pointing at a 32-bit IDT instead of the BIOS one.

**Path A (debug the trampoline)** was explored to the
kill-criteria boundary in an earlier session and stopped without
resolution.

## 2. Path B in one paragraph

Do all BIOS I/O (`INT 13h`/`15h`/`10h`) in real mode **before**
`_start` ever flips CR0.PE. Buffer the collected data (E820
entries, partition table, kernel + config bytes, VBE mode info)
at fixed physical addresses in conventional / extended memory.
Enter protected mode exactly once, with the guarantee that we
never need BIOS again — `kmain` in protected mode works purely
off the pre-loaded buffers and pivots straight to long mode +
kernel handoff via the existing `enter_long_mode` /
`handoff::jump_to_kernel` path. The `call_bios_int` trampoline,
its GDT, and the 32↔real round-trip are deleted.

This matches Limine's own stage3 structure
(`common/lib/real.c`), and eliminates the entire class of bugs
the trampoline produced.

## 3. Real-mode memory map we can use

```
0x00000 – 0x003FF  IVT                (256 × 4 B, leave intact)
0x00400 – 0x004FF  BDA                (read-only for us)
0x00500 – 0x07BFF  ~30 KiB free       scratch/bounce
0x07C00 – 0x07DFF  stage1 MBR         (stale after handoff; reusable)
0x07E00 – 0x07FFF  ~512 B free
0x08000 – 0x???    zamak-bios image   (currently ~85 KiB, grows)
0x???   – 0x7FFFF  ~400 KiB free      ← buffers live here
0x80000 – 0x9FBFF  ~128 KiB free      (EBDA may claim the top)
0x9FC00 – 0xFFFFF  video / ROM        do not touch
```

For anything above 1 MiB (big kernels, initrd for Linux-mode
entries) we need **unreal mode** — flat-limit descriptor caches
on DS/ES/FS/GS while CS stays real-mode. See §6 below.

## 4. New data-handoff struct

`BootDataBundle` lives at a fixed physical address. The
real-mode phase populates it; protected-mode `kmain` consumes
it. `#[repr(C)]` with compile-time offset asserts per §3.9.7.

```rust
#[repr(C, packed)]
pub struct BootDataBundle {
    /// Magic "ZBDL" (0x4C42_445A). `kmain` refuses to proceed
    /// if missing — catches accidental stale bytes.
    pub magic: u32,

    /// BIOS boot drive ID (DL from stage1).
    pub boot_drive: u8,

    /// E820 memory map (INT 15h E820). `e820_count` is the
    /// number of valid entries in `e820[]`.
    pub e820_count: u32,
    pub e820: [E820Entry; 128],

    /// Resolved FAT32 partition: starting LBA + type code from
    /// the MBR partition table the real-mode half parsed.
    pub partition_lba: u32,
    pub partition_type: u8,

    /// VBE/GOP framebuffer info — real-mode half sets the mode
    /// and records the resulting framebuffer descriptor.
    pub vbe_info: VbeModeInfo,

    /// `zamak.conf` bytes (always fits in 4 KiB).
    pub config_len: u32,
    pub config: [u8; 4096],

    /// RSDP (ACPI) physical address, discovered by scanning
    /// 0xE0000..0xFFFFF and EBDA. 0 if not found.
    pub rsdp_phys: u64,

    /// SMBIOS entry-point struct physical address (scan
    /// 0xF0000..0xFFFFF for `_SM_` / `_SM3_` anchors).
    pub smbios_phys: u64,

    /// Kernel ELF image. Large — lives above 1 MiB (see §6).
    /// `kernel_phys` is the physical base; `kernel_len` the byte
    /// count. Protected-mode code parses the ELF from there.
    pub kernel_phys: u64,
    pub kernel_len: u64,
}

const _: () = {
    assert!(core::mem::size_of::<BootDataBundle>() < 8192);
};
```

`BootDataBundle` itself is placed at a known conventional-memory
address (proposal: **0x01000**, ~4 KiB above the IVT/BDA scratch
area and well below zamak-bios's image). The protected-mode code
reads from this fixed pointer.

## 5. Real-mode I/O primitives

Each BIOS call gets a thin `global_asm!` wrapper with a C-ABI
signature so Rust-in-real-mode can call them. All live in a new
`zamak-bios/src/rm_io.rs` module.

### 5.1 `rm_disk_read_ext`

```rust
extern "C" {
    /// Reads `count` sectors from `lba` on drive `drive` into
    /// the 20-bit address `dest_segoff` (seg:off as u32, where
    /// segment is the high 16 bits and offset the low 16). Uses
    /// INT 13h AH=0x42 Extended Read.
    ///
    /// Returns 0 on success, the BIOS AH error code on failure.
    pub fn rm_disk_read_ext(drive: u8, lba: u64, count: u16, dest_segoff: u32) -> u8;
}
```

Assembly:

```
rm_disk_read_ext:
    ; prologue saves callee-saves (BP, BX, SI, DI)
    ; build DAP at a fixed scratch location (0x00700)
    ; INT 13h AH=0x42 with DL=drive, DS:SI=DAP
    ; AH=0 → RAX=0; else RAX=AH
    ; epilogue
```

Since we're 16-bit code, follow the 16-bit Sys-V-like calling
convention the compiler emits for `extern "C"` on
`i686-unknown-none`. Arguments on the stack, return in AX.

The `dest_segoff` argument is what INT 13h's DAP already uses
(segment:offset with segment shifted left 4), so the wrapper
just copies it into the DAP directly.

### 5.2 `rm_e820_walk`

```rust
extern "C" {
    /// Walks INT 15h AX=E820h until continuation index = 0.
    /// Writes into the caller's E820Entry array, up to `max`
    /// entries. Returns the count actually written.
    pub fn rm_e820_walk(entries: u32, max: u32) -> u32;
}
```

The standard loop: `ebx = 0`, `mov eax, 0xE820`, `mov ecx, 24`,
`mov edx, 0x534D4150 "SMAP"`, `int 15h`; increment output
pointer by `ecx` bytes each iteration; stop when CF set or
`ebx = 0`.

### 5.3 `rm_vbe_probe` / `rm_vbe_set_mode`

```rust
extern "C" {
    pub fn rm_vbe_info(dest_seg_off: u32) -> u16;  // → status AX
    pub fn rm_vbe_mode_info(mode: u16, dest_seg_off: u32) -> u16;
    pub fn rm_vbe_set_mode(mode: u16) -> u16;
}
```

VBE 3.0: INT 10h AX=4F00h/4F01h/4F02h. The VESA info block and
per-mode info block are BIOS-written at `ES:DI` (set by caller
to a scratch buffer).

### 5.4 `rm_read_byte_high`, `rm_write_byte_high` (unreal-mode ops)

```rust
extern "C" {
    pub fn rm_memcpy_to_high(dest_phys: u64, src_seg_off: u32, len: u32);
    pub fn rm_memcpy_from_high(dest_seg_off: u32, src_phys: u64, len: u32);
}
```

Precondition: unreal mode is active (DS/ES/FS have 4 GiB
limits; see §6). Implementation uses `rep movsb` with a 32-bit
offset via `fs:` prefix — the segment register holds 0 but the
descriptor cache allows 32-bit offsets.

## 6. Unreal mode

Needed for two reasons:

1. **Kernel above 1 MiB.** `BootDataBundle.kernel_phys` sits
   above 1 MiB (say 0x10_0000). Real-mode code (CS still 16-bit)
   still can't `mov [eax]`-style access > 1 MiB, but with unreal
   DS/ES/FS the displacement encoding silently uses a 32-bit
   limit.
2. **Large E820 tables / VBE info** we want to keep out of the
   tight 0x500..0x7BFF scratch window.

Setup sequence (one-shot in `_start`, before the BIOS-I/O
phase):

```
; 1. Load a small GDT with:
;      sel 0x08 = code16 (not used, kept for symmetry)
;      sel 0x10 = data with base=0, limit=0xFFFFFFFF, G=1, D/B=1
cli
lgdt [unreal_gdtr]

; 2. Enter protected mode briefly.
mov eax, cr0
or  eax, 1
mov cr0, eax

; 3. Load flat data selector into FS (the one we'll override with).
mov bx, 0x10
mov fs, bx

; 4. Exit protected mode. CS descriptor cache is unchanged;
;    DS/ES/SS caches are still whatever BIOS set them. Only FS
;    now has the flat limit.
and eax, 0xFFFFFFFE
mov cr0, eax
sti

; 5. Optional: also load DS/ES/GS with the same selector and
;    drop back to real mode to carry the flat limit on them
;    too. This variant breaks BIOS calls that assume DS=0 so
;    we keep FS-only and use explicit fs: prefixes.
```

After this point, inside real-mode asm (`.code16`), we can do:

```
mov esi, <source 16-bit pointer in low memory>
mov edi, <dest 32-bit phys address above 1 MiB>
mov ecx, <bytes>
rep movsb fs:[edi]
```

and the CPU will copy to any 32-bit physical address. BIOS
calls keep working because they use DS/ES, which we didn't
modify.

**Validation:** our real-mode wrappers ALWAYS set the BIOS-call
segment registers (DS, ES) explicitly before `int NN`, so
nothing relies on their initial values. The existing Limine
code does this — follow the same discipline.

## 7. Phase breakdown

### Phase 1 — scaffolding (≈ half a day)

- [ ] Add `zamak-bios/src/rm_io.rs` empty module; register it in
      `main.rs`.
- [ ] Add `BootDataBundle` struct + `E820Entry`/`VbeModeInfo`
      types in `zamak-bios/src/boot_bundle.rs`. All
      `#[repr(C, packed)]`. Compile-time size/offset asserts.
- [ ] Carve out `#[cfg(feature = "legacy_trampoline")]` on the
      existing `call_bios_int` and all of `kmain`'s calls into
      it, so we can land the Path B work incrementally without
      losing the old code. Default OFF.

### Phase 2 — real-mode I/O wrappers (≈ 1 day)

- [ ] `rm_disk_read_ext` global_asm!. Unit-test shape by
      inspecting the emitted bytes (asserts on the DAP layout
      via `const_assert!`).
- [ ] `rm_e820_walk` global_asm!.
- [ ] `rm_vbe_*` global_asm!.
- [ ] Each wrapper returns a status code; Rust callers branch
      on it.

### Phase 3 — unreal mode (≈ 1 day)

- [ ] `unreal_enter()` in `rm_io.rs` as a `global_asm!`+callable
      function. Callable exactly once from `_start`.
- [ ] `rm_memcpy_to_high(dest_phys, src_seg_off, len)` that uses
      `rep movsb fs:[edi]`. Round-tripped via QEMU with a manual
      serial breadcrumb before / after to verify the 32-bit
      destination actually receives the bytes.

### Phase 4 — real-mode FAT32 loader (≈ 1.5 days)

The realm where most iteration will happen.

- [ ] `rm_fat32.rs` with just enough to traverse the tree for a
      known path:
  - `read_mbr_ptable(drive)` → pick the first FAT32 (type 0x0C)
    partition, record starting LBA.
  - `read_bpb(drive, part_lba)` → parse sectors-per-cluster,
    reserved-sectors, FAT size, root-cluster.
  - `find_file(path: &[u8])` → walk root directory (cluster
    chain), match short-name entries, return first-cluster +
    length.
  - `read_file_to(bundle, file_cluster, file_len, dest_phys)` →
    walk FAT, read each cluster into a bounce buffer in low
    memory, `rm_memcpy_to_high` to `dest_phys`.
- [ ] Start with ASCII-only 8.3 filenames to avoid the
      long-filename-entry machinery. `zamak.conf` and
      `kernel.elf` both fit 8.3.
- [ ] Read buffer sizing: the FAT table can be large, but we
      only need cluster entries on demand. Bounce a single
      sector (512 B) per FAT read. Cluster reads can be sized
      up to the sectors-per-cluster bound (typically 8).

### Phase 5 — real-mode orchestration (≈ 0.5 day)

- [ ] Add a Rust function `real_mode_phase(drive_id) ->
      &'static BootDataBundle` called from `_start` assembly
      BEFORE the CR0.PE transition:
  1. `unreal_enter()`.
  2. `rm_e820_walk(&bundle.e820[..], 128)` → fills `e820_count`.
  3. `read_mbr_ptable` → `partition_lba` / `partition_type`.
  4. FAT32 mount + `find_file("zamak.conf")` → read into
     `bundle.config`.
  5. Parse config header lines just enough to find
     `KERNEL_PATH=`. (The full parser runs in protected mode.)
  6. FAT32 `find_file(kernel_path)` → read above 1 MiB at
     `bundle.kernel_phys = 0x100_0000`.
  7. RSDP scan (0xE0000..0xFFFFF + EBDA).
  8. SMBIOS scan (0xF0000..0xFFFFF).
  9. VBE mode probe + set (1024×768×32, fall back to
     800×600×32, then text mode).
- [ ] Rust in real mode: we don't fully trust the toolchain to
      generate clean 16-bit code for all operations, but
      `i686-unknown-none` + `-C relocation-model=static` + `-C
      code-model=kernel` + `global_asm!` for the hot I/O paths
      is enough. Keep the Rust half at parsing + bookkeeping,
      never generate instructions that assume a flat memory
      model (no 32-bit pointers dereferenced as Rust values).

### Phase 6 — collapse `kmain` to protected-mode-only (≈ 0.5 day)

- [ ] Drop the `drive_id: u8` argument. New signature:
      `extern "C" fn kmain(bundle: &'static BootDataBundle) -> !`.
- [ ] Remove `Disk`/`Fat32`-in-protected-mode code from
      `zamak-bios/src/{disk,fat32}.rs` (or keep them behind the
      `legacy_trampoline` feature for now).
- [ ] Config parser, ELF loader, Limine request-scan,
      `fulfill_requests`, `paging::setup_paging`,
      `enter_long_mode` — all take bytes from `bundle` instead
      of calling BIOS.
- [ ] Delete `call_bios_int` entirely (or gate it behind
      `legacy_trampoline`).

### Phase 7 — reintegrate + re-enable smoke (≈ 0.5 day)

- [ ] Restore the `bios-boot-smoke` case in
      `zamak-test/src/main.rs::suites()` — the case definition
      already exists in git history (commit `db68d69`'s
      deletion).
- [ ] Verify `build-images.sh` still produces a valid
      `target/zamak-bios.img` (the disk layout doesn't need to
      change — real-mode Path B reads from the same FAT32
      partition).
- [ ] `cargo run -p zamak-test -- --suite boot-smoke --timeout 30`
      locally; expect `[PASS] bios-boot-smoke` and
      `[PASS] uefi-boot-smoke`.
- [ ] Push; CI `qemu-smoke` job now runs TWO boot-smoke cases
      (BIOS + UEFI) plus `linux-bzimage` and
      `asm-verification`.
- [ ] Flip `M1-16` from `[~]` to `[✓]` in TODO.md. Delete this
      file. Tag a release.

## 8. Risks and mitigations

| Risk | Likelihood | Mitigation |
|---|---|---|
| Unreal-mode setup works on QEMU's SeaBIOS but breaks on OVMF-legacy or real hardware | Medium | Limine's recipe is battle-tested on > 20 years of hardware; copy its GDT and sequencing exactly |
| Rust compiler emits 32-bit instructions (movsxd, etc.) where we expected 16-bit | Medium | Verify with `objdump -d -m i8086` after every phase. Keep Rust-in-real-mode to simple types and no wide arithmetic. Fall back to `global_asm!` for any hot path that miscompiles |
| FAT32 long filenames in the partition break our parser | Low | Filter out VFAT long-name entries (0x0F attribute) and only match 8.3 short names. Config files (`zamak.conf`, `kernel.elf`) are always short-name-compatible |
| Kernel larger than `kernel_buf` size | Low | Pick `kernel_phys = 0x10_0000` with no upper bound — the write goes to any 32-bit physical address. Only a real machine with < 4 MiB conventional memory could fail |
| 16-bit Rust ABI mismatch between `extern "C"` wrappers and the compiler's calling convention | Medium | Match asm prologue/epilogue to `-C code-model=kernel` + `i686-unknown-none` conventions. If unsure, use `nakedfn!` and hand-roll the frame |
| `kmain` signature change breaks other zamak-bios call sites | Low | Path B deletes the `drive_id` path entirely; compiler will catch every stale reference |

### Kill criteria (carried over from Path A)

If after 7 days of focused work `bios-boot-smoke` still doesn't
print `LIMINE_PROTOCOL_OK`, escalate to either:

- Cap scope to loading a FIXED kernel at a FIXED LBA with no
  filesystem — enough for M1-16 to flip `[✓]` but leaves the
  FAT32-in-real-mode piece as future work.
- OR drop M1-16 from the v1.0 release requirements; ship
  UEFI-only. ZAMAK's PRD allows this (§2.1 lists BIOS as
  in-scope but doesn't require it for v1.0 feature parity).

## 9. Files touched (estimate)

| File | Lines added | Lines removed |
|---|---|---|
| `zamak-bios/src/rm_io.rs` (new) | ~400 | 0 |
| `zamak-bios/src/rm_fat32.rs` (new) | ~350 | 0 |
| `zamak-bios/src/boot_bundle.rs` (new) | ~120 | 0 |
| `zamak-bios/src/entry.rs` | ~200 | ~250 (delete `call_bios_int`) |
| `zamak-bios/src/main.rs` | ~150 added, ~400 removed |
| `zamak-bios/Cargo.toml` | `legacy_trampoline` feature | — |
| `zamak-test/src/main.rs` | ~7 (re-enable `bios-boot-smoke`) | 0 |
| `TODO.md` | flip M1-16 | — |
| `CHANGELOG.md` | `[Unreleased]` entry | — |

Net: ~+1000 lines, ~−650 lines.

## 10. Open questions

- **Do we care about Linux-mode BIOS boot?** Today the Linux
  path is x86-64 UEFI-only. If Path B should also support
  `PROTOCOL=linux` via BIOS, the real-mode phase needs to load
  the bzImage + optional initrd, and the protected-mode code
  needs a BIOS-flavored Linux handoff path. Simplest MVP: BIOS
  supports Limine Protocol only; `PROTOCOL=linux` entries panic
  with a clear "Linux via BIOS not yet supported" message. ✓
  recommend this.
- **ZAMAK-specific config extensions that need BIOS data.**
  The theme engine, menu editor, and SMBIOS-injected config
  paths all happen in protected mode from pre-loaded buffers —
  no change. ✓

## 11. Out of scope

- AHCI/NVMe direct disk I/O. Stick with INT 13h Extended Read.
- Secure Boot / measured boot. BIOS doesn't provide the same
  primitives; UEFI-only concern.
- exFAT or ext4 real-mode parsers.
- LBA48 beyond `u32` (INT 13h AH=0x42's DAP has a full `u64`
  anyway; we just pass through).

---

*— End of design doc. Once implemented, delete this file in the
same commit that flips M1-16 to `[✓]` in TODO.md.*
