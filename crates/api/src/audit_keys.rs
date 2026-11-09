use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

#[path = "audit_keys_collection.rs"]
mod collection;
#[path = "audit_keys_styling.rs"]
mod styling;

pub use styling::{
    styling_finding_key, styling_keys, styling_ledger_keys, styling_ledger_keys_with_diff,
};

use collection::AuditCollection;

use fallow_config::{ResolvedConfig, Severity};
use fallow_types::envelope::AuditIntroduced;
use fallow_types::identity::{IdentifiedFinding, IdentityPaths, dead_code_occurrence_keys};

pub use crate::audit_complexity::{
    ComplexityBaseline, ComplexityClass, ComplexityMetrics, classify_complexity_attribution,
    classify_complexity_attribution_against_all, classify_complexity_findings,
    classify_complexity_findings_against_all, complexity_baseline, complexity_baseline_keys,
    complexity_ledger_keys, remap_complexity_baseline_for_renames,
};

/// One dead-code finding classified for audit comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditFindingRecord {
    /// JSON collection containing the finding.
    pub collection: &'static str,
    /// Stable position inside the collection for typed annotation routing.
    pub ordinal: usize,
    /// Stable cross-run identity used for base comparison.
    pub stable_key: String,
    /// Whether the finding is absent from the base snapshot.
    pub introduced: bool,
    /// Effective rule severity after per-file overrides.
    pub effective_severity: Severity,
}

fn is_counted_dead_code_finding(collection: &str, severity: Severity) -> bool {
    severity != Severity::Off
        && (!fallow_types::results::ERROR_ONLY_RESULT_KEYS.contains(&collection)
            || severity == Severity::Error)
}

/// Exhaustive dead-code comparison ledger shared by audit verdict and output.
#[derive(Debug, Clone, Default)]
pub struct DeadCodeAuditLedger {
    records: Vec<AuditFindingRecord>,
    keys: FxHashSet<String>,
    introduced_keys: FxHashSet<String>,
    inherited_keys: FxHashSet<String>,
    #[cfg(test)]
    classifications: usize,
}

impl DeadCodeAuditLedger {
    /// Classified findings in deterministic output order.
    #[must_use]
    pub fn records(&self) -> &[AuditFindingRecord] {
        &self.records
    }

    /// Stable current-run key set for cache snapshots and graph hashes.
    #[must_use]
    pub const fn keys(&self) -> &FxHashSet<String> {
        &self.keys
    }

    #[cfg(test)]
    const fn classification_count(&self) -> usize {
        self.classifications
    }

    /// Number of visible findings introduced since the base snapshot.
    #[must_use]
    pub fn introduced_count(&self) -> usize {
        self.introduced_keys.len()
    }

    /// Number of counted findings whose effective severity is not off.
    /// Advisory component findings remain available in the records.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.records
            .iter()
            .filter(|record| {
                is_counted_dead_code_finding(record.collection, record.effective_severity)
            })
            .count()
    }

    /// Number of visible findings inherited from the base snapshot.
    #[must_use]
    pub fn inherited_count(&self) -> usize {
        self.inherited_keys.len()
    }

    /// Whether an introduced finding has effective error severity.
    #[must_use]
    pub fn has_introduced_errors(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.introduced && record.effective_severity == Severity::Error)
    }
    /// Whether an introduced finding has effective warning severity.
    #[must_use]
    pub fn has_introduced_warnings(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.introduced && record.effective_severity == Severity::Warn)
    }
    /// Demote introduced findings that have no syntactic counterpart in the
    /// current head run to inherited (advisory) status.
    ///
    /// Used by the audit's degraded type-aware path: when base and head
    /// semantic identities cannot be compared, attribution diffs the
    /// pre-refinement syntactic key sets. A head finding absent from its own
    /// syntactic set exists only because of semantic evidence and cannot be
    /// attributed against a syntactic base, so it must not fail the new-only
    /// gate. Findings present syntactically keep their classification, so a
    /// genuinely new syntactic finding still gates.
    pub fn demote_unattributable_introductions(&mut self, head_syntactic: &FxHashSet<String>) {
        for record in &mut self.records {
            if record.introduced && !head_syntactic.contains(&record.stable_key) {
                record.introduced = false;
                self.introduced_keys.remove(&record.stable_key);
                if is_counted_dead_code_finding(record.collection, record.effective_severity) {
                    self.inherited_keys.insert(record.stable_key.clone());
                }
            }
        }
    }
    /// Persist comparison membership into existing typed output fields.
    ///
    /// `StaleSuppression` is the sole legacy finding without a typed
    /// `introduced` slot. Its serializer keeps a narrow stable-key fallback.
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive result destructuring and field annotation must stay together so new finding collections require an explicit audit decision"
    )]
    pub fn annotate_results(&self, results: &mut fallow_types::results::AnalysisResults) {
        let fallow_types::results::AnalysisResults {
            shared_state: _,
            unused_files,
            unused_exports,
            unused_types,
            private_type_leaks,
            deprecated_exports_in_use,
            unused_dependencies,
            unused_dev_dependencies,
            unused_optional_dependencies,
            unused_enum_members,
            unused_class_members,
            unused_namespace_members,
            unused_store_members,
            unresolved_imports,
            unlisted_dependencies,
            unlisted_binaries,
            duplicate_exports,
            duplicate_export_aliases,
            type_only_dependencies,
            test_only_dependencies,
            dev_dependencies_in_production,
            circular_dependencies,
            re_export_cycles,
            package_cycles,
            boundary_violations,
            boundary_coverage_violations,
            boundary_call_violations,
            policy_violations,
            stale_suppressions: _stale_suppressions,
            unused_catalog_entries,
            empty_catalog_groups,
            unresolved_catalog_references,
            unused_dependency_overrides,
            misconfigured_dependency_overrides,
            invalid_client_exports,
            mixed_client_server_barrels,
            misplaced_directives,
            unprovided_injects,
            unrendered_components,
            unused_component_props,
            absent_component_props,
            unused_component_emits,
            unused_component_inputs,
            unused_component_outputs,
            unused_svelte_events,
            unused_effect_layers,
            unused_tagged_errors,
            unproduced_catch_tags,
            unused_server_actions,
            unused_load_data_keys,
            unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
            route_collisions,
            dynamic_segment_name_conflicts,
            cascade_hidden: _cascade_hidden,
            cascade: _cascade,
            suppression_count: _suppression_count,
            unused_component_props_exempted: _unused_component_props_exempted,
            active_suppressions: _active_suppressions,
            feature_flags: _feature_flags,
            security_findings: _security_findings,
            security_unresolved_edge_files: _security_unresolved_edge_files,
            security_unresolved_callee_sites: _security_unresolved_callee_sites,
            security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
            prop_drilling_chains,
            thin_wrappers,
            duplicate_prop_shapes,
            export_usages: _export_usages,
            entry_point_summary: _entry_point_summary,
            render_fan_in: _render_fan_in,
            react_component_intel: _react_component_intel,
            semantic_reachable_files: _,
            semantic_framework_contracts: _semantic_framework_contracts,
        } = results;

        macro_rules! annotate {
            ($field:ident, $collection:literal) => {
                for (item, introduced) in $field.iter_mut().zip(
                    self.records
                        .iter()
                        .filter(|record| record.collection == $collection)
                        .map(|record| record.introduced),
                ) {
                    item.introduced = Some(AuditIntroduced(introduced));
                }
            };
        }

        annotate!(prop_drilling_chains, "prop_drilling_chains");
        annotate!(thin_wrappers, "thin_wrappers");
        annotate!(duplicate_prop_shapes, "duplicate_prop_shapes");
        annotate!(unused_files, "unused_files");
        annotate!(unused_exports, "unused_exports");
        annotate!(unused_types, "unused_types");
        annotate!(private_type_leaks, "private_type_leaks");
        annotate!(deprecated_exports_in_use, "deprecated_exports_in_use");
        annotate!(unused_dependencies, "unused_dependencies");
        annotate!(unused_dev_dependencies, "unused_dev_dependencies");
        annotate!(unused_optional_dependencies, "unused_optional_dependencies");
        annotate!(unused_enum_members, "unused_enum_members");
        annotate!(unused_class_members, "unused_class_members");
        annotate!(unused_namespace_members, "unused_namespace_members");
        annotate!(unused_store_members, "unused_store_members");
        annotate!(unresolved_imports, "unresolved_imports");
        annotate!(unlisted_dependencies, "unlisted_dependencies");
        annotate!(unlisted_binaries, "unlisted_binaries");
        annotate!(duplicate_exports, "duplicate_exports");
        annotate!(duplicate_export_aliases, "duplicate_export_aliases");
        annotate!(type_only_dependencies, "type_only_dependencies");
        annotate!(test_only_dependencies, "test_only_dependencies");
        annotate!(
            dev_dependencies_in_production,
            "dev_dependencies_in_production"
        );
        annotate!(circular_dependencies, "circular_dependencies");
        annotate!(re_export_cycles, "re_export_cycles");
        annotate!(package_cycles, "package_cycles");
        annotate!(boundary_violations, "boundary_violations");
        annotate!(boundary_coverage_violations, "boundary_coverage_violations");
        annotate!(boundary_call_violations, "boundary_call_violations");
        annotate!(policy_violations, "policy_violations");
        annotate!(unused_catalog_entries, "unused_catalog_entries");
        annotate!(empty_catalog_groups, "empty_catalog_groups");
        annotate!(
            unresolved_catalog_references,
            "unresolved_catalog_references"
        );
        annotate!(unused_dependency_overrides, "unused_dependency_overrides");
        annotate!(
            misconfigured_dependency_overrides,
            "misconfigured_dependency_overrides"
        );
        annotate!(invalid_client_exports, "invalid_client_exports");
        annotate!(mixed_client_server_barrels, "mixed_client_server_barrels");
        annotate!(misplaced_directives, "misplaced_directives");
        annotate!(unprovided_injects, "unprovided_injects");
        annotate!(unrendered_components, "unrendered_components");
        annotate!(route_collisions, "route_collisions");
        annotate!(
            dynamic_segment_name_conflicts,
            "dynamic_segment_name_conflicts"
        );
        annotate!(unused_component_props, "unused_component_props");
        annotate!(absent_component_props, "absent_component_props");
        annotate!(unused_component_emits, "unused_component_emits");
        annotate!(unused_component_inputs, "unused_component_inputs");
        annotate!(unused_component_outputs, "unused_component_outputs");
        annotate!(unused_svelte_events, "unused_svelte_events");
        annotate!(unused_effect_layers, "unused_effect_layers");
        annotate!(unused_tagged_errors, "unused_tagged_errors");
        annotate!(unproduced_catch_tags, "unproduced_catch_tags");
        annotate!(unused_server_actions, "unused_server_actions");
        annotate!(unused_load_data_keys, "unused_load_data_keys");
    }
}

