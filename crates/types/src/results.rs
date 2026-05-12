//! Analysis result types for all issue categories.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::extract::MemberKind;
use crate::serde_path;
use crate::suppress::IssueKind;

/// Summary of detected entry points, grouped by discovery source.
///
/// Used to surface entry-point detection status in human and JSON output,
/// so library authors can verify that fallow found the right entry points.
#[derive(Debug, Clone, Default)]
pub struct EntryPointSummary {
    /// Total number of entry points detected.
    pub total: usize,
    /// Breakdown by source category (e.g., "package.json" -> 3, "plugin" -> 12).
    /// Sorted by key for deterministic output.
    pub by_source: Vec<(String, usize)>,
}

/// Complete analysis results.
///
/// # Examples
///
/// ```
/// use fallow_types::results::{AnalysisResults, UnusedFile};
/// use std::path::PathBuf;
///
/// let mut results = AnalysisResults::default();
/// assert_eq!(results.total_issues(), 0);
/// assert!(!results.has_issues());
///
/// results.unused_files.push(UnusedFile {
///     path: PathBuf::from("src/dead.ts"),
/// });
/// assert_eq!(results.total_issues(), 1);
/// assert!(results.has_issues());
/// ```
#[derive(Debug, Default, Clone, Serialize)]
pub struct AnalysisResults {
    /// Files not reachable from any entry point.
    pub unused_files: Vec<UnusedFile>,
    /// Exports never imported by other modules.
    pub unused_exports: Vec<UnusedExport>,
    /// Type exports never imported by other modules.
    pub unused_types: Vec<UnusedExport>,
    /// Exported symbols whose public signature references same-file private types.
    pub private_type_leaks: Vec<PrivateTypeLeak>,
    /// Dependencies listed in package.json but never imported.
    pub unused_dependencies: Vec<UnusedDependency>,
    /// Dev dependencies listed in package.json but never imported.
    pub unused_dev_dependencies: Vec<UnusedDependency>,
    /// Optional dependencies listed in package.json but never imported.
    pub unused_optional_dependencies: Vec<UnusedDependency>,
    /// Enum members never accessed.
    pub unused_enum_members: Vec<UnusedMember>,
    /// Class members never accessed.
    pub unused_class_members: Vec<UnusedMember>,
    /// Import specifiers that could not be resolved.
    pub unresolved_imports: Vec<UnresolvedImport>,
    /// Dependencies used in code but not listed in package.json.
    pub unlisted_dependencies: Vec<UnlistedDependency>,
    /// Exports with the same name across multiple modules.
    pub duplicate_exports: Vec<DuplicateExport>,
    /// Production dependencies only used via type-only imports (could be devDependencies).
    /// Only populated in production mode.
    pub type_only_dependencies: Vec<TypeOnlyDependency>,
    /// Production dependencies only imported by test files (could be devDependencies).
    #[serde(default)]
    pub test_only_dependencies: Vec<TestOnlyDependency>,
    /// Circular dependency chains detected in the module graph.
    pub circular_dependencies: Vec<CircularDependency>,
    /// Imports that cross architecture boundary rules.
    #[serde(default)]
    pub boundary_violations: Vec<BoundaryViolation>,
    /// Suppression comments or JSDoc tags that no longer match any issue.
    #[serde(default)]
    pub stale_suppressions: Vec<StaleSuppression>,
    /// Entries in pnpm-workspace.yaml catalogs that no workspace package references.
    #[serde(default)]
    pub unused_catalog_entries: Vec<UnusedCatalogEntry>,
    /// Workspace package.json references to pnpm catalogs that don't declare the package.
    #[serde(default)]
    pub unresolved_catalog_references: Vec<UnresolvedCatalogReference>,
    /// Entries in pnpm `overrides:` / `pnpm.overrides` that no workspace package depends on.
    #[serde(default)]
    pub unused_dependency_overrides: Vec<UnusedDependencyOverride>,
    /// Entries in pnpm `overrides:` / `pnpm.overrides` whose key or value cannot be parsed.
    #[serde(default)]
    pub misconfigured_dependency_overrides: Vec<MisconfiguredDependencyOverride>,
    /// Number of suppression entries that matched an issue during analysis.
    /// Human output uses this for the suppression footer; it is skipped in
    /// machine output to avoid changing the public JSON issue contract.
    #[serde(skip)]
    pub suppression_count: usize,
    /// Detected feature flag patterns. Advisory output, not included in issue counts.
    /// Skipped during default serialization: injected separately in JSON output when enabled.
    #[serde(skip)]
    pub feature_flags: Vec<FeatureFlag>,
    /// Usage counts for all exports across the project. Used by the LSP for Code Lens.
    /// Not included in issue counts -- this is metadata, not an issue type.
    /// Skipped during serialization: this is internal LSP data, not part of the JSON output schema.
    #[serde(skip)]
    pub export_usages: Vec<ExportUsage>,
    /// Summary of detected entry points, grouped by discovery source.
    /// Not included in issue counts -- this is informational metadata.
    /// Skipped during serialization: rendered separately in JSON output.
    #[serde(skip)]
    pub entry_point_summary: Option<EntryPointSummary>,
}

impl AnalysisResults {
    /// Total number of issues found.
    ///
    /// Sums across all issue categories (unused files, exports, types,
    /// dependencies, members, unresolved imports, unlisted deps, duplicates,
    /// type-only deps, circular deps, and boundary violations).
    ///
    /// # Examples
    ///
    /// ```
    /// use fallow_types::results::{AnalysisResults, UnusedFile, UnresolvedImport};
    /// use std::path::PathBuf;
    ///
    /// let mut results = AnalysisResults::default();
    /// results.unused_files.push(UnusedFile { path: PathBuf::from("a.ts") });
    /// results.unresolved_imports.push(UnresolvedImport {
    ///     path: PathBuf::from("b.ts"),
    ///     specifier: "./missing".to_string(),
    ///     line: 1,
    ///     col: 0,
    ///     specifier_col: 0,
    /// });
    /// assert_eq!(results.total_issues(), 2);
    /// ```
    #[must_use]
    pub const fn total_issues(&self) -> usize {
        self.unused_files.len()
            + self.unused_exports.len()
            + self.unused_types.len()
            + self.private_type_leaks.len()
            + self.unused_dependencies.len()
            + self.unused_dev_dependencies.len()
            + self.unused_optional_dependencies.len()
            + self.unused_enum_members.len()
            + self.unused_class_members.len()
            + self.unresolved_imports.len()
            + self.unlisted_dependencies.len()
            + self.duplicate_exports.len()
            + self.type_only_dependencies.len()
            + self.test_only_dependencies.len()
            + self.circular_dependencies.len()
            + self.boundary_violations.len()
            + self.stale_suppressions.len()
            + self.unused_catalog_entries.len()
            + self.unresolved_catalog_references.len()
            + self.unused_dependency_overrides.len()
            + self.misconfigured_dependency_overrides.len()
    }

    /// Whether any issues were found.
    #[must_use]
    pub const fn has_issues(&self) -> bool {
        self.total_issues() > 0
    }

