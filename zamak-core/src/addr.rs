// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Newtype wrappers for hardware-constrained values (PRD §3.9.3).
//!
//! Raw integer types (`u64`, `usize`) must never be passed directly to
//! assembly wrappers. These newtypes enforce invariants at construction
//! time, making invalid hardware values unrepresentable.
//!
//! # Examples
//!
//! ```
//! use zamak_core::addr::{PhysAddr, PageAlignedPhysAddr};
//!
//! let addr = PhysAddr::new(0x1000).unwrap();
//! let page = PageAlignedPhysAddr::new(0x1000).unwrap();
//! assert_eq!(page.as_u64(), 0x1000);
//!
//! // Misaligned address is rejected at construction time.
//! assert!(PageAlignedPhysAddr::new(0x1001).is_err());
//! ```

// Rust guideline compliant 2026-03-30

#![allow(clippy::doc_markdown)]

use core::fmt;

/// Maximum physical address for x86-64 (52-bit physical address space).
const MAX_PHYS_ADDR_X86_64: u64 = (1 << 52) - 1;

/// Error returned when constructing a newtype wrapper with an invalid value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidHwValue {
    /// Address exceeds the architecture's physical address limit.
    PhysAddrOutOfRange(u64),
    /// Address is not aligned to the required boundary.
    Misaligned { addr: u64, required_align: u64 },
    /// Address is not in canonical form.
    NonCanonical(u64),
    /// Address is not below the 1 MiB real-mode boundary.
    AboveRealModeLimit(u64),
    /// CR3 value has invalid flag or alignment bits.
    InvalidCr3(u64),
    /// MAIR value contains an invalid attribute encoding.
    InvalidMair(u64),
    /// SATP mode field is not a supported Sv mode.
    InvalidSatp(u64),
}

impl fmt::Display for InvalidHwValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PhysAddrOutOfRange(a) => {
                write!(f, "physical address {a:#x} exceeds MAX_PHYS_ADDR")
            }
            Self::Misaligned {
                addr,
                required_align,
            } => {
                write!(f, "address {addr:#x} not aligned to {required_align:#x}")
            }
            Self::NonCanonical(a) => {
                write!(f, "virtual address {a:#x} is not canonical")
            }
            Self::AboveRealModeLimit(a) => {
                write!(f, "address {a:#x} is above the 1 MiB real-mode limit")
            }
            Self::InvalidCr3(v) => {
                write!(f, "CR3 value {v:#x} has invalid flags or alignment")
            }
            Self::InvalidMair(v) => {
                write!(f, "MAIR value {v:#x} contains invalid encoding")
            }
            Self::InvalidSatp(v) => {
                write!(f, "SATP value {v:#x} has unsupported mode")
            }
        }
    }
}

/// A physical address guaranteed to be within the architecture's limit.
///
/// On x86-64, physical addresses must be below 2^52.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PhysAddr(u64);

impl PhysAddr {
    /// Creates a new `PhysAddr` after validating the range.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::PhysAddrOutOfRange`] if `addr` exceeds
    /// the 52-bit physical address limit.
    pub const fn new(addr: u64) -> Result<Self, InvalidHwValue> {
        if addr > MAX_PHYS_ADDR_X86_64 {
            Err(InvalidHwValue::PhysAddrOutOfRange(addr))
        } else {
            Ok(Self(addr))
        }
    }

    /// Returns the raw `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl PhysAddr {
    /// Adds a byte offset, returning `None` on overflow or out-of-range.
    ///
    /// This is the only way to advance a `PhysAddr`; the newtype
    /// intentionally does not implement `Add<u64>` so that all arithmetic
    /// is explicitly checked (PRD §3.5 — `checked_*` for address math).
    #[must_use]
    pub const fn checked_add(self, offset: u64) -> Option<Self> {
        match self.0.checked_add(offset) {
            Some(sum) if sum <= MAX_PHYS_ADDR_X86_64 => Some(Self(sum)),
            _ => None,
        }
    }