/// Stable-key membership for one non-dead-code audit domain.
#[derive(Debug, Clone, Default)]
pub struct AuditDomainLedger {
    records: Vec<(String, bool)>,
    keys: FxHashSet<String>,
    introduced_keys: FxHashSet<String>,
    inherited_keys: FxHashSet<String>,
    demoted_keys: FxHashSet<String>,
}

impl AuditDomainLedger {
    /// Compare ordered stable keys against an optional base snapshot.
    #[must_use]
    pub fn compare(
        keys: impl IntoIterator<Item = String>,
        base: Option<&FxHashSet<String>>,
    ) -> Self {
        let mut records = Vec::new();
        let mut unique_keys = FxHashSet::default();
        let mut introduced_keys = FxHashSet::default();
        let mut inherited_keys = FxHashSet::default();
        for key in keys {
            let introduced = base.is_some_and(|base| !base.contains(&key));
            if base.is_some() {
                if introduced {
                    introduced_keys.insert(key.clone());
                } else {
                    inherited_keys.insert(key.clone());
                }
            }
            unique_keys.insert(key.clone());
            records.push((key, introduced));
        }
        Self {
            records,
            keys: unique_keys,
            introduced_keys,
            inherited_keys,
            demoted_keys: FxHashSet::default(),
        }
    }

    /// Build a ledger from keys that the caller already classified, in
    /// typed output order. `has_base` is false when the run has no base
    /// snapshot, so no finding counts as introduced or inherited.
    #[must_use]
    pub fn from_classified(
        records: impl IntoIterator<Item = (String, bool)>,
        has_base: bool,
    ) -> Self {
        let mut ledger = Self::default();
        for (key, introduced) in records {
            let introduced = has_base && introduced;
            if has_base {
                if introduced {
                    ledger.introduced_keys.insert(key.clone());
                } else {
                    ledger.inherited_keys.insert(key.clone());
                }
            }
            ledger.keys.insert(key.clone());
            ledger.records.push((key, introduced));
        }
        ledger
    }

    /// Stable current-run key set.
    #[must_use]
    pub const fn keys(&self) -> &FxHashSet<String> {
        &self.keys
    }

    /// Number of introduced findings.
    #[must_use]
    pub fn introduced_count(&self) -> usize {
        self.introduced_keys.len()
    }

    /// Number of inherited findings.
    #[must_use]
    pub fn inherited_count(&self) -> usize {
        self.inherited_keys.len()
    }

    /// Introduced membership in typed output order.
    pub fn introduced(&self) -> impl ExactSizeIterator<Item = bool> + '_ {
        self.records.iter().map(|(_, introduced)| *introduced)
    }

    /// Number of findings demoted from introduced to inherited. Demoted
    /// findings stay counted as inherited, so
    /// `demoted_count() <= inherited_count()` holds by construction.
    #[must_use]
    pub fn demoted_count(&self) -> usize {
        self.demoted_keys.len()
    }

    /// Keys demoted from introduced to inherited by
    /// [`Self::demote_introductions`].
    #[must_use]
    pub const fn demoted_keys(&self) -> &FxHashSet<String> {
        &self.demoted_keys
    }

    /// Demoted membership in typed output order, parallel to
    /// [`Self::introduced`].
    pub fn demoted(&self) -> impl ExactSizeIterator<Item = bool> + '_ {
        self.records
            .iter()
            .map(|(key, _)| self.demoted_keys.contains(key))
    }

    /// Demote introduced findings whose keys are in `demote` to inherited
    /// (advisory) status so they no longer fail the new-only gate. Each key
    /// actually flipped is recorded so output surfaces can report the
    /// demotion (issue #2220).
    pub fn demote_introductions(&mut self, demote: &FxHashSet<String>) {
        for (key, introduced) in &mut self.records {
            if *introduced && demote.contains(key) {
                *introduced = false;
                self.introduced_keys.remove(key);
                self.inherited_keys.insert(key.clone());
                self.demoted_keys.insert(key.clone());
            }
        }
    }
}

/// One-pass audit comparison shared by attribution, verdict, and annotations.
#[derive(Debug, Clone, Default)]
pub struct AuditComparison {
    /// Dead-code finding ledger with effective severity routing.
    pub dead_code: DeadCodeAuditLedger,
    /// Complexity finding membership.
    pub health: AuditDomainLedger,
    /// Attribution class of each complexity finding, in report order. Empty
    /// without a base snapshot.
    pub health_classes: Vec<crate::audit_complexity::ComplexityClass>,
    /// Duplication group membership.
    pub dupes: AuditDomainLedger,
    /// Styling finding membership.
    pub styling: AuditDomainLedger,
}

impl AuditComparison {
    /// Number of complexity findings whose attribution is `worsened`.
    #[must_use]
    pub fn complexity_worsened_count(&self) -> usize {
        self.health_classes
            .iter()
            .filter(|class| class.is_worsened())
            .count()
    }

    /// Reclassify worsened complexity findings as inherited, for
    /// `audit.worsened: off`. Growth of an exceeded metric stays worsened, see
    /// [`ComplexityClass::clear_worsened_for_off`].
    pub fn clear_worsened(&mut self) {
        for class in &mut self.health_classes {
            class.clear_worsened_for_off();
        }
    }
}

