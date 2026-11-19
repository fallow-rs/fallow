//! Architecture boundary zone and rule definitions.

use std::fmt;
use std::path::{Path, PathBuf};

use super::glob_validation::compile_path_glob;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if predicates receive field references"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Which `BoundaryRule` field carries an unknown zone name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneReferenceKind {
    /// Rule's `from` field names an undefined zone.
    From,
    /// One entry in the rule's `allow` list names an undefined zone.
    Allow,
    /// One entry in the rule's `allowTypeOnly` list names an undefined zone.
    AllowTypeOnly,
    /// A `boundaries.calls.forbidden[]` entry's `from` names an undefined zone.
    CallsFrom,
}

impl ZoneReferenceKind {
    fn config_field(self) -> &'static str {
        match self {
            Self::From | Self::CallsFrom => "from",
            Self::Allow => "allow",
            Self::AllowTypeOnly => "allowTypeOnly",
        }
    }
}

/// One offending zone-name reference in a `boundaries.rules[]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownZoneRef {
    /// Zero-based index into `boundaries.rules[]`.
    pub rule_index: usize,
    /// Which field on the rule carries the unknown name.
    pub kind: ZoneReferenceKind,
    /// The unknown zone name as authored.
    pub zone_name: String,
}

/// One redundant-root-prefix pattern in a `boundaries.zones[]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedundantRootPrefix {
    /// Name of the zone whose pattern redundantly includes its root.
    pub zone_name: String,
    /// The offending pattern as authored.
    pub pattern: String,
    /// The normalized root that the pattern redundantly repeats.
    pub root: String,
}

/// One rejected `boundaries.calls.forbidden[]` callee pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidForbiddenCallee {
    /// Zero-based index into `boundaries.calls.forbidden[]`.
    pub rule_index: usize,
    /// The offending pattern as authored.
    pub pattern: String,
    /// Why the pattern was rejected.
    pub reason: String,
}

/// Validation error from `FallowConfig::validate_resolved_boundaries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneValidationError {
    /// A rule references an undefined zone.
    UnknownZoneReference(UnknownZoneRef),
    /// A zone pattern repeats the zone root.
    RedundantRootPrefix(RedundantRootPrefix),
    /// A forbidden-call entry carries an unusable callee pattern.
    InvalidForbiddenCallee(InvalidForbiddenCallee),
}

impl fmt::Display for ZoneValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownZoneReference(err) if err.kind == ZoneReferenceKind::CallsFrom => {
                write!(
                    f,
                    "boundaries.calls.forbidden[{}].from: references undefined zone '{}'",
                    err.rule_index, err.zone_name,
                )
            }
            Self::UnknownZoneReference(err) => write!(
                f,
                "boundaries.rules[{}].{}: references undefined zone '{}'",
                err.rule_index,
                err.kind.config_field(),
                err.zone_name,
            ),
            Self::InvalidForbiddenCallee(err) => write!(
                f,
                "boundaries.calls.forbidden[{}].callee: pattern '{}' {}",
                err.rule_index, err.pattern, err.reason,
            ),
            Self::RedundantRootPrefix(err) => write!(
                f,
                "FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX: zone '{}': pattern '{}' starts with the zone root '{}'. Patterns are now resolved relative to root; remove the redundant prefix from the pattern.",
                err.zone_name, err.pattern, err.root,
            ),
        }
    }
}

impl std::error::Error for ZoneValidationError {}

/// Built-in architecture presets.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryPreset {
    /// Layered architecture.
    Layered,
    /// Hexagonal / ports-and-adapters.
    Hexagonal,
    /// Feature-Sliced Design.
    FeatureSliced,
    /// Bulletproof React.
    Bulletproof,
}

