//! Evaluation of external-plugin `manifestEntries` rules.
//!
//! A [`ManifestEntryRule`] seeds entry points DERIVED from framework manifest
//! files: it finds manifests by a recursive glob (a bounded, `.gitignore`-aware
//! second walk, because manifests are config files and are NOT in the
//! source-discovery set), parses each one, and for every manifest that passes
//! the rule-level `when` gate resolves each `entries[].path` relative to that
//! manifest's directory (with `${dotted.field}` interpolation) into a
//! root-relative entry pattern.
//!
//! The dominant failure mode is silent-none across a large manifest set (a typo
//! in a field path seeds nothing), so evaluation emits loud `tracing::warn!`
//! diagnostics: a `manifests` glob that matches nothing, a `when` that excludes
//! every matched manifest, a referenced field path that resolves in zero
//! matched manifests, an empty `entries` list, and unparseable manifests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fallow_config::{ExternalPluginDef, ManifestEntryRule};
use serde_json::Value;

use super::PathRule;
use super::config_parser::normalize_config_path;

/// A kind of `manifestEntries` diagnostic, kebab-serialized for agents that
/// branch on it. Centralizes the vocabulary shared by the production warn path
/// (`evaluate_manifest_entries`) and the agent-facing check path
/// (`check_manifest_entries` / `fallow plugin-check`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningKind {
    /// The `manifests` glob matched zero files.
    ManifestsMatchedNone,
    /// The `when` gate excluded every matched manifest.
    WhenExcludedAll,
    /// A referenced field path resolved in none of the gated manifests (typo).
    FieldPathUnresolved,
    /// The rule's `entries` list is empty; it seeds nothing.
    EntriesEmpty,
    /// One or more matched manifests could not be read or parsed.
    ManifestParseFailed,
    /// An entry resolved outside the project root and was skipped.
    EntryOutsideRoot,
    /// A rule seeded entries but none of the seeded paths exist on disk.
    /// Check-only (production seeds the pattern regardless of existence).
    SeededPathsMissing,
}

impl WarningKind {
    /// The kebab-case token agents branch on.
    #[must_use]
    pub fn as_kebab(self) -> &'static str {
        match self {
            Self::ManifestsMatchedNone => "manifests-matched-none",
            Self::WhenExcludedAll => "when-excluded-all",
            Self::FieldPathUnresolved => "field-path-unresolved",
            Self::EntriesEmpty => "entries-empty",
            Self::ManifestParseFailed => "manifest-parse-failed",
            Self::EntryOutsideRoot => "entry-outside-root",
            Self::SeededPathsMissing => "seeded-paths-missing",
        }
    }
}

/// A single `manifestEntries` diagnostic with typed payload slots (agents read
/// the slot their `kind` implies rather than parsing prose).
#[derive(Debug, Clone)]
pub struct CheckWarning {
    pub kind: WarningKind,
    /// The offending `manifests` glob (for `manifests-matched-none`).
    pub glob: Option<String>,
    /// The offending dotted field path (for `field-path-unresolved`).
    pub field_path: Option<String>,
    /// The manifest a per-manifest warning relates to (root-relative).
    pub manifest: Option<String>,
    /// The offending resolved entry (for `entry-outside-root`).
    pub entry: Option<String>,
}

impl CheckWarning {
    /// A warning carrying only the offending `manifests` glob.
    fn glob(kind: WarningKind, glob: &str) -> Self {
        Self {
            kind,
            glob: Some(glob.to_string()),
            field_path: None,
            manifest: None,
            entry: None,
        }
    }

    /// A warning carrying only the offending dotted field path.
    fn field(kind: WarningKind, field_path: String) -> Self {
        Self {
            kind,
            glob: None,
            field_path: Some(field_path),
            manifest: None,
            entry: None,
        }
    }

    /// A warning carrying only the offending manifest (root-relative).
    fn manifest(kind: WarningKind, manifest: String) -> Self {
        Self {
            kind,
            glob: None,
            field_path: None,
            manifest: Some(manifest),
            entry: None,
        }
    }
}