/// Remap root-relative path tokens inside base-snapshot attribution keys so a
/// finding on a renamed file matches its base counterpart under the old path.
///
/// `renames` maps base-relative old paths to head-relative new paths, both in
/// [`relative_key_path`] form. Keys embed paths as whole `:`-separated
/// segments, with sorted `|`-separated path lists inside one segment for
/// multi-file findings (duplication groups, circular dependencies, duplicate
/// exports). Segments are remapped only on an exact path match, and
/// multi-path list segments are re-sorted after remapping so they stay
/// canonical. Non-path segments (names, lines, hashes) never collide with a
/// relative path and pass through unchanged.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashMap/FxHashSet across audit attribution keys"
)]
pub fn remap_keys_for_renames(
    keys: &FxHashSet<String>,
    renames: &FxHashMap<String, String>,
) -> FxHashSet<String> {
    keys.iter()
        .map(|key| remap_key_for_renames(key, renames))
        .collect()
}

pub(crate) fn remap_key_for_renames(key: &str, renames: &FxHashMap<String, String>) -> String {
    if !key
        .split([':', '|'])
        .any(|segment| renames.contains_key(segment))
    {
        return key.to_string();
    }
    let segments: Vec<String> = key
        .split(':')
        .map(|segment| {
            if segment.contains('|') {
                let mut pieces: Vec<&str> = segment
                    .split('|')
                    .map(|piece| renames.get(piece).map_or(piece, String::as_str))
                    .collect();
                pieces.sort_unstable();
                pieces.dedup();
                pieces.join("|")
            } else {
                renames
                    .get(segment)
                    .cloned()
                    .unwrap_or_else(|| segment.to_string())
            }
        })
        .collect();
    segments.join(":")
}

/// Canonical path form used inside attribution keys. The engine owns the one
/// implementation, because its module-graph maps use the same keys.
pub use fallow_engine::module_graph::relative_key_path;

/// The audit keys of one finding collection, in collection order.
///
/// Each key is the canonical key of the finding (see
/// [`fallow_types::identity`]), so a line shift does not change it. A repeated
/// key gets an occurrence suffix, so the base and the head compare by count:
/// a new second finding with an inherited key is introduced.
fn collection_keys<T: IdentifiedFinding>(items: &[T], root: &Path) -> Vec<String> {
    dead_code_occurrence_keys(items, &IdentityPaths::new(root))
}

/// Build the set of audit attribution keys for all dead-code findings in
/// `results`.
///
/// Each key is a stable string that uniquely identifies one finding across
/// runs (e.g. `unused-file:src/dead.ts`, `unused-export:src/a.ts:Foo`).
/// `retain_introduced_dead_code` and `annotate_dead_code_json` use the same
/// key format to diff the current run against a base snapshot.
///
/// This destructure is deliberately exhaustive: adding a field to
/// `AnalysisResults` must fail compilation here so the author decides
/// explicitly whether the new finding type needs an audit key (add a loop)
/// or has no key representation today (bind with underscore and document why).
///
/// Sibling exhaustive sites: `fallow_engine::changed_files::filter_results_by_changed_files`,
/// The six dependency-related finding slices, bundled so the dependency
/// dispatcher takes one parameter instead of six.
#[derive(Clone, Copy)]
struct DependencyFindingSlices<'a> {
    unused_dependencies: &'a [fallow_types::output_dead_code::UnusedDependencyFinding],
    unused_dev_dependencies: &'a [fallow_types::output_dead_code::UnusedDevDependencyFinding],
    unused_optional_dependencies:
        &'a [fallow_types::output_dead_code::UnusedOptionalDependencyFinding],
    unlisted_dependencies: &'a [fallow_types::output_dead_code::UnlistedDependencyFinding],
    type_only_dependencies: &'a [fallow_types::output_dead_code::TypeOnlyDependencyFinding],
    test_only_dependencies: &'a [fallow_types::output_dead_code::TestOnlyDependencyFinding],
    dev_dependencies_in_production:
        &'a [fallow_types::output_dead_code::DevDependencyInProductionFinding],
}

/// The six framework-specific finding slices, bundled so the framework
/// dispatcher takes one parameter instead of six.
#[derive(Clone, Copy)]
struct FrameworkFindingSlices<'a> {
    unprovided_injects: &'a [fallow_types::output_dead_code::UnprovidedInjectFinding],
    unrendered_components: &'a [fallow_types::output_dead_code::UnrenderedComponentFinding],
    unused_server_actions: &'a [fallow_types::output_dead_code::UnusedServerActionFinding],
    unused_load_data_keys: &'a [fallow_types::output_dead_code::UnusedLoadDataKeyFinding],
    route_collisions: &'a [fallow_types::output_dead_code::RouteCollisionFinding],
    dynamic_segment_name_conflicts:
        &'a [fallow_types::output_dead_code::DynamicSegmentNameConflictFinding],
}

#[derive(Clone, Copy)]
struct ComponentContractFindingSlices<'a> {
    unused_component_props: &'a [fallow_types::output_dead_code::UnusedComponentPropFinding],
    absent_component_props: &'a [fallow_types::output_dead_code::AbsentComponentPropFinding],
    unused_component_emits: &'a [fallow_types::output_dead_code::UnusedComponentEmitFinding],
    unused_component_inputs: &'a [fallow_types::output_dead_code::UnusedComponentInputFinding],
    unused_component_outputs: &'a [fallow_types::output_dead_code::UnusedComponentOutputFinding],
    unused_svelte_events: &'a [fallow_types::output_dead_code::UnusedSvelteEventFinding],
    unused_effect_layers: &'a [fallow_types::output_dead_code::UnusedEffectLayerFinding],
    unused_tagged_errors: &'a [fallow_types::output_dead_code::UnusedTaggedErrorFinding],
    unproduced_catch_tags: &'a [fallow_types::output_dead_code::UnproducedCatchTagFinding],
}

/// `dead_code_keys`, `retain_introduced_dead_code`.
/// Non-exhaustive siblings the compiler will NOT flag (wire manually when a
/// finding type is added): `annotate_dead_code_json` (same key formats, this
/// file) and the per-collection `off` filter branches in
/// `crates/engine/src/dead_code.rs` (`apply_rule_severities`). The severity of
/// each record comes from the `RuleSeverity` table in
/// `crates/engine/src/effective_severity.rs`, whose visitors are exhaustive.
/// TypeScript mirror: `editors/vscode/scripts/codegen-contracts.mjs` derives
/// backwards-compatible aliases from `fallow schema` `ts_alias` rows.
pub fn dead_code_keys(
    results: &fallow_types::results::AnalysisResults,
    root: &Path,
) -> FxHashSet<String> {
    let mut collector = DeadCodeKeyCollector::new(root);
    collector.add_all_findings(results);
    collector.into_keys()
}

/// Build the exhaustive dead-code comparison ledger once for an audit run.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across audit attribution keys"
)]
pub fn dead_code_audit_ledger(
    results: &fallow_types::results::AnalysisResults,
    root: &Path,
    config: &ResolvedConfig,
    base: Option<&FxHashSet<String>>,
) -> DeadCodeAuditLedger {
    let mut collector = DeadCodeKeyCollector::for_comparison(root, config, base);
    collector.add_all_findings(results);
    collector.into_ledger()
}

