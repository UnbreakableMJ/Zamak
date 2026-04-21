// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Chainloading support (FR-PROTO-004).
//!
//! Provides types and logic for the `efi` (UEFI chainload) and `bios`
//! (BIOS boot-sector chainload) protocols. Firmware-specific dispatch
//! (invoking `LoadImage`/`StartImage` or jumping to 0x7C00 real-mode) is
//! handled by the `zamak-uefi` and `zamak-bios` crates.

// Rust guideline compliant 2026-03-30

use alloc::string::String;

/// The current firmware environment, used to hide incompatible chainload entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Firmware {
    Bios,
    Uefi,
}

/// A chainload target parsed from a `bios_chainload` or `efi_chainload` entry.
#[derive(Debug, Clone)]
pub enum ChainloadTarget {
    /// UEFI PE/COFF application at the given URI.
    Efi { image_path: String },
    /// BIOS boot-sector chainload from drive/partition.
    Bios {
        /// 1-based drive number. `None` = boot drive.
        drive: Option<u32>,
        /// 1-based partition. `None` or 0 = chainload whole drive (MBR).
        partition: Option<u32>,
        /// MBR ID override (32-bit hex).
        mbr_id: Option<u32>,
        /// GPT GUID override.
        gpt_guid: Option<String>,
    },
}

impl ChainloadTarget {
    /// Returns `true` if this target is compatible with the current firmware.
    ///
    /// UEFI chainload entries are only usable under UEFI; BIOS chainload
    /// entries are only usable under BIOS. Entries incompatible with the
    /// current environment must be hidden from the menu (§CONFIG.md note
    /// on BIOS/UEFI chainload mutual exclusivity).
    pub fn compatible_with(&self, fw: Firmware) -> bool {
        matches!(
            (self, fw),
            (Self::Efi { .. }, Firmware::Uefi) | (Self::Bios { .. }, Firmware::Bios)
        )
    }
}

/// Parses a chainload target from a config entry's options.
///
/// `protocol` should be one of `"efi"`, `"uefi"`, `"efi_chainload"`, or
/// `"bios"` / `"bios_chainload"` (Limine aliases — see CONFIG.md).
///
/// The `get_option` closure receives an upper-case key and returns an
/// owned `String` for that key, or `None` if absent.
pub fn parse<F>(protocol: &str, mut get_option: F) -> Option<ChainloadTarget>
where
    F: FnMut(&str) -> Option<String>,
{
    let canonical = canonical_protocol(protocol)?;
    match canonical {
        Protocol::Efi => {
            let image_path = get_option("PATH").or_else(|| get_option("IMAGE_PATH"))?;
            Some(ChainloadTarget::Efi { image_path })
        }
        Protocol::Bios => {
            let drive = get_option("DRIVE").as_deref().and_then(parse_uint);
            let partition = get_option("PARTITION").as_deref().and_then(parse_uint);
            let mbr_id = get_option("MBR_ID").as_deref().and_then(parse_hex);
            let gpt_guid = get_option("GPT_UUID").or_else(|| get_option("GPT_GUID"));
            Some(ChainloadTarget::Bios {
                drive,
                partition,
                mbr_id,
                gpt_guid,
            })
        }
    }
}

/// Returns `true` if the given protocol string identifies any chainload variant.
pub fn is_chainload_protocol(protocol: &str) -> bool {
    canonical_protocol(protocol).is_some()
}

/// Returns the firmware this chainload protocol targets, regardless of alias.
pub fn target_firmware(protocol: &str) -> Option<Firmware> {
    match canonical_protocol(protocol)? {
        Protocol::Efi => Some(Firmware::Uefi),
        Protocol::Bios => Some(Firmware::Bios),
    }
}

/// Filters a list of menu entries, removing chainload entries that are
/// incompatible with the current firmware (§CONFIG.md).
///
/// Leaf entries only — directories are preserved and recursed into by the caller.
pub fn should_hide(protocol: &str, fw: Firmware) -> bool {
    match target_firmware(protocol) {
        Some(target) => target != fw,
        None => false, // Not a chainload protocol — never hide.
    }
}

