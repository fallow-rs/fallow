//! Shared grouped-output builders for programmatic and CLI consumers.

use std::collections::BTreeMap;
use std::path::Path;

use fallow_engine::duplicates::CloneFingerprintSet;
use fallow_types::duplicates::{CloneGroup, DuplicationReport, DuplicationStats};
use fallow_types::results::AnalysisResults;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AttributedCloneGroup, AttributedCloneGroupFinding, AttributedInstance, CloneFamilyFinding,
    DuplicationGroup, DuplicationGrouping,
};

/// Canonical label for issues that cannot be attributed to a group.
pub const UNOWNED_GROUP_LABEL: &str = "(unowned)";

/// The group header part that names the opt-in component health signals of
/// `results`, such as `; 3 health signals: 3 duplicate prop shapes`. Empty when
/// there are none. The signals do not count toward `total_issues`, so a group
/// header names them next to the issue count. The human and markdown grouped
/// reports share this text.
#[must_use]
pub fn health_signal_header_part(results: &AnalysisResults) -> String {
    let kinds = [
        (results.prop_drilling_chains.len(), "prop drilling chain"),
        (results.thin_wrappers.len(), "thin wrapper"),
        (results.duplicate_prop_shapes.len(), "duplicate prop shape"),
    ];
    let count: usize = kinds.iter().map(|(n, _)| n).sum();
    if count == 0 {
        return String::new();
    }
    let parts: Vec<String> = kinds
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, label)| format!("{n} {label}{}", plural_suffix(*n)))
        .collect();
    format!(
        "; {count} health signal{}: {}",
        plural_suffix(count),
        parts.join(", ")
    )
}

const fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// A single grouped dead-code analysis bucket.
pub struct ResultGroup {
    /// Group label such as owner, directory, package, or section.
    pub key: String,
    /// Section default owners for section grouping.
    ///
    /// `None` for grouping modes without owner metadata. `Some(vec![])` for
    /// groups that have no section owners.
    pub owners: Option<Vec<String>>,
    /// Issues belonging to this group.
    pub results: AnalysisResults,
}

/// Partition analysis results into groups using caller-provided path resolvers.
///
/// The caller owns all environment-specific context, such as CODEOWNERS,
/// package discovery, root-relative path normalization, or section metadata.
#[must_use]
pub fn group_analysis_results_with<F, O>(
    results: &AnalysisResults,
    mut key_for_path: F,
    mut owners_for_path: O,
    include_owners: bool,
) -> Vec<ResultGroup>
where
    F: FnMut(&Path) -> String,
    O: FnMut(&Path) -> Option<Vec<String>>,
{
    let mut group_owners: FxHashMap<String, Vec<String>> = FxHashMap::default();
    let mut builder = GroupingBuilder::new(|path: &Path| {
        let key = key_for_path(path);
        if include_owners && !group_owners.contains_key(&key) {
            let owners = owners_for_path(path).unwrap_or_default();
            group_owners.insert(key.clone(), owners);
        }
        key
    });
    builder.group_symbol_issues(results);
    builder.group_dependency_issues(results);
    builder.group_relationship_issues(results);
    builder.group_workspace_config_issues(results);
    builder.group_component_health_issues(results);

    finalize_groups(builder.into_groups(), group_owners, include_owners)
}

struct GroupingBuilder<F> {
    groups: FxHashMap<String, AnalysisResults>,
    key_for: F,
}

