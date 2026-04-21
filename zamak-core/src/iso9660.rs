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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_le32_matches_from_le_bytes() {
        let b = [0x78, 0x56, 0x34, 0x12];
        assert_eq!(read_le32(&b), 0x1234_5678);
    }

    #[test]
    fn parse_directory_record_extracts_file_entry() {
        let mut rec = alloc::vec![0u8; 40];
        rec[0] = 40; // record_len
        // LBA LE at [2..6].
        rec[2..6].copy_from_slice(&100u32.to_le_bytes());
        // data size LE at [10..14].
        rec[10..14].copy_from_slice(&512u32.to_le_bytes());
        // Flags: 0 = regular file.
        rec[25] = 0;
        // Name length.
        rec[32] = 5;
        rec[33..38].copy_from_slice(b"A.TXT");

        let entry = parse_directory_record(&rec).unwrap();
        assert_eq!(entry.name, "A.TXT");
        assert_eq!(entry.size, 512);
        assert_eq!(entry.opaque_id, 100);
        assert_eq!(entry.file_type, FileType::File);
    }

    #[test]
    fn parse_directory_record_detects_directory_flag() {
        let mut rec = alloc::vec![0u8; 38];
        rec[0] = 38;
        rec[2..6].copy_from_slice(&20u32.to_le_bytes());
        rec[10..14].copy_from_slice(&2048u32.to_le_bytes());
        rec[25] = 0x02; // directory flag
        rec[32] = 3;
        rec[33..36].copy_from_slice(b"DIR");
        let entry = parse_directory_record(&rec).unwrap();
        assert_eq!(entry.file_type, FileType::Directory);
    }

    #[test]
    fn parse_directory_record_skips_dot_and_dotdot() {
        let mut rec = alloc::vec![0u8; 34];
        rec[0] = 34;
        rec[32] = 1;
        rec[33] = 0x00; // "."
        assert!(parse_directory_record(&rec).is_none());
        rec[33] = 0x01; // ".."
        assert!(parse_directory_record(&rec).is_none());
    }

    #[test]
    fn parse_directory_record_rejects_too_short() {
        let rec = [0u8; 33];
        assert!(parse_directory_record(&rec).is_none());
    }

    #[test]
    fn parse_directory_record_rejects_zero_name_len() {
        let mut rec = alloc::vec![0u8; 40];
        rec[0] = 40;
        rec[32] = 0;
        assert!(parse_directory_record(&rec).is_none());
    }

    // ---------------- full-stack mount / find test -----------------

    /// Synthetic ISO 9660 image: a PVD at sector 16 pointing to a
    /// root directory at sector 20; the root dir holds one file
    /// named "BOOT.BIN;1" at sector 30 with 512 bytes of 0xAB.
    struct MockIso {
        sectors: alloc::vec::Vec<[u8; 2048]>,
    }

    impl MockIso {
        fn build() -> Self {
            let mut sectors = alloc::vec![[0u8; 2048]; 64];

            // Sector 16: Primary Volume Descriptor.
            sectors[16][0] = VD_TYPE_PRIMARY;
            sectors[16][1..6].copy_from_slice(STANDARD_ID);
            // Root directory record at offset 156 (34 bytes).
            let root = &mut sectors[16][156..190];
            root[0] = 34; // record length
            root[2..6].copy_from_slice(&20u32.to_le_bytes()); // LBA
            root[10..14].copy_from_slice(&2048u32.to_le_bytes()); // size
            root[25] = 0x02; // directory flag
            root[32] = 1;
            root[33] = 0x00; // self "."

            // Sector 17: Volume Descriptor Terminator (unused by mount()
            // once PVD succeeds, but catches scan-bug regressions).
            sectors[17][0] = VD_TYPE_TERMINATOR;
            sectors[17][1..6].copy_from_slice(STANDARD_ID);

            // Sector 20: root directory with one file record.
            let mut off = 0;
            // "." entry.
            sectors[20][off] = 34;
            sectors[20][off + 2..off + 6].copy_from_slice(&20u32.to_le_bytes());
            sectors[20][off + 10..off + 14].copy_from_slice(&2048u32.to_le_bytes());
            sectors[20][off + 25] = 0x02;
            sectors[20][off + 32] = 1;
            sectors[20][off + 33] = 0x00;
            off += 34;
            // File entry "BOOT.BIN;1".
            let name = b"BOOT.BIN;1";
            let rec_len = 33 + name.len();
            sectors[20][off] = rec_len as u8;
            sectors[20][off + 2..off + 6].copy_from_slice(&30u32.to_le_bytes());
            sectors[20][off + 10..off + 14].copy_from_slice(&512u32.to_le_bytes());
            sectors[20][off + 25] = 0x00;
            sectors[20][off + 32] = name.len() as u8;
            sectors[20][off + 33..off + 33 + name.len()].copy_from_slice(name);

            // Sector 30: file contents.
            sectors[30].fill(0xAB);

            Self { sectors }
        }
    }

    impl BlockDevice for MockIso {
        fn read_sectors(
            &self,
            start_sector: u64,
            count: usize,
            buffer: &mut [u8],
        ) -> Result<(), Error> {
            // Our "device_sector_size" is ISO_SECTOR_SIZE (2048) in the
            // mock — that keeps the test simple: one ISO sector = one
            // device sector.
            for i in 0..count {
                let s = (start_sector as usize) + i;
                if s >= self.sectors.len() {
                    return Err(Error::IoError);
                }
                let dst = &mut buffer[i * 2048..(i + 1) * 2048];
                dst.copy_from_slice(&self.sectors[s]);
            }
            Ok(())
        }
    }

    #[test]
    fn mount_reads_pvd_and_finds_boot_bin() {
        let dev = MockIso::build();
        let fs = Iso9660::mount(&dev, 2048).expect("mount must succeed");
        let entry = fs
            .find_file("/BOOT.BIN")
            .expect("find_file must locate BOOT.BIN");
        assert_eq!(entry.size, 512);
        assert_eq!(entry.file_type, FileType::File);
        let mut buf = alloc::vec![0u8; 512];
        let n = fs.read_file(&entry, &mut buf).expect("read_file must succeed");
        assert_eq!(n, 512);
        assert!(buf.iter().all(|&b| b == 0xAB));
    }

    #[test]
    fn find_missing_file_returns_not_found() {
        let dev = MockIso::build();
        let fs = Iso9660::mount(&dev, 2048).unwrap();
        let err = fs.find_file("/NOPE.TXT").unwrap_err();
        assert!(matches!(err, Error::FileNotFound));
    }

    #[test]
    fn mount_rejects_non_iso_media() {
        struct BadDev;
        impl BlockDevice for BadDev {
            fn read_sectors(
                &self,
                _s: u64,
                count: usize,
                buf: &mut [u8],
            ) -> Result<(), Error> {
                // Return a buffer whose standard ID is not CD001.
                for i in 0..count * 2048 {
                    if i < buf.len() {
                        buf[i] = 0xFF;
                    }
                }
                Ok(())
            }
        }
        match Iso9660::mount(&BadDev, 2048) {
            Err(Error::InvalidFilesystem) => {}
            Err(e) => panic!("expected InvalidFilesystem, got {:?}", e),
            Ok(_) => panic!("expected mount to fail on bogus media"),
        }
    }
}
