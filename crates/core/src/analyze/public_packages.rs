//! The `publicPackages` filter: drop the dead-code findings on the public
//! surface of the selected workspace packages. Also the
//! `public-package-candidate` hint for a package that looks published but
//! that `publicPackages` does not list.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{
    PackageJson, ResolvedConfig, RulesConfig, Severity, WorkspaceDiagnostic,
    WorkspaceDiagnosticKind, WorkspaceInfo,
};

use super::{
    PostDetectionInput, add_exportless_package_source_indexes, add_package_public_api_entry_points,
    graph_file_ids_by_path,
};
use crate::discover::FileId;
use crate::graph::{ExportNamespace, ModuleGraph};
use crate::results::{AnalysisResults, UnusedExport, UnusedMember};

/// The entry points of one or more packages, and every declaration that an
/// entry point exports or re-exports in the value or the type namespace.
struct PublicSurface<'a> {
    path_to_file_id: &'a FxHashMap<&'a Path, FileId>,
    entry_points: FxHashSet<FileId>,
    declarations: FxHashSet<(FileId, String)>,
}

impl PublicSurface<'_> {
    /// Whether the export `name` of the file at `path` is on the surface. A
    /// member is on the surface when its parent is.
    fn contains(&self, path: &Path, name: &str) -> bool {
        self.path_to_file_id.get(path).is_some_and(|&file_id| {
            self.entry_points.contains(&file_id)
                || self.declarations.contains(&(file_id, name.to_owned()))
        })
    }

    /// Drop each finding on the surface.
    fn retain_off_surface(&self, results: &mut AnalysisResults) {
        results
            .unused_exports
            .retain(|e| !self.contains(&e.export.path, &e.export.export_name));
        results
            .unused_types
            .retain(|e| !self.contains(&e.export.path, &e.export.export_name));
        results
            .unused_enum_members
            .retain(|e| !self.contains(&e.member.path, &e.member.parent_name));
        results
            .unused_class_members
            .retain(|e| !self.contains(&e.member.path, &e.member.parent_name));
        results
            .unused_protected_class_members
            .retain(|e| !self.contains(&e.member.path, &e.member.parent_name));
        results
            .unused_namespace_members
            .retain(|e| !self.contains(&e.member.path, &e.member.parent_name));
    }

    /// The number of `findings` on the surface.
    fn count_on_surface(&self, findings: &[SurfaceFinding<'_>]) -> usize {
        findings
            .iter()
            .filter(|finding| self.contains(finding.path, finding.name))
            .count()
    }
}

/// Builds the [`PublicSurface`] of a set of packages.
struct SurfaceBuilder<'a> {
    graph: &'a ModuleGraph,
    path_to_file_id: FxHashMap<&'a Path, FileId>,
    canonical_project_root: std::path::PathBuf,
}

impl<'a> SurfaceBuilder<'a> {
    fn new(graph: &'a ModuleGraph, config: &ResolvedConfig) -> Self {
        Self {
            graph,
            path_to_file_id: graph_file_ids_by_path(graph),
            canonical_project_root: dunce::canonicalize(&config.root)
                .unwrap_or_else(|_| config.root.clone()),
        }
    }

    /// Add the entry points of the package at `root` to `entry_points`.
    /// `pkg.private` must be unset: the caller decides whether a private
    /// package has a surface.
    fn add_entry_points(
        &self,
        entry_points: &mut FxHashSet<FileId>,
        root: &Path,
        pkg: &PackageJson,
    ) {
        add_package_public_api_entry_points(
            entry_points,
            self.graph,
            &self.path_to_file_id,
            root,
            pkg,
            &self.canonical_project_root,
        );
        add_exportless_package_source_indexes(entry_points, self.graph, root, pkg);
    }

    fn surface(&self, entry_points: FxHashSet<FileId>) -> PublicSurface<'_> {
        let declarations = [ExportNamespace::Value, ExportNamespace::Type]
            .into_iter()
            .flat_map(|namespace| {
                self.graph
                    .public_export_origins_in_namespace(&entry_points, namespace)
            })
            .map(|origin| (origin.file_id(), origin.export_name().to_owned()))
            .collect();
        PublicSurface {
            path_to_file_id: &self.path_to_file_id,
            entry_points,
            declarations,
        }
    }
}

/// Drop the findings on the public surface of the `publicPackages` workspaces.
///
/// The surface is each entry point of a public package plus every declaration
/// that an entry point exports or re-exports, also for a `private: true`
/// manifest. An export on the surface is
/// used, and a class or enum on the surface keeps all of its members. Code in
/// a public package that the surface cannot reach stays reported.
pub(super) fn filter_public_workspace_results(input: &mut PostDetectionInput<'_, '_>) {
    let (graph, config, workspaces) = (input.graph, input.config, input.workspaces);
    if config.public_packages.is_empty() || workspaces.is_empty() {
        return;
    }
    let builder = SurfaceBuilder::new(graph, config);
    let mut entry_points = FxHashSet::default();
    for workspace in workspaces.iter().filter(|workspace| {
        fallow_config::workspace_is_public(&workspace.name, &config.public_packages)
    }) {
        let Some(mut pkg) = fallow_config::load_dir_package_json(&workspace.root) else {
            continue;
        };
        // `publicPackages` names the package explicitly, so a `private: true`
        // manifest does not remove its surface.
        pkg.private = None;
        builder.add_entry_points(&mut entry_points, &workspace.root, &pkg);
    }
    if entry_points.is_empty() {
        return;
    }
    builder
        .surface(entry_points)
        .retain_off_surface(input.results);
}

