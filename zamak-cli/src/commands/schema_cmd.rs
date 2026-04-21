// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! `zamak schema [<command>]` — emit JSON Schema Draft 2020-12 for
//! the whole tool or a specific sub-command (SFRS §6.1).

use crate::error::CliError;
use crate::json::Value;

pub fn run(args: &[String]) -> Result<Value, CliError> {
    let mut target: Option<&str> = None;
    for a in args {
        if a.starts_with("--") {
            return Err(CliError::usage(format!("schema: unknown option '{a}'")));
        }
        if target.is_some() {
            return Err(CliError::usage(
                "schema: accepts at most one positional argument (the command name)",
            ));
        }
        target = Some(a);
    }

    match target {
        None => Ok(crate::schema::full_schema()),
        Some(name) => {
            let spec = crate::schema::find(name).ok_or_else(|| {
                CliError::not_found(format!("schema: no such command '{name}'"))
                    .with_hint("Run 'zamak describe --json' to list commands.")
            })?;
            Ok(crate::schema::command_schema(spec))
        }
    }
}
