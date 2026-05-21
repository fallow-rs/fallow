//! Architecture boundary zone and rule definitions.

use std::fmt;
use std::path::Path;

use globset::Glob;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which zone-reference surface on a `BoundaryRule` carries an unknown name.
///
/// The diagnostic surfaces the kind so users editing a multi-field rule know
/// whether to fix `from`, `allow`, or `allowTypeOnly`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneReferenceKind {
    /// Rule's `from` field names an undefined zone.
    From,
    /// One entry in the rule's `allow` list names an undefined zone.
    Allow,
    /// One entry in the rule's `allowTypeOnly` list names an undefined zone.
    AllowTypeOnly,
}

impl ZoneReferenceKind {
    fn config_field(self) -> &'static str {
        match self {
            Self::From => "from",
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

/// One offending redundant-root-prefix pattern in a `boundaries.zones[]` entry.
///
/// Patterns are resolved relative to the zone `root`, so prefixing the pattern
/// with the same root double-prefixes the path and never matches a real file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedundantRootPrefix {
    /// Name of the zone whose pattern redundantly includes its root.
    pub zone_name: String,
    /// The offending pattern as authored.
    pub pattern: String,
    /// The normalized root that the pattern redundantly repeats.
    pub root: String,
}

/// Aggregated boundary-config validation error for `FallowConfig::validate_resolved_boundaries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZoneValidationError {
    /// A `boundaries.rules[]` entry references a zone NOT present in
    /// `boundaries.zones[]` (post-preset-expansion and post-auto-discover).
    UnknownZoneReference(UnknownZoneRef),
    /// A `boundaries.zones[].patterns[]` entry redundantly prefixes its
    /// pattern with the zone `root`.
    RedundantRootPrefix(RedundantRootPrefix),
}

impl fmt::Display for ZoneValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownZoneReference(err) => write!(
                f,
                "boundaries.rules[{}].{}: references undefined zone '{}'",
                err.rule_index,
                err.kind.config_field(),
                err.zone_name,
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
///
/// Each preset expands into a set of zones and import rules for a common
/// architecture pattern. User-defined zones and rules merge on top of the
/// preset defaults (zones with the same name replace the preset zone;
/// rules with the same `from` replace the preset rule).
///
/// # Examples
///
/// ```
/// use fallow_config::BoundaryPreset;
///
/// let preset: BoundaryPreset = serde_json::from_str(r#""layered""#).unwrap();
/// assert!(matches!(preset, BoundaryPreset::Layered));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryPreset {
    /// Classic layered architecture: presentation → application → domain ← infrastructure.
    /// Infrastructure may also import from application (common in DI frameworks).
    Layered,
    /// Hexagonal / ports-and-adapters: adapters → ports → domain.
    Hexagonal,
    /// Feature-Sliced Design: app > pages > widgets > features > entities > shared.
    /// Each layer may only import from layers below it.
    FeatureSliced,
    /// Bulletproof React: app → features → shared + server.
    /// Feature modules are isolated from each other via `autoDiscover`: every
    /// immediate child of `src/features/` becomes its own `features/<name>` zone,
    /// and cross-feature imports are reported as boundary violations.
    /// Top-level files in `src/features/` are classified by the logical
    /// `features` parent zone, so barrels can re-export child features while
    /// non-barrel top-level files still obey the `features` boundary rule.
    Bulletproof,
}

impl BoundaryPreset {
    /// Expand the preset into default zones and rules.
    ///
    /// `source_root` is the directory prefix for zone patterns (e.g., `"src"`, `"lib"`).
    /// Patterns are generated as `{source_root}/{zone_name}/**`.
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
                // Discovered child zones classify concrete feature modules
                // first; the parent pattern catches top-level feature files
                // such as barrels and shared types.
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
///
/// Defines zones (directory groupings) and rules (which zones may import from which).
/// Optionally uses a built-in preset as a starting point.
///
/// # Examples
///
/// ```
/// use fallow_config::BoundaryConfig;
///
/// let json = r#"{
///     "zones": [
///         { "name": "ui", "patterns": ["src/components/**"] },
///         { "name": "db", "patterns": ["src/db/**"] }
///     ],
///     "rules": [
///         { "from": "ui", "allow": ["db"] }
///     ]
/// }"#;
/// let config: BoundaryConfig = serde_json::from_str(json).unwrap();
/// assert_eq!(config.zones.len(), 2);
/// assert_eq!(config.rules.len(), 1);
/// ```
///
/// Using a preset:
///
/// ```
/// use fallow_config::BoundaryConfig;
///
/// let json = r#"{ "preset": "layered" }"#;
/// let mut config: BoundaryConfig = serde_json::from_str(json).unwrap();
/// config.expand("src");
/// assert_eq!(config.zones.len(), 4);
/// assert_eq!(config.rules.len(), 4);
/// ```
#[derive(Debug, Default, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryConfig {
    /// Built-in architecture preset. When set, expands into default zones and rules.
    /// User-defined zones and rules merge on top: zones with the same name replace
    /// the preset zone; rules with the same `from` replace the preset rule.
    /// Preset patterns use `{rootDir}/{zone}/**` where rootDir is auto-detected
    /// from tsconfig.json (falls back to `src`).
    /// Note: preset patterns are flat (`src/<zone>/**`). For monorepos with
    /// per-package source directories, define zones explicitly instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<BoundaryPreset>,
    /// Named zones mapping directory patterns to architectural layers.
    #[serde(default)]
    pub zones: Vec<BoundaryZone>,
    /// Import rules between zones. A zone with a rule entry can only import
    /// from the listed zones (plus itself). A zone without a rule entry is unrestricted.
    #[serde(default)]
    pub rules: Vec<BoundaryRule>,
}

/// A named zone grouping files by directory pattern.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryZone {
    /// Zone identifier referenced in rules (e.g., `"ui"`, `"database"`, `"shared"`).
    pub name: String,
    /// Glob patterns (relative to project root) that define zone membership.
    /// A file belongs to the first zone whose pattern matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    /// Directories whose immediate child directories should become separate
    /// zones under this logical group.
    ///
    /// For example, `{ "name": "features", "autoDiscover": ["src/features"] }`
    /// creates zones such as `features/auth` and `features/billing`, each with
    /// a pattern for its own subtree. Rules that reference `features` expand to
    /// every discovered child zone. If `patterns` is also set, the parent zone
    /// remains as a fallback after discovered child zones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_discover: Vec<String>,
    /// Optional subtree scope for monorepo per-package boundaries.
    ///
    /// When set, the zone's `patterns` are matched against paths *relative*
    /// to this directory rather than the project root. At classification
    /// time, fallow checks that a candidate path starts with `root` and
    /// strips that prefix before glob-matching the patterns against the
    /// remainder. Files outside the subtree never match the zone.
    ///
    /// Useful for monorepos where each package has the same internal
    /// directory layout: instead of writing `packages/app/src/**` and
    /// `packages/core/src/**` (which collide on shared zone names), set
    /// `root: "packages/app/"` and `patterns: ["src/**"]` per package.
    ///
    /// Trailing slash and leading `./` are normalized; backslashes are
    /// converted to forward slashes. Patterns must NOT redundantly include
    /// the root prefix: `root: "packages/app/"` with
    /// `patterns: ["packages/app/src/**"]` is rejected with
    /// `FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX` because patterns are
    /// resolved relative to the root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
}