/// What one matched-and-parsed manifest yielded under a rule.
#[derive(Debug, Clone)]
pub struct ManifestResult {
    /// Root-relative manifest path.
    pub path: String,
    /// Whether the rule-level `when` gate passed for this manifest.
    pub when_passed: bool,
    /// Root-relative entry globs seeded from this manifest (empty unless
    /// `when_passed`). Each still encodes its own extension (e.g. `{ts,tsx}`).
    pub seeded: Vec<String>,
}

/// The result of evaluating one `manifestEntries` rule: the shared source of
/// truth for BOTH production seeding and the agent-facing check output, so the
/// two can never drift.
#[derive(Debug, Clone)]
pub struct RuleReport {
    /// The rule's `manifests` glob.
    pub manifests: String,
    /// Root-relative paths of the manifests the glob matched (sorted, stable).
    pub manifests_matched: Vec<String>,
    /// Per-matched-manifest results (sorted by path).
    pub matched: Vec<ManifestResult>,
    /// Diagnostics for this rule, sorted by `(kind, manifest, entry, field_path)`
    /// so the JSON is byte-identical across machines and CI runs.
    pub warnings: Vec<CheckWarning>,
}

/// Evaluate every `manifestEntries` rule on an active external plugin, returning
/// the root-relative entry patterns to seed. Delegates to the shared
/// `build_rule_report` so the seeded set and the `fallow plugin-check` report
/// are computed by identical logic, then re-emits each report warning as a
/// `tracing::warn!` (the loud stderr behavior is preserved).
///
/// Manifest files are config files, not source files, so they are not in the
/// source-discovery set; this does a bounded `.gitignore`-respecting walk (like
/// plugin detection's file-existence fallback) to find them. Manifests under
/// gitignored / `node_modules` directories are intentionally invisible.
#[must_use]
pub fn evaluate_manifest_entries(ext: &ExternalPluginDef, root: &Path) -> Vec<PathRule> {
    let mut out = Vec::new();
    for rule in &ext.manifest_entries {
        let report = build_rule_report(rule, root);
        for manifest in &report.matched {
            for seed in &manifest.seeded {
                out.push(PathRule::new(seed.clone()));
            }
        }
        emit_report_warnings(&ext.name, &report);
    }
    out
}

/// Evaluate every `manifestEntries` rule and return the STRUCTURED report per
/// rule, without seeding or warning. This is the read-only dry-run the
/// `fallow plugin-check` command surfaces to agents.
#[must_use]
pub fn check_manifest_entries(ext: &ExternalPluginDef, root: &Path) -> Vec<RuleReport> {
    ext.manifest_entries
        .iter()
        .map(|rule| build_rule_report(rule, root))
        .collect()
}

