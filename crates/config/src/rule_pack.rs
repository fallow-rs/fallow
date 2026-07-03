use std::path::{Path, PathBuf};

use fallow_types::suppress::is_valid_policy_identifier;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::glob_validation::compile_user_glob;
use crate::config::{BoundaryConfig, ResolvedBoundaryConfig, Severity};

/// Supported rule-pack file extensions. TOML is intentionally not supported:
/// JSON Schema autocomplete is the headline authoring feature and TOML
/// editors do not consume it.
const RULE_PACK_EXTENSIONS: &[&str] = &["json", "jsonc"];

/// The rule-pack format version this fallow build understands.
const SUPPORTED_PACK_VERSION: u32 = 1;

/// Which check a rule-pack rule performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RulePackRuleKind {
    /// Ban call sites whose callee path matches one of `callees`.
    BannedCall,
    /// Ban imports and re-exports whose raw specifier matches one of
    /// `specifiers`.
    BannedImport,
    /// Ban call sites whose catalogue-derived effect matches one of `effects`.
    BannedEffect,
    /// Ban exported names that match one of `exports`.
    BannedExport,
}

/// Internal side-effect taxonomy derived from security catalogue rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EffectKind {
    Pure,
    Read,
    Write,
    Network,
    Storage,
    Process,
    Shell,
    Crypto,
    Randomness,
    Dom,
    Database,
    FrameworkCallback,
    Unknown,
}

impl EffectKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pure => "pure",
            Self::Read => "read",
            Self::Write => "write",
            Self::Network => "network",
            Self::Storage => "storage",
            Self::Process => "process",
            Self::Shell => "shell",
            Self::Crypto => "crypto",
            Self::Randomness => "randomness",
            Self::Dom => "dom",
            Self::Database => "database",
            Self::FrameworkCallback => "framework-callback",
            Self::Unknown => "unknown",
        }
    }
}

/// One declarative policy rule inside a rule pack.
///
/// `callees` applies only to `banned-call` rules; `specifiers` and
/// `ignoreTypeOnly` apply only to `banned-import` rules; `effects` applies
/// only to `banned-effect` rules; `exports` applies only to `banned-export`
/// rules. `zones` can scope any rule kind to files classified into one of the
/// named boundary zones. Setting a field on the wrong kind is a load error
/// (fail loud, never silently ignore policy).
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RulePackRule {
    /// Rule id, unique within the pack. Must use only ASCII letters, digits,
    /// `.`, `_`, and `-` so `"<pack>/<id>"` is unambiguous in output,
    /// baselines, and scoped suppression comments.
    pub id: String,
    /// Which check this rule performs.
    pub kind: RulePackRuleKind,
    /// Callee patterns to ban (`banned-call` only). Matching is segment-aware
    /// and import-resolved, identical to `boundaries.calls.forbidden`:
    /// `child_process.*` covers `import { exec } from "node:child_process"`,
    /// the bare specifier, and namespace/default imports; `fetch` matches only
    /// the global `fetch`; a leading `*.member` matches any object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub callees: Vec<String>,
    /// Import specifiers to ban (`banned-import` only). Matched segment-aware
    /// against the RAW specifier: `moment` covers `moment` and
    /// `moment/locale/nl` but not `moment-timezone`. A trailing `/*` form,
    /// such as `@org/ui/*`, matches subpaths only (`@org/ui/internal`) and
    /// not the package root (`@org/ui`). Aliased or rewritten specifiers
    /// (e.g. `npm:moment`) are not matched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub specifiers: Vec<String>,
    /// Effect classes to ban (`banned-effect` only). Effects are derived from
    /// `security_matchers.toml` catalogue rows and matched against captured
    /// call sites after import-resolution canonicalization.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<EffectKind>,
    /// Export names to ban (`banned-export` only). `"default"` matches the
    /// default export; any other entry matches an exported name exactly; a
    /// single trailing `*` makes it a prefix match (`internal*`). No other
    /// glob syntax is supported. Re-exports are out of scope for this rule.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<String>,
    /// When `true`, type-only imports (`import type ...` and type-only
    /// re-exports) are ignored by `banned-import`; type-only exports are
    /// ignored by `banned-export`. Defaults to `false`: type-only sites are
    /// flagged too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignore_type_only: bool,
    /// Optional include globs (project-root-relative). Empty or absent means
    /// the rule applies to every analyzed file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Optional exclude globs (project-root-relative), applied after `files`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Optional boundary zones this rule applies to. Empty or absent means the
    /// rule applies regardless of zone; non-empty values require matching
    /// configured boundaries and combine with `files`/`exclude` as AND.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<String>,
    /// Author-provided message naming the sanctioned alternative. Rendered
    /// next to each finding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Per-rule severity overriding the `rules."policy-violation"` master.
    /// `off` disables this rule. When the master itself is `off`, the whole
    /// evaluator is disabled and per-rule severity cannot resurrect it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
}

