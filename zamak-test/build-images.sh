#!/usr/bin/env sh
# SPDX-License-Identifier: GPL-3.0-or-later
# SPDX-FileCopyrightText: 2026 Mohamed Hammad
#
# Assembles the disk images that `zamak-test --suite boot-smoke` and
# `zamak-test --suite asm-verification` expect.
#
# Outputs:
#   target/zamak-bios.img    — BIOS disk image (real M1-16 chain:
#                              MBR stage1 at LBA 0 + zamak-bios stage2
#                              blob at LBA 1 + FAT32 partition with
#                              zamak.conf + kernel.elf)
#   target/esp.img           — 64 MiB FAT32 ESP with BOOTX64.EFI,
#                              zamak.conf, kernel.elf, and
#                              asm-verify-kernel.elf
#   target/asm-verify.img    — ESP variant whose zamak.conf points at
#                              /asm-verify-kernel.elf
#
# Dependencies: sh, dd, mtools (mcopy, mformat, mmd), sfdisk, objcopy.

set -eu

ZAMAK_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${ZAMAK_ROOT}/target"

mkdir -p "${TARGET_DIR}"

# 1. Build both test kernels (boot smoke + asm verification) with nightly.
echo "[build-images] building zamak-test-kernel + zamak-asm-verify-kernel..."
(cd "${ZAMAK_ROOT}/zamak-test-kernel" && cargo +nightly build --release)
KERNEL_ELF="${ZAMAK_ROOT}/zamak-test-kernel/target/x86_64-unknown-none/release/zamak-test-kernel"
ASM_VERIFY_ELF="${ZAMAK_ROOT}/zamak-test-kernel/target/x86_64-unknown-none/release/zamak-asm-verify-kernel"

# 2. Build BOOTX64.EFI (UEFI app).
echo "[build-images] building zamak-uefi for x86_64..."
cargo +nightly build -p zamak-uefi --release --target x86_64-unknown-uefi \
    -Zbuild-std=core,alloc,compiler_builtins \
    -Zbuild-std-features=compiler-builtins-mem

# 3. Build the BIOS boot chain: stage1 MBR + zamak-bios stage2 blob.
#    Both use custom 32-bit i686 target JSONs with rust-lld + panic=abort.
echo "[build-images] building zamak-stage1 (MBR) + zamak-bios (stage2)..."
cargo +nightly build -p zamak-stage1 --release \
    --target "${ZAMAK_ROOT}/zamak-stage1/i686-zamak-stage1.json" \
    -Zjson-target-spec \
    -Zbuild-std=core,compiler_builtins \
    -Zbuild-std-features=compiler-builtins-mem
cargo +nightly build -p zamak-bios --release \
    --target "${ZAMAK_ROOT}/zamak-bios/i686-zamak.json" \
    -Zjson-target-spec \
    -Zbuild-std=core,alloc,compiler_builtins \
    -Zbuild-std-features=compiler-builtins-mem

STAGE1_ELF="${TARGET_DIR}/i686-zamak-stage1/release/zamak-stage1"
STAGE2_ELF="${TARGET_DIR}/i686-zamak/release/zamak-bios"

# 4. Strip ELFs to raw flat binaries. Stage1 must be exactly 512 bytes
#    (enforced by its linker script but we double-check).
STAGE1_BIN="${TARGET_DIR}/zamak-stage1.bin"
STAGE2_BIN="${TARGET_DIR}/zamak-bios.bin"
objcopy -O binary "${STAGE1_ELF}" "${STAGE1_BIN}"
objcopy -O binary "${STAGE2_ELF}" "${STAGE2_BIN}"
STAGE1_SIZE=$(stat -c%s "${STAGE1_BIN}")
if [ "${STAGE1_SIZE}" -ne 512 ]; then
    echo "[build-images] ERROR: stage1 is ${STAGE1_SIZE} bytes, expected 512" >&2
    exit 1
fi

# 5. Emit a minimal zamak.conf (shared with the UEFI ESP).
ZAMAK_CONF="${TARGET_DIR}/zamak.conf"
cat > "${ZAMAK_CONF}" <<'EOF'
TIMEOUT=0
DEFAULT_ENTRY=1