/// The shared core: walk manifests, gate on `when`, seed entries, and collect
/// diagnostics into a [`RuleReport`]. Deterministically ordered.
fn build_rule_report(rule: &ManifestEntryRule, root: &Path) -> RuleReport {
    let mut report = RuleReport {
        manifests: rule.manifests.clone(),
        manifests_matched: Vec::new(),
        matched: Vec::new(),
        warnings: Vec::new(),
    };

    if rule.entries.is_empty() {
        report.warnings.push(CheckWarning::glob(
            WarningKind::EntriesEmpty,
            &rule.manifests,
        ));
        return report;
    }

    let Ok(glob) = globset::Glob::new(&rule.manifests) else {
        // Glob validity is enforced at config load; a compile failure here is
        // defensive and, like a non-matching glob, seeds nothing.
        report.warnings.push(CheckWarning::glob(
            WarningKind::ManifestsMatchedNone,
            &rule.manifests,
        ));
        return report;
    };
    let matcher = glob.compile_matcher();

    let referenced = referenced_field_paths(rule);
    let mut resolved: BTreeMap<&str, bool> =
        referenced.iter().map(|p| (p.as_str(), false)).collect();
    let mut passed = 0usize;
    let mut parsed = 0usize;

    for file in discover_manifest_paths(root, &matcher) {
        let rel_manifest = root_relative_forward_slash(&file, root)
            .unwrap_or_else(|| file.to_string_lossy().replace('\\', "/"));
        report.manifests_matched.push(rel_manifest.clone());

        let manifest: Value = match std::fs::read_to_string(&file)
            .ok()
            .and_then(|source| fallow_config::jsonc::parse_to_value(&source).ok())
        {
            Some(value) => value,
            None => {
                // Per-file diagnostic (with the offending manifest) so an agent
                // does not have to set-difference manifests_matched vs matched.
                report.warnings.push(CheckWarning::manifest(
                    WarningKind::ManifestParseFailed,
                    rel_manifest,
                ));
                continue;
            }
        };
        parsed += 1;

        let when_passed = when_matches(&manifest, &rule.when);
        let mut seeded = Vec::new();
        if when_passed {
            passed += 1;
            for path in &referenced {
                if dotted_lookup(&manifest, path).is_some()
                    && let Some(flag) = resolved.get_mut(path.as_str())
                {
                    *flag = true;
                }
            }
            let (entries, mut entry_warnings) = seed_rule_entries(rule, &manifest, &file, root);
            seeded = entries;
            report.warnings.append(&mut entry_warnings);
        }
        report.matched.push(ManifestResult {
            path: rel_manifest,
            when_passed,
            seeded,
        });
    }

    report.warnings.extend(rule_level_warnings(
        &rule.manifests,
        report.manifests_matched.len(),
        parsed,
        passed,
        &resolved,
    ));

    // manifests_matched inherits discover_manifest_paths' sorted order; matched
    // and warnings are sorted here so the JSON is byte-identical across runs and
    // filesystems (warnings tie-break on manifest then entry, since a rule can
    // emit multiple parse-failed / entry-outside-root warnings).
    report.matched.sort_by(|a, b| a.path.cmp(&b.path));
    report.warnings.sort_by(|a, b| {
        a.kind
            .as_kebab()
            .cmp(b.kind.as_kebab())
            .then_with(|| a.manifest.cmp(&b.manifest))
            .then_with(|| a.entry.cmp(&b.entry))
            .then_with(|| a.field_path.cmp(&b.field_path))
    });
    report
}

/// Assemble the RULE-LEVEL diagnostics (matched-none / when-excluded-all /
/// field-path-unresolved) from the walk tallies. Per-manifest diagnostics
/// (parse-failed, entry-outside-root) are pushed during the walk. `parsed` is
/// the count of manifests that read + parsed; `passed` cleared the `when` gate.
fn rule_level_warnings(
    manifests: &str,
    matched: usize,
    parsed: usize,
    passed: usize,
    resolved: &BTreeMap<&str, bool>,
) -> Vec<CheckWarning> {
    let mut out = Vec::new();
    if matched == 0 {
        out.push(CheckWarning::glob(
            WarningKind::ManifestsMatchedNone,
            manifests,
        ));
        return out;
    }
    // Only claim the `when` gate excluded everything when there WERE parseable
    // manifests for it to gate; if all failed to parse, the per-file
    // parse-failed warnings already explain the zero seed.
    if parsed > 0 && passed == 0 {
        out.push(CheckWarning::glob(WarningKind::WhenExcludedAll, manifests));
        return out;
    }
    if passed == 0 {
        return out;
    }
    for (path, was_resolved) in resolved {
        if !was_resolved {
            out.push(CheckWarning::field(
                WarningKind::FieldPathUnresolved,
                (*path).to_string(),
            ));
        }
    }
    out
}

/// Seed one manifest's entries: returns the root-relative entry globs plus any
/// `entry-outside-root` diagnostics.
fn seed_rule_entries(
    rule: &ManifestEntryRule,
    manifest: &Value,
    manifest_path: &Path,
    root: &Path,
) -> (Vec<String>, Vec<CheckWarning>) {
    let rel_manifest = root_relative_forward_slash(manifest_path, root);
    let mut seeded = Vec::new();
    let mut warnings = Vec::new();
    for seed in &rule.entries {
        if !when_matches(manifest, &seed.when) {
            continue;
        }
        for concrete in expand_interpolations(&seed.path, manifest) {
            match normalize_config_path(&concrete, manifest_path, root) {
                Some(rel) => seeded.push(rel),
                None => warnings.push(CheckWarning {
                    kind: WarningKind::EntryOutsideRoot,
                    glob: None,
                    field_path: None,
                    manifest: rel_manifest.clone(),
                    entry: Some(concrete),
                }),
            }
        }
    }
    (seeded, warnings)
}