impl<F> GroupingBuilder<F>
where
    F: FnMut(&Path) -> String,
{
    fn new(key_for: F) -> Self {
        Self {
            groups: FxHashMap::default(),
            key_for,
        }
    }

    fn entry_for_path(&mut self, path: &Path) -> &mut AnalysisResults {
        let key = (self.key_for)(path);
        self.groups.entry(key).or_default()
    }

    fn entry_for_key(&mut self, key: String) -> &mut AnalysisResults {
        self.groups.entry(key).or_default()
    }

    fn into_groups(self) -> FxHashMap<String, AnalysisResults> {
        self.groups
    }

    fn group_symbol_issues(&mut self, results: &AnalysisResults) {
        for item in &results.unused_files {
            self.entry_for_path(&item.file.path)
                .unused_files
                .push(item.clone());
        }
        for item in &results.unused_exports {
            self.entry_for_path(&item.export.path)
                .unused_exports
                .push(item.clone());
        }
        for item in &results.unused_types {
            self.entry_for_path(&item.export.path)
                .unused_types
                .push(item.clone());
        }
        for item in &results.private_type_leaks {
            self.entry_for_path(&item.leak.path)
                .private_type_leaks
                .push(item.clone());
        }
        for item in &results.deprecated_exports_in_use {
            self.entry_for_path(&item.export.path)
                .deprecated_exports_in_use
                .push(item.clone());
        }
        for item in &results.unused_enum_members {
            self.entry_for_path(&item.member.path)
                .unused_enum_members
                .push(item.clone());
        }
        for item in &results.unused_class_members {
            self.entry_for_path(&item.member.path)
                .unused_class_members
                .push(item.clone());
        }
        for item in &results.unused_store_members {
            self.entry_for_path(&item.member.path)
                .unused_store_members
                .push(item.clone());
        }
        for item in &results.unresolved_imports {
            self.entry_for_path(&item.import.path)
                .unresolved_imports
                .push(item.clone());
        }
    }

    fn group_dependency_issues(&mut self, results: &AnalysisResults) {
        for item in &results.unused_dependencies {
            self.entry_for_path(&item.dep.path)
                .unused_dependencies
                .push(item.clone());
        }
        for item in &results.unused_dev_dependencies {
            self.entry_for_path(&item.dep.path)
                .unused_dev_dependencies
                .push(item.clone());
        }
        for item in &results.unused_optional_dependencies {
            self.entry_for_path(&item.dep.path)
                .unused_optional_dependencies
                .push(item.clone());
        }
        for item in &results.type_only_dependencies {
            self.entry_for_path(&item.dep.path)
                .type_only_dependencies
                .push(item.clone());
        }
        for item in &results.test_only_dependencies {
            self.entry_for_path(&item.dep.path)
                .test_only_dependencies
                .push(item.clone());
        }
        for item in &results.dev_dependencies_in_production {
            self.entry_for_path(&item.dep.path)
                .dev_dependencies_in_production
                .push(item.clone());
        }

        for item in &results.unlisted_dependencies {
            let key = item.dep.imported_from.first().map_or_else(
                || UNOWNED_GROUP_LABEL.to_string(),
                |site| (self.key_for)(&site.path),
            );
            self.entry_for_key(key)
                .unlisted_dependencies
                .push(item.clone());
        }
        for item in &results.duplicate_exports {
            let key = item.export.locations.first().map_or_else(
                || UNOWNED_GROUP_LABEL.to_string(),
                |loc| (self.key_for)(&loc.path),
            );
            self.entry_for_key(key).duplicate_exports.push(item.clone());
        }
    }

    fn group_relationship_issues(&mut self, results: &AnalysisResults) {
        self.group_structure_issues(results);
        self.group_framework_boundary_issues(results);
        self.group_component_contract_issues(results);
    }

    fn group_structure_issues(&mut self, results: &AnalysisResults) {
        for item in &results.circular_dependencies {
            let key = item
                .cycle
                .files
                .first()
                .map_or_else(|| UNOWNED_GROUP_LABEL.to_string(), |f| (self.key_for)(f));
            self.entry_for_key(key)
                .circular_dependencies
                .push(item.clone());
        }
        for item in &results.re_export_cycles {
            let key = item
                .cycle
                .files
                .first()
                .map_or_else(|| UNOWNED_GROUP_LABEL.to_string(), |f| (self.key_for)(f));
            self.entry_for_key(key).re_export_cycles.push(item.clone());
        }
        // The first example import file owns a package cycle, the same file
        // that workspace scope and per-file severity use.
        for item in &results.package_cycles {
            let key = item.cycle.edges.first().map_or_else(
                || UNOWNED_GROUP_LABEL.to_string(),
                |edge| (self.key_for)(&edge.path),
            );
            self.entry_for_key(key).package_cycles.push(item.clone());
        }
        for item in &results.boundary_violations {
            self.entry_for_path(&item.violation.from_path)
                .boundary_violations
                .push(item.clone());
        }
        for item in &results.boundary_coverage_violations {
            self.entry_for_path(&item.violation.path)
                .boundary_coverage_violations
                .push(item.clone());
        }
        for item in &results.boundary_call_violations {
            self.entry_for_path(&item.violation.path)
                .boundary_call_violations
                .push(item.clone());
        }
        for item in &results.policy_violations {
            self.entry_for_path(&item.violation.path)
                .policy_violations
                .push(item.clone());
        }
    }

    fn group_framework_boundary_issues(&mut self, results: &AnalysisResults) {
        for item in &results.invalid_client_exports {
            self.entry_for_path(&item.export.path)
                .invalid_client_exports
                .push(item.clone());
        }
        for item in &results.mixed_client_server_barrels {
            self.entry_for_path(&item.barrel.path)
                .mixed_client_server_barrels
                .push(item.clone());
        }
        for item in &results.misplaced_directives {
            self.entry_for_path(&item.directive_site.path)
                .misplaced_directives
                .push(item.clone());
        }
        for item in &results.unprovided_injects {
            self.entry_for_path(&item.inject.path)
                .unprovided_injects
                .push(item.clone());
        }
        for item in &results.unrendered_components {
            self.entry_for_path(&item.component.path)
                .unrendered_components
                .push(item.clone());
        }
        for item in &results.route_collisions {
            self.entry_for_path(&item.collision.path)
                .route_collisions
                .push(item.clone());
        }
        for item in &results.dynamic_segment_name_conflicts {
            self.entry_for_path(&item.conflict.path)
                .dynamic_segment_name_conflicts
                .push(item.clone());
        }
    }

    fn group_component_contract_issues(&mut self, results: &AnalysisResults) {
        for item in &results.unused_component_props {
            self.entry_for_path(&item.prop.path)
                .unused_component_props
                .push(item.clone());
        }
        for item in &results.absent_component_props {
            self.entry_for_path(&item.prop.path)
                .absent_component_props
                .push(item.clone());
        }
        for item in &results.unused_component_emits {
            self.entry_for_path(&item.emit.path)
                .unused_component_emits
                .push(item.clone());
        }
        for item in &results.unused_component_inputs {
            self.entry_for_path(&item.input.path)
                .unused_component_inputs
                .push(item.clone());
        }
        for item in &results.unused_component_outputs {
            self.entry_for_path(&item.output.path)
                .unused_component_outputs
                .push(item.clone());
        }
        for item in &results.unused_svelte_events {
            self.entry_for_path(&item.event.path)
                .unused_svelte_events
                .push(item.clone());
        }
        for item in &results.unused_server_actions {
            self.entry_for_path(&item.action.path)
                .unused_server_actions
                .push(item.clone());
        }
        for item in &results.unused_load_data_keys {
            self.entry_for_path(&item.key.path)
                .unused_load_data_keys
                .push(item.clone());
        }
        for item in &results.stale_suppressions {
            self.entry_for_path(&item.path)
                .stale_suppressions
                .push(item.clone());
        }
    }

    fn group_workspace_config_issues(&mut self, results: &AnalysisResults) {
        for item in &results.unused_catalog_entries {
            self.entry_for_path(&item.entry.path)
                .unused_catalog_entries
                .push(item.clone());
        }
        for item in &results.empty_catalog_groups {
            self.entry_for_path(&item.group.path)
                .empty_catalog_groups
                .push(item.clone());
        }
        for item in &results.unresolved_catalog_references {
            self.entry_for_path(&item.reference.path)
                .unresolved_catalog_references
                .push(item.clone());
        }
        for item in &results.unused_dependency_overrides {
            self.entry_for_path(&item.entry.path)
                .unused_dependency_overrides
                .push(item.clone());
        }
        for item in &results.misconfigured_dependency_overrides {
            self.entry_for_path(&item.entry.path)
                .misconfigured_dependency_overrides
                .push(item.clone());
        }
    }

    /// Group the opt-in component health signals. They do not count toward
    /// `total_issues`, but the flat report lists them, so the groups must too.
    fn group_component_health_issues(&mut self, results: &AnalysisResults) {
        // The first hop owns the drilled prop and anchors the finding, the
        // same file that suppression and CI annotations use.
        for item in &results.prop_drilling_chains {
            let key = item.chain.hops.first().map_or_else(
                || UNOWNED_GROUP_LABEL.to_string(),
                |hop| (self.key_for)(&hop.file),
            );
            self.entry_for_key(key)
                .prop_drilling_chains
                .push(item.clone());
        }
        for item in &results.thin_wrappers {
            self.entry_for_path(&item.wrapper.file)
                .thin_wrappers
                .push(item.clone());
        }
        for item in &results.duplicate_prop_shapes {
            self.entry_for_path(&item.shape.file)
                .duplicate_prop_shapes
                .push(item.clone());
        }
    }
}

