// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! ZAMAK theme file parser and color-token resolver.
//!
//! Parses `zamak-theme.toml` files and resolves color tokens at boot time.
//! The theme file uses TOML syntax with five token groups: `surface`,
//! `accent`, `palette`, `editor`, and `branding` (PRD §3.1.1).
//!
//! This is a `#![no_std]` crate consumed by `zamak-core`.

// Rust guideline compliant 2026-03-30

#![no_std]

/// An RGB color value stored as three bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    /// Create a new RGB color.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse a 6-digit hexadecimal RGB string (e.g., `"50FA7B"`).
    ///
    /// Returns `None` if the string is not exactly 6 valid hex characters.
    #[must_use]
    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Self { r, g, b })
    }

    /// Convert to a 32-bit packed value (0x00RRGGBB).
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }
}

/// Base terminal surface colors.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceTokens {
    pub background: Rgb,
    pub foreground: Rgb,
    pub dim: Rgb,
    pub bright: Rgb,
}

/// Semantic accent colors for UI chrome.
#[derive(Debug, Clone, Copy)]
pub struct AccentTokens {
    pub primary: Rgb,
    pub secondary: Rgb,
    pub error: Rgb,
    pub warning: Rgb,
    pub success: Rgb,
}

/// Syntax-highlighting colors for the config editor.
#[derive(Debug, Clone, Copy)]
pub struct EditorTokens {
    pub key: Rgb,
    pub colon: Rgb,
    pub value: Rgb,
    pub comment: Rgb,
    pub invalid: Rgb,
}

/// Boot menu branding strip colors.
#[derive(Debug, Clone, Copy)]
pub struct BrandingTokens {
    pub text_color: Rgb,
    pub bar_color: Rgb,
}

/// Full resolved theme with all token groups.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub surface: SurfaceTokens,
    pub accent: AccentTokens,
    pub ansi: [Rgb; 16],
    pub editor: EditorTokens,
    pub branding: BrandingTokens,
}

/// Supported theme variants (§7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeVariant {
    Dark,
    Light,
}

impl ThemeVariant {
    /// Parses a variant string. Defaults to `Dark` for unrecognized values.
    ///
    /// Note: this is not an `impl FromStr` because it is infallible — we want
    /// the `Default::Dark` fallback behaviour rather than a `Result`.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "light" | "Light" | "LIGHT" => Self::Light,
            _ => Self::Dark,
        }
    }
}

/// Errors from parsing a theme TOML file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeParseError {
    /// A color value is not a valid 6-digit hex string.
    InvalidHexColor,
}

impl Theme {
    /// Parses a `zamak-theme.toml` string and returns a `Theme`.
    ///
    /// The parser handles `[section]` headers with `key = "value"` entries.
    /// Color values must be 6-digit hex strings (with or without `#` prefix).
    /// Unknown sections and keys are silently ignored.
    /// Missing values fall back to the built-in defaults.
    ///
    /// # Supported sections
    ///
    /// - `[surface]`: background, foreground, dim, bright
    /// - `[accent]`: primary, secondary, error, warning, success
    /// - `[palette]`: ansi_0 through ansi_15
    /// - `[editor]`: key, colon, value, comment, invalid
    /// - `[branding]`: text_color, bar_color
    ///
    /// Returns a theme adjusted for the given variant.
    ///
    /// For `Dark`, the theme is unchanged. For `Light`, surface background
    /// and foreground are swapped, and dim/bright are adjusted.
    #[must_use]
    pub fn with_variant(mut self, variant: ThemeVariant) -> Self {
        if variant == ThemeVariant::Light {
            core::mem::swap(&mut self.surface.background, &mut self.surface.foreground);
            core::mem::swap(&mut self.surface.dim, &mut self.surface.bright);
        }
        self
    }

