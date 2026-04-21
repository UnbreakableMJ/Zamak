// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! ISO 9660 (ECMA-119) read-only filesystem driver.
//!
//! Supports basic ISO 9660 without extensions (no Rock Ridge, no Joliet).
//! Sufficient for reading boot files from CD/DVD media.
//!
//! # Layout
//!
//! ISO 9660 uses 2048-byte logical sectors. The Primary Volume Descriptor
//! (PVD) is at sector 16 (byte offset 32768).

// Rust guideline compliant 2026-03-30

use crate::fs::{BlockDevice, Error, FileEntry, FileSystem, FileType};
use alloc::string::String;
use alloc::vec;

/// ISO 9660 logical sector size (2048 bytes).
const SECTOR_SIZE: usize = 2048;

/// Sector number of the Primary Volume Descriptor.
const PVD_SECTOR: u64 = 16;

/// Volume descriptor type: Primary.
const VD_TYPE_PRIMARY: u8 = 1;

/// Volume descriptor type: Terminator.
const VD_TYPE_TERMINATOR: u8 = 255;

/// Standard identifier for ISO 9660 volume descriptors.
const STANDARD_ID: &[u8; 5] = b"CD001";

/// ISO 9660 filesystem instance.
pub struct Iso9660<'a, D: BlockDevice> {
    device: &'a D,
    root_lba: u32,
    root_size: u32,
    /// Underlying device sector size (typically 512 for BIOS disks).
    device_sector_size: usize,
}

impl<'a, D: BlockDevice> Iso9660<'a, D> {
    /// Mounts an ISO 9660 filesystem from the given block device.
    ///
    /// Reads the Primary Volume Descriptor and extracts the root
    /// directory record.
    ///
    /// # Arguments
    ///
    /// * `device` — Block device to read from.
    /// * `device_sector_size` — Sector size of the device (typically 512 for BIOS).
    pub fn mount(device: &'a D, device_sector_size: usize) -> Result<Self, Error> {
        let sectors_per_iso = SECTOR_SIZE / device_sector_size;
        let mut buf = vec![0u8; SECTOR_SIZE];

        // Scan volume descriptors starting at sector 16.
        let mut vd_sector = PVD_SECTOR;
        loop {
            let dev_sector = vd_sector * sectors_per_iso as u64;
            device.read_sectors(dev_sector, sectors_per_iso, &mut buf)?;

            let vd_type = buf[0];
            let std_id = &buf[1..6];

            if std_id != STANDARD_ID {
                return Err(Error::InvalidFilesystem);
            }

            if vd_type == VD_TYPE_TERMINATOR {
                return Err(Error::InvalidFilesystem);
            }

            if vd_type == VD_TYPE_PRIMARY {
                // Root directory record is at offset 156, 34 bytes.
                let root_record = &buf[156..190];
                let root_lba = read_le32(&root_record[2..6]);
                let root_size = read_le32(&root_record[10..14]);

                return Ok(Self {
                    device,
                    root_lba,
                    root_size,
                    device_sector_size,
                });
            }

            vd_sector += 1;

            // Sanity limit — don't scan forever.
            if vd_sector > 32 {
                return Err(Error::InvalidFilesystem);
            }
        }
    }

    /// Reads a directory at the given LBA and size, looking for a file by name.
    fn find_in_directory(
        &self,
        dir_lba: u32,
        dir_size: u32,
        name: &str,
    ) -> Result<FileEntry, Error> {
        let sectors_per_iso = SECTOR_SIZE / self.device_sector_size;
        let total_sectors = (dir_size as usize).div_ceil(SECTOR_SIZE);
        let mut buf = vec![0u8; SECTOR_SIZE];

        for i in 0..total_sectors {
            let iso_sector = dir_lba as u64 + i as u64;
            let dev_sector = iso_sector * sectors_per_iso as u64;
            self.device
                .read_sectors(dev_sector, sectors_per_iso, &mut buf)?;

            let mut offset = 0;
            while offset < SECTOR_SIZE {
                let record_len = buf[offset] as usize;
                if record_len == 0 {
                    break; // No more entries in this sector.
                }

                if offset + record_len > SECTOR_SIZE {
                    break;
                }

                let record = &buf[offset..offset + record_len];
                let entry = parse_directory_record(record);

                if let Some(ref entry) = entry {
                    // ISO 9660 filenames may have ";1" version suffix.
                    let entry_name_clean = entry.name.trim_end_matches(";1");
                    if entry_name_clean.eq_ignore_ascii_case(name) {
                        return Ok(entry.clone());
                    }
                }

                offset += record_len;
            }
        }

        Err(Error::FileNotFound)
    }

