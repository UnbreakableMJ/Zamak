// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Configuration file parser with macro expansion (FR-CFG-001, FR-CFG-002).
//!
//! Parses the Limine-compatible `zamak.conf` format with support for:
//! - `${NAME}=value` macro definitions
//! - `${NAME}` variable references in values
//! - Built-in variables: `${ARCH}`, `${FW_TYPE}`, `${BOOT_DRIVE}`

// Rust guideline compliant 2026-03-30

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Debug, Default)]
pub struct Config {
    pub global_options: BTreeMap<String, String>,
    pub entries: Vec<MenuEntry>,
    pub timeout: u64,
    /// Path to `zamak-theme.toml` (Limine URI syntax). `None` uses built-in theme.
    pub theme_path: Option<String>,
    /// Theme variant: `"dark"` (default) or `"light"` (§7.1).
    pub theme_variant: String,
    /// Enrolled BLAKE2B-256 config hash. When `Some`, the config editor is disabled
    /// and the config is verified against this hash at boot (FR-CFG-006).
    pub config_hash: Option<[u8; 32]>,
    /// Whether the built-in config editor is enabled.
    /// Automatically set to `false` when `config_hash` is `Some`.
    pub editor_enabled: bool,
    /// 1-based index of the default boot entry.
    pub default_entry: usize,
    /// Quiet mode — suppress all screen output except panics.
    pub quiet: bool,
    /// Enable serial I/O.
    pub serial: bool,
    /// Serial baudrate (default 115200, BIOS only).
    pub serial_baudrate: u32,
    /// Verbose boot logging.
    pub verbose: bool,
    /// If `false`, hash mismatches produce a warning instead of a panic.
    pub hash_mismatch_panic: bool,
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
    pub comment: String,
    pub options: BTreeMap<String, String>,
    pub modules: Vec<ModuleConfig>,
    /// Nesting depth: 1 for `/Entry`, 2 for `//SubEntry`, etc.
    pub depth: usize,
    /// If `true`, this directory entry is expanded by default (Limine `+` prefix).
    pub expanded: bool,
    /// Sub-entries of this directory entry.
    pub children: Vec<MenuEntry>,
}

/// Boot environment passed to the parser for built-in macro expansion.
#[derive(Debug, Clone)]
pub struct BootEnvironment {
    /// Architecture string: "x86_64", "ia32", "aarch64", "riscv64", "loongarch64".
    pub arch: &'static str,
    /// Firmware type: "bios" or "uefi".
    pub fw_type: &'static str,
    /// Boot drive identifier (e.g., "0x80" for first HDD).
    pub boot_drive: String,
}

impl Default for BootEnvironment {
    fn default() -> Self {
        Self {
            arch: if cfg!(target_arch = "x86_64") {
                "x86_64"
            } else if cfg!(target_arch = "x86") {
                "ia32"
            } else if cfg!(target_arch = "aarch64") {
                "aarch64"
            } else if cfg!(target_arch = "riscv64") {
                "riscv64"
            } else {
                "unknown"
            },
            fw_type: "bios",
            boot_drive: String::new(),
        }
    }
}

/// Parses the config with default environment (no macro expansion).
pub fn parse(content: &str) -> Config {
    parse_with_env(content, &BootEnvironment::default())
}

