//! Architecture boundary zone and rule definitions.

use std::fmt;
use std::path::Path;

use globset::Glob;
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
                root: None,
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
                auto_discover: vec![],
                root: None,
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
    /// Report source files that do not match any boundary zone.
    #[serde(default, skip_serializing_if = "is_false")]
    pub require_all_files: bool,
    /// Glob patterns for files that may remain unmatched by any zone.
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
    /// Forbidden callee pattern(s). Matching is segment-aware, not substring:
    /// `child_process.*` matches `child_process.exec` (and named imports from
    /// `child_process` / `node:child_process`), `fetch` matches only `fetch`,
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
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryZone {
    /// Zone name.
    pub name: String,
    /// Membership patterns.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    /// Directories whose children become zones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_discover: Vec<String>,
    /// Optional subtree scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
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
    /// Report source files that do not match any boundary zone.
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
    /// Return the preset name if one is configured but not yet expanded.
    #[must_use]
    pub fn preset_name(&self) -> Option<&str> {
        self.preset.as_ref().map(|p| match p {
            BoundaryPreset::Layered => "layered",
            BoundaryPreset::Hexagonal => "hexagonal",
            BoundaryPreset::FeatureSliced => "feature-sliced",
            BoundaryPreset::Bulletproof => "bulletproof",
        })
    }

    /// Validate that patterns do not repeat the zone root.
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
            for pattern in &zone.patterns {
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
                        Glob::new(pattern)
                            .expect("boundaries.zones[].patterns was validated at config load time")
                            .compile_matcher()
                    })
                    .collect();
                let root = zone.root.as_deref().map(normalize_zone_root);
                ResolvedZone {
                    name: zone.name.clone(),
                    patterns: zone.patterns.clone(),
                    matchers,
                    root,
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
                    Glob::new(pattern)
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
        let child_pattern = format!("{}/**", join_relative_path(discover_dir, child_name));
        let entry = self
            .zones_by_name
            .entry(zone_name.clone())
            .or_insert_with(|| BoundaryZone {
                name: zone_name.clone(),
                patterns: vec![],
                auto_discover: vec![],
                root: zone.root.clone(),
            });
        if !entry
            .patterns
            .iter()
            .any(|pattern| pattern == &child_pattern)
        {
            entry.patterns.push(child_pattern);
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
        for zone in &self.zones {
            let candidate: &str = match zone.root.as_deref() {
                Some(root) if !root.is_empty() => {
                    let Some(stripped) = relative_path.strip_prefix(root) else {
                        continue;
                    };
                    stripped
                }
                _ => relative_path,
            };
            if zone.matchers.iter().any(|m| m.is_match(candidate)) {
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
mod tests {
    use super::*;

    #[test]
    fn empty_config() {
        let config = BoundaryConfig::default();
        assert!(config.is_empty());
        assert!(config.validate_zone_references().is_empty());
    }

    #[test]
    fn deserialize_json() {
        let json = r#"{
            "zones": [
                { "name": "ui", "patterns": ["src/components/**", "src/pages/**"] },
                { "name": "db", "patterns": ["src/db/**"] },
                { "name": "shared", "patterns": ["src/shared/**"] }
            ],
            "rules": [
                { "from": "ui", "allow": ["shared"] },
                { "from": "db", "allow": ["shared"] }
            ]
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.zones.len(), 3);
        assert_eq!(config.rules.len(), 2);
        assert_eq!(config.zones[0].name, "ui");
        assert_eq!(
            config.zones[0].patterns,
            vec!["src/components/**", "src/pages/**"]
        );
        assert_eq!(config.rules[0].from, "ui");
        assert_eq!(config.rules[0].allow, vec!["shared"]);
    }

    #[test]
    fn deserialize_boundary_coverage() {
        let json = r#"{
            "coverage": {
                "requireAllFiles": true,
                "allowUnmatched": ["src/generated/**"]
            }
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();

        assert!(config.coverage.require_all_files);
        assert_eq!(config.coverage.allow_unmatched, vec!["src/generated/**"]);
        assert!(!config.is_empty());
    }

    #[test]
    fn deserialize_toml() {
        let toml_str = r#"
[[zones]]
name = "ui"
patterns = ["src/components/**"]

[[zones]]
name = "db"
patterns = ["src/db/**"]

[[rules]]
from = "ui"
allow = ["db"]
"#;
        let config: BoundaryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.zones.len(), 2);
        assert_eq!(config.rules.len(), 1);
    }

    #[test]
    fn deserialize_boundary_calls_single_and_array() {
        let json = r#"{
            "zones": [{ "name": "domain", "patterns": ["src/domain/**"] }],
            "calls": {
                "forbidden": [
                    { "from": "domain", "callee": "child_process.*" },
                    { "from": "domain", "callee": ["console.*", "process.exit"] }
                ]
            }
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.calls.forbidden.len(), 2);
        assert_eq!(
            config.calls.forbidden[0].callee.iter().collect::<Vec<_>>(),
            vec!["child_process.*"]
        );
        assert_eq!(
            config.calls.forbidden[1].callee.iter().collect::<Vec<_>>(),
            vec!["console.*", "process.exit"]
        );
        assert!(!config.is_empty());
        assert!(config.validate_zone_references().is_empty());
        assert!(config.validate_call_rules().is_empty());
    }

    #[test]
    fn deserialize_boundary_calls_toml() {
        let toml_str = r#"
[[zones]]
name = "domain"
patterns = ["src/domain/**"]

[[calls.forbidden]]
from = "domain"
callee = "child_process.*"

[[calls.forbidden]]
from = "domain"
callee = ["console.*"]
"#;
        let config: BoundaryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.calls.forbidden.len(), 2);
        assert_eq!(
            config.calls.forbidden[0].callee.iter().collect::<Vec<_>>(),
            vec!["child_process.*"]
        );
        assert_eq!(
            config.calls.forbidden[1].callee.iter().collect::<Vec<_>>(),
            vec!["console.*"]
        );
    }

    #[test]
    fn validate_zone_references_calls_from_unknown() {
        let json = r#"{
            "zones": [{ "name": "domain", "patterns": ["src/domain/**"] }],
            "calls": { "forbidden": [{ "from": "nonexistent", "callee": "console.*" }] }
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        let errors = config.validate_zone_references();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind, ZoneReferenceKind::CallsFrom);
        assert_eq!(errors[0].zone_name, "nonexistent");
        let rendered = ZoneValidationError::UnknownZoneReference(errors[0].clone()).to_string();
        assert!(
            rendered.contains("boundaries.calls.forbidden[0].from"),
            "unexpected rendering: {rendered}"
        );
    }

    #[test]
    fn validate_call_rules_rejects_inert_patterns() {
        let json = r#"{
            "zones": [{ "name": "domain", "patterns": ["src/domain/**"] }],
            "calls": {
                "forbidden": [
                    { "from": "domain", "callee": "*" },
                    { "from": "domain", "callee": "  " },
                    { "from": "domain", "callee": "foo..bar" },
                    { "from": "domain", "callee": [] }
                ]
            }
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        let errors = config.validate_call_rules();
        assert_eq!(errors.len(), 4);
        assert!(errors[0].reason.contains("matches nothing"));
        assert!(errors[1].reason.contains("must not be empty"));
        assert!(errors[2].reason.contains("empty path segment"));
        assert!(errors[3].reason.contains("at least one callee pattern"));
    }

    #[test]
    fn validate_call_rules_rejects_misplaced_wildcards() {
        let json = r#"{
            "zones": [{ "name": "domain", "patterns": ["src/domain/**"] }],
            "calls": {
                "forbidden": [
                    { "from": "domain", "callee": "a.*.b" },
                    { "from": "domain", "callee": "*.query.*" },
                    { "from": "domain", "callee": "con*ole.log" },
                    { "from": "domain", "callee": ["console.*", "*.innerHTML", "child_process.exec"] }
                ]
            }
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        let errors = config.validate_call_rules();
        assert_eq!(errors.len(), 3);
        assert!(errors[0].reason.contains("not both and not mid-path"));
        assert!(errors[1].reason.contains("not both and not mid-path"));
        assert!(errors[2].reason.contains("not globs"));
    }

    #[test]
    fn resolve_groups_calls_by_zone() {
        let json = r#"{
            "zones": [
                { "name": "domain", "patterns": ["src/domain/**"] },
                { "name": "ui", "patterns": ["src/ui/**"] }
            ],
            "calls": {
                "forbidden": [
                    { "from": "domain", "callee": "child_process.*" },
                    { "from": "domain", "callee": ["console.*"] },
                    { "from": "ui", "callee": "process.exit" }
                ]
            }
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        let resolved = config.resolve();
        assert_eq!(
            resolved.calls_forbidden_by_zone.get("domain"),
            Some(&vec![
                "child_process.*".to_string(),
                "console.*".to_string()
            ])
        );
        assert_eq!(
            resolved.calls_forbidden_by_zone.get("ui"),
            Some(&vec!["process.exit".to_string()])
        );
        assert!(!resolved.is_empty());
    }

    #[test]
    fn auto_discover_expands_child_zones_and_parent_rules() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "app".to_string(),
                    patterns: vec!["src/app/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
            ],
            rules: vec![
                BoundaryRule {
                    from: "app".to_string(),
                    allow: vec!["features".to_string()],
                    allow_type_only: vec![],
                },
                BoundaryRule {
                    from: "features".to_string(),
                    allow: vec![],
                    allow_type_only: vec![],
                },
            ],
        };

        config.expand_auto_discover(temp.path());

        let zone_names: Vec<_> = config.zones.iter().map(|zone| zone.name.as_str()).collect();
        assert_eq!(zone_names, vec!["app", "features/auth", "features/billing"]);
        assert_eq!(
            config.zones[1].patterns,
            vec!["src/features/auth/**".to_string()]
        );
        assert_eq!(
            config.zones[2].patterns,
            vec!["src/features/billing/**".to_string()]
        );
        let app_rule = config
            .rules
            .iter()
            .find(|rule| rule.from == "app")
            .expect("app rule should be preserved");
        assert_eq!(
            app_rule.allow,
            vec!["features/auth".to_string(), "features/billing".to_string()]
        );
        assert!(
            config
                .rules
                .iter()
                .any(|rule| rule.from == "features/auth" && rule.allow.is_empty())
        );
        assert!(
            config
                .rules
                .iter()
                .any(|rule| rule.from == "features/billing" && rule.allow.is_empty())
        );
        assert!(config.validate_zone_references().is_empty());
    }

    #[test]
    fn auto_discover_parent_fallback_allows_children_without_relaxing_child_rules() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "app".to_string(),
                    patterns: vec!["src/app/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec!["src/features/**".to_string()],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec!["src/shared/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![
                BoundaryRule {
                    from: "app".to_string(),
                    allow: vec!["features".to_string(), "shared".to_string()],
                    allow_type_only: vec![],
                },
                BoundaryRule {
                    from: "features".to_string(),
                    allow: vec!["shared".to_string()],
                    allow_type_only: vec![],
                },
            ],
        };

        config.expand_auto_discover(temp.path());

        let zone_names: Vec<_> = config.zones.iter().map(|zone| zone.name.as_str()).collect();
        assert_eq!(
            zone_names,
            vec![
                "app",
                "features/auth",
                "features/billing",
                "features",
                "shared"
            ]
        );

        let app_rule = config
            .rules
            .iter()
            .find(|rule| rule.from == "app")
            .expect("app rule should be preserved");
        assert_eq!(
            app_rule.allow,
            vec![
                "features/auth".to_string(),
                "features/billing".to_string(),
                "features".to_string(),
                "shared".to_string()
            ]
        );

        let parent_rule = config
            .rules
            .iter()
            .find(|rule| rule.from == "features")
            .expect("parent fallback rule should be preserved");
        assert_eq!(
            parent_rule.allow,
            vec![
                "shared".to_string(),
                "features/auth".to_string(),
                "features/billing".to_string()
            ]
        );

        let auth_rule = config
            .rules
            .iter()
            .find(|rule| rule.from == "features/auth")
            .expect("auth child rule should be generated");
        assert_eq!(auth_rule.allow, vec!["shared".to_string()]);

        let billing_rule = config
            .rules
            .iter()
            .find(|rule| rule.from == "features/billing")
            .expect("billing child rule should be generated");
        assert_eq!(billing_rule.allow, vec!["shared".to_string()]);
        assert!(config.validate_zone_references().is_empty());
    }

    #[test]
    fn auto_discover_explicit_child_rule_wins_over_generated_parent_rule() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        for explicit_child_first in [true, false] {
            let explicit_child_rule = BoundaryRule {
                from: "features/auth".to_string(),
                allow: vec!["shared".to_string(), "features/billing".to_string()],
                allow_type_only: vec![],
            };
            let parent_rule = BoundaryRule {
                from: "features".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            };
            let rules = if explicit_child_first {
                vec![explicit_child_rule, parent_rule]
            } else {
                vec![parent_rule, explicit_child_rule]
            };

            let mut config = BoundaryConfig {
                coverage: BoundaryCoverageConfig::default(),
                calls: BoundaryCallsConfig::default(),
                preset: None,
                zones: vec![
                    BoundaryZone {
                        name: "features".to_string(),
                        patterns: vec![],
                        auto_discover: vec!["src/features".to_string()],
                        root: None,
                    },
                    BoundaryZone {
                        name: "shared".to_string(),
                        patterns: vec!["src/shared/**".to_string()],
                        auto_discover: vec![],
                        root: None,
                    },
                ],
                rules,
            };

            config.expand_auto_discover(temp.path());

            let auth_rule = config
                .rules
                .iter()
                .find(|rule| rule.from == "features/auth")
                .expect("explicit child rule should remain");
            assert_eq!(
                auth_rule.allow,
                vec!["shared".to_string(), "features/billing".to_string()],
                "explicit child rule should win regardless of rule order"
            );

            let billing_rule = config
                .rules
                .iter()
                .find(|rule| rule.from == "features/billing")
                .expect("parent rule should still generate sibling child rule");
            assert_eq!(billing_rule.allow, vec!["shared".to_string()]);
            assert!(config.validate_zone_references().is_empty());
        }
    }

    #[test]
    fn logical_groups_returned_for_simple_auto_discover_zone() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "app".to_string(),
                    patterns: vec!["src/app/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "features".to_string(),
                allow: vec!["app".to_string()],
                allow_type_only: vec![],
            }],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g.name, "features");
        assert_eq!(g.children, vec!["features/auth", "features/billing"]);
        assert_eq!(g.auto_discover, vec!["src/features"]);
        assert_eq!(g.source_zone_index, 1);
        assert_eq!(g.status, LogicalGroupStatus::Ok);
        assert!(g.fallback_zone.is_none());
        let rule = g
            .authored_rule
            .as_ref()
            .expect("authored rule preserved verbatim");
        assert_eq!(rule.allow, vec!["app"]);
        assert!(rule.allow_type_only.is_empty());
    }

    #[test]
    fn logical_groups_preserve_verbatim_auto_discover_strings() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["./src/features/".to_string()],
                root: None,
            }],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].auto_discover, vec!["./src/features/"]);
        assert_eq!(groups[0].children, vec!["features/auth"]);
    }

    #[test]
    fn logical_groups_bulletproof_keeps_fallback_zone_cross_reference() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec!["src/features/**".to_string()],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            }],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].fallback_zone.as_deref(), Some("features"));
        assert!(config.zones.iter().any(|z| z.name == "features"));
    }

    #[test]
    fn logical_groups_status_empty_when_no_child_dirs() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features")).unwrap();
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            }],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].status, LogicalGroupStatus::Empty);
        assert!(groups[0].children.is_empty());
    }

    #[test]
    fn logical_groups_status_invalid_path_when_dir_missing() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            }],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].status, LogicalGroupStatus::InvalidPath);
        assert!(groups[0].children.is_empty());
    }

    #[test]
    fn logical_groups_status_ok_wins_over_invalid_when_mixed() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string(), "src/modules".to_string()],
                root: None,
            }],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].status, LogicalGroupStatus::Ok);
        assert_eq!(groups[0].children, vec!["features/auth"]);
    }

    #[test]
    fn logical_groups_preserve_declaration_order() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/zeta/a")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/alpha/a")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/mid/a")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "zeta".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/zeta".to_string()],
                    root: None,
                },
                BoundaryZone {
                    name: "alpha".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/alpha".to_string()],
                    root: None,
                },
                BoundaryZone {
                    name: "mid".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/mid".to_string()],
                    root: None,
                },
            ],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["zeta", "alpha", "mid"]);
    }

    #[test]
    fn logical_groups_merged_from_records_duplicate_indices() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/extra/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
                BoundaryZone {
                    name: "other".to_string(),
                    patterns: vec!["src/other/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/extra".to_string()],
                    root: None,
                },
            ],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].merged_from.as_deref(), Some(&[0_usize, 2][..]));
        assert_eq!(groups[0].source_zone_index, 0);
    }

    #[test]
    fn logical_groups_merged_from_none_on_single_declaration() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            }],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert!(groups[0].merged_from.is_none());
    }

    #[test]
    fn logical_groups_echo_original_zone_root() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("packages/app/src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: Some("packages/app/".to_string()),
            }],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(
            groups[0].original_zone_root.as_deref(),
            Some("packages/app/")
        );
    }

    #[test]
    fn logical_groups_original_zone_root_none_when_unset() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            }],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert!(groups[0].original_zone_root.is_none());
    }

    #[test]
    fn logical_groups_child_source_indices_populated_for_multi_path() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/modules/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string(), "src/modules".to_string()],
                root: None,
            }],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(
            groups[0].children,
            vec!["features/auth", "features/billing"]
        );
        assert_eq!(groups[0].child_source_indices, vec![0, 1]);
    }

    #[test]
    fn logical_groups_child_source_indices_empty_for_single_path() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            }],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert!(groups[0].child_source_indices.is_empty());
    }

    #[test]
    fn logical_groups_child_source_indices_after_duplicate_merge_shifted() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/extra/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/extra".to_string()],
                    root: None,
                },
            ],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].auto_discover, vec!["src/features", "src/extra"]);
        let auth_idx = groups[0]
            .children
            .iter()
            .position(|c| c == "features/auth")
            .unwrap();
        let billing_idx = groups[0]
            .children
            .iter()
            .position(|c| c == "features/billing")
            .unwrap();
        assert_eq!(groups[0].child_source_indices[auth_idx], 0);
        assert_eq!(groups[0].child_source_indices[billing_idx], 1);
    }

    #[test]
    fn logical_groups_merge_duplicate_parent_zone_declarations() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/extra/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/extra".to_string()],
                    root: None,
                },
            ],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "features");
        assert_eq!(groups[0].auto_discover, vec!["src/features", "src/extra"]);
        assert!(groups[0].children.iter().any(|c| c == "features/auth"));
        assert!(groups[0].children.iter().any(|c| c == "features/billing"));
        assert_eq!(groups[0].source_zone_index, 0);
    }

    #[test]
    fn logical_groups_duplicate_identical_declarations_no_double_count() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
                BoundaryZone {
                    name: "features".to_string(),
                    patterns: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    root: None,
                },
            ],
            rules: vec![],
        };

        let groups = config.expand_auto_discover(temp.path());
        assert_eq!(groups.len(), 1);
        let zone_names: Vec<&str> = config.zones.iter().map(|z| z.name.as_str()).collect();
        assert_eq!(zone_names, vec!["features/auth", "features/billing"]);
        assert_eq!(
            groups[0].children,
            vec!["features/auth", "features/billing"]
        );
        assert_eq!(
            groups[0].auto_discover,
            vec!["src/features", "src/features"]
        );
        assert_eq!(groups[0].merged_from.as_deref(), Some(&[0_usize, 1][..]));
    }

    #[test]
    fn logical_groups_empty_when_no_auto_discover_present() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/components/**".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        assert!(groups.is_empty());
    }

    #[test]
    fn logical_groups_propagate_through_resolve() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                root: None,
            }],
            rules: vec![],
        };
        let groups = config.expand_auto_discover(temp.path());
        let mut resolved = config.resolve();
        resolved.logical_groups = groups;
        assert_eq!(resolved.logical_groups.len(), 1);
        assert_eq!(resolved.logical_groups[0].name, "features");
        assert_eq!(resolved.logical_groups[0].children, vec!["features/auth"]);
    }

    #[test]
    fn validate_zone_references_valid() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["db".to_string()],
                allow_type_only: vec![],
            }],
        };
        assert!(config.validate_zone_references().is_empty());
    }

    #[test]
    fn validate_zone_references_invalid_from() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec![],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![BoundaryRule {
                from: "nonexistent".to_string(),
                allow: vec!["ui".to_string()],
                allow_type_only: vec![],
            }],
        };
        let errors = config.validate_zone_references();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].zone_name, "nonexistent");
        assert_eq!(errors[0].kind, ZoneReferenceKind::From);
        assert_eq!(errors[0].rule_index, 0);
    }

    #[test]
    fn validate_zone_references_invalid_allow() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec![],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["nonexistent".to_string()],
                allow_type_only: vec![],
            }],
        };
        let errors = config.validate_zone_references();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].zone_name, "nonexistent");
        assert_eq!(errors[0].kind, ZoneReferenceKind::Allow);
    }

    #[test]
    fn validate_zone_references_invalid_allow_type_only() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec![],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
                allow_type_only: vec!["nonexistent_type_zone".to_string()],
            }],
        };
        let errors = config.validate_zone_references();
        assert_eq!(errors.len(), 1, "got: {errors:?}");
        assert_eq!(errors[0].zone_name, "nonexistent_type_zone");
        assert_eq!(errors[0].kind, ZoneReferenceKind::AllowTypeOnly);
    }

    #[test]
    fn resolve_and_classify() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/components/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec!["src/db/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert_eq!(
            resolved.classify_zone("src/components/Button.tsx"),
            Some("ui")
        );
        assert_eq!(resolved.classify_zone("src/db/queries.ts"), Some("db"));
        assert_eq!(resolved.classify_zone("src/utils/helpers.ts"), None);
    }

    #[test]
    fn first_match_wins() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "specific".to_string(),
                    patterns: vec!["src/shared/db-utils/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec!["src/shared/**".to_string()],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert_eq!(
            resolved.classify_zone("src/shared/db-utils/pool.ts"),
            Some("specific")
        );
        assert_eq!(
            resolved.classify_zone("src/shared/helpers.ts"),
            Some("shared")
        );
    }

    #[test]
    fn self_import_always_allowed() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec![],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec![],
                allow_type_only: vec![],
            }],
        };
        let resolved = config.resolve();
        assert!(resolved.is_import_allowed("ui", "ui"));
    }

    #[test]
    fn unrestricted_zone_allows_all() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert!(resolved.is_import_allowed("shared", "db"));
    }

    #[test]
    fn restricted_zone_blocks_unlisted() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "db".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "shared".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["shared".to_string()],
                allow_type_only: vec![],
            }],
        };
        let resolved = config.resolve();
        assert!(resolved.is_import_allowed("ui", "shared"));
        assert!(!resolved.is_import_allowed("ui", "db"));
    }

    #[test]
    fn empty_allow_blocks_all_except_self() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "isolated".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "other".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "isolated".to_string(),
                allow: vec![],
                allow_type_only: vec![],
            }],
        };
        let resolved = config.resolve();
        assert!(resolved.is_import_allowed("isolated", "isolated"));
        assert!(!resolved.is_import_allowed("isolated", "other"));
    }

    #[test]
    fn zone_root_filters_classification_to_subtree() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/**".to_string()],
                    auto_discover: vec![],
                    root: Some("packages/app/".to_string()),
                },
                BoundaryZone {
                    name: "domain".to_string(),
                    patterns: vec!["src/**".to_string()],
                    auto_discover: vec![],
                    root: Some("packages/core/".to_string()),
                },
            ],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert_eq!(
            resolved.classify_zone("packages/app/src/login.tsx"),
            Some("ui")
        );
        assert_eq!(
            resolved.classify_zone("packages/core/src/order.ts"),
            Some("domain")
        );
        assert_eq!(resolved.classify_zone("src/login.tsx"), None);
        assert_eq!(resolved.classify_zone("packages/utils/src/x.ts"), None);
    }

    /// `root` matching is case-sensitive.
    #[test]
    fn zone_root_is_case_sensitive() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/app/".to_string()),
            }],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert_eq!(
            resolved.classify_zone("packages/app/src/login.tsx"),
            Some("ui"),
            "exact-case path classifies"
        );
        assert_eq!(
            resolved.classify_zone("packages/App/src/login.tsx"),
            None,
            "case-different path does not classify (root is case-sensitive)"
        );
        assert_eq!(
            resolved.classify_zone("Packages/app/src/login.tsx"),
            None,
            "case-different prefix does not classify"
        );
    }

    #[test]
    fn zone_root_normalizes_trailing_slash_and_dot_prefix() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "no-slash".to_string(),
                    patterns: vec!["src/**".to_string()],
                    auto_discover: vec![],
                    root: Some("packages/app".to_string()),
                },
                BoundaryZone {
                    name: "dot-prefixed".to_string(),
                    patterns: vec!["src/**".to_string()],
                    auto_discover: vec![],
                    root: Some("./packages/lib/".to_string()),
                },
            ],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert_eq!(resolved.zones[0].root.as_deref(), Some("packages/app/"));
        assert_eq!(resolved.zones[1].root.as_deref(), Some("packages/lib/"));
        assert_eq!(
            resolved.classify_zone("packages/app/src/x.ts"),
            Some("no-slash")
        );
        assert_eq!(
            resolved.classify_zone("packages/lib/src/x.ts"),
            Some("dot-prefixed")
        );
    }

    #[test]
    fn validate_root_prefixes_flags_redundant_pattern() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["packages/app/src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/app/".to_string()),
            }],
            rules: vec![],
        };
        let errors = config.validate_root_prefixes();
        assert_eq!(errors.len(), 1, "expected one redundant-prefix error");
        assert_eq!(errors[0].zone_name, "ui");
        assert_eq!(errors[0].pattern, "packages/app/src/**");
        assert_eq!(errors[0].root, "packages/app/");
        let rendered = ZoneValidationError::RedundantRootPrefix(errors[0].clone()).to_string();
        assert!(
            rendered.contains("FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX"),
            "Display should carry legacy tag: {rendered}"
        );
        assert!(
            rendered.contains("zone 'ui'"),
            "Display rendering: {rendered}"
        );
        assert!(
            rendered.contains("packages/app/src/**"),
            "Display rendering: {rendered}"
        );
    }

    #[test]
    fn validate_root_prefixes_handles_unnormalized_root() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["./packages/app/src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/app".to_string()),
            }],
            rules: vec![],
        };
        let errors = config.validate_root_prefixes();
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn validate_root_prefixes_empty_when_no_overlap() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/**".to_string()],
                auto_discover: vec![],
                root: Some("packages/app/".to_string()),
            }],
            rules: vec![],
        };
        assert!(config.validate_root_prefixes().is_empty());
    }

    #[test]
    fn validate_root_prefixes_skips_zones_without_root() {
        let json = r#"{
            "zones": [{ "name": "ui", "patterns": ["src/**"] }],
            "rules": []
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        assert!(config.validate_root_prefixes().is_empty());
    }

    /// Empty-normalized roots must be ignored.
    #[test]
    fn validate_root_prefixes_skips_empty_root() {
        for raw_root in ["", ".", "./"] {
            let config = BoundaryConfig {
                coverage: BoundaryCoverageConfig::default(),
                calls: BoundaryCallsConfig::default(),
                preset: None,
                zones: vec![BoundaryZone {
                    name: "ui".to_string(),
                    patterns: vec!["src/**".to_string(), "lib/**".to_string()],
                    auto_discover: vec![],
                    root: Some(raw_root.to_string()),
                }],
                rules: vec![],
            };
            let errors = config.validate_root_prefixes();
            assert!(
                errors.is_empty(),
                "empty-normalized root {raw_root:?} produced spurious errors: {errors:?}"
            );
        }
    }

    #[test]
    fn deserialize_zone_with_root() {
        let json = r#"{
            "zones": [
                { "name": "ui", "patterns": ["src/**"], "root": "packages/app/" }
            ],
            "rules": []
        }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.zones[0].root.as_deref(), Some("packages/app/"));
    }

    #[test]
    fn deserialize_preset_json() {
        let json = r#"{ "preset": "layered" }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, Some(BoundaryPreset::Layered));
        assert!(config.zones.is_empty());
    }

    #[test]
    fn deserialize_preset_hexagonal_json() {
        let json = r#"{ "preset": "hexagonal" }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, Some(BoundaryPreset::Hexagonal));
    }

    #[test]
    fn deserialize_preset_feature_sliced_json() {
        let json = r#"{ "preset": "feature-sliced" }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, Some(BoundaryPreset::FeatureSliced));
    }

    #[test]
    fn deserialize_preset_toml() {
        let toml_str = r#"preset = "layered""#;
        let config: BoundaryConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.preset, Some(BoundaryPreset::Layered));
    }

    #[test]
    fn deserialize_invalid_preset_rejected() {
        let json = r#"{ "preset": "invalid_preset" }"#;
        let result: Result<BoundaryConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn preset_absent_by_default() {
        let config = BoundaryConfig::default();
        assert!(config.preset.is_none());
        assert!(config.is_empty());
    }

    #[test]
    fn preset_makes_config_non_empty() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Layered),
            zones: vec![],
            rules: vec![],
        };
        assert!(!config.is_empty());
    }

    #[test]
    fn expand_layered_produces_four_zones() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Layered),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 4);
        assert_eq!(config.rules.len(), 4);
        assert!(config.preset.is_none(), "preset cleared after expand");
        assert_eq!(config.zones[0].name, "presentation");
        assert_eq!(config.zones[0].patterns, vec!["src/presentation/**"]);
    }

    #[test]
    fn expand_layered_rules_correct() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Layered),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        let pres_rule = config
            .rules
            .iter()
            .find(|r| r.from == "presentation")
            .unwrap();
        assert_eq!(pres_rule.allow, vec!["application"]);
        let app_rule = config
            .rules
            .iter()
            .find(|r| r.from == "application")
            .unwrap();
        assert_eq!(app_rule.allow, vec!["domain"]);
        let dom_rule = config.rules.iter().find(|r| r.from == "domain").unwrap();
        assert!(dom_rule.allow.is_empty());
        let infra_rule = config
            .rules
            .iter()
            .find(|r| r.from == "infrastructure")
            .unwrap();
        assert_eq!(infra_rule.allow, vec!["domain", "application"]);
    }

    #[test]
    fn expand_hexagonal_produces_three_zones() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Hexagonal),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 3);
        assert_eq!(config.rules.len(), 3);
        assert_eq!(config.zones[0].name, "adapters");
        assert_eq!(config.zones[1].name, "ports");
        assert_eq!(config.zones[2].name, "domain");
    }

    #[test]
    fn expand_feature_sliced_produces_six_zones() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::FeatureSliced),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 6);
        assert_eq!(config.rules.len(), 6);
        let app_rule = config.rules.iter().find(|r| r.from == "app").unwrap();
        assert_eq!(
            app_rule.allow,
            vec!["pages", "widgets", "features", "entities", "shared"]
        );
        let shared_rule = config.rules.iter().find(|r| r.from == "shared").unwrap();
        assert!(shared_rule.allow.is_empty());
        let ent_rule = config.rules.iter().find(|r| r.from == "entities").unwrap();
        assert_eq!(ent_rule.allow, vec!["shared"]);
    }

    #[test]
    fn expand_bulletproof_produces_four_zones() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Bulletproof),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 4);
        assert_eq!(config.rules.len(), 4);
        assert_eq!(config.zones[0].name, "app");
        assert_eq!(config.zones[1].name, "features");
        assert_eq!(config.zones[2].name, "shared");
        assert_eq!(config.zones[3].name, "server");
        assert!(config.zones[2].patterns.len() > 1);
        assert!(
            config.zones[2]
                .patterns
                .contains(&"src/components/**".to_string())
        );
        assert!(
            config.zones[2]
                .patterns
                .contains(&"src/hooks/**".to_string())
        );
        assert!(config.zones[2].patterns.contains(&"src/lib/**".to_string()));
        assert!(
            config.zones[2]
                .patterns
                .contains(&"src/providers/**".to_string())
        );
    }

    #[test]
    fn expand_bulletproof_rules_correct() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Bulletproof),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        let app_rule = config.rules.iter().find(|r| r.from == "app").unwrap();
        assert_eq!(app_rule.allow, vec!["features", "shared", "server"]);
        let feat_rule = config.rules.iter().find(|r| r.from == "features").unwrap();
        assert_eq!(feat_rule.allow, vec!["shared", "server"]);
        let srv_rule = config.rules.iter().find(|r| r.from == "server").unwrap();
        assert_eq!(srv_rule.allow, vec!["shared"]);
        let shared_rule = config.rules.iter().find(|r| r.from == "shared").unwrap();
        assert!(shared_rule.allow.is_empty());
    }

    #[test]
    fn expand_bulletproof_then_resolve_classifies() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Bulletproof),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        let resolved = config.resolve();
        assert_eq!(
            resolved.classify_zone("src/app/dashboard/page.tsx"),
            Some("app")
        );
        assert_eq!(
            resolved.classify_zone("src/features/auth/hooks/useAuth.ts"),
            Some("features"),
            "without expand_auto_discover, src/features/... falls back to the parent zone"
        );
        assert_eq!(
            resolved.classify_zone("src/components/Button/Button.tsx"),
            Some("shared")
        );
        assert_eq!(
            resolved.classify_zone("src/hooks/useFormatters.ts"),
            Some("shared")
        );
        assert_eq!(
            resolved.classify_zone("src/server/db/schema/users.ts"),
            Some("server")
        );
        assert!(resolved.is_import_allowed("features", "shared"));
        assert!(resolved.is_import_allowed("features", "server"));
        assert!(!resolved.is_import_allowed("features", "app"));
        assert!(!resolved.is_import_allowed("shared", "features"));
        assert!(!resolved.is_import_allowed("server", "features"));
    }

    /// Bulletproof barrels should not violate child boundaries.
    #[test]
    fn bulletproof_features_barrel_can_import_children() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Bulletproof),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        config.expand_auto_discover(temp.path());
        let resolved = config.resolve();

        assert_eq!(
            resolved.classify_zone("src/features/index.ts"),
            Some("features"),
            "src/features/index.ts barrel should classify as the parent features zone"
        );
        assert_eq!(
            resolved.classify_zone("src/features/auth/login.ts"),
            Some("features/auth")
        );
        assert_eq!(
            resolved.classify_zone("src/features/billing/invoice.ts"),
            Some("features/billing")
        );
        assert!(resolved.is_import_allowed("features", "features/auth"));
        assert!(resolved.is_import_allowed("features", "features/billing"));
        assert!(!resolved.is_import_allowed("features/auth", "features/billing"));
    }

    #[test]
    fn expand_uses_custom_source_root() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Hexagonal),
            zones: vec![],
            rules: vec![],
        };
        config.expand("lib");
        assert_eq!(config.zones[0].patterns, vec!["lib/adapters/**"]);
        assert_eq!(config.zones[2].patterns, vec!["lib/domain/**"]);
    }

    #[test]
    fn user_zone_replaces_preset_zone() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Hexagonal),
            zones: vec![BoundaryZone {
                name: "domain".to_string(),
                patterns: vec!["src/core/**".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 3);
        let domain = config.zones.iter().find(|z| z.name == "domain").unwrap();
        assert_eq!(domain.patterns, vec!["src/core/**"]);
    }

    #[test]
    fn user_zone_adds_to_preset() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Hexagonal),
            zones: vec![BoundaryZone {
                name: "shared".to_string(),
                patterns: vec!["src/shared/**".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 4);
        assert!(config.zones.iter().any(|z| z.name == "shared"));
    }

    #[test]
    fn user_rule_replaces_preset_rule() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Hexagonal),
            zones: vec![],
            rules: vec![BoundaryRule {
                from: "adapters".to_string(),
                allow: vec!["ports".to_string(), "domain".to_string()],
                allow_type_only: vec![],
            }],
        };
        config.expand("src");
        let adapter_rule = config.rules.iter().find(|r| r.from == "adapters").unwrap();
        assert_eq!(adapter_rule.allow, vec!["ports", "domain"]);
        assert_eq!(
            config.rules.iter().filter(|r| r.from == "adapters").count(),
            1
        );
    }

    #[test]
    fn expand_without_preset_is_noop() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 1);
        assert_eq!(config.zones[0].name, "ui");
    }

    #[test]
    fn expand_then_validate_succeeds() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Layered),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        assert!(config.validate_zone_references().is_empty());
    }

    #[test]
    fn expand_then_resolve_classifies() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::Hexagonal),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        let resolved = config.resolve();
        assert_eq!(
            resolved.classify_zone("src/adapters/http/handler.ts"),
            Some("adapters")
        );
        assert_eq!(resolved.classify_zone("src/domain/user.ts"), Some("domain"));
        assert!(!resolved.is_import_allowed("adapters", "domain"));
        assert!(resolved.is_import_allowed("adapters", "ports"));
    }

    #[test]
    fn preset_name_returns_correct_string() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::FeatureSliced),
            zones: vec![],
            rules: vec![],
        };
        assert_eq!(config.preset_name(), Some("feature-sliced"));

        let empty = BoundaryConfig::default();
        assert_eq!(empty.preset_name(), None);
    }

    #[test]
    fn preset_name_all_variants() {
        let cases = [
            (BoundaryPreset::Layered, "layered"),
            (BoundaryPreset::Hexagonal, "hexagonal"),
            (BoundaryPreset::FeatureSliced, "feature-sliced"),
            (BoundaryPreset::Bulletproof, "bulletproof"),
        ];
        for (preset, expected_name) in cases {
            let config = BoundaryConfig {
                coverage: BoundaryCoverageConfig::default(),
                calls: BoundaryCallsConfig::default(),
                preset: Some(preset),
                zones: vec![],
                rules: vec![],
            };
            assert_eq!(
                config.preset_name(),
                Some(expected_name),
                "preset_name() mismatch for variant"
            );
        }
    }

    #[test]
    fn resolved_boundary_config_empty() {
        let resolved = ResolvedBoundaryConfig::default();
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolved_boundary_config_with_zones_not_empty() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec!["src/ui/**".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert!(!resolved.is_empty());
    }

    #[test]
    fn resolved_boundary_config_with_only_logical_groups_not_empty() {
        let resolved = ResolvedBoundaryConfig {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![LogicalGroup {
                name: "features".to_string(),
                children: vec![],
                auto_discover: vec!["src/features".to_string()],
                authored_rule: None,
                fallback_zone: None,
                source_zone_index: 0,
                status: LogicalGroupStatus::Empty,
                merged_from: None,
                original_zone_root: None,
                child_source_indices: vec![],
            }],
            coverage: ResolvedBoundaryCoverageConfig::default(),
            calls_forbidden_by_zone: rustc_hash::FxHashMap::default(),
        };
        assert!(!resolved.is_empty());
    }

    #[test]
    fn boundary_config_with_only_rules_is_empty() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![],
            rules: vec![BoundaryRule {
                from: "ui".to_string(),
                allow: vec!["db".to_string()],
                allow_type_only: vec![],
            }],
        };
        assert!(config.is_empty());
    }

    #[test]
    fn boundary_config_with_zones_not_empty() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec![],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        assert!(!config.is_empty());
    }

    #[test]
    fn zone_with_multiple_patterns_matches_any() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec![
                    "src/components/**".to_string(),
                    "src/pages/**".to_string(),
                    "src/views/**".to_string(),
                ],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        let resolved = config.resolve();
        assert_eq!(
            resolved.classify_zone("src/components/Button.tsx"),
            Some("ui")
        );
        assert_eq!(resolved.classify_zone("src/pages/Home.tsx"), Some("ui"));
        assert_eq!(
            resolved.classify_zone("src/views/Dashboard.tsx"),
            Some("ui")
        );
        assert_eq!(resolved.classify_zone("src/utils/helpers.ts"), None);
    }

    #[test]
    fn validate_zone_references_multiple_errors() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "ui".to_string(),
                patterns: vec![],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![
                BoundaryRule {
                    from: "nonexistent_from".to_string(),
                    allow: vec!["nonexistent_allow".to_string()],
                    allow_type_only: vec![],
                },
                BoundaryRule {
                    from: "ui".to_string(),
                    allow: vec!["also_nonexistent".to_string()],
                    allow_type_only: vec![],
                },
            ],
        };
        let errors = config.validate_zone_references();
        assert_eq!(errors.len(), 3);
    }

    #[test]
    fn expand_feature_sliced_with_custom_root() {
        let mut config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: Some(BoundaryPreset::FeatureSliced),
            zones: vec![],
            rules: vec![],
        };
        config.expand("lib");
        assert_eq!(config.zones[0].patterns, vec!["lib/app/**"]);
        assert_eq!(config.zones[5].patterns, vec!["lib/shared/**"]);
    }

    #[test]
    fn zone_not_in_rules_is_unrestricted() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![
                BoundaryZone {
                    name: "a".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "b".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
                BoundaryZone {
                    name: "c".to_string(),
                    patterns: vec![],
                    auto_discover: vec![],
                    root: None,
                },
            ],
            rules: vec![BoundaryRule {
                from: "a".to_string(),
                allow: vec!["b".to_string()],
                allow_type_only: vec![],
            }],
        };
        let resolved = config.resolve();
        assert!(resolved.is_import_allowed("a", "b"));
        assert!(!resolved.is_import_allowed("a", "c"));
        assert!(resolved.is_import_allowed("b", "a"));
        assert!(resolved.is_import_allowed("b", "c"));
        assert!(resolved.is_import_allowed("c", "a"));
    }

    #[test]
    fn boundary_preset_json_roundtrip() {
        let presets = [
            BoundaryPreset::Layered,
            BoundaryPreset::Hexagonal,
            BoundaryPreset::FeatureSliced,
            BoundaryPreset::Bulletproof,
        ];
        for preset in presets {
            let json = serde_json::to_string(&preset).unwrap();
            let restored: BoundaryPreset = serde_json::from_str(&json).unwrap();
            assert_eq!(restored, preset);
        }
    }

    #[test]
    fn deserialize_preset_bulletproof_json() {
        let json = r#"{ "preset": "bulletproof" }"#;
        let config: BoundaryConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.preset, Some(BoundaryPreset::Bulletproof));
    }

    #[test]
    #[should_panic(expected = "validated at config load time")]
    fn resolve_panics_on_unvalidated_invalid_zone_glob() {
        let config = BoundaryConfig {
            coverage: BoundaryCoverageConfig::default(),
            calls: BoundaryCallsConfig::default(),
            preset: None,
            zones: vec![BoundaryZone {
                name: "broken".to_string(),
                patterns: vec!["[invalid".to_string()],
                auto_discover: vec![],
                root: None,
            }],
            rules: vec![],
        };
        let _ = config.resolve();
    }
}