impl BoundaryPreset {
    /// Every built-in preset, in a stable order.
    ///
    /// A new variant added to the enum forces a compile error in the exhaustive
    /// `name()` and `description()` matches below, which is the reminder to add
    /// it here too so the agent capability manifest never silently omits it.
    #[must_use]
    pub const fn all() -> &'static [BoundaryPreset] {
        &[
            Self::Layered,
            Self::Hexagonal,
            Self::FeatureSliced,
            Self::Bulletproof,
        ]
    }

    /// The kebab-case config value that selects this preset (`boundaries.preset`).
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Layered => "layered",
            Self::Hexagonal => "hexagonal",
            Self::FeatureSliced => "feature-sliced",
            Self::Bulletproof => "bulletproof",
        }
    }

    /// One-line intent, for agent-facing discovery in the capability manifest.
    #[must_use]
    pub const fn description(&self) -> &'static str {
        match self {
            Self::Layered => {
                "Classic layers: presentation depends on application depends on domain; \
                 infrastructure depends on domain and application."
            }
            Self::Hexagonal => {
                "Ports and adapters: adapters depend on ports, ports depend on the domain; \
                 the domain depends on nothing outward."
            }
            Self::FeatureSliced => {
                "Feature-Sliced Design: app, pages, widgets, features, entities, shared; \
                 each layer may only import from the layers below it."
            }
            Self::Bulletproof => {
                "Bulletproof React: app and features depend on shared and server; \
                 features stay isolated from each other."
            }
        }
    }

    /// Expand the preset into default zones and rules.
    #[must_use]
    pub fn default_config(&self, source_root: &str) -> (Vec<BoundaryZone>, Vec<BoundaryRule>) {
        match self {
            Self::Layered => Self::layered_config(source_root),
            Self::Hexagonal => Self::hexagonal_config(source_root),
            Self::FeatureSliced => Self::feature_sliced_config(source_root),
            Self::Bulletproof => Self::bulletproof_config(source_root),
        }
    }

    fn zone(name: &str, source_root: &str) -> BoundaryZone {
        BoundaryZone {
            name: name.to_owned(),
            patterns: vec![format!("{source_root}/{name}/**")],
            auto_discover: vec![],
            root: None,
            public_files: vec![],
        }
    }

    fn rule(from: &str, allow: &[&str]) -> BoundaryRule {
        BoundaryRule {
            from: from.to_owned(),
            allow: allow.iter().map(|s| (*s).to_owned()).collect(),
            allow_type_only: Vec::new(),
        }
    }

    fn layered_config(source_root: &str) -> (Vec<BoundaryZone>, Vec<BoundaryRule>) {
        let zones = vec![
            Self::zone("presentation", source_root),
            Self::zone("application", source_root),
            Self::zone("domain", source_root),
            Self::zone("infrastructure", source_root),
        ];
        let rules = vec![
            Self::rule("presentation", &["application"]),
            Self::rule("application", &["domain"]),
            Self::rule("domain", &[]),
            Self::rule("infrastructure", &["domain", "application"]),
        ];
        (zones, rules)
    }

    fn hexagonal_config(source_root: &str) -> (Vec<BoundaryZone>, Vec<BoundaryRule>) {
        let zones = vec![
            Self::zone("adapters", source_root),
            Self::zone("ports", source_root),
            Self::zone("domain", source_root),
        ];
        let rules = vec![
            Self::rule("adapters", &["ports"]),
            Self::rule("ports", &["domain"]),
            Self::rule("domain", &[]),
        ];
        (zones, rules)
    }

    fn feature_sliced_config(source_root: &str) -> (Vec<BoundaryZone>, Vec<BoundaryRule>) {
        let layer_names = ["app", "pages", "widgets", "features", "entities", "shared"];
        let zones = layer_names
            .iter()
            .map(|name| Self::zone(name, source_root))
            .collect();
        let rules = layer_names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let below: Vec<&str> = layer_names[i + 1..].to_vec();
                Self::rule(name, &below)
            })
            .collect();
        (zones, rules)
    }

    fn bulletproof_config(source_root: &str) -> (Vec<BoundaryZone>, Vec<BoundaryRule>) {
        let zones = vec![
            Self::zone("app", source_root),
            BoundaryZone {
                name: "features".to_owned(),
                patterns: vec![format!("{source_root}/features/**")],
                auto_discover: vec![format!("{source_root}/features")],
                ..BoundaryZone::default()
            },
            BoundaryZone {
                name: "shared".to_owned(),
                patterns: [
                    "components",
                    "hooks",
                    "lib",
                    "utils",
                    "utilities",
                    "providers",
                    "shared",
                    "types",
                    "styles",
                    "i18n",
                ]
                .iter()
                .map(|dir| format!("{source_root}/{dir}/**"))
                .collect(),
                ..BoundaryZone::default()
            },
            Self::zone("server", source_root),
        ];
        let rules = vec![
            Self::rule("app", &["features", "shared", "server"]),
            Self::rule("features", &["shared", "server"]),
            Self::rule("server", &["shared"]),
            Self::rule("shared", &[]),
        ];
        (zones, rules)
    }
}

/// Architecture boundary configuration.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryConfig {
    /// Optional built-in preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<BoundaryPreset>,
    /// Zone definitions.
    #[serde(default)]
    pub zones: Vec<BoundaryZone>,
    /// Zone import rules.
    #[serde(default)]
    pub rules: Vec<BoundaryRule>,
    /// Optional policy for files that match no zone.
    #[serde(default, skip_serializing_if = "BoundaryCoverageConfig::is_default")]
    pub coverage: BoundaryCoverageConfig,
    /// Optional forbidden-call policy for zoned files.
    #[serde(default, skip_serializing_if = "BoundaryCallsConfig::is_default")]
    pub calls: BoundaryCallsConfig,
}

/// Boundary zone coverage policy.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryCoverageConfig {
    /// Report every analyzed source file that does not match any boundary
    /// zone, also a file that no entry point reaches.
    #[serde(default, skip_serializing_if = "is_false")]
    pub require_all_files: bool,
    /// Glob patterns for files that may remain unmatched by any zone.
    /// A single `*` stays within one path segment; use `**` to match across directories.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_unmatched: Vec<String>,
}

impl BoundaryCoverageConfig {
    fn is_default(value: &Self) -> bool {
        !value.require_all_files && value.allow_unmatched.is_empty()
    }
}

/// Boundary forbidden-call policy. Applies only to files classified into a
/// zone; unzoned files are unrestricted, matching the import rules.
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryCallsConfig {
    /// Callee patterns that files in a zone may not call.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forbidden: Vec<ForbiddenCallRule>,
}

impl BoundaryCallsConfig {
    fn is_default(value: &Self) -> bool {
        value.forbidden.is_empty()
    }

    /// Whether no forbidden-call rules are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.forbidden.is_empty()
    }
}

/// One forbidden-call entry: files in zone `from` may not call callees
/// matching `callee`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForbiddenCallRule {
    /// Zone whose files may not make matching calls.
    pub from: String,
    /// Forbidden callee pattern(s). A pattern matches `Date()` and `new Date()`. Matching is
    /// segment-aware, not substring: `child_process.*` matches `child_process.exec` (and named
    /// imports from `child_process` / `node:child_process`), `fetch` matches only `fetch`,
    /// and a leading `*.` suffix-matches any object (`*.innerHTML`).
    pub callee: ForbiddenCallee,
}

/// One callee pattern or a list of patterns for a single `from` zone.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ForbiddenCallee {
    /// A single callee pattern.
    Single(String),
    /// Multiple callee patterns sharing the same `from` zone.
    Many(Vec<String>),
}

impl ForbiddenCallee {
    /// Iterate the configured pattern strings.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        match self {
            Self::Single(pattern) => std::slice::from_ref(pattern),
            Self::Many(patterns) => patterns.as_slice(),
        }
        .iter()
        .map(String::as_str)
    }
}

/// A zone grouping files by directory pattern.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryZone {
    /// Zone name.
    pub name: String,
    /// Membership patterns.
    /// A single `*` stays within one path segment; use `**` to match across directories.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    /// Directories whose children become zones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_discover: Vec<String>,
    /// Optional subtree scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Globs of the public entry files of this zone.
    ///
    /// When the list is not empty, a file of another zone may import a file
    /// of this zone only through a file that is in this zone and matches a
    /// glob. The globs use the same base as `patterns`. On an `autoDiscover`
    /// zone, each glob is relative to each discovered folder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_files: Vec<String>,
}