/// A finding that a public surface can cover: the file and the export name,
/// or the parent name of a member.
struct SurfaceFinding<'a> {
    path: &'a Path,
    name: &'a str,
}

/// Record one `public-package-candidate` hint for each workspace package that looks
/// published and that `publicPackages` does not list (issue #5335).
///
/// A package looks published when its manifest is not `"private": true` and
/// has an `exports`, `main` or `module` field. It gets the hint only when at
/// least one reported finding is on its public surface. A finding counts when
/// its rule is not `off` for its file and `ignoreFindings` does not cover it.
/// A baseline applies later, so it does not change the count. The engine
/// narrows the hints to the run scope. The hint changes no finding.
pub(super) fn record_candidate_diagnostics(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    results: &AnalysisResults,
) {
    fallow_config::record_workspace_diagnostics(
        &config.root,
        candidate_diagnostics(graph, config, workspaces, results),
    );
}

/// The hints that [`record_candidate_diagnostics`] records, sorted by path.
fn candidate_diagnostics(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    results: &AnalysisResults,
) -> Vec<WorkspaceDiagnostic> {
    if workspaces.is_empty() {
        return Vec::new();
    }
    let findings = reported_surface_findings(config, results);
    if findings.is_empty() {
        return Vec::new();
    }
    let builder = SurfaceBuilder::new(graph, config);
    let mut diagnostics: Vec<WorkspaceDiagnostic> = workspaces
        .iter()
        .filter(|workspace| {
            !fallow_config::workspace_is_public(&workspace.name, &config.public_packages)
                && has_finding_under(&findings, &workspace.root)
        })
        .filter_map(|workspace| {
            let pkg = fallow_config::load_dir_package_json(&workspace.root)?;
            if !looks_published(&pkg) {
                return None;
            }
            let mut entry_points = FxHashSet::default();
            builder.add_entry_points(&mut entry_points, &workspace.root, &pkg);
            if entry_points.is_empty() {
                return None;
            }
            let count = builder.surface(entry_points).count_on_surface(&findings);
            (count > 0).then(|| {
                WorkspaceDiagnostic::new(
                    &config.root,
                    workspace.root.clone(),
                    WorkspaceDiagnosticKind::PublicPackageCandidate {
                        package: workspace.name.clone(),
                        findings: u32::try_from(count).unwrap_or(u32::MAX),
                    },
                )
            })
        })
        .collect();
    diagnostics.sort_by(|a, b| a.path.cmp(&b.path));
    diagnostics
}

/// Whether the manifest is not `"private": true` and names an entry in
/// `exports`, `main` or `module`.
fn looks_published(pkg: &PackageJson) -> bool {
    !pkg.private.unwrap_or(false)
        && (pkg.exports.is_some() || pkg.main.is_some() || pkg.module.is_some())
}

/// Whether a finding is in a file under the package directory `root`. A
/// package whose entry re-exports a file outside its own directory is rare,
/// so this check lets the hint skip the surface walk of each package with no
/// finding.
fn has_finding_under(findings: &[SurfaceFinding<'_>], root: &Path) -> bool {
    let canonical = dunce::canonicalize(root).ok();
    findings.iter().any(|finding| {
        finding.path.starts_with(root)
            || canonical
                .as_deref()
                .is_some_and(|canonical| finding.path.starts_with(canonical))
    })
}

/// The findings of the kinds that a public surface covers, without each
/// finding whose rule is `off` for its file or that `ignoreFindings` covers.
/// The engine removes those findings after this pass.
fn reported_surface_findings<'a>(
    config: &ResolvedConfig,
    results: &'a AnalysisResults,
) -> Vec<SurfaceFinding<'a>> {
    let reported = |path: &Path, rule: fn(&RulesConfig) -> Severity| {
        rule(&config.resolve_rules_for_path(path)) != Severity::Off
            && !config.ignore_findings.is_ignored_path(&config.root, path)
    };
    let export = |export: &'a UnusedExport, rule: fn(&RulesConfig) -> Severity| {
        reported(&export.path, rule).then_some(SurfaceFinding {
            path: &export.path,
            name: &export.export_name,
        })
    };
    let member = |member: &'a UnusedMember, rule: fn(&RulesConfig) -> Severity| {
        reported(&member.path, rule).then_some(SurfaceFinding {
            path: &member.path,
            name: &member.parent_name,
        })
    };
    let exports = (results.unused_exports.iter())
        .filter_map(|e| export(&e.export, |rules| rules.unused_exports))
        .chain(
            (results.unused_types.iter())
                .filter_map(|e| export(&e.export, |rules| rules.unused_types)),
        );
    let members = (results.unused_enum_members.iter())
        .filter_map(|e| member(&e.member, |rules| rules.unused_enum_members))
        .chain(
            (results.unused_class_members.iter())
                .filter_map(|e| member(&e.member, |rules| rules.unused_class_members)),
        )
        .chain(
            (results.unused_protected_class_members.iter())
                .filter_map(|e| member(&e.member, |rules| rules.unused_protected_class_members)),
        )
        .chain(
            (results.unused_namespace_members.iter())
                .filter_map(|e| member(&e.member, |rules| rules.unused_namespace_members)),
        );
    exports.chain(members).collect()
}