impl DeadCodeKeyCollector<'_> {
    #[expect(
        clippy::too_many_lines,
        reason = "flat field-by-field destructure of the large AnalysisResults struct (with per-field provenance comments) plus straight-line dispatch; length tracks the field count, not branching"
    )]
    fn add_all_findings(&mut self, results: &fallow_types::results::AnalysisResults) {
        let fallow_types::results::AnalysisResults {
            shared_state: _,
            unused_files,
            unused_exports,
            unused_types,
            private_type_leaks,
            deprecated_exports_in_use,
            unused_dependencies,
            unused_dev_dependencies,
            unused_optional_dependencies,
            unused_enum_members,
            unused_class_members,
            unused_namespace_members,
            unused_store_members,
            unresolved_imports,
            unlisted_dependencies,
            unlisted_binaries,
            duplicate_exports,
            duplicate_export_aliases,
            type_only_dependencies,
            test_only_dependencies,
            dev_dependencies_in_production,
            circular_dependencies,
            re_export_cycles,
            package_cycles,
            boundary_violations,
            boundary_coverage_violations,
            boundary_call_violations,
            policy_violations,
            stale_suppressions,
            unused_catalog_entries,
            empty_catalog_groups,
            unresolved_catalog_references,
            unused_dependency_overrides,
            misconfigured_dependency_overrides,
            invalid_client_exports,
            mixed_client_server_barrels,
            misplaced_directives,
            unprovided_injects,
            unrendered_components,
            unused_component_props,
            absent_component_props,
            unused_component_emits,
            unused_component_inputs,
            unused_component_outputs,
            unused_svelte_events,
            unused_effect_layers,
            unused_tagged_errors,
            unproduced_catch_tags,
            unused_server_actions,
            unused_load_data_keys,
            unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
            route_collisions,
            dynamic_segment_name_conflicts,
            // Non-finding fields: counts and metadata, not attributable to a key.
            cascade_hidden: _cascade_hidden,
            cascade: _cascade,
            suppression_count: _suppression_count,
            unused_component_props_exempted: _unused_component_props_exempted,
            active_suppressions: _active_suppressions,
            feature_flags: _feature_flags,
            // Security findings are emitted via `fallow security`, not the audit
            // dead-code gate; they have no dead-code key representation today.
            security_findings: _security_findings,
            security_unresolved_edge_files: _security_unresolved_edge_files,
            security_unresolved_callee_sites: _security_unresolved_callee_sites,
            security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
            prop_drilling_chains,
            thin_wrappers,
            duplicate_prop_shapes,
            // Export usages and entry-point summary are metadata, not issue
            // collections; no key needed.
            export_usages: _export_usages,
            entry_point_summary: _entry_point_summary,
            // Render fan-in is a whole-project descriptive metric, not an issue
            // collection; no attribution key needed.
            render_fan_in: _render_fan_in,
            // Per-component React intel is a descriptive ambient-editor carrier,
            // not an issue collection; no attribution key needed.
            react_component_intel: _react_component_intel,
            semantic_reachable_files: _,
            semantic_framework_contracts: _semantic_framework_contracts,
        } = results;

        self.add_items(AuditCollection::PropDrillingChains, prop_drilling_chains);
        self.add_items(AuditCollection::ThinWrappers, thin_wrappers);
        self.add_items(AuditCollection::DuplicatePropShapes, duplicate_prop_shapes);
        self.add_core_findings(
            unused_files,
            unused_exports,
            unused_types,
            private_type_leaks,
        );
        self.add_deprecated_exports_in_use(deprecated_exports_in_use);
        self.add_client_directive_findings(
            invalid_client_exports,
            mixed_client_server_barrels,
            misplaced_directives,
        );
        self.add_unlisted_binaries(unlisted_binaries);
        self.add_dependency_findings(&DependencyFindingSlices {
            unused_dependencies,
            unused_dev_dependencies,
            unused_optional_dependencies,
            unlisted_dependencies,
            type_only_dependencies,
            test_only_dependencies,
            dev_dependencies_in_production,
        });
        self.add_dependency_override_findings(
            unused_dependency_overrides,
            misconfigured_dependency_overrides,
        );
        self.add_member_findings(
            unused_enum_members,
            unused_class_members,
            unused_namespace_members,
            unused_store_members,
        );
        self.add_component_contract_findings(&ComponentContractFindingSlices {
            unused_component_props,
            absent_component_props,
            unused_component_emits,
            unused_component_inputs,
            unused_component_outputs,
            unused_svelte_events,
            unused_effect_layers,
            unused_tagged_errors,
            unproduced_catch_tags,
        });
        self.add_items(
            AuditCollection::DuplicateExportAliases,
            duplicate_export_aliases,
        );
        self.add_graph_findings(
            unresolved_imports,
            duplicate_exports,
            circular_dependencies,
            re_export_cycles,
            package_cycles,
        );
        self.add_boundary_findings(
            boundary_violations,
            boundary_coverage_violations,
            boundary_call_violations,
            policy_violations,
            stale_suppressions,
        );
        self.add_catalog_findings(
            unresolved_catalog_references,
            unused_catalog_entries,
            empty_catalog_groups,
        );
        self.add_framework_findings(&FrameworkFindingSlices {
            unprovided_injects,
            unrendered_components,
            unused_server_actions,
            unused_load_data_keys,
            route_collisions,
            dynamic_segment_name_conflicts,
        });
    }
}

struct DeadCodeKeyCollector<'a> {
    root: &'a Path,
    keys: FxHashSet<String>,
    introduced_keys: FxHashSet<String>,
    inherited_keys: FxHashSet<String>,
    records: Vec<AuditFindingRecord>,
    collection_counts: FxHashMap<&'static str, usize>,
    config: Option<&'a ResolvedConfig>,
    base: Option<&'a FxHashSet<String>>,
    #[cfg(test)]
    classifications: usize,
}