/// An import rule between zones.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryRule {
    /// Source zone.
    pub from: String,
    /// Allowed target zones.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Allowed type-only targets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_type_only: Vec<String>,
}

/// Resolved boundary config with pre-compiled glob matchers.
#[derive(Debug, Clone, Default)]
pub struct ResolvedBoundaryConfig {
    /// Compiled zones.
    pub zones: Vec<ResolvedZone>,
    /// Compiled rules.
    pub rules: Vec<ResolvedBoundaryRule>,
    /// Captured logical groups.
    pub logical_groups: Vec<LogicalGroup>,
    /// Resolved coverage policy.
    pub coverage: ResolvedBoundaryCoverageConfig,
    /// Forbidden callee patterns grouped by `from` zone, in config order.
    /// Patterns stay raw strings; the analysis layer parses them into its
    /// segment-aware matcher.
    pub calls_forbidden_by_zone: rustc_hash::FxHashMap<String, Vec<String>>,
}

/// Resolved boundary zone coverage policy.
#[derive(Debug, Clone, Default)]
pub struct ResolvedBoundaryCoverageConfig {
    /// Report every analyzed source file that does not match any boundary
    /// zone, also a file that no entry point reaches.
    pub require_all_files: bool,
    /// Compiled allow-list matchers for unmatched files.
    pub allow_unmatched: Vec<globset::GlobMatcher>,
}

/// A user-declared zone that fanned out via `autoDiscover`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LogicalGroup {
    /// Parent zone name.
    pub name: String,
    /// Child zone names.
    pub children: Vec<String>,
    /// Authored `autoDiscover` paths.
    pub auto_discover: Vec<String>,
    /// Authored parent rule, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_rule: Option<AuthoredRule>,
    /// Fallback zone name, if the parent kept patterns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_zone: Option<String>,
    /// Original `zones[]` index.
    pub source_zone_index: usize,
    /// Discovery status.
    pub status: LogicalGroupStatus,
    /// Merged duplicate parent indices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<Vec<usize>>,
    /// Authored parent root, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_zone_root: Option<String>,
    /// Child-to-source indexes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_source_indices: Vec<usize>,
}

/// Discovery outcome for a [`LogicalGroup`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogicalGroupStatus {
    /// Children were discovered.
    Ok,
    /// Paths were readable but empty.
    Empty,
    /// A path was invalid or unreadable.
    InvalidPath,
}

/// Pre-expansion rule preserved on a [`LogicalGroup`].
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AuthoredRule {
    /// Authored `allow` list.
    pub allow: Vec<String>,
    /// Authored `allowTypeOnly` list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_type_only: Vec<String>,
}

/// A zone with pre-compiled glob matchers.
#[derive(Debug, Clone)]
pub struct ResolvedZone {
    /// Zone name.
    pub name: String,
    /// Authored membership patterns.
    pub patterns: Vec<String>,
    /// Compiled matchers.
    pub matchers: Vec<globset::GlobMatcher>,
    /// Normalized subtree scope.
    pub root: Option<String>,
    /// Authored public entry globs, after the `autoDiscover` expansion.
    pub public_files: Vec<String>,
    /// Compiled public entry matchers.
    pub public_file_matchers: Vec<globset::GlobMatcher>,
}

impl ResolvedZone {
    /// The candidate for a project-relative path, scoped to the zone root.
    /// `None` when the path is outside the root.
    fn candidate<'p>(&self, relative_path: &'p str) -> Option<globset::Candidate<'p>> {
        match self.root.as_deref() {
            Some(root) if !root.is_empty() => relative_path
                .strip_prefix(root)
                .map(globset::Candidate::new),
            _ => Some(globset::Candidate::new(relative_path)),
        }
    }
}

/// A resolved boundary rule.
#[derive(Debug, Clone)]
pub struct ResolvedBoundaryRule {
    /// Source zone.
    pub from_zone: String,
    /// Allowed imports.
    pub allowed_zones: Vec<String>,
    /// Allowed type-only imports.
    pub allow_type_only_zones: Vec<String>,
}

impl BoundaryConfig {
    /// Whether any boundaries are configured (including via preset).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.preset.is_none()
            && self.zones.is_empty()
            && !self.coverage.require_all_files
            && self.calls.is_empty()
    }

    /// Expand the preset into explicit zones and rules.
    pub fn expand(&mut self, source_root: &str) {
        let Some(preset) = self.preset.take() else {
            return;
        };

        let (preset_zones, preset_rules) = preset.default_config(source_root);

        let user_zone_names: rustc_hash::FxHashSet<&str> =
            self.zones.iter().map(|z| z.name.as_str()).collect();

        let mut merged_zones: Vec<BoundaryZone> = preset_zones
            .into_iter()
            .filter(|pz| {
                if user_zone_names.contains(pz.name.as_str()) {
                    tracing::info!(
                        "boundary preset: user zone '{}' replaces preset zone",
                        pz.name
                    );
                    false
                } else {
                    true
                }
            })
            .collect();
        merged_zones.append(&mut self.zones);
        self.zones = merged_zones;

        let user_rule_sources: rustc_hash::FxHashSet<&str> =
            self.rules.iter().map(|r| r.from.as_str()).collect();

        let mut merged_rules: Vec<BoundaryRule> = preset_rules
            .into_iter()
            .filter(|pr| {
                if user_rule_sources.contains(pr.from.as_str()) {
                    tracing::info!(
                        "boundary preset: user rule for '{}' replaces preset rule",
                        pr.from
                    );
                    false
                } else {
                    true
                }
            })
            .collect();
        merged_rules.append(&mut self.rules);
        self.rules = merged_rules;
    }

    /// The directories whose child directories `autoDiscover` turns into
    /// zones, as absolute paths under `project_root`, after the preset
    /// expansion. A new or removed child directory changes the zones.
    /// Invalid `autoDiscover` paths are left out.
    #[must_use]
    pub fn auto_discover_dirs(&self, project_root: &Path) -> Vec<PathBuf> {
        let mut expanded = self.clone();
        super::resolution::expand_boundary_preset(&mut expanded, project_root);
        let mut dirs: Vec<PathBuf> = expanded
            .zones
            .iter()
            .flat_map(|zone| {
                let zone_root = zone
                    .root
                    .as_deref()
                    .map(normalize_zone_root)
                    .unwrap_or_default();
                zone.auto_discover.iter().filter_map(move |raw_dir| {
                    let dir = normalize_auto_discover_dir(raw_dir)?;
                    let relative = join_relative_path(&zone_root, &dir);
                    Some(if relative.is_empty() {
                        project_root.to_path_buf()
                    } else {
                        project_root.join(relative)
                    })
                })
            })
            .collect();
        dirs.sort();
        dirs.dedup();
        dirs
    }

    /// Expand `autoDiscover` zones into concrete child zones.
    pub fn expand_auto_discover(&mut self, project_root: &Path) -> Vec<LogicalGroup> {
        if self.zones.iter().all(|zone| zone.auto_discover.is_empty()) {
            return Vec::new();
        }

        let original_zones = std::mem::take(&mut self.zones);
        let mut expanded_zones = Vec::new();
        let mut group_expansions: rustc_hash::FxHashMap<String, Vec<String>> =
            rustc_hash::FxHashMap::default();
        let mut group_drafts: Vec<LogicalGroupDraft> = Vec::new();

        for (source_zone_index, zone) in original_zones.into_iter().enumerate() {
            if zone.auto_discover.is_empty() {
                expanded_zones.push(zone);
                continue;
            }

            let expansion = expand_auto_discover_zone(
                project_root,
                zone,
                source_zone_index,
                &mut expanded_zones,
            );
            if !expansion.expanded_names.is_empty() {
                group_expansions
                    .entry(expansion.group_name.clone())
                    .or_default()
                    .extend(expansion.expanded_names);
            }
            merge_logical_group_draft(&mut group_drafts, expansion.draft);
        }

        self.zones = expanded_zones;

        let original_rules = std::mem::take(&mut self.rules);
        let logical_groups = build_logical_groups_from_drafts(group_drafts, &original_rules);

        if group_expansions.is_empty() {
            self.rules = original_rules;
            return logical_groups;
        }

        self.rules = expand_rules_for_groups(original_rules, &group_expansions);
        logical_groups
    }
}

