mod jscpd;
mod jsonc;
mod knip;
mod knip_fields;
mod knip_tables;
#[cfg(test)]
mod tests;
mod toml_gen;

use std::io::Read as _;
use std::path::Path;
use std::process::ExitCode;

use jscpd::migrate_jscpd;
use jsonc::generate_jsonc;
use knip::migrate_knip;
use toml_gen::generate_toml;

/// A warning about a config field that could not be migrated.
#[derive(Debug)]
struct MigrationWarning {
    pub(super) source: &'static str,
    pub(super) field: String,
    pub(super) message: String,
    pub(super) suggestion: Option<String>,
}

impl std::fmt::Display for MigrationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] `{}`: {}", self.source, self.field, self.message)?;
        if let Some(ref suggestion) = self.suggestion {
            write!(f, " (suggestion: {suggestion})")?;
        }
        Ok(())
    }
}

/// Result of migrating one or more source configs.
#[derive(Debug)]
struct MigrationResult {
    pub(super) config: serde_json::Value,
    pub(super) warnings: Vec<MigrationWarning>,
    pub(super) sources: Vec<String>,
}

/// Run the migrate command.
pub fn run_migrate(root: &Path, use_toml: bool, dry_run: bool, from: Option<&Path>) -> ExitCode {
    // Check if a fallow config already exists
    let existing_names = [
        ".fallowrc.json",
        ".fallowrc.jsonc",
        "fallow.toml",
        ".fallow.toml",
    ];
    if !dry_run {
        for name in &existing_names {
            let path = root.join(name);
            if path.exists() {
                eprintln!(
                    "Error: {name} already exists. Remove it first or use --dry-run to preview."
                );
                return ExitCode::from(2);
            }
        }
    }

    let result = from.map_or_else(|| migrate_auto_detect(root), migrate_from_file);

    let result = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(2);
        }
    };

    if result.sources.is_empty() {
        eprintln!("No knip or jscpd configuration found to migrate.");
        return ExitCode::from(2);
    }

    // Generate output
    let output_content = if use_toml {
        generate_toml(&result)
    } else {
        generate_jsonc(&result)
    };

    if dry_run {
        println!("{output_content}");
    } else {
        let filename = if use_toml {
            "fallow.toml"
        } else {
            ".fallowrc.json"
        };
        let output_path = root.join(filename);
        if let Err(e) = std::fs::write(&output_path, &output_content) {
            eprintln!("Error: failed to write {filename}: {e}");
            return ExitCode::from(2);
        }
        eprintln!("Created {filename}");
    }

    // Print source info
    for source in &result.sources {
        eprintln!("Migrated from: {source}");
    }

    // Print warnings
    if !result.warnings.is_empty() {
        eprintln!();
        eprintln!("Warnings ({} skipped fields):", result.warnings.len());
        for warning in &result.warnings {
            eprintln!("  {warning}");
        }
    }

    ExitCode::SUCCESS
}

/// Auto-detect and migrate from knip and/or jscpd configs in the given root.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "JS/TS extensions are always lowercase"
)]
fn migrate_auto_detect(root: &Path) -> Result<MigrationResult, String> {
    let mut config = serde_json::Map::new();
    let mut warnings = Vec::new();
    let mut sources = Vec::new();

    // Try knip configs
    let knip_files = [
        "knip.json",
        "knip.jsonc",
        ".knip.json",
        ".knip.jsonc",
        "knip.ts",
        "knip.config.ts",
    ];

    for name in &knip_files {
        let path = root.join(name);
        if path.exists() {
            if name.ends_with(".ts") {
                warnings.push(MigrationWarning {
                    source: "knip",
                    field: name.to_string(),
                    message: format!(
                        "TypeScript config files ({name}) cannot be parsed. \
                         Convert to knip.json first, then re-run migrate."
                    ),
                    suggestion: None,
                });
                continue;
            }
            let knip_value = load_json_or_jsonc(&path)?;
            migrate_knip(&knip_value, &mut config, &mut warnings);
            sources.push(name.to_string());
            break; // Only use the first knip config found
        }
    }

    // Try jscpd standalone config
    let mut found_jscpd_file = false;
    let jscpd_path = root.join(".jscpd.json");
    if jscpd_path.exists() {
        let jscpd_value = load_json_or_jsonc(&jscpd_path)?;
        migrate_jscpd(&jscpd_value, &mut config, &mut warnings);
        sources.push(".jscpd.json".to_string());
        found_jscpd_file = true;
    }

    // Check package.json for embedded knip/jscpd config (single read)
    let need_pkg_knip = sources.is_empty();
    let need_pkg_jscpd = !found_jscpd_file;
    if need_pkg_knip || need_pkg_jscpd {
        let pkg_path = root.join("package.json");
        if pkg_path.exists() {
            let pkg_content = std::fs::read_to_string(&pkg_path)
                .map_err(|e| format!("failed to read package.json: {e}"))?;
            let pkg_value: serde_json::Value = serde_json::from_str(&pkg_content)
                .map_err(|e| format!("failed to parse package.json: {e}"))?;
            if need_pkg_knip && let Some(knip_config) = pkg_value.get("knip") {
                migrate_knip(knip_config, &mut config, &mut warnings);
                sources.push("package.json (knip key)".to_string());
            }
            if need_pkg_jscpd && let Some(jscpd_config) = pkg_value.get("jscpd") {
                migrate_jscpd(jscpd_config, &mut config, &mut warnings);
                sources.push("package.json (jscpd key)".to_string());
            }
        }
    }

    Ok(MigrationResult {
        config: serde_json::Value::Object(config),
        warnings,
        sources,
    })
}