fn finalize_groups(
    groups: FxHashMap<String, AnalysisResults>,
    mut group_owners: FxHashMap<String, Vec<String>>,
    include_owners: bool,
) -> Vec<ResultGroup> {
    let mut sorted: Vec<_> = groups
        .into_iter()
        .map(|(key, results)| {
            let owners = if include_owners {
                Some(group_owners.remove(&key).unwrap_or_default())
            } else {
                None
            };
            ResultGroup {
                key,
                owners,
                results,
            }
        })
        .collect();
    sorted.sort_by(|a, b| {
        let a_unowned = a.key == UNOWNED_GROUP_LABEL;
        let b_unowned = b.key == UNOWNED_GROUP_LABEL;
        match (a_unowned, b_unowned) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b
                .results
                .total_issues()
                .cmp(&a.results.total_issues())
                .then_with(|| a.key.cmp(&b.key)),
        }
    });
    sorted
}

/// Return the majority owner for a clone group using caller-provided path attribution.
#[must_use]
pub fn largest_clone_group_owner_with<F>(group: &CloneGroup, mut key_for_path: F) -> String
where
    F: FnMut(&Path) -> String,
{
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for instance in &group.instances {
        let key = key_for_path(&instance.file);
        *counts.entry(key).or_insert(0) += 1;
    }
    if counts.is_empty() {
        return UNOWNED_GROUP_LABEL.to_string();
    }
    let mut best_key: Option<String> = None;
    let mut best_count: u32 = 0;
    for (key, count) in counts {
        if best_key.is_none() || count > best_count {
            best_count = count;
            best_key = Some(key);
        }
    }
    best_key.unwrap_or_else(|| UNOWNED_GROUP_LABEL.to_string())
}