/// An import rule between zones.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryRule {
    /// The zone this rule applies to (the importing side).
    pub from: String,
    /// Zones that `from` is allowed to import from. Self-imports are always allowed.
    /// An empty list means the zone may not import from any other zone.
    #[serde(default)]
    pub allow: Vec<String>,
    /// Zones that `from` may type-only-import from even when not listed in
    /// `allow`. Mirrors the `allow` shape: a list of target zone names. A
    /// type-only import declaration (`import type {...}`, `import type * as ns`,
    /// or a per-specifier inline `type` qualifier on every named specifier) to a
    /// listed zone is not reported as a boundary violation. Mixed-specifier
    /// imports (`import { type Foo, Bar }`) that carry at least one value
    /// symbol still fire because the runtime dependency on `Bar` is real.
    /// Type-only re-exports (`export type { Foo } from "..."`) participate
    /// in the same allowance because they surface as edges flagged
    /// `is_type_only: true` and, like type-only imports, are erased at
    /// compile time.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_type_only: Vec<String>,
}

/// Resolved boundary config with pre-compiled glob matchers.
#[derive(Debug, Default)]
pub struct ResolvedBoundaryConfig {
    /// Zones with compiled glob matchers for fast file classification.
    pub zones: Vec<ResolvedZone>,
    /// Rules indexed by source zone name.
    pub rules: Vec<ResolvedBoundaryRule>,
    /// Pre-expansion logical groups captured during `expand_auto_discover`,
    /// preserved here for observability (`fallow list --boundaries --format
    /// json`). One entry per `autoDiscover`-bearing zone in user-declaration
    /// order. Empty unless the user (or a preset) wrote at least one
    /// `autoDiscover`. See [`LogicalGroup`] for the per-entry shape.
    pub logical_groups: Vec<LogicalGroup>,
}

/// A user-declared zone that fanned out into one or more child zones via
/// `autoDiscover`. Surfaced verbatim through `fallow list --boundaries
/// --format json` so consumers (config UIs, Sankey renderers, agent-driven
/// config tooling, dashboards) can reconstruct the original grouping intent
/// after expansion has flattened the parent name out of `zones[]`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct LogicalGroup {
    /// Logical parent zone name as authored by the user (e.g. `"features"`).
    pub name: String,
    /// Discovered child zone names in stable directory-sorted order
    /// (e.g. `["features/auth", "features/billing"]`). Empty when the parent
    /// directory was empty or unreadable; `status` discriminates the two.
    pub children: Vec<String>,
    /// The exact `autoDiscover` strings the user wrote, preserved verbatim
    /// (no normalization). Round-trip tooling depends on byte-exact match
    /// against the user's config source.
    pub auto_discover: Vec<String>,
    /// Pre-expansion rule keyed on this parent zone name, captured before
    /// `expand_auto_discover` rewrote it into per-child rules. `None` when
    /// the user wrote no rule for the parent (the children are then
    /// unrestricted unless a per-child rule exists).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authored_rule: Option<AuthoredRule>,
    /// When the parent zone also carried explicit `patterns`, it stayed in
    /// `zones[]` after expansion as a fallback classifier. This is its name
    /// (always equal to [`Self::name`]). `None` when the parent had no
    /// patterns and was dropped from `zones[]` entirely. Lets consumers wire
    /// the logical-group entry to its zone twin without name-matching
    /// heuristics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_zone: Option<String>,
    /// Position of the parent zone in the user's pre-expansion `zones[]`
    /// array. Enables byte-accurate config patches by agent tooling without
    /// re-parsing the user's config source.
    pub source_zone_index: usize,
    /// Why [`Self::children`] is what it is.
    pub status: LogicalGroupStatus,
    /// Parent zone indices whose declarations were merged into this group
    /// because they shared a name (`{ name: "features", autoDiscover: [...] }`
    /// declared twice). `None` on the common case (single declaration);
    /// `Some([i, j, ...])` when at least two declarations were merged. The
    /// FIRST entry equals [`Self::source_zone_index`]; subsequent entries are
    /// the positions of the additional declarations in user-declaration order.
    /// Surfaced in JSON so consumers (config-edit agents, config-hygiene
    /// dashboards) can detect duplicates that `tracing::warn!` would otherwise
    /// hide from `--format json` consumption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merged_from: Option<Vec<usize>>,
    /// The parent zone's `root` (subtree scope) as the user authored it,
    /// echoed onto the logical group so monorepo-aware tooling can tell
    /// whether `root` was set on the parent (and inherited by every
    /// discovered child) or set per-child. `None` when the parent had no
    /// `root` field. The string is verbatim from the user's config (not
    /// the post-`normalize_zone_root` form) for byte-exact round-trip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_zone_root: Option<String>,
    /// For each entry in [`Self::children`], the index into
    /// [`Self::auto_discover`] of the path that produced it (or the FIRST
    /// path that produced it when multiple `autoDiscover` entries each yield
    /// the same child name). Empty when only one `autoDiscover` path was
    /// authored (every child trivially maps to index 0); populated only when
    /// the parent has two or more `autoDiscover` entries so consumers can
    /// attribute children to specific source directories. The length equals
    /// `children.len()` when populated.
    ///
    /// `#[serde(default)]` pairs with `skip_serializing_if` so the JSON
    /// runtime omits this field on the common single-path case AND the
    /// derived schema marks it optional (schemars 1 promotes any field with a
    /// `serde(default)` attribute out of `required`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_source_indices: Vec<usize>,
}