/// A declarative rule pack loaded from a standalone JSON or JSONC file listed
/// in the `rulePacks` config key.
///
/// Rule packs are pure data: loading a pack never executes project code. They
/// encode project-specific policy (banned calls, banned imports, and
/// catalogue-backed banned effects) evaluated over fallow's static extraction
/// data, reporting as `policy-violation`
/// findings.
///
/// ```jsonc
/// {
///   "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/rule-pack-schema.json",
///   "version": 1,
///   "name": "team-policy",
///   "description": "House rules for the platform team",
///   "rules": [
///     {
///       "id": "no-child-process",
///       "kind": "banned-call",
///       "callees": ["child_process.*"],
///       "message": "Use the sandboxed runner instead.",
///       "severity": "error"
///     },
///     {
///       "id": "no-network",
///       "kind": "banned-effect",
///       "effects": ["network"],
///       "message": "Keep this package side-effect free."
///     },
///     {
///       "id": "no-moment",
///       "kind": "banned-import",
///       "specifiers": ["moment"],
///       "message": "Use date-fns."
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RulePackDef {
    /// JSON Schema reference (ignored during deserialization).
    #[serde(rename = "$schema", default, skip_serializing)]
    #[schemars(skip)]
    pub schema: Option<String>,
    /// Pack format version. Must be `1`; the field exists so future rule
    /// kinds can be added without breaking older fallow builds silently.
    pub version: u32,
    /// Pack name, unique across all loaded packs. Must use only ASCII
    /// letters, digits, `.`, `_`, and `-` so `"<pack>/<id>"` is unambiguous in
    /// output, baselines, and scoped suppression comments.
    pub name: String,
    /// Optional human description of the pack's intent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The policy rules this pack enforces. Must be non-empty: an empty pack
    /// would silently enforce nothing.
    pub rules: Vec<RulePackRule>,
}

impl RulePackDef {
    /// Generate JSON Schema for the rule-pack format (consumed by
    /// `fallow rule-pack-schema` for editor autocomplete).
    #[must_use]
    pub fn json_schema() -> serde_json::Value {
        serde_json::to_value(schemars::schema_for!(RulePackDef)).unwrap_or_default()
    }
}

/// One rule-pack load or validation failure, anchored at the offending pack
/// file.
#[derive(Debug, Clone)]
pub struct RulePackError {
    /// The pack file (as listed in `rulePacks`, root-joined).
    pub path: PathBuf,
    /// What went wrong, including the rule id when the error is rule-scoped.
    pub message: String,
}

impl std::fmt::Display for RulePackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

/// Load and validate every rule pack listed in the `rulePacks` config key.
///
/// Paths are project-root-relative. Every failure is collected (missing file,
/// unsupported extension, parse error, schema violation) so the user sees all
/// problems in one run. A pack that fails any check fails the whole load:
/// silently skipping policy would be worse than failing.
///
/// # Errors
///
/// Returns the accumulated list of [`RulePackError`] entries when any listed
/// pack is missing, unparsable, or invalid.
pub fn load_rule_packs(
    root: &Path,
    pack_paths: &[String],
) -> Result<Vec<RulePackDef>, Vec<RulePackError>> {
    let mut packs = Vec::new();
    let mut errors = Vec::new();
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    for path_str in pack_paths {
        load_one_rule_pack(root, path_str, &canonical_root, &mut packs, &mut errors);
    }

    push_duplicate_pack_name_errors(root, &packs, &mut errors);

    if errors.is_empty() {
        Ok(packs)
    } else {
        Err(errors)
    }
}