    /// Returns the difference between two physical addresses, or `None` if
    /// `self < other`.
    #[must_use]
    pub const fn checked_sub(self, other: Self) -> Option<u64> {
        self.0.checked_sub(other.0)
    }

    /// Returns the physical address of the page containing this address.
    #[must_use]
    pub const fn page_floor(self) -> Self {
        Self(self.0 & !0xFFF)
    }

    /// Returns the physical address of the first byte past the page
    /// containing this address, or `None` if that wraps or overflows.
    #[must_use]
    pub const fn page_ceil(self) -> Option<Self> {
        match self.0.checked_add(0xFFF) {
            Some(v) => {
                let rounded = v & !0xFFF;
                if rounded > MAX_PHYS_ADDR_X86_64 {
                    None
                } else {
                    Some(Self(rounded))
                }
            }
            None => None,
        }
    }
}

impl fmt::Display for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhysAddr({:#x})", self.0)
    }
}

/// A physical address guaranteed to be 4 KiB page-aligned and within range.
///
/// Invariant: `addr & 0xFFF == 0` and `addr <= MAX_PHYS_ADDR`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PageAlignedPhysAddr(u64);

impl PageAlignedPhysAddr {
    /// Page size (4 KiB).
    pub const PAGE_SIZE: u64 = 0x1000;

    /// Creates a new page-aligned physical address.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::Misaligned`] if `addr` is not 4 KiB
    /// aligned, or [`InvalidHwValue::PhysAddrOutOfRange`] if out of range.
    pub const fn new(addr: u64) -> Result<Self, InvalidHwValue> {
        if addr > MAX_PHYS_ADDR_X86_64 {
            return Err(InvalidHwValue::PhysAddrOutOfRange(addr));
        }
        if addr & (Self::PAGE_SIZE - 1) != 0 {
            return Err(InvalidHwValue::Misaligned {
                addr,
                required_align: Self::PAGE_SIZE,
            });
        }
        Ok(Self(addr))
    }

    /// Returns the raw `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Converts to a generic [`PhysAddr`].
    #[must_use]
    pub const fn as_phys_addr(self) -> PhysAddr {
        PhysAddr(self.0)
    }
}

impl fmt::Display for PageAlignedPhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PageAlignedPhysAddr({:#x})", self.0)
    }
}

/// A canonical virtual address.
///
/// On x86-64 with 4-level paging, bits 48..63 must equal bit 47
/// (sign extension). With 5-level paging (LA57), bits 57..63 must
/// equal bit 56.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct VirtAddr(u64);

impl VirtAddr {
    /// Creates a new canonical virtual address.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::NonCanonical`] if the address is not
    /// in canonical form (assuming 48-bit virtual address space).
    pub const fn new(addr: u64) -> Result<Self, InvalidHwValue> {
        // Check 48-bit canonical form: bits 47..63 must all be the same.
        let canonical = ((addr << 16) as i64 >> 16) as u64;
        if addr != canonical {
            Err(InvalidHwValue::NonCanonical(addr))
        } else {
            Ok(Self(addr))
        }
    }