impl<'a> DeadCodeKeyCollector<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            keys: FxHashSet::default(),
            introduced_keys: FxHashSet::default(),
            inherited_keys: FxHashSet::default(),
            records: Vec::new(),
            collection_counts: FxHashMap::default(),
            config: None,
            base: None,
            #[cfg(test)]
            classifications: 0,
        }
    }

    fn for_comparison(
        root: &'a Path,
        config: &'a ResolvedConfig,
        base: Option<&'a FxHashSet<String>>,
    ) -> Self {
        Self {
            root,
            keys: FxHashSet::default(),
            introduced_keys: FxHashSet::default(),
            inherited_keys: FxHashSet::default(),
            records: Vec::new(),
            collection_counts: FxHashMap::default(),
            config: Some(config),
            base,
            #[cfg(test)]
            classifications: 0,
        }
    }

    fn into_keys(self) -> FxHashSet<String> {
        self.keys
    }

    fn into_ledger(self) -> DeadCodeAuditLedger {
        DeadCodeAuditLedger {
            records: self.records,
            keys: self.keys,
            introduced_keys: self.introduced_keys,
            inherited_keys: self.inherited_keys,
            #[cfg(test)]
            classifications: self.classifications,
        }
    }

    fn insert(&mut self, collection: AuditCollection, key: String, effective_severity: Severity) {
        let collection = collection.json_key();
        let ordinal = self.collection_counts.entry(collection).or_default();
        if self.config.is_some() {
            #[cfg(test)]
            {
                self.classifications += 1;
            }
            let introduced = self.base.is_some_and(|base| !base.contains(&key));
            if self.base.is_some() && is_counted_dead_code_finding(collection, effective_severity) {
                if introduced {
                    self.introduced_keys.insert(key.clone());
                } else {
                    self.inherited_keys.insert(key.clone());
                }
            }
            self.records.push(AuditFindingRecord {
                collection,
                ordinal: *ordinal,
                introduced,
                effective_severity,
                stable_key: key.clone(),
            });
        }
        *ordinal += 1;
        self.keys.insert(key);
    }

    /// Insert every finding of one collection under its audit key.
    fn add_items<T>(&mut self, collection: AuditCollection, items: &[T])
    where
        T: IdentifiedFinding + fallow_engine::dead_code::RuleSeverity,
    {
        let keys = collection_keys(items, self.root);
        for (item, key) in items.iter().zip(keys) {
            self.insert_rule(collection, key, item);
        }
    }

    /// Insert a finding with the severity that the shared rule table gives
    /// it. The exit code and the CI formats read the same table.
    fn insert_rule(
        &mut self,
        collection: AuditCollection,
        key: String,
        item: &impl fallow_engine::dead_code::RuleSeverity,
    ) {
        let effective = self.config.map_or(Severity::Off, |config| {
            item.rule_severity(&fallow_engine::dead_code::SeveritySource::from_config(
                config,
            ))
        });
        self.insert(collection, key, effective);
    }

    fn add_core_findings(
        &mut self,
        unused_files: &[fallow_types::output_dead_code::UnusedFileFinding],
        unused_exports: &[fallow_types::output_dead_code::UnusedExportFinding],
        unused_types: &[fallow_types::output_dead_code::UnusedTypeFinding],
        private_type_leaks: &[fallow_types::output_dead_code::PrivateTypeLeakFinding],
    ) {
        self.add_unused_files(unused_files);
        self.add_unused_exports(unused_exports);
        self.add_unused_types(unused_types);
        self.add_private_type_leaks(private_type_leaks);
    }

    fn add_client_directive_findings(
        &mut self,
        invalid_client_exports: &[fallow_types::output_dead_code::InvalidClientExportFinding],
        mixed_client_server_barrels: &[fallow_types::output_dead_code::MixedClientServerBarrelFinding],
        misplaced_directives: &[fallow_types::output_dead_code::MisplacedDirectiveFinding],
    ) {
        self.add_invalid_client_exports(invalid_client_exports);
        self.add_mixed_client_server_barrels(mixed_client_server_barrels);
        self.add_misplaced_directives(misplaced_directives);
    }

    fn add_dependency_findings(&mut self, deps: &DependencyFindingSlices<'_>) {
        let DependencyFindingSlices {
            unused_dependencies,
            unused_dev_dependencies,
            unused_optional_dependencies,
            unlisted_dependencies,
            type_only_dependencies,
            test_only_dependencies,
            dev_dependencies_in_production,
        } = *deps;
        self.add_unused_dependencies(unused_dependencies);
        self.add_unused_dev_dependencies(unused_dev_dependencies);
        self.add_unused_optional_dependencies(unused_optional_dependencies);
        self.add_unlisted_dependencies(unlisted_dependencies);
        self.add_type_only_dependencies(type_only_dependencies);
        self.add_test_only_dependencies(test_only_dependencies);
        self.add_dev_dependencies_in_production(dev_dependencies_in_production);
    }

    fn add_dependency_override_findings(
        &mut self,
        unused_dependency_overrides: &[fallow_types::output_dead_code::UnusedDependencyOverrideFinding],
        misconfigured_dependency_overrides: &[fallow_types::output_dead_code::MisconfiguredDependencyOverrideFinding],
    ) {
        self.add_unused_dependency_overrides(unused_dependency_overrides);
        self.add_misconfigured_dependency_overrides(misconfigured_dependency_overrides);
    }

    fn add_member_findings(
        &mut self,
        unused_enum_members: &[fallow_types::output_dead_code::UnusedEnumMemberFinding],
        unused_class_members: &[fallow_types::output_dead_code::UnusedClassMemberFinding],
        unused_namespace_members: &[fallow_types::output_dead_code::UnusedNamespaceMemberFinding],
        unused_store_members: &[fallow_types::output_dead_code::UnusedStoreMemberFinding],
    ) {
        self.add_unused_enum_members(unused_enum_members);
        self.add_unused_class_members(unused_class_members);
        self.add_unused_namespace_members(unused_namespace_members);
        self.add_unused_store_members(unused_store_members);
    }

    fn add_component_contract_findings(&mut self, components: &ComponentContractFindingSlices<'_>) {
        let ComponentContractFindingSlices {
            unused_component_props,
            absent_component_props,
            unused_component_emits,
            unused_component_inputs,
            unused_component_outputs,
            unused_svelte_events,
            unused_effect_layers,
            unused_tagged_errors,
            unproduced_catch_tags,
        } = *components;
        self.add_unused_component_props(unused_component_props);
        self.add_items(
            AuditCollection::AbsentComponentProps,
            absent_component_props,
        );
        self.add_unused_component_emits(unused_component_emits);
        self.add_unused_component_inputs(unused_component_inputs);
        self.add_unused_component_outputs(unused_component_outputs);
        self.add_unused_svelte_events(unused_svelte_events);
        self.add_unused_effect_layers(unused_effect_layers);
        self.add_unused_tagged_errors(unused_tagged_errors);
        self.add_unproduced_catch_tags(unproduced_catch_tags);
    }

    fn add_graph_findings(
        &mut self,
        unresolved_imports: &[fallow_types::output_dead_code::UnresolvedImportFinding],
        duplicate_exports: &[fallow_types::output_dead_code::DuplicateExportFinding],
        circular_dependencies: &[fallow_types::output_dead_code::CircularDependencyFinding],
        re_export_cycles: &[fallow_types::output_dead_code::ReExportCycleFinding],
        package_cycles: &[fallow_types::output_dead_code::PackageCycleFinding],
    ) {
        self.add_unresolved_imports(unresolved_imports);
        self.add_duplicate_exports(duplicate_exports);
        self.add_circular_dependencies(circular_dependencies);
        self.add_re_export_cycles(re_export_cycles);
        self.add_package_cycles(package_cycles);
    }

    fn add_boundary_findings(
        &mut self,
        boundary_violations: &[fallow_types::output_dead_code::BoundaryViolationFinding],
        boundary_coverage_violations: &[fallow_types::output_dead_code::BoundaryCoverageViolationFinding],
        boundary_call_violations: &[fallow_types::output_dead_code::BoundaryCallViolationFinding],
        policy_violations: &[fallow_types::output_dead_code::PolicyViolationFinding],
        stale_suppressions: &[fallow_types::results::StaleSuppression],
    ) {
        self.add_boundary_violations(boundary_violations);
        self.add_boundary_coverage_violations(boundary_coverage_violations);
        self.add_boundary_call_violations(boundary_call_violations);
        self.add_policy_violations(policy_violations);
        self.add_stale_suppressions(stale_suppressions);
    }

    fn add_catalog_findings(
        &mut self,
        unresolved_catalog_references: &[fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding],
        unused_catalog_entries: &[fallow_types::output_dead_code::UnusedCatalogEntryFinding],
        empty_catalog_groups: &[fallow_types::output_dead_code::EmptyCatalogGroupFinding],
    ) {
        self.add_unresolved_catalog_references(unresolved_catalog_references);
        self.add_unused_catalog_entries(unused_catalog_entries);
        self.add_empty_catalog_groups(empty_catalog_groups);
    }

    fn add_framework_findings(&mut self, framework: &FrameworkFindingSlices<'_>) {
        let FrameworkFindingSlices {
            unprovided_injects,
            unrendered_components,
            unused_server_actions,
            unused_load_data_keys,
            route_collisions,
            dynamic_segment_name_conflicts,
        } = *framework;
        self.add_unprovided_injects(unprovided_injects);
        self.add_unrendered_components(unrendered_components);
        self.add_unused_server_actions(unused_server_actions);
        self.add_unused_load_data_keys(unused_load_data_keys);
        self.add_route_collisions(route_collisions);
        self.add_dynamic_segment_name_conflicts(dynamic_segment_name_conflicts);
    }

    fn add_unused_files(&mut self, items: &[fallow_types::output_dead_code::UnusedFileFinding]) {
        self.add_items(AuditCollection::UnusedFiles, items);
    }

    fn add_unused_exports(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedExportFinding],
    ) {
        self.add_items(AuditCollection::UnusedExports, items);
    }

    fn add_unused_types(&mut self, items: &[fallow_types::output_dead_code::UnusedTypeFinding]) {
        self.add_items(AuditCollection::UnusedTypes, items);
    }

    fn add_private_type_leaks(
        &mut self,
        items: &[fallow_types::output_dead_code::PrivateTypeLeakFinding],
    ) {
        self.add_items(AuditCollection::PrivateTypeLeaks, items);
    }

    fn add_deprecated_exports_in_use(
        &mut self,
        items: &[fallow_types::output_dead_code::DeprecatedExportInUseFinding],
    ) {
        self.add_items(AuditCollection::DeprecatedExportsInUse, items);
    }

    fn add_invalid_client_exports(
        &mut self,
        items: &[fallow_types::output_dead_code::InvalidClientExportFinding],
    ) {
        self.add_items(AuditCollection::InvalidClientExports, items);
    }

    fn add_mixed_client_server_barrels(
        &mut self,
        items: &[fallow_types::output_dead_code::MixedClientServerBarrelFinding],
    ) {
        self.add_items(AuditCollection::MixedClientServerBarrels, items);
    }

    fn add_misplaced_directives(
        &mut self,
        items: &[fallow_types::output_dead_code::MisplacedDirectiveFinding],
    ) {
        self.add_items(AuditCollection::MisplacedDirectives, items);
    }

    fn add_unprovided_injects(
        &mut self,
        items: &[fallow_types::output_dead_code::UnprovidedInjectFinding],
    ) {
        self.add_items(AuditCollection::UnprovidedInjects, items);
    }

    fn add_unrendered_components(
        &mut self,
        items: &[fallow_types::output_dead_code::UnrenderedComponentFinding],
    ) {
        self.add_items(AuditCollection::UnrenderedComponents, items);
    }

    fn add_unused_component_props(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedComponentPropFinding],
    ) {
        self.add_items(AuditCollection::UnusedComponentProps, items);
    }

    fn add_unused_component_emits(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedComponentEmitFinding],
    ) {
        self.add_items(AuditCollection::UnusedComponentEmits, items);
    }

    fn add_unused_component_inputs(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedComponentInputFinding],
    ) {
        self.add_items(AuditCollection::UnusedComponentInputs, items);
    }

    fn add_unused_component_outputs(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedComponentOutputFinding],
    ) {
        self.add_items(AuditCollection::UnusedComponentOutputs, items);
    }

    fn add_unused_svelte_events(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedSvelteEventFinding],
    ) {
        self.add_items(AuditCollection::UnusedSvelteEvents, items);
    }
    fn add_unused_effect_layers(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedEffectLayerFinding],
    ) {
        self.add_items(AuditCollection::UnusedEffectLayers, items);
    }
    fn add_unused_tagged_errors(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedTaggedErrorFinding],
    ) {
        self.add_items(AuditCollection::UnusedTaggedErrors, items);
    }
    fn add_unproduced_catch_tags(
        &mut self,
        items: &[fallow_types::output_dead_code::UnproducedCatchTagFinding],
    ) {
        self.add_items(AuditCollection::UnproducedCatchTags, items);
    }

    fn add_unused_server_actions(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedServerActionFinding],
    ) {
        self.add_items(AuditCollection::UnusedServerActions, items);
    }

    fn add_unused_load_data_keys(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedLoadDataKeyFinding],
    ) {
        self.add_items(AuditCollection::UnusedLoadDataKeys, items);
    }

    fn add_route_collisions(
        &mut self,
        items: &[fallow_types::output_dead_code::RouteCollisionFinding],
    ) {
        self.add_items(AuditCollection::RouteCollisions, items);
    }

    fn add_dynamic_segment_name_conflicts(
        &mut self,
        items: &[fallow_types::output_dead_code::DynamicSegmentNameConflictFinding],
    ) {
        self.add_items(AuditCollection::DynamicSegmentNameConflicts, items);
    }

    fn add_unused_dependencies(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedDependencyFinding],
    ) {
        self.add_items(AuditCollection::UnusedDependencies, items);
    }

    fn add_unused_dev_dependencies(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedDevDependencyFinding],
    ) {
        self.add_items(AuditCollection::UnusedDevDependencies, items);
    }

    fn add_unused_optional_dependencies(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedOptionalDependencyFinding],
    ) {
        self.add_items(AuditCollection::UnusedOptionalDependencies, items);
    }

    fn add_unused_enum_members(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedEnumMemberFinding],
    ) {
        self.add_items(AuditCollection::UnusedEnumMembers, items);
    }

    fn add_unused_class_members(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedClassMemberFinding],
    ) {
        self.add_items(AuditCollection::UnusedClassMembers, items);
    }

    fn add_unused_namespace_members(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedNamespaceMemberFinding],
    ) {
        self.add_items(AuditCollection::UnusedNamespaceMembers, items);
    }

    fn add_unused_store_members(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedStoreMemberFinding],
    ) {
        self.add_items(AuditCollection::UnusedStoreMembers, items);
    }

    fn add_unresolved_imports(
        &mut self,
        items: &[fallow_types::output_dead_code::UnresolvedImportFinding],
    ) {
        self.add_items(AuditCollection::UnresolvedImports, items);
    }

    fn add_unlisted_dependencies(
        &mut self,
        items: &[fallow_types::output_dead_code::UnlistedDependencyFinding],
    ) {
        self.add_items(AuditCollection::UnlistedDependencies, items);
    }

    fn add_unlisted_binaries(
        &mut self,
        items: &[fallow_types::output_dead_code::UnlistedBinaryFinding],
    ) {
        self.add_items(AuditCollection::UnlistedBinaries, items);
    }

    fn add_duplicate_exports(
        &mut self,
        items: &[fallow_types::output_dead_code::DuplicateExportFinding],
    ) {
        self.add_items(AuditCollection::DuplicateExports, items);
    }

    fn add_type_only_dependencies(
        &mut self,
        items: &[fallow_types::output_dead_code::TypeOnlyDependencyFinding],
    ) {
        self.add_items(AuditCollection::TypeOnlyDependencies, items);
    }

    fn add_test_only_dependencies(
        &mut self,
        items: &[fallow_types::output_dead_code::TestOnlyDependencyFinding],
    ) {
        self.add_items(AuditCollection::TestOnlyDependencies, items);
    }

    fn add_dev_dependencies_in_production(
        &mut self,
        items: &[fallow_types::output_dead_code::DevDependencyInProductionFinding],
    ) {
        self.add_items(AuditCollection::DevDependenciesInProduction, items);
    }

    fn add_circular_dependencies(
        &mut self,
        items: &[fallow_types::output_dead_code::CircularDependencyFinding],
    ) {
        self.add_items(AuditCollection::CircularDependencies, items);
    }

    fn add_re_export_cycles(
        &mut self,
        items: &[fallow_types::output_dead_code::ReExportCycleFinding],
    ) {
        self.add_items(AuditCollection::ReExportCycles, items);
    }

    fn add_package_cycles(
        &mut self,
        items: &[fallow_types::output_dead_code::PackageCycleFinding],
    ) {
        self.add_items(AuditCollection::PackageCycles, items);
    }

    fn add_boundary_violations(
        &mut self,
        items: &[fallow_types::output_dead_code::BoundaryViolationFinding],
    ) {
        self.add_items(AuditCollection::BoundaryViolations, items);
    }

    fn add_boundary_coverage_violations(
        &mut self,
        items: &[fallow_types::output_dead_code::BoundaryCoverageViolationFinding],
    ) {
        self.add_items(AuditCollection::BoundaryCoverageViolations, items);
    }

    fn add_boundary_call_violations(
        &mut self,
        items: &[fallow_types::output_dead_code::BoundaryCallViolationFinding],
    ) {
        self.add_items(AuditCollection::BoundaryCallViolations, items);
    }

    fn add_policy_violations(
        &mut self,
        items: &[fallow_types::output_dead_code::PolicyViolationFinding],
    ) {
        self.add_items(AuditCollection::PolicyViolations, items);
    }

    fn add_stale_suppressions(&mut self, items: &[fallow_types::results::StaleSuppression]) {
        self.add_items(AuditCollection::StaleSuppressions, items);
    }

    fn add_unresolved_catalog_references(
        &mut self,
        items: &[fallow_types::output_dead_code::UnresolvedCatalogReferenceFinding],
    ) {
        self.add_items(AuditCollection::UnresolvedCatalogReferences, items);
    }

    fn add_unused_catalog_entries(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedCatalogEntryFinding],
    ) {
        self.add_items(AuditCollection::UnusedCatalogEntries, items);
    }

    fn add_empty_catalog_groups(
        &mut self,
        items: &[fallow_types::output_dead_code::EmptyCatalogGroupFinding],
    ) {
        self.add_items(AuditCollection::EmptyCatalogGroups, items);
    }

    fn add_unused_dependency_overrides(
        &mut self,
        items: &[fallow_types::output_dead_code::UnusedDependencyOverrideFinding],
    ) {
        self.add_items(AuditCollection::UnusedDependencyOverrides, items);
    }

    fn add_misconfigured_dependency_overrides(
        &mut self,
        items: &[fallow_types::output_dead_code::MisconfiguredDependencyOverrideFinding],
    ) {
        self.add_items(AuditCollection::MisconfiguredDependencyOverrides, items);
    }
}