/// Re-emit a rule report's warnings as `tracing::warn!` on the production path.
fn emit_report_warnings(plugin_name: &str, report: &RuleReport) {
    for warning in &report.warnings {
        match warning.kind {
            WarningKind::EntriesEmpty => tracing::warn!(
                "Plugin '{plugin_name}': manifestEntries rule for '{}' has an empty 'entries' \
                 list; it seeds nothing.",
                report.manifests
            ),
            WarningKind::ManifestsMatchedNone => tracing::warn!(
                "Plugin '{plugin_name}': manifestEntries 'manifests' glob '{}' matched no files. \
                 Check the glob and whether the manifests live under an ignored directory.",
                report.manifests
            ),
            WarningKind::ManifestParseFailed => tracing::warn!(
                "Plugin '{plugin_name}': manifestEntries skipped manifest '{}' (glob '{}') because \
                 it could not be read or parsed.",
                warning.manifest.as_deref().unwrap_or(""),
                report.manifests
            ),
            WarningKind::WhenExcludedAll => tracing::warn!(
                "Plugin '{plugin_name}': manifestEntries 'when' gate excluded all matched \
                 manifest(s) for glob '{}'. No entries were seeded.",
                report.manifests
            ),
            WarningKind::FieldPathUnresolved => tracing::warn!(
                "Plugin '{plugin_name}': manifestEntries field path '{}' resolved in none of the \
                 gated manifest(s). Likely a typo in a 'when' key or a ${{...}} interpolation.",
                warning.field_path.as_deref().unwrap_or("")
            ),
            WarningKind::EntryOutsideRoot => tracing::warn!(
                "Plugin '{plugin_name}': manifestEntries entry '{}' (from manifest '{}') resolved \
                 outside the project root and was skipped.",
                warning.entry.as_deref().unwrap_or(""),
                warning.manifest.as_deref().unwrap_or("")
            ),
            // Check-only; never produced by build_rule_report.
            WarningKind::SeededPathsMissing => {}
        }
    }
}