    /// Returns the raw `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl VirtAddr {
    /// Adds a byte offset, returning `None` on arithmetic overflow or if
    /// the result leaves canonical form.
    ///
    /// Like [`PhysAddr::checked_add`], this is the only way to advance a
    /// `VirtAddr`; no `Add<u64>` is provided so every step is checked.
    #[must_use]
    pub const fn checked_add(self, offset: u64) -> Option<Self> {
        match self.0.checked_add(offset) {
            Some(sum) => {
                let canonical = ((sum << 16) as i64 >> 16) as u64;
                if sum == canonical {
                    Some(Self(sum))
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// Returns the signed byte difference between two virtual addresses.
    #[must_use]
    pub fn wrapping_sub(self, other: Self) -> i64 {
        self.0.wrapping_sub(other.0) as i64
    }
}

impl fmt::Display for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VirtAddr({:#x})", self.0)
    }
}

/// A physical address below 1 MiB, suitable for real-mode AP trampolines.
///
/// SMP application processors start in real mode and can only address
/// the first 1 MiB. The trampoline code must be placed within this range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TrampolineAddr(u64);

impl TrampolineAddr {
    /// Real-mode addressable limit (1 MiB).
    const REAL_MODE_LIMIT: u64 = 0x10_0000;

    /// Creates a new trampoline address.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::AboveRealModeLimit`] if `addr >= 1 MiB`.
    pub const fn new(addr: u64) -> Result<Self, InvalidHwValue> {
        if addr >= Self::REAL_MODE_LIMIT {
            Err(InvalidHwValue::AboveRealModeLimit(addr))
        } else {
            Ok(Self(addr))
        }
    }

    /// Returns the raw `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for TrampolineAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TrampolineAddr({:#x})", self.0)
    }
}

/// A validated CR3 register value.
///
/// Bits 12+ must contain a page-aligned PML4 base address.
/// Bits 0..11 are flags (PCD, PWT, etc.) and must contain only
/// valid flag combinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Cr3Value(u64);

impl Cr3Value {
    /// Valid flag bits in CR3 (bits 3 = PWT, bit 4 = PCD).
    const VALID_FLAGS_MASK: u64 = (1 << 3) | (1 << 4);

    /// Creates a new CR3 value from a page-aligned PML4 base and flags.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::InvalidCr3`] if the base address is
    /// not page-aligned or if reserved flag bits are set.
    pub const fn new(value: u64) -> Result<Self, InvalidHwValue> {
        let base = value & !0xFFF;
        let flags = value & 0xFFF;

        // Base must be page-aligned (guaranteed by masking, but
        // the full value must have bits 0..2 and 5..11 clear).
        if flags & !Self::VALID_FLAGS_MASK != 0 {
            return Err(InvalidHwValue::InvalidCr3(value));
        }

        // Base must be within physical address range.
        if base > MAX_PHYS_ADDR_X86_64 {
            return Err(InvalidHwValue::InvalidCr3(value));
        }

        Ok(Self(value))
    }

    /// Creates a CR3 value from a page-aligned PML4 address with no flags.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::InvalidCr3`] if `pml4_base` is not
    /// page-aligned or exceeds the physical address limit.
    pub const fn from_pml4(pml4_base: u64) -> Result<Self, InvalidHwValue> {
        if pml4_base & 0xFFF != 0 {
            return Err(InvalidHwValue::InvalidCr3(pml4_base));
        }
        if pml4_base > MAX_PHYS_ADDR_X86_64 {
            return Err(InvalidHwValue::InvalidCr3(pml4_base));
        }
        Ok(Self(pml4_base))
    }

    /// Returns the raw `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns the PML4 base address (bits 12+).
    #[must_use]
    pub const fn base_addr(self) -> u64 {
        self.0 & !0xFFF
    }
}

impl fmt::Display for Cr3Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cr3Value({:#x})", self.0)
    }
}

/// A validated MAIR_EL1 register value (AArch64).
///
/// Contains 8 attribute fields (bytes 0..7), each encoding a memory
/// type. This wrapper validates that all 8 fields contain valid MAIR
/// encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct MairValue(u64);

impl MairValue {
    /// Creates a new MAIR value after validating all 8 attribute fields.
    ///
    /// Each byte must be a valid MAIR attribute encoding:
    /// - `0x00` = Device-nGnRnE
    /// - `0x04` = Device-nGnRE
    /// - `0x08` = Device-nGRE
    /// - `0x0C` = Device-GRE
    /// - `0x44` = Normal Non-Cacheable
    /// - `0xBB` = Normal Write-Back (inner + outer)
    /// - `0xFF` = Normal Write-Back, Read-Allocate, Write-Allocate
    ///
    /// For simplicity, this validates the high nibble pattern:
    /// - `0x0_` = Device memory (low nibble encodes gathering/reordering)
    /// - Others = Normal memory (each nibble encodes cacheability)
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::InvalidMair`] if any attribute byte
    /// is not a recognized encoding.
    pub const fn new(value: u64) -> Result<Self, InvalidHwValue> {
        // Validate each of the 8 attribute bytes.
        let mut i = 0;
        while i < 8 {
            let attr = ((value >> (i * 8)) & 0xFF) as u8;
            if !Self::is_valid_attr(attr) {
                return Err(InvalidHwValue::InvalidMair(value));
            }
            i += 1;
        }
        Ok(Self(value))
    }

    /// Checks if a single MAIR attribute byte is valid.
    ///
    /// Device memory: high nibble = 0x0, low nibble = 0x0/0x4/0x8/0xC.
    /// Normal memory: both nibbles nonzero (each encodes cacheability).
    /// Exception: 0x00 is valid (Device-nGnRnE), 0x44 is valid (NC).
    const fn is_valid_attr(attr: u8) -> bool {
        if attr == 0x00 {
            return true; // Device-nGnRnE
        }
        let hi = attr >> 4;
        let lo = attr & 0x0F;
        if hi == 0 {
            // Device memory: low nibble must be 0x0, 0x4, 0x8, or 0xC.
            matches!(lo, 0x4 | 0x8 | 0xC)
        } else {
            // Normal memory: both nibbles must be nonzero.
            lo != 0
        }
    }

    /// Returns the raw `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

impl fmt::Display for MairValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "MairValue({:#018x})", self.0)
    }
}

/// A validated RISC-V SATP register value.
///
/// The mode field (bits 60..63) must be a supported Sv mode:
/// - 0 = Bare (no translation)
/// - 8 = Sv39 (3-level, 39-bit virtual address)
/// - 9 = Sv48 (4-level, 48-bit virtual address)
/// - 10 = Sv57 (5-level, 57-bit virtual address)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SatpValue(u64);

impl SatpValue {
    /// Creates a new SATP value after validating the mode field.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidHwValue::InvalidSatp`] if the mode field
    /// is not Bare (0), Sv39 (8), Sv48 (9), or Sv57 (10).
    pub const fn new(value: u64) -> Result<Self, InvalidHwValue> {
        let mode = value >> 60;
        match mode {
            0 | 8 | 9 | 10 => Ok(Self(value)),
            _ => Err(InvalidHwValue::InvalidSatp(value)),
        }
    }