/// Resolve boundaries in the same shape used by analysis, without loading
/// rule packs or running discovery.
#[must_use]
pub fn resolve_boundaries_for_rule_pack_validation(
    mut boundaries: BoundaryConfig,
    root: &Path,
) -> ResolvedBoundaryConfig {
    if boundaries.preset.is_some() {
        let source_root = crate::workspace::parse_tsconfig_root_dir(root)
            .filter(|r| r != "." && !r.starts_with("..") && !Path::new(r).is_absolute())
            .unwrap_or_else(|| "src".to_owned());
        boundaries.expand(&source_root);
    }
    let logical_groups = boundaries.expand_auto_discover(root);
    let mut resolved = boundaries.resolve();
    resolved.logical_groups = logical_groups;
    resolved
}

/// Validate that rule-pack `zones` references point at resolved boundary zones.
#[must_use]
pub fn validate_rule_pack_zone_references(
    root: &Path,
    pack_paths: &[String],
    packs: &[RulePackDef],
    boundaries: &ResolvedBoundaryConfig,
) -> Vec<RulePackError> {
    let configured_zones: FxHashSet<&str> = boundaries
        .zones
        .iter()
        .map(|zone| zone.name.as_str())
        .collect();
    let configured_zone_list = if configured_zones.is_empty() {
        "none".to_owned()
    } else {
        let mut zones: Vec<&str> = configured_zones.iter().copied().collect();
        zones.sort_unstable();
        zones.join(", ")
    };

    let mut errors = Vec::new();
    for (pack_index, pack) in packs.iter().enumerate() {
        let path = pack_paths
            .get(pack_index)
            .map_or_else(|| root.to_path_buf(), |path| root.join(path));
        for rule in &pack.rules {
            if rule.zones.is_empty() {
                continue;
            }
            if configured_zones.is_empty() {
                errors.push(RulePackError {
                    path: path.clone(),
                    message: format!(
                        "rule '{}': `zones` requires configured boundary zones, but none are configured",
                        rule.id
                    ),
                });
                continue;
            }
            for zone in &rule.zones {
                if !configured_zones.contains(zone.as_str()) {
                    errors.push(RulePackError {
                        path: path.clone(),
                        message: format!(
                            "rule '{}': unknown zone '{}' in `zones`; configured zones: {}",
                            rule.id, zone, configured_zone_list
                        ),
                    });
                }
            }
        }
    }
    errors
}

/// Load, validate, and stage a single listed rule pack, collecting any failure.
fn load_one_rule_pack(
    root: &Path,
    path_str: &str,
    canonical_root: &Path,
    packs: &mut Vec<RulePackDef>,
    errors: &mut Vec<RulePackError>,
) {
    let path = root.join(path_str);
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !RULE_PACK_EXTENSIONS.contains(&ext) {
        errors.push(RulePackError {
            path: path.clone(),
            message: format!("unsupported rule pack extension '.{ext}'; expected .json or .jsonc"),
        });
        return;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) => {
            errors.push(RulePackError {
                path,
                message: format!("failed to read rule pack: {e}"),
            });
            return;
        }
    };
    // Checked after the read so a missing file reports as missing even on
    // platforms where the project root itself sits behind a symlink.
    if !crate::external_plugin::is_within_root(&path, canonical_root) {
        errors.push(RulePackError {
            path,
            message: "resolves outside the project root".to_owned(),
        });
        return;
    }
    let parsed: Result<RulePackDef, String> = if ext == "jsonc" {
        crate::jsonc::parse_to_value::<RulePackDef>(&content).map_err(|e| e.to_string())
    } else {
        serde_json::from_str::<RulePackDef>(&content).map_err(|e| e.to_string())
    };
    match parsed {
        Ok(pack) => {
            let before = errors.len();
            validate_pack(&pack, &path, errors);
            if errors.len() == before {
                packs.push(pack);
            }
        }
        Err(message) => {
            errors.push(RulePackError {
                path,
                message: format!("failed to parse rule pack: {message}"),
            });
        }
    }
}

