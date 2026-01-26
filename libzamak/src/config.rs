// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;
use alloc::string::ToString;

#[derive(Debug, Default)]
pub struct Config {
    pub global_options: BTreeMap<String, String>,
    pub entries: Vec<MenuEntry>,
    pub timeout: u64,
}

#[derive(Debug, Default, Clone)]
pub struct ModuleConfig {
    pub path: String,
    pub string: String,
}

#[derive(Debug, Default)]
pub struct MenuEntry {
    pub name: String,
    pub protocol: String,
    pub kernel_path: String,
    pub cmdline: String,
    pub options: BTreeMap<String, String>,
    pub modules: Vec<ModuleConfig>,
}

pub fn parse(content: &str) -> Config {
    let mut config = Config::default();
    config.timeout = 5; // Default timeout
    let mut current_entry: Option<MenuEntry> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with(':') || line.starts_with('/') {
            // New menu entry
            if let Some(entry) = current_entry.take() {
                config.entries.push(entry);
            }

            let name = line[1..].trim().to_string();
            current_entry = Some(MenuEntry {
                name,
                ..Default::default()
            });
        } else {
            let (key, value) = if let Some(idx) = line.find('=') {
                (line[..idx].trim().to_uppercase(), line[idx + 1..].trim().to_string())
            } else if let Some(idx) = line.find(':') {
                (line[..idx].trim().to_uppercase(), line[idx + 1..].trim().to_string())
            } else {
                continue;
            };

            if let Some(ref mut entry) = current_entry {
                match key.as_str() {
                    "PROTOCOL" => entry.protocol = value,
                    "KERNEL_PATH" | "PATH" => entry.kernel_path = value,
                    "CMDLINE" | "KERNEL_CMDLINE" => entry.cmdline = value,
                    "MODULE_PATH" => {
                        entry.modules.push(ModuleConfig {
                            path: value,
                            string: String::new(),
                        });
                    }
                    "MODULE_STRING" | "MODULE_CMDLINE" => {
                        if let Some(last) = entry.modules.last_mut() {
                            last.string = value;
                        }
                    }
                    _ => {
                        entry.options.insert(key, value);
                    }
                }
            } else {
                if key == "TIMEOUT" {
                    if let Ok(val) = value.parse::<u64>() {
                        config.timeout = val;
                    }
                }
                config.global_options.insert(key, value);
            }
        }
    }

    if let Some(entry) = current_entry {
        config.entries.push(entry);
    }

    config
}
