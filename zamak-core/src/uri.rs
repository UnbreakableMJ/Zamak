// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! URI path resolution for Limine-compatible boot paths (FR-CFG-003).
//!
//! Parses and resolves URI schemes used in `zamak.conf`:
//! - `boot()` — the boot volume (where the bootloader lives)
//! - `hdd(d:p)` — hard disk `d`, partition `p`
//! - `odd(d:p)` — optical disc drive `d`, partition `p`
//! - `guid(uuid)` — partition by GPT/MBR GUID
//! - `fslabel(label)` — partition by filesystem label
//! - `tftp(ip)` — TFTP server at the given IP
//!
//! Paths may end with a `#hash` suffix for BLAKE2B verification.

// Rust guideline compliant 2026-03-30

use alloc::string::{String, ToString};
use core::fmt;

/// A parsed URI from the config file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootUri {
    /// The URI scheme/source.
    pub source: UriSource,
    /// The file path within the resolved volume (e.g., `/boot/vmlinuz`).
    pub path: String,
    /// Optional BLAKE2B hash suffix for verification.
    pub hash: Option<String>,
}

/// The source/volume specifier in a boot URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriSource {
    /// `boot()` — the volume the bootloader was loaded from.
    Boot,
    /// `hdd(drive:partition)` — hard disk by index.
    Hdd { drive: u32, partition: u32 },
    /// `odd(drive:partition)` — optical disc drive by index.
    Odd { drive: u32, partition: u32 },
    /// `guid(uuid-string)` — partition by GPT/MBR GUID.
    Guid(String),
    /// `fslabel(label)` — partition by filesystem label.
    FsLabel(String),
    /// `tftp(ip)` — TFTP network boot source.
    Tftp(String),
}

/// Errors when parsing a boot URI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UriParseError {
    /// No URI scheme found (no parentheses).
    NoScheme,
    /// Unknown URI scheme.
    UnknownScheme(String),
    /// Missing closing parenthesis.
    UnclosedParen,
    /// Missing colon separator in `hdd(d:p)` / `odd(d:p)`.
    MissingDrivePartSep,
    /// Invalid drive or partition number.
    InvalidNumber,
    /// No file path after the URI scheme.
    NoPath,
}

impl fmt::Display for UriParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoScheme => write!(f, "no URI scheme found"),
            Self::UnknownScheme(s) => write!(f, "unknown URI scheme: {s}"),
            Self::UnclosedParen => write!(f, "missing closing parenthesis"),
            Self::MissingDrivePartSep => write!(f, "missing ':' in drive:partition"),
            Self::InvalidNumber => write!(f, "invalid drive/partition number"),
            Self::NoPath => write!(f, "no file path after URI scheme"),
        }
    }
}

/// Parses a Limine-compatible URI string.
///
/// Format: `scheme(args):/path/to/file#hash`
///
/// The `#hash` suffix is optional. If present, it is separated and
/// returned in `BootUri::hash`.
pub fn parse_uri(input: &str) -> Result<BootUri, UriParseError> {
    let input = input.trim();

    // Split off the optional #hash suffix.
    let (path_part, hash) = if let Some(hash_pos) = input.rfind('#') {
        let hash_str = &input[hash_pos + 1..];
        // Only treat as hash if it looks like a hex string.
        if !hash_str.is_empty() && hash_str.bytes().all(|b| b.is_ascii_hexdigit()) {
            (&input[..hash_pos], Some(hash_str.to_string()))
        } else {
            (input, None)
        }
    } else {
        (input, None)
    };

    // Find the scheme by locating the first '('.
    let open_paren = path_part.find('(').ok_or(UriParseError::NoScheme)?;
    let scheme = &path_part[..open_paren];

    let close_paren = path_part[open_paren..]
        .find(')')
        .ok_or(UriParseError::UnclosedParen)?
        + open_paren;
    let args = &path_part[open_paren + 1..close_paren];

    // The file path follows after `)` with an optional `:` separator.
    let rest = &path_part[close_paren + 1..];
    let path = if let Some(after_colon) = rest.strip_prefix(':') {
        after_colon.to_string()
    } else if rest.is_empty() {
        return Err(UriParseError::NoPath);
    } else {
        rest.to_string()
    };

    let source = match scheme {
        "boot" => UriSource::Boot,
        "hdd" => parse_drive_partition(args).map(|(d, p)| UriSource::Hdd {
            drive: d,
            partition: p,
        })?,
        "odd" => parse_drive_partition(args).map(|(d, p)| UriSource::Odd {
            drive: d,
            partition: p,
        })?,
        "guid" => UriSource::Guid(args.to_string()),
        "fslabel" => UriSource::FsLabel(args.to_string()),
        "tftp" => UriSource::Tftp(args.to_string()),
        other => return Err(UriParseError::UnknownScheme(other.to_string())),
    };

    Ok(BootUri { source, path, hash })
}