    pub fn from_toml(toml: &str) -> Self {
        let mut theme = Self::default();
        let mut section = "";

        for line in toml.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section header: [name]
            if line.starts_with('[') {
                if let Some(end) = line.find(']') {
                    section = line[1..end].trim();
                }
                continue;
            }

            // Key = "value" or key = value
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim();
                let raw_value = line[eq_pos + 1..].trim();
                // Strip quotes if present.
                let value = raw_value
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(raw_value);
                // Strip optional # prefix from hex colors.
                let hex = value.strip_prefix('#').unwrap_or(value);

                if let Some(color) = Rgb::from_hex(hex) {
                    Self::apply_color(&mut theme, section, key, color);
                }
            }
        }

        theme
    }

    fn apply_color(theme: &mut Self, section: &str, key: &str, color: Rgb) {
        match section {
            "surface" => match key {
                "background" => theme.surface.background = color,
                "foreground" => theme.surface.foreground = color,
                "dim" => theme.surface.dim = color,
                "bright" => theme.surface.bright = color,
                _ => {}
            },
            "accent" => match key {
                "primary" => theme.accent.primary = color,
                "secondary" => theme.accent.secondary = color,
                "error" => theme.accent.error = color,
                "warning" => theme.accent.warning = color,
                "success" => theme.accent.success = color,
                _ => {}
            },
            "palette" => {
                // ansi_0 through ansi_15.
                if let Some(idx_str) = key.strip_prefix("ansi_") {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if idx < 16 {
                            theme.ansi[idx] = color;
                        }
                    }
                }
            }
            "editor" => match key {
                "key" => theme.editor.key = color,
                "colon" => theme.editor.colon = color,
                "value" => theme.editor.value = color,
                "comment" => theme.editor.comment = color,
                "invalid" => theme.editor.invalid = color,
                _ => {}
            },
            "branding" => match key {
                "text_color" => theme.branding.text_color = color,
                "bar_color" => theme.branding.bar_color = color,
                _ => {}
            },
            _ => {}
        }
    }
}