/// Discovery outcome for a [`LogicalGroup`]. Discriminates "no children" into
/// "the directory exists and is empty" versus "at least one `autoDiscover`
/// path was invalid or unreadable", so consumers can render an actionable
/// hint instead of "0 children, mystery".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogicalGroupStatus {
    /// At least one child zone was discovered.
    Ok,
    /// Every `autoDiscover` path resolved to a readable directory, but
    /// none contained child directories.
    Empty,
    /// At least one `autoDiscover` path was malformed (contained `..`,
    /// absolute) or did not resolve to a readable directory, and zero
    /// children were discovered across all paths. When a mix of invalid and
    /// valid paths produces children, status is [`Self::Ok`] instead.
    InvalidPath,
}

/// Pre-expansion `from`-rule preserved on a [`LogicalGroup`]. Surfaces the
/// user's original intent (`{ from: "features", allow: ["shared"] }`) even
/// after `expand_auto_discover` rewrote it into per-child rules
/// (`features/auth -> shared`, `features/billing -> shared`).
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct AuthoredRule {
    /// Pre-expansion `allow` list as the user wrote it.
    pub allow: Vec<String>,
    /// Pre-expansion `allowTypeOnly` list as the user wrote it. Omitted
    /// from JSON output when empty; `serde(default)` keeps the derived
    /// schema in lock-step (schemars 1 marks any field with a
    /// `serde(default)` attribute as non-required).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow_type_only: Vec<String>,
}

/// A zone with pre-compiled glob matchers.
#[derive(Debug)]
pub struct ResolvedZone {
    /// Zone identifier.
    pub name: String,
    /// Pre-compiled glob matchers for zone membership.
    /// When `root` is set, matchers are applied to the path with the
    /// `root` prefix stripped (subtree-relative patterns).
    pub matchers: Vec<globset::GlobMatcher>,
    /// Normalized subtree scope (e.g. `"packages/app/"`). When present,
    /// only paths starting with this prefix can match this zone, and the
    /// prefix is stripped before glob matching. Forward slashes only,
    /// always trailing slash. `None` means patterns are matched against
    /// the project-root-relative path as-is.
    pub root: Option<String>,
}

/// A resolved boundary rule.
#[derive(Debug)]
pub struct ResolvedBoundaryRule {
    /// The zone this rule restricts.
    pub from_zone: String,
    /// Zones that `from_zone` is allowed to import from.
    pub allowed_zones: Vec<String>,
    /// Zones that `from_zone` may type-only-import from even when not listed
    /// in `allowed_zones`. See `BoundaryRule::allow_type_only`.
    pub allow_type_only_zones: Vec<String>,
}

