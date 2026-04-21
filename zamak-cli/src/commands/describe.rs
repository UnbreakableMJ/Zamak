// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! `zamak describe` — capability manifest (SFRS §6.2).

use crate::error::CliError;
use crate::json::{obj, Value};

pub fn run(args: &[String]) -> Result<Value, CliError> {
    for a in args {
        return Err(CliError::usage(format!(
            "describe: unknown argument '{a}'"
        )));
    }

    let commands: Vec<Value> = crate::schema::COMMANDS
        .iter()
        .map(|c| {
            obj([
                ("name", Value::str(c.name)),
                ("description", Value::str(c.description)),
                ("destructive", Value::Bool(c.destructive)),
                ("idempotent", Value::Bool(c.idempotent)),
                ("supports_dry_run", Value::Bool(c.supports_dry_run)),
                ("supports_json", Value::Bool(c.supports_json)),
                ("supports_fields", Value::Bool(c.supports_fields)),
            ])
        })
        .collect();

    let global_flags = Value::Array(
        [
            "--json",
            "--format",
            "--fields",
            "--dry-run",
            "--verbose",
            "--quiet",
            "--color",
            "--no-color",
            "--yes",
            "--force",
            "--print0",
            "--help",
            "--version",
        ]
        .iter()
        .map(|s| Value::str(*s))
        .collect(),
    );

    let formats = Value::Array(
        ["human", "json", "jsonl", "yaml", "csv", "explore"]
            .iter()
            .map(|s| Value::str(*s))
            .collect(),
    );

    let tui_feature;
    #[cfg(feature = "tui")]
    {
        tui_feature = true;
    }
    #[cfg(not(feature = "tui"))]
    {
        tui_feature = false;
    }

    Ok(obj([
        ("tool", Value::str(crate::meta::TOOL_NAME)),
        ("version", Value::str(crate::meta::TOOL_VERSION)),
        (
            "description",
            Value::str("ZAMAK bootloader host CLI — MBR install, config enrollment, SBOM"),
        ),
        ("commands", Value::Array(commands)),
        ("global_flags", global_flags),
        ("output_formats", formats),
        ("mcp_available", Value::Bool(false)),
        ("tui_feature", Value::Bool(tui_feature)),
        ("schema_url", Value::str(crate::schema::SCHEMA_URL)),
        ("docs_base", Value::str(crate::meta::DOCS_BASE)),
    ]))
}
