// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! JSON Schema (Draft 2020-12) definitions for every sub-command.
//!
//! Single source of truth consumed by:
//! - `zamak schema` (SFRS §6.1)
//! - `zamak describe` (SFRS §6.2)
//! - a future `zamak mcp` server, once the sub-command count exceeds
//!   the §3.8 threshold (>10).

use crate::error::ErrorCode;
use crate::json::{obj, Value};

pub const SCHEMA_URL: &str = "https://json-schema.org/draft/2020-12/schema";

/// Descriptor of one sub-command, aligning with the §6.2 manifest.
pub struct CommandSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub destructive: bool,
    pub idempotent: bool,
    pub supports_dry_run: bool,
    pub supports_json: bool,
    pub supports_fields: bool,
    pub parameters: fn() -> Value,
    pub output: fn() -> Value,
    pub exit_codes: &'static [ErrorCode],
    pub examples: fn() -> Value,
}

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "install",
        description: "Write stage1 MBR and record stage2 location (FR-CLI-001)",
        destructive: true,
        idempotent: true,
        supports_dry_run: true,
        supports_json: true,
        supports_fields: true,
        parameters: params_install,
        output: output_install,
        exit_codes: EXIT_CODES_DEFAULT,
        examples: examples_install,
    },
    CommandSpec {
        name: "enroll-config",
        description: "Compute BLAKE2B-256 config hash and patch EFI binary (FR-CLI-002)",
        destructive: true,
        idempotent: true,
        supports_dry_run: true,
        supports_json: true,
        supports_fields: true,
        parameters: params_enroll,
        output: output_enroll,
        exit_codes: EXIT_CODES_DEFAULT,
        examples: examples_enroll,
    },
    CommandSpec {
        name: "sbom",
        description: "Generate SPDX 2.3 JSON SBOM for a release (FR-CLI-003)",
        destructive: false,
        idempotent: true,
        supports_dry_run: false,
        supports_json: true,
        supports_fields: true,
        parameters: params_sbom,
        output: output_sbom,
        exit_codes: EXIT_CODES_DEFAULT,
        examples: examples_sbom,
    },
    CommandSpec {
        name: "schema",
        description: "Emit JSON Schema for the whole tool or a specific command",
        destructive: false,
        idempotent: true,
        supports_dry_run: false,
        supports_json: true,
        supports_fields: false,
        parameters: params_schema,
        output: output_schema,
        exit_codes: EXIT_CODES_DEFAULT,
        examples: examples_schema,
    },
    CommandSpec {
        name: "describe",
        description: "Emit capability manifest for every sub-command",
        destructive: false,
        idempotent: true,
        supports_dry_run: false,
        supports_json: true,
        supports_fields: false,
        parameters: params_describe,
        output: output_describe,
        exit_codes: EXIT_CODES_DEFAULT,
        examples: examples_describe,
    },
    CommandSpec {
        name: "completions",
        description: "Emit shell completion script for bash, zsh, fish, or nushell",
        destructive: false,
        idempotent: true,
        supports_dry_run: false,
        supports_json: false,
        supports_fields: false,
        parameters: params_completions,
        output: output_completions,
        exit_codes: EXIT_CODES_DEFAULT,
        examples: examples_completions,
    },
];

pub const EXIT_CODES_DEFAULT: &[ErrorCode] = &[
    ErrorCode::Success,
    ErrorCode::General,
    ErrorCode::UsageError,
    ErrorCode::NotFound,
    ErrorCode::PermissionDenied,
    ErrorCode::Conflict,
    ErrorCode::InvalidArgument,
];

pub fn find(name: &str) -> Option<&'static CommandSpec> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// Builds the complete JSON Schema document for the whole tool.
pub fn full_schema() -> Value {
    let cmds: Vec<Value> = COMMANDS.iter().map(command_schema).collect();
    obj([
        ("$schema", Value::str(SCHEMA_URL)),
        ("title", Value::str("zamak")),
        ("description", Value::str("ZAMAK bootloader host CLI")),
        ("version", Value::str(crate::meta::TOOL_VERSION)),
        ("commands", Value::Array(cmds)),
    ])
}

