// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::string::ToString;

#[derive(Debug, Default)]
pub struct Config {
    pub global_options: BTreeMap<String, String>,
    pub entries: Vec<MenuEntry>,
}

#[derive(Debug, Default)]
pub struct MenuEntry {
    pub name: String,
    pub level: usize,
    pub expanded: bool,
    pub options: BTreeMap<String, String>,
}

pub fn parse(content: &str) -> Config {
    let mut config = Config::default();
    let mut current_entry: Option<MenuEntry> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('/') {
            // New menu entry
            if let Some(entry) = current_entry.take() {
                config.entries.push(entry);
            }

            let mut level = 0;
            let mut chars = line.chars();
            let mut current_char = chars.next();
            while let Some('/') = current_char {
                level += 1;
                current_char = chars.next();
            }

            let mut name_part = String::new();
            if let Some(c) = current_char {
                name_part.push(c);
            }
            name_part.push_str(chars.as_str());
            
            let name_part = name_part.trim();
            let mut name = name_part.to_string();
            let mut expanded = false;

            if name.starts_with('+') {
                expanded = true;
                name = name[1..].trim().to_string();
            }

            current_entry = Some(MenuEntry {
                name,
                level,
                expanded,
                ..Default::default()
            });
        } else if let Some(colon_idx) = line.find(':') {
            // Option
            let key = line[..colon_idx].trim().to_uppercase();
            let value = line[colon_idx + 1..].trim().to_string();

            if let Some(ref mut entry) = current_entry {
                entry.options.insert(key, value);
            } else {
                config.global_options.insert(key, value);
            }
        }
    }

    if let Some(entry) = current_entry {
        config.entries.push(entry);
    }

    config
}
