// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! `zamak enroll-config` — BLAKE2B-256 hash the config file and patch
//! it into an EFI binary's `ZAMAK_CFG_HASH` slot (FR-CLI-002).

use std::fs;

use crate::env::EnvSnapshot;
use crate::error::CliError;
use crate::hash::{blake2b_256, hex32};
use crate::json::{obj, Value};
use crate::output::OutputPolicy;
use crate::validate::{confirm_destructive, reject_control_chars};

/// Signature marking the enrolled-hash slot. Must match
/// `zamak_core::enrolled_hash::ENROLLED_HASH_SIGNATURE`.
pub const ENROLLED_HASH_SIGNATURE: [u8; 16] = [
    b'Z', b'A', b'M', b'A', b'K', b'_', b'C', b'F', b'G', b'_', b'H', b'A', b'S', b'H', 0xA5,
    0x5A,
];

pub fn find_hash_slot(binary: &[u8]) -> Option<usize> {
    binary.windows(16).position(|w| w == ENROLLED_HASH_SIGNATURE)
}

pub fn run(
    args: &[String],
    policy: &OutputPolicy,
    globals: &crate::args::GlobalFlags,
    env: &EnvSnapshot,
) -> Result<Value, CliError> {
    let mut config_path: Option<&str> = None;
    let mut efi_path: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                i += 1;
                config_path = args.get(i).map(|s| s.as_str());
            }
            "--efi" => {
                i += 1;
                efi_path = args.get(i).map(|s| s.as_str());
            }
            other => {
                return Err(CliError::usage(format!(
                    "enroll-config: unknown option '{other}'"
                )))
            }
        }
        i += 1;
    }

    let config_path =
        config_path.ok_or_else(|| CliError::usage("enroll-config: --config is required"))?;
    let efi_path =
        efi_path.ok_or_else(|| CliError::usage("enroll-config: --efi is required"))?;

    reject_control_chars("enroll-config --config", config_path)?;
    reject_control_chars("enroll-config --efi", efi_path)?;

    let config_data = fs::read(config_path).map_err(|e| {
        CliError::from_io(&format!("enroll-config: read '{config_path}'"), e)
    })?;
    let hash = blake2b_256(&config_data);
    let hex = hex32(&hash);

    let mut efi_data = fs::read(efi_path)
        .map_err(|e| CliError::from_io(&format!("enroll-config: read '{efi_path}'"), e))?;
    let offset = find_hash_slot(&efi_data).ok_or_else(|| {
        CliError::not_found(format!(
            "enroll-config: no ZAMAK hash slot found in '{efi_path}'"
        ))
        .with_hint(
            "Rebuild the EFI binary with zamak-uefi 0.7.0+ — it embeds the \
             ZAMAK_CFG_HASH marker that enroll-config patches.",
        )
    })?;
    let hash_start = offset + ENROLLED_HASH_SIGNATURE.len();

    // Idempotency: if the slot already holds this exact hash, skip.
    let already_enrolled = efi_data[hash_start..hash_start + 32] == hash;

    let data = obj([
        ("config", Value::str(config_path)),
        ("efi", Value::str(efi_path)),
        ("blake2b_256", Value::str(&hex)),
        ("patch_offset", Value::UInt(hash_start as u64)),
        ("dry_run", Value::Bool(globals.dry_run)),
        ("already_enrolled", Value::Bool(already_enrolled)),
    ]);

    if globals.dry_run {
        crate::output::emit_info(
            policy,
            &format!(
                "enroll-config: DRY RUN — would patch BLAKE2B-256={hex} \
                 into '{efi_path}' at offset {hash_start:#x}"
            ),
        );
        return Ok(data);
    }

    if already_enrolled {
        crate::output::emit_info(
            policy,
            &format!("enroll-config: '{efi_path}' already has hash {hex} (idempotent no-op)"),
        );
        return Ok(data);
    }

    confirm_destructive(
        "enroll-config",
        env.stdin_is_tty,
        globals.yes,
        globals.force,
    )?;

    efi_data[hash_start..hash_start + 32].copy_from_slice(&hash);
    fs::write(efi_path, &efi_data)
        .map_err(|e| CliError::from_io(&format!("enroll-config: write '{efi_path}'"), e))?;

    crate::output::emit_info(
        policy,
        &format!(
            "enroll-config: patched '{efi_path}' at offset {hash_start:#x} with BLAKE2B-256={hex}"
        ),
    );
    Ok(data)
}
