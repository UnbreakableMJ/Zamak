// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Concrete-type FAT32 walker over a borrowed byte slice.
//!
//! Used by zamak-bios's M1-16 Path B `kmain` after the real-mode
//! orchestration bulk-loads the boot partition into a contiguous
//! high-memory buffer. The walker is built without trait objects,
//! without packed-struct pointer casts, and without allocations on
//! the parse path because the original `&mut dyn BlockDevice`-backed
//! parser hung when run through nightly's i686 codegen — likely the
//! vtable dispatch in the bump-allocator setting.
//!
//! Inputs: a byte slice that begins at the partition's first sector
//! (BPB at offset 0). The walker only reads the bytes it needs to
//! satisfy each query, so callers can hand it the entire partition
//! image regardless of size.
//!
//! Filename matching supports both 8.3 short names and VFAT long
//! filenames (LFN). Long names are reassembled across consecutive
//! `0x0F`-attribute entries that precede a short-name entry, then
//! compared case-insensitively against the search string. ASCII-only
//! long names are required — non-ASCII LFN segments cause the entry
//! to fall back to short-name matching.

// Rust guideline compliant 2026-03-30

const SECTOR_SIZE: usize = 512;
const DIR_ENTRY_SIZE: usize = 32;
const ATTR_LFN: u8 = 0x0F;
const ATTR_DIRECTORY: u8 = 0x10;

/// FAT32 EOC marker. Any FAT entry with value ≥ this terminates a
/// cluster chain. Sourced from the FAT32 spec, §3.5.
const FAT32_END_OF_CHAIN: u32 = 0x0FFF_FFF8;

/// Maximum LFN segments per name. The VFAT spec caps long names at
/// 255 UCS-2 chars = 20 segments × 13 chars. The walker uses 20-slot
/// buffers throughout.
const MAX_LFN_SEGMENTS: usize = 20;

/// Maximum reassembled long-name length in chars. Equals
/// `MAX_LFN_SEGMENTS * 13`.
const MAX_LFN_CHARS: usize = MAX_LFN_SEGMENTS * 13;

#[derive(Debug, Clone, Copy)]
pub struct DirEntryFacts {
    pub first_cluster: u32,
    pub len: u32,
    pub is_dir: bool,
}

#[derive(Debug)]
pub struct RamFat32<'a> {
    image: &'a [u8],
    sectors_per_cluster: u32,
    reserved_sectors: u32,
    root_cluster: u32,
    first_data_sector: u32,
}

impl<'a> RamFat32<'a> {
    pub fn parse(image: &'a [u8]) -> Option<Self> {
        if image.len() < SECTOR_SIZE {
            return None;
        }
        let bytes_per_sector = read_u16_le(image, 0x0B) as u32;
        if bytes_per_sector != SECTOR_SIZE as u32 {
            return None;
        }
        let sectors_per_cluster = image[0x0D] as u32;
        if sectors_per_cluster == 0 {
            return None;
        }
        let reserved_sectors = read_u16_le(image, 0x0E) as u32;
        if reserved_sectors == 0 {
            return None;
        }
        let fat_count = image[0x10] as u32;
        if fat_count == 0 {
            return None;
        }
        let sectors_per_fat = read_u32_le(image, 0x24);
        if sectors_per_fat == 0 {
            return None;
        }
        let root_cluster = read_u32_le(image, 0x2C);
        if root_cluster < 2 {
            return None;
        }
        if image[0x1FE] != 0x55 || image[0x1FF] != 0xAA {
            return None;
        }
        let fat_total = fat_count.checked_mul(sectors_per_fat)?;
        let first_data_sector = reserved_sectors.checked_add(fat_total)?;
        Some(Self {
            image,
            sectors_per_cluster,
            reserved_sectors,
            root_cluster,
            first_data_sector,
        })
    }

