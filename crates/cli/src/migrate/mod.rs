mod jscpd;
mod jsonc;
mod knip;
mod knip_fields;
mod knip_tables;
mod stylelint;
#[cfg(test)]
mod tests;
mod toml_gen;

use std::path::Path;
use std::process::ExitCode;

use jscpd::migrate_jscpd;
use jsonc::generate_jsonc;
use knip::migrate_knip;
use stylelint::migrate_stylelint;
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

/// Output format selection for the generated fallow config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Jsonc,
    Toml,
}

impl OutputFormat {
    #[expect(
        clippy::case_sensitive_file_extension_comparisons,
        reason = "config file extensions are always lowercase"
    )]
    fn pick(use_toml: bool, use_jsonc: bool, result: &MigrationResult) -> Self {
        if use_toml {
            return Self::Toml;
        }
        if use_jsonc {
            return Self::Jsonc;
        }
        if result
            .sources
            .iter()
            .any(|s| source_head(s).ends_with(".jsonc"))
        {
            Self::Jsonc
        } else {
            Self::Json
        }
    }

    fn filename(self) -> &'static str {
        match self {
            Self::Toml => "fallow.toml",
            Self::Jsonc => ".fallowrc.jsonc",
            Self::Json => ".fallowrc.json",
        }
    }
}

/// Run the migrate command.
///
/// Output format and filename are picked in priority order: `--toml` writes
/// `fallow.toml`, `--jsonc` writes `.fallowrc.jsonc`, otherwise the source
/// extension is mirrored (`knip.jsonc` produces `.fallowrc.jsonc`,
/// `knip.json` / `package.json` keys produce `.fallowrc.json`). The
/// generated JSONC content includes `//` comments either way; the `.jsonc`
/// extension exists so editors auto-detect JSON-with-comments syntax
/// highlighting.
pub fn run_migrate(
    root: &Path,
    use_toml: bool,
    use_jsonc: bool,
    dry_run: bool,
    from: Option<&Path>,
) -> ExitCode {
    if !dry_run && let Some(code) = reject_existing_fallow_config(root) {
        return code;
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
        eprintln!("No knip, jscpd, or stylelint configuration found to migrate.");
        return ExitCode::from(2);
    }

    let format = OutputFormat::pick(use_toml, use_jsonc, &result);

    let output_content = match format {
        OutputFormat::Toml => generate_toml(&result),
        OutputFormat::Jsonc | OutputFormat::Json => generate_jsonc(&result),
    };

    if dry_run {
        println!("{output_content}");
    } else {
        let filename = format.filename();
        let output_path = root.join(filename);
        if let Err(e) = std::fs::write(&output_path, &output_content) {
            eprintln!("Error: failed to write {filename}: {e}");
            return ExitCode::from(2);
        }
        eprintln!("Created {filename}");
    }

    report_migration_outcome(&result);
    ExitCode::SUCCESS
}

/// Reject the migration when a fallow config already exists at `root`.
/// Returns `Some(exit_code)` to abort, `None` to proceed.
fn reject_existing_fallow_config(root: &Path) -> Option<ExitCode> {
    let existing_names = [
        ".fallowrc.json",
        ".fallowrc.jsonc",
        "fallow.toml",
        ".fallow.toml",
    ];
    for name in &existing_names {
        if root.join(name).exists() {
            eprintln!("Error: {name} already exists. Remove it first or use --dry-run to preview.");
            return Some(ExitCode::from(2));
        }
    }
    None
}

/// Print the migrated-sources list, skipped-field warnings, and the
/// knip glob-engine caveat after a successful migration.
fn report_migration_outcome(result: &MigrationResult) {
    for source in &result.sources {
        eprintln!("Migrated from: {}", source_head(source));
    }

    if !result.warnings.is_empty() {
        let count = result.warnings.len();
        let header = if count == 1 { "Warning" } else { "Warnings" };
        let noun = if count == 1 { "field" } else { "fields" };
        eprintln!();
        eprintln!("{header} ({count} skipped {noun}):");
        for warning in &result.warnings {
            eprintln!("  {warning}");
        }
    }

    if should_emit_glob_caveat(result) {
        eprintln!();
        eprintln!(
            "Note: knip and fallow use different glob engines; verify migrated entry / ignorePatterns with `fallow dead-code` before relying on CI. See https://docs.fallow.tools/migration/from-knip"
        );
    }
}