/// Push one error per pack name declared by more than one loaded pack.
fn push_duplicate_pack_name_errors(
    root: &Path,
    packs: &[RulePackDef],
    errors: &mut Vec<RulePackError>,
) {
    let mut seen_names: FxHashSet<&str> = FxHashSet::default();
    for pack in packs {
        if !seen_names.insert(pack.name.as_str()) {
            errors.push(RulePackError {
                path: root.to_path_buf(),
                message: format!(
                    "rule pack name '{}' is declared by more than one pack; pack names must be \
                     unique because findings are identified as '<pack>/<rule-id>'",
                    pack.name
                ),
            });
        }
    }
}

/// Validate a parsed pack. Pushes one error per problem so a pack with three
/// bad rules reports all three.
fn validate_pack(pack: &RulePackDef, path: &Path, errors: &mut Vec<RulePackError>) {
    let err = |message: String| RulePackError {
        path: path.to_path_buf(),
        message,
    };

    if pack.version != SUPPORTED_PACK_VERSION {
        errors.push(err(format!(
            "unsupported rule pack version {}; this fallow build supports version \
             {SUPPORTED_PACK_VERSION}",
            pack.version
        )));
    }
    if pack.name.trim().is_empty() {
        errors.push(err("pack `name` must not be empty".to_owned()));
    } else if !is_valid_policy_identifier(&pack.name) {
        errors.push(err(format!(
            "pack `name` '{}' must use only ASCII letters, digits, '.', '_', and '-'",
            pack.name
        )));
    }
    if pack.rules.is_empty() {
        errors.push(err(
            "pack declares no rules; an empty pack would silently enforce nothing".to_owned(),
        ));
    }

    let mut seen_ids: FxHashSet<&str> = FxHashSet::default();
    for rule in &pack.rules {
        if rule.id.trim().is_empty() {
            errors.push(err("rule `id` must not be empty".to_owned()));
            continue;
        }
        if !is_valid_policy_identifier(&rule.id) {
            errors.push(err(format!(
                "rule `id` '{}' must use only ASCII letters, digits, '.', '_', and '-'",
                rule.id
            )));
            continue;
        }
        if !seen_ids.insert(rule.id.as_str()) {
            errors.push(err(format!(
                "duplicate rule id '{}'; rule ids must be unique within a pack",
                rule.id
            )));
        }
        validate_rule(rule, path, errors);
    }
}

/// Validate one rule's kind-specific fields and patterns.
fn validate_rule(rule: &RulePackRule, path: &Path, errors: &mut Vec<RulePackError>) {
    let err = |message: String| RulePackError {
        path: path.to_path_buf(),
        message: format!("rule '{}': {message}", rule.id),
    };

    match rule.kind {
        RulePackRuleKind::BannedCall => validate_banned_call_rule(rule, &err, errors),
        RulePackRuleKind::BannedImport => validate_banned_import_rule(rule, &err, errors),
        RulePackRuleKind::BannedEffect => validate_banned_effect_rule(rule, &err, errors),
        RulePackRuleKind::BannedExport => validate_banned_export_rule(rule, &err, errors),
    }

    validate_rule_file_globs(rule, &err, errors);
}

/// Validate a `banned-call` rule's required and cross-kind fields.
fn validate_banned_call_rule(
    rule: &RulePackRule,
    err: &impl Fn(String) -> RulePackError,
    errors: &mut Vec<RulePackError>,
) {
    if rule.callees.is_empty() {
        errors.push(err(
            "banned-call rules must list at least one `callees` pattern".to_owned(),
        ));
    }
    if !rule.specifiers.is_empty() {
        errors.push(err(
            "`specifiers` applies only to banned-import rules".to_owned()
        ));
    }
    if !rule.effects.is_empty() {
        errors.push(err(
            "`effects` applies only to banned-effect rules".to_owned()
        ));
    }
    if !rule.exports.is_empty() {
        errors.push(err(
            "`exports` applies only to banned-export rules".to_owned()
        ));
    }
    if rule.ignore_type_only {
        errors.push(err(
            "`ignoreTypeOnly` applies only to banned-import rules".to_owned()
        ));
    }
    for pattern in &rule.callees {
        if let Some(reason) = callee_pattern_error(pattern) {
            errors.push(err(format!("callee pattern `{pattern}` {reason}")));
        }
    }
}