/// Migrate from a specific config file.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "JS/TS extensions are always lowercase"
)]
fn migrate_from_file(path: &Path) -> Result<MigrationResult, String> {
    if !path.exists() {
        return Err(format!("config file not found: {}", path.display()));
    }

    let mut config = serde_json::Map::new();
    let mut warnings = Vec::new();
    let mut sources = Vec::new();

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();

    if filename.contains("knip") {
        if filename.ends_with(".ts") {
            return Err(format!(
                "TypeScript config files ({filename}) cannot be parsed. \
                 Convert to knip.json first, then re-run migrate."
            ));
        }
        let knip_value = load_json_or_jsonc(path)?;
        migrate_knip(&knip_value, &mut config, &mut warnings);
        sources.push(path.display().to_string());
    } else if filename.contains("jscpd") {
        let jscpd_value = load_json_or_jsonc(path)?;
        migrate_jscpd(&jscpd_value, &mut config, &mut warnings);
        sources.push(path.display().to_string());
    } else if filename == "package.json" {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let pkg_value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
        if let Some(knip_config) = pkg_value.get("knip") {
            migrate_knip(knip_config, &mut config, &mut warnings);
            sources.push(format!("{} (knip key)", path.display()));
        }
        if let Some(jscpd_config) = pkg_value.get("jscpd") {
            migrate_jscpd(jscpd_config, &mut config, &mut warnings);
            sources.push(format!("{} (jscpd key)", path.display()));
        }
        if sources.is_empty() {
            return Err(format!(
                "no knip or jscpd configuration found in {}",
                path.display()
            ));
        }
    } else {
        // Try to detect format from content
        let value = load_json_or_jsonc(path)?;
        // If it has knip-like fields, treat as knip
        if value.get("entry").is_some()
            || value.get("ignore").is_some()
            || value.get("rules").is_some()
            || value.get("project").is_some()
            || value.get("ignoreDependencies").is_some()
            || value.get("ignoreExportsUsedInFile").is_some()
        {
            migrate_knip(&value, &mut config, &mut warnings);
            sources.push(path.display().to_string());
        }
        // If it has jscpd-like fields, treat as jscpd
        else if value.get("minTokens").is_some()
            || value.get("minLines").is_some()
            || value.get("threshold").is_some()
            || value.get("mode").is_some()
        {
            migrate_jscpd(&value, &mut config, &mut warnings);
            sources.push(path.display().to_string());
        } else {
            return Err(format!(
                "could not determine config format for {}",
                path.display()
            ));
        }
    }

    Ok(MigrationResult {
        config: serde_json::Value::Object(config),
        warnings,
        sources,
    })
}

/// Load a JSON or JSONC file, stripping comments and trailing commas if present.
fn load_json_or_jsonc(path: &Path) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    // Try plain JSON first
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
        return Ok(value);
    }

    // Try stripping comments (JSONC)
    let mut stripped = String::new();
    json_comments::StripComments::new(content.as_bytes())
        .read_to_string(&mut stripped)
        .map_err(|e| format!("failed to strip comments from {}: {e}", path.display()))?;

    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&stripped) {
        return Ok(value);
    }

    // Real-world JSONC (e.g. knip.jsonc, tsconfig.json) frequently uses
    // trailing commas. serde_json rejects them, so strip them as a final
    // pass before reporting a parse error to the user.
    let cleaned = strip_trailing_commas(&stripped);
    serde_json::from_str(&cleaned).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

/// Strip JSONC-style trailing commas (`,` immediately before `}` or `]`)
/// without touching commas inside string literals.
fn strip_trailing_commas(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut last_emit = 0;
    let mut in_string = false;
    let mut escaped = false;

    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if b == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if b == b',' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j < bytes.len()
                && (bytes[j] == b'}' || bytes[j] == b']')
                && comma_follows_json_value(bytes, i)
            {
                out.push_str(&input[last_emit..i]);
                last_emit = i + 1;
            }
        }
        i += 1;
    }

    out.push_str(&input[last_emit..]);
    out
}

fn comma_follows_json_value(bytes: &[u8], comma_index: usize) -> bool {
    let Some(prev) = bytes[..comma_index]
        .iter()
        .rev()
        .copied()
        .find(|b| !b.is_ascii_whitespace())
    else {
        return false;
    };

    matches!(prev, b'"' | b'}' | b']' | b'0'..=b'9' | b'e' | b'l')
}

/// Extract a string-or-array field as a `Vec<String>`.
fn string_or_array(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => vec![],
    }
}
