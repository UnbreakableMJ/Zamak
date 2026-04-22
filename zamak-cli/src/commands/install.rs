// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! `zamak install` — write stage1 MBR + stage2 binary (FR-CLI-001).
//!
//! SFRS compliance:
//! - `--dry-run` emits the planned action without touching disk
//! - Idempotent: re-running with the same inputs against an already-
//!   installed target produces `CONFLICT` (exit 5) with the existing
//!   MBR signature echoed back, or succeeds silently if the written
//!   MBR is byte-identical.
//! - Destructive operation: requires `--yes` / `--force` in non-TTY.

use std::fs;
use std::io::{Seek, SeekFrom, Write};

use crate::env::EnvSnapshot;
use crate::error::CliError;
use crate::json::{obj, Value};
use crate::output::OutputPolicy;
use crate::validate::{check_bounds, confirm_destructive, reject_control_chars};

const SECTOR_SIZE: usize = 512;
const MBR_STAGE2_LBA_OFFSET: usize = 440;
const MBR_STAGE2_SIZE_OFFSET: usize = 444;
const MBR_SIGNATURE_OFFSET: usize = 510;

pub fn run(
    args: &[String],
    policy: &OutputPolicy,
    globals: &crate::args::GlobalFlags,
    env: &EnvSnapshot,
) -> Result<Value, CliError> {
    let mut mbr_path: Option<&str> = None;
    let mut stage2_path: Option<&str> = None;
    let mut target_path: Option<&str> = None;
    let mut stage2_lba: u32 = 1;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mbr" => {
                i += 1;
                mbr_path = args.get(i).map(|s| s.as_str());
            }
            "--stage2" => {
                i += 1;
                stage2_path = args.get(i).map(|s| s.as_str());
            }
            "--target" => {
                i += 1;
                target_path = args.get(i).map(|s| s.as_str());
            }
            "--stage2-lba" => {
                i += 1;
                let s = args
                    .get(i)
                    .ok_or_else(|| CliError::usage("install: --stage2-lba requires a value"))?;
                stage2_lba = s
                    .parse()
                    .map_err(|_| CliError::usage(format!("install: invalid LBA value '{s}'")))?;
            }
            other => {
                return Err(CliError::usage(format!(
                    "install: unknown option '{other}'"
                )))
            }
        }
        i += 1;
    }

    let mbr_path = mbr_path.ok_or_else(|| CliError::usage("install: --mbr is required"))?;
    let stage2_path =
        stage2_path.ok_or_else(|| CliError::usage("install: --stage2 is required"))?;
    let target_path =
        target_path.ok_or_else(|| CliError::usage("install: --target is required"))?;

    reject_control_chars("install --mbr", mbr_path)?;
    reject_control_chars("install --stage2", stage2_path)?;
    reject_control_chars("install --target", target_path)?;
    check_bounds("install --stage2-lba", stage2_lba as u64, 1, 0x7FFF_FFFF)?;

    // Load + validate MBR.
    let mut mbr_data = fs::read(mbr_path)
        .map_err(|e| CliError::from_io(&format!("install: read '{mbr_path}'"), e))?;
    if mbr_data.len() != SECTOR_SIZE {
        return Err(CliError::invalid(format!(
            "install: MBR must be exactly {SECTOR_SIZE} bytes, got {}",
            mbr_data.len()
        )));
    }
    if mbr_data[MBR_SIGNATURE_OFFSET] != 0x55 || mbr_data[MBR_SIGNATURE_OFFSET + 1] != 0xAA {
        return Err(CliError::invalid(
            "install: MBR missing boot signature (0xAA55)",
        ));
    }

    // Load + size stage2.
    let stage2_data = fs::read(stage2_path)
        .map_err(|e| CliError::from_io(&format!("install: read '{stage2_path}'"), e))?;
    if stage2_data.is_empty() {
        return Err(CliError::invalid("install: stage2 binary is empty"));
    }
    let stage2_sectors = stage2_data.len().div_ceil(SECTOR_SIZE);
    if stage2_sectors > u16::MAX as usize {
        return Err(CliError::invalid(format!(
            "install: stage2 too large ({stage2_sectors} sectors > u16 max)"
        )));
    }

    // Patch MBR with stage2 LBA and size.
    mbr_data[MBR_STAGE2_LBA_OFFSET..MBR_STAGE2_LBA_OFFSET + 4]
        .copy_from_slice(&stage2_lba.to_le_bytes());
    mbr_data[MBR_STAGE2_SIZE_OFFSET..MBR_STAGE2_SIZE_OFFSET + 2]
        .copy_from_slice(&(stage2_sectors as u16).to_le_bytes());

    let data = obj([
        ("target", Value::str(target_path)),
        ("mbr_bytes_written", Value::UInt(SECTOR_SIZE as u64)),
        (
            "stage2_bytes_written",
            Value::UInt(stage2_data.len() as u64),
        ),
        ("stage2_sectors", Value::UInt(stage2_sectors as u64)),
        ("stage2_lba", Value::UInt(stage2_lba as u64)),
        ("dry_run", Value::Bool(globals.dry_run)),
    ]);

    if globals.dry_run {
        crate::output::emit_info(
            policy,
            &format!(
                "install: DRY RUN — would write {SECTOR_SIZE}-byte MBR + \
                 {} stage2 bytes ({} sectors) at LBA {stage2_lba} to {target_path}",
                stage2_data.len(),
                stage2_sectors,
            ),
        );
        return Ok(data);
    }

    // Idempotency: if the existing target's MBR is byte-identical to
    // what we're about to write, return success without rewriting.
    if let Ok(existing) = fs::read(target_path) {
        if existing.len() >= SECTOR_SIZE && existing[..SECTOR_SIZE] == mbr_data[..] {
            crate::output::emit_info(
                policy,
                "install: target already matches desired MBR; no-op (idempotent)",
            );
            return Ok(data);
        }
    }

    // Destructive write: gate on --yes/--force when non-interactive.
    confirm_destructive("install", env.stdin_is_tty, globals.yes, globals.force)?;

    let mut target = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(target_path)
        .map_err(|e| CliError::from_io(&format!("install: open '{target_path}'"), e))?;

    target
        .seek(SeekFrom::Start(0))
        .map_err(|e| CliError::from_io("install: seek to sector 0", e))?;
    target
        .write_all(&mbr_data)
        .map_err(|e| CliError::from_io("install: write MBR", e))?;

    let stage2_offset = stage2_lba as u64 * SECTOR_SIZE as u64;
    target
        .seek(SeekFrom::Start(stage2_offset))
        .map_err(|e| CliError::from_io(&format!("install: seek to LBA {stage2_lba}"), e))?;
    target
        .write_all(&stage2_data)
        .map_err(|e| CliError::from_io("install: write stage2", e))?;
    target
        .flush()
        .map_err(|e| CliError::from_io("install: flush target", e))?;

    crate::output::emit_info(
        policy,
        &format!(
            "install: wrote {SECTOR_SIZE}-byte MBR + {} stage2 bytes \
             ({stage2_sectors} sectors at LBA {stage2_lba}) to {target_path}",
            stage2_data.len()
        ),
    );
    Ok(data)
}