/// Validate a `banned-import` rule's required and cross-kind fields.
fn validate_banned_import_rule(
    rule: &RulePackRule,
    err: &impl Fn(String) -> RulePackError,
    errors: &mut Vec<RulePackError>,
) {
    if rule.specifiers.is_empty() {
        errors.push(err(
            "banned-import rules must list at least one `specifiers` entry".to_owned(),
        ));
    }
    if !rule.callees.is_empty() {
        errors.push(err("`callees` applies only to banned-call rules".to_owned()));
    }
    if !rule.effects.is_empty() {
        errors.push(err(
            "`effects` applies only to banned-effect rules".to_owned()
        ));
    }
    if !rule.exports.is_empty() {
        errors.push(err(
            "`exports` applies only to banned-export rules".to_owned()
        ));
    }
    for specifier in &rule.specifiers {
        if specifier.trim().is_empty() {
            errors.push(err("specifier must not be empty".to_owned()));
        } else if let Some(prefix) = specifier.strip_suffix("/*") {
            if prefix.is_empty() || prefix.contains('*') {
                errors.push(err(format!(
                    "specifier `{specifier}` contains `*`; specifier matching is segment-aware, \
                     not glob. Only a single trailing `/*` deep-import form is allowed"
                )));
            }
        } else if specifier.contains('*') {
            errors.push(err(format!(
                "specifier `{specifier}` contains `*`; specifier matching is \
                 segment-aware, not glob. List the package or path prefix; subpaths are \
                 covered automatically, or use a single trailing `/*` to match subpaths only"
            )));
        }
    }
}

/// Validate a `banned-effect` rule's required and cross-kind fields.
fn validate_banned_effect_rule(
    rule: &RulePackRule,
    err: &impl Fn(String) -> RulePackError,
    errors: &mut Vec<RulePackError>,
) {
    if rule.effects.is_empty() {
        errors.push(err(
            "banned-effect rules must list at least one `effects` entry".to_owned(),
        ));
    }
    if !rule.callees.is_empty() {
        errors.push(err("`callees` applies only to banned-call rules".to_owned()));
    }
    if !rule.specifiers.is_empty() {
        errors.push(err(
            "`specifiers` applies only to banned-import rules".to_owned()
        ));
    }
    if !rule.exports.is_empty() {
        errors.push(err(
            "`exports` applies only to banned-export rules".to_owned()
        ));
    }
    if rule.ignore_type_only {
        errors.push(err(
            "`ignoreTypeOnly` applies only to banned-import and banned-export rules".to_owned(),
        ));
    }
}

/// Validate a `banned-export` rule's required and cross-kind fields.
fn validate_banned_export_rule(
    rule: &RulePackRule,
    err: &impl Fn(String) -> RulePackError,
    errors: &mut Vec<RulePackError>,
) {
    if rule.exports.is_empty() {
        errors.push(err(
            "banned-export rules must list at least one `exports` entry".to_owned(),
        ));
    }
    if !rule.callees.is_empty() {
        errors.push(err("`callees` applies only to banned-call rules".to_owned()));
    }
    if !rule.specifiers.is_empty() {
        errors.push(err(
            "`specifiers` applies only to banned-import rules".to_owned()
        ));
    }
    if !rule.effects.is_empty() {
        errors.push(err(
            "`effects` applies only to banned-effect rules".to_owned()
        ));
    }
    for export in &rule.exports {
        if export.trim().is_empty() {
            errors.push(err("export pattern must not be empty".to_owned()));
        } else if let Some(stripped) = export.strip_suffix('*') {
            if stripped.is_empty() || stripped.contains('*') {
                errors.push(err(format!(
                    "export pattern `{export}` may only use a single trailing `*` after a prefix"
                )));
            }
        } else if export.contains('*') {
            errors.push(err(format!(
                "export pattern `{export}` may only use `*` as a single trailing prefix wildcard"
            )));
        }
    }
}