struct AutoDiscoverExpansion {
    group_name: String,
    expanded_names: Vec<String>,
    draft: LogicalGroupDraft,
}

fn expand_auto_discover_zone(
    project_root: &Path,
    mut zone: BoundaryZone,
    source_zone_index: usize,
    expanded_zones: &mut Vec<BoundaryZone>,
) -> AutoDiscoverExpansion {
    let group_name = zone.name.clone();
    let raw_auto_discover = zone.auto_discover.clone();
    let original_zone_root = zone.root.clone();
    let DiscoveryOutcome {
        zones: discovered_zones,
        source_indices: discovered_source_indices,
        had_invalid_path,
    } = discover_child_zones(project_root, &zone);
    let status = discovery_status(discovered_zones.len(), had_invalid_path);
    let mut expanded_names: Vec<String> = discovered_zones
        .iter()
        .map(|child| child.name.clone())
        .collect();
    let child_names_only = expanded_names.clone();

    for child_zone in discovered_zones {
        merge_zone_by_name(expanded_zones, child_zone);
    }

    let fallback_zone = merge_fallback_auto_discover_zone(&mut zone, &group_name, expanded_zones);
    if fallback_zone.is_some() {
        expanded_names.push(group_name.clone());
    }

    AutoDiscoverExpansion {
        group_name: group_name.clone(),
        expanded_names,
        draft: LogicalGroupDraft {
            name: group_name,
            children: child_names_only,
            auto_discover: raw_auto_discover,
            fallback_zone,
            source_zone_index,
            status,
            merged_from: None,
            original_zone_root,
            child_source_indices: discovered_source_indices,
        },
    }
}

const fn discovery_status(discovered_count: usize, had_invalid_path: bool) -> LogicalGroupStatus {
    if discovered_count > 0 {
        LogicalGroupStatus::Ok
    } else if had_invalid_path {
        LogicalGroupStatus::InvalidPath
    } else {
        LogicalGroupStatus::Empty
    }
}

fn merge_fallback_auto_discover_zone(
    zone: &mut BoundaryZone,
    group_name: &str,
    expanded_zones: &mut Vec<BoundaryZone>,
) -> Option<String> {
    if zone.patterns.is_empty() {
        return None;
    }
    zone.auto_discover.clear();
    // The public globs are relative to each discovered folder, so they do
    // not apply to the files that only the parent patterns match.
    zone.public_files.clear();
    merge_zone_by_name(expanded_zones, zone.clone());
    Some(group_name.to_owned())
}

fn merge_logical_group_draft(group_drafts: &mut Vec<LogicalGroupDraft>, draft: LogicalGroupDraft) {
    let Some(existing) = group_drafts.iter_mut().find(|d| d.name == draft.name) else {
        group_drafts.push(draft);
        return;
    };

    tracing::warn!(
        "boundary zone '{}' is declared multiple times with autoDiscover; merging discovered children",
        draft.name
    );
    let auto_discover_offset = existing.auto_discover.len();
    merge_logical_group_children(existing, &draft, auto_discover_offset);
    existing.auto_discover.extend(draft.auto_discover);
    if existing.fallback_zone.is_none() {
        existing.fallback_zone = draft.fallback_zone;
    }
    existing.status = merge_status(existing.status, draft.status);
    let chain = existing
        .merged_from
        .get_or_insert_with(|| vec![existing.source_zone_index]);
    chain.push(draft.source_zone_index);
}

fn merge_logical_group_children(
    existing: &mut LogicalGroupDraft,
    draft: &LogicalGroupDraft,
    auto_discover_offset: usize,
) {
    let existing_children: rustc_hash::FxHashSet<String> =
        existing.children.iter().cloned().collect();
    for (idx, name) in draft.children.iter().enumerate() {
        if existing_children.contains(name) {
            continue;
        }
        existing.children.push(name.clone());
        existing
            .child_source_indices
            .push(draft.child_source_indices[idx] + auto_discover_offset);
    }
}