pub fn command_schema(spec: &CommandSpec) -> Value {
    let exit_map: Vec<(String, Value)> = spec
        .exit_codes
        .iter()
        .map(|c| (c.exit_code().to_string(), Value::str(c.as_str())))
        .collect();
    obj([
        ("$schema", Value::str(SCHEMA_URL)),
        ("name", Value::str(spec.name)),
        ("description", Value::str(spec.description)),
        ("destructive", Value::Bool(spec.destructive)),
        ("idempotent", Value::Bool(spec.idempotent)),
        ("supports_dry_run", Value::Bool(spec.supports_dry_run)),
        ("supports_json", Value::Bool(spec.supports_json)),
        ("supports_fields", Value::Bool(spec.supports_fields)),
        ("parameters", (spec.parameters)()),
        ("output", (spec.output)()),
        ("exit_codes", Value::Object(exit_map)),
        ("examples", (spec.examples)()),
    ])
}

// ---------- parameters ----------

fn params_install() -> Value {
    schema_object(&[
        param(
            "mbr",
            "string",
            true,
            "Path to stage1 MBR binary (512 bytes)",
        ),
        param("stage2", "string", true, "Path to stage2 binary"),
        param("target", "string", true, "Target device or image file"),
        param_with_default(
            "stage2-lba",
            "integer",
            false,
            "LBA where stage2 is written",
            Value::Int(1),
        ),
    ])
}

fn params_enroll() -> Value {
    schema_object(&[
        param("config", "string", true, "Path to zamak.conf"),
        param("efi", "string", true, "Path to BOOTX64.EFI"),
    ])
}

fn params_sbom() -> Value {
    schema_object(&[
        param("version", "string", true, "ZAMAK release version"),
        param(
            "output",
            "string",
            false,
            "Write SBOM to this file instead of stdout",
        ),
        param(
            "artifacts",
            "array<string>",
            false,
            "Paths to release binaries for SHA-256 inclusion",
        ),
    ])
}

fn params_schema() -> Value {
    schema_object(&[param(
        "command",
        "string",
        false,
        "Emit schema for this specific sub-command (omit for the full tool schema)",
    )])
}

fn params_describe() -> Value {
    schema_object(&[])
}

fn params_completions() -> Value {
    schema_object(&[param_with_enum(
        "shell",
        "string",
        true,
        "Target shell",
        &["bash", "zsh", "fish", "nushell"],
    )])
}

// ---------- output ----------

fn output_install() -> Value {
    shape_object(
        "object",
        "Install result",
        &[
            ("mbr_bytes_written", "integer"),
            ("stage2_bytes_written", "integer"),
            ("stage2_sectors", "integer"),
            ("stage2_lba", "integer"),
            ("target", "string"),
            ("dry_run", "boolean"),
        ],
    )
}

fn output_enroll() -> Value {
    shape_object(
        "object",
        "Enrollment result",
        &[
            ("config", "string"),
            ("efi", "string"),
            ("blake2b_256", "string"),
            ("patch_offset", "integer"),
            ("dry_run", "boolean"),
        ],
    )
}

fn output_sbom() -> Value {
    shape_object(
        "object",
        "SPDX 2.3 document (JSON payload under `data`)",
        &[("spdxVersion", "string"), ("name", "string")],
    )
}

fn output_schema() -> Value {
    shape_simple("object", "A JSON Schema Draft 2020-12 document")
}

fn output_describe() -> Value {
    shape_simple("object", "Capability manifest (commands, formats, flags)")
}

fn output_completions() -> Value {
    shape_simple("string", "Completion script text")
}

// ---------- examples ----------

fn examples_install() -> Value {
    Value::Array(vec![
        Value::str("zamak install --mbr mbr.bin --stage2 stage2.bin --target disk.img"),
        Value::str(
            "zamak install --mbr mbr.bin --stage2 stage2.bin --target disk.img --json --dry-run",
        ),
    ])
}

fn examples_enroll() -> Value {
    Value::Array(vec![
        Value::str("zamak enroll-config --config zamak.conf --efi BOOTX64.EFI"),
        Value::str("zamak enroll-config --config zamak.conf --efi BOOTX64.EFI --json"),
    ])
}

