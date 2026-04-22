// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! Sub-command implementations. Each one returns its `data` payload
//! as a JSON `Value` so `OutputPolicy::emit` can wrap it in the SFRS
//! envelope, plus any side effects (writes, prints) that its
//! contract requires.

pub mod completions;
pub mod describe;
pub mod enroll_config;
pub mod install;
pub mod sbom;
pub mod schema_cmd;
