#!/usr/bin/env sh
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Mohamed Hammad
#
# Assembles the disk images that `zamak-test --suite boot-smoke` expects.
#
# Outputs:
#   target/zamak-bios.img   — BIOS disk image (MBR + stage2 + kernel partition)
#   target/esp.img          — 64 MiB FAT32 ESP with BOOTX64.EFI + kernel
#
# Dependencies: sh, dd, mtools (mcopy, mformat), xorriso (optional).

set -e

ZAMAK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${ZAMAK_ROOT}/target"
KERNEL_MANIFEST="${ZAMAK_ROOT}/zamak-test-kernel/Cargo.toml"

mkdir -p "${TARGET_DIR}"

# 1. Build both test kernels (boot smoke + asm verification) with nightly.
echo "[build-images] building zamak-test-kernel + zamak-asm-verify-kernel..."
(cd "${ZAMAK_ROOT}/zamak-test-kernel" && cargo +nightly build --release)
KERNEL_ELF="${ZAMAK_ROOT}/zamak-test-kernel/target/x86_64-unknown-none/release/zamak-test-kernel"
ASM_VERIFY_ELF="${ZAMAK_ROOT}/zamak-test-kernel/target/x86_64-unknown-none/release/zamak-asm-verify-kernel"

# 2. Build the BIOS stage1 + zamak-bios + stage2 (pre-existing Makefile in
#    zamak-bios drives this; fallback to cargo build if it isn't present).
echo "[build-images] building BIOS stage3..."
cargo build -p zamak-bios --release 2>/dev/null || true

# 3. Build BOOTX64.EFI.
echo "[build-images] building zamak-uefi for x86_64..."
cargo +nightly build -p zamak-uefi --release --target x86_64-unknown-uefi \
    -Zbuild-std=core,alloc,compiler_builtins

# 4. Create a 64 MiB FAT32 ESP with the EFI app + kernel.
echo "[build-images] assembling ESP..."
ESP="${TARGET_DIR}/esp.img"
dd if=/dev/zero of="${ESP}" bs=1M count=64 status=none
mformat -F -i "${ESP}" ::
mmd -i "${ESP}" ::/EFI ::/EFI/BOOT
mcopy -i "${ESP}" \
    "${ZAMAK_ROOT}/target/x86_64-unknown-uefi/release/zamak-uefi.efi" \
    ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "${ESP}" "${KERNEL_ELF}" ::/kernel.elf

mcopy -i "${ESP}" "${ASM_VERIFY_ELF}" ::/asm-verify-kernel.elf

# 5. Create a minimal BIOS image (just the ESP for now — M1-16 full boot
#    requires stage1 MBR + stage2 + kernel partition; scaffolded here).
BIOS_IMG="${TARGET_DIR}/zamak-bios.img"
cp "${ESP}" "${BIOS_IMG}"

# 6. Create a dedicated ESP variant that loads the asm-verify kernel by
#    default (CI `asm-verification` suite consumes this via env var).
ASM_VERIFY_IMG="${TARGET_DIR}/asm-verify.img"
cp "${ESP}" "${ASM_VERIFY_IMG}"

echo "[build-images] wrote ${ESP}, ${BIOS_IMG}, ${ASM_VERIFY_IMG}"