/// Collect every field path a rule references (rule-level `when` keys, per-seed
/// `when` keys, and `${...}` interpolations in seed paths) for typo diagnostics.
fn referenced_field_paths(rule: &ManifestEntryRule) -> Vec<String> {
    let mut paths: Vec<String> = rule.when.keys().cloned().collect();
    for seed in &rule.entries {
        paths.extend(seed.when.keys().cloned());
        paths.extend(interpolation_field_paths(&seed.path));
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Extract the dotted field paths named by `${...}` interpolations in a path.
fn interpolation_field_paths(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = path;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        if let Some(end) = after.find('}') {
            out.push(after[..end].to_string());
            rest = &after[end + 1..];
        } else {
            break;
        }
    }
    out
}

/// Expand `${dotted.field}` interpolations in a path against a manifest, fanning
/// out over string / array field values. Returns an empty vec when any
/// interpolation resolves to nothing (a missing field seeds nothing).
fn expand_interpolations(path: &str, manifest: &Value) -> Vec<String> {
    let Some(start) = path.find("${") else {
        return vec![path.to_string()];
    };
    let prefix = &path[..start];
    let after = &path[start + 2..];
    let Some(end) = after.find('}') else {
        // Unterminated interpolation: not a valid template, seed nothing.
        return Vec::new();
    };
    let field = &after[..end];
    let suffix = &after[end + 1..];

    // Recurse on the SUFFIX only (strictly shorter, so termination is
    // guaranteed) and cartesian-combine with this field's values. A substituted
    // value is treated as a literal segment, never re-scanned for `${...}`, so a
    // manifest whose field value contains `${...}` cannot cause runaway recursion.
    let mut out = Vec::new();
    let tails = expand_interpolations(suffix, manifest);
    for value in field_segment_values(manifest, field) {
        for tail in &tails {
            out.push(format!("{prefix}{value}{tail}"));
        }
    }
    out
}

/// The path-segment string values a dotted field yields: a string or number
/// yields one; an array yields one per scalar element; anything else yields none.
fn field_segment_values(manifest: &Value, field: &str) -> Vec<String> {
    match dotted_lookup(manifest, field) {
        Some(Value::String(s)) if !s.is_empty() => vec![s.clone()],
        Some(Value::Number(n)) => vec![n.to_string()],
        Some(Value::Array(items)) => items.iter().filter_map(scalar_segment).collect(),
        _ => Vec::new(),
    }
}

fn scalar_segment(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Whether every `(dotted-path, expected)` pair in `when` matches the manifest
/// by strict equality. An empty map always matches.
fn when_matches(manifest: &Value, when: &BTreeMap<String, Value>) -> bool {
    when.iter()
        .all(|(path, expected)| dotted_lookup(manifest, path) == Some(expected))
}

/// Look up a dotted field path (`plugin.browser`) in a JSON value.
fn dotted_lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

/// Walk `root` (respecting `.gitignore`, skipping `node_modules`) and return the
/// absolute paths of files whose root-relative path matches `matcher`. Bounded
/// to the manifest glob; runs only when an active plugin declares manifestEntries.
fn discover_manifest_paths(root: &Path, matcher: &globset::GlobMatcher) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let canonical_root = root.canonicalize().ok();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| entry.file_name() != "node_modules")
        .build();
    for entry in walker.flatten() {
        let Some(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        if file_type.is_symlink()
            && !is_contained_regular_file_symlink(path, canonical_root.as_deref())
        {
            tracing::debug!(
                path = %path.display(),
                "skipping manifest symlink with a broken, non-file, or outside-root target"
            );
            continue;
        }
        if let Some(rel) = root_relative_forward_slash(path, root)
            && matcher.is_match(Path::new(&rel))
        {
            out.push(path.to_path_buf());
        }
    }
    // `ignore::WalkBuilder` yields raw filesystem order; sort so seeding and the
    // check report (manifests_matched, per-manifest warnings) are deterministic
    // across machines and CI runners.
    out.sort();
    out
}

fn is_contained_regular_file_symlink(path: &Path, canonical_root: Option<&Path>) -> bool {
    let Some(root) = canonical_root else {
        return false;
    };
    let Ok(target) = path.canonicalize() else {
        return false;
    };
    target.starts_with(root) && target.metadata().is_ok_and(|metadata| metadata.is_file())
}

/// Root-relative forward-slash string for a discovered (absolute) path, or
/// `None` if it is not under `root`.
fn root_relative_forward_slash(file: &Path, root: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::{EntryPointRole, ManifestFormat, ManifestSeedRule};

    fn json(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    fn seed(path: &str, when: &[(&str, Value)]) -> ManifestSeedRule {
        ManifestSeedRule {
            path: path.to_string(),
            when: when
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
        }
    }

    #[test]
    fn dotted_lookup_traverses_nested_fields() {
        let m = json(r#"{"plugin": {"browser": true, "id": "actions"}}"#);
        assert_eq!(
            dotted_lookup(&m, "plugin.browser"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            dotted_lookup(&m, "plugin.id"),
            Some(&Value::String("actions".into()))
        );
        assert_eq!(dotted_lookup(&m, "plugin.missing"), None);
        assert_eq!(dotted_lookup(&m, "absent.field"), None);
    }

    #[test]
    fn when_matches_is_strict_equality_and_presence_is_not_matched() {
        let m = json(r#"{"type": "plugin", "plugin": {"browser": false}}"#);
        let mut when = BTreeMap::new();
        when.insert("type".to_string(), Value::String("plugin".into()));
        assert!(when_matches(&m, &when));

        // browser is present but false: matching against `true` must FAIL
        // (strict equality, no presence overload).
        let mut when_browser = BTreeMap::new();
        when_browser.insert("plugin.browser".to_string(), Value::Bool(true));
        assert!(!when_matches(&m, &when_browser));

        // empty when always matches
        assert!(when_matches(&m, &BTreeMap::new()));
    }

    #[test]
    fn expand_interpolations_string_array_and_missing() {
        let m = json(r#"{"plugin": {"extraPublicDirs": ["common", "types"], "id": "actions"}}"#);
        // string field -> one entry
        assert_eq!(
            expand_interpolations("${plugin.id}/index.ts", &m),
            vec!["actions/index.ts"]
        );
        // array field -> one entry per element
        assert_eq!(
            expand_interpolations("${plugin.extraPublicDirs}/index.{ts,tsx}", &m),
            vec!["common/index.{ts,tsx}", "types/index.{ts,tsx}"]
        );
        // missing field -> nothing seeded
        assert!(expand_interpolations("${plugin.absent}/index.ts", &m).is_empty());
        // no interpolation -> passthrough
        assert_eq!(
            expand_interpolations("public/index.{ts,tsx}", &m),
            vec!["public/index.{ts,tsx}"]
        );
    }

    #[test]
    fn evaluate_seeds_relative_to_manifest_dir_with_when_and_fanout() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let manifest_dir = root.join("x-pack/plugins/actions");
        std::fs::create_dir_all(&manifest_dir).unwrap();
        let manifest_path = manifest_dir.join("kibana.jsonc");
        std::fs::write(
            &manifest_path,
            r#"{
                // a real Kibana-shaped manifest
                "type": "plugin",
                "plugin": { "browser": true, "server": false, "extraPublicDirs": ["common"] },
            }"#,
        )
        .unwrap();

        let ext = ExternalPluginDef {
            schema: None,
            name: "kibana".to_string(),
            detection: None,
            enablers: vec![],
            entry_points: vec![],
            entry_point_role: EntryPointRole::Runtime,
            manifest_entries: vec![ManifestEntryRule {
                manifests: "**/kibana.jsonc".to_string(),
                format: ManifestFormat::Jsonc,
                when: BTreeMap::from([("type".to_string(), Value::String("plugin".into()))]),
                entries: vec![
                    seed(
                        "public/index.{ts,tsx}",
                        &[("plugin.browser", Value::Bool(true))],
                    ),
                    seed(
                        "server/index.{ts,tsx}",
                        &[("plugin.server", Value::Bool(true))],
                    ),
                    seed("${plugin.extraPublicDirs}/index.{ts,tsx}", &[]),
                ],
            }],
            config_patterns: vec![],
            always_used: vec![],
            tooling_dependencies: vec![],
            used_exports: vec![],
            used_class_members: vec![],
        };

        let rules = evaluate_manifest_entries(&ext, root);
        let paths: Vec<&str> = rules.iter().map(|r| r.pattern.as_str()).collect();

        // browser:true seeds public; server:false does NOT seed server; extraPublicDirs fans out.
        assert!(paths.contains(&"x-pack/plugins/actions/public/index.{ts,tsx}"));
        assert!(paths.contains(&"x-pack/plugins/actions/common/index.{ts,tsx}"));
        assert!(
            !paths.iter().any(|p| p.contains("server/index")),
            "server:false must not seed the server entry, got {paths:?}"
        );
    }

    fn plugin_with(rules: Vec<ManifestEntryRule>) -> ExternalPluginDef {
        ExternalPluginDef {
            schema: None,
            name: "kibana".to_string(),
            detection: None,
            enablers: vec![],
            entry_points: vec![],
            entry_point_role: EntryPointRole::Runtime,
            manifest_entries: rules,
            config_patterns: vec![],
            always_used: vec![],
            tooling_dependencies: vec![],
            used_exports: vec![],
            used_class_members: vec![],
        }
    }

    fn rule(
        manifests: &str,
        when: &[(&str, Value)],
        entries: Vec<ManifestSeedRule>,
    ) -> ManifestEntryRule {
        ManifestEntryRule {
            manifests: manifests.to_string(),
            format: ManifestFormat::Jsonc,
            when: when
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
            entries,
        }
    }

    fn write_manifest(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[cfg(unix)]
    fn symlink_file(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).expect("create file symlink");
    }

    #[cfg(windows)]
    fn symlink_file(target: &Path, link: &Path) {
        std::os::windows::fs::symlink_file(target, link).expect("create file symlink");
    }

    #[test]
    fn manifest_symlinks_must_target_regular_files_inside_root() {
        let dir = tempfile::tempdir().expect("create project");
        let outside = tempfile::tempdir().expect("create outside dir");
        let root = dir.path();
        let targets = root.join("targets");
        let plugins = root.join("plugins");
        std::fs::create_dir_all(&targets).unwrap();
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(targets.join("inside.jsonc"), r#"{"type":"plugin"}"#).unwrap();
        std::fs::write(outside.path().join("outside.jsonc"), r#"{"type":"plugin"}"#).unwrap();

        symlink_file(
            &targets.join("inside.jsonc"),
            &plugins.join("inside-kibana.jsonc"),
        );
        symlink_file(
            &outside.path().join("outside.jsonc"),
            &plugins.join("outside-kibana.jsonc"),
        );
        symlink_file(
            &targets.join("missing.jsonc"),
            &plugins.join("broken-kibana.jsonc"),
        );

        let matcher = globset::Glob::new("**/*-kibana.jsonc")
            .unwrap()
            .compile_matcher();
        let paths = discover_manifest_paths(root, &matcher);
        let relative: Vec<String> = paths
            .iter()
            .filter_map(|path| root_relative_forward_slash(path, root))
            .collect();

        assert_eq!(relative, vec!["plugins/inside-kibana.jsonc"]);
    }

    fn kinds(reports: &[RuleReport]) -> Vec<WarningKind> {
        reports
            .iter()
            .flat_map(|r| r.warnings.iter().map(|w| w.kind))
            .collect()
    }

    #[test]
    fn check_reports_matched_manifests_when_gate_and_seeded_entries() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_manifest(
            root,
            "plugins/alpha/kibana.jsonc",
            r#"{"type":"plugin","plugin":{"browser":true,"server":true}}"#,
        );
        write_manifest(
            root,
            "plugins/beta/kibana.jsonc",
            r#"{"type":"plugin","plugin":{"browser":true,"server":false}}"#,
        );
        let ext = plugin_with(vec![rule(
            "**/kibana.jsonc",
            &[("type", Value::String("plugin".into()))],
            vec![
                seed(
                    "public/index.{ts,tsx}",
                    &[("plugin.browser", Value::Bool(true))],
                ),
                seed(
                    "server/index.{ts,tsx}",
                    &[("plugin.server", Value::Bool(true))],
                ),
            ],
        )]);

        let reports = check_manifest_entries(&ext, root);
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert!(
            report.warnings.is_empty(),
            "clean plugin, got {:?}",
            report.warnings
        );
        // manifests_matched is sorted (agents diff across runs).
        assert_eq!(
            report.manifests_matched,
            vec![
                "plugins/alpha/kibana.jsonc".to_string(),
                "plugins/beta/kibana.jsonc".to_string()
            ]
        );

        let beta = report
            .matched
            .iter()
            .find(|m| m.path == "plugins/beta/kibana.jsonc")
            .expect("beta matched");
        assert!(beta.when_passed);
        assert!(beta.seeded.iter().any(|s| s.contains("beta/public/index")));
        assert!(
            !beta.seeded.iter().any(|s| s.contains("server/index")),
            "beta server:false must not seed the server entry, got {:?}",
            beta.seeded
        );
    }

    #[test]
    fn check_warns_manifests_matched_none() {
        let dir = tempfile::tempdir().unwrap();
        let ext = plugin_with(vec![rule(
            "**/nonexistent.jsonc",
            &[],
            vec![seed("public/index.ts", &[])],
        )]);
        let reports = check_manifest_entries(&ext, dir.path());
        assert!(kinds(&reports).contains(&WarningKind::ManifestsMatchedNone));
        assert_eq!(
            reports[0].warnings[0].glob.as_deref(),
            Some("**/nonexistent.jsonc")
        );
    }

    #[test]
    fn check_warns_field_path_unresolved_on_typo() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_manifest(
            root,
            "plugins/alpha/kibana.jsonc",
            r#"{"type":"plugin","plugin":{"browser":true}}"#,
        );
        let ext = plugin_with(vec![rule(
            "**/kibana.jsonc",
            &[("type", Value::String("plugin".into()))],
            // typo: plugin.extarPublicDirs does not exist
            vec![seed("${plugin.extarPublicDirs}/index.ts", &[])],
        )]);
        let reports = check_manifest_entries(&ext, root);
        let warn = reports[0]
            .warnings
            .iter()
            .find(|w| w.kind == WarningKind::FieldPathUnresolved)
            .expect("field-path-unresolved warning");
        assert_eq!(warn.field_path.as_deref(), Some("plugin.extarPublicDirs"));
    }

    #[test]
    fn check_warns_when_excluded_all() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_manifest(root, "plugins/alpha/kibana.jsonc", r#"{"type":"package"}"#);
        let ext = plugin_with(vec![rule(
            "**/kibana.jsonc",
            &[("type", Value::String("plugin".into()))],
            vec![seed("public/index.ts", &[])],
        )]);
        let reports = check_manifest_entries(&ext, root);
        assert!(kinds(&reports).contains(&WarningKind::WhenExcludedAll));
    }

    #[test]
    fn check_warns_manifest_parse_failed_per_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_manifest(root, "plugins/good/kibana.jsonc", r#"{"type":"plugin"}"#);
        write_manifest(root, "plugins/bad/kibana.jsonc", "{ this is not valid json");
        let ext = plugin_with(vec![rule(
            "**/kibana.jsonc",
            &[("type", Value::String("plugin".into()))],
            vec![seed("public/index.ts", &[])],
        )]);
        let reports = check_manifest_entries(&ext, root);
        let warn = reports[0]
            .warnings
            .iter()
            .find(|w| w.kind == WarningKind::ManifestParseFailed)
            .expect("manifest-parse-failed warning");
        // carries the offending file, not just the glob (agents read the slot).
        assert_eq!(warn.manifest.as_deref(), Some("plugins/bad/kibana.jsonc"));
    }

    #[test]
    fn check_output_is_deterministic_across_walk_order() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Names chosen so raw readdir order is unlikely to be sorted.
        for name in ["mmm", "aaa", "zzz", "ccc"] {
            write_manifest(
                root,
                &format!("plugins/{name}/kibana.jsonc"),
                r#"{"type":"plugin"}"#,
            );
        }
        let ext = plugin_with(vec![rule(
            "**/kibana.jsonc",
            &[("type", Value::String("plugin".into()))],
            // escapes root -> one entry-outside-root warning per manifest.
            vec![seed("../../../../escape/index.ts", &[])],
        )]);
        let reports = check_manifest_entries(&ext, root);
        let r = &reports[0];
        // manifests_matched and the per-file warnings are sorted, not walk order.
        let mut sorted = r.manifests_matched.clone();
        sorted.sort();
        assert_eq!(
            r.manifests_matched, sorted,
            "manifests_matched must be sorted"
        );
        let warn_manifests: Vec<&str> = r
            .warnings
            .iter()
            .filter_map(|w| w.manifest.as_deref())
            .collect();
        let mut sorted_w = warn_manifests.clone();
        sorted_w.sort_unstable();
        assert_eq!(
            warn_manifests, sorted_w,
            "entry-outside-root warnings must be sorted"
        );
    }

    #[test]
    fn check_warns_entry_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write_manifest(root, "plugins/alpha/kibana.jsonc", r#"{"type":"plugin"}"#);
        let ext = plugin_with(vec![rule(
            "**/kibana.jsonc",
            &[("type", Value::String("plugin".into()))],
            // escapes above root from plugins/alpha
            vec![seed("../../../../escape/index.ts", &[])],
        )]);
        let reports = check_manifest_entries(&ext, root);
        let warn = reports[0]
            .warnings
            .iter()
            .find(|w| w.kind == WarningKind::EntryOutsideRoot)
            .expect("entry-outside-root warning");
        assert!(warn.entry.as_deref().is_some_and(|e| e.contains("escape")));
        assert_eq!(warn.manifest.as_deref(), Some("plugins/alpha/kibana.jsonc"));
    }
}
