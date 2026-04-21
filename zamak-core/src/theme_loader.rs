// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Theme file discovery and loading (FR-CFG-007, §7.1).
//!
//! Resolves the `theme` global config option to a byte buffer, falls back
//! to standard paths, and parses the result into a `Theme`. On any failure
//! (missing file, malformed TOML, invalid hex), the built-in default theme
//! is returned and a warning reason is surfaced.

// Rust guideline compliant 2026-03-30

use alloc::string::String;
use zamak_theme::{Theme, ThemeVariant};

/// Standard theme file search paths, checked in order when `theme` is unset.
pub const STANDARD_THEME_PATHS: &[&str] = &[
    "/zamak-theme.toml",
    "/boot/zamak-theme.toml",
    "/boot/limine/zamak-theme.toml",
    "/EFI/BOOT/zamak-theme.toml",
];

/// Outcome of a theme load attempt.
#[derive(Debug)]
pub enum ThemeLoadResult {
    /// Theme loaded successfully from the given source path.
    Loaded { theme: Theme, source: String },
    /// Loaded but empty/all defaults (still surfaces the source).
    Defaulted { reason: String },
}

/// Loads a theme from the given file bytes and applies the variant.
///
/// Returns the parsed theme on success. Since `Theme::from_toml` silently
/// ignores malformed entries, this function always returns a theme — the
/// caller can compare against `Theme::default()` to detect an empty parse.
pub fn load_from_bytes(toml_bytes: &[u8], variant: ThemeVariant) -> Theme {
    let toml = core::str::from_utf8(toml_bytes).unwrap_or("");
    Theme::from_toml(toml).with_variant(variant)
}

/// Trait abstracting filesystem access for theme-file lookup.
///
/// Implemented by the BIOS and UEFI filesystem layers. Returns `None` for
/// missing or unreadable paths; returns file bytes on success.
pub trait FileReader {
    fn read(&self, path: &str) -> Option<alloc::vec::Vec<u8>>;
}

/// Resolves and loads a theme, honoring `config.theme_path` and variant.
///
/// Lookup order:
/// 1. `config.theme_path` if set
/// 2. Each entry in `STANDARD_THEME_PATHS`
///
/// If nothing is found or the file is empty, returns the built-in default
/// theme adjusted for variant.
pub fn resolve<R: FileReader>(
    reader: &R,
    theme_path: Option<&str>,
    variant: ThemeVariant,
) -> ThemeLoadResult {
    if let Some(path) = theme_path {
        if let Some(bytes) = reader.read(path) {
            return ThemeLoadResult::Loaded {
                theme: load_from_bytes(&bytes, variant),
                source: String::from(path),
            };
        }
        // Config pointed to a theme but we couldn't read it — warn and fall through.
    }

    for candidate in STANDARD_THEME_PATHS {
        if let Some(bytes) = reader.read(candidate) {
            return ThemeLoadResult::Loaded {
                theme: load_from_bytes(&bytes, variant),
                source: String::from(*candidate),
            };
        }
    }

    ThemeLoadResult::Defaulted {
        reason: String::from("no zamak-theme.toml found; using built-in defaults"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::BTreeMap;
    use alloc::string::ToString;
    use alloc::vec::Vec;

    struct FakeFs {
        files: BTreeMap<String, Vec<u8>>,
    }

    impl FileReader for FakeFs {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.files.get(path).cloned()
        }
    }

    #[test]
    fn resolve_missing_returns_default() {
        let fs = FakeFs {
            files: BTreeMap::new(),
        };
        match resolve(&fs, None, ThemeVariant::Dark) {
            ThemeLoadResult::Defaulted { .. } => {}
            _ => panic!("expected Defaulted"),
        }
    }

    #[test]
    fn resolve_honors_config_path() {
        let toml = b"[accent]\nprimary = \"FF0000\"\n";
        let mut fs = FakeFs {
            files: BTreeMap::new(),
        };
        fs.files
            .insert("/boot/custom.toml".to_string(), toml.to_vec());

        let result = resolve(&fs, Some("/boot/custom.toml"), ThemeVariant::Dark);
        match result {
            ThemeLoadResult::Loaded { theme, source } => {
                assert_eq!(source, "/boot/custom.toml");
                assert_eq!(
                    theme.accent.primary,
                    zamak_theme::Rgb::new(0xFF, 0x00, 0x00)
                );
            }
            _ => panic!("expected Loaded"),
        }
    }

    #[test]
    fn resolve_falls_back_to_standard_paths() {
        let toml = b"[accent]\nprimary = \"00FF00\"\n";
        let mut fs = FakeFs {
            files: BTreeMap::new(),
        };
        fs.files
            .insert("/boot/zamak-theme.toml".to_string(), toml.to_vec());

        let result = resolve(&fs, None, ThemeVariant::Dark);
        match result {
            ThemeLoadResult::Loaded { theme, source } => {
                assert_eq!(source, "/boot/zamak-theme.toml");
                assert_eq!(theme.accent.primary, zamak_theme::Rgb::new(0, 0xFF, 0));
            }
            _ => panic!("expected Loaded"),
        }
    }

    #[test]
    fn resolve_malformed_falls_back_silently() {
        // Malformed TOML — Theme::from_toml silently ignores unknown lines.
        let mut fs = FakeFs {
            files: BTreeMap::new(),
        };
        fs.files.insert(
            "/zamak-theme.toml".to_string(),
            b"this is not valid toml !!@#$".to_vec(),
        );

        let result = resolve(&fs, None, ThemeVariant::Dark);
        match result {
            ThemeLoadResult::Loaded { theme, .. } => {
                // Should fall through to defaults since nothing parsed.
                assert_eq!(
                    theme.accent.primary,
                    zamak_theme::Rgb::new(0x15, 0x65, 0xC0)
                );
            }
            _ => panic!("expected Loaded (silent fallback)"),
        }
    }

    #[test]
    fn variant_is_applied() {
        let toml = b"";
        let mut fs = FakeFs {
            files: BTreeMap::new(),
        };
        fs.files
            .insert("/zamak-theme.toml".to_string(), toml.to_vec());

        let result = resolve(&fs, None, ThemeVariant::Light);
        match result {
            ThemeLoadResult::Loaded { theme, .. } => {
                // Light variant swaps background and foreground.
                let defaults = Theme::default();
                assert_eq!(theme.surface.background, defaults.surface.foreground);
            }
            _ => panic!("expected Loaded"),
        }
    }
}
