#!/usr/bin/env bash

# Zamak Hybrid Image Generator
# Creates a GPT disk image bootable via UEFI and BIOS

set -e

IMG="zamak_hybrid.img"
SZ=64M
MBR="mbr.bin"
STAGE2="stage2.bin"
UEFI_LOADER="target/x86_64-unknown-uefi/release/zamak-loader.efi"
KERNEL="BOOT/KERNEL" # Example kernel path
CONFIG="zamak.con"

echo "--- Building Zamak Hybrid Image ---"

# 1. Build components if needed
# (Assuming they are built by the caller or previously)

# 2. Create blank image
echo "Creating $SZ blank image..."
rm -f "$IMG"
truncate -s $SZ "$IMG"

# 3. Create GPT partition table
# - Partition 1: EFI System Partition (48MB)
echo "Creating GPT partitions..."
sgdisk -Z "$IMG"
sgdisk -o "$IMG"
sgdisk -n 1:2048:+48M -t 1:ef00 -c 1:"EFI System Partition" "$IMG"

# 4. Format ESP as FAT32
# We'll use mtools to format the partition directly inside the image
echo "Formatting ESP (FAT32)..."
# Find start and size of partition 1 for mtools
# Start: 2048, End: 100352 (sectors)
PART_START=2048
PART_COUNT=$((100352 - 2048 + 1))

mformat -i "$IMG@@$((PART_START * 512))" -v "ZAMAK_ESP" -F -h 32 -t 32 -n 32 -c 1 ::

# 5. Copy files to ESP
echo "Copying files to ESP..."
mmd -i "$IMG@@$((PART_START * 512))" ::/EFI
mmd -i "$IMG@@$((PART_START * 512))" ::/EFI/BOOT
mcopy -i "$IMG@@$((PART_START * 512))" -s "$UEFI_LOADER" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$IMG@@$((PART_START * 512))" -s "$CONFIG" ::/zamak.con
# (Optional) Create /BOOT directory and copy kernel
mmd -i "$IMG@@$((PART_START * 512))" ::/BOOT
# mcopy -i "$IMG@@$((PART_START * 512))" "$KERNEL" ::/BOOT/KERNEL

# 6. Install BIOS Bootloader
echo "Installing BIOS components..."

# Calculate stage2 size in sectors
ST2_SIZE=$(stat -c%s "$STAGE2")
ST2_SECTORS=$(( (ST2_SIZE + 511) / 512 ))

# Patch MBR with Stage 2 LBA (34) and Size
# LBA is at offset 440 (4 bytes)
# Size is at offset 444 (2 bytes)
# Little endian
printf "\x22\x00\x00\x00" | dd of="$MBR" bs=1 seek=440 conv=notrunc status=none
printf "\\$(printf '%o' $((ST2_SECTORS & 0xFF)))\\$(printf '%o' $((ST2_SECTORS >> 8)))" | dd of="$MBR" bs=1 seek=444 conv=notrunc status=none

# DD MBR to LBA 0 (First 446 bytes to preserve partition table)
dd if="$MBR" of="$IMG" bs=1 count=446 conv=notrunc status=none
# Ensure boot signature is present (sgdisk should have done this, but just in case)
printf "\x55\xaa" | dd of="$IMG" bs=1 seek=510 conv=notrunc status=none

# DD Stage 2 to LBA 34 (after GPT header/table)
dd if="$STAGE2" of="$IMG" bs=512 seek=34 conv=notrunc status=none

echo "--- Success: $IMG is ready ---"