/// Retain only findings whose audit key was NOT present in `base` (i.e. was
/// introduced on the current branch).
///
/// When `base` is `None` (no baseline), all findings are kept.
///
/// This destructure is deliberately exhaustive: adding a field to
/// `AnalysisResults` must fail compilation here so the author decides
/// explicitly whether the new finding type needs an introduced-retain (add a
/// retain block) or has no key representation today (bind with underscore and
/// document why).
///
/// Sibling exhaustive sites: `fallow_engine::changed_files::filter_results_by_changed_files`,
/// `dead_code_keys`, `retain_introduced_dead_code`.
/// Non-exhaustive siblings the compiler will NOT flag (wire manually when a
/// finding type is added): `annotate_dead_code_json` (same key formats, this
/// file) and the per-collection `off` filter branches in
/// `crates/engine/src/dead_code.rs` (`apply_rule_severities`). The severity of
/// each record comes from the `RuleSeverity` table in
/// `crates/engine/src/effective_severity.rs`, whose visitors are exhaustive.
/// TypeScript mirror: `editors/vscode/scripts/codegen-contracts.mjs` derives
/// backwards-compatible aliases from `fallow schema` `ts_alias` rows.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across audit attribution keys"
)]
pub fn retain_introduced_dead_code(
    results: &mut fallow_types::results::AnalysisResults,
    root: &Path,
    base: Option<&FxHashSet<String>>,
) {
    let Some(base) = base else {
        return;
    };

    // Compute the introduced set before taking any mutable borrows. Note the
    // order differs from the pre-destructure code, which narrowed
    // unused_files/exports/types first and computed keys from the narrowed
    // results. Computing from the un-narrowed results is equivalent: those
    // retains keep exactly the items whose key is NOT in `base`, and the
    // `!base.contains(key)` filter below removes the same base-member keys
    // from the full key set, so `introduced` is identical either way.
    classify_introduced_dead_code_fields(results);
    macro_rules! retain {
        ($($field:ident),* $(,)?) => {
            $(retain_introduced(&mut results.$field, root, base);)*
        };
    }
    retain!(
        prop_drilling_chains,
        thin_wrappers,
        duplicate_prop_shapes,
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        deprecated_exports_in_use,
        unused_enum_members,
        unused_class_members,
        unused_namespace_members,
        unused_store_members,
        unresolved_imports,
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        unlisted_dependencies,
        unlisted_binaries,
        duplicate_exports,
        duplicate_export_aliases,
        type_only_dependencies,
        test_only_dependencies,
        circular_dependencies,
        re_export_cycles,
        package_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        stale_suppressions,
        unresolved_catalog_references,
        unused_catalog_entries,
        empty_catalog_groups,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        invalid_client_exports,
        mixed_client_server_barrels,
        misplaced_directives,
        unprovided_injects,
        unrendered_components,
        unused_component_props,
        absent_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_effect_layers,
        unused_tagged_errors,
        unproduced_catch_tags,
        unused_server_actions,
        unused_load_data_keys,
        route_collisions,
        dynamic_segment_name_conflicts,
    );
}

