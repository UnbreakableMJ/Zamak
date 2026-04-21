// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Mohamed Hammad

//! `zamak sbom` — emit SPDX 2.3 JSON SBOM (FR-CLI-003).
//!
//! The SPDX document itself is structured data — returning it as the
//! envelope's `data` field means:
//!
//! - `zamak sbom --json | jq '.data.spdxVersion'` works
//! - `zamak sbom --fields spdxVersion,name` projects fields
//! - `zamak sbom --output file.spdx.json` writes the bare SPDX doc
//!   (no envelope) to preserve round-trip with spdx-tools.

use std::fs;

use crate::error::CliError;
use crate::hash::{hex32, sha256};
use crate::json::{obj, Value};
use crate::output::OutputPolicy;
use crate::validate::reject_control_chars;

pub fn run(
    args: &[String],
    policy: &OutputPolicy,
    _globals: &crate::args::GlobalFlags,
) -> Result<Value, CliError> {
    let mut version: Option<&str> = None;
    let mut output: Option<&str> = None;
    let mut artifacts: Vec<&str> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            // The SFRS-8 global `--version` would shadow this one if we
            // reused the name; accept `--release-version` (canonical) and
            // keep `--version` as a legacy alias only inside the sbom
            // argument window (after the sub-command name).
            "--release-version" | "--version" => {
                i += 1;
                version = args.get(i).map(|s| s.as_str());
            }
            "--output" | "-o" => {
                i += 1;
                output = args.get(i).map(|s| s.as_str());
            }
            other if other.starts_with("--") => {
                return Err(CliError::usage(format!(
                    "sbom: unknown option '{other}'"
                )))
            }
            _ => artifacts.push(&args[i]),
        }
        i += 1;
    }

    let version = version.unwrap_or("0.0.0-dev");
    reject_control_chars("sbom --release-version", version)?;
    if let Some(o) = output {
        reject_control_chars("sbom --output", o)?;
    }
    for a in &artifacts {
        reject_control_chars("sbom artifact", a)?;
    }

    let timestamp = crate::time::iso8601_now();
    let (document, files) = build_spdx_document(version, &timestamp, &artifacts, policy);

    if let Some(path) = output {
        fs::write(path, &document)
            .map_err(|e| CliError::from_io(&format!("sbom: write '{path}'"), e))?;
        crate::output::emit_info(policy, &format!("sbom: wrote SPDX document to {path}"));
    }

    Ok(obj([
        ("spdxVersion", Value::str("SPDX-2.3")),
        ("name", Value::str(format!("zamak-{version}"))),
        ("version", Value::str(version)),
        ("created", Value::str(&timestamp)),
        ("files", Value::Array(files)),
        (
            "document_bytes",
            Value::UInt(document.len() as u64),
        ),
        ("output", match output {
            Some(p) => Value::str(p),
            None => Value::Null,
        }),
    ]))
}

fn build_spdx_document(
    version: &str,
    created: &str,
    artifacts: &[&str],
    policy: &OutputPolicy,
) -> (String, Vec<Value>) {
    let mut files_meta: Vec<Value> = Vec::new();
    let mut files_block = String::new();
    let mut relationships = String::from(
        r#"    {
      "spdxElementId": "SPDXRef-DOCUMENT",
      "relationshipType": "DESCRIBES",
      "relatedSpdxElement": "SPDXRef-Package-zamak"
    }"#,
    );

    for (idx, path) in artifacts.iter().enumerate() {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                crate::output::emit_warn(
                    policy,
                    &format!("sbom: skipping '{path}': {e}"),
                );
                continue;
            }
        };
        let sha = hex32(&sha256(&bytes));
        if !files_block.is_empty() {
            files_block.push_str(",\n");
        }
        let file_id = format!("SPDXRef-File-{idx}");
        let file_name = path.rsplit('/').next().unwrap_or(path);
        files_block.push_str(&format!(
            r#"    {{
      "SPDXID": "{file_id}",
      "fileName": "{file_name}",
      "checksums": [{{ "algorithm": "SHA256", "checksumValue": "{sha}" }}],
      "licenseConcluded": "GPL-3.0-or-later",
      "copyrightText": "Copyright 2026 Mohamed Hammad"
    }}"#
        ));
        relationships.push_str(&format!(
            r#",
    {{
      "spdxElementId": "SPDXRef-Package-zamak",
      "relationshipType": "CONTAINS",
      "relatedSpdxElement": "{file_id}"
    }}"#
        ));
        files_meta.push(obj([
            ("path", Value::str(*path)),
            ("sha256", Value::str(&sha)),
            ("bytes", Value::UInt(bytes.len() as u64)),
        ]));
    }

    let files_section = if files_block.is_empty() {
        String::new()
    } else {
        format!(",\n  \"files\": [\n{files_block}\n  ]")
    };

    let package = format!(
        r#"    {{
      "SPDXID": "SPDXRef-Package-zamak",
      "name": "zamak",
      "versionInfo": "{version}",
      "downloadLocation": "NOASSERTION",
      "filesAnalyzed": false,
      "licenseConcluded": "GPL-3.0-or-later",
      "licenseDeclared": "GPL-3.0-or-later",
      "copyrightText": "Copyright 2026 Mohamed Hammad",
      "supplier": "Person: Mohamed Hammad"
    }}"#
    );

    let tool_version = crate::meta::TOOL_VERSION;
    let doc = format!(
        r#"{{
  "spdxVersion": "SPDX-2.3",
  "dataLicense": "CC0-1.0",
  "SPDXID": "SPDXRef-DOCUMENT",
  "name": "zamak-{version}",
  "documentNamespace": "https://steelbore.org/spdxdocs/zamak-{version}",
  "creationInfo": {{
    "created": "{created}",
    "creators": [
      "Tool: zamak-cli-{tool_version}",
      "Person: Mohamed Hammad"
    ],
    "licenseListVersion": "3.23"
  }},
  "packages": [
{package}
  ]{files_section},
  "relationships": [
{relationships}
  ]
}}
"#
    );
    (doc, files_meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spdx_document_has_required_shape() {
        let fake_policy = OutputPolicy {
            format: crate::output::Format::Json,
            color: false,
            quiet: true,
            verbose: false,
            fields: None,
            print0: false,
        };
        let (doc, _) = build_spdx_document("1.0.0", "2026-04-19T12:00:00Z", &[], &fake_policy);
        assert!(doc.contains(r#""spdxVersion": "SPDX-2.3""#));
        assert!(doc.contains(r#""SPDXID": "SPDXRef-DOCUMENT""#));
        assert!(doc.contains(r#""versionInfo": "1.0.0""#));
        assert!(doc.contains(r#""created": "2026-04-19T12:00:00Z""#));
        assert!(doc.contains("GPL-3.0-or-later"));
    }
}