    pub fn find_path(&self, path: &str) -> Option<DirEntryFacts> {
        let mut current = self.root_cluster;
        let mut last: Option<DirEntryFacts> = None;
        for part in path.split('/') {
            if part.is_empty() {
                continue;
            }
            let facts = self.find_in_dir(current, part)?;
            if facts.is_dir {
                current = facts.first_cluster;
            }
            last = Some(facts);
        }
        last
    }

    pub fn read_file(&self, facts: &DirEntryFacts, dest: &mut [u8]) -> usize {
        let total = (facts.len as usize).min(dest.len());
        let cluster_size = self.sectors_per_cluster as usize * SECTOR_SIZE;
        let mut cluster = facts.first_cluster;
        let mut written = 0;
        while written < total && cluster >= 2 && cluster < FAT32_END_OF_CHAIN {
            let bytes = match self.cluster_bytes(cluster) {
                Some(b) => b,
                None => break,
            };
            let take = (total - written).min(cluster_size).min(bytes.len());
            dest[written..written + take].copy_from_slice(&bytes[..take]);
            written += take;
            if written == total {
                break;
            }
            cluster = match self.next_cluster(cluster) {
                Some(c) => c,
                None => break,
            };
        }
        written
    }

    fn sector(&self, lba: u32) -> Option<&[u8]> {
        let start = (lba as usize).checked_mul(SECTOR_SIZE)?;
        let end = start.checked_add(SECTOR_SIZE)?;
        if end > self.image.len() {
            return None;
        }
        Some(&self.image[start..end])
    }

    fn cluster_first_sector(&self, cluster: u32) -> Option<u32> {
        let off = cluster
            .checked_sub(2)?
            .checked_mul(self.sectors_per_cluster)?;
        self.first_data_sector.checked_add(off)
    }

    fn cluster_bytes(&self, cluster: u32) -> Option<&[u8]> {
        let first = self.cluster_first_sector(cluster)?;
        let start = (first as usize).checked_mul(SECTOR_SIZE)?;
        let len = (self.sectors_per_cluster as usize).checked_mul(SECTOR_SIZE)?;
        let end = start.checked_add(len)?;
        if end > self.image.len() {
            return None;
        }
        Some(&self.image[start..end])
    }

    fn next_cluster(&self, cluster: u32) -> Option<u32> {
        let byte_offset = (cluster as usize).checked_mul(4)?;
        let lba = (self.reserved_sectors as usize).checked_add(byte_offset / SECTOR_SIZE)?;
        let ofs = byte_offset % SECTOR_SIZE;
        let sector = self.sector(lba as u32)?;
        if ofs + 4 > sector.len() {
            return None;
        }
        Some(read_u32_le(sector, ofs) & 0x0FFF_FFFF)
    }

    fn find_in_dir(&self, dir_cluster: u32, search: &str) -> Option<DirEntryFacts> {
        let search_bytes = search.as_bytes();
        let search_8_3 = name_to_8_3(search);
        let mut lfn = LfnAccum::new();
        let mut cluster = dir_cluster;
        loop {
            let bytes = self.cluster_bytes(cluster)?;
            let mut ofs = 0;
            while ofs + DIR_ENTRY_SIZE <= bytes.len() {
                let entry = &bytes[ofs..ofs + DIR_ENTRY_SIZE];
                let first = entry[0];
                if first == 0x00 {
                    return None;
                }
                if first == 0xE5 {
                    lfn.reset();
                    ofs += DIR_ENTRY_SIZE;
                    continue;
                }
                let attrs = entry[0x0B];
                if attrs == ATTR_LFN {
                    lfn.absorb(entry);
                    ofs += DIR_ENTRY_SIZE;
                    continue;
                }
                let lfn_match = lfn.matches(search_bytes);
                let sfn_match = entry[..11] == search_8_3;
                if lfn_match || sfn_match {
                    let high = read_u16_le(entry, 0x14) as u32;
                    let low = read_u16_le(entry, 0x1A) as u32;
                    let len = read_u32_le(entry, 0x1C);
                    return Some(DirEntryFacts {
                        first_cluster: (high << 16) | low,
                        len,
                        is_dir: (attrs & ATTR_DIRECTORY) != 0,
                    });
                }
                lfn.reset();
                ofs += DIR_ENTRY_SIZE;
            }
            cluster = self.next_cluster(cluster)?;
            if cluster < 2 || cluster >= FAT32_END_OF_CHAIN {
                return None;
            }
        }
    }
}