enum Protocol {
    Efi,
    Bios,
}

fn canonical_protocol(p: &str) -> Option<Protocol> {
    // Limine aliases: see CONFIG.md §5.1.
    let lower = p.to_ascii_lowercase();
    match lower.as_str() {
        "efi" | "uefi" | "efi_chainload" => Some(Protocol::Efi),
        "bios" | "bios_chainload" => Some(Protocol::Bios),
        _ => None,
    }
}

fn parse_uint(s: &str) -> Option<u32> {
    s.parse().ok()
}

fn parse_hex(s: &str) -> Option<u32> {
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix("0X"))
        .unwrap_or(s);
    u32::from_str_radix(s, 16).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;

    fn opts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parse_efi_chainload() {
        let o = opts(&[("PATH", "boot():/EFI/other/OTHER.EFI")]);
        let target = parse("efi_chainload", |k| o.get(k).cloned()).unwrap();
        match target {
            ChainloadTarget::Efi { image_path } => {
                assert_eq!(image_path, "boot():/EFI/other/OTHER.EFI");
            }
            _ => panic!("expected Efi"),
        }
    }

    #[test]
    fn parse_bios_chainload() {
        let o = opts(&[("DRIVE", "2"), ("PARTITION", "1")]);
        let target = parse("bios", |k| o.get(k).cloned()).unwrap();
        match target {
            ChainloadTarget::Bios {
                drive, partition, ..
            } => {
                assert_eq!(drive, Some(2));
                assert_eq!(partition, Some(1));
            }
            _ => panic!("expected Bios"),
        }
    }

    #[test]
    fn parse_bios_mbr_id_hex() {
        let o = opts(&[("MBR_ID", "0xDEADBEEF")]);
        let target = parse("bios_chainload", |k| o.get(k).cloned()).unwrap();
        match target {
            ChainloadTarget::Bios { mbr_id, .. } => {
                assert_eq!(mbr_id, Some(0xDEAD_BEEF));
            }
            _ => panic!("expected Bios"),
        }
    }

    #[test]
    fn compatibility_check() {
        let efi = ChainloadTarget::Efi {
            image_path: String::new(),
        };
        let bios = ChainloadTarget::Bios {
            drive: None,
            partition: None,
            mbr_id: None,
            gpt_guid: None,
        };
        assert!(efi.compatible_with(Firmware::Uefi));
        assert!(!efi.compatible_with(Firmware::Bios));
        assert!(bios.compatible_with(Firmware::Bios));
        assert!(!bios.compatible_with(Firmware::Uefi));
    }

    #[test]
    fn should_hide_mismatched_firmware() {
        assert!(should_hide("efi", Firmware::Bios));
        assert!(should_hide("bios", Firmware::Uefi));
        assert!(!should_hide("efi", Firmware::Uefi));
        assert!(!should_hide("bios", Firmware::Bios));
        // Non-chainload protocols are never hidden.
        assert!(!should_hide("linux", Firmware::Bios));
        assert!(!should_hide("limine", Firmware::Uefi));
    }

    #[test]
    fn is_chainload_recognises_aliases() {
        assert!(is_chainload_protocol("efi"));
        assert!(is_chainload_protocol("uefi"));
        assert!(is_chainload_protocol("efi_chainload"));
        assert!(is_chainload_protocol("bios"));
        assert!(is_chainload_protocol("bios_chainload"));
        assert!(!is_chainload_protocol("linux"));
        assert!(!is_chainload_protocol("limine"));
    }

    #[test]
    fn missing_efi_path_returns_none() {
        let o = opts(&[]);
        let target = parse("efi", |k| o.get(k).cloned());
        assert!(target.is_none());
    }

    #[test]
    fn image_path_alias_for_efi() {
        let o = opts(&[("IMAGE_PATH", "boot():/X.EFI")]);
        let target = parse("efi", |k| o.get(k).cloned()).unwrap();
        match target {
            ChainloadTarget::Efi { image_path } => {
                assert_eq!(image_path, "boot():/X.EFI");
            }
            _ => panic!("expected Efi"),
        }
    }
}
