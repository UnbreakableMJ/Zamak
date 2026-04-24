#!/usr/bin/env sh
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Mohamed Hammad
#
# Assembles the disk images that `zamak-test --suite boot-smoke` and
# `zamak-test --suite asm-verification` expect.
#
# Outputs:
#   target/zamak-bios.img    — BIOS disk image (scaffolded; currently
#                              the same FAT32 ESP as below — M1-16
#                              will replace this with MBR + stage2 +
#                              kernel partition once the full BIOS
#                              boot chain lands)
#   target/esp.img           — 64 MiB FAT32 ESP with BOOTX64.EFI,
#                              zamak.conf, kernel.elf, and
#                              asm-verify-kernel.elf
#   target/asm-verify.img    — ESP variant whose zamak.conf points at
#                              /asm-verify-kernel.elf
#
# Dependencies: sh, dd, mtools (mcopy, mformat, mmd).

set -eu

ZAMAK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${ZAMAK_ROOT}/target"

mkdir -p "${TARGET_DIR}"

# 1. Build both test kernels (boot smoke + asm verification) with nightly.
echo "[build-images] building zamak-test-kernel + zamak-asm-verify-kernel..."
(cd "${ZAMAK_ROOT}/zamak-test-kernel" && cargo +nightly build --release)
KERNEL_ELF="${ZAMAK_ROOT}/zamak-test-kernel/target/x86_64-unknown-none/release/zamak-test-kernel"
ASM_VERIFY_ELF="${ZAMAK_ROOT}/zamak-test-kernel/target/x86_64-unknown-none/release/zamak-asm-verify-kernel"

# 2. Build BOOTX64.EFI (UEFI app). zamak-bios is intentionally NOT
#    built here — M1-16 will wire it in alongside stage1/stage2 when
#    the full BIOS boot chain lands. Skipping keeps this script honest
#    about what it produces.
echo "[build-images] building zamak-uefi for x86_64..."
cargo +nightly build -p zamak-uefi --release --target x86_64-unknown-uefi \
    -Zbuild-std=core,alloc,compiler_builtins \
    -Zbuild-std-features=compiler-builtins-mem

# 3. Emit a minimal zamak.conf that points the menu at /kernel.elf via
#    the Limine Protocol. zamak-uefi searches `\zamak.conf` and
#    `\boot\zamak.conf` on the ESP (main.rs::config_paths); if neither
#    exists the loader falls into an infinite key-wait loop and the
#    QEMU suites time out.
ZAMAK_CONF="${TARGET_DIR}/zamak.conf"
cat > "${ZAMAK_CONF}" <<'EOF'
TIMEOUT=0
DEFAULT_ENTRY=1

/zamak-test-kernel
    PROTOCOL=limine
    KERNEL_PATH=/kernel.elf
EOF

# 4. Same shape for the asm-verify variant.
ASM_VERIFY_CONF="${TARGET_DIR}/asm-verify.conf"
cat > "${ASM_VERIFY_CONF}" <<'EOF'
TIMEOUT=0
DEFAULT_ENTRY=1

/asm-verify-kernel
    PROTOCOL=limine
    KERNEL_PATH=/asm-verify-kernel.elf
EOF

# 5. Create a 64 MiB FAT32 ESP with the EFI app + config + kernel.
echo "[build-images] assembling ESP..."
ESP="${TARGET_DIR}/esp.img"
dd if=/dev/zero of="${ESP}" bs=1M count=64 status=none
mformat -F -i "${ESP}" ::
mmd -i "${ESP}" ::/EFI ::/EFI/BOOT
mcopy -i "${ESP}" \
    "${ZAMAK_ROOT}/target/x86_64-unknown-uefi/release/zamak-uefi.efi" \
    ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "${ESP}" "${ZAMAK_CONF}" ::/zamak.conf
mcopy -i "${ESP}" "${KERNEL_ELF}" ::/kernel.elf
mcopy -i "${ESP}" "${ASM_VERIFY_ELF}" ::/asm-verify-kernel.elf

# 6. BIOS image — currently a copy of the ESP (M1-16 scaffolded). Real
#    BIOS boot chain (stage1 MBR + stage2 decompressor + zamak-bios.sys
#    + kernel partition) will replace this.
BIOS_IMG="${TARGET_DIR}/zamak-bios.img"
cp "${ESP}" "${BIOS_IMG}"

# 7. asm-verify ESP: rebuild from scratch so zamak.conf points at the
#    verify kernel by default.
ASM_VERIFY_IMG="${TARGET_DIR}/asm-verify.img"
dd if=/dev/zero of="${ASM_VERIFY_IMG}" bs=1M count=64 status=none
mformat -F -i "${ASM_VERIFY_IMG}" ::
mmd -i "${ASM_VERIFY_IMG}" ::/EFI ::/EFI/BOOT
mcopy -i "${ASM_VERIFY_IMG}" \
    "${ZAMAK_ROOT}/target/x86_64-unknown-uefi/release/zamak-uefi.efi" \
    ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "${ASM_VERIFY_IMG}" "${ASM_VERIFY_CONF}" ::/zamak.conf
mcopy -i "${ASM_VERIFY_IMG}" "${ASM_VERIFY_ELF}" ::/asm-verify-kernel.elf

echo "[build-images] wrote ${ESP}, ${BIOS_IMG}, ${ASM_VERIFY_IMG}"