/// Parses the config with the given boot environment for macro expansion.
pub fn parse_with_env(content: &str, env: &BootEnvironment) -> Config {
    let mut config = Config {
        timeout: 5,
        theme_variant: String::from("dark"),
        editor_enabled: true,
        default_entry: 1,
        serial_baudrate: 115200,
        hash_mismatch_panic: true,
        ..Config::default()
    };
    let mut current_entry: Option<MenuEntry> = None;

    // User-defined macros (${NAME}=value).
    let mut macros = BTreeMap::<String, String>::new();

    // Seed built-in macros (FR-CFG-002).
    macros.insert("ARCH".into(), env.arch.into());
    macros.insert("FW_TYPE".into(), env.fw_type.into());
    if !env.boot_drive.is_empty() {
        macros.insert("BOOT_DRIVE".into(), env.boot_drive.clone());
    }

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Check for macro definition: ${NAME}=value
        if let Some(def) = parse_macro_definition(line) {
            let expanded_value = expand_macros(&def.1, &macros);
            macros.insert(def.0, expanded_value);
            continue;
        }

        if line.starts_with(':') || line.starts_with('/') {
            // Count depth (number of leading `/` characters).
            let depth = if line.starts_with('/') {
                line.bytes().take_while(|&b| b == b'/').count()
            } else {
                1 // `:` prefix = depth 1 (legacy compatibility)
            };

            // Strip the leading `/`s or `:`.
            let rest = if line.starts_with('/') {
                &line[depth..]
            } else {
                &line[1..]
            };

            // Check for `+` prefix (expanded directory marker).
            let (expanded, title) = if let Some(after_plus) = rest.strip_prefix('+') {
                (true, after_plus.trim())
            } else {
                (false, rest.trim())
            };

            let name = expand_macros(title, &macros);

            // Flush previous entry.
            if let Some(entry) = current_entry.take() {
                push_entry(&mut config.entries, entry);
            }

            current_entry = Some(MenuEntry {
                name,
                depth,
                expanded,
                ..Default::default()
            });
        } else {
            let (key, value) = if let Some(idx) = line.find('=') {
                (
                    line[..idx].trim().to_uppercase(),
                    expand_macros(line[idx + 1..].trim(), &macros),
                )
            } else if let Some(idx) = line.find(':') {
                (
                    line[..idx].trim().to_uppercase(),
                    expand_macros(line[idx + 1..].trim(), &macros),
                )
            } else {
                continue;
            };

            if let Some(ref mut entry) = current_entry {
                match key.as_str() {
                    "PROTOCOL" => entry.protocol = value,
                    "KERNEL_PATH" | "PATH" => entry.kernel_path = value,
                    "CMDLINE" | "KERNEL_CMDLINE" => entry.cmdline = value,
                    "COMMENT" => entry.comment = value,
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
                match key.as_str() {
                    "TIMEOUT" => {
                        if let Ok(val) = value.parse::<u64>() {
                            config.timeout = val;
                        }
                    }
                    "THEME" => {
                        config.theme_path = Some(value.clone());
                    }
                    "THEME_VARIANT" => {
                        config.theme_variant = value.clone();
                    }
                    "EDITOR_ENABLED" => {
                        config.editor_enabled = value == "yes" || value == "true" || value == "1";
                    }
                    "DEFAULT_ENTRY" => {
                        if let Ok(val) = value.parse::<usize>() {
                            config.default_entry = val;
                        }
                    }
                    "QUIET" => {
                        config.quiet = value == "yes";
                    }
                    "SERIAL" => {
                        config.serial = value == "yes";
                    }
                    "SERIAL_BAUDRATE" => {
                        if let Ok(val) = value.parse::<u32>() {
                            config.serial_baudrate = val;
                        }
                    }
                    "VERBOSE" => {
                        config.verbose = value == "yes";
                    }
                    "HASH_MISMATCH_PANIC" => {
                        config.hash_mismatch_panic = value != "no";
                    }
                    _ => {}
                }
                config.global_options.insert(key, value);
            }
        }
    }

    if let Some(entry) = current_entry {
        push_entry(&mut config.entries, entry);
    }

    config
}

/// Pushes an entry into the tree, nesting it under the appropriate parent
/// based on its depth. Depth 1 entries go at the top level; depth 2+ entries
/// become children of the most recent entry at depth-1.
fn push_entry(entries: &mut Vec<MenuEntry>, entry: MenuEntry) {
    if entry.depth <= 1 {
        entries.push(entry);
        return;
    }

    // Find the most recent top-level entry to nest under.
    if let Some(parent) = entries.last_mut() {
        push_entry_recursive(&mut parent.children, entry, 2);
    } else {
        // No parent — push as top-level anyway.
        entries.push(entry);
    }
}

