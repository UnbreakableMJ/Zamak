// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Boot menu TUI with theme-driven colors (FR-UI-001, FR-UI-003).

// Rust guideline compliant 2026-03-30

use crate::config::{Config, MenuEntry};
use crate::font::PsfFont;
use crate::gfx::{Canvas, Color};
use crate::wallpaper::{Bitmap, Style as WallpaperStyle};
use alloc::string::String;
use alloc::vec::Vec;
use zamak_theme::Theme;

pub enum Key {
    Up,
    Down,
    Enter,
    Edit,
    Esc,
    /// F10 — "boot now" accelerator inside the config editor (FR-UI-002).
    F10,
    /// Backspace — delete the last character of the edit buffer.
    Backspace,
    Char(char),
    None,
}

/// Severity of a live-validation finding returned by the editor validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorDiagnosticLevel {
    Ok,
    Warning,
    Error,
}

/// A single validation finding shown under the edit buffer.
#[derive(Debug, Clone)]
pub struct EditorDiagnostic {
    pub level: EditorDiagnosticLevel,
    pub message: String,
}

/// Live-validation callback signature (FR-UI-002).
///
/// Called after each keystroke with the current `edit_buffer` contents.
/// Returns a diagnostic the editor then renders beneath the input box.
/// Implementations typically call `zamak_core::config::parse` + a cheap
/// structural check and map parse failures to `Error`.
pub type EditorValidator = fn(&str) -> EditorDiagnostic;

pub trait InputSource {
    fn read_key(&mut self) -> Key;
}

pub struct MenuState {
    pub selected_idx: usize,
    pub timeout: u64, // In ticks (approx 100ms per tick)
    pub editing: bool,
    pub edit_buffer: String,
    /// When `true`, the config editor is disabled (config hash enrolled).
    pub editor_locked: bool,
    /// Which directory entries are expanded (parallel to the flattened list).
    /// Maps the flat index of a directory entry to its expand state.
    pub expanded: Vec<bool>,
    /// Optional live-validation callback invoked on every editor keystroke.
    pub validator: Option<EditorValidator>,
    /// Latest diagnostic produced by `validator`; rendered under the box.
    pub last_diagnostic: Option<EditorDiagnostic>,
    /// Set to `true` when the user has pressed F10 inside the editor,
    /// signalling the outer event loop to commit the edit and boot.
    pub boot_requested: bool,
}

impl MenuState {
    pub fn new(timeout_sec: u64) -> Self {
        Self {
            selected_idx: 0,
            timeout: timeout_sec * 10,
            editing: false,
            edit_buffer: String::new(),
            editor_locked: false,
            expanded: Vec::new(),
            validator: None,
            last_diagnostic: None,
            boot_requested: false,
        }
    }

    pub fn new_locked(timeout_sec: u64) -> Self {
        Self {
            editor_locked: true,
            ..Self::new(timeout_sec)
        }
    }

    /// Installs a live-validation callback for the config editor.
    #[must_use]
    pub fn with_validator(mut self, validator: EditorValidator) -> Self {
        self.validator = Some(validator);
        self
    }

    /// Feeds a key into the config editor (FR-UI-002).
    ///
    /// Returns `true` when the caller should commit the edit and boot
    /// (F10 pressed on a non-`Error` diagnostic).
    pub fn handle_editor_key(&mut self, key: Key) -> bool {
        if !self.editing || self.editor_locked {
            return false;
        }
        match key {
            Key::Esc => {
                self.editing = false;
                self.edit_buffer.clear();
                self.last_diagnostic = None;
                false
            }
            Key::F10 => {
                // F10 commits only if the last diagnostic is not an error.
                let allow = self
                    .last_diagnostic
                    .as_ref()
                    .map_or(true, |d| d.level != EditorDiagnosticLevel::Error);
                if allow {
                    self.boot_requested = true;
                    self.editing = false;
                    true
                } else {
                    false
                }
            }
            Key::Backspace => {
                self.edit_buffer.pop();
                self.revalidate();
                false
            }
            Key::Char(c) => {
                self.edit_buffer.push(c);
                self.revalidate();
                false
            }
            _ => false,
        }
    }