impl BoundaryConfig {
    /// Whether any boundaries are configured (including via preset).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.preset.is_none() && self.zones.is_empty()
    }

    /// Expand the preset (if set) into zones and rules, merging user overrides on top.
    ///
    /// `source_root` is the directory prefix for preset zone patterns (e.g., `"src"`).
    /// After expansion, `self.preset` is cleared and all zones/rules are explicit.
    ///
    /// Merge semantics:
    /// - User zones with the same name as a preset zone **replace** the preset zone entirely.
    /// - User rules with the same `from` as a preset rule **replace** the preset rule.
    /// - User zones/rules with new names **add** to the preset set.
    pub fn expand(&mut self, source_root: &str) {
        let Some(preset) = self.preset.take() else {
            return;
        };

        let (preset_zones, preset_rules) = preset.default_config(source_root);

        // Build set of user-defined zone names for override detection.
        let user_zone_names: rustc_hash::FxHashSet<&str> =
            self.zones.iter().map(|z| z.name.as_str()).collect();

        // Start with preset zones, replacing any that the user overrides.
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
        // Append all user zones (both overrides and additions).
        merged_zones.append(&mut self.zones);
        self.zones = merged_zones;

        // Build set of user-defined rule `from` names for override detection.
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

    /// Expand auto-discovered boundary groups into concrete child zones.
    ///
    /// A zone with `autoDiscover: ["src/features"]` discovers the immediate
    /// child directories below `src/features` and emits child zones named
    /// `zone_name/child`. Rules that reference the logical parent are expanded
    /// to all discovered children. If the parent also has explicit `patterns`,
    /// it is kept after the children as a fallback so child directories remain
    /// isolated by first-match classification. The parent fallback rule
    /// automatically allows its discovered children so top-level barrels can
    /// re-export child modules without relaxing sibling isolation on the child
    /// rules.
    ///
    /// Returns one [`LogicalGroup`] per pre-expansion zone that carried a
    /// non-empty `autoDiscover`, in user-declaration order. The caller (the
    /// resolution pipeline) stashes the result onto
    /// [`ResolvedBoundaryConfig::logical_groups`] for `fallow list
    /// --boundaries --format json` to render. Discarding the return is fine
    /// for callers that only need the expansion side effect (classification);
    /// the data is regenerated on the next run.
    ///
    /// Duplicate parent zone name behavior: when two `BoundaryZone`
    /// declarations share a name and both carry `autoDiscover`, their
    /// discovered children merge into a single `LogicalGroup` whose
    /// `auto_discover` concatenates both source path lists in declaration
    /// order. This mirrors the existing rule-side merge behavior (both rules
    /// expand to the same union of child names). A `tracing::warn!` surfaces
    /// the duplicate at config-load time so the user can deduplicate the
    /// source; the merged behavior is a soft default rather than a hard
    /// rejection so existing configs continue to load.
    pub fn expand_auto_discover(&mut self, project_root: &Path) -> Vec<LogicalGroup> {
        if self.zones.iter().all(|zone| zone.auto_discover.is_empty()) {
            return Vec::new();
        }

        let original_zones = std::mem::take(&mut self.zones);
        let mut expanded_zones = Vec::new();
        let mut group_expansions: rustc_hash::FxHashMap<String, Vec<String>> =
            rustc_hash::FxHashMap::default();
        // Preserves user-declaration order: `FxHashMap` iteration is not
        // insertion-ordered, and consumers (snapshot tests, diff-based
        // dashboards) depend on stable JSON output across runs.
        let mut group_drafts: Vec<LogicalGroupDraft> = Vec::new();

        for (source_zone_index, mut zone) in original_zones.into_iter().enumerate() {
            if zone.auto_discover.is_empty() {
                expanded_zones.push(zone);
                continue;
            }

            let group_name = zone.name.clone();
            // Capture the user's verbatim `autoDiscover` strings before
            // discovery normalizes them; round-trip tooling depends on
            // byte-exact match against the source.
            let raw_auto_discover = zone.auto_discover.clone();
            let original_zone_root = zone.root.clone();
            let DiscoveryOutcome {
                zones: discovered_zones,
                source_indices: discovered_source_indices,
                had_invalid_path,
            } = discover_child_zones(project_root, &zone);
            let discovered_count = discovered_zones.len();
            let mut expanded_names: Vec<String> = discovered_zones
                .iter()
                .map(|child| child.name.clone())
                .collect();
            let child_names_only = expanded_names.clone();
            for child_zone in discovered_zones {
                merge_zone_by_name(&mut expanded_zones, child_zone);
            }

            let fallback_zone = if zone.patterns.is_empty() {
                None
            } else {
                expanded_names.push(group_name.clone());
                zone.auto_discover.clear();
                merge_zone_by_name(&mut expanded_zones, zone);
                Some(group_name.clone())
            };

            if !expanded_names.is_empty() {
                group_expansions
                    .entry(group_name.clone())
                    .or_default()
                    .extend(expanded_names);
            }

            let status = if discovered_count > 0 {
                LogicalGroupStatus::Ok
            } else if had_invalid_path {
                LogicalGroupStatus::InvalidPath
            } else {
                LogicalGroupStatus::Empty
            };

            // Merge into existing draft if the user declared the same parent
            // name twice. Concatenates `auto_discover`, dedupes `children`
            // against the existing set so a duplicate declaration discovering
            // the same child does not double-count via `file_count` lookup,
            // preserves the FIRST `source_zone_index` and `original_zone_root`,
            // shifts the new batch's `child_source_indices` by the existing
            // `auto_discover.len()` so they continue to address the
            // post-concatenation array (and drops indices for children
            // already present, since attribution belongs to the first
            // producer), and appends the new `source_zone_index` to
            // `merged_from` so the duplicate is visible in JSON output.
            if let Some(existing) = group_drafts.iter_mut().find(|d| d.name == group_name) {
                tracing::warn!(
                    "boundary zone '{}' is declared multiple times with autoDiscover; merging discovered children",
                    group_name
                );
                let auto_discover_offset = existing.auto_discover.len();
                existing.auto_discover.extend(raw_auto_discover);
                let existing_children: rustc_hash::FxHashSet<String> =
                    existing.children.iter().cloned().collect();
                for (idx, name) in child_names_only.iter().enumerate() {
                    if existing_children.contains(name) {
                        continue;
                    }
                    existing.children.push(name.clone());
                    existing
                        .child_source_indices
                        .push(discovered_source_indices[idx] + auto_discover_offset);
                }
                if existing.fallback_zone.is_none() {
                    existing.fallback_zone = fallback_zone;
                }
                existing.status = merge_status(existing.status, status);
                let chain = existing
                    .merged_from
                    .get_or_insert_with(|| vec![existing.source_zone_index]);
                chain.push(source_zone_index);
            } else {
                group_drafts.push(LogicalGroupDraft {
                    name: group_name,
                    children: child_names_only,
                    auto_discover: raw_auto_discover,
                    fallback_zone,
                    source_zone_index,
                    status,
                    merged_from: None,
                    original_zone_root,
                    child_source_indices: discovered_source_indices,
                });
            }
        }

        self.zones = expanded_zones;

        // Index draft names so we can look up the authored rule per logical
        // group regardless of whether the group produced any children.
        // Groups whose discovery was Empty / InvalidPath contribute NO entry
        // to `group_expansions` (no children means no rule expansion), but
        // their authored rule still belongs on the surfaced LogicalGroup so
        // consumers see the user's intent even when discovery turned up
        // empty.
        let draft_names: rustc_hash::FxHashSet<&str> =
            group_drafts.iter().map(|d| d.name.as_str()).collect();

        // Capture authored rules BEFORE `original_rules` is consumed below.
        // The match-up is by `rule.from == group_name`; the last matching
        // rule wins to mirror `dedupe_rules_keep_last` semantics.
        let original_rules = std::mem::take(&mut self.rules);
        let authored_rules: rustc_hash::FxHashMap<&str, AuthoredRule> = original_rules
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
            .collect();

        let logical_groups: Vec<LogicalGroup> = group_drafts
            .into_iter()
            .map(|draft| {
                // `child_source_indices` is only signal-bearing when the
                // parent has two or more `auto_discover` paths; with one
                // path every child trivially has index 0. Skip the noise
                // on the common case so the JSON stays tight; the field
                // is `#[serde(skip_serializing_if = "Vec::is_empty")]`.
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
            .collect();

        if group_expansions.is_empty() {
            // No groups produced any children, so rule expansion is a no-op;
            // restore the rules verbatim. `logical_groups` still carries the
            // Empty / InvalidPath drafts so consumers can render the user's
            // grouping intent and act on the "discovery turned up nothing"
            // signal.
            self.rules = original_rules;
            return logical_groups;
        }

        self.rules = expand_rules_for_groups(original_rules, &group_expansions);
        logical_groups
    }
}

/// Merge a discovered (or fallback) zone into the post-expansion zones
/// vector by name. A naive `expanded_zones.push(zone)` duplicates entries
/// when the user declared the same parent name twice (each iteration of the
/// outer expansion loop re-runs discovery on its own `autoDiscover` paths
/// and would push the same child names again, producing duplicates in
/// `zones[]` AND triggering the `file_count` summation in
/// `compute_boundary_data` to double-count each child). Merging by name
/// keeps `zones[]` unique and unifies the patterns from both declarations
/// on the same `BoundaryZone`. Existing patterns are preserved verbatim;
/// only NEW patterns are appended.
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

/// Rewrite the user's pre-expansion rules to reference the discovered child
/// zones in place of the logical parent. Three rule shapes are produced:
///
/// 1. Rules whose `from` is the parent group expand into one explicit rule
///    per child (or one for the parent fallback when the parent kept its
///    `patterns`).
/// 2. Rules whose `allow` references a group expand to allow every child
///    of that group.
/// 3. Rules untouched by group expansion pass through unchanged.
///
/// Extracted out of [`BoundaryConfig::expand_auto_discover`] so the
/// orchestrator stays under the SIG unit-size threshold; the body itself
/// is unchanged from the pre-#373 inline form.
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

    /// Validate that no zone's pattern redundantly includes its `root`
    /// prefix. Patterns are resolved relative to the zone root, so prefixing
    /// the pattern with the same root double-prefixes the path and never
    /// matches.
    ///
    /// The rendered diagnostic carries the legacy
    /// `FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX` tag via
    /// [`ZoneValidationError`]'s `Display` impl, so CI logs grepping for the
    /// old text continue to work.
    #[must_use]
    pub fn validate_root_prefixes(&self) -> Vec<RedundantRootPrefix> {
        let mut errors = Vec::new();
        for zone in &self.zones {
            let Some(raw_root) = zone.root.as_deref() else {
                continue;
            };
            let normalized = normalize_zone_root(raw_root);
            // Skip empty-root zones: `""`, `"."`, and `"./"` all normalize to
            // `""`, which behaves as no root at classification time. Without
            // this guard `starts_with("")` is always true and every pattern
            // produces a spurious redundant-prefix error.
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

    /// Validate that all zone names referenced in rules are defined in `zones`.
    ///
    /// Walks every zone-reference surface on `BoundaryRule`: `from`, `allow`,
    /// and `allow_type_only`. An unknown zone in `allow_type_only` silently
    /// behaves as "not allowed" at runtime, so it MUST surface here for parity
    /// with the existing `allow`-side diagnostic.
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
        errors
    }

    /// Resolve into compiled form with pre-built glob matchers.
    ///
    /// User patterns were validated at config load time
    /// (see `FallowConfig::validate_user_globs`).
    #[must_use]
    pub fn resolve(&self) -> ResolvedBoundaryConfig {
        let zones = self
            .zones
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
                    matchers,
                    root,
                }
            })
            .collect();

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
            zones,
            rules,
            // `expand_auto_discover` is the only producer; the resolution
            // pipeline (`crates/config/src/config/resolution.rs`) assigns the
            // returned `Vec<LogicalGroup>` onto the resolved boundaries after
            // `resolve()` runs. `resolve()` itself has no view of the
            // pre-expansion state, so it leaves the field empty here.
            logical_groups: Vec::new(),
        }
    }
}