fn examples_sbom() -> Value {
    Value::Array(vec![
        Value::str("zamak sbom --version 0.7.0 dist/BOOTX64.EFI"),
        Value::str("zamak sbom --version 0.7.0 --json | jq .data.spdxVersion"),
    ])
}

fn examples_schema() -> Value {
    Value::Array(vec![
        Value::str("zamak schema"),
        Value::str("zamak schema install"),
        Value::str("zamak schema --json | jq '.commands[].name'"),
    ])
}

fn examples_describe() -> Value {
    Value::Array(vec![
        Value::str("zamak describe --json"),
        Value::str("zamak describe --json | jq '.commands[] | select(.destructive)'"),
    ])
}

fn examples_completions() -> Value {
    Value::Array(vec![
        Value::str("zamak completions bash > /etc/bash_completion.d/zamak"),
        Value::str("zamak completions nushell | save -f ~/.config/nushell/zamak.nu"),
    ])
}

// ---------- helpers ----------

fn param(
    name: &'static str,
    ty: &'static str,
    required: bool,
    desc: &'static str,
) -> (String, Value) {
    (
        name.to_string(),
        obj([
            ("type", Value::str(ty)),
            ("required", Value::Bool(required)),
            ("description", Value::str(desc)),
        ]),
    )
}

fn param_with_default(
    name: &'static str,
    ty: &'static str,
    required: bool,
    desc: &'static str,
    default: Value,
) -> (String, Value) {
    (
        name.to_string(),
        obj([
            ("type", Value::str(ty)),
            ("required", Value::Bool(required)),
            ("description", Value::str(desc)),
            ("default", default),
        ]),
    )
}

fn param_with_enum(
    name: &'static str,
    ty: &'static str,
    required: bool,
    desc: &'static str,
    values: &[&'static str],
) -> (String, Value) {
    let enum_values: Vec<Value> = values.iter().map(|s| Value::str(*s)).collect();
    (
        name.to_string(),
        obj([
            ("type", Value::str(ty)),
            ("required", Value::Bool(required)),
            ("description", Value::str(desc)),
            ("enum", Value::Array(enum_values)),
        ]),
    )
}

fn schema_object(props: &[(String, Value)]) -> Value {
    Value::Object(
        [
            ("type".to_string(), Value::str("object")),
            ("properties".to_string(), Value::Object(props.to_vec())),
        ]
        .to_vec(),
    )
}

fn shape_object(
    ty: &'static str,
    desc: &'static str,
    fields: &[(&'static str, &'static str)],
) -> Value {
    let props: Vec<(String, Value)> = fields
        .iter()
        .map(|(n, t)| ((*n).to_string(), obj([("type", Value::str(*t))])))
        .collect();
    obj([
        ("type", Value::str(ty)),
        ("description", Value::str(desc)),
        ("properties", Value::Object(props)),
    ])
}

fn shape_simple(ty: &'static str, desc: &'static str) -> Value {
    obj([("type", Value::str(ty)), ("description", Value::str(desc))])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_has_a_spec() {
        for name in [
            "install",
            "enroll-config",
            "sbom",
            "schema",
            "describe",
            "completions",
        ] {
            assert!(find(name).is_some(), "no spec for {name}");
        }
    }

    #[test]
    fn full_schema_references_2020_12() {
        let s = full_schema().to_compact();
        assert!(s.contains(r#""$schema":"https://json-schema.org/draft/2020-12/schema""#));
    }

    #[test]
    fn command_schema_includes_required_fields() {
        let spec = find("install").unwrap();
        let s = command_schema(spec).to_compact();
        assert!(s.contains(r#""destructive":true"#));
        assert!(s.contains(r#""supports_dry_run":true"#));
        assert!(s.contains(r#""examples""#));
        assert!(s.contains(r#""exit_codes""#));
    }

    #[test]
    fn exit_codes_map_numbers_to_upper_snake() {
        let spec = find("install").unwrap();
        let s = command_schema(spec).to_compact();
        assert!(s.contains(r#""0":"SUCCESS""#));
        assert!(s.contains(r#""3":"NOT_FOUND""#));
    }
}
