// SPDX-License-Identifier: GPL-3.0-or-later

/// Trait for KASLR Random Number Generation
pub trait KaslrRng {
    /// Get a generic 64-bit random number.
    /// Weak entropy is acceptable for KASLR purposes if strong entropy is unavailable.
    fn get_u64(&mut self) -> u64;
}