/// Normalize a zone `root` string into the canonical form used at
/// classification time: forward slashes, no leading `./`, always a
/// trailing slash. Empty / `"."` / `"./"` collapse to `""` which means
/// "subtree is the project root" and effectively behaves like no root.
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

/// Discovery result for a single auto-discover zone. Carries the discovered
/// child `BoundaryZone`s, a flag for "at least one `autoDiscover` path was
/// malformed or unreadable" (distinguishes [`LogicalGroupStatus::InvalidPath`]
/// from [`LogicalGroupStatus::Empty`]), and parallel-to-zones
/// `source_indices` recording which `autoDiscover` entry produced each child
/// (FIRST producer wins when two paths yield the same child name).
struct DiscoveryOutcome {
    zones: Vec<BoundaryZone>,
    source_indices: Vec<usize>,
    had_invalid_path: bool,
}

/// Intermediate accumulator for a [`LogicalGroup`] before its
/// [`AuthoredRule`] is resolved (rules are not consumed until after the zone
/// loop completes, so the rule lookup happens in a second pass).
struct LogicalGroupDraft {
    name: String,
    children: Vec<String>,
    auto_discover: Vec<String>,
    fallback_zone: Option<String>,
    source_zone_index: usize,
    status: LogicalGroupStatus,
    /// `None` until a second declaration with the same `name` is merged in;
    /// then `Some(vec![first_index, ..])` with one entry per merged
    /// declaration in user-declaration order.
    merged_from: Option<Vec<usize>>,
    /// Echo of the parent zone's `root` field as the user authored it
    /// (verbatim, not normalized). On duplicate-merge, the FIRST declaration
    /// wins (consistent with `source_zone_index`).
    original_zone_root: Option<String>,
    /// Parallel to `children`: for child at index `i`, the index into
    /// `auto_discover` of the path that produced it (FIRST producer wins on
    /// collisions). When merging duplicate parent declarations, indices from
    /// the second batch are shifted by the first batch's `auto_discover.len()`
    /// so they continue to address the concatenated `auto_discover` array.
    child_source_indices: Vec<usize>,
}

/// Merge two `LogicalGroupStatus` values when a duplicate parent zone name
/// is encountered: `Ok` wins (at least one child was discovered),
/// `InvalidPath` beats `Empty` (a malformed/unreadable path is a louder
/// signal than "no subdirs"), and otherwise we keep the existing status.
const fn merge_status(existing: LogicalGroupStatus, new: LogicalGroupStatus) -> LogicalGroupStatus {
    match (existing, new) {
        (LogicalGroupStatus::Ok, _) | (_, LogicalGroupStatus::Ok) => LogicalGroupStatus::Ok,
        (LogicalGroupStatus::InvalidPath, _) | (_, LogicalGroupStatus::InvalidPath) => {
            LogicalGroupStatus::InvalidPath
        }
        (LogicalGroupStatus::Empty, LogicalGroupStatus::Empty) => LogicalGroupStatus::Empty,
    }
}