fn push_entry_recursive(children: &mut Vec<MenuEntry>, entry: MenuEntry, current_depth: usize) {
    if entry.depth <= current_depth {
        children.push(entry);
    } else if let Some(parent) = children.last_mut() {
        push_entry_recursive(&mut parent.children, entry, current_depth + 1);
    } else {
        children.push(entry);
    }
}

/// Checks if a line is a macro definition (`${NAME}=value`).
///
/// Returns `Some((name, value))` if it matches, `None` otherwise.
fn parse_macro_definition(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if !line.starts_with("${") {
        return None;
    }

    // Find the closing `}`.
    let close = line.find('}')?;
    let name = &line[2..close];

    // Must be followed by `=`.
    let rest = &line[close + 1..];
    if !rest.starts_with('=') {
        return None;
    }

    let value = rest[1..].trim();
    Some((name.to_string(), value.to_string()))
}

/// Expands `${NAME}` references in a string using the macro table.
///
/// Unknown macros are left as-is (e.g., `${UNKNOWN}` passes through).
/// Expansion is single-pass (no recursive expansion).
fn expand_macros(input: &str, macros: &BTreeMap<String, String>) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && bytes[i] == b'$' && bytes[i + 1] == b'{' {
            // Find closing '}'.
            if let Some(close_offset) = input[i + 2..].find('}') {
                let name = &input[i + 2..i + 2 + close_offset];
                if let Some(value) = macros.get(name) {
                    result.push_str(value);
                } else {
                    // Unknown macro — pass through verbatim.
                    result.push_str(&input[i..i + 2 + close_offset + 1]);
                }
                i += 2 + close_offset + 1;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }

    result
}

/// Enrolls a BLAKE2B-256 config hash into the parsed config.
///
/// When a hash is enrolled, the editor is automatically disabled (FR-CFG-006).
/// At boot, the config file content should be verified against this hash
/// using [`verify_config_hash`].
pub fn enroll_config_hash(config: &mut Config, hash: [u8; 32]) {
    config.config_hash = Some(hash);
    config.editor_enabled = false;
}