    /// Sort all result arrays for deterministic output ordering.
    ///
    /// Parallel collection (rayon, `FxHashMap` iteration) does not guarantee
    /// insertion order, so the same project can produce different orderings
    /// across runs. This method canonicalises every result list by sorting on
    /// (path, line, col, name) so that JSON/SARIF/human output is stable.
    #[expect(
        clippy::too_many_lines,
        reason = "one short sort_by per result array; splitting would add indirection without clarity"
    )]
    pub fn sort(&mut self) {
        self.unused_files.sort_by(|a, b| a.path.cmp(&b.path));

        self.unused_exports.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.export_name.cmp(&b.export_name))
        });

        self.unused_types.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.export_name.cmp(&b.export_name))
        });

        self.private_type_leaks.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.export_name.cmp(&b.export_name))
                .then(a.type_name.cmp(&b.type_name))
        });

        self.unused_dependencies.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.package_name.cmp(&b.package_name))
        });

        self.unused_dev_dependencies.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.package_name.cmp(&b.package_name))
        });

        self.unused_optional_dependencies.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.package_name.cmp(&b.package_name))
        });

        self.unused_enum_members.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.parent_name.cmp(&b.parent_name))
                .then(a.member_name.cmp(&b.member_name))
        });

        self.unused_class_members.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.parent_name.cmp(&b.parent_name))
                .then(a.member_name.cmp(&b.member_name))
        });

        self.unresolved_imports.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.specifier.cmp(&b.specifier))
        });

        self.unlisted_dependencies
            .sort_by(|a, b| a.package_name.cmp(&b.package_name));
        for dep in &mut self.unlisted_dependencies {
            dep.imported_from
                .sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        }

        self.duplicate_exports
            .sort_by(|a, b| a.export_name.cmp(&b.export_name));
        for dup in &mut self.duplicate_exports {
            dup.locations
                .sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        }

        self.type_only_dependencies.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.package_name.cmp(&b.package_name))
        });

        self.test_only_dependencies.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.package_name.cmp(&b.package_name))
        });

        self.circular_dependencies
            .sort_by(|a, b| a.files.cmp(&b.files).then(a.length.cmp(&b.length)));

        self.boundary_violations.sort_by(|a, b| {
            a.from_path
                .cmp(&b.from_path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
                .then(a.to_path.cmp(&b.to_path))
        });

        self.stale_suppressions.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
        });

        self.unused_catalog_entries.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| {
                    catalog_sort_key(&a.catalog_name).cmp(&catalog_sort_key(&b.catalog_name))
                })
                .then(a.catalog_name.cmp(&b.catalog_name))
                .then(a.entry_name.cmp(&b.entry_name))
        });
        for entry in &mut self.unused_catalog_entries {
            entry.hardcoded_consumers.sort();
            entry.hardcoded_consumers.dedup();
        }

        self.unresolved_catalog_references.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then_with(|| {
                    catalog_sort_key(&a.catalog_name).cmp(&catalog_sort_key(&b.catalog_name))
                })
                .then(a.catalog_name.cmp(&b.catalog_name))
                .then(a.entry_name.cmp(&b.entry_name))
        });
        for finding in &mut self.unresolved_catalog_references {
            finding.available_in_catalogs.sort();
            finding.available_in_catalogs.dedup();
        }

        self.unused_dependency_overrides.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.raw_key.cmp(&b.raw_key))
        });

        self.misconfigured_dependency_overrides.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.raw_key.cmp(&b.raw_key))
        });

        self.feature_flags.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.flag_name.cmp(&b.flag_name))
        });

        for usage in &mut self.export_usages {
            usage.reference_locations.sort_by(|a, b| {
                a.path
                    .cmp(&b.path)
                    .then(a.line.cmp(&b.line))
                    .then(a.col.cmp(&b.col))
            });
        }
        self.export_usages.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.export_name.cmp(&b.export_name))
        });
    }
}

/// Sort key for catalog names: the default catalog ("default") sorts before any named catalog.
fn catalog_sort_key(name: &str) -> (u8, &str) {
    if name == "default" {
        (0, name)
    } else {
        (1, name)
    }
}

/// A file that is not reachable from any entry point.
#[derive(Debug, Clone, Serialize)]
pub struct UnusedFile {
    /// Absolute path to the unused file.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
}

/// An export that is never imported by other modules.
#[derive(Debug, Clone, Serialize)]
pub struct UnusedExport {
    /// File containing the unused export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the unused export.
    pub export_name: String,
    /// Whether this is a type-only export.
    pub is_type_only: bool,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Byte offset into the source file (used by the fix command).
    pub span_start: u32,
    /// Whether this finding comes from a barrel/index re-export rather than the source definition.
    pub is_re_export: bool,
}

/// A public export signature that references a same-file private type.
#[derive(Debug, Clone, Serialize)]
pub struct PrivateTypeLeak {
    /// File containing the exported symbol.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Export whose public signature leaks the private type.
    pub export_name: String,
    /// Private type referenced by the public signature.
    pub type_name: String,
    /// 1-based line number of the leaking type reference.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Byte offset of the type reference.
    pub span_start: u32,
}

/// A dependency that is listed in package.json but never imported.
#[derive(Debug, Clone, Serialize)]
pub struct UnusedDependency {
    /// Package name, including internal workspace package names.
    pub package_name: String,
    /// Whether this is in `dependencies`, `devDependencies`, or `optionalDependencies`.
    pub location: DependencyLocation,
    /// Path to the package.json where this dependency is listed.
    /// For root deps this is `<root>/package.json`, for workspace deps it is `<ws>/package.json`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
    /// Workspace roots that import this package even though the declaring workspace does not.
    #[serde(
        serialize_with = "serde_path::serialize_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub used_in_workspaces: Vec<PathBuf>,
}

/// Where in package.json a dependency is listed.
///
/// # Examples
///
/// ```
/// use fallow_types::results::DependencyLocation;
///
/// // All three variants are constructible
/// let loc = DependencyLocation::Dependencies;
/// let dev = DependencyLocation::DevDependencies;
/// let opt = DependencyLocation::OptionalDependencies;
/// // Debug output includes the variant name
/// assert!(format!("{loc:?}").contains("Dependencies"));
/// assert!(format!("{dev:?}").contains("DevDependencies"));
/// assert!(format!("{opt:?}").contains("OptionalDependencies"));
/// ```
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DependencyLocation {
    /// Listed in `dependencies`.
    Dependencies,
    /// Listed in `devDependencies`.
    DevDependencies,
    /// Listed in `optionalDependencies`.
    OptionalDependencies,
}

/// An unused enum or class member.
#[derive(Debug, Clone, Serialize)]
pub struct UnusedMember {
    /// File containing the unused member.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the parent enum or class.
    pub parent_name: String,
    /// Name of the unused member.
    pub member_name: String,
    /// Whether this is an enum member, class method, or class property.
    pub kind: MemberKind,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// An import that could not be resolved.
#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedImport {
    /// File containing the unresolved import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The import specifier that could not be resolved.
    pub specifier: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset of the import statement.
    pub col: u32,
    /// 0-based byte column offset of the source string literal (the specifier in quotes).
    /// Used by the LSP to underline just the specifier, not the entire import line.
    pub specifier_col: u32,
}