impl Default for Theme {
    /// Built-in default theme using Material Design color tokens (§3.1.2).
    ///
    /// Primary accent: Material Design Blue 800 (#1565C0).
    /// Error: Material Design Red 700 (#D32F2F).
    /// On-surface: Material Design Grey 900 (#212121) adapted to Void Navy.
    /// Background: Void Navy (#000027) from Steelbore palette.
    fn default() -> Self {
        Self {
            surface: SurfaceTokens {
                background: Rgb::new(0x00, 0x00, 0x27), // Void Navy
                foreground: Rgb::new(0xD9, 0x8E, 0x32), // Molten Amber
                dim: Rgb::new(0xA0, 0x6A, 0x20),
                bright: Rgb::new(0xF5, 0xC8, 0x7A),
            },
            accent: AccentTokens {
                primary: Rgb::new(0x15, 0x65, 0xC0),   // Material Design Blue 800
                secondary: Rgb::new(0x50, 0xFA, 0x7B), // Radium Green
                error: Rgb::new(0xD3, 0x2F, 0x2F),     // Material Design Red 700
                warning: Rgb::new(0xF5, 0x7F, 0x17),
                success: Rgb::new(0x50, 0xFA, 0x7B), // Radium Green
            },
            ansi: [
                Rgb::new(0x00, 0x00, 0x27), // 0  Black (Void Navy)
                Rgb::new(0xFF, 0x5C, 0x5C), // 1  Red (Red Oxide)
                Rgb::new(0x50, 0xFA, 0x7B), // 2  Green (Radium Green)
                Rgb::new(0xD9, 0x8E, 0x32), // 3  Yellow (Molten Amber)
                Rgb::new(0x15, 0x65, 0xC0), // 4  Blue (Material Design Blue 800)
                Rgb::new(0xBD, 0x93, 0xF9), // 5  Magenta
                Rgb::new(0x8B, 0xE9, 0xFD), // 6  Cyan (Liquid Coolant)
                Rgb::new(0xF8, 0xF8, 0xF2), // 7  White
                Rgb::new(0x44, 0x47, 0x5A), // 8  Bright black
                Rgb::new(0xFF, 0x6E, 0x6E), // 9  Bright red
                Rgb::new(0x69, 0xFF, 0x94), // 10 Bright green
                Rgb::new(0xF1, 0xFA, 0x8C), // 11 Bright yellow
                Rgb::new(0x42, 0xA5, 0xF5), // 12 Bright blue (Material Design Blue 400)
                Rgb::new(0xD6, 0xAC, 0xFF), // 13 Bright magenta
                Rgb::new(0xA4, 0xF0, 0xFF), // 14 Bright cyan
                Rgb::new(0xFF, 0xFF, 0xFF), // 15 Bright white
            ],
            editor: EditorTokens {
                key: Rgb::new(0x8B, 0xE9, 0xFD),   // Liquid Coolant
                colon: Rgb::new(0x50, 0xFA, 0x7B), // Radium Green
                value: Rgb::new(0xD9, 0x8E, 0x32), // Molten Amber
                comment: Rgb::new(0xA0, 0x6A, 0x20),
                invalid: Rgb::new(0xFF, 0x5C, 0x5C), // Red Oxide
            },
            branding: BrandingTokens {
                text_color: Rgb::new(0xD9, 0x8E, 0x32), // Molten Amber
                bar_color: Rgb::new(0x15, 0x65, 0xC0),  // Material Design Blue 800
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_from_hex() {
        assert_eq!(Rgb::from_hex("FF5C5C"), Some(Rgb::new(0xFF, 0x5C, 0x5C)));
        assert_eq!(Rgb::from_hex("000027"), Some(Rgb::new(0, 0, 0x27)));
        assert_eq!(Rgb::from_hex("ZZZ"), None);
    }

    #[test]
    fn parse_theme_toml() {
        let toml = r##"
[surface]
background = "#112233"
foreground = "AABBCC"

[accent]
primary = "FF0000"

[palette]
ansi_0 = "000000"
ansi_15 = "FFFFFF"

[editor]
key = "8BE9FD"

[branding]
bar_color = "4B7EB0"
"##;
        let theme = Theme::from_toml(toml);
        assert_eq!(theme.surface.background, Rgb::new(0x11, 0x22, 0x33));
        assert_eq!(theme.surface.foreground, Rgb::new(0xAA, 0xBB, 0xCC));
        assert_eq!(theme.accent.primary, Rgb::new(0xFF, 0x00, 0x00));
        assert_eq!(theme.ansi[0], Rgb::new(0, 0, 0));
        assert_eq!(theme.ansi[15], Rgb::new(0xFF, 0xFF, 0xFF));
        assert_eq!(theme.editor.key, Rgb::new(0x8B, 0xE9, 0xFD));
        assert_eq!(theme.branding.bar_color, Rgb::new(0x4B, 0x7E, 0xB0));
    }

    #[test]
    fn missing_values_use_defaults() {
        let theme = Theme::from_toml("[surface]\nbackground = \"112233\"");
        // foreground should be the default (Molten Amber).
        assert_eq!(theme.surface.foreground, Rgb::new(0xD9, 0x8E, 0x32));
        // background overridden.
        assert_eq!(theme.surface.background, Rgb::new(0x11, 0x22, 0x33));
    }

    #[test]
    fn light_variant_swaps_surface() {
        let theme = Theme::default();
        let bg = theme.surface.background;
        let fg = theme.surface.foreground;
        let light = theme.with_variant(ThemeVariant::Light);
        assert_eq!(light.surface.background, fg);
        assert_eq!(light.surface.foreground, bg);
    }

    #[test]
    fn dark_variant_unchanged() {
        let theme = Theme::default();
        let dark = theme.with_variant(ThemeVariant::Dark);
        assert_eq!(dark.surface.background, theme.surface.background);
        assert_eq!(dark.surface.foreground, theme.surface.foreground);
    }

    #[test]
    fn variant_parse() {
        assert_eq!(ThemeVariant::parse("light"), ThemeVariant::Light);
        assert_eq!(ThemeVariant::parse("dark"), ThemeVariant::Dark);
        assert_eq!(ThemeVariant::parse("unknown"), ThemeVariant::Dark);
    }

    #[test]
    fn default_uses_material_design_blue_800() {
        let theme = Theme::default();
        // Material Design Blue 800 = #1565C0
        assert_eq!(theme.accent.primary, Rgb::new(0x15, 0x65, 0xC0));
        // Material Design Red 700 = #D32F2F
        assert_eq!(theme.accent.error, Rgb::new(0xD3, 0x2F, 0x2F));
    }
}