/// Parses `"d:p"` into `(drive, partition)`.
fn parse_drive_partition(args: &str) -> Result<(u32, u32), UriParseError> {
    let colon = args.find(':').ok_or(UriParseError::MissingDrivePartSep)?;
    let drive: u32 = args[..colon]
        .parse()
        .map_err(|_| UriParseError::InvalidNumber)?;
    let partition: u32 = args[colon + 1..]
        .parse()
        .map_err(|_| UriParseError::InvalidNumber)?;
    Ok((drive, partition))
}

/// Verifies file content against a BLAKE2B hash from a URI `#hash` suffix.
///
/// The hash string is expected to be lowercase hex. The output length
/// is inferred from the hex string length (e.g., 64 hex chars = 32-byte hash).
///
/// Returns `true` if the hash matches, `false` otherwise.
/// Returns `true` if `expected_hex` is `None` (no hash to verify).
pub fn verify_hash(data: &[u8], expected_hex: Option<&str>) -> bool {
    let expected_hex = match expected_hex {
        Some(h) if !h.is_empty() => h,
        _ => return true, // No hash specified — skip verification.
    };

    // Infer output length from hex string length (2 hex chars per byte).
    let out_len = expected_hex.len() / 2;
    if out_len == 0 || out_len > 64 {
        return false;
    }

    let digest = crate::blake2b::Blake2b::hash(data, out_len);

    // Compare against the expected hex string.
    let mut hex_buf = [0u8; 128]; // 64 bytes * 2 hex chars.
    for (i, &byte) in digest[..out_len].iter().enumerate() {
        hex_buf[i * 2] = hex_nibble(byte >> 4);
        hex_buf[i * 2 + 1] = hex_nibble(byte & 0x0F);
    }

    let computed = core::str::from_utf8(&hex_buf[..out_len * 2]).unwrap_or("");
    // Constant-time comparison is not needed here (boot integrity, not crypto auth).
    computed.eq_ignore_ascii_case(expected_hex)
}

fn hex_nibble(n: u8) -> u8 {
    if n < 10 {
        b'0' + n
    } else {
        b'a' + (n - 10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_boot_uri() {
        let uri = parse_uri("boot():/boot/vmlinuz").unwrap();
        assert_eq!(uri.source, UriSource::Boot);
        assert_eq!(uri.path, "/boot/vmlinuz");
        assert!(uri.hash.is_none());
    }

    #[test]
    fn parse_hdd_uri() {
        let uri = parse_uri("hdd(0:1):/EFI/BOOT/kernel").unwrap();
        assert_eq!(
            uri.source,
            UriSource::Hdd {
                drive: 0,
                partition: 1
            }
        );
        assert_eq!(uri.path, "/EFI/BOOT/kernel");
    }

    #[test]
    fn parse_guid_uri_with_hash() {
        let uri =
            parse_uri("guid(01234567-89ab-cdef-0123-456789abcdef):/boot/vmlinuz#abcdef01").unwrap();
        assert_eq!(
            uri.source,
            UriSource::Guid("01234567-89ab-cdef-0123-456789abcdef".into())
        );
        assert_eq!(uri.path, "/boot/vmlinuz");
        assert_eq!(uri.hash.as_deref(), Some("abcdef01"));
    }

    #[test]
    fn parse_fslabel_uri() {
        let uri = parse_uri("fslabel(ROOTFS):/boot/initramfs.img").unwrap();
        assert_eq!(uri.source, UriSource::FsLabel("ROOTFS".into()));
        assert_eq!(uri.path, "/boot/initramfs.img");
    }

    #[test]
    fn parse_tftp_uri() {
        let uri = parse_uri("tftp(192.168.1.1):/pxelinux/vmlinuz").unwrap();
        assert_eq!(uri.source, UriSource::Tftp("192.168.1.1".into()));
        assert_eq!(uri.path, "/pxelinux/vmlinuz");
    }

    #[test]
    fn parse_odd_uri() {
        let uri = parse_uri("odd(0:0):/kernel.elf").unwrap();
        assert_eq!(
            uri.source,
            UriSource::Odd {
                drive: 0,
                partition: 0
            }
        );
    }

    #[test]
    fn unknown_scheme_errors() {
        assert_eq!(
            parse_uri("ftp(host):/file").unwrap_err(),
            UriParseError::UnknownScheme("ftp".into())
        );
    }

    #[test]
    fn no_scheme_errors() {
        assert_eq!(
            parse_uri("/just/a/path").unwrap_err(),
            UriParseError::NoScheme
        );
    }

    #[test]
    fn verify_hash_none_passes() {
        assert!(verify_hash(b"anything", None));
    }

    #[test]
    fn verify_hash_matches() {
        // Compute a known BLAKE2B-32 hash and verify it.
        let data = b"hello";
        let digest = crate::blake2b::Blake2b::hash(data, 32);
        let hex: String = digest[..32]
            .iter()
            .map(|b| alloc::format!("{b:02x}"))
            .collect();
        assert!(verify_hash(data, Some(&hex)));
    }

    #[test]
    fn verify_hash_mismatch() {
        assert!(!verify_hash(
            b"hello",
            Some("0000000000000000000000000000000000000000000000000000000000000000")
        ));
    }
}