fn build_logical_groups_from_drafts(
    group_drafts: Vec<LogicalGroupDraft>,
    original_rules: &[BoundaryRule],
) -> Vec<LogicalGroup> {
    let authored_rules = authored_rules_for_logical_groups(&group_drafts, original_rules);

    group_drafts
        .into_iter()
        .map(|draft| {
            let child_source_indices = if draft.auto_discover.len() > 1 {
                draft.child_source_indices
            } else {
                Vec::new()
            };
            LogicalGroup {
                authored_rule: authored_rules.get(draft.name.as_str()).cloned(),
                name: draft.name,
                children: draft.children,
                auto_discover: draft.auto_discover,
                fallback_zone: draft.fallback_zone,
                source_zone_index: draft.source_zone_index,
                status: draft.status,
                merged_from: draft.merged_from,
                original_zone_root: draft.original_zone_root,
                child_source_indices,
            }
        })
        .collect()
}

fn authored_rules_for_logical_groups<'a>(
    group_drafts: &[LogicalGroupDraft],
    original_rules: &'a [BoundaryRule],
) -> rustc_hash::FxHashMap<&'a str, AuthoredRule> {
    let draft_names: rustc_hash::FxHashSet<&str> =
        group_drafts.iter().map(|d| d.name.as_str()).collect();
    original_rules
        .iter()
        .filter(|rule| draft_names.contains(rule.from.as_str()))
        .map(|rule| {
            (
                rule.from.as_str(),
                AuthoredRule {
                    allow: rule.allow.clone(),
                    allow_type_only: rule.allow_type_only.clone(),
                },
            )
        })
        .collect()
}

/// Merge a discovered zone into `zones[]` by name.
fn merge_zone_by_name(expanded_zones: &mut Vec<BoundaryZone>, zone: BoundaryZone) {
    if let Some(existing) = expanded_zones.iter_mut().find(|z| z.name == zone.name) {
        for glob in zone.public_files {
            if !existing.public_files.contains(&glob) {
                existing.public_files.push(glob);
            }
        }
        for pattern in zone.patterns {
            if !existing.patterns.contains(&pattern) {
                existing.patterns.push(pattern);
            }
        }
    } else {
        expanded_zones.push(zone);
    }
}

/// Expand rules across discovered child groups.
fn expand_rules_for_groups(
    original_rules: Vec<BoundaryRule>,
    group_expansions: &rustc_hash::FxHashMap<String, Vec<String>>,
) -> Vec<BoundaryRule> {
    let mut generated_rules = Vec::new();
    let mut explicit_rules = Vec::new();
    for rule in original_rules {
        let allow = expand_rule_allow(&rule.allow, group_expansions);
        let allow_type_only = expand_rule_allow(&rule.allow_type_only, group_expansions);

        if let Some(from_zones) = group_expansions.get(&rule.from) {
            for from in from_zones {
                let (allow, allow_type_only) = if from == &rule.from {
                    (
                        expand_parent_fallback_allow(&allow, from_zones, &rule.from),
                        allow_type_only.clone(),
                    )
                } else {
                    (
                        expand_generated_child_allow(&rule.allow, group_expansions, &rule.from),
                        expand_generated_child_allow(
                            &rule.allow_type_only,
                            group_expansions,
                            &rule.from,
                        ),
                    )
                };
                let expanded_rule = BoundaryRule {
                    from: from.clone(),
                    allow,
                    allow_type_only,
                };
                if from == &rule.from {
                    explicit_rules.push(expanded_rule);
                } else {
                    generated_rules.push(expanded_rule);
                }
            }
        } else {
            explicit_rules.push(BoundaryRule {
                from: rule.from,
                allow,
                allow_type_only,
            });
        }
    }

    let mut expanded_rules = dedupe_rules_keep_last(generated_rules);
    expanded_rules.extend(dedupe_rules_keep_last(explicit_rules));
    dedupe_rules_keep_last(expanded_rules)
}

impl BoundaryConfig {
    /// Validate that patterns and `publicFiles` globs do not repeat the zone root.
    #[must_use]
    pub fn validate_root_prefixes(&self) -> Vec<RedundantRootPrefix> {
        let mut errors = Vec::new();
        for zone in &self.zones {
            let Some(raw_root) = zone.root.as_deref() else {
                continue;
            };
            let normalized = normalize_zone_root(raw_root);
            if normalized.is_empty() {
                continue;
            }
            for pattern in zone.patterns.iter().chain(&zone.public_files) {
                let normalized_pattern = pattern.replace('\\', "/");
                let stripped = normalized_pattern
                    .strip_prefix("./")
                    .unwrap_or(&normalized_pattern);
                if stripped.starts_with(&normalized) {
                    errors.push(RedundantRootPrefix {
                        zone_name: zone.name.clone(),
                        pattern: pattern.clone(),
                        root: normalized.clone(),
                    });
                }
            }
        }
        errors
    }

    /// Validate that every zone reference points at a defined zone.
    #[must_use]
    pub fn validate_zone_references(&self) -> Vec<UnknownZoneRef> {
        let zone_names: rustc_hash::FxHashSet<&str> =
            self.zones.iter().map(|z| z.name.as_str()).collect();

        let mut errors = Vec::new();
        for (i, rule) in self.rules.iter().enumerate() {
            if !zone_names.contains(rule.from.as_str()) {
                errors.push(UnknownZoneRef {
                    rule_index: i,
                    kind: ZoneReferenceKind::From,
                    zone_name: rule.from.clone(),
                });
            }
            for allowed in &rule.allow {
                if !zone_names.contains(allowed.as_str()) {
                    errors.push(UnknownZoneRef {
                        rule_index: i,
                        kind: ZoneReferenceKind::Allow,
                        zone_name: allowed.clone(),
                    });
                }
            }
            for allowed_type_only in &rule.allow_type_only {
                if !zone_names.contains(allowed_type_only.as_str()) {
                    errors.push(UnknownZoneRef {
                        rule_index: i,
                        kind: ZoneReferenceKind::AllowTypeOnly,
                        zone_name: allowed_type_only.clone(),
                    });
                }
            }
        }
        for (i, rule) in self.calls.forbidden.iter().enumerate() {
            if !zone_names.contains(rule.from.as_str()) {
                errors.push(UnknownZoneRef {
                    rule_index: i,
                    kind: ZoneReferenceKind::CallsFrom,
                    zone_name: rule.from.clone(),
                });
            }
        }
        errors
    }