/// Auto-detect and migrate from knip, jscpd, and/or stylelint configs in the given root.
fn migrate_auto_detect(root: &Path) -> Result<MigrationResult, String> {
    let mut config = serde_json::Map::new();
    let mut warnings = Vec::new();
    let mut sources = Vec::new();

    migrate_first_knip_file(root, &mut config, &mut warnings, &mut sources)?;

    let mut found_jscpd_file = false;
    let jscpd_path = root.join(".jscpd.json");
    if jscpd_path.exists() {
        let jscpd_value = load_json_or_jsonc(&jscpd_path)?;
        migrate_jscpd(&jscpd_value, &mut config, &mut warnings);
        sources.push(".jscpd.json".to_string());
        found_jscpd_file = true;
    }

    migrate_first_stylelint_file(root, &mut config, &mut warnings, &mut sources)?;

    let package_needs = PackageJsonMigrationNeeds {
        knip: sources.is_empty(),
        jscpd: !found_jscpd_file,
        stylelint: !sources.iter().any(|source| source.contains("stylelint")),
    };
    if package_needs.any() {
        migrate_package_json_keys(
            root,
            package_needs,
            &mut config,
            &mut warnings,
            &mut sources,
        )?;
    }

    Ok(MigrationResult {
        config: serde_json::Value::Object(config),
        warnings,
        sources,
    })
}

/// Find and migrate the first standalone knip config file under `root`.
/// A `.ts` config is recorded as an unparseable warning and skipped.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "JS/TS extensions are always lowercase"
)]
fn migrate_first_knip_file(
    root: &Path,
    config: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
    sources: &mut Vec<String>,
) -> Result<(), String> {
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
            migrate_knip(&knip_value, config, warnings);
            sources.push(name.to_string());
            break; // Only use the first knip config found
        }
    }
    Ok(())
}

/// Find and migrate the first standalone Stylelint config under `root`.
fn migrate_first_stylelint_file(
    root: &Path,
    config: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
    sources: &mut Vec<String>,
) -> Result<(), String> {
    let stylelint_files = [
        ".stylelintrc",
        ".stylelintrc.json",
        ".stylelintrc.jsonc",
        "stylelint.config.js",
        "stylelint.config.cjs",
        "stylelint.config.mjs",
    ];

    for name in &stylelint_files {
        let path = root.join(name);
        if path.exists() {
            let stylelint_value = load_stylelint_config(&path)?;
            migrate_stylelint(&stylelint_value, config, warnings);
            sources.push(name.to_string());
            break;
        }
    }
    Ok(())
}

/// Migrate the `knip` / `jscpd` / `stylelint` keys embedded in `package.json` when no
/// standalone config supplied them.
#[derive(Clone, Copy)]
struct PackageJsonMigrationNeeds {
    knip: bool,
    jscpd: bool,
    stylelint: bool,
}

impl PackageJsonMigrationNeeds {
    const fn any(self) -> bool {
        self.knip || self.jscpd || self.stylelint
    }
}

fn migrate_package_json_keys(
    root: &Path,
    needs: PackageJsonMigrationNeeds,
    config: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
    sources: &mut Vec<String>,
) -> Result<(), String> {
    let pkg_path = root.join("package.json");
    if !pkg_path.exists() {
        return Ok(());
    }
    let pkg_content = std::fs::read_to_string(&pkg_path)
        .map_err(|e| format!("failed to read package.json: {e}"))?;
    let pkg_value: serde_json::Value = serde_json::from_str(&pkg_content)
        .map_err(|e| format!("failed to parse package.json: {e}"))?;
    if needs.knip
        && let Some(knip_config) = pkg_value.get("knip")
    {
        migrate_knip(knip_config, config, warnings);
        sources.push("package.json (knip key)".to_string());
    }
    if needs.jscpd
        && let Some(jscpd_config) = pkg_value.get("jscpd")
    {
        migrate_jscpd(jscpd_config, config, warnings);
        sources.push("package.json (jscpd key)".to_string());
    }
    if needs.stylelint
        && let Some(stylelint_config) = pkg_value.get("stylelint")
    {
        migrate_stylelint(stylelint_config, config, warnings);
        sources.push("package.json (stylelint key)".to_string());
    }
    Ok(())
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
    } else if filename.contains("stylelint") || filename.starts_with(".stylelintrc") {
        let stylelint_value = load_stylelint_config(path)?;
        migrate_stylelint(&stylelint_value, &mut config, &mut warnings);
        sources.push(path.display().to_string());
    } else if filename == "package.json" {
        migrate_package_json_file(path, &mut config, &mut warnings, &mut sources)?;
    } else {
        migrate_unnamed_config_file(path, &mut config, &mut warnings, &mut sources)?;
    }

    Ok(MigrationResult {
        config: serde_json::Value::Object(config),
        warnings,
        sources,
    })
}

/// Migrate the `knip` / `jscpd` / `stylelint` keys from an explicitly named `package.json`.
fn migrate_package_json_file(
    path: &Path,
    config: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
    sources: &mut Vec<String>,
) -> Result<(), String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let pkg_value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
    if let Some(knip_config) = pkg_value.get("knip") {
        migrate_knip(knip_config, config, warnings);
        sources.push(format!("{} (knip key)", path.display()));
    }
    if let Some(jscpd_config) = pkg_value.get("jscpd") {
        migrate_jscpd(jscpd_config, config, warnings);
        sources.push(format!("{} (jscpd key)", path.display()));
    }
    if let Some(stylelint_config) = pkg_value.get("stylelint") {
        migrate_stylelint(stylelint_config, config, warnings);
        sources.push(format!("{} (stylelint key)", path.display()));
    }
    if sources.is_empty() {
        return Err(format!(
            "no knip, jscpd, or stylelint configuration found in {}",
            path.display()
        ));
    }
    Ok(())
}