/zamak-test-kernel
    PROTOCOL=limine
    KERNEL_PATH=/kernel.elf
EOF

# 6. asm-verify variant.
ASM_VERIFY_CONF="${TARGET_DIR}/asm-verify.conf"
cat > "${ASM_VERIFY_CONF}" <<'EOF'
TIMEOUT=0
DEFAULT_ENTRY=1

/asm-verify-kernel
    PROTOCOL=limine
    KERNEL_PATH=/asm-verify-kernel.elf
EOF

# 7. Assemble the UEFI ESP (64 MiB FAT32, no partition table — UEFI firmware
#    treats the whole disk as an ESP when it's the only filesystem present).
echo "[build-images] assembling UEFI ESP..."
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

# 8. Assemble the real BIOS image.
#    Layout: LBA 0 stage1, LBA 1..N stage2 (zamak-bios), then a 2 MiB gap
#    and a 32 MiB FAT32 partition holding zamak.conf + kernel.elf.
echo "[build-images] assembling BIOS disk image..."
BIOS_IMG="${TARGET_DIR}/zamak-bios.img"
STAGE2_BYTES=$(stat -c%s "${STAGE2_BIN}")
STAGE2_SECTORS=$(( (STAGE2_BYTES + 511) / 512 ))
# 2 MiB (4096 sectors) of padding before the partition gives sfdisk room
# to align at 2048-sector boundaries regardless of stage2 size.
PART_START_LBA=4096
while [ "${PART_START_LBA}" -le "${STAGE2_SECTORS}" ]; do
    PART_START_LBA=$(( PART_START_LBA + 2048 ))
done
PART_SIZE_MB=32
# Total image = partition start (sectors) + partition size + 2 MiB tail.
IMG_SECTORS=$(( PART_START_LBA + PART_SIZE_MB * 2048 + 4096 ))

dd if=/dev/zero of="${BIOS_IMG}" bs=512 count="${IMG_SECTORS}" status=none
# Stage1 at LBA 0 (includes a zeroed partition-table region 446-509).
dd if="${STAGE1_BIN}" of="${BIOS_IMG}" bs=512 count=1 conv=notrunc status=none
# Stage2 (zamak-bios) starting at LBA 1.
dd if="${STAGE2_BIN}" of="${BIOS_IMG}" bs=512 seek=1 conv=notrunc status=none

# Stamp the MBR partition table first: sfdisk also writes its own
# "disk signature" at 440-443 and reserved bytes at 444-445, which
# collides with zamak-stage1's `.Lstage2_lba`/`.Lstage2_size`
# patchable fields. Do sfdisk before our patches so ours win.
echo "${PART_START_LBA},${PART_SIZE_MB}M,c,*" | \
    sfdisk --no-reread --no-tell-kernel "${BIOS_IMG}" >/dev/null

# Now overwrite 440-443 with stage2 LBA (= 1, u32 LE) and 444-445 with
# stage2 size in sectors (u16 LE). Preserves the partition table at
# 446-509 and the AA55 boot signature at 510.
printf '\1\0\0\0' | \
    dd of="${BIOS_IMG}" bs=1 seek=440 count=4 conv=notrunc status=none
LO=$(( STAGE2_SECTORS & 0xFF ))
HI=$(( (STAGE2_SECTORS >> 8) & 0xFF ))
printf "$(printf '\\%o\\%o' "${LO}" "${HI}")" | \
    dd of="${BIOS_IMG}" bs=1 seek=444 count=2 conv=notrunc status=none

# Format + populate the FAT32 partition via mtools' @@offset syntax
# (offset expressed in sectors with the "S" suffix).
mformat -F -i "${BIOS_IMG}@@${PART_START_LBA}S" ::
mcopy -i "${BIOS_IMG}@@${PART_START_LBA}S" "${ZAMAK_CONF}" ::/zamak.conf
mcopy -i "${BIOS_IMG}@@${PART_START_LBA}S" "${KERNEL_ELF}" ::/kernel.elf

# 9. asm-verify ESP: same as the UEFI ESP but with zamak.conf pointing
#    at the verify kernel by default.
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