/// A dependency used in code but not listed in package.json.
#[derive(Debug, Clone, Serialize)]
pub struct UnlistedDependency {
    /// Package name, including internal workspace package names.
    pub package_name: String,
    /// Import sites where this unlisted dependency is used (file path, line, column).
    pub imported_from: Vec<ImportSite>,
}

/// A location where an import occurs.
#[derive(Debug, Clone, Serialize)]
pub struct ImportSite {
    /// File containing the import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// An export that appears multiple times across the project.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateExport {
    /// The duplicated export name.
    pub export_name: String,
    /// Locations where this export name appears.
    pub locations: Vec<DuplicateLocation>,
}

/// A location where a duplicate export appears.
#[derive(Debug, Clone, Serialize)]
pub struct DuplicateLocation {
    /// File containing the duplicate export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// A production dependency that is only used via type-only imports.
/// In production builds, type imports are erased, so this dependency
/// is not needed at runtime and could be moved to devDependencies.
#[derive(Debug, Clone, Serialize)]
pub struct TypeOnlyDependency {
    /// npm package name.
    pub package_name: String,
    /// Path to the package.json where the dependency is listed.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
}

/// A pnpm catalog entry declared in pnpm-workspace.yaml that no workspace package
/// references via the `catalog:` protocol.
///
/// The default catalog (top-level `catalog:` key) uses `catalog_name: "default"`.
/// Named catalogs (under `catalogs.<name>:`) use their declared name.
#[derive(Debug, Clone, Serialize)]
pub struct UnusedCatalogEntry {
    /// Package name declared in the catalog (e.g. `"react"`, `"@scope/lib"`).
    pub entry_name: String,
    /// Catalog group: `"default"` for the top-level `catalog:` map, or the
    /// named catalog key for entries declared under `catalogs.<name>:`.
    pub catalog_name: String,
    /// Path to `pnpm-workspace.yaml`, relative to the analyzed root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the catalog entry within `pnpm-workspace.yaml`.
    pub line: u32,
    /// Workspace `package.json` files that declare the same package with a
    /// hardcoded version range instead of `catalog:`. Empty when no consumer
    /// uses a hardcoded version. Sorted lexicographically for deterministic
    /// output.
    #[serde(
        default,
        serialize_with = "serde_path::serialize_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub hardcoded_consumers: Vec<PathBuf>,
}

/// A workspace package.json reference (`catalog:` or `catalog:<name>`) that points
/// at a catalog which does not declare the consumed package.
///
/// `pnpm install` errors at install time with `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_CATALOG_PROTOCOL`
/// when this happens. fallow surfaces it statically so the failure is caught at
/// `fallow check` time, before any install.
///
/// The default catalog (bare `catalog:` references the top-level `catalog:` map)
/// uses `catalog_name: "default"`. Named catalogs (`catalog:react17`) use the
/// declared catalog name.
#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedCatalogReference {
    /// Package name being referenced via the catalog protocol (e.g. `"react"`).
    pub entry_name: String,
    /// Catalog group the reference points at: `"default"` for bare `catalog:` references,
    /// or the named catalog key for `catalog:<name>` references.
    pub catalog_name: String,
    /// Absolute path to the consumer `package.json`. Matches the storage
    /// convention used by every path-anchored finding type (`UnusedFile`,
    /// `UnresolvedImport`, `UnusedExport`, etc.) so the shared filtering
    /// pipelines (`filter_results_by_changed_files`, per-file overrides,
    /// audit attribution) work without a separate root-join pass. JSON
    /// output strips the project-root prefix via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in the consumer `package.json`.
    pub line: u32,
    /// Other catalogs (in the same `pnpm-workspace.yaml`) that DO declare this
    /// package. Empty when no catalog has the package. Sorted lexicographically.
    /// Lets agents and humans decide whether to switch the reference to a
    /// different catalog or to add the entry to the named catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_in_catalogs: Vec<String>,
}

/// Where an override entry was declared. Serialized as the filename label
/// (`"pnpm-workspace.yaml"` or `"package.json"`) so the value in JSON output
/// matches the value users write in `ignoreDependencyOverrides[].source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DependencyOverrideSource {
    /// Top-level `overrides:` key in `pnpm-workspace.yaml`.
    #[serde(rename = "pnpm-workspace.yaml")]
    PnpmWorkspaceYaml,
    /// `pnpm.overrides` in a root `package.json`.
    #[serde(rename = "package.json")]
    PnpmPackageJson,
}

impl DependencyOverrideSource {
    /// Stable string label matching the serde rename. Used in baseline keys,
    /// audit keys, jq comparisons, and `ignoreDependencyOverrides[].source`.
    #[must_use]
    pub const fn as_label(&self) -> &'static str {
        match self {
            Self::PnpmWorkspaceYaml => "pnpm-workspace.yaml",
            Self::PnpmPackageJson => "package.json",
        }
    }
}

impl std::fmt::Display for DependencyOverrideSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_label())
    }
}

/// An entry in pnpm's `overrides:` map (or the legacy `pnpm.overrides` in
/// `package.json`) whose target package is not declared in any workspace
/// `package.json`. Conservative static algorithm: no lockfile read, so this
/// can produce false negatives for overrides targeting purely-transitive
/// packages (e.g. CVE-fix overrides). The `hint` field flags that case.
#[derive(Debug, Clone, Serialize)]
pub struct UnusedDependencyOverride {
    /// The full original override key as written in the source (e.g.
    /// `"react>react-dom"`, `"@types/react@<18"`). Preserved for round-trip
    /// reporting so agents see the unmodified spelling.
    pub raw_key: String,
    /// The target package the override rewrites (e.g. `"react-dom"` for
    /// `"react>react-dom"`, `"@types/react"` for `"@types/react@<18"`).
    pub target_package: String,
    /// Optional parent package (left side of `>`). `None` for bare-target keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_package: Option<String>,
    /// Optional version selector on the target (e.g. `Some("<18")` for
    /// `"@types/react@<18"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_constraint: Option<String>,
    /// The right-hand side of the entry: the version pnpm should force.
    pub version_range: String,
    /// Where the override entry was declared.
    pub source: DependencyOverrideSource,
    /// Path to the source file. `pnpm-workspace.yaml` or a `package.json`,
    /// stored as an absolute filesystem path so `--changed-since` and
    /// per-file `overrides.rules` can compare directly against the analyzer's
    /// changed-set / per-path rule lookups. JSON serialization strips the
    /// project root via `serde_path::serialize`, matching the
    /// `UnresolvedCatalogReference` convention.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the entry within the source file.
    pub line: u32,
    /// Soft hint for cases the conservative algorithm cannot disambiguate.
    /// Emitted on every unused-override finding (both bare-target and
    /// parent-chain shapes) so consumers know the result may be a false
    /// positive against a transitive dependency (CVE-fix / canary-aliasing
    /// pattern).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Why a dependency-override entry is misconfigured. `pnpm install` would
/// either fail at install time or silently no-op on these entries; surfacing
/// them statically catches the issue before pnpm does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyOverrideMisconfigReason {
    /// The override key could not be parsed into a recognised pnpm shape
    /// (e.g. dangling `>`, missing target, garbage characters).
    UnparsableKey,
    /// The override value is missing, empty, or contains line breaks.
    EmptyValue,
}