struct LfnAccum {
    chars: [u8; MAX_LFN_CHARS],
    char_count: usize,
    valid: bool,
    have_last: bool,
}

impl LfnAccum {
    fn new() -> Self {
        // SAFETY: all fields are POD (`u8` arrays + scalar primitives);
        // the all-zero bit pattern is a valid initial state. Using
        // `mem::zeroed` rather than the array-literal `[0; N]`
        // form lets LLVM lower the init to a single `rep stosb`
        // instead of going through a (potentially missing) compiler
        // builtins helper on the i686-zamak target.
        unsafe { core::mem::zeroed() }
    }

    fn reset(&mut self) {
        self.char_count = 0;
        self.valid = false;
        self.have_last = false;
    }

    fn absorb(&mut self, entry: &[u8]) {
        let seq_byte = entry[0];
        let is_last = (seq_byte & 0x40) != 0;
        let seq = (seq_byte & 0x1F) as usize;
        if seq == 0 || seq > MAX_LFN_SEGMENTS {
            self.reset();
            return;
        }
        // The "last" segment (highest seq) appears first on disk.
        // Subsequent segments must have monotonically decreasing
        // sequence numbers; if not, the chain is corrupt — reset.
        if is_last {
            self.reset();
            self.have_last = true;
            self.valid = true;
            self.char_count = seq * 13;
        } else if !self.have_last {
            return;
        }
        // Place this segment's 13 chars at slot (seq-1)*13.
        let base = (seq - 1) * 13;
        let positions: [usize; 13] = [
            0x01, 0x03, 0x05, 0x07, 0x09, 0x0E, 0x10, 0x12, 0x14, 0x16, 0x18, 0x1C, 0x1E,
        ];
        for (i, &p) in positions.iter().enumerate() {
            let lo = entry[p];
            let hi = entry[p + 1];
            let codepoint = (hi as u16) << 8 | lo as u16;
            let dest = base + i;
            if dest >= MAX_LFN_CHARS {
                self.valid = false;
                return;
            }
            if codepoint == 0x0000 || codepoint == 0xFFFF {
                if dest < self.char_count {
                    self.char_count = dest;
                }
                self.chars[dest] = 0;
                continue;
            }
            if codepoint > 0x7F {
                self.valid = false;
                return;
            }
            self.chars[dest] = codepoint as u8;
        }
    }

    fn matches(&self, search: &[u8]) -> bool {
        if !self.valid || self.char_count == 0 {
            return false;
        }
        if self.char_count != search.len() {
            return false;
        }
        ascii_eq_ignore_case(&self.chars[..self.char_count], search)
    }
}

fn read_u16_le(buf: &[u8], ofs: usize) -> u16 {
    (buf[ofs] as u16) | ((buf[ofs + 1] as u16) << 8)
}

fn read_u32_le(buf: &[u8], ofs: usize) -> u32 {
    (buf[ofs] as u32)
        | ((buf[ofs + 1] as u32) << 8)
        | ((buf[ofs + 2] as u32) << 16)
        | ((buf[ofs + 3] as u32) << 24)
}

fn ascii_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for i in 0..a.len() {
        if ascii_lower(a[i]) != ascii_lower(b[i]) {
            return false;
        }
    }
    true
}

fn ascii_lower(b: u8) -> u8 {
    if (b'A'..=b'Z').contains(&b) {
        b + 32
    } else {
        b
    }
}