    /// Reads raw data from a contiguous extent on the ISO.
    fn read_extent(&self, lba: u32, size: u32, buffer: &mut [u8]) -> Result<usize, Error> {
        let sectors_per_iso = SECTOR_SIZE / self.device_sector_size;
        let total_bytes = size as usize;
        let total_iso_sectors = total_bytes.div_ceil(SECTOR_SIZE);

        let mut sector_buf = vec![0u8; SECTOR_SIZE];
        let mut bytes_read = 0;

        for i in 0..total_iso_sectors {
            let iso_sector = lba as u64 + i as u64;
            let dev_sector = iso_sector * sectors_per_iso as u64;
            self.device
                .read_sectors(dev_sector, sectors_per_iso, &mut sector_buf)?;

            let remaining = total_bytes - bytes_read;
            let copy_len = remaining.min(SECTOR_SIZE);
            let dest_end = bytes_read + copy_len;

            if dest_end > buffer.len() {
                let actual_copy = buffer.len() - bytes_read;
                buffer[bytes_read..].copy_from_slice(&sector_buf[..actual_copy]);
                return Ok(buffer.len());
            }

            buffer[bytes_read..dest_end].copy_from_slice(&sector_buf[..copy_len]);
            bytes_read += copy_len;
        }

        Ok(bytes_read)
    }
}

impl<'a, D: BlockDevice> FileSystem for Iso9660<'a, D> {
    fn find_file(&self, path: &str) -> Result<FileEntry, Error> {
        let path = path.trim_start_matches('/');
        let parts: alloc::vec::Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        let mut current_lba = self.root_lba;
        let mut current_size = self.root_size;

        for (i, part) in parts.iter().enumerate() {
            let entry = self.find_in_directory(current_lba, current_size, part)?;

            if i < parts.len() - 1 {
                // Intermediate path component — must be a directory.
                if entry.file_type != FileType::Directory {
                    return Err(Error::NotADirectory);
                }
                current_lba = entry.opaque_id as u32;
                current_size = entry.size as u32;
            } else {
                return Ok(entry);
            }
        }

        // Path was empty or root — return root directory entry.
        Ok(FileEntry {
            name: String::from("/"),
            size: self.root_size as u64,
            file_type: FileType::Directory,
            opaque_id: self.root_lba as u64,
        })
    }

    fn read_file(&self, entry: &FileEntry, buffer: &mut [u8]) -> Result<usize, Error> {
        self.read_extent(entry.opaque_id as u32, entry.size as u32, buffer)
    }
}

/// Parses a single ISO 9660 directory record.
fn parse_directory_record(record: &[u8]) -> Option<FileEntry> {
    let record_len = record[0] as usize;
    if record_len < 34 {
        return None;
    }

    let extent_lba = read_le32(&record[2..6]);
    let data_size = read_le32(&record[10..14]);
    let flags = record[25];
    let name_len = record[32] as usize;

    if name_len == 0 || 33 + name_len > record_len {
        return None;
    }

    let name_bytes = &record[33..33 + name_len];

    // Skip "." (0x00) and ".." (0x01) entries.
    if name_len == 1 && (name_bytes[0] == 0x00 || name_bytes[0] == 0x01) {
        return None;
    }

    let name = core::str::from_utf8(name_bytes).unwrap_or("?").to_string();

    let file_type = if flags & 0x02 != 0 {
        FileType::Directory
    } else {
        FileType::File
    };

    Some(FileEntry {
        name,
        size: data_size as u64,
        file_type,
        opaque_id: extent_lba as u64,
    })
}

/// Reads a little-endian u32 from a byte slice.
///
/// ISO 9660 stores multi-byte values in both little-endian and big-endian
/// (both-byte format). We read the little-endian copy at the lower offset.
fn read_le32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

use alloc::string::ToString;