    /// Validate `boundaries.calls.forbidden[]` callee patterns. Rejects
    /// patterns that would parse but silently match nothing (empty or
    /// whitespace-only patterns, a bare `*` with no callee segments, empty
    /// dot-segments) and entries with an empty pattern list, so an inert rule
    /// fails loudly at load time instead of reporting zero findings forever.
    #[must_use]
    pub fn validate_call_rules(&self) -> Vec<InvalidForbiddenCallee> {
        let mut errors = Vec::new();
        for (i, rule) in self.calls.forbidden.iter().enumerate() {
            if rule.callee.iter().next().is_none() {
                errors.push(InvalidForbiddenCallee {
                    rule_index: i,
                    pattern: String::new(),
                    reason: "must list at least one callee pattern".to_owned(),
                });
                continue;
            }
            for pattern in rule.callee.iter() {
                let trimmed = pattern.trim();
                if trimmed.is_empty() {
                    errors.push(InvalidForbiddenCallee {
                        rule_index: i,
                        pattern: pattern.to_owned(),
                        reason: "must not be empty".to_owned(),
                    });
                } else if trimmed == "*" {
                    errors.push(InvalidForbiddenCallee {
                        rule_index: i,
                        pattern: pattern.to_owned(),
                        reason: "matches nothing: a bare `*` has no callee segments. Name a \
                                 specific callee such as `console.*` or `child_process.exec`"
                            .to_owned(),
                    });
                } else if trimmed.split('.').any(|segment| segment.trim().is_empty()) {
                    errors.push(InvalidForbiddenCallee {
                        rule_index: i,
                        pattern: pattern.to_owned(),
                        reason: "contains an empty path segment".to_owned(),
                    });
                } else if let Some(reason) = wildcard_placement_error(trimmed) {
                    errors.push(InvalidForbiddenCallee {
                        rule_index: i,
                        pattern: pattern.to_owned(),
                        reason,
                    });
                }
            }
        }
        errors
    }

    /// Resolve into compiled form with pre-built glob matchers.
    #[must_use]
    pub fn resolve(&self) -> ResolvedBoundaryConfig {
        let rules = self
            .rules
            .iter()
            .map(|rule| ResolvedBoundaryRule {
                from_zone: rule.from.clone(),
                allowed_zones: rule.allow.clone(),
                allow_type_only_zones: rule.allow_type_only.clone(),
            })
            .collect();

        ResolvedBoundaryConfig {
            zones: self.resolve_zones(),
            rules,
            logical_groups: Vec::new(),
            coverage: self.resolve_coverage(),
            calls_forbidden_by_zone: self.resolve_calls_forbidden_by_zone(),
        }
    }

    /// Compile each zone's membership patterns into glob matchers.
    #[expect(
        clippy::expect_used,
        reason = "boundary glob patterns are validated before config resolution"
    )]
    fn resolve_zones(&self) -> Vec<ResolvedZone> {
        self.zones
            .iter()
            .map(|zone| {
                let matchers = zone
                    .patterns
                    .iter()
                    .map(|pattern| {
                        compile_path_glob(pattern)
                            .expect("boundaries.zones[].patterns was validated at config load time")
                            .compile_matcher()
                    })
                    .collect();
                let public_file_matchers = zone
                    .public_files
                    .iter()
                    .map(|pattern| {
                        compile_path_glob(pattern)
                            .expect(
                                "boundaries.zones[].publicFiles was validated at config load time",
                            )
                            .compile_matcher()
                    })
                    .collect();
                let root = zone.root.as_deref().map(normalize_zone_root);
                ResolvedZone {
                    name: zone.name.clone(),
                    patterns: zone.patterns.clone(),
                    matchers,
                    root,
                    public_files: zone.public_files.clone(),
                    public_file_matchers,
                }
            })
            .collect()
    }

    /// Compile the coverage `allowUnmatched` patterns into glob matchers.
    #[expect(
        clippy::expect_used,
        reason = "boundary glob patterns are validated before config resolution"
    )]
    fn resolve_coverage(&self) -> ResolvedBoundaryCoverageConfig {
        ResolvedBoundaryCoverageConfig {
            require_all_files: self.coverage.require_all_files,
            allow_unmatched: self
                .coverage
                .allow_unmatched
                .iter()
                .map(|pattern| {
                    compile_path_glob(pattern)
                        .expect(
                            "boundaries.coverage.allowUnmatched was validated at config load time",
                        )
                        .compile_matcher()
                })
                .collect(),
        }
    }

    /// Group trimmed forbidden-call patterns by their `from` zone, in config order.
    fn resolve_calls_forbidden_by_zone(&self) -> rustc_hash::FxHashMap<String, Vec<String>> {
        let mut calls_forbidden_by_zone: rustc_hash::FxHashMap<String, Vec<String>> =
            rustc_hash::FxHashMap::default();
        for rule in &self.calls.forbidden {
            let patterns = calls_forbidden_by_zone
                .entry(rule.from.clone())
                .or_default();
            for pattern in rule.callee.iter() {
                patterns.push(pattern.trim().to_owned());
            }
        }
        calls_forbidden_by_zone
    }
}

/// Reject `*` placements the segment-aware callee matcher cannot honor.
/// Callee patterns are not globs: `*` must be a whole segment, and only the
/// leading object position (`*.member`) or the trailing member position
/// (`object.*`) is supported, never both and never mid-path.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the parent module is glob re-exported from lib.rs, so `pub` would leak this helper into the public API; pub(crate) is the minimal widening for the rule-pack validator"
)]
pub(crate) fn wildcard_placement_error(pattern: &str) -> Option<String> {
    let segments: Vec<&str> = pattern.split('.').collect();
    let last = segments.len() - 1;
    if segments
        .iter()
        .any(|segment| segment.contains('*') && *segment != "*")
    {
        return Some(
            "uses `*` inside a segment; callee patterns are not globs, so `*` must be a \
             whole segment (`*.member` or `object.*`)"
                .to_owned(),
        );
    }
    let star_positions: Vec<usize> = segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| **segment == "*")
        .map(|(i, _)| i)
        .collect();
    if star_positions.len() > 1 || star_positions.iter().any(|&i| i != 0 && i != last) {
        return Some(
            "may use `*` only as the leading object segment (`*.member`) or the trailing \
             member segment (`object.*`), not both and not mid-path"
                .to_owned(),
        );
    }
    None
}

