// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

use alloc::string::String;
use alloc::vec;
#[allow(unused_imports)]
use alloc::vec::Vec;
use zamak_core::fs::{BlockDevice, Error, FileEntry, FileSystem, FileType};

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct BiosParameterBlock {
    pub jmp: [u8; 3],
    pub oem: [u8; 8],
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub fat_count: u8,
    pub root_entries: u16,
    pub total_sectors_16: u16,
    pub media_type: u8,
    pub sectors_per_fat_16: u16,
    pub sectors_per_track: u16,
    pub head_count: u16,
    pub hidden_sectors: u32,
    pub total_sectors_32: u32,
    pub sectors_per_fat_32: u32,
    pub flags: u16,
    pub fat_version: u16,
    pub root_cluster: u32,
    pub fs_info: u16,
    pub backup_boot_sector: u16,
    pub reserved: [u8; 12],
    pub drive_number: u8,
    pub nt_flags: u8,
    pub signature: u8,
    pub volume_id: u32,
    pub volume_label: [u8; 11],
    pub system_id: [u8; 8],
}

#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct DirectoryEntry {
    pub name: [u8; 11],
    pub attributes: u8,
    pub nt_reserved: u8,
    pub creation_time_tenths: u8,
    pub creation_time: u16,
    pub creation_date: u16,
    pub last_access_date: u16,
    pub first_cluster_high: u16,
    pub last_write_time: u16,
    pub last_write_date: u16,
    pub first_cluster_low: u16,
    pub file_size: u32,
}

pub struct Fat32<'a> {
    disk: &'a mut dyn BlockDevice,
    bpb: BiosParameterBlock,
    lba_start: u64,
}

impl<'a> Fat32<'a> {
    pub fn parse(disk: &'a mut dyn BlockDevice, lba_start: u64) -> Result<Self, Error> {
        let mut buffer = [0u8; 512];
        disk.read_sectors(lba_start, 1, &mut buffer)?;
        let bpb = unsafe { *(buffer.as_ptr() as *const BiosParameterBlock) };
        Ok(Self {
            disk,
            bpb,
            lba_start,
        })
    }

    fn first_data_sector(&self) -> u64 {
        self.lba_start
            + self.bpb.reserved_sectors as u64
            + (self.bpb.fat_count as u64 * self.bpb.sectors_per_fat_32 as u64)
    }

    fn cluster_to_lba(&self, cluster: u32) -> u64 {
        self.first_data_sector() + (cluster as u64 - 2) * self.bpb.sectors_per_cluster as u64
    }

    fn next_cluster(&self, cluster: u32) -> Result<u32, Error> {
        let fat_sector =
            self.lba_start + self.bpb.reserved_sectors as u64 + (cluster as u64 * 4 / 512);
        let fat_offset = (cluster as usize * 4) % 512;

        let mut buffer = [0u8; 512];
        self.disk.read_sectors(fat_sector, 1, &mut buffer)?;

        let next = unsafe { *(buffer.as_ptr().add(fat_offset) as *const u32) };
        Ok(next & 0x0FFFFFFF)
    }

    fn find_in_cluster(&self, start_cluster: u32, name: &str) -> Result<DirectoryEntry, Error> {
        let mut cluster = start_cluster;

        loop {
            let lba = self.cluster_to_lba(cluster);
            let mut buffer = vec![0u8; (self.bpb.sectors_per_cluster as usize) * 512];
            self.disk
                .read_sectors(lba, self.bpb.sectors_per_cluster as usize, &mut buffer)?;

            let entries = unsafe {
                core::slice::from_raw_parts(
                    buffer.as_ptr() as *const DirectoryEntry,
                    buffer.len() / core::mem::size_of::<DirectoryEntry>(),
                )
            };

            for entry in entries {
                if entry.name[0] == 0x00 {
                    return Err(Error::FileNotFound);
                }
                if entry.name[0] == 0xE5 {
                    continue;
                } // Deleted

                if self.compare_name(entry, name) {
                    return Ok(*entry);
                }
            }

            cluster = self.next_cluster(cluster)?;
            if cluster >= 0x0FFFFFF8 {
                break;
            }
        }

        Err(Error::FileNotFound)
    }

    fn compare_name(&self, entry: &DirectoryEntry, search: &str) -> bool {
        let mut entry_name = [b' '; 11];
        let mut j = 0;
        for (_i, c) in search.chars().enumerate() {
            if c == '.' {
                j = 8;
                continue;
            }
            if j >= 11 {
                break;
            }
            entry_name[j] = c.to_ascii_uppercase() as u8;
            j += 1;
        }

        entry.name == entry_name
    }
}

impl<'a> FileSystem for Fat32<'a> {
    fn find_file(&self, path: &str) -> Result<FileEntry, Error> {
        let mut current_cluster = self.bpb.root_cluster;
        let parts = path.split('/');

        let mut last_entry = None;

        for part in parts {
            if part.is_empty() {
                continue;
            }
            let entry = self.find_in_cluster(current_cluster, part)?;
            if (entry.attributes & 0x10) != 0 {
                // Directory
                current_cluster =
                    ((entry.first_cluster_high as u32) << 16) | (entry.first_cluster_low as u32);
            }
            last_entry = Some(entry);
        }

        let entry = last_entry.ok_or(Error::FileNotFound)?;
        let file_type = if (entry.attributes & 0x10) != 0 {
            FileType::Directory
        } else {
            FileType::File
        };

        let start_cluster =
            ((entry.first_cluster_high as u32) << 16) | (entry.first_cluster_low as u32);

        Ok(FileEntry {
            name: String::from(path),
            size: entry.file_size as u64,
            file_type,
            opaque_id: start_cluster as u64,
        })
    }

    fn read_file(&self, entry: &FileEntry, buffer: &mut [u8]) -> Result<usize, Error> {
        let mut cluster = entry.opaque_id as u32;
        let mut bytes_left = entry.size as usize; // read all requested if buffer allows
        if bytes_left > buffer.len() {
            bytes_left = buffer.len();
        }

        let mut offset = 0;

        while bytes_left > 0 && cluster < 0x0FFFFFF8 {
            let lba = self.cluster_to_lba(cluster);
            let sectors = self.bpb.sectors_per_cluster as usize;

            // We should read sectors directly to buffer if aligned?
            // For now simple reliable way: read to temp buffer if buffer chunk is small or use direct if logic allows.
            // Let's use a temp buffer matching cluster size for simplicity, then copy.
            // Optimization: Read directly to `buffer[offset..]` if size matches.

            let cluster_size = sectors * 512;
            let mut cluster_buf = vec![0u8; cluster_size];
            self.disk.read_sectors(lba, sectors, &mut cluster_buf)?;

            let to_copy = core::cmp::min(bytes_left, cluster_size);
            buffer[offset..offset + to_copy].copy_from_slice(&cluster_buf[..to_copy]);

            offset += to_copy;
            bytes_left -= to_copy;

            cluster = self.next_cluster(cluster)?;
        }

        Ok(offset)
    }
}
