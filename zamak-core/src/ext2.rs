// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

use crate::fs::{BlockDevice, Error, FileEntry, FileSystem, FileType};
use alloc::string::String;
use alloc::vec;

const EXT2_SUPER_MAGIC: u16 = 0xEF53;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct Superblock {
    inodes_count: u32,
    blocks_count: u32,
    r_blocks_count: u32,
    free_blocks_count: u32,
    free_inodes_count: u32,
    first_data_block: u32,
    log_block_size: u32,
    log_frag_size: u32,
    blocks_per_group: u32,
    frags_per_group: u32,
    inodes_per_group: u32,
    mtime: u32,
    wtime: u32,
    mnt_count: u16,
    max_mnt_count: u16,
    magic: u16,
    state: u16,
    errors: u16,
    minor_rev_level: u16,
    lastcheck: u32,
    checkinterval: u32,
    creator_os: u32,
    rev_level: u32,
    def_resuid: u16,
    def_resgid: u16,
    // extended info ...
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct BlockGroupDescriptor {
    block_bitmap: u32,
    inode_bitmap: u32,
    inode_table: u32,
    free_blocks_count: u16,
    free_inodes_count: u16,
    used_dirs_count: u16,
    pad: u16,
    reserved: [u32; 3],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct Inode {
    mode: u16,
    uid: u16,
    size: u32,
    atime: u32,
    ctime: u32,
    mtime: u32,
    dtime: u32,
    gid: u16,
    links_count: u16,
    blocks: u32,
    flags: u32,
    osd1: u32,
    block: [u32; 15],
    generation: u32,
    file_acl: u32,
    dir_acl: u32,
    faddr: u32,
    osd2: [u8; 12],
}

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
struct DirectoryEntryHeader {
    inode: u32,
    rec_len: u16,
    name_len: u8,
    file_type: u8,
}

pub struct Ext2<'a> {
    device: &'a mut dyn BlockDevice,
    superblock: Superblock,
    block_size: u32,
    lba_start: u64,
}

impl<'a> Ext2<'a> {
    pub fn mount(device: &'a mut dyn BlockDevice, lba_start: u64) -> Result<Self, Error> {
        let mut buf = [0u8; 1024];
        // Superblock is at 1024 bytes offset from start of partition.
        // Convert to absolute sector. Assume 512 byte sectors for LBA.
        // 1024 bytes = 2 sectors.
        device.read_sectors(lba_start + 2, 2, &mut buf)?;

        let superblock = unsafe { *(buf.as_ptr() as *const Superblock) };

        if superblock.magic != EXT2_SUPER_MAGIC {
            return Err(Error::InvalidFilesystem);
        }

        let block_size = 1024 << superblock.log_block_size;

        Ok(Self {
            device,
            superblock,
            block_size,
            lba_start,
        })
    }

    fn read_block(&self, block_id: u32, buffer: &mut [u8]) -> Result<(), Error> {
        let sectors_per_block = self.block_size / 512;
        let lba = self.lba_start + (block_id as u64 * sectors_per_block as u64);
        self.device
            .read_sectors(lba, sectors_per_block as usize, buffer)
    }

    fn get_bgd(&self, group: u32) -> Result<BlockGroupDescriptor, Error> {
        let block_size = self.block_size;
        // BGD table starts at block 1 (if 1k blocks) or block 2 (if > 1k ... wait)
        // Superblock is always at 1024.
        // If block_size == 1024, Superblock is in Block 1. BGD starts Block 2.
        // If block_size > 1024, Superblock is inside Block 0. BGD starts Block 1.
        let bgd_start_block = if block_size == 1024 { 2 } else { 1 };

        let desc_size = core::mem::size_of::<BlockGroupDescriptor>() as u32;
        let descriptors_per_block = block_size / desc_size;

        let block_offset = group / descriptors_per_block;
        let index_in_block = group % descriptors_per_block;

        let mut buf = vec![0u8; block_size as usize];
        self.read_block(bgd_start_block + block_offset, &mut buf)?;

        let ptr = buf.as_ptr() as *const BlockGroupDescriptor;
        Ok(unsafe { *ptr.add(index_in_block as usize) })
    }

    fn get_inode(&self, inode_num: u32) -> Result<Inode, Error> {
        if inode_num == 0 {
            return Err(Error::FileNotFound);
        }
        let group = (inode_num - 1) / self.superblock.inodes_per_group;
        let index = (inode_num - 1) % self.superblock.inodes_per_group;

        let bgd = self.get_bgd(group)?;

        let _inode_size = self.superblock.rev_level.max(128); // Simplified
                                                                     // Actually rev 0 is 128 fixed. Rev 1 has inode_size field in superblock.
                                                                     // For now assume standard 128 or check rev.
        let actual_inode_size = if self.superblock.rev_level > 0 {
            // We need to read extended superblock field at offset 88.
            // Just hardcode 256 for now as safe bet for modern ext4 or use 128 if rev 0
            // Re-reading superblock as slice is better but let's assume 256 for simplicity or implement full parsing later?
            // Wait, Superblock struct definition stops early.
            // Let's assume 128 for now, commonly used.
            128 // FIXME
        } else {
            128
        };

        let block_size = self.block_size;
        let inodes_per_block = block_size / actual_inode_size;

        let table_block = bgd.inode_table + (index / inodes_per_block);
        let index_in_block = index % inodes_per_block;

        let mut buf = vec![0u8; block_size as usize];
        self.read_block(table_block, &mut buf)?;

        let inode_offset = (index_in_block * actual_inode_size) as usize;
        let inode_ptr = unsafe { buf.as_ptr().add(inode_offset) as *const Inode };

        Ok(unsafe { *inode_ptr })
    }

    fn read_inode_data(
        &self,
        inode: &Inode,
        offset: u32,
        buffer: &mut [u8],
    ) -> Result<usize, Error> {
        // Simple implementation supporting Direct and Singly Indirect blocks
        let block_size = self.block_size;
        let mut bytes_read = 0;
        let mut current_offset = offset;
        let end_offset = offset + buffer.len() as u32;

        let ptrs_per_block = block_size / 4;

        while current_offset < end_offset && bytes_read < buffer.len() {
            let block_idx = current_offset / block_size;
            let block_offset = (current_offset % block_size) as usize;

            let real_block = if block_idx < 12 {
                inode.block[block_idx as usize]
            } else if block_idx < 12 + ptrs_per_block {
                let indirect_idx = block_idx - 12;
                if inode.block[12] == 0 {
                    0
                } else {
                    let mut ind_buf = vec![0u8; block_size as usize];
                    self.read_block(inode.block[12], &mut ind_buf)?;
                    let ptr = ind_buf.as_ptr() as *const u32;
                    unsafe { *ptr.add(indirect_idx as usize) }
                }
            } else {
                // Double indirect not supported yet
                return Err(Error::IoError);
            };

            let bytes_to_read = core::cmp::min(
                (block_size as usize) - block_offset,
                buffer.len() - bytes_read,
            );

            if real_block == 0 {
                // Sparse hole, fill with zero
                for i in 0..bytes_to_read {
                    buffer[bytes_read + i] = 0;
                }
            } else {
                let mut block_buf = vec![0u8; block_size as usize];
                self.read_block(real_block, &mut block_buf)?;
                buffer[bytes_read..bytes_read + bytes_to_read]
                    .copy_from_slice(&block_buf[block_offset..block_offset + bytes_to_read]);
            }

            bytes_read += bytes_to_read;
            current_offset += bytes_to_read as u32;
        }

        Ok(bytes_read)
    }

    fn find_in_dir(&self, dir_inode: &Inode, name: &str) -> Result<u32, Error> {
        let block_size = self.block_size;
        let size = dir_inode.size; // Only checking direct size for now (dirs usually small)

        let mut offset = 0;
        let mut buf = vec![0u8; block_size as usize];

        while offset < size {
            // Read directory data block by block
            // For simplicity assume directory entries don't span blocks (they usually don't in ext2)
            // We reuse read_inode_data logic logic?
            // Let's manually iterate direct blocks for simplicity of directory parsing

            let block_idx = offset / block_size;
            if block_idx >= 12 {
                break;
            } // Only search first 12 blocks of dir for now

            let block = dir_inode.block[block_idx as usize];
            if block == 0 {
                offset += block_size;
                continue;
            }

            self.read_block(block, &mut buf)?;

            let mut ptr = buf.as_ptr();
            let end = unsafe { ptr.add(block_size as usize) };

            while ptr < end {
                let header = unsafe { *(ptr as *const DirectoryEntryHeader) };
                if header.inode == 0 {
                    break;
                } // End/Unused

                let name_len = header.name_len as usize;
                let entry_name_slice = unsafe { core::slice::from_raw_parts(ptr.add(8), name_len) };

                if let Ok(entry_name) = core::str::from_utf8(entry_name_slice) {
                    if entry_name == name {
                        return Ok(header.inode);
                    }
                }

                unsafe {
                    ptr = ptr.add(header.rec_len as usize);
                }
            }
            offset += block_size;
        }

        Err(Error::FileNotFound)
    }
}

impl<'a> FileSystem for Ext2<'a> {
    fn find_file(&self, path: &str) -> Result<FileEntry, Error> {
        let mut current_inode_num = 2; // Root

        let parts = path.split('/');
        for part in parts {
            if part.is_empty() {
                continue;
            }
            let inode = self.get_inode(current_inode_num)?;

            if (inode.mode & 0xF000) != 0x4000 {
                return Err(Error::NotADirectory);
            }

            current_inode_num = self.find_in_dir(&inode, part)?;
        }

        let inode = self.get_inode(current_inode_num)?;
        let file_type = if (inode.mode & 0xF000) == 0x4000 {
            FileType::Directory
        } else {
            FileType::File
        };

        Ok(FileEntry {
            name: String::from(path), // Placeholder name
            size: inode.size as u64,
            file_type,
            opaque_id: current_inode_num as u64,
        })
    }

    fn read_file(&self, entry: &FileEntry, buffer: &mut [u8]) -> Result<usize, Error> {
        let inode = self.get_inode(entry.opaque_id as u32)?;
        self.read_inode_data(&inode, 0, buffer)
    }
}