/// Normalize a zone root for classification.
fn normalize_zone_root(raw: &str) -> String {
    let with_slashes = raw.replace('\\', "/");
    let trimmed = with_slashes.trim_start_matches("./");
    let no_dot = if trimmed == "." { "" } else { trimmed };
    if no_dot.is_empty() {
        String::new()
    } else if no_dot.ends_with('/') {
        no_dot.to_owned()
    } else {
        format!("{no_dot}/")
    }
}

fn normalize_auto_discover_dir(raw: &str) -> Option<String> {
    let with_slashes = raw.replace('\\', "/");
    let trimmed = with_slashes.trim_start_matches("./").trim_end_matches('/');
    if trimmed.starts_with('/') || trimmed.split('/').any(|part| part == "..") {
        None
    } else if trimmed == "." {
        Some(String::new())
    } else {
        Some(trimmed.to_owned())
    }
}

fn join_relative_path(prefix: &str, suffix: &str) -> String {
    match (prefix.is_empty(), suffix.is_empty()) {
        (true, true) => String::new(),
        (true, false) => suffix.to_owned(),
        (false, true) => prefix.trim_end_matches('/').to_owned(),
        (false, false) => format!("{}/{}", prefix.trim_end_matches('/'), suffix),
    }
}

/// Discovery result for one auto-discover zone.
struct DiscoveryOutcome {
    zones: Vec<BoundaryZone>,
    source_indices: Vec<usize>,
    had_invalid_path: bool,
}

/// Intermediate accumulator for a [`LogicalGroup`].
struct LogicalGroupDraft {
    name: String,
    children: Vec<String>,
    auto_discover: Vec<String>,
    fallback_zone: Option<String>,
    source_zone_index: usize,
    status: LogicalGroupStatus,
    /// Merged duplicate declarations.
    merged_from: Option<Vec<usize>>,
    /// Authored parent root.
    original_zone_root: Option<String>,
    /// Child-to-source index mapping.
    child_source_indices: Vec<usize>,
}

/// Merge duplicate `LogicalGroupStatus` values.
const fn merge_status(existing: LogicalGroupStatus, new: LogicalGroupStatus) -> LogicalGroupStatus {
    match (existing, new) {
        (LogicalGroupStatus::Ok, _) | (_, LogicalGroupStatus::Ok) => LogicalGroupStatus::Ok,
        (LogicalGroupStatus::InvalidPath, _) | (_, LogicalGroupStatus::InvalidPath) => {
            LogicalGroupStatus::InvalidPath
        }
        (LogicalGroupStatus::Empty, LogicalGroupStatus::Empty) => LogicalGroupStatus::Empty,
    }
}

/// Accumulator for child zones discovered across a zone's autoDiscover dirs.
#[derive(Default)]
struct ChildZoneAccumulator {
    zones_by_name: rustc_hash::FxHashMap<String, BoundaryZone>,
    first_source_index: rustc_hash::FxHashMap<String, usize>,
}

impl ChildZoneAccumulator {
    /// Register one discovered child directory as a child zone, merging the
    /// glob pattern into an existing entry and recording its first source index.
    fn register_child(
        &mut self,
        zone: &BoundaryZone,
        discover_dir: &str,
        child_name: &str,
        source_index: usize,
    ) {
        let zone_name = format!("{}/{}", zone.name, child_name);
        let child_dir = join_relative_path(discover_dir, child_name);
        let child_pattern = format!("{child_dir}/**");
        let entry = self
            .zones_by_name
            .entry(zone_name.clone())
            .or_insert_with(|| BoundaryZone {
                name: zone_name.clone(),
                root: zone.root.clone(),
                ..BoundaryZone::default()
            });
        if !entry
            .patterns
            .iter()
            .any(|pattern| pattern == &child_pattern)
        {
            entry.patterns.push(child_pattern);
        }
        for glob in &zone.public_files {
            let child_glob = join_relative_path(&child_dir, glob.trim_start_matches("./"));
            if !entry.public_files.contains(&child_glob) {
                entry.public_files.push(child_glob);
            }
        }
        self.first_source_index
            .entry(zone_name)
            .or_insert(source_index);
    }
}

/// Read one normalized autoDiscover directory and register its immediate child
/// directories as child zones. Returns `false` when the path was invalid or
/// unreadable so the caller can flag the discovery as invalid.
fn discover_child_zones_in_dir(
    project_root: &Path,
    zone: &BoundaryZone,
    normalized_root: &str,
    raw_dir: &str,
    source_index: usize,
    accumulator: &mut ChildZoneAccumulator,
) -> bool {
    let Some(discover_dir) = normalize_auto_discover_dir(raw_dir) else {
        tracing::warn!(
            "invalid boundary autoDiscover path '{}' in zone '{}': paths must be project-relative and must not contain '..'",
            raw_dir,
            zone.name
        );
        return false;
    };

    let fs_relative = join_relative_path(normalized_root, &discover_dir);
    let absolute_dir = if fs_relative.is_empty() {
        project_root.to_path_buf()
    } else {
        project_root.join(&fs_relative)
    };
    let Ok(entries) = std::fs::read_dir(&absolute_dir) else {
        tracing::warn!(
            "boundary zone '{}' autoDiscover path '{}' did not resolve to a readable directory",
            zone.name,
            raw_dir
        );
        return false;
    };

    let mut children: Vec<_> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
        .collect();
    children.sort_by_key(std::fs::DirEntry::file_name);

    for child in children {
        let child_name = child.file_name().to_string_lossy().to_string();
        if child_name.is_empty() {
            continue;
        }
        accumulator.register_child(zone, &discover_dir, &child_name, source_index);
    }
    true
}

