// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! BLAKE2B cryptographic hash function (RFC 7693).
//!
//! Pure `no_std` implementation for use in config hash verification
//! (`#hash` URI suffix, FR-CFG-003) and `zamak enroll-config` (FR-CLI-002).
//!
//! # Examples
//!
//! ```
//! use zamak_core::blake2b::Blake2b;
//!
//! // `hash` returns the full 64-byte state; only the first `out_len` bytes
//! // (32 here, BLAKE2B-256) are valid output.
//! let full = Blake2b::hash(b"hello world", 32);
//! let digest = &full[..32];
//! assert_eq!(digest.len(), 32);
//! ```

// Rust guideline compliant 2026-03-30

/// BLAKE2B block size in bytes.
const BLOCK_BYTES: usize = 128;

/// Number of rounds in the compression function.
const ROUNDS: usize = 12;

/// BLAKE2B initialization vector (first 8 primes, fractional parts of square roots).
const IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

/// BLAKE2B message permutation schedule (sigma).
const SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

/// BLAKE2B hasher state.
///
/// Supports incremental hashing via [`update`](Blake2b::update) and
/// [`finalize`](Blake2b::finalize), or one-shot via [`hash`](Blake2b::hash).
pub struct Blake2b {
    h: [u64; 8],
    t: [u64; 2],
    buf: [u8; BLOCK_BYTES],
    buf_len: usize,
    out_len: usize,
}

impl Blake2b {
    /// Creates a new BLAKE2B hasher with the given output length (1..=64 bytes).
    ///
    /// # Panics
    ///
    /// Panics if `out_len` is 0 or greater than 64.
    #[must_use]
    pub fn new(out_len: usize) -> Self {
        assert!(
            (1..=64).contains(&out_len),
            "BLAKE2B output length must be 1..=64"
        );

        let mut h = IV;
        // Parameter block: fanout=1, depth=1, digest_length=out_len.
        h[0] ^= 0x0101_0000 ^ (out_len as u64);

        Self {
            h,
            t: [0; 2],
            buf: [0u8; BLOCK_BYTES],
            buf_len: 0,
            out_len,
        }
    }

    /// Feeds data into the hasher.
    pub fn update(&mut self, data: &[u8]) {
        let mut offset = 0;

        // If we have buffered data, try to fill the block.
        if self.buf_len > 0 {
            let space = BLOCK_BYTES - self.buf_len;
            let copy_len = data.len().min(space);
            self.buf[self.buf_len..self.buf_len + copy_len].copy_from_slice(&data[..copy_len]);
            self.buf_len += copy_len;
            offset += copy_len;

            if self.buf_len == BLOCK_BYTES && offset < data.len() {
                self.increment_counter(BLOCK_BYTES as u64);
                let block = self.buf;
                compress(&mut self.h, &block, self.t, false);
                self.buf_len = 0;
            }
        }

        // Process full blocks (keeping at least 1 byte for finalize).
        while offset + BLOCK_BYTES < data.len() {
            self.increment_counter(BLOCK_BYTES as u64);
            compress(
                &mut self.h,
                &data[offset..offset + BLOCK_BYTES],
                self.t,
                false,
            );
            offset += BLOCK_BYTES;
        }

        // Buffer remaining bytes.
        let remaining = data.len() - offset;
        if remaining > 0 {
            self.buf[self.buf_len..self.buf_len + remaining].copy_from_slice(&data[offset..]);
            self.buf_len += remaining;
        }
    }

    /// Finalizes the hash and returns the digest.
    ///
    /// Returns a fixed-size array of 64 bytes; only the first `out_len`
    /// bytes are meaningful.
    #[must_use]
    pub fn finalize(mut self) -> [u8; 64] {
        self.increment_counter(self.buf_len as u64);

        // Pad the final block with zeros.
        for i in self.buf_len..BLOCK_BYTES {
            self.buf[i] = 0;
        }

        let block = self.buf;
        compress(&mut self.h, &block, self.t, true);

        let mut out = [0u8; 64];
        for (i, word) in self.h.iter().enumerate() {
            let bytes = word.to_le_bytes();
            out[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
        }
        out
    }

    /// One-shot hash: computes BLAKE2B of `data` with the given output length.
    #[must_use]
    pub fn hash(data: &[u8], out_len: usize) -> [u8; 64] {
        let mut hasher = Self::new(out_len);
        hasher.update(data);
        hasher.finalize()
    }

    /// Returns the configured output length in bytes.
    #[must_use]
    pub fn output_len(&self) -> usize {
        self.out_len
    }

    fn increment_counter(&mut self, inc: u64) {
        self.t[0] = self.t[0].wrapping_add(inc);
        if self.t[0] < inc {
            self.t[1] = self.t[1].wrapping_add(1);
        }
    }
}

/// The G mixing function.
#[inline(always)]
fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

/// BLAKE2B compression function.
fn compress(h: &mut [u64; 8], block: &[u8], t: [u64; 2], last: bool) {
    // Parse message words.
    let mut m = [0u64; 16];
    for (i, word) in m.iter_mut().enumerate() {
        let off = i * 8;
        *word = u64::from_le_bytes([
            block[off],
            block[off + 1],
            block[off + 2],
            block[off + 3],
            block[off + 4],
            block[off + 5],
            block[off + 6],
            block[off + 7],
        ]);
    }

    // Initialize working vector.
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(h);
    v[8..16].copy_from_slice(&IV);
    v[12] ^= t[0];
    v[13] ^= t[1];
    if last {
        v[14] = !v[14];
    }

    // Twelve rounds of mixing.
    for s in SIGMA.iter().take(ROUNDS) {
        g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }

    // Finalize.
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_32() {
        // BLAKE2B-256 of empty string (reference vector).
        let hash = Blake2b::hash(b"", 32);
        assert_eq!(
            &hash[..32],
            &[
                0x0e, 0x57, 0x51, 0xc0, 0x26, 0xe5, 0x43, 0xb2, 0xe8, 0xab, 0x2e, 0xb0, 0x60, 0x99,
                0xda, 0xa1, 0xd1, 0xe5, 0xdf, 0x47, 0x77, 0x8f, 0x77, 0x87, 0xfa, 0xab, 0x45, 0xcd,
                0xf1, 0x2f, 0xe3, 0xa8,
            ]
        );
    }

    #[test]
    fn abc_64() {
        // BLAKE2B-512 of "abc" (reference vector from RFC 7693 Appendix A).
        let hash = Blake2b::hash(b"abc", 64);
        assert_eq!(hash[0], 0xba);
        assert_eq!(hash[1], 0x80);
        assert_eq!(hash[2], 0xa5);
    }

    #[test]
    fn incremental_matches_oneshot() {
        let data = b"The quick brown fox jumps over the lazy dog";
        let oneshot = Blake2b::hash(data, 64);

        let mut hasher = Blake2b::new(64);
        hasher.update(&data[..10]);
        hasher.update(&data[10..30]);
        hasher.update(&data[30..]);
        let incremental = hasher.finalize();

        assert_eq!(&oneshot[..64], &incremental[..64]);
    }
}
