// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! MBR boot sector (Stage 1) — **moved to `zamak-stage1` crate**.
//!
//! The MBR assembly now lives in `zamak-stage1/src/mbr.rs` as a
//! standalone binary crate per PRD §4.1. This module is retained
//! for backward compatibility and will be removed in a future cleanup.

// Rust guideline compliant 2026-03-30