/// Migrate a config file whose name does not reveal its tool by sniffing
/// distinctive knip / jscpd / stylelint keys in its contents.
fn migrate_unnamed_config_file(
    path: &Path,
    config: &mut serde_json::Map<String, serde_json::Value>,
    warnings: &mut Vec<MigrationWarning>,
    sources: &mut Vec<String>,
) -> Result<(), String> {
    let value = load_json_or_jsonc(path)?;
    if value.get("entry").is_some()
        || value.get("ignore").is_some()
        || value.get("rules").is_some()
        || value.get("project").is_some()
        || value.get("ignoreDependencies").is_some()
        || value.get("ignoreExportsUsedInFile").is_some()
    {
        migrate_knip(&value, config, warnings);
        sources.push(format!("{} (knip config)", path.display()));
    } else if value.get("minTokens").is_some()
        || value.get("minLines").is_some()
        || value.get("threshold").is_some()
        || value.get("mode").is_some()
    {
        migrate_jscpd(&value, config, warnings);
        sources.push(format!("{} (jscpd config)", path.display()));
    } else if value.get("rules").is_some() && value.get("extends").is_some() {
        migrate_stylelint(&value, config, warnings);
        sources.push(format!("{} (stylelint config)", path.display()));
    } else {
        return Err(format!(
            "could not determine config format for {}",
            path.display()
        ));
    }
    Ok(())
}

/// Load a JSON or JSONC file, accepting comments and trailing commas.
fn load_json_or_jsonc(path: &Path) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    jsonc_parser::parse_to_serde_value(&content, &jsonc_parse_options())
        .map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "config file extensions are always lowercase"
)]
fn load_stylelint_config(path: &Path) -> Result<serde_json::Value, String> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if filename.ends_with(".js") || filename.ends_with(".cjs") || filename.ends_with(".mjs") {
        return load_stylelint_js_config(path);
    }
    load_json_or_jsonc(path)
}

fn load_stylelint_js_config(path: &Path) -> Result<serde_json::Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let Some(start) = content.find('{') else {
        return Err(format!(
            "failed to parse {}: no object literal found",
            path.display()
        ));
    };
    let Some(end) = content.rfind('}') else {
        return Err(format!(
            "failed to parse {}: no object literal end found",
            path.display()
        ));
    };
    if start >= end {
        return Err(format!(
            "failed to parse {}: invalid object literal",
            path.display()
        ));
    }
    let object = &content[start..=end];
    jsonc_parser::parse_to_serde_value(
        object,
        &jsonc_parser::ParseOptions {
            allow_comments: true,
            allow_loose_object_property_names: true,
            allow_trailing_commas: true,
            allow_missing_commas: false,
            allow_single_quoted_strings: true,
            allow_hexadecimal_numbers: false,
            allow_unary_plus_numbers: false,
        },
    )
    .map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

fn jsonc_parse_options() -> jsonc_parser::ParseOptions {
    jsonc_parser::ParseOptions {
        allow_comments: true,
        allow_loose_object_property_names: false,
        allow_trailing_commas: true,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    }
}

/// Strip JSONC-style trailing commas (`,` immediately before `}` or `]`)
/// without touching commas inside string literals.
#[cfg(test)]
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

#[cfg(test)]
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

/// Strip any trailing ` (...)` suffix from a `MigrationResult.sources` entry,
/// returning the original filename / path portion. The migrator appends
/// `" (knip key)"`, `" (jscpd key)"`, `" (knip config)"`, or `" (jscpd config)"`
/// to a source so downstream predicates can detect tool provenance, but
/// extension-matching predicates (`OutputFormat::pick`'s `.jsonc` auto-mirror)
/// and user-facing output must see the original filename. Uses `rsplit_once`
/// so a project path containing its own ` (...)` segment (e.g.
/// `/path/to/react (v18)/knip.jsonc`) is preserved correctly; the closing-paren
/// guard rejects accidental matches on unbalanced text. See issue #457.
fn source_head(s: &str) -> &str {
    if let Some((head, tail)) = s.rsplit_once(" (")
        && tail.ends_with(')')
    {
        return head;
    }
    s
}

/// Decide whether the migrate command should print a glob-semantics caveat
/// after the warnings block. Emitted only when knip contributed to the
/// migration AND the resulting config carries `entry` or `ignorePatterns`,
/// since those are the only fields where knip's glob engine and fallow's
/// `globset` can diverge. See issue #457.
fn should_emit_glob_caveat(result: &MigrationResult) -> bool {
    let knip_contributed = result.sources.iter().any(|s| s.contains("knip"));
    if !knip_contributed {
        return false;
    }
    let Some(obj) = result.config.as_object() else {
        return false;
    };
    obj.contains_key("entry") || obj.contains_key("ignorePatterns")
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