impl DependencyOverrideMisconfigReason {
    /// Human-readable summary of the reason.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::UnparsableKey => "override key cannot be parsed",
            Self::EmptyValue => "override value is missing or empty",
        }
    }
}

/// An override entry whose key or value is malformed. Default severity is
/// `error` because pnpm refuses to install (or silently produces a no-op
/// override) when it encounters these shapes.
#[derive(Debug, Clone, Serialize)]
pub struct MisconfiguredDependencyOverride {
    /// The full original override key as written in the source.
    pub raw_key: String,
    /// Parsed target package name when the key was syntactically valid (the
    /// `EmptyValue` reason path). `None` for `UnparsableKey` findings whose
    /// key could not be parsed at all. Used by JSON `add-to-config` actions to
    /// emit a paste-ready `ignoreDependencyOverrides` value that matches the
    /// suppression matcher (which also keys on `target_package`); avoids the
    /// pitfall where `raw_key` like `"react@<18"` would not match the rule
    /// that targets package `"react"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_package: Option<String>,
    /// The right-hand side of the entry, exactly as written. Empty when the
    /// value was missing.
    pub raw_value: String,
    /// Classifier for the misconfiguration.
    pub reason: DependencyOverrideMisconfigReason,
    /// Where the override entry was declared.
    pub source: DependencyOverrideSource,
    /// Path to the source file. Stored as an absolute filesystem path so
    /// `--changed-since` and per-file `overrides.rules` can compare directly.
    /// JSON serialization strips the project root via `serde_path::serialize`.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the entry within the source file.
    pub line: u32,
}

/// A production dependency that is only imported by test files.
/// Since it is never used in production code, it could be moved to devDependencies.
#[derive(Debug, Clone, Serialize)]
pub struct TestOnlyDependency {
    /// npm package name.
    pub package_name: String,
    /// Path to the package.json where the dependency is listed.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
}

/// A circular dependency chain detected in the module graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircularDependency {
    /// Files forming the cycle, in import order.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// Number of files in the cycle.
    pub length: usize,
    /// 1-based line number of the import that starts the cycle (in the first file).
    #[serde(default)]
    pub line: u32,
    /// 0-based byte column offset of the import that starts the cycle.
    #[serde(default)]
    pub col: u32,
    /// Whether this cycle crosses workspace package boundaries.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_cross_package: bool,
}

/// An import that crosses an architecture boundary rule.
#[derive(Debug, Clone, Serialize)]
pub struct BoundaryViolation {
    /// The file making the disallowed import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub from_path: PathBuf,
    /// The file being imported that violates the boundary.
    #[serde(serialize_with = "serde_path::serialize")]
    pub to_path: PathBuf,
    /// The zone the importing file belongs to.
    pub from_zone: String,
    /// The zone the imported file belongs to.
    pub to_zone: String,
    /// The raw import specifier from the source file.
    pub import_specifier: String,
    /// 1-based line number of the import statement in the source file.
    pub line: u32,
    /// 0-based byte column offset of the import statement.
    pub col: u32,
}

/// The origin of a stale suppression: inline comment or JSDoc tag.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum SuppressionOrigin {
    /// A `// fallow-ignore-next-line` or `// fallow-ignore-file` comment.
    Comment {
        /// The issue kind token from the comment (e.g., "unused-exports"), or None for blanket.
        #[serde(skip_serializing_if = "Option::is_none")]
        issue_kind: Option<String>,
        /// Whether this was a file-level suppression.
        is_file_level: bool,
    },
    /// An `@expected-unused` JSDoc tag on an export.
    JsdocTag {
        /// The name of the export that was tagged.
        export_name: String,
    },
}

/// A suppression comment or JSDoc tag that no longer matches any issue.
#[derive(Debug, Clone, Serialize)]
pub struct StaleSuppression {
    /// File containing the stale suppression.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the suppression comment or tag.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// The origin and details of the stale suppression.
    pub origin: SuppressionOrigin,
}

impl StaleSuppression {
    /// Produce a human-readable description of this stale suppression.
    #[must_use]
    pub fn description(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                issue_kind,
                is_file_level,
            } => {
                let directive = if *is_file_level {
                    "fallow-ignore-file"
                } else {
                    "fallow-ignore-next-line"
                };
                match issue_kind {
                    Some(kind) => format!("// {directive} {kind}"),
                    None => format!("// {directive}"),
                }
            }
            SuppressionOrigin::JsdocTag { export_name } => {
                format!("@expected-unused on {export_name}")
            }
        }
    }

    /// Produce an explanation of why this suppression is stale.
    #[must_use]
    pub fn explanation(&self) -> String {
        match &self.origin {
            SuppressionOrigin::Comment {
                issue_kind,
                is_file_level,
            } => {
                let scope = if *is_file_level {
                    "in this file"
                } else {
                    "on the next line"
                };
                match issue_kind {
                    Some(kind) => format!("no {kind} issue found {scope}"),
                    None => format!("no issues found {scope}"),
                }
            }
            SuppressionOrigin::JsdocTag { export_name } => {
                format!("{export_name} is now used")
            }
        }
    }

    /// The suppressed `IssueKind`, if this was a comment suppression with a specific kind.
    #[must_use]
    pub fn suppressed_kind(&self) -> Option<IssueKind> {
        match &self.origin {
            SuppressionOrigin::Comment { issue_kind, .. } => {
                issue_kind.as_deref().and_then(IssueKind::parse)
            }
            SuppressionOrigin::JsdocTag { .. } => None,
        }
    }
}

/// The detection method used to identify a feature flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagKind {
    /// Environment variable check (e.g., `process.env.FEATURE_X`).
    EnvironmentVariable,
    /// Feature flag SDK call (e.g., `useFlag('name')`, `variation('name', false)`).
    SdkCall,
    /// Config object property access (e.g., `config.features.newCheckout`).
    ConfigObject,
}

/// Detection confidence for a feature flag finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagConfidence {
    /// Low confidence: heuristic match (config object patterns).
    Low,
    /// Medium confidence: pattern match with some ambiguity.
    Medium,
    /// High confidence: unambiguous pattern (env vars, direct SDK calls).
    High,
}

/// A detected feature flag use site.
#[derive(Debug, Clone, Serialize)]
pub struct FeatureFlag {
    /// File containing the feature flag usage.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name or identifier of the flag (e.g., `ENABLE_NEW_CHECKOUT`, `new-checkout`).
    pub flag_name: String,
    /// How the flag was detected.
    pub kind: FlagKind,
    /// Detection confidence level.
    pub confidence: FlagConfidence,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Start byte offset of the guarded code block (if-branch span), if detected.
    #[serde(skip)]
    pub guard_span_start: Option<u32>,
    /// End byte offset of the guarded code block (if-branch span), if detected.
    #[serde(skip)]
    pub guard_span_end: Option<u32>,
    /// SDK or provider name (e.g., "LaunchDarkly", "Statsig"), if detected from SDK call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk_name: Option<String>,
    /// Line range of the guarded code block (derived from guard_span + line_offsets).
    /// Used for cross-reference with dead code findings.
    #[serde(skip)]
    pub guard_line_start: Option<u32>,
    /// End line of the guarded code block.
    #[serde(skip)]
    pub guard_line_end: Option<u32>,
    /// Unused exports found within the guarded code block.
    /// Populated by cross-reference with dead code analysis.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub guarded_dead_exports: Vec<String>,
}