fn name_to_8_3(part: &str) -> [u8; 11] {
    // Iterate bytes directly rather than calling `&str::chars()`. The
    // ZAMAK boot path only carries ASCII filenames, and the byte loop
    // dodges nightly i686 codegen quirks observed in the chars()
    // iterator's `next()` during Path B bring-up.
    let mut out = [b' '; 11];
    let mut idx = 0usize;
    let bytes = part.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = bytes[i];
        i += 1;
        if ch == b'.' {
            idx = 8;
            continue;
        }
        if idx >= 11 {
            break;
        }
        out[idx] = if (b'a'..=b'z').contains(&ch) {
            ch - 32
        } else {
            ch
        };
        idx += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;

    /// Synthesizes a 4-sector FAT32 image: BPB at sector 0, FAT at
    /// sector 1, root cluster at sector 2, file cluster at sector 3.
    /// One file: "README  TXT" (8.3-clean), 12 bytes "Hello world!".
    fn build_basic_image() -> Vec<u8> {
        let mut img = vec![0u8; 4 * SECTOR_SIZE];

        // BPB
        img[0x0B] = 0x00;
        img[0x0C] = 0x02;
        img[0x0D] = 1;
        img[0x0E] = 1;
        img[0x10] = 1;
        img[0x24] = 1;
        img[0x2C] = 2;
        img[0x1FE] = 0x55;
        img[0x1FF] = 0xAA;

        // FAT
        let fat = SECTOR_SIZE;
        img[fat + 8] = 0xF8;
        img[fat + 9] = 0xFF;
        img[fat + 10] = 0xFF;
        img[fat + 11] = 0x0F;
        img[fat + 12] = 0xF8;
        img[fat + 13] = 0xFF;
        img[fat + 14] = 0xFF;
        img[fat + 15] = 0x0F;

        // Root cluster: one SFN entry pointing at file in cluster 3
        let root = 2 * SECTOR_SIZE;
        img[root..root + 11].copy_from_slice(b"README  TXT");
        img[root + 0x0B] = 0x00;
        img[root + 0x1A] = 3;
        img[root + 0x1B] = 0;
        img[root + 0x1C] = 12;

        // File cluster
        let file = 3 * SECTOR_SIZE;
        img[file..file + 12].copy_from_slice(b"Hello world!");

        img
    }

    /// Adds an LFN entry pair to the root: long name "zamak.conf"
    /// (10 ASCII chars, single segment) preceding an SFN
    /// "ZAMAK~1 CON" pointing at cluster 4. File contents at cluster
    /// 4 (sector 4) — caller must ensure the image is large enough.
    fn add_zamak_conf_with_lfn(img: &mut Vec<u8>) {
        let extra_clusters = 2;
        let needed = (4 + extra_clusters) * SECTOR_SIZE;
        if img.len() < needed {
            img.resize(needed, 0);
        }

        // FAT[4] = EOC for the conf file (1 cluster).
        let fat = SECTOR_SIZE;
        img[fat + 16] = 0xF8;
        img[fat + 17] = 0xFF;
        img[fat + 18] = 0xFF;
        img[fat + 19] = 0x0F;

        // Root cluster slot 1 (offset 32): LFN segment.
        // Slot 2 (offset 64): the SFN.
        let root = 2 * SECTOR_SIZE;
        let lfn = root + 32;
        let sfn = root + 64;

        // LFN entry. seq=1 with bit 0x40 set → "last" (only segment).
        // Checksum field at 0x0D — we don't validate it so leave 0.
        img[lfn + 0x00] = 0x41;
        img[lfn + 0x0B] = ATTR_LFN;
        let positions: [usize; 13] = [
            0x01, 0x03, 0x05, 0x07, 0x09, 0x0E, 0x10, 0x12, 0x14, 0x16, 0x18, 0x1C, 0x1E,
        ];
        let lfn_chars: [u16; 13] = [
            b'z' as u16,
            b'a' as u16,
            b'm' as u16,
            b'a' as u16,
            b'k' as u16,
            b'.' as u16,
            b'c' as u16,
            b'o' as u16,
            b'n' as u16,
            b'f' as u16,
            0x0000,
            0xFFFF,
            0xFFFF,
        ];
        for (i, &p) in positions.iter().enumerate() {
            let cp = lfn_chars[i];
            img[lfn + p] = (cp & 0xFF) as u8;
            img[lfn + p + 1] = (cp >> 8) as u8;
        }

        // SFN entry "ZAMAK~1 CON".
        img[sfn..sfn + 11].copy_from_slice(b"ZAMAK~1 CON");
        img[sfn + 0x0B] = 0x00;
        img[sfn + 0x1A] = 4;
        img[sfn + 0x1B] = 0;
        img[sfn + 0x1C] = 4;

        // File cluster 4 → sector 4. 4 bytes of "TIME".
        let cf = 4 * SECTOR_SIZE;
        img[cf..cf + 4].copy_from_slice(b"TIME");
    }

    #[test]
    fn parses_basic_bpb() {
        let img = build_basic_image();
        let fs = RamFat32::parse(&img).expect("BPB parse");
        assert_eq!(fs.sectors_per_cluster, 1);
        assert_eq!(fs.reserved_sectors, 1);
        assert_eq!(fs.root_cluster, 2);
        assert_eq!(fs.first_data_sector, 2);
    }

    #[test]
    fn rejects_bad_signature() {
        let mut img = build_basic_image();
        img[0x1FE] = 0;
        assert!(RamFat32::parse(&img).is_none());
    }

    #[test]
    fn rejects_short_image() {
        let img = vec![0u8; 100];
        assert!(RamFat32::parse(&img).is_none());
    }

    #[test]
    fn finds_root_file_via_sfn() {
        let img = build_basic_image();
        let fs = RamFat32::parse(&img).unwrap();
        let facts = fs.find_path("readme.txt").unwrap();
        assert_eq!(facts.first_cluster, 3);
        assert_eq!(facts.len, 12);
        assert!(!facts.is_dir);
    }

    #[test]
    fn reads_file_contents() {
        let img = build_basic_image();
        let fs = RamFat32::parse(&img).unwrap();
        let facts = fs.find_path("README.TXT").unwrap();
        let mut buf = [0u8; 32];
        let n = fs.read_file(&facts, &mut buf);
        assert_eq!(n, 12);
        assert_eq!(&buf[..n], b"Hello world!");
    }

    #[test]
    fn missing_file_returns_none() {
        let img = build_basic_image();
        let fs = RamFat32::parse(&img).unwrap();
        assert!(fs.find_path("nope.txt").is_none());
    }

    #[test]
    fn finds_via_lfn() {
        let mut img = build_basic_image();
        add_zamak_conf_with_lfn(&mut img);
        let fs = RamFat32::parse(&img).unwrap();
        let facts = fs.find_path("zamak.conf").expect("LFN match");
        assert_eq!(facts.first_cluster, 4);
        assert_eq!(facts.len, 4);
        let mut buf = [0u8; 8];
        let n = fs.read_file(&facts, &mut buf);
        assert_eq!(&buf[..n], b"TIME");
    }

    #[test]
    fn lfn_match_is_case_insensitive() {
        let mut img = build_basic_image();
        add_zamak_conf_with_lfn(&mut img);
        let fs = RamFat32::parse(&img).unwrap();
        assert!(fs.find_path("ZAMAK.CONF").is_some());
        assert!(fs.find_path("Zamak.Conf").is_some());
    }

    #[test]
    fn name_to_8_3_pads_correctly() {
        assert_eq!(&name_to_8_3("a.b"), b"A       B  ");
        assert_eq!(&name_to_8_3("readme.txt"), b"README  TXT");
        assert_eq!(&name_to_8_3("kernel.elf"), b"KERNEL  ELF");
    }
}
