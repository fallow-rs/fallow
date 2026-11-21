//! One visitor that narrows an `AnalysisResults` value to a result scope.
//!
//! The changed-file scope, the diff scope and the workspace scope each
//! implement `ResultScope`. `retain_in_scope` names every finding array
//! one time, so a new array reaches every scope.

use std::path::Path;

use fallow_types::output_dead_code::{
    DuplicateExportFinding, EffectiveSeverity, PropDrillingChainFinding, UnlistedBinaryFinding,
    UnlistedDependencyFinding,
};
use fallow_types::results::{
    AnalysisResults, BinaryUseSite, DuplicateLocation, ImportSite, PropDrillHop, SecurityFinding,
};

/// Where a finding lives, for a scope test.
#[derive(Clone, Copy)]
pub enum Anchor<'a> {
    /// A source position with a real line.
    Line { path: &'a Path, line: u32 },
    /// A file. Used when the finding has no real line: an unused file, a
    /// route file, an unzoned file, a member of a cycle.
    File { path: &'a Path },
}

/// A finding that a manifest or the project owns, not a source position.
#[derive(Clone, Copy)]
pub enum ProjectFinding<'a> {
    /// A dependency finding of one `package.json`.
    ManifestDependency { manifest: &'a Path },
    /// An unused catalog entry.
    CatalogEntry,
    /// An empty catalog group in the file at `path`.
    EmptyCatalogGroup { path: &'a Path },
    /// A catalog reference in the manifest at `path`.
    CatalogReference { path: &'a Path },
    /// A dependency override in the file at `path`.
    DependencyOverride { path: &'a Path },
}

/// How a scope narrows a finding with a list of use sites
/// (`unlisted_dependencies`, `unlisted_binaries`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SiteRule {
    /// Keep the finding and its full list when one site is in scope.
    AnySite,
    /// Remove each site out of scope. Drop the finding when no site stays.
    TrimSites,
}

/// How a scope narrows `duplicate_exports`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OwnerRule {
    /// Keep the full finding when one location is in scope.
    AnyOwner,
    /// Remove each location out of scope. Keep the finding when two or more
    /// locations stay.
    TrimOwners,
}

/// Which prop-drilling chains, thin wrappers and duplicate prop shapes a
/// scope narrows.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ComponentRule {
    /// Every finding.
    All,
    /// Only findings with the effective severity `Error`. Advisory findings
    /// stay.
    EnforcedOnly,
}

/// One result scope. The visitor asks it about each finding.
pub trait ResultScope {
    /// Whether a finding at `anchor` is in scope.
    fn keeps(&self, anchor: Anchor<'_>) -> bool;
    /// Whether a manifest-owned or project-owned finding is in scope.
    fn keeps_project_finding(&self, finding: ProjectFinding<'_>) -> bool;
    /// Whether a security finding is in scope. Each scope reads other hops.
    fn keeps_security_finding(&self, finding: &SecurityFinding) -> bool;
    /// The rule for findings with a list of use sites.
    fn site_rule(&self) -> SiteRule;
    /// The rule for `duplicate_exports`.
    fn owner_rule(&self) -> OwnerRule;
    /// The rule for component findings.
    fn component_rule(&self) -> ComponentRule;
}

/// Keep the findings of `results` that `scope` keeps.
#[expect(
    clippy::too_many_lines,
    reason = "the one exhaustive destructure names every field of AnalysisResults, so a new finding array is a compile error until it gets an anchor here"
)]
pub fn retain_in_scope(results: &mut AnalysisResults, scope: &impl ResultScope) {
    let AnalysisResults {
        shared_state,
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
        unused_protected_class_members,
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
        route_collisions,
        dynamic_segment_name_conflicts,
        unused_component_props,
        absent_component_props,
        constant_parameters,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_effect_layers,
        unused_tagged_errors,
        unproduced_catch_tags,
        unused_server_actions,
        unused_load_data_keys,
        unused_load_data_keys_global_abstain: _,
        prop_drilling_chains,
        thin_wrappers,
        duplicate_prop_shapes,
        cascade_hidden: _,
        cascade: _,
        suppression_count: _,
        unused_component_props_exempted: _,
        active_suppressions: _,
        feature_flags: _,
        security_findings,
        suppressed_security_findings: _,
        security_unresolved_edge_files: _,
        security_unresolved_callee_sites: _,
        security_unresolved_callee_diagnostics,
        export_usages: _,
        entry_point_summary: _,
        render_fan_in: _,
        react_component_intel: _,
        semantic_reachable_files: _,
        semantic_framework_contracts: _,
        constant_parameter_candidates: _,
    } = results;

    if let Some(inventory) = shared_state {
        inventory.retain_sites(|site| {
            scope.keeps(Anchor::Line {
                path: &site.path,
                line: site.line as u32,
            })
        });
    }

    retain_files(unused_files, scope, |f| &f.file.path);
    retain_lines(unused_exports, scope, |f| (&f.export.path, f.export.line));
    retain_lines(unused_types, scope, |f| (&f.export.path, f.export.line));
    retain_lines(private_type_leaks, scope, |f| (&f.leak.path, f.leak.line));
    retain_lines(deprecated_exports_in_use, scope, |f| {
        (&f.export.path, f.export.line)
    });
    retain_lines(unused_enum_members, scope, |f| {
        (&f.member.path, f.member.line)
    });
    retain_lines(unused_class_members, scope, |f| {
        (&f.member.path, f.member.line)
    });
    retain_lines(unused_namespace_members, scope, |f| {
        (&f.member.path, f.member.line)
    });
    retain_lines(unused_protected_class_members, scope, |f| {
        (&f.member.path, f.member.line)
    });
    retain_lines(unused_store_members, scope, |f| {
        (&f.member.path, f.member.line)
    });
    retain_lines(unresolved_imports, scope, |f| {
        (&f.import.path, f.import.line)
    });

    retain_lines(unprovided_injects, scope, |f| {
        (&f.inject.path, f.inject.line)
    });
    retain_lines(unrendered_components, scope, |f| {
        (&f.component.path, f.component.line)
    });
    retain_lines(unused_component_props, scope, |f| {
        (&f.prop.path, f.prop.line)
    });
    retain_lines(absent_component_props, scope, |f| {
        (&f.prop.path, f.prop.line)
    });
    retain_lines(constant_parameters, scope, |f| {
        (&f.parameter.path, f.parameter.line)
    });
    retain_lines(unused_component_emits, scope, |f| {
        (&f.emit.path, f.emit.line)
    });
    retain_lines(unused_component_inputs, scope, |f| {
        (&f.input.path, f.input.line)
    });
    retain_lines(unused_component_outputs, scope, |f| {
        (&f.output.path, f.output.line)
    });
    retain_lines(unused_svelte_events, scope, |f| {
        (&f.event.path, f.event.line)
    });
    retain_lines(unused_effect_layers, scope, |f| {
        (&f.layer.path, f.layer.line)
    });
    retain_lines(unused_tagged_errors, scope, |f| {
        (&f.error.path, f.error.line)
    });
    retain_lines(unproduced_catch_tags, scope, |f| {
        (&f.catch_tag.path, f.catch_tag.line)
    });
    retain_lines(unused_server_actions, scope, |f| {
        (&f.action.path, f.action.line)
    });
    retain_lines(unused_load_data_keys, scope, |f| (&f.key.path, f.key.line));

    retain_lines(invalid_client_exports, scope, |f| {
        (&f.export.path, f.export.line)
    });
    retain_lines(mixed_client_server_barrels, scope, |f| {
        (&f.barrel.path, f.barrel.line)
    });
    retain_lines(misplaced_directives, scope, |f| {
        (&f.directive_site.path, f.directive_site.line)
    });
    retain_files(route_collisions, scope, |f| &f.collision.path);
    retain_files(dynamic_segment_name_conflicts, scope, |f| &f.conflict.path);

    retain_lines(boundary_violations, scope, |f| {
        (&f.violation.from_path, f.violation.line)
    });
    retain_files(boundary_coverage_violations, scope, |f| &f.violation.path);
    retain_lines(boundary_call_violations, scope, |f| {
        (&f.violation.path, f.violation.line)
    });
    retain_lines(policy_violations, scope, |f| {
        (&f.violation.path, f.violation.line)
    });
    retain_lines(stale_suppressions, scope, |f| (&f.path, f.line));

    duplicate_export_aliases.retain(|f| {
        let alias = &f.alias;
        if alias.locations.is_empty() {
            return scope.keeps(line_anchor(&alias.path, alias.line));
        }
        alias
            .locations
            .iter()
            .any(|location| scope.keeps(line_anchor(&alias.path, location.line)))
    });
    circular_dependencies.retain(|f| any_file(scope, &f.cycle.files));
    re_export_cycles.retain(|f| any_file(scope, &f.cycle.files));
    package_cycles.retain(|f| {
        f.cycle
            .edges
            .iter()
            .any(|edge| scope.keeps(Anchor::File { path: &edge.path }))
    });
    retain_unlisted_dependencies(unlisted_dependencies, scope);
    retain_unlisted_binaries(unlisted_binaries, scope);
    retain_duplicate_exports(duplicate_exports, scope);

    unused_dependencies.retain(|f| scope.keeps_project_finding(manifest(&f.dep.path)));
    unused_dev_dependencies.retain(|f| scope.keeps_project_finding(manifest(&f.dep.path)));
    unused_optional_dependencies.retain(|f| scope.keeps_project_finding(manifest(&f.dep.path)));
    type_only_dependencies.retain(|f| scope.keeps_project_finding(manifest(&f.dep.path)));
    test_only_dependencies.retain(|f| scope.keeps_project_finding(manifest(&f.dep.path)));
    dev_dependencies_in_production.retain(|f| scope.keeps_project_finding(manifest(&f.dep.path)));
    unused_catalog_entries.retain(|_| scope.keeps_project_finding(ProjectFinding::CatalogEntry));
    empty_catalog_groups.retain(|f| {
        scope.keeps_project_finding(ProjectFinding::EmptyCatalogGroup {
            path: &f.group.path,
        })
    });
    unresolved_catalog_references.retain(|f| {
        scope.keeps_project_finding(ProjectFinding::CatalogReference {
            path: &f.reference.path,
        })
    });
    unused_dependency_overrides.retain(|f| {
        scope.keeps_project_finding(ProjectFinding::DependencyOverride {
            path: &f.entry.path,
        })
    });
    misconfigured_dependency_overrides.retain(|f| {
        scope.keeps_project_finding(ProjectFinding::DependencyOverride {
            path: &f.entry.path,
        })
    });

    security_findings.retain(|f| scope.keeps_security_finding(f));
    retain_lines(security_unresolved_callee_diagnostics, scope, |f| {
        (&f.path, f.line)
    });

    thin_wrappers.retain(|f| {
        component_in_scope(scope, f.effective_severity, &f.wrapper.file, f.wrapper.line)
    });
    duplicate_prop_shapes
        .retain(|f| component_in_scope(scope, f.effective_severity, &f.shape.file, f.shape.line));
    retain_prop_drilling_chains(prop_drilling_chains, scope);
}

fn line_anchor(path: &Path, line: u32) -> Anchor<'_> {
    Anchor::Line { path, line }
}