    fn revalidate(&mut self) {
        if let Some(v) = self.validator {
            self.last_diagnostic = Some(v(&self.edit_buffer));
        }
    }
}

/// A flattened entry reference for display.
pub struct FlatEntry<'a> {
    pub entry: &'a MenuEntry,
    /// Display indentation (visual depth).
    pub depth: usize,
    /// Whether this entry has children (is a directory).
    pub is_directory: bool,
    /// Whether this directory is currently expanded.
    pub is_expanded: bool,
}

/// Flattens the menu-entry tree into a display list, honoring expand state.
///
/// Each directory entry appears once, followed by its children if expanded.
/// Collapsed directories' children are skipped.
pub fn flatten_entries<'a>(entries: &'a [MenuEntry], state: &MenuState) -> Vec<FlatEntry<'a>> {
    let mut flat = Vec::new();
    flatten_recursive(entries, state, 0, &mut flat, &mut 0);
    flat
}

fn flatten_recursive<'a>(
    entries: &'a [MenuEntry],
    state: &MenuState,
    depth: usize,
    flat: &mut Vec<FlatEntry<'a>>,
    flat_idx: &mut usize,
) {
    for entry in entries {
        let is_directory = !entry.children.is_empty();
        let my_flat_idx = *flat_idx;
        let is_expanded = is_directory
            && (entry.expanded || state.expanded.get(my_flat_idx).copied().unwrap_or(false));

        flat.push(FlatEntry {
            entry,
            depth,
            is_directory,
            is_expanded,
        });
        *flat_idx += 1;

        if is_directory && is_expanded {
            flatten_recursive(&entry.children, state, depth + 1, flat, flat_idx);
        }
    }
}

/// Optional wallpaper reference passed to [`draw_menu_with_wallpaper`].
pub struct WallpaperRef<'a> {
    pub bitmap: &'a Bitmap,
    pub style: WallpaperStyle,
}

/// Draws the boot menu using theme-driven colors.
pub fn draw_menu(
    canvas: &mut Canvas,
    font: &PsfFont,
    config: &Config,
    state: &MenuState,
    theme: &Theme,
    time_remaining: u64,
) {
    draw_menu_with_wallpaper(canvas, font, config, state, theme, time_remaining, None);
}

