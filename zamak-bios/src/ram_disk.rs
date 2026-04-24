// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! In-memory `BlockDevice` adapter.
//!
//! The real-mode orchestration in `_start` bulk-loads the first N
//! bytes of the boot partition into a high-memory buffer (see
//! `BootDataBundle::partition_image_phys`). Protected-mode kmain then
//! wraps that byte slice in a [`RamDisk`] so the existing
//! `zamak_core::fat32::Fat32` parser — which only knows how to talk to
//! something implementing [`BlockDevice`] — runs against the
//! pre-loaded image without ever calling BIOS again.
//!
//! The adapter is sector-exact: `read_sectors` does a bounds check
//! and returns `Error::IoError` when the caller walks past the end of
//! the loaded image. That surfaces rather than silently filling a
//! buffer with zeros, which would mislead the FAT32 walker into
//! thinking a directory entry starting at `0x00` (free slot) marked
//! the end of the table.

// Rust guideline compliant 2026-04-25

use zamak_core::fs::{BlockDevice, Error};

/// Byte-slice backed `BlockDevice`. The slice is the raw partition
/// image starting at the partition's first sector (i.e. the BPB is at
/// offset 0, not offset 446 as it would be on an MBR).
pub struct RamDisk<'a> {
    image: &'a [u8],
}

impl<'a> RamDisk<'a> {
    pub fn new(image: &'a [u8]) -> Self {
        Self { image }
    }
}

impl<'a> BlockDevice for RamDisk<'a> {
    fn read_sectors(
        &self,
        start_sector: u64,
        count: usize,
        buffer: &mut [u8],
    ) -> Result<(), Error> {
        let bytes_needed = count
            .checked_mul(512)
            .ok_or(Error::IoError)?;
        if buffer.len() < bytes_needed {
            return Err(Error::IoError);
        }
        let start_byte = start_sector
            .checked_mul(512)
            .ok_or(Error::IoError)? as usize;
        let end_byte = start_byte
            .checked_add(bytes_needed)
            .ok_or(Error::IoError)?;
        if end_byte > self.image.len() {
            return Err(Error::IoError);
        }
        buffer[..bytes_needed].copy_from_slice(&self.image[start_byte..end_byte]);
        Ok(())
    }
}
