// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Enrolled config-hash slot (FR-CFG-006).
//!
//! Defines a well-known byte signature followed by a 32-byte hash slot
//! that the ZAMAK bootloader embeds in its binary. The host-side
//! `zamak enroll-config` command locates this signature in the EFI
//! binary and overwrites the slot with a BLAKE2B-256 hash of the
//! config file.
//!
//! At boot, the bootloader reads the slot via [`EnrolledHashSlot::read`].
//! If the slot is non-zero, editor access is disabled and the loaded
//! config is verified against this hash.

// Rust guideline compliant 2026-03-30

/// 16-byte signature marking the enrolled hash slot.
///
/// Chosen to be unique enough that scanning a few-MiB binary will not
/// produce false positives in code/data sections.
pub const ENROLLED_HASH_SIGNATURE: [u8; 16] = [
    b'Z', b'A', b'M', b'A', b'K', b'_', b'C', b'F', b'G', b'_', b'H', b'A', b'S', b'H', 0xA5, 0x5A,
];

/// Length of the hash slot in bytes (BLAKE2B-256).
pub const ENROLLED_HASH_LEN: usize = 32;

/// Total size of the marker + hash slot.
pub const ENROLLED_HASH_RECORD_LEN: usize = 16 + ENROLLED_HASH_LEN;

/// The enrolled hash slot structure — signature followed by the hash.
///
/// A `static` instance of this type is embedded in the bootloader. The
/// host CLI locates it by scanning for [`ENROLLED_HASH_SIGNATURE`] and
/// overwrites the `hash` field.
#[repr(C, align(16))]
pub struct EnrolledHashSlot {
    pub signature: [u8; 16],
    pub hash: [u8; ENROLLED_HASH_LEN],
}

impl EnrolledHashSlot {
    /// Empty (unenrolled) slot: signature populated, hash all zero.
    pub const fn empty() -> Self {
        Self {
            signature: ENROLLED_HASH_SIGNATURE,
            hash: [0u8; ENROLLED_HASH_LEN],
        }
    }

    /// Returns `Some(hash)` if the slot has been enrolled (non-zero),
    /// or `None` if it is still all zero.
    pub fn read(&self) -> Option<[u8; ENROLLED_HASH_LEN]> {
        if self.hash.iter().all(|&b| b == 0) {
            None
        } else {
            Some(self.hash)
        }
    }
}

/// Scans a byte slice for the enrolled hash signature.
///
/// Returns the byte offset of the start of the signature, or `None` if
/// not found. The hash slot itself begins at `offset + 16`.
pub fn find_slot(binary: &[u8]) -> Option<usize> {
    // Signature is 16 bytes; scan at 4-byte stride for alignment-friendly speed.
    if binary.len() < ENROLLED_HASH_RECORD_LEN {
        return None;
    }
    let sig = &ENROLLED_HASH_SIGNATURE;
    binary.windows(16).position(|w| w == sig)
}

/// Reads the enrolled hash directly from a byte slice at a given offset.
///
/// Returns the hash if non-zero, `None` otherwise. Returns `None` if
/// the offset is out of bounds.
pub fn read_hash_at(binary: &[u8], signature_offset: usize) -> Option<[u8; ENROLLED_HASH_LEN]> {
    let hash_start = signature_offset + 16;
    let hash_end = hash_start + ENROLLED_HASH_LEN;
    if hash_end > binary.len() {
        return None;
    }
    let mut hash = [0u8; ENROLLED_HASH_LEN];
    hash.copy_from_slice(&binary[hash_start..hash_end]);
    if hash.iter().all(|&b| b == 0) {
        None
    } else {
        Some(hash)
    }
}

/// Errors from [`patch_hash`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchError {
    /// The hash slot signature was not found in the binary.
    SignatureMissing,
    /// The hash slot extends past the end of the buffer.
    SlotTruncated,
}

/// Overwrites the hash slot in a mutable binary buffer.
pub fn patch_hash(
    binary: &mut [u8],
    hash: &[u8; ENROLLED_HASH_LEN],
) -> Result<(), PatchError> {
    let offset = find_slot(binary).ok_or(PatchError::SignatureMissing)?;
    let hash_start = offset + 16;
    let hash_end = hash_start + ENROLLED_HASH_LEN;
    if hash_end > binary.len() {
        return Err(PatchError::SlotTruncated);
    }
    binary[hash_start..hash_end].copy_from_slice(hash);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_slot_reads_none() {
        let slot = EnrolledHashSlot::empty();
        assert!(slot.read().is_none());
    }

    #[test]
    fn populated_slot_reads_some() {
        let mut slot = EnrolledHashSlot::empty();
        slot.hash[0] = 0xAB;
        assert_eq!(slot.read().map(|h| h[0]), Some(0xAB));
    }

    #[test]
    fn find_slot_locates_signature() {
        let mut buf = alloc::vec![0u8; 128];
        // Place signature at offset 64.
        buf[64..80].copy_from_slice(&ENROLLED_HASH_SIGNATURE);
        assert_eq!(find_slot(&buf), Some(64));
    }

    #[test]
    fn find_slot_returns_none_when_absent() {
        let buf = alloc::vec![0xFFu8; 128];
        assert_eq!(find_slot(&buf), None);
    }

    #[test]
    fn patch_hash_writes_into_slot() {
        let mut buf = alloc::vec![0u8; 128];
        buf[32..48].copy_from_slice(&ENROLLED_HASH_SIGNATURE);

        let hash = [0xAAu8; ENROLLED_HASH_LEN];
        patch_hash(&mut buf, &hash).unwrap();

        // Slot should now contain the hash at offset 48.
        assert_eq!(&buf[48..48 + ENROLLED_HASH_LEN], &hash);
    }

    #[test]
    fn patch_hash_fails_without_signature() {
        let mut buf = alloc::vec![0u8; 128];
        let hash = [0xAAu8; ENROLLED_HASH_LEN];
        assert!(patch_hash(&mut buf, &hash).is_err());
    }

    #[test]
    fn read_hash_at_roundtrip() {
        let mut buf = alloc::vec![0u8; 128];
        buf[16..32].copy_from_slice(&ENROLLED_HASH_SIGNATURE);
        let hash = [0xCDu8; ENROLLED_HASH_LEN];
        patch_hash(&mut buf, &hash).unwrap();

        let read = read_hash_at(&buf, 16).unwrap();
        assert_eq!(read, hash);
    }

    #[test]
    fn read_hash_at_zero_returns_none() {
        let mut buf = alloc::vec![0u8; 128];
        buf[0..16].copy_from_slice(&ENROLLED_HASH_SIGNATURE);
        // hash slot remains zero.
        assert!(read_hash_at(&buf, 0).is_none());
    }

    #[test]
    fn slot_has_correct_size() {
        assert_eq!(
            core::mem::size_of::<EnrolledHashSlot>(),
            ENROLLED_HASH_RECORD_LEN
        );
    }
}