fn discover_child_zones(project_root: &Path, zone: &BoundaryZone) -> DiscoveryOutcome {
    let mut accumulator = ChildZoneAccumulator::default();
    let normalized_root = zone
        .root
        .as_deref()
        .map(normalize_zone_root)
        .unwrap_or_default();
    let mut had_invalid_path = false;

    for (source_index, raw_dir) in zone.auto_discover.iter().enumerate() {
        if !discover_child_zones_in_dir(
            project_root,
            zone,
            &normalized_root,
            raw_dir,
            source_index,
            &mut accumulator,
        ) {
            had_invalid_path = true;
        }
    }

    let mut zones: Vec<_> = accumulator.zones_by_name.into_values().collect();
    zones.sort_by(|a, b| a.name.cmp(&b.name));
    let source_indices: Vec<usize> = zones
        .iter()
        .map(|z| {
            accumulator
                .first_source_index
                .get(z.name.as_str())
                .copied()
                .unwrap_or(0)
        })
        .collect();
    DiscoveryOutcome {
        zones,
        source_indices,
        had_invalid_path,
    }
}

fn expand_rule_allow(
    allow: &[String],
    group_expansions: &rustc_hash::FxHashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut expanded = Vec::new();
    for zone in allow {
        if let Some(expansion) = group_expansions.get(zone) {
            expanded.extend(expansion.iter().cloned());
        } else {
            expanded.push(zone.clone());
        }
    }
    dedupe_preserving_order(expanded)
}

fn expand_parent_fallback_allow(
    allow: &[String],
    from_zones: &[String],
    parent_name: &str,
) -> Vec<String> {
    let mut expanded = allow.to_vec();
    expanded.extend(
        from_zones
            .iter()
            .filter(|from_zone| from_zone.as_str() != parent_name)
            .cloned(),
    );
    dedupe_preserving_order(expanded)
}

fn expand_generated_child_allow(
    allow: &[String],
    group_expansions: &rustc_hash::FxHashMap<String, Vec<String>>,
    source_group: &str,
) -> Vec<String> {
    let mut expanded = Vec::new();
    for zone in allow {
        if zone == source_group {
            if group_expansions
                .get(source_group)
                .is_some_and(|from_zones| from_zones.iter().any(|from_zone| from_zone == zone))
            {
                expanded.push(zone.clone());
            }
        } else if let Some(expansion) = group_expansions.get(zone) {
            expanded.extend(expansion.iter().cloned());
        } else {
            expanded.push(zone.clone());
        }
    }
    dedupe_preserving_order(expanded)
}

fn dedupe_preserving_order(values: Vec<String>) -> Vec<String> {
    let mut seen = rustc_hash::FxHashSet::default();
    values
        .into_iter()
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn dedupe_rules_keep_last(rules: Vec<BoundaryRule>) -> Vec<BoundaryRule> {
    let mut seen = rustc_hash::FxHashSet::default();
    let mut deduped: Vec<_> = rules
        .into_iter()
        .rev()
        .filter(|rule| seen.insert(rule.from.clone()))
        .collect();
    deduped.reverse();
    deduped
}

impl ResolvedBoundaryConfig {
    /// Whether any boundaries are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.zones.is_empty()
            && self.logical_groups.is_empty()
            && !self.coverage.require_all_files
            && self.calls_forbidden_by_zone.is_empty()
    }

    /// Classify a project-relative path into a zone.
    #[must_use]
    pub fn classify_zone(&self, relative_path: &str) -> Option<&str> {
        // A candidate normalizes its path once. Unscoped zones share one
        // candidate, so a path is not normalized again for every glob.
        let unscoped = globset::Candidate::new(relative_path);
        for zone in &self.zones {
            let scoped;
            let candidate = match zone.root.as_deref() {
                Some(root) if !root.is_empty() => {
                    let Some(stripped) = zone.candidate(relative_path) else {
                        continue;
                    };
                    scoped = stripped;
                    &scoped
                }
                _ => &unscoped,
            };
            if zone
                .matchers
                .iter()
                .any(|m| m.is_match_candidate(candidate))
            {
                return Some(&zone.name);
            }
        }
        None
    }

    /// Whether an unmatched file is explicitly allowed by coverage policy.
    #[must_use]
    pub fn allows_unmatched(&self, relative_path: &str) -> bool {
        self.coverage
            .allow_unmatched
            .iter()
            .any(|matcher| matcher.is_match(relative_path))
    }

    /// Check whether an import is allowed.
    #[must_use]
    pub fn is_import_allowed(&self, from_zone: &str, to_zone: &str) -> bool {
        if from_zone == to_zone {
            return true;
        }

        let rule = self.rules.iter().find(|r| r.from_zone == from_zone);

        match rule {
            None => true,
            Some(r) => r.allowed_zones.iter().any(|z| z == to_zone),
        }
    }

    /// Whether the zone named `zone_name` declares public entry files
    /// (`publicFiles`). Other zones may then import only those files.
    #[must_use]
    pub fn has_public_files(&self, zone_name: &str) -> bool {
        self.zones
            .iter()
            .any(|zone| zone.name == zone_name && !zone.public_file_matchers.is_empty())
    }

    /// Whether the project-relative path matches a `publicFiles` glob of the
    /// zone named `zone_name`. The caller checks that the file is in the zone.
    #[must_use]
    pub fn is_public_file(&self, zone_name: &str, relative_path: &str) -> bool {
        self.zones
            .iter()
            .filter(|zone| zone.name == zone_name)
            .any(|zone| {
                zone.candidate(relative_path).is_some_and(|candidate| {
                    zone.public_file_matchers
                        .iter()
                        .any(|m| m.is_match_candidate(&candidate))
                })
            })
    }

    /// Check whether a type-only import is allowed.
    #[must_use]
    pub fn is_type_only_allowed(&self, from_zone: &str, to_zone: &str) -> bool {
        let Some(rule) = self.rules.iter().find(|r| r.from_zone == from_zone) else {
            return false;
        };
        rule.allow_type_only_zones.iter().any(|z| z == to_zone)
    }
}

#[cfg(test)]
#[path = "boundaries_tests.rs"]
mod tests;