// Size assertion: FeatureFlag is stored in a Vec per analysis run.
const _: () = assert!(std::mem::size_of::<FeatureFlag>() <= 160);

/// Usage count for an export symbol. Used by the LSP Code Lens to show
/// reference counts above each export declaration.
#[derive(Debug, Clone, Serialize)]
pub struct ExportUsage {
    /// File containing the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the exported symbol.
    pub export_name: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// Number of files that reference this export.
    pub reference_count: usize,
    /// Locations where this export is referenced. Used by the LSP Code Lens
    /// to enable click-to-navigate via `editor.action.showReferences`.
    pub reference_locations: Vec<ReferenceLocation>,
}

/// A location where an export is referenced (import site in another file).
#[derive(Debug, Clone, Serialize)]
pub struct ReferenceLocation {
    /// File containing the import that references the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_results_no_issues() {
        let results = AnalysisResults::default();
        assert_eq!(results.total_issues(), 0);
        assert!(!results.has_issues());
    }

    #[test]
    fn results_with_unused_file() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("test.ts"),
        });
        assert_eq!(results.total_issues(), 1);
        assert!(results.has_issues());
    }

    #[test]
    fn results_with_unused_export() {
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: PathBuf::from("test.ts"),
            export_name: "foo".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        assert_eq!(results.total_issues(), 1);
        assert!(results.has_issues());
    }

    #[test]
    fn results_total_counts_all_types() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("a.ts"),
        });
        results.unused_exports.push(UnusedExport {
            path: PathBuf::from("b.ts"),
            export_name: "x".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_types.push(UnusedExport {
            path: PathBuf::from("c.ts"),
            export_name: "T".to_string(),
            is_type_only: true,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_dependencies.push(UnusedDependency {
            package_name: "dep".to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_dev_dependencies.push(UnusedDependency {
            package_name: "dev".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("d.ts"),
            parent_name: "E".to_string(),
            member_name: "A".to_string(),
            kind: MemberKind::EnumMember,
            line: 1,
            col: 0,
        });
        results.unused_class_members.push(UnusedMember {
            path: PathBuf::from("e.ts"),
            parent_name: "C".to_string(),
            member_name: "m".to_string(),
            kind: MemberKind::ClassMethod,
            line: 1,
            col: 0,
        });
        results.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("f.ts"),
            specifier: "./missing".to_string(),
            line: 1,
            col: 0,
            specifier_col: 0,
        });
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "unlisted".to_string(),
            imported_from: vec![ImportSite {
                path: PathBuf::from("g.ts"),
                line: 1,
                col: 0,
            }],
        });
        results.duplicate_exports.push(DuplicateExport {
            export_name: "dup".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("h.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("i.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        });
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "optional".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "type-only".to_string(),
            path: PathBuf::from("package.json"),
            line: 8,
        });
        results.test_only_dependencies.push(TestOnlyDependency {
            package_name: "test-only".to_string(),
            path: PathBuf::from("package.json"),
            line: 9,
        });
        results.circular_dependencies.push(CircularDependency {
            files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
            length: 2,
            line: 3,
            col: 0,
            is_cross_package: false,
        });
        results.boundary_violations.push(BoundaryViolation {
            from_path: PathBuf::from("src/ui/Button.tsx"),
            to_path: PathBuf::from("src/db/queries.ts"),
            from_zone: "ui".to_string(),
            to_zone: "database".to_string(),
            import_specifier: "../db/queries".to_string(),
            line: 3,
            col: 0,
        });

        // 15 categories, one of each
        assert_eq!(results.total_issues(), 15);
        assert!(results.has_issues());
    }

    // ── total_issues / has_issues consistency ──────────────────

    #[test]
    fn total_issues_and_has_issues_are_consistent() {
        let results = AnalysisResults::default();
        assert_eq!(results.total_issues(), 0);
        assert!(!results.has_issues());
        assert_eq!(results.total_issues() > 0, results.has_issues());
    }

    // ── total_issues counts each category independently ─────────

    #[test]
    fn total_issues_sums_all_categories_independently() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("a.ts"),
        });
        assert_eq!(results.total_issues(), 1);

        results.unused_files.push(UnusedFile {
            path: PathBuf::from("b.ts"),
        });
        assert_eq!(results.total_issues(), 2);

        results.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("c.ts"),
            specifier: "./missing".to_string(),
            line: 1,
            col: 0,
            specifier_col: 0,
        });
        assert_eq!(results.total_issues(), 3);
    }

    // ── default is truly empty ──────────────────────────────────

    #[test]
    fn default_results_all_fields_empty() {
        let r = AnalysisResults::default();
        assert!(r.unused_files.is_empty());
        assert!(r.unused_exports.is_empty());
        assert!(r.unused_types.is_empty());
        assert!(r.unused_dependencies.is_empty());
        assert!(r.unused_dev_dependencies.is_empty());
        assert!(r.unused_optional_dependencies.is_empty());
        assert!(r.unused_enum_members.is_empty());
        assert!(r.unused_class_members.is_empty());
        assert!(r.unresolved_imports.is_empty());
        assert!(r.unlisted_dependencies.is_empty());
        assert!(r.duplicate_exports.is_empty());
        assert!(r.type_only_dependencies.is_empty());
        assert!(r.test_only_dependencies.is_empty());
        assert!(r.circular_dependencies.is_empty());
        assert!(r.boundary_violations.is_empty());
        assert!(r.unused_catalog_entries.is_empty());
        assert!(r.unresolved_catalog_references.is_empty());
        assert!(r.export_usages.is_empty());
    }

    // ── EntryPointSummary ────────────────────────────────────────

    #[test]
    fn entry_point_summary_default() {
        let summary = EntryPointSummary::default();
        assert_eq!(summary.total, 0);
        assert!(summary.by_source.is_empty());
    }

    #[test]
    fn entry_point_summary_not_in_default_results() {
        let r = AnalysisResults::default();
        assert!(r.entry_point_summary.is_none());
    }

    #[test]
    fn entry_point_summary_some_preserves_data() {
        let r = AnalysisResults {
            entry_point_summary: Some(EntryPointSummary {
                total: 5,
                by_source: vec![("package.json".to_string(), 2), ("plugin".to_string(), 3)],
            }),
            ..AnalysisResults::default()
        };
        let summary = r.entry_point_summary.as_ref().unwrap();
        assert_eq!(summary.total, 5);
        assert_eq!(summary.by_source.len(), 2);
        assert_eq!(summary.by_source[0], ("package.json".to_string(), 2));
    }

    // ── sort: unused_files by path ──────────────────────────────

    #[test]
    fn sort_unused_files_by_path() {
        let mut r = AnalysisResults::default();
        r.unused_files.push(UnusedFile {
            path: PathBuf::from("z.ts"),
        });
        r.unused_files.push(UnusedFile {
            path: PathBuf::from("a.ts"),
        });
        r.unused_files.push(UnusedFile {
            path: PathBuf::from("m.ts"),
        });
        r.sort();
        let paths: Vec<_> = r
            .unused_files
            .iter()
            .map(|f| f.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(paths, vec!["a.ts", "m.ts", "z.ts"]);
    }

    // ── sort: unused_exports by path, line, name ────────────────

    #[test]
    fn sort_unused_exports_by_path_line_name() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, name: &str| UnusedExport {
            path: PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: false,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
        };
        r.unused_exports.push(mk("b.ts", 5, "beta"));
        r.unused_exports.push(mk("a.ts", 10, "zeta"));
        r.unused_exports.push(mk("a.ts", 10, "alpha"));
        r.unused_exports.push(mk("a.ts", 1, "gamma"));
        r.sort();
        let keys: Vec<_> = r
            .unused_exports
            .iter()
            .map(|e| format!("{}:{}:{}", e.path.to_string_lossy(), e.line, e.export_name))
            .collect();
        assert_eq!(
            keys,
            vec![
                "a.ts:1:gamma",
                "a.ts:10:alpha",
                "a.ts:10:zeta",
                "b.ts:5:beta"
            ]
        );
    }

    // ── sort: unused_types (same sort as unused_exports) ────────

    #[test]
    fn sort_unused_types_by_path_line_name() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, name: &str| UnusedExport {
            path: PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: true,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
        };
        r.unused_types.push(mk("z.ts", 1, "Z"));
        r.unused_types.push(mk("a.ts", 1, "A"));
        r.sort();
        assert_eq!(r.unused_types[0].path, PathBuf::from("a.ts"));
        assert_eq!(r.unused_types[1].path, PathBuf::from("z.ts"));
    }

    // ── sort: unused_dependencies by path, line, name ───────────

    #[test]
    fn sort_unused_dependencies_by_path_line_name() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, name: &str| UnusedDependency {
            package_name: name.to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from(path),
            line,
            used_in_workspaces: Vec::new(),
        };
        r.unused_dependencies.push(mk("b/package.json", 3, "zlib"));
        r.unused_dependencies.push(mk("a/package.json", 5, "react"));
        r.unused_dependencies.push(mk("a/package.json", 5, "axios"));
        r.sort();
        let names: Vec<_> = r
            .unused_dependencies
            .iter()
            .map(|d| d.package_name.as_str())
            .collect();
        assert_eq!(names, vec!["axios", "react", "zlib"]);
    }

    // ── sort: unused_dev_dependencies ───────────────────────────

    #[test]
    fn sort_unused_dev_dependencies() {
        let mut r = AnalysisResults::default();
        r.unused_dev_dependencies.push(UnusedDependency {
            package_name: "vitest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 10,
            used_in_workspaces: Vec::new(),
        });
        r.unused_dev_dependencies.push(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        r.sort();
        assert_eq!(r.unused_dev_dependencies[0].package_name, "jest");
        assert_eq!(r.unused_dev_dependencies[1].package_name, "vitest");
    }

    // ── sort: unused_optional_dependencies ──────────────────────

    #[test]
    fn sort_unused_optional_dependencies() {
        let mut r = AnalysisResults::default();
        r.unused_optional_dependencies.push(UnusedDependency {
            package_name: "zod".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("package.json"),
            line: 3,
            used_in_workspaces: Vec::new(),
        });
        r.unused_optional_dependencies.push(UnusedDependency {
            package_name: "ajv".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("package.json"),
            line: 2,
            used_in_workspaces: Vec::new(),
        });
        r.sort();
        assert_eq!(r.unused_optional_dependencies[0].package_name, "ajv");
        assert_eq!(r.unused_optional_dependencies[1].package_name, "zod");
    }

    // ── sort: unused_enum_members by path, line, parent, member ─

    #[test]
    fn sort_unused_enum_members_by_path_line_parent_member() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, parent: &str, member: &str| UnusedMember {
            path: PathBuf::from(path),
            parent_name: parent.to_string(),
            member_name: member.to_string(),
            kind: MemberKind::EnumMember,
            line,
            col: 0,
        };
        r.unused_enum_members.push(mk("a.ts", 5, "Status", "Z"));
        r.unused_enum_members.push(mk("a.ts", 5, "Status", "A"));
        r.unused_enum_members.push(mk("a.ts", 1, "Direction", "Up"));
        r.sort();
        let keys: Vec<_> = r
            .unused_enum_members
            .iter()
            .map(|m| format!("{}:{}", m.parent_name, m.member_name))
            .collect();
        assert_eq!(keys, vec!["Direction:Up", "Status:A", "Status:Z"]);
    }

    // ── sort: unused_class_members by path, line, parent, member

    #[test]
    fn sort_unused_class_members() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, parent: &str, member: &str| UnusedMember {
            path: PathBuf::from(path),
            parent_name: parent.to_string(),
            member_name: member.to_string(),
            kind: MemberKind::ClassMethod,
            line,
            col: 0,
        };
        r.unused_class_members.push(mk("b.ts", 1, "Foo", "z"));
        r.unused_class_members.push(mk("a.ts", 1, "Bar", "a"));
        r.sort();
        assert_eq!(r.unused_class_members[0].path, PathBuf::from("a.ts"));
        assert_eq!(r.unused_class_members[1].path, PathBuf::from("b.ts"));
    }

    // ── sort: unresolved_imports by path, line, col, specifier ──

    #[test]
    fn sort_unresolved_imports_by_path_line_col_specifier() {
        let mut r = AnalysisResults::default();
        let mk = |path: &str, line: u32, col: u32, spec: &str| UnresolvedImport {
            path: PathBuf::from(path),
            specifier: spec.to_string(),
            line,
            col,
            specifier_col: 0,
        };
        r.unresolved_imports.push(mk("a.ts", 5, 0, "./z"));
        r.unresolved_imports.push(mk("a.ts", 5, 0, "./a"));
        r.unresolved_imports.push(mk("a.ts", 1, 0, "./m"));
        r.sort();
        let specs: Vec<_> = r
            .unresolved_imports
            .iter()
            .map(|i| i.specifier.as_str())
            .collect();
        assert_eq!(specs, vec!["./m", "./a", "./z"]);
    }

    // ── sort: unlisted_dependencies + inner imported_from ───────

    #[test]
    fn sort_unlisted_dependencies_by_name_and_inner_sites() {
        let mut r = AnalysisResults::default();
        r.unlisted_dependencies.push(UnlistedDependency {
            package_name: "zod".to_string(),
            imported_from: vec![
                ImportSite {
                    path: PathBuf::from("b.ts"),
                    line: 10,
                    col: 0,
                },
                ImportSite {
                    path: PathBuf::from("a.ts"),
                    line: 1,
                    col: 0,
                },
            ],
        });
        r.unlisted_dependencies.push(UnlistedDependency {
            package_name: "axios".to_string(),
            imported_from: vec![ImportSite {
                path: PathBuf::from("c.ts"),
                line: 1,
                col: 0,
            }],
        });
        r.sort();

        // Outer sort: by package_name
        assert_eq!(r.unlisted_dependencies[0].package_name, "axios");
        assert_eq!(r.unlisted_dependencies[1].package_name, "zod");

        // Inner sort: imported_from sorted by path, then line
        let zod_sites: Vec<_> = r.unlisted_dependencies[1]
            .imported_from
            .iter()
            .map(|s| s.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(zod_sites, vec!["a.ts", "b.ts"]);
    }

    // ── sort: duplicate_exports + inner locations ───────────────

    #[test]
    fn sort_duplicate_exports_by_name_and_inner_locations() {
        let mut r = AnalysisResults::default();
        r.duplicate_exports.push(DuplicateExport {
            export_name: "z".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("c.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("a.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        });
        r.duplicate_exports.push(DuplicateExport {
            export_name: "a".to_string(),
            locations: vec![DuplicateLocation {
                path: PathBuf::from("b.ts"),
                line: 1,
                col: 0,
            }],
        });
        r.sort();

        // Outer sort: by export_name
        assert_eq!(r.duplicate_exports[0].export_name, "a");
        assert_eq!(r.duplicate_exports[1].export_name, "z");

        // Inner sort: locations sorted by path, then line
        let z_locs: Vec<_> = r.duplicate_exports[1]
            .locations
            .iter()
            .map(|l| l.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(z_locs, vec!["a.ts", "c.ts"]);
    }

    // ── sort: type_only_dependencies ────────────────────────────

    #[test]
    fn sort_type_only_dependencies() {
        let mut r = AnalysisResults::default();
        r.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: PathBuf::from("package.json"),
            line: 10,
        });
        r.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "ajv".to_string(),
            path: PathBuf::from("package.json"),
            line: 5,
        });
        r.sort();
        assert_eq!(r.type_only_dependencies[0].package_name, "ajv");
        assert_eq!(r.type_only_dependencies[1].package_name, "zod");
    }

    // ── sort: test_only_dependencies ────────────────────────────

    #[test]
    fn sort_test_only_dependencies() {
        let mut r = AnalysisResults::default();
        r.test_only_dependencies.push(TestOnlyDependency {
            package_name: "vitest".to_string(),
            path: PathBuf::from("package.json"),
            line: 15,
        });
        r.test_only_dependencies.push(TestOnlyDependency {
            package_name: "jest".to_string(),
            path: PathBuf::from("package.json"),
            line: 10,
        });
        r.sort();
        assert_eq!(r.test_only_dependencies[0].package_name, "jest");
        assert_eq!(r.test_only_dependencies[1].package_name, "vitest");
    }

    // ── sort: circular_dependencies by files, then length ───────

    #[test]
    fn sort_circular_dependencies_by_files_then_length() {
        let mut r = AnalysisResults::default();
        r.circular_dependencies.push(CircularDependency {
            files: vec![PathBuf::from("b.ts"), PathBuf::from("c.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        r.circular_dependencies.push(CircularDependency {
            files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: true,
        });
        r.sort();
        assert_eq!(r.circular_dependencies[0].files[0], PathBuf::from("a.ts"));
        assert_eq!(r.circular_dependencies[1].files[0], PathBuf::from("b.ts"));
    }

    // ── sort: boundary_violations by from_path, line, col, to_path

    #[test]
    fn sort_boundary_violations() {
        let mut r = AnalysisResults::default();
        let mk = |from: &str, line: u32, col: u32, to: &str| BoundaryViolation {
            from_path: PathBuf::from(from),
            to_path: PathBuf::from(to),
            from_zone: "a".to_string(),
            to_zone: "b".to_string(),
            import_specifier: to.to_string(),
            line,
            col,
        };
        r.boundary_violations.push(mk("z.ts", 1, 0, "a.ts"));
        r.boundary_violations.push(mk("a.ts", 5, 0, "b.ts"));
        r.boundary_violations.push(mk("a.ts", 1, 0, "c.ts"));
        r.sort();
        let from_paths: Vec<_> = r
            .boundary_violations
            .iter()
            .map(|v| format!("{}:{}", v.from_path.to_string_lossy(), v.line))
            .collect();
        assert_eq!(from_paths, vec!["a.ts:1", "a.ts:5", "z.ts:1"]);
    }

    // ── sort: export_usages + inner reference_locations ─────────

    #[test]
    fn sort_export_usages_and_inner_reference_locations() {
        let mut r = AnalysisResults::default();
        r.export_usages.push(ExportUsage {
            path: PathBuf::from("z.ts"),
            export_name: "foo".to_string(),
            line: 1,
            col: 0,
            reference_count: 2,
            reference_locations: vec![
                ReferenceLocation {
                    path: PathBuf::from("c.ts"),
                    line: 10,
                    col: 0,
                },
                ReferenceLocation {
                    path: PathBuf::from("a.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        });
        r.export_usages.push(ExportUsage {
            path: PathBuf::from("a.ts"),
            export_name: "bar".to_string(),
            line: 1,
            col: 0,
            reference_count: 1,
            reference_locations: vec![ReferenceLocation {
                path: PathBuf::from("b.ts"),
                line: 1,
                col: 0,
            }],
        });
        r.sort();

        // Outer sort: by path, then line, then export_name
        assert_eq!(r.export_usages[0].path, PathBuf::from("a.ts"));
        assert_eq!(r.export_usages[1].path, PathBuf::from("z.ts"));

        // Inner sort: reference_locations sorted by path, line, col
        let refs: Vec<_> = r.export_usages[1]
            .reference_locations
            .iter()
            .map(|l| l.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(refs, vec!["a.ts", "c.ts"]);
    }

    // ── sort: empty results does not panic ──────────────────────

    #[test]
    fn sort_empty_results_is_noop() {
        let mut r = AnalysisResults::default();
        r.sort(); // should not panic
        assert_eq!(r.total_issues(), 0);
    }

    // ── sort: single-element lists remain stable ────────────────

    #[test]
    fn sort_single_element_lists_stable() {
        let mut r = AnalysisResults::default();
        r.unused_files.push(UnusedFile {
            path: PathBuf::from("only.ts"),
        });
        r.sort();
        assert_eq!(r.unused_files[0].path, PathBuf::from("only.ts"));
    }

    // ── serialization ──────────────────────────────────────────

    #[test]
    fn serialize_empty_results() {
        let r = AnalysisResults::default();
        let json = serde_json::to_value(&r).unwrap();

        // All arrays should be present and empty
        assert!(json["unused_files"].as_array().unwrap().is_empty());
        assert!(json["unused_exports"].as_array().unwrap().is_empty());
        assert!(json["circular_dependencies"].as_array().unwrap().is_empty());

        // Skipped fields should be absent
        assert!(json.get("export_usages").is_none());
        assert!(json.get("entry_point_summary").is_none());
    }

    #[test]
    fn serialize_unused_file_path() {
        let r = UnusedFile {
            path: PathBuf::from("src/utils/index.ts"),
        };
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["path"], "src/utils/index.ts");
    }

    #[test]
    fn serialize_dependency_location_camel_case() {
        let dep = UnusedDependency {
            package_name: "react".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        };
        let json = serde_json::to_value(&dep).unwrap();
        assert_eq!(json["location"], "devDependencies");

        let dep2 = UnusedDependency {
            package_name: "react".to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("package.json"),
            line: 3,
            used_in_workspaces: Vec::new(),
        };
        let json2 = serde_json::to_value(&dep2).unwrap();
        assert_eq!(json2["location"], "dependencies");

        let dep3 = UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("package.json"),
            line: 7,
            used_in_workspaces: Vec::new(),
        };
        let json3 = serde_json::to_value(&dep3).unwrap();
        assert_eq!(json3["location"], "optionalDependencies");
    }

    #[test]
    fn serialize_circular_dependency_skips_false_cross_package() {
        let cd = CircularDependency {
            files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        };
        let json = serde_json::to_value(&cd).unwrap();
        // skip_serializing_if = "std::ops::Not::not" means false is skipped
        assert!(json.get("is_cross_package").is_none());
    }

    #[test]
    fn serialize_circular_dependency_includes_true_cross_package() {
        let cd = CircularDependency {
            files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: true,
        };
        let json = serde_json::to_value(&cd).unwrap();
        assert_eq!(json["is_cross_package"], true);
    }

    #[test]
    fn serialize_unused_export_fields() {
        let e = UnusedExport {
            path: PathBuf::from("src/mod.ts"),
            export_name: "helper".to_string(),
            is_type_only: true,
            line: 42,
            col: 7,
            span_start: 100,
            is_re_export: true,
        };
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["path"], "src/mod.ts");
        assert_eq!(json["export_name"], "helper");
        assert_eq!(json["is_type_only"], true);
        assert_eq!(json["line"], 42);
        assert_eq!(json["col"], 7);
        assert_eq!(json["span_start"], 100);
        assert_eq!(json["is_re_export"], true);
    }

    #[test]
    fn serialize_boundary_violation_fields() {
        let v = BoundaryViolation {
            from_path: PathBuf::from("src/ui/button.tsx"),
            to_path: PathBuf::from("src/db/queries.ts"),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            import_specifier: "../db/queries".to_string(),
            line: 3,
            col: 0,
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["from_path"], "src/ui/button.tsx");
        assert_eq!(json["to_path"], "src/db/queries.ts");
        assert_eq!(json["from_zone"], "ui");
        assert_eq!(json["to_zone"], "db");
        assert_eq!(json["import_specifier"], "../db/queries");
    }

    #[test]
    fn serialize_unlisted_dependency_with_import_sites() {
        let d = UnlistedDependency {
            package_name: "chalk".to_string(),
            imported_from: vec![
                ImportSite {
                    path: PathBuf::from("a.ts"),
                    line: 1,
                    col: 0,
                },
                ImportSite {
                    path: PathBuf::from("b.ts"),
                    line: 5,
                    col: 3,
                },
            ],
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["package_name"], "chalk");
        let sites = json["imported_from"].as_array().unwrap();
        assert_eq!(sites.len(), 2);
        assert_eq!(sites[0]["path"], "a.ts");
        assert_eq!(sites[1]["line"], 5);
    }

    #[test]
    fn serialize_duplicate_export_with_locations() {
        let d = DuplicateExport {
            export_name: "Button".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("src/a.ts"),
                    line: 10,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("src/b.ts"),
                    line: 20,
                    col: 5,
                },
            ],
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["export_name"], "Button");
        let locs = json["locations"].as_array().unwrap();
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0]["line"], 10);
        assert_eq!(locs[1]["col"], 5);
    }

    #[test]
    fn serialize_type_only_dependency() {
        let d = TypeOnlyDependency {
            package_name: "@types/react".to_string(),
            path: PathBuf::from("package.json"),
            line: 12,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["package_name"], "@types/react");
        assert_eq!(json["line"], 12);
    }

    #[test]
    fn serialize_test_only_dependency() {
        let d = TestOnlyDependency {
            package_name: "vitest".to_string(),
            path: PathBuf::from("package.json"),
            line: 8,
        };
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["package_name"], "vitest");
        assert_eq!(json["line"], 8);
    }

    #[test]
    fn serialize_unused_member() {
        let m = UnusedMember {
            path: PathBuf::from("enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Pending".to_string(),
            kind: MemberKind::EnumMember,
            line: 3,
            col: 4,
        };
        let json = serde_json::to_value(&m).unwrap();
        assert_eq!(json["parent_name"], "Status");
        assert_eq!(json["member_name"], "Pending");
        assert_eq!(json["line"], 3);
    }

    #[test]
    fn serialize_unresolved_import() {
        let i = UnresolvedImport {
            path: PathBuf::from("app.ts"),
            specifier: "./missing-module".to_string(),
            line: 7,
            col: 0,
            specifier_col: 21,
        };
        let json = serde_json::to_value(&i).unwrap();
        assert_eq!(json["specifier"], "./missing-module");
        assert_eq!(json["specifier_col"], 21);
    }

    // ── deserialize: CircularDependency serde(default) fields ──

    #[test]
    fn deserialize_circular_dependency_with_defaults() {
        // CircularDependency derives Deserialize; line/col/is_cross_package have #[serde(default)]
        let json = r#"{"files":["a.ts","b.ts"],"length":2}"#;
        let cd: CircularDependency = serde_json::from_str(json).unwrap();
        assert_eq!(cd.files.len(), 2);
        assert_eq!(cd.length, 2);
        assert_eq!(cd.line, 0);
        assert_eq!(cd.col, 0);
        assert!(!cd.is_cross_package);
    }

    #[test]
    fn deserialize_circular_dependency_with_all_fields() {
        let json =
            r#"{"files":["a.ts","b.ts"],"length":2,"line":5,"col":10,"is_cross_package":true}"#;
        let cd: CircularDependency = serde_json::from_str(json).unwrap();
        assert_eq!(cd.line, 5);
        assert_eq!(cd.col, 10);
        assert!(cd.is_cross_package);
    }

    // ── clone produces independent copies ───────────────────────

    #[test]
    fn clone_results_are_independent() {
        let mut r = AnalysisResults::default();
        r.unused_files.push(UnusedFile {
            path: PathBuf::from("a.ts"),
        });
        let mut cloned = r.clone();
        cloned.unused_files.push(UnusedFile {
            path: PathBuf::from("b.ts"),
        });
        assert_eq!(r.total_issues(), 1);
        assert_eq!(cloned.total_issues(), 2);
    }

    // ── export_usages not counted in total_issues ───────────────

    #[test]
    fn export_usages_not_counted_in_total_issues() {
        let mut r = AnalysisResults::default();
        r.export_usages.push(ExportUsage {
            path: PathBuf::from("mod.ts"),
            export_name: "foo".to_string(),
            line: 1,
            col: 0,
            reference_count: 3,
            reference_locations: vec![],
        });
        // export_usages is metadata, not an issue type
        assert_eq!(r.total_issues(), 0);
        assert!(!r.has_issues());
    }

    // ── entry_point_summary not counted in total_issues ─────────

    #[test]
    fn entry_point_summary_not_counted_in_total_issues() {
        let r = AnalysisResults {
            entry_point_summary: Some(EntryPointSummary {
                total: 10,
                by_source: vec![("config".to_string(), 10)],
            }),
            ..AnalysisResults::default()
        };
        assert_eq!(r.total_issues(), 0);
        assert!(!r.has_issues());
    }
}