/// Draws the boot menu with an optional wallpaper underlay (FR-UI-001).
///
/// If `wallpaper` is `Some`, the image is drawn first using the given style;
/// the theme's surface background is still painted for pixels the wallpaper
/// does not cover (only relevant for `Centered` / smaller wallpapers).
pub fn draw_menu_with_wallpaper(
    canvas: &mut Canvas,
    font: &PsfFont,
    config: &Config,
    state: &MenuState,
    theme: &Theme,
    time_remaining: u64,
    wallpaper: Option<WallpaperRef>,
) {
    let bg = Color::from(theme.surface.background);
    let fg = Color::from(theme.surface.foreground);
    let accent = Color::from(theme.accent.primary);
    let accent2 = Color::from(theme.accent.secondary);
    let error = Color::from(theme.accent.error);
    let success = Color::from(theme.accent.success);
    let brand_text = Color::from(theme.branding.text_color);
    let brand_bar = Color::from(theme.branding.bar_color);

    // 1. Clear screen, then overlay the wallpaper if any.
    canvas.clear(bg);
    if let Some(wp) = wallpaper {
        crate::wallpaper::draw(canvas, wp.bitmap, wp.style);
    }

    // 2. Header.
    let title = "STEELBORE :: ZAMAK 0.6.9";
    font.draw_string(canvas, 10, 10, title, brand_text, None);
    canvas.draw_rect(10, 24, canvas.width() - 20, 1, brand_bar);

    // 3. Entries (flattened tree).
    let flat = flatten_entries(&config.entries, state);
    let start_y = 50;
    for (i, fe) in flat.iter().enumerate() {
        let y = start_y + (i as u64 * 16);
        let indent = 20 + (fe.depth as u64 * 16);
        let selected = i == state.selected_idx;

        let entry_color = if selected { accent } else { fg };

        // Prefix: selection caret or directory marker.
        let prefix = if selected {
            ">"
        } else if fe.is_directory {
            if fe.is_expanded {
                "v"
            } else {
                ">"
            }
        } else {
            " "
        };
        font.draw_string(canvas, indent, y, prefix, entry_color, None);
        font.draw_string(
            canvas,
            indent + 16,
            y,
            fe.entry.name.as_str(),
            entry_color,
            None,
        );
    }

    // 4. Footer / timeout.
    let footer_y = canvas.height() - 20;
    if time_remaining > 0 {
        font.draw_string(
            canvas,
            10,
            footer_y,
            "BOOTING SEQUENCE INITIATED...",
            success,
            None,
        );

        let bar_width: u64 = 200;
        let denom = (config.timeout * 10).max(1);
        let progress = (time_remaining * bar_width) / denom;
        canvas.draw_rect(
            canvas.width() - 220,
            footer_y,
            bar_width,
            10,
            Color {
                r: 0,
                g: 0x20,
                b: 0,
            },
        );
        canvas.draw_rect(canvas.width() - 220, footer_y, progress, 10, success);
    } else {
        font.draw_string(
            canvas,
            10,
            footer_y,
            "READY. AWAITING COMMAND.",
            accent,
            None,
        );
    }

    // 5. Hash-lock indicator.
    if state.editor_locked {
        let lock_y = footer_y - 16;
        font.draw_string(
            canvas,
            10,
            lock_y,
            "[CONFIG HASH ENROLLED - EDITOR LOCKED]",
            error,
            None,
        );
    }

    // 6. Edit overlay (suppressed when locked).
    if state.editing && !state.editor_locked {
        let edit_y = canvas.height() / 2;
        let edit_bg = Color::from(theme.surface.dim);
        canvas.draw_rect(40, edit_y - 20, canvas.width() - 80, 40, edit_bg);
        canvas.draw_rect(40, edit_y - 20, canvas.width() - 80, 1, accent);
        canvas.draw_rect(40, edit_y + 20, canvas.width() - 80, 1, accent);

        font.draw_string(
            canvas,
            50,
            edit_y - 15,
            "KERNEL ARGUMENTS:",
            accent,
            Some(edit_bg),
        );
        font.draw_string(
            canvas,
            50,
            edit_y,
            &state.edit_buffer,
            accent2,
            Some(edit_bg),
        );

        let cursor_x = 50 + (state.edit_buffer.len() as u64 * 8);
        canvas.draw_rect(cursor_x, edit_y, 8, 16, accent2);

        // Hint + live diagnostic (FR-UI-002).
        font.draw_string(
            canvas,
            50,
            edit_y + 22,
            "[F10] boot  [Esc] cancel",
            accent,
            Some(edit_bg),
        );
        if let Some(diag) = &state.last_diagnostic {
            let (color, prefix) = match diag.level {
                EditorDiagnosticLevel::Ok => (success, "OK  "),
                EditorDiagnosticLevel::Warning => (accent2, "WARN"),
                EditorDiagnosticLevel::Error => (error, "ERR "),
            };
            font.draw_string(canvas, 50, edit_y + 36, prefix, color, Some(edit_bg));
            font.draw_string(canvas, 90, edit_y + 36, diag.message.as_str(), color, Some(edit_bg));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::parse;

    #[test]
    fn flatten_leaf_entries_only() {
        let config = parse("/Linux\n/BSD\n");
        let state = MenuState::new(5);
        let flat = flatten_entries(&config.entries, &state);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].depth, 0);
        assert!(!flat[0].is_directory);
    }

    #[test]
    fn flatten_collapsed_directory_hides_children() {
        let content = "\
/Distros
//Arch
//Debian
";
        let config = parse(content);
        let state = MenuState::new(5);
        let flat = flatten_entries(&config.entries, &state);
        // Only the directory should show (not expanded).
        assert_eq!(flat.len(), 1);
        assert!(flat[0].is_directory);
        assert!(!flat[0].is_expanded);
    }

    #[test]
    fn flatten_expanded_directory_shows_children() {
        let content = "\
/+Distros
//Arch
//Debian
";
        let config = parse(content);
        let state = MenuState::new(5);
        let flat = flatten_entries(&config.entries, &state);
        // Directory + 2 children.
        assert_eq!(flat.len(), 3);
        assert!(flat[0].is_directory);
        assert!(flat[0].is_expanded);
        assert_eq!(flat[1].depth, 1);
        assert_eq!(flat[2].depth, 1);
        assert_eq!(flat[1].entry.name, "Arch");
        assert_eq!(flat[2].entry.name, "Debian");
    }

    #[test]
    fn flatten_runtime_expand_override() {
        let content = "\
/Distros
//Arch
//Debian
";
        let config = parse(content);
        let mut state = MenuState::new(5);
        // Expand the first directory via runtime state.
        state.expanded = alloc::vec![true];
        let flat = flatten_entries(&config.entries, &state);
        assert_eq!(flat.len(), 3);
    }

    // ---------- Editor F10 + validator tests (M3-10) ----------

    fn stub_validator(buf: &str) -> EditorDiagnostic {
        if buf.contains("error") {
            EditorDiagnostic {
                level: EditorDiagnosticLevel::Error,
                message: String::from("contains 'error'"),
            }
        } else if buf.is_empty() {
            EditorDiagnostic {
                level: EditorDiagnosticLevel::Warning,
                message: String::from("cmdline is empty"),
            }
        } else {
            EditorDiagnostic {
                level: EditorDiagnosticLevel::Ok,
                message: String::from("looks fine"),
            }
        }
    }

    #[test]
    fn editor_f10_commits_on_ok_diagnostic() {
        let mut state = MenuState::new(5).with_validator(stub_validator);
        state.editing = true;
        state.handle_editor_key(Key::Char('q'));
        state.handle_editor_key(Key::Char('u'));
        state.handle_editor_key(Key::Char('i'));
        state.handle_editor_key(Key::Char('e'));
        state.handle_editor_key(Key::Char('t'));
        assert_eq!(
            state.last_diagnostic.as_ref().map(|d| d.level),
            Some(EditorDiagnosticLevel::Ok)
        );

        let booted = state.handle_editor_key(Key::F10);
        assert!(booted);
        assert!(state.boot_requested);
        assert!(!state.editing);
    }

    #[test]
    fn editor_f10_refuses_on_error_diagnostic() {
        let mut state = MenuState::new(5).with_validator(stub_validator);
        state.editing = true;
        for c in "this is an error buffer".chars() {
            state.handle_editor_key(Key::Char(c));
        }
        assert_eq!(
            state.last_diagnostic.as_ref().map(|d| d.level),
            Some(EditorDiagnosticLevel::Error)
        );

        let booted = state.handle_editor_key(Key::F10);
        assert!(!booted);
        assert!(!state.boot_requested);
        assert!(state.editing, "editor should stay open on error-gated F10");
    }

    #[test]
    fn editor_esc_clears_buffer_and_exits() {
        let mut state = MenuState::new(5).with_validator(stub_validator);
        state.editing = true;
        state.handle_editor_key(Key::Char('x'));
        state.handle_editor_key(Key::Esc);
        assert!(!state.editing);
        assert!(state.edit_buffer.is_empty());
        assert!(state.last_diagnostic.is_none());
    }

    #[test]
    fn editor_backspace_shrinks_buffer_and_revalidates() {
        let mut state = MenuState::new(5).with_validator(stub_validator);
        state.editing = true;
        for c in "abc".chars() {
            state.handle_editor_key(Key::Char(c));
        }
        assert_eq!(state.edit_buffer, "abc");
        state.handle_editor_key(Key::Backspace);
        assert_eq!(state.edit_buffer, "ab");
        assert_eq!(
            state.last_diagnostic.as_ref().map(|d| d.level),
            Some(EditorDiagnosticLevel::Ok)
        );
    }

    #[test]
    fn editor_ignores_input_when_locked() {
        let mut state = MenuState::new_locked(5).with_validator(stub_validator);
        state.editing = true;
        state.handle_editor_key(Key::Char('x'));
        // Locked editor must not consume input at all.
        assert!(state.edit_buffer.is_empty());
        assert!(state.last_diagnostic.is_none());
    }

    #[test]
    fn editor_ignores_input_when_not_editing() {
        let mut state = MenuState::new(5).with_validator(stub_validator);
        state.editing = false;
        state.handle_editor_key(Key::Char('x'));
        assert!(state.edit_buffer.is_empty());
    }
}