/// Build grouped duplication output using caller-provided path attribution.
#[must_use]
pub fn build_duplication_grouping_with<F>(
    report: &DuplicationReport,
    mode: &'static str,
    mut key_for_path: F,
) -> DuplicationGrouping
where
    F: FnMut(&Path) -> String,
{
    let fingerprints = CloneFingerprintSet::from_groups(&report.clone_groups);
    let buckets = build_attributed_clone_buckets(report, &mut key_for_path);
    let mut groups: Vec<DuplicationGroup> = buckets
        .into_iter()
        .map(|(key, groups)| duplication_group(key, groups, report, &fingerprints))
        .collect();
    sort_duplication_groups(&mut groups);

    DuplicationGrouping { mode, groups }
}

fn build_attributed_clone_buckets<F>(
    report: &DuplicationReport,
    key_for_path: &mut F,
) -> BTreeMap<String, Vec<AttributedCloneGroup>>
where
    F: FnMut(&Path) -> String,
{
    let mut buckets: BTreeMap<String, Vec<AttributedCloneGroup>> = BTreeMap::new();
    for group in &report.clone_groups {
        let attributed = attributed_clone_group(group, key_for_path);
        buckets
            .entry(attributed.primary_owner.clone())
            .or_default()
            .push(attributed);
    }
    buckets
}

fn attributed_clone_group<F>(group: &CloneGroup, key_for_path: &mut F) -> AttributedCloneGroup
where
    F: FnMut(&Path) -> String,
{
    let primary_owner = largest_clone_group_owner_with(group, &mut *key_for_path);
    let instances = group
        .instances
        .iter()
        .map(|instance| AttributedInstance {
            owner: key_for_path(&instance.file),
            instance: instance.clone(),
        })
        .collect();
    AttributedCloneGroup {
        primary_owner,
        token_count: group.token_count,
        line_count: group.line_count,
        similarity: group.similarity,
        instances,
    }
}

fn duplication_group(
    key: String,
    attributed_groups: Vec<AttributedCloneGroup>,
    report: &DuplicationReport,
    fingerprints: &CloneFingerprintSet,
) -> DuplicationGroup {
    let mut subset = duplication_subset_report(&attributed_groups, report);
    subset.stats = fallow_engine::duplicates::recompute_stats(&subset);
    let clone_families = clone_families_for_bucket(&attributed_groups, report, fingerprints);
    subset.stats.clone_families = clone_families.len();
    let clone_groups = attributed_groups
        .into_iter()
        .map(|group| {
            let fingerprint = group.fingerprint(fingerprints);
            AttributedCloneGroupFinding::with_fingerprint(group, fingerprint)
        })
        .collect();

    DuplicationGroup {
        key,
        stats: subset.stats,
        clone_groups,
        clone_families,
    }
}

fn duplication_subset_report(
    attributed_groups: &[AttributedCloneGroup],
    report: &DuplicationReport,
) -> DuplicationReport {
    DuplicationReport {
        clone_groups: attributed_groups
            .iter()
            .map(|group| CloneGroup {
                instances: group
                    .instances
                    .iter()
                    .map(|instance| instance.instance.clone())
                    .collect(),
                token_count: group.token_count,
                line_count: group.line_count,
                similarity: group.similarity,
            })
            .collect(),
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        stats: DuplicationStats {
            total_files: report.stats.total_files,
            files_with_clones: 0,
            total_lines: report.stats.total_lines,
            duplicated_lines: 0,
            total_tokens: report.stats.total_tokens,
            duplicated_tokens: 0,
            clone_groups: 0,
            clone_families: 0,
            clone_instances: 0,
            duplication_percentage: 0.0,
            // Filtering and bounded-work counters are report-scoped and cannot
            // be attributed reliably to one grouped bucket.
            clone_groups_below_min_occurrences: 0,
            clone_groups_ignored: 0,
            near_candidates_skipped: 0,
        },
    }
}