    /// Returns the raw `u64` value.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Returns the ASID field (bits 44..59).
    #[must_use]
    pub const fn asid(self) -> u16 {
        ((self.0 >> 44) & 0xFFFF) as u16
    }

    /// Returns the PPN field (bits 0..43).
    #[must_use]
    pub const fn ppn(self) -> u64 {
        self.0 & ((1 << 44) - 1)
    }
}

impl fmt::Display for SatpValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SatpValue({:#x})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phys_addr_valid() {
        assert!(PhysAddr::new(0).is_ok());
        assert!(PhysAddr::new(0x1000).is_ok());
        assert!(PhysAddr::new(MAX_PHYS_ADDR_X86_64).is_ok());
    }

    #[test]
    fn phys_addr_out_of_range() {
        assert!(PhysAddr::new(MAX_PHYS_ADDR_X86_64 + 1).is_err());
        assert!(PhysAddr::new(u64::MAX).is_err());
    }

    #[test]
    fn page_aligned_valid() {
        assert!(PageAlignedPhysAddr::new(0).is_ok());
        assert!(PageAlignedPhysAddr::new(0x1000).is_ok());
        assert!(PageAlignedPhysAddr::new(0x20_0000).is_ok());
    }

    #[test]
    fn page_aligned_misaligned() {
        assert!(PageAlignedPhysAddr::new(1).is_err());
        assert!(PageAlignedPhysAddr::new(0x1001).is_err());
        assert!(PageAlignedPhysAddr::new(0xFFF).is_err());
    }

    #[test]
    fn virt_addr_canonical() {
        assert!(VirtAddr::new(0).is_ok());
        assert!(VirtAddr::new(0x0000_7FFF_FFFF_FFFF).is_ok());
        assert!(VirtAddr::new(0xFFFF_8000_0000_0000).is_ok());
        assert!(VirtAddr::new(0xFFFF_FFFF_FFFF_FFFF).is_ok());
    }

    #[test]
    fn virt_addr_non_canonical() {
        assert!(VirtAddr::new(0x0000_8000_0000_0000).is_err());
        assert!(VirtAddr::new(0x0001_0000_0000_0000).is_err());
    }

    #[test]
    fn trampoline_addr_valid() {
        assert!(TrampolineAddr::new(0).is_ok());
        assert!(TrampolineAddr::new(0x8000).is_ok());
        assert!(TrampolineAddr::new(0xF_FFFF).is_ok());
    }

    #[test]
    fn trampoline_addr_above_limit() {
        assert!(TrampolineAddr::new(0x10_0000).is_err());
        assert!(TrampolineAddr::new(0x20_0000).is_err());
    }

    #[test]
    fn cr3_valid() {
        assert!(Cr3Value::from_pml4(0x1000).is_ok());
        assert!(Cr3Value::from_pml4(0x20_0000).is_ok());
        // With PWT flag (bit 3).
        assert!(Cr3Value::new(0x1000 | (1 << 3)).is_ok());
    }

    #[test]
    fn cr3_misaligned() {
        assert!(Cr3Value::from_pml4(0x1001).is_err());
    }

    #[test]
    fn cr3_reserved_bits() {
        // Bit 0 is reserved, should fail.
        assert!(Cr3Value::new(0x1000 | 1).is_err());
    }

    #[test]
    fn satp_valid_modes() {
        // Bare mode.
        assert!(SatpValue::new(0).is_ok());
        // Sv39.
        assert!(SatpValue::new(8u64 << 60).is_ok());
        // Sv48.
        assert!(SatpValue::new(9u64 << 60).is_ok());
        // Sv57.
        assert!(SatpValue::new(10u64 << 60).is_ok());
    }

    #[test]
    fn satp_invalid_mode() {
        assert!(SatpValue::new(1u64 << 60).is_err());
        assert!(SatpValue::new(15u64 << 60).is_err());
    }

    #[test]
    fn phys_addr_checked_add_in_range() {
        let a = PhysAddr::new(0x1000).unwrap();
        assert_eq!(a.checked_add(0x100).unwrap().as_u64(), 0x1100);
    }

    #[test]
    fn phys_addr_checked_add_overflow() {
        let a = PhysAddr::new(MAX_PHYS_ADDR_X86_64).unwrap();
        assert!(a.checked_add(1).is_none());
        assert!(a.checked_add(u64::MAX).is_none());
    }

    #[test]
    fn phys_addr_checked_sub() {
        let a = PhysAddr::new(0x2000).unwrap();
        let b = PhysAddr::new(0x1000).unwrap();
        assert_eq!(a.checked_sub(b), Some(0x1000));
        assert_eq!(b.checked_sub(a), None);
    }

    #[test]
    fn phys_addr_page_floor_and_ceil() {
        let a = PhysAddr::new(0x1234).unwrap();
        assert_eq!(a.page_floor().as_u64(), 0x1000);
        assert_eq!(a.page_ceil().unwrap().as_u64(), 0x2000);

        let page_aligned = PhysAddr::new(0x1000).unwrap();
        assert_eq!(page_aligned.page_floor().as_u64(), 0x1000);
        assert_eq!(page_aligned.page_ceil().unwrap().as_u64(), 0x1000);
    }

    #[test]
    fn virt_addr_checked_add_stays_canonical() {
        let a = VirtAddr::new(0xFFFF_8000_0000_0000).unwrap();
        assert!(a.checked_add(0x1000).is_some());
    }

    #[test]
    fn virt_addr_checked_add_rejects_non_canonical() {
        // Adding past the end of the low canonical half must fail.
        let a = VirtAddr::new(0x0000_7FFF_FFFF_F000).unwrap();
        assert!(a.checked_add(0x2000).is_none());
    }

    #[test]
    fn virt_addr_wrapping_sub() {
        let a = VirtAddr::new(0xFFFF_8000_0000_0000).unwrap();
        let b = VirtAddr::new(0xFFFF_8000_0000_1000).unwrap();
        assert_eq!(b.wrapping_sub(a), 0x1000);
    }
}