fn discover_child_zones(project_root: &Path, zone: &BoundaryZone) -> DiscoveryOutcome {
    let mut zones_by_name: rustc_hash::FxHashMap<String, BoundaryZone> =
        rustc_hash::FxHashMap::default();
    // Tracks which `autoDiscover` path index FIRST produced each child zone
    // name. When two paths yield the same child name, the first producer
    // wins (the merged `BoundaryZone` accumulates patterns from both but
    // attribution stays stable).
    let mut first_source_index: rustc_hash::FxHashMap<String, usize> =
        rustc_hash::FxHashMap::default();
    let normalized_root = zone
        .root
        .as_deref()
        .map(normalize_zone_root)
        .unwrap_or_default();
    let mut had_invalid_path = false;

    for (source_index, raw_dir) in zone.auto_discover.iter().enumerate() {
        let Some(discover_dir) = normalize_auto_discover_dir(raw_dir) else {
            tracing::warn!(
                "invalid boundary autoDiscover path '{}' in zone '{}': paths must be project-relative and must not contain '..'",
                raw_dir,
                zone.name
            );
            had_invalid_path = true;
            continue;
        };

        let fs_relative = join_relative_path(&normalized_root, &discover_dir);
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
            had_invalid_path = true;
            continue;
        };

        let mut children: Vec<_> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|file_type| file_type.is_dir()))
            .collect();
        children.sort_by_key(|entry| entry.file_name());

        for child in children {
            let child_name = child.file_name().to_string_lossy().to_string();
            if child_name.is_empty() {
                continue;
            }

            let zone_name = format!("{}/{}", zone.name, child_name);
            let child_pattern = format!("{}/**", join_relative_path(&discover_dir, &child_name));
            let entry = zones_by_name
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
            first_source_index.entry(zone_name).or_insert(source_index);
        }
    }

    let mut zones: Vec<_> = zones_by_name.into_values().collect();
    zones.sort_by(|a, b| a.name.cmp(&b.name));
    let source_indices: Vec<usize> = zones
        .iter()
        .map(|z| {
            // Every entry inserted into `zones_by_name` was also inserted
            // into `first_source_index` in the same loop body, so this lookup
            // is infallible. Fall back to 0 defensively for any future
            // refactor that decouples the two maps.
            first_source_index
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
    ///
    /// Considers `logical_groups` too: when every `autoDiscover` zone
    /// produced zero children, `zones` is empty but the user authored a
    /// boundaries section that should still be surfaced (so `fallow list
    /// --boundaries` can render the `Empty` / `InvalidPath` status to the
    /// user). Without this, the whole boundaries block silently disappears
    /// from the output the moment discovery finds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.zones.is_empty() && self.logical_groups.is_empty()
    }

    /// Classify a file path into a zone. Returns the first matching zone name.
    /// Path should be relative to the project root with forward slashes.
    ///
    /// When a zone declares a `root` (subtree scope), the path must start
    /// with that prefix and the prefix is stripped before glob matching;
    /// otherwise the zone is skipped. Zones without a `root` keep
    /// project-root-relative behavior.
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

    /// Check if an import from `from_zone` to `to_zone` is allowed.
    /// Returns `true` if the import is permitted.
    #[must_use]
    pub fn is_import_allowed(&self, from_zone: &str, to_zone: &str) -> bool {
        // Self-imports are always allowed.
        if from_zone == to_zone {
            return true;
        }

        // Find the rule for the source zone.
        let rule = self.rules.iter().find(|r| r.from_zone == from_zone);

        match rule {
            // Zone has no rule entry — unrestricted.
            None => true,
            // Zone has a rule — check the allowlist.
            Some(r) => r.allowed_zones.iter().any(|z| z == to_zone),
        }
    }

    /// Check whether a type-only import from `from_zone` to `to_zone` is
    /// permitted by the rule's `allowTypeOnly` list. Only consulted by the
    /// boundary detector after `is_import_allowed` has already returned
    /// `false`; the caller is responsible for verifying the import is in
    /// fact type-only (all symbols on the edge carry the type-only flag).
    /// Returns `false` when no rule exists for `from_zone`, since rule-less
    /// zones are unrestricted and `is_import_allowed` short-circuits before
    /// this is called.
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
    fn auto_discover_expands_child_zones_and_parent_rules() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
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

    // ── LogicalGroup return value (issue #373) ──────────────────

    #[test]
    fn logical_groups_returned_for_simple_auto_discover_zone() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
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
        // Parent had no explicit patterns → not retained as fallback.
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
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                // Trailing slash + leading `./` are normalized during discovery
                // but the logical group must echo the user's literal string so
                // round-trip config tooling does not introduce spurious diffs.
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
            preset: None,
            zones: vec![BoundaryZone {
                // Bulletproof shape: parent carries BOTH patterns AND
                // autoDiscover, so the parent stays in zones[] as a fallback
                // classifier while ALSO becoming a logical group.
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
        // Parent zone is still present in zones[] as the fallback classifier.
        assert!(config.zones.iter().any(|z| z.name == "features"));
    }

    #[test]
    fn logical_groups_status_empty_when_no_child_dirs() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features")).unwrap();
        // No child subdirs created.

        let mut config = BoundaryConfig {
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
        // src/features intentionally not created.

        let mut config = BoundaryConfig {
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
        // src/modules intentionally not created (invalid path).

        let mut config = BoundaryConfig {
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
        // One path produced children → status is Ok even though another path
        // was invalid. The InvalidPath warning still surfaces via tracing.
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
        // Insertion order is preserved; not alphabetized.
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["zeta", "alpha", "mid"]);
    }

    #[test]
    fn logical_groups_merged_from_records_duplicate_indices() {
        // The single-declaration path leaves merged_from None; the
        // duplicate-merge path populates it with every contributing index.
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/extra/billing")).unwrap();

        let mut config = BoundaryConfig {
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
        // merged_from holds both contributing zone indices in declaration
        // order: position 0 and position 2 (the "other" zone at position 1
        // is unrelated).
        assert_eq!(groups[0].merged_from.as_deref(), Some(&[0_usize, 2][..]));
        // The first index also wins source_zone_index.
        assert_eq!(groups[0].source_zone_index, 0);
    }

    #[test]
    fn logical_groups_merged_from_none_on_single_declaration() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
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
        // Common case: no duplicate, no merged_from.
        assert!(groups[0].merged_from.is_none());
    }

    #[test]
    fn logical_groups_echo_original_zone_root() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("packages/app/src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                auto_discover: vec!["src/features".to_string()],
                // Monorepo subtree scope on the parent; should round-trip
                // verbatim to logical_groups[0].original_zone_root so
                // patcher tools can distinguish parent-set vs per-child root.
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
            preset: None,
            zones: vec![BoundaryZone {
                name: "features".to_string(),
                patterns: vec![],
                // Two paths: each produces one child. Children are
                // alphabetically sorted across paths, so auth (from index 0)
                // sorts before billing (from index 1).
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
        // With one path, every child trivially has source index 0. The
        // helper field is suppressed (empty Vec) so the JSON stays tight
        // on the common case.
        assert!(groups[0].child_source_indices.is_empty());
    }

    #[test]
    fn logical_groups_child_source_indices_after_duplicate_merge_shifted() {
        // When two parent declarations merge, the child indices from the
        // SECOND batch must be shifted by the FIRST batch's
        // auto_discover.len() so they continue to address the
        // post-concatenation `auto_discover` array correctly.
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/extra/billing")).unwrap();

        let mut config = BoundaryConfig {
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
        // Merged auto_discover has 2 entries; index 0 = src/features,
        // index 1 = src/extra. The features/billing child came from the
        // second batch's first path, which post-shift is index 1.
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
        // The two declarations merge into a single logical group with
        // concatenated auto_discover paths and children.
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "features");
        assert_eq!(groups[0].auto_discover, vec!["src/features", "src/extra"]);
        assert!(groups[0].children.iter().any(|c| c == "features/auth"));
        assert!(groups[0].children.iter().any(|c| c == "features/billing"));
        assert_eq!(groups[0].source_zone_index, 0);
    }

    #[test]
    fn logical_groups_duplicate_identical_declarations_no_double_count() {
        // Regression for codex parallel review (post-impl pass): two
        // identical `features` declarations with the same `autoDiscover`
        // path used to emit duplicate `zones[]` entries, duplicate
        // `children[]`, and double-counted `file_count` (4 for 2 real
        // files). `merge_zone_by_name` keeps `zones[]` unique by name and
        // the merge logic dedupes children against the existing set.
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
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
        // zones[] must NOT contain duplicates of features/auth or
        // features/billing.
        let zone_names: Vec<&str> = config.zones.iter().map(|z| z.name.as_str()).collect();
        assert_eq!(zone_names, vec!["features/auth", "features/billing"]);
        // children[] must NOT contain duplicates.
        assert_eq!(
            groups[0].children,
            vec!["features/auth", "features/billing"]
        );
        // auto_discover preserves both verbatim (the duplicate is visible
        // via merged_from + the warning, but the path list itself
        // concatenates).
        assert_eq!(
            groups[0].auto_discover,
            vec!["src/features", "src/features"]
        );
        // merged_from records both zone indices.
        assert_eq!(groups[0].merged_from.as_deref(), Some(&[0_usize, 1][..]));
    }

    #[test]
    fn logical_groups_empty_when_no_auto_discover_present() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = BoundaryConfig {
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
        // End-to-end: data populated by expand_auto_discover survives a
        // round trip through `BoundaryConfig::resolve()` so consumers of
        // `ResolvedBoundaryConfig.logical_groups` see the same content.
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();

        let mut config = BoundaryConfig {
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
        // `resolve()` itself does not have access to the pre-expansion state;
        // the resolution pipeline stitches the groups back on. Mirror that
        // here so the test exercises the same shape consumers see.
        resolved.logical_groups = groups;
        assert_eq!(resolved.logical_groups.len(), 1);
        assert_eq!(resolved.logical_groups[0].name, "features");
        assert_eq!(resolved.logical_groups[0].children, vec!["features/auth"]);
    }

    #[test]
    fn validate_zone_references_valid() {
        let config = BoundaryConfig {
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
        // An undefined zone in `allowTypeOnly` silently behaves as "not
        // allowed" at runtime, which the user almost always meant as a typo
        // for an existing zone. Surface the same diagnostic as `allow`.
        let config = BoundaryConfig {
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
        // Files inside packages/app/ classify as ui
        assert_eq!(
            resolved.classify_zone("packages/app/src/login.tsx"),
            Some("ui")
        );
        // Files inside packages/core/ classify as domain (same pattern, different root)
        assert_eq!(
            resolved.classify_zone("packages/core/src/order.ts"),
            Some("domain")
        );
        // Files outside either subtree do not match
        assert_eq!(resolved.classify_zone("src/login.tsx"), None);
        assert_eq!(resolved.classify_zone("packages/utils/src/x.ts"), None);
    }

    /// Case-sensitivity contract: `root` matching is case-sensitive,
    /// matching the existing globset case-sensitivity for `patterns`. On
    /// case-insensitive filesystems (HFS+, NTFS) two files differing only
    /// in case still classify only when the configured `root` exactly
    /// matches the path's case as fallow recorded it. Locking this down
    /// prevents silent platform-divergent classification.
    #[test]
    fn zone_root_is_case_sensitive() {
        let config = BoundaryConfig {
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
        // Display preserves the legacy FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX
        // tag so existing CI grep recipes continue to work.
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
        // Root without trailing slash + pattern with leading "./" should
        // still be detected as redundant after normalization.
        let config = BoundaryConfig {
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

    /// Regression: an empty `root` (or `"."`/`"./"`, both of which normalize
    /// to `""`) used to make `starts_with("")` always true, producing a
    /// spurious FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX error for every
    /// pattern in the zone. The validation must skip empty-normalized roots
    /// the same way `classify_zone` does.
    #[test]
    fn validate_root_prefixes_skips_empty_root() {
        for raw_root in ["", ".", "./"] {
            let config = BoundaryConfig {
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

    // ── Preset deserialization ─────────────────────────────────

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
            preset: Some(BoundaryPreset::Layered),
            zones: vec![],
            rules: vec![],
        };
        assert!(!config.is_empty());
    }

    // ── Preset expansion ───────────────────────────────────────

    #[test]
    fn expand_layered_produces_four_zones() {
        let mut config = BoundaryConfig {
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
            preset: Some(BoundaryPreset::Layered),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        // presentation → application only
        let pres_rule = config
            .rules
            .iter()
            .find(|r| r.from == "presentation")
            .unwrap();
        assert_eq!(pres_rule.allow, vec!["application"]);
        // application → domain only
        let app_rule = config
            .rules
            .iter()
            .find(|r| r.from == "application")
            .unwrap();
        assert_eq!(app_rule.allow, vec!["domain"]);
        // domain → nothing
        let dom_rule = config.rules.iter().find(|r| r.from == "domain").unwrap();
        assert!(dom_rule.allow.is_empty());
        // infrastructure → domain + application (DI-friendly)
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
            preset: Some(BoundaryPreset::FeatureSliced),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        assert_eq!(config.zones.len(), 6);
        assert_eq!(config.rules.len(), 6);
        // app can import everything below
        let app_rule = config.rules.iter().find(|r| r.from == "app").unwrap();
        assert_eq!(
            app_rule.allow,
            vec!["pages", "widgets", "features", "entities", "shared"]
        );
        // shared imports nothing
        let shared_rule = config.rules.iter().find(|r| r.from == "shared").unwrap();
        assert!(shared_rule.allow.is_empty());
        // entities → shared only
        let ent_rule = config.rules.iter().find(|r| r.from == "entities").unwrap();
        assert_eq!(ent_rule.allow, vec!["shared"]);
    }

    #[test]
    fn expand_bulletproof_produces_four_zones() {
        let mut config = BoundaryConfig {
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
        // shared zone has multiple patterns
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
            preset: Some(BoundaryPreset::Bulletproof),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        // app → features, shared, server
        let app_rule = config.rules.iter().find(|r| r.from == "app").unwrap();
        assert_eq!(app_rule.allow, vec!["features", "shared", "server"]);
        // features → shared, server
        let feat_rule = config.rules.iter().find(|r| r.from == "features").unwrap();
        assert_eq!(feat_rule.allow, vec!["shared", "server"]);
        // server → shared
        let srv_rule = config.rules.iter().find(|r| r.from == "server").unwrap();
        assert_eq!(srv_rule.allow, vec!["shared"]);
        // shared → nothing (isolated)
        let shared_rule = config.rules.iter().find(|r| r.from == "shared").unwrap();
        assert!(shared_rule.allow.is_empty());
    }

    #[test]
    fn expand_bulletproof_then_resolve_classifies() {
        // `expand()` alone (without `expand_auto_discover`) does not produce
        // the per-feature child zones yet, but the parent `features` fallback
        // still classifies top-level and nested `src/features/...` files.
        let mut config = BoundaryConfig {
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
        // features cannot import shared directly — only via allowed rules
        assert!(resolved.is_import_allowed("features", "shared"));
        assert!(resolved.is_import_allowed("features", "server"));
        assert!(!resolved.is_import_allowed("features", "app"));
        assert!(!resolved.is_import_allowed("shared", "features"));
        assert!(!resolved.is_import_allowed("server", "features"));
    }

    /// Regression for the bulletproof barrel pattern: a top-level
    /// `src/features/index.ts` barrel re-exporting child features must NOT
    /// trigger `features → features/<child>` boundary violations. The parent
    /// fallback rule allows discovered children while generated child rules
    /// still enforce sibling isolation.
    #[test]
    fn bulletproof_features_barrel_can_import_children() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/auth")).unwrap();
        std::fs::create_dir_all(temp.path().join("src/features/billing")).unwrap();

        let mut config = BoundaryConfig {
            preset: Some(BoundaryPreset::Bulletproof),
            zones: vec![],
            rules: vec![],
        };
        config.expand("src");
        config.expand_auto_discover(temp.path());
        let resolved = config.resolve();

        // Top-level barrel inside src/features falls back to the parent zone.
        assert_eq!(
            resolved.classify_zone("src/features/index.ts"),
            Some("features"),
            "src/features/index.ts barrel should classify as the parent features zone"
        );
        // Discovered child zones still classify normally.
        assert_eq!(
            resolved.classify_zone("src/features/auth/login.ts"),
            Some("features/auth")
        );
        assert_eq!(
            resolved.classify_zone("src/features/billing/invoice.ts"),
            Some("features/billing")
        );
        // Parent barrels can re-export child features.
        assert!(resolved.is_import_allowed("features", "features/auth"));
        assert!(resolved.is_import_allowed("features", "features/billing"));
        // Sibling-feature import is still a cross-zone violation.
        assert!(!resolved.is_import_allowed("features/auth", "features/billing"));
    }

    #[test]
    fn expand_uses_custom_source_root() {
        let mut config = BoundaryConfig {
            preset: Some(BoundaryPreset::Hexagonal),
            zones: vec![],
            rules: vec![],
        };
        config.expand("lib");
        assert_eq!(config.zones[0].patterns, vec!["lib/adapters/**"]);
        assert_eq!(config.zones[2].patterns, vec!["lib/domain/**"]);
    }

    // ── Preset merge behavior ──────────────────────────────────

    #[test]
    fn user_zone_replaces_preset_zone() {
        let mut config = BoundaryConfig {
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
        // 3 zones total: adapters + ports from preset, domain from user
        assert_eq!(config.zones.len(), 3);
        let domain = config.zones.iter().find(|z| z.name == "domain").unwrap();
        assert_eq!(domain.patterns, vec!["src/core/**"]);
    }

    #[test]
    fn user_zone_adds_to_preset() {
        let mut config = BoundaryConfig {
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
        assert_eq!(config.zones.len(), 4); // 3 preset + 1 user
        assert!(config.zones.iter().any(|z| z.name == "shared"));
    }

    #[test]
    fn user_rule_replaces_preset_rule() {
        let mut config = BoundaryConfig {
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
        // User rule allows both ports and domain (preset only allowed ports)
        assert_eq!(adapter_rule.allow, vec!["ports", "domain"]);
        // Other preset rules untouched
        assert_eq!(
            config.rules.iter().filter(|r| r.from == "adapters").count(),
            1
        );
    }

    #[test]
    fn expand_without_preset_is_noop() {
        let mut config = BoundaryConfig {
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

    // ── ResolvedBoundaryConfig::is_empty ────────────────────────────

    #[test]
    fn resolved_boundary_config_empty() {
        let resolved = ResolvedBoundaryConfig::default();
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolved_boundary_config_with_zones_not_empty() {
        let config = BoundaryConfig {
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
        // Regression for issue #373 smoke: a config whose every autoDiscover
        // zone produced zero children ends up with empty `zones[]` but a
        // populated `logical_groups[]`. The boundaries section must still
        // surface so `fallow list --boundaries` can render the Empty /
        // InvalidPath status (otherwise the whole block silently disappears
        // and the user has no signal that discovery turned up nothing).
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
        };
        assert!(!resolved.is_empty());
    }

    // ── BoundaryConfig::is_empty edge cases ─────────────────────────

    #[test]
    fn boundary_config_with_only_rules_is_empty() {
        // Having rules but no zones/preset is still "empty" since rules without zones
        // cannot produce boundary violations.
        let config = BoundaryConfig {
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

    // ── Multiple zone patterns ──────────────────────────────────────

    #[test]
    fn zone_with_multiple_patterns_matches_any() {
        let config = BoundaryConfig {
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

    // ── validate_zone_references with multiple errors ───────────────

    #[test]
    fn validate_zone_references_multiple_errors() {
        let config = BoundaryConfig {
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
        // Rule 0: invalid "from" + invalid "allow" = 2 errors
        // Rule 1: valid "from", invalid "allow" = 1 error
        assert_eq!(errors.len(), 3);
    }

    // ── Preset expansion with custom source root ────────────────────

    #[test]
    fn expand_feature_sliced_with_custom_root() {
        let mut config = BoundaryConfig {
            preset: Some(BoundaryPreset::FeatureSliced),
            zones: vec![],
            rules: vec![],
        };
        config.expand("lib");
        assert_eq!(config.zones[0].patterns, vec!["lib/app/**"]);
        assert_eq!(config.zones[5].patterns, vec!["lib/shared/**"]);
    }

    // ── is_import_allowed for zone not in rules (unrestricted) ──────

    #[test]
    fn zone_not_in_rules_is_unrestricted() {
        let config = BoundaryConfig {
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
        // "a" is restricted: can import from "b" but not "c"
        assert!(resolved.is_import_allowed("a", "b"));
        assert!(!resolved.is_import_allowed("a", "c"));
        // "b" has no rule entry: unrestricted
        assert!(resolved.is_import_allowed("b", "a"));
        assert!(resolved.is_import_allowed("b", "c"));
        // "c" has no rule entry: unrestricted
        assert!(resolved.is_import_allowed("c", "a"));
    }

    // ── Preset serialization/deserialization roundtrip ───────────────

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

    // ── Zone with invalid glob ──────────────────────────────────────

    #[test]
    #[should_panic(expected = "validated at config load time")]
    fn resolve_panics_on_unvalidated_invalid_zone_glob() {
        // Per issue #463, boundaries.zones[].patterns are validated by
        // FallowConfig::load before reaching resolve(). A program that
        // constructs a config in-code with an invalid pattern has skipped
        // that validation; resolve() asserts the invariant by panicking.
        let config = BoundaryConfig {
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