fn manifest(path: &Path) -> ProjectFinding<'_> {
    ProjectFinding::ManifestDependency { manifest: path }
}

fn any_file(scope: &impl ResultScope, files: &[std::path::PathBuf]) -> bool {
    files.iter().any(|path| scope.keeps(Anchor::File { path }))
}

/// Keep the findings whose real source line is in scope.
fn retain_lines<T>(
    findings: &mut Vec<T>,
    scope: &impl ResultScope,
    position: impl Fn(&T) -> (&Path, u32),
) {
    findings.retain(|finding| {
        let (path, line) = position(finding);
        scope.keeps(Anchor::Line { path, line })
    });
}

/// Keep the findings whose file is in scope.
fn retain_files<T>(findings: &mut Vec<T>, scope: &impl ResultScope, file: impl Fn(&T) -> &Path) {
    findings.retain(|finding| {
        scope.keeps(Anchor::File {
            path: file(finding),
        })
    });
}

/// Whether an advisory component finding bypasses the scope.
fn advisory_bypass(scope: &impl ResultScope, severity: Option<EffectiveSeverity>) -> bool {
    scope.component_rule() == ComponentRule::EnforcedOnly
        && severity != Some(EffectiveSeverity::Error)
}

fn component_in_scope(
    scope: &impl ResultScope,
    severity: Option<EffectiveSeverity>,
    path: &Path,
    line: u32,
) -> bool {
    advisory_bypass(scope, severity) || scope.keeps(line_anchor(path, line))
}