/// Keep the findings of one collection whose audit key is absent from `base`.
fn retain_introduced<T: IdentifiedFinding>(
    items: &mut Vec<T>,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let mut keys = collection_keys(items, root).into_iter();
    items.retain(|_| keys.next().is_some_and(|key| !base.contains(&key)));
}

fn classify_introduced_dead_code_fields(results: &fallow_types::results::AnalysisResults) {
    let fallow_types::results::AnalysisResults {
        shared_state: _,
        unused_files: _unused_files,
        unused_exports: _unused_exports,
        unused_types: _unused_types,
        private_type_leaks: _private_type_leaks,
        deprecated_exports_in_use: _deprecated_exports_in_use,
        unused_dependencies: _unused_dependencies,
        unused_dev_dependencies: _unused_dev_dependencies,
        unused_optional_dependencies: _unused_optional_dependencies,
        unused_enum_members: _unused_enum_members,
        unused_class_members: _unused_class_members,
        unused_namespace_members: _unused_namespace_members,
        unused_store_members: _unused_store_members,
        unresolved_imports: _unresolved_imports,
        unlisted_dependencies: _unlisted_dependencies,
        unlisted_binaries: _unlisted_binaries,
        duplicate_exports: _duplicate_exports,
        duplicate_export_aliases: _duplicate_export_aliases,
        type_only_dependencies: _type_only_dependencies,
        test_only_dependencies: _test_only_dependencies,
        dev_dependencies_in_production: _dev_dependencies_in_production,
        circular_dependencies: _circular_dependencies,
        re_export_cycles: _re_export_cycles,
        package_cycles: _package_cycles,
        boundary_violations: _boundary_violations,
        boundary_coverage_violations: _boundary_coverage_violations,
        boundary_call_violations: _boundary_call_violations,
        policy_violations: _policy_violations,
        stale_suppressions: _stale_suppressions,
        unused_catalog_entries: _unused_catalog_entries,
        empty_catalog_groups: _empty_catalog_groups,
        unresolved_catalog_references: _unresolved_catalog_references,
        unused_dependency_overrides: _unused_dependency_overrides,
        misconfigured_dependency_overrides: _misconfigured_dependency_overrides,
        invalid_client_exports: _invalid_client_exports,
        mixed_client_server_barrels: _mixed_client_server_barrels,
        misplaced_directives: _misplaced_directives,
        unprovided_injects: _unprovided_injects,
        unrendered_components: _unrendered_components,
        unused_component_props: _unused_component_props,
        absent_component_props: _absent_component_props,
        unused_component_emits: _unused_component_emits,
        unused_component_inputs: _unused_component_inputs,
        unused_component_outputs: _unused_component_outputs,
        unused_svelte_events: _unused_svelte_events,
        unused_effect_layers: _unused_effect_layers,
        unused_tagged_errors: _unused_tagged_errors,
        unproduced_catch_tags: _unproduced_catch_tags,
        unused_server_actions: _unused_server_actions,
        unused_load_data_keys: _unused_load_data_keys,
        unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
        route_collisions: _route_collisions,
        dynamic_segment_name_conflicts: _dynamic_segment_name_conflicts,
        // Non-finding fields: counts and metadata, not subject to base-keyed
        // filtering.
        cascade_hidden: _cascade_hidden,
        cascade: _cascade,
        suppression_count: _suppression_count,
        unused_component_props_exempted: _unused_component_props_exempted,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        // Security findings are emitted via `fallow security`, not the audit
        // dead-code gate; they have no key representation and are not filtered
        // here.
        security_findings: _security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
        // Prop chains use source identities and occurrence-aware audit filtering.
        prop_drilling_chains: _prop_drilling_chains,
        // Component health findings use source identities and occurrence-aware filtering.
        thin_wrappers: _thin_wrappers,
        duplicate_prop_shapes: _duplicate_prop_shapes,
        // Export usages and entry-point summary are metadata, not issue
        // collections; no key needed.
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
        // Render fan-in is a whole-project descriptive metric, not an issue
        // collection; no key needed.
        render_fan_in: _render_fan_in,
        // Per-component React intel is a descriptive ambient-editor carrier, not
        // an issue collection; no key needed.
        react_component_intel: _react_component_intel,
        semantic_reachable_files: _,
        semantic_framework_contracts: _semantic_framework_contracts,
    } = results;
}

/// The `introduced` flag of each finding in one collection, in order.
fn introduced_flags<T: IdentifiedFinding>(
    items: &[T],
    root: &Path,
    base: &FxHashSet<String>,
) -> Vec<bool> {
    collection_keys(items, root)
        .iter()
        .map(|key| !base.contains(key))
        .collect()
}

fn issue_was_introduced(key: &str, base: &FxHashSet<String>) -> bool {
    !base.contains(key)
}

fn annotate_issue_array<I>(json: &mut serde_json::Value, key: &str, introduced: I)
where
    I: IntoIterator<Item = bool>,
{
    let Some(items) = json.get_mut(key).and_then(serde_json::Value::as_array_mut) else {
        return;
    };
    for (item, introduced) in items.iter_mut().zip(introduced) {
        if let serde_json::Value::Object(map) = item {
            map.insert("introduced".to_string(), serde_json::json!(introduced));
        }
    }
}

/// Insert `"introduced": bool` into every dead-code finding object in the
/// serialized JSON by re-deriving each key and testing membership against
/// the base snapshot. JSON arrays must be in the same order as the typed
/// `results` collections they were serialized from.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across audit attribution keys"
)]
pub fn annotate_dead_code_json(
    json: &mut serde_json::Value,
    results: &fallow_types::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    macro_rules! annotate {
        ($($field:ident),* $(,)?) => {
            $(annotate_issue_array(
                json,
                stringify!($field),
                introduced_flags(&results.$field, root, base),
            );)*
        };
    }
    annotate!(
        prop_drilling_chains,
        thin_wrappers,
        duplicate_prop_shapes,
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        deprecated_exports_in_use,
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        type_only_dependencies,
        test_only_dependencies,
        unlisted_dependencies,
        unlisted_binaries,
        unused_enum_members,
        unused_class_members,
        unused_namespace_members,
        unused_store_members,
        unresolved_imports,
        duplicate_exports,
        duplicate_export_aliases,
        circular_dependencies,
        re_export_cycles,
        package_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        stale_suppressions,
        unresolved_catalog_references,
        unused_catalog_entries,
        empty_catalog_groups,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        invalid_client_exports,
        mixed_client_server_barrels,
        misplaced_directives,
        unprovided_injects,
        unrendered_components,
        unused_component_props,
        absent_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_effect_layers,
        unused_tagged_errors,
        unproduced_catch_tags,
        unused_server_actions,
        route_collisions,
        dynamic_segment_name_conflicts,
    );
}