/// Validate a rule's `files` and `exclude` include/exclude globs.
fn validate_rule_file_globs(
    rule: &RulePackRule,
    err: &impl Fn(String) -> RulePackError,
    errors: &mut Vec<RulePackError>,
) {
    for (field, patterns) in [("files", &rule.files), ("exclude", &rule.exclude)] {
        for pattern in patterns {
            if let Err(e) = compile_user_glob(pattern, "rulePacks rules[].files/exclude") {
                errors.push(err(format!("invalid `{field}` glob `{pattern}`: {e}")));
            }
        }
    }
}

/// Reject callee patterns the segment-aware matcher cannot honor, using the
/// same rules as `boundaries.calls.forbidden` (`validate_call_rules`).
fn callee_pattern_error(pattern: &str) -> Option<String> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Some("must not be empty".to_owned());
    }
    if trimmed == "*" {
        return Some(
            "matches nothing: a bare `*` has no callee segments. Name a specific callee such as \
             `console.*` or `child_process.exec`"
                .to_owned(),
        );
    }
    if trimmed.split('.').any(|segment| segment.trim().is_empty()) {
        return Some("contains an empty path segment".to_owned());
    }
    crate::config::wildcard_placement_error(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_pack(dir: &Path, name: &str, content: &str) -> String {
        std::fs::write(dir.join(name), content).unwrap();
        name.to_owned()
    }

    fn valid_pack_json() -> &'static str {
        r#"{
            "version": 1,
            "name": "team-policy",
            "description": "House rules",
            "rules": [
                {
                    "id": "no-child-process",
                    "kind": "banned-call",
                    "callees": ["child_process.*", "execa"],
                    "files": ["src/**"],
                    "exclude": ["src/tooling/**"],
                    "message": "Use the sandboxed runner instead.",
                    "severity": "error"
                },
                {
                    "id": "no-network",
                    "kind": "banned-effect",
                    "effects": ["network"],
                    "message": "Keep this package side-effect free."
                },
                {
                    "id": "no-moment",
                    "kind": "banned-import",
                    "specifiers": ["moment"],
                    "ignoreTypeOnly": true,
                    "message": "Use date-fns."
                }
            ]
        }"#
    }

    #[test]
    fn loads_valid_json_pack() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(dir.path(), "policy.json", valid_pack_json());
        let packs = load_rule_packs(dir.path(), &[path]).unwrap();
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].name, "team-policy");
        assert_eq!(packs[0].rules.len(), 3);
        assert_eq!(packs[0].rules[0].kind, RulePackRuleKind::BannedCall);
        assert_eq!(packs[0].rules[0].severity, Some(Severity::Error));
        assert_eq!(packs[0].rules[1].kind, RulePackRuleKind::BannedEffect);
        assert_eq!(packs[0].rules[1].effects, vec![EffectKind::Network]);
        assert_eq!(packs[0].rules[2].kind, RulePackRuleKind::BannedImport);
        assert!(packs[0].rules[2].ignore_type_only);
        assert_eq!(packs[0].rules[2].severity, None);
    }

    #[test]
    fn loads_jsonc_pack_with_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.jsonc",
            r#"{
                // why: keep the domain layer pure
                "version": 1,
                "name": "jsonc-policy",
                "rules": [
                    { "id": "no-console", "kind": "banned-call", "callees": ["console.*"] },
                ]
            }"#,
        );
        let packs = load_rule_packs(dir.path(), &[path]).unwrap();
        assert_eq!(packs[0].name, "jsonc-policy");
    }

    #[test]
    fn parses_zone_scoped_rules() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "domain-network", "kind": "banned-effect",
                  "effects": ["network"], "zones": ["domain"] }
            ] }"#,
        );
        let packs = load_rule_packs(dir.path(), &[path]).unwrap();
        assert_eq!(packs[0].rules[0].zones, vec!["domain"]);
    }

    #[test]
    fn validates_rule_pack_zones_against_resolved_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "domain-network", "kind": "banned-effect",
                  "effects": ["network"], "zones": ["unknown"] }
            ] }"#,
        );
        let packs = load_rule_packs(dir.path(), std::slice::from_ref(&path)).unwrap();
        let boundaries = BoundaryConfig {
            zones: vec![crate::config::BoundaryZone {
                name: "domain".to_owned(),
                patterns: vec!["src/domain/**".to_owned()],
                auto_discover: Vec::new(),
                root: None,
            }],
            ..BoundaryConfig::default()
        }
        .resolve();

        let errors = validate_rule_pack_zone_references(dir.path(), &[path], &packs, &boundaries);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].message.contains("unknown zone 'unknown'"));
        assert!(errors[0].message.contains("configured zones: domain"));
    }

    #[test]
    fn rejects_rule_pack_zones_when_boundaries_are_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "domain-network", "kind": "banned-effect",
                  "effects": ["network"], "zones": ["domain"] }
            ] }"#,
        );
        let packs = load_rule_packs(dir.path(), std::slice::from_ref(&path)).unwrap();
        let errors = validate_rule_pack_zone_references(
            dir.path(),
            &[path],
            &packs,
            &ResolvedBoundaryConfig::default(),
        );
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0]
                .message
                .contains("`zones` requires configured boundary zones")
        );
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 2, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call", "callees": ["fetch"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(
            errors[0]
                .message
                .contains("unsupported rule pack version 2")
        );
    }

    #[test]
    fn rejects_unknown_kind_with_expected_list() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-thing", "callees": ["fetch"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("banned-thing"));
        assert!(errors[0].message.contains("banned-effect"));
        assert!(errors[0].message.contains("banned-call"));
        assert!(errors[0].message.contains("banned-import"));
        assert!(errors[0].message.contains("banned-export"));
    }

    #[test]
    fn rejects_unknown_field() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call", "callees": ["fetch"], "file": ["src/**"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("file"));
    }

    #[test]
    fn rejects_empty_rules_and_empty_pack_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": " ", "rules": [] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        let joined = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("declares no rules"));
        assert!(joined.contains("`name` must not be empty"));
    }

    #[test]
    fn rejects_pack_names_that_cannot_be_scoped_suppression_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "team/policy", "rules": [
                { "id": "no-child-process", "kind": "banned-call", "callees": ["fetch"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("pack `name` 'team/policy'"));
        assert!(errors[0].message.contains("ASCII letters"));
    }

    #[test]
    fn rejects_rule_ids_that_cannot_be_scoped_suppression_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "team-policy", "rules": [
                { "id": "no:child-process", "kind": "banned-call", "callees": ["fetch"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("rule `id` 'no:child-process'"));
        assert!(errors[0].message.contains("ASCII letters"));
    }

    #[test]
    fn rejects_duplicate_rule_ids_within_pack() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call", "callees": ["fetch"] },
                { "id": "a", "kind": "banned-import", "specifiers": ["moment"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("duplicate rule id 'a'"));
    }

    #[test]
    fn rejects_duplicate_pack_names() {
        let dir = tempfile::tempdir().unwrap();
        let a = write_pack(
            dir.path(),
            "a.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call", "callees": ["fetch"] }
            ] }"#,
        );
        let b = write_pack(
            dir.path(),
            "b.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "b", "kind": "banned-call", "callees": ["eval"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[a, b]).unwrap_err();
        assert!(errors[0].message.contains("rule pack name 'p'"));
    }

    #[test]
    fn rejects_cross_kind_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call", "callees": ["fetch"],
                  "specifiers": ["moment"], "effects": ["network"], "exports": ["default"],
                  "ignoreTypeOnly": true },
                { "id": "b", "kind": "banned-import", "specifiers": ["moment"],
                  "callees": ["fetch"], "effects": ["network"], "exports": ["default"] },
                { "id": "c", "kind": "banned-effect", "effects": ["network"],
                  "callees": ["fetch"], "specifiers": ["moment"], "exports": ["default"],
                  "ignoreTypeOnly": true },
                { "id": "d", "kind": "banned-export", "exports": ["default"],
                  "callees": ["fetch"], "specifiers": ["moment"], "effects": ["network"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        let joined = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("`specifiers` applies only to banned-import"));
        assert!(
            joined.contains("`ignoreTypeOnly` applies only to banned-import and banned-export")
        );
        assert!(joined.contains("`callees` applies only to banned-call"));
        assert!(joined.contains("`effects` applies only to banned-effect"));
        assert!(joined.contains("`exports` applies only to banned-export"));
    }

    #[test]
    fn rejects_missing_kind_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call" },
                { "id": "b", "kind": "banned-import" },
                { "id": "c", "kind": "banned-effect" },
                { "id": "d", "kind": "banned-export" }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        let joined = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("must list at least one `callees` pattern"));
        assert!(joined.contains("must list at least one `specifiers` entry"));
        assert!(joined.contains("must list at least one `effects` entry"));
        assert!(joined.contains("must list at least one `exports` entry"));
    }

    #[test]
    fn loads_banned_export_rule() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "no-default", "kind": "banned-export",
                  "exports": ["default", "internal*"], "ignoreTypeOnly": true }
            ] }"#,
        );
        let packs = load_rule_packs(dir.path(), &[path]).unwrap();
        assert_eq!(packs[0].rules[0].kind, RulePackRuleKind::BannedExport);
        assert_eq!(packs[0].rules[0].exports, vec!["default", "internal*"]);
        assert!(packs[0].rules[0].ignore_type_only);
    }

    #[test]
    fn rejects_invalid_banned_export_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "bad", "kind": "banned-export",
                  "exports": ["", "*", "a*b"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        let joined = errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("export pattern must not be empty"));
        assert!(joined.contains("may only use a single trailing `*` after a prefix"));
        assert!(joined.contains("may only use `*` as a single trailing prefix wildcard"));
    }

    #[test]
    fn rejects_inert_callee_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call",
                  "callees": ["*", "a..b", "child*", "a.*.b"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert_eq!(errors.len(), 4);
    }

    #[test]
    fn rejects_glob_specifiers() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-import", "specifiers": ["moment/**"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("segment-aware, not glob"));
    }

    #[test]
    fn accepts_trailing_star_deep_import_specifier() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "no-ui-deep-imports", "kind": "banned-import",
                  "specifiers": ["@org/ui/*"] }
            ] }"#,
        );
        let packs = load_rule_packs(dir.path(), &[path]).unwrap();
        assert_eq!(packs[0].rules[0].specifiers, vec!["@org/ui/*"]);
    }

    #[test]
    fn rejects_non_trailing_star_import_specifier() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "bad-deep-imports", "kind": "banned-import",
                  "specifiers": ["@org/*/x"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("single trailing `/*`"));
    }

    #[test]
    fn rejects_traversal_globs() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_pack(
            dir.path(),
            "policy.json",
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call", "callees": ["fetch"],
                  "files": ["../outside/**"] }
            ] }"#,
        );
        let errors = load_rule_packs(dir.path(), &[path]).unwrap_err();
        assert!(errors[0].message.contains("invalid `files` glob"));
    }

    #[test]
    fn rejects_missing_pack_file_and_bad_extension() {
        let dir = tempfile::tempdir().unwrap();
        write_pack(dir.path(), "policy.toml", "version = 1");
        let errors = load_rule_packs(
            dir.path(),
            &["missing.json".to_owned(), "policy.toml".to_owned()],
        )
        .unwrap_err();
        assert_eq!(errors.len(), 2);
        assert!(errors[0].message.contains("failed to read rule pack"));
        assert!(
            errors[1]
                .message
                .contains("unsupported rule pack extension")
        );
    }

    #[test]
    fn rejects_paths_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("project");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(
            dir.path().join("outside.json"),
            r#"{ "version": 1, "name": "p", "rules": [
                { "id": "a", "kind": "banned-call", "callees": ["fetch"] }
            ] }"#,
        )
        .unwrap();
        let errors = load_rule_packs(&inner, &["../outside.json".to_owned()]).unwrap_err();
        assert!(errors[0].message.contains("outside the project root"));
    }

    #[test]
    fn schema_validates_doc_example_shape() {
        let schema = RulePackDef::json_schema();
        let properties = schema
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("schema should expose properties");
        assert!(properties.contains_key("version"));
        assert!(properties.contains_key("name"));
        assert!(properties.contains_key("rules"));

        // The doc-comment example must parse with the same serde shape the
        // schema is generated from.
        let pack: RulePackDef = serde_json::from_str(valid_pack_json()).unwrap();
        assert_eq!(pack.version, 1);
    }
}