fn retain_unlisted_dependencies(
    findings: &mut Vec<UnlistedDependencyFinding>,
    scope: &impl ResultScope,
) {
    let in_scope = |site: &ImportSite| scope.keeps(line_anchor(&site.path, site.line));
    match scope.site_rule() {
        SiteRule::AnySite => findings.retain(|f| f.dep.imported_from.iter().any(in_scope)),
        SiteRule::TrimSites => {
            for finding in &mut *findings {
                finding.dep.imported_from.retain(in_scope);
            }
            findings.retain(|f| !f.dep.imported_from.is_empty());
        }
    }
}

fn retain_unlisted_binaries(findings: &mut Vec<UnlistedBinaryFinding>, scope: &impl ResultScope) {
    let in_scope = |site: &BinaryUseSite| scope.keeps(line_anchor(&site.path, site.line));
    match scope.site_rule() {
        SiteRule::AnySite => findings.retain(|f| f.binary.used_in.iter().any(in_scope)),
        SiteRule::TrimSites => {
            for finding in &mut *findings {
                finding.binary.used_in.retain(in_scope);
            }
            findings.retain(|f| !f.binary.used_in.is_empty());
        }
    }
}

fn retain_duplicate_exports(findings: &mut Vec<DuplicateExportFinding>, scope: &impl ResultScope) {
    let in_scope =
        |location: &DuplicateLocation| scope.keeps(line_anchor(&location.path, location.line));
    match scope.owner_rule() {
        OwnerRule::AnyOwner => findings.retain(|f| f.export.locations.iter().any(in_scope)),
        OwnerRule::TrimOwners => {
            for finding in &mut *findings {
                finding.export.locations.retain(in_scope);
            }
            findings.retain(|f| f.export.locations.len() >= 2);
        }
    }
}

fn retain_prop_drilling_chains(
    findings: &mut Vec<PropDrillingChainFinding>,
    scope: &impl ResultScope,
) {
    findings.retain(|f| {
        if advisory_bypass(scope, f.effective_severity) {
            return true;
        }
        let hops = &f.chain.hops;
        let in_scope = |hop: &PropDrillHop| scope.keeps(line_anchor(&hop.file, hop.line));
        if f.effective_severity == Some(EffectiveSeverity::Error) {
            return hops.is_empty() || hops.iter().any(in_scope);
        }
        hops.first().is_some_and(in_scope)
    });
}

#[cfg(test)]
#[path = "result_scope_tests.rs"]
mod tests;