/// Annotate the sole legacy dead-code collection without a typed
/// `introduced` field. Every wrapper-backed collection is annotated from the
/// persisted [`AuditComparison`] instead.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across audit attribution keys"
)]
pub fn annotate_stale_suppressions_json(
    json: &mut serde_json::Value,
    results: &fallow_types::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "stale_suppressions",
        stale_suppression_introduced_flags(results, root, base),
    );
}

/// The audit flag of each stale suppression, in report order. A flag is
/// `true` when the base snapshot does not hold the key of the suppression.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across audit attribution keys"
)]
#[must_use]
pub fn stale_suppression_introduced_flags(
    results: &fallow_types::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) -> Vec<bool> {
    introduced_flags(&results.stale_suppressions, root, base)
}

const STALE_SUPPRESSION_CHECK_NAMES: [&str; 2] = [
    "fallow/stale-suppression",
    "fallow/missing-suppression-reason",
];

/// Set the audit attribution on the stale-suppression issues of the
/// dead-code CodeClimate issues.
///
/// `introduced` holds one audit flag per stale suppression, in the order of
/// `AnalysisResults::stale_suppressions`. The builder emits one issue per
/// stale suppression in that order, so the flags match the issues by
/// position. The inline review comment then states if the change introduced
/// the stale suppression.
pub fn set_stale_suppression_audit_attribution(
    issues: &mut [fallow_output::CodeClimateIssue],
    introduced: impl IntoIterator<Item = bool>,
) {
    let stale_issues = issues
        .iter_mut()
        .filter(|issue| STALE_SUPPRESSION_CHECK_NAMES.contains(&issue.check_name.as_str()));
    for (issue, introduced) in stale_issues.zip(introduced) {
        issue.audit = Some(crate::dead_code_codeclimate::audit_review_context(
            introduced,
        ));
    }
}

/// Insert `"introduced": bool` into each `findings` and `styling_findings`
/// object in serialized health JSON, matching entries to the typed report by
/// array position.
///
/// A complexity finding keeps the typed `introduced` flag that the audit
/// comparison set, because that flag compares metric values. Only a finding
/// without the typed flag falls back to identity-key membership in `base`.
/// A styling finding also keeps its typed flag, because the audit can use the
/// run's diff to tell equal findings apart.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across audit attribution keys"
)]
pub fn annotate_health_json(
    json: &mut serde_json::Value,
    report: &fallow_output::HealthReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    if let Some(items) = json
        .get_mut("findings")
        .and_then(serde_json::Value::as_array_mut)
    {
        for (item, finding) in items.iter_mut().zip(&report.findings) {
            if let serde_json::Value::Object(map) = item {
                map.insert(
                    "introduced".to_string(),
                    serde_json::json!(finding.introduced.unwrap_or_else(|| {
                        issue_was_introduced(&health_finding_key(finding, root), base)
                    })),
                );
            }
        }
    }
    styling::annotate_styling_json(json, &report.styling_findings, root, base);
}

/// Insert `"introduced": bool` into each `clone_groups` object in serialized
/// duplication JSON, matching groups to the typed report by array position.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across audit attribution keys"
)]
pub fn annotate_dupes_json(
    json: &mut serde_json::Value,
    report: &fallow_types::duplicates::DuplicationReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("clone_groups")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, group) in items.iter_mut().zip(&report.clone_groups) {
        if let serde_json::Value::Object(map) = item {
            map.insert(
                "introduced".to_string(),
                serde_json::json!(issue_was_introduced(&dupe_group_key(group, root), base)),
            );
        }
    }
}

/// Attach precomputed introduced membership to an audit JSON array.
pub fn annotate_domain_json(
    json: &mut serde_json::Value,
    collection: &str,
    introduced: impl IntoIterator<Item = bool>,
) {
    annotate_issue_array(json, collection, introduced);
}

/// Attach a `demotion_reason` to demoted entries of an audit JSON array,
/// matching entries to demoted membership by array position. Non-demoted
/// entries are left untouched so output without demotions stays byte-identical
/// (issue #2220).
pub fn annotate_domain_demotions_json(
    json: &mut serde_json::Value,
    collection: &str,
    demoted: impl IntoIterator<Item = bool>,
    reason: crate::CloneDemotionReason,
) {
    let Some(items) = json
        .get_mut(collection)
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, demoted) in items.iter_mut().zip(demoted) {
        if demoted && let serde_json::Value::Object(map) = item {
            map.insert("demotion_reason".to_string(), serde_json::json!(reason));
        }
    }
}

/// Attribution key set for every complexity finding in a health report.
pub fn health_keys(report: &fallow_output::HealthReport, root: &Path) -> FxHashSet<String> {
    report
        .findings
        .iter()
        .map(|finding| health_finding_key(finding, root))
        .collect()
}

/// Identity key for one complexity finding: `complexity:<path>:<function>`.
///
/// Line numbers are excluded, so a finding survives unrelated edits above it.
/// The exceeded category is excluded, so a category change keeps the match.
/// The new-only gate compares the metric values of matched findings, see
/// [`classify_complexity_findings`].
pub fn health_finding_key(finding: &fallow_output::ComplexityViolation, root: &Path) -> String {
    format!(
        "complexity:{}:{}",
        relative_key_path(Path::new(&finding.path), root),
        finding.name,
    )
}

/// Attribution key set for every clone group in a duplication report.
pub fn dupes_keys(
    report: &fallow_types::duplicates::DuplicationReport,
    root: &Path,
) -> FxHashSet<String> {
    report
        .clone_groups
        .iter()
        .map(|group| dupe_group_key(group, root))
        .collect()
}

/// Attribution key for one clone group:
/// `dupe:<sorted files joined by |>:<tokens>:<lines>:<fragment hash>`.
/// The fragment hash keeps distinct groups spanning the same files apart;
/// line positions are excluded so a group survives shifts within its files.
/// The fragment hash leaves out `\r`, so the line endings do not change the key.
pub fn dupe_group_key(group: &fallow_types::duplicates::CloneGroup, root: &Path) -> String {
    let mut files: Vec<String> = group
        .instances
        .iter()
        .map(|instance| relative_key_path(&instance.file, root))
        .collect();
    files.sort();
    files.dedup();
    let mut hasher = DefaultHasher::new();
    for instance in &group.instances {
        if instance.fragment.contains('\r') {
            instance.fragment.replace('\r', "").hash(&mut hasher);
        } else {
            instance.fragment.hash(&mut hasher);
        }
    }
    format!(
        "dupe:{}:{}:{}:{:x}",
        files.join("|"),
        group.token_count,
        group.line_count,
        hasher.finish()
    )
}

/// Keys of clone groups whose every instance lies entirely outside the diff's
/// added lines: no instance range contains an added line, so the changeset did
/// not write the duplicated text, and the group only became reportable (under
/// a new attribution key) because the changeset removed code elsewhere;
/// deduplicating one region can create or re-shape a clone group across files
/// (issue #2164). New-only gating demotes these groups to inherited so a
/// clone-removal refactor is not failed by the duplication it did not write.
/// An instance whose path cannot be mapped into the diff's namespace is
/// treated as touched, so its group keeps gating.
pub fn preexisting_dupe_group_keys<'a>(
    groups: impl IntoIterator<Item = &'a fallow_types::duplicates::CloneGroup>,
    root: &Path,
    diff: &fallow_output::DiffIndex,
) -> FxHashSet<String> {
    let instance_touched = |instance: &fallow_types::duplicates::CloneInstance| -> bool {
        let Some(rel) = diff.key_for(&instance.file, root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        diff.range_overlaps_added(&rel, start, end)
    };
    groups
        .into_iter()
        .filter(|group| !group.instances.iter().any(instance_touched))
        .map(|group| dupe_group_key(group, root))
        .collect()
}

#[cfg(test)]
#[path = "audit_keys_tests.rs"]
mod tests;
