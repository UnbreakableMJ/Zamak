// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::vec::Vec;

#[derive(Debug)]
pub enum Error {
    IoError,
    FileNotFound,
    InvalidFilesystem,
    NotADirectory,
}

pub trait BlockDevice {
    fn read_sectors(&self, start_sector: u64, count: usize, buffer: &mut [u8]) -> Result<(), Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Directory,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: alloc::string::String,
    pub size: u64,
    pub file_type: FileType,
    pub opaque_id: u64, // Inode number or Start Cluster
}

pub trait FileSystem {
    fn find_file(&self, path: &str) -> Result<FileEntry, Error>;
    fn read_file(&self, entry: &FileEntry, buffer: &mut [u8]) -> Result<usize, Error>;
}