fn clone_families_for_bucket(
    attributed_groups: &[AttributedCloneGroup],
    report: &DuplicationReport,
    fingerprints: &CloneFingerprintSet,
) -> Vec<CloneFamilyFinding> {
    let bucket_files: FxHashSet<&Path> = attributed_groups
        .iter()
        .flat_map(|group| group.instances.iter().map(|i| i.instance.file.as_path()))
        .collect();

    report
        .clone_families
        .iter()
        .filter(|family| {
            family
                .files
                .iter()
                .any(|path| bucket_files.contains(path.as_path()))
        })
        .map(|family| CloneFamilyFinding::with_fingerprints(family.clone(), fingerprints))
        .collect()
}

fn sort_duplication_groups(groups: &mut [DuplicationGroup]) {
    groups.sort_by(|a, b| {
        let a_unowned = a.key == UNOWNED_GROUP_LABEL;
        let b_unowned = b.key == UNOWNED_GROUP_LABEL;
        match (a_unowned, b_unowned) {
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            _ => b
                .clone_groups
                .len()
                .cmp(&a.clone_groups.len())
                .then_with(|| a.key.cmp(&b.key)),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Group by the full file path, so every finding anchor gets its own group.
    fn group_by_path(results: &AnalysisResults) -> Vec<ResultGroup> {
        group_analysis_results_with(
            results,
            |path: &Path| path.to_string_lossy().into_owned(),
            |_: &Path| None,
            false,
        )
    }

    /// The length of each serialized issue list of `results`, by key.
    fn list_lengths(results: &AnalysisResults) -> BTreeMap<String, usize> {
        let value = serde_json::to_value(results).expect("serialize results");
        value
            .as_object()
            .expect("results object")
            .iter()
            .filter_map(|(key, value)| value.as_array().map(|items| (key.clone(), items.len())))
            .collect()
    }

    #[test]
    fn every_issue_list_is_grouped() {
        // The fixture names every field, so a new field must get a value there.
        let source = crate::editor::tests::merge_test_source_with_all_fields();

        let expected = list_lengths(&source);
        for (key, len) in &expected {
            assert!(
                *len > 0,
                "the fixture must fill `{key}` so this guard checks its grouping"
            );
        }

        let groups = group_by_path(&source);
        let mut grouped: BTreeMap<String, usize> = BTreeMap::new();
        for group in &groups {
            for (key, len) in list_lengths(&group.results) {
                *grouped.entry(key).or_insert(0) += len;
            }
        }
        for (key, len) in &expected {
            assert_eq!(
                grouped.get(key).copied().unwrap_or(0),
                *len,
                "--group-by drops `{key}` findings: add them to `GroupingBuilder`"
            );
        }

        let grouped_total: usize = groups.iter().map(|g| g.results.total_issues()).sum();
        assert_eq!(grouped_total, source.total_issues());
    }

    #[test]
    fn prop_drilling_chain_without_hops_is_unowned() {
        let mut source = crate::editor::tests::merge_test_source_with_all_fields();
        let mut chain = source.prop_drilling_chains.remove(0);
        chain.chain.hops.clear();
        let mut results = AnalysisResults::default();
        results.prop_drilling_chains.push(chain);

        let groups = group_by_path(&results);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key, UNOWNED_GROUP_LABEL);
        assert_eq!(groups[0].results.prop_drilling_chains.len(), 1);
    }

    #[test]
    fn health_signal_header_part_names_each_signal_type() {
        let source = crate::editor::tests::merge_test_source_with_all_fields();
        let mut results = AnalysisResults::default();
        assert_eq!(health_signal_header_part(&results), "");

        results
            .prop_drilling_chains
            .push(source.prop_drilling_chains[0].clone());
        results.thin_wrappers.push(source.thin_wrappers[0].clone());
        results.thin_wrappers.push(source.thin_wrappers[0].clone());
        assert_eq!(
            health_signal_header_part(&results),
            "; 3 health signals: 1 prop drilling chain, 2 thin wrappers"
        );

        let mut single = AnalysisResults::default();
        single
            .duplicate_prop_shapes
            .push(source.duplicate_prop_shapes[0].clone());
        assert_eq!(
            health_signal_header_part(&single),
            "; 1 health signal: 1 duplicate prop shape"
        );
    }
}