/// Verifies config file content against the enrolled hash.
///
/// Returns `true` if no hash is enrolled (open config) or if the hash matches.
/// Returns `false` if a hash is enrolled and the content does not match.
/// On mismatch, the bootloader should panic per FR-CFG-006.
pub fn verify_config_hash(config: &Config, config_content: &[u8]) -> bool {
    let Some(enrolled) = config.config_hash else {
        return true;
    };
    let full = crate::blake2b::Blake2b::hash(config_content, 32);
    // Constant-time comparison against the first 32 bytes.
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= enrolled[i] ^ full[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_builtin_macros() {
        let env = BootEnvironment {
            arch: "x86_64",
            fw_type: "uefi",
            boot_drive: "0x80".into(),
        };
        let content = "\
TIMEOUT=5

:My Kernel (${ARCH})
PROTOCOL=limine
KERNEL_PATH=boot():/boot/kernel-${ARCH}
CMDLINE=fw=${FW_TYPE} drive=${BOOT_DRIVE}
";
        let config = parse_with_env(content, &env);
        assert_eq!(config.entries.len(), 1);
        let entry = &config.entries[0];
        assert_eq!(entry.name, "My Kernel (x86_64)");
        assert_eq!(entry.kernel_path, "boot():/boot/kernel-x86_64");
        assert_eq!(entry.cmdline, "fw=uefi drive=0x80");
    }

    #[test]
    fn user_defined_macros() {
        let content = "\
${KERNEL_VER}=6.8.0
${ROOT}=boot():/boot

:Linux ${KERNEL_VER}
PROTOCOL=linux
KERNEL_PATH=${ROOT}/vmlinuz-${KERNEL_VER}
CMDLINE=root=/dev/sda1
";
        let config = parse(content);
        assert_eq!(config.entries.len(), 1);
        let entry = &config.entries[0];
        assert_eq!(entry.name, "Linux 6.8.0");
        assert_eq!(entry.kernel_path, "boot():/boot/vmlinuz-6.8.0");
    }

    #[test]
    fn unknown_macros_pass_through() {
        let mut macros = BTreeMap::new();
        macros.insert("A".into(), "hello".into());
        assert_eq!(expand_macros("${A} ${B}", &macros), "hello ${B}");
    }

    #[test]
    fn theme_global_options() {
        let content = "\
TIMEOUT=3
THEME=boot():/boot/zamak-theme.toml
THEME_VARIANT=light

:Test
PROTOCOL=limine
KERNEL_PATH=boot():/boot/kernel
";
        let config = parse(content);
        assert_eq!(
            config.theme_path.as_deref(),
            Some("boot():/boot/zamak-theme.toml")
        );
        assert_eq!(config.theme_variant, "light");
        assert!(config.editor_enabled);
    }

    #[test]
    fn editor_disabled_by_config() {
        let config = parse("EDITOR_ENABLED=no\n");
        assert!(!config.editor_enabled);
    }

    #[test]
    fn config_hash_enrollment_disables_editor() {
        let mut config = parse("TIMEOUT=5\n");
        assert!(config.editor_enabled);
        enroll_config_hash(&mut config, [0xAA; 32]);
        assert!(!config.editor_enabled);
        assert_eq!(config.config_hash, Some([0xAA; 32]));
    }

    #[test]
    fn config_hash_verification() {
        let content = b"TIMEOUT=5\n";
        let full = crate::blake2b::Blake2b::hash(content, 32);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&full[..32]);
        let mut config = Config::default();
        enroll_config_hash(&mut config, hash);
        assert!(verify_config_hash(&config, content));
        assert!(!verify_config_hash(&config, b"TIMEOUT=10\n"));
    }

    #[test]
    fn no_hash_always_verifies() {
        let config = Config::default();
        assert!(verify_config_hash(&config, b"anything"));
    }

    #[test]
    fn limine_style_slash_entries() {
        let content = "\
timeout: 5

/Linux 6.8
PROTOCOL: linux
PATH: boot():/boot/vmlinuz
CMDLINE: root=/dev/sda1
";
        let config = parse(content);
        assert_eq!(config.timeout, 5);
        assert_eq!(config.entries.len(), 1);
        assert_eq!(config.entries[0].name, "Linux 6.8");
        assert_eq!(config.entries[0].depth, 1);
        assert_eq!(config.entries[0].protocol, "linux");
    }

    #[test]
    fn sub_entries_with_depth() {
        let content = "\
/+Linux Distros
//Arch Linux
PROTOCOL: linux
PATH: boot():/boot/arch-vmlinuz

//Debian
PROTOCOL: linux
PATH: boot():/boot/debian-vmlinuz
";
        let config = parse(content);
        assert_eq!(config.entries.len(), 1);
        let dir = &config.entries[0];
        assert_eq!(dir.name, "Linux Distros");
        assert!(dir.expanded);
        assert_eq!(dir.depth, 1);
        assert_eq!(dir.children.len(), 2);
        assert_eq!(dir.children[0].name, "Arch Linux");
        assert_eq!(dir.children[0].depth, 2);
        assert_eq!(dir.children[1].name, "Debian");
    }

    #[test]
    fn comment_option() {
        let content = "\
/My OS
COMMENT: This is my custom kernel
PROTOCOL: limine
PATH: boot():/boot/kernel
";
        let config = parse(content);
        assert_eq!(config.entries[0].comment, "This is my custom kernel");
    }

    #[test]
    fn global_options_limine_style() {
        let content = "\
timeout: 10
quiet: yes
serial: yes
serial_baudrate: 9600
default_entry: 2
verbose: yes
hash_mismatch_panic: no
";
        let config = parse(content);
        assert_eq!(config.timeout, 10);
        assert!(config.quiet);
        assert!(config.serial);
        assert_eq!(config.serial_baudrate, 9600);
        assert_eq!(config.default_entry, 2);
        assert!(config.verbose);
        assert!(!config.hash_mismatch_panic);
    }
}
