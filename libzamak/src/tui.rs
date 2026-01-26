// SPDX-License-Identifier: GPL-3.0-or-later

use crate::gfx::{Canvas, Color, VOID_NAVY, MOLTEN_AMBER, STEEL_BLUE, RADIUM_GREEN};
use crate::font::PsfFont;
use crate::config::Config;
use alloc::string::String;
use alloc::vec::Vec;

pub enum Key {
    Up,
    Down,
    Enter,
    Edit,
    Esc,
    Char(char),
    None,
}

pub trait InputSource {
    fn read_key(&mut self) -> Key;
}

pub struct MenuState {
    pub selected_idx: usize,
    pub timeout: u64, // In ticks (approx 100ms per tick)
    pub editing: bool,
    pub edit_buffer: String,
}

impl MenuState {
    pub fn new(timeout_sec: u64) -> Self {
        Self {
            selected_idx: 0,
            timeout: timeout_sec * 10, // 100ms ticks
            editing: false,
            edit_buffer: String::new(),
        }
    }
}

pub fn draw_menu(canvas: &mut Canvas, font: &PsfFont, config: &Config, state: &MenuState, time_remaining: u64) {
    // 1. Clear Screen
    canvas.clear(VOID_NAVY);

    // 2. Draw Header (Steelbore Logo / Title)
    let title = "STEELBORE :: ZAMAK 0.6.9";
    font.draw_string(canvas, 10, 10, title, STEEL_BLUE, None);
    
    // Draw separator
    canvas.draw_rect(10, 24, canvas.width() - 20, 1, STEEL_BLUE);

    // 3. Draw Entries
    let start_y = 50;
    for (i, entry) in config.entries.iter().enumerate() {
        let y = start_y + (i as u64 * 16);
        let prefix = if i == state.selected_idx { "> " } else { "  " };
        let name = entry.name.as_str();
        
        // Highlight selection
        let fg = if i == state.selected_idx { MOLTEN_AMBER } else { STEEL_BLUE };
        
        font.draw_string(canvas, 20, y, prefix, fg, None);
        font.draw_string(canvas, 40, y, name, fg, None);
    }

    // 4. Draw Footer / Timeout
    let footer_y = canvas.height() - 20;
    if time_remaining > 0 {
        // Simple string building to avoid fmt overhead if needed, but we have alloc
        // "Booting in X..."
        font.draw_string(canvas, 10, footer_y, "BOOTING SEQUENCE INITIATED...", RADIUM_GREEN, None);
        
        // Timeout Gauge
        let bar_width = 200;
        let progress = (time_remaining as u64 * bar_width) / (config.timeout as u64 * 10).max(1);
        canvas.draw_rect(canvas.width() - 220, footer_y, bar_width, 10,  Color{r:0,g:0x20,b:0}); 
        canvas.draw_rect(canvas.width() - 220, footer_y, progress, 10, RADIUM_GREEN);
    } else {
        font.draw_string(canvas, 10, footer_y, "READY. AWAITING COMMAND.", STEEL_BLUE, None);
    }

    // 5. Edit Mode Overlay
    if state.editing {
        let edit_y = canvas.height() / 2;
        let edit_bg = Color { r: 0x10, g: 0x10, b: 0x30 };
        // Draw box
        canvas.draw_rect(40, edit_y - 20, canvas.width() - 80, 40, edit_bg);
        canvas.draw_rect(40, edit_y - 20, canvas.width() - 80, 1, STEEL_BLUE);
        canvas.draw_rect(40, edit_y + 20, canvas.width() - 80, 1, STEEL_BLUE);
        
        font.draw_string(canvas, 50, edit_y - 15, "KERNEL ARGUMENTS:", STEEL_BLUE, Some(edit_bg));
        font.draw_string(canvas, 50, edit_y, &state.edit_buffer, MOLTEN_AMBER, Some(edit_bg));
        
        // Cursor
        let cursor_x = 50 + (state.edit_buffer.len() as u64 * 8);
        canvas.draw_rect(cursor_x, edit_y, 8, 16, MOLTEN_AMBER);
    }
}
