//! Catalog entries and groups, dependency overrides, test-only
//! dependencies, and dev dependencies in production.

use super::*;

/// A package manager catalog entry that no workspace package references via
/// the `catalog:` protocol.
///
/// The default catalog uses `catalog_name: "default"`. Named catalogs
/// (`catalogs.<name>`) use their declared name. The source file is the
/// `pnpm-workspace.yaml` of the root or of a nested pnpm project, or the
/// root `package.json` for Bun catalogs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedCatalogEntry {
    /// Package name declared in the catalog (e.g. `"react"`, `"@scope/lib"`).
    pub entry_name: String,
    /// Catalog group: `"default"` for the default catalog map, or the named
    /// catalog key for entries declared under `catalogs.<name>`.
    pub catalog_name: String,
    /// Path to the catalog source file, relative to the analyzed root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the catalog entry within the source file.
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

/// A named `catalogs.<name>` group with no package entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EmptyCatalogGroup {
    /// Catalog group name declared under the `catalogs` map.
    pub catalog_name: String,
    /// Path to the catalog source file, relative to the analyzed root.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the empty group header within the source file.
    pub line: u32,
}

/// A workspace package.json reference (`catalog:` or `catalog:<name>`) that points
/// at a catalog which does not declare the consumed package.
///
/// Package manager installs error when this happens. fallow surfaces it
/// statically so the failure is caught at `fallow dead-code` time, before any
/// install.
///
/// The default catalog (bare `catalog:`) uses `catalog_name: "default"`.
/// Named catalogs (`catalog:react17`) use the declared catalog name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Other catalogs in the same catalog source that DO declare this package.
    /// Empty when no catalog has the package. Sorted lexicographically. Lets
    /// agents and humans decide whether to switch the reference to a different
    /// catalog or to add the entry to the named catalog.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_in_catalogs: Vec<String>,
}

/// Where an override entry was declared. Serialized as the filename label
/// (`"pnpm-workspace.yaml"` or `"package.json"`) so the value in JSON output
/// matches the value users write in `ignoreDependencyOverrides[].source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DependencyOverrideSource {
    /// Top-level `overrides:` key in `pnpm-workspace.yaml`.
    #[serde(rename = "pnpm-workspace.yaml")]
    PnpmWorkspaceYaml,
    /// `pnpm.overrides`, the top-level npm `overrides` object, or (in a bun
    /// repository) the Yarn-style top-level `resolutions` object in a root
    /// `package.json`.
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
/// `package.json`), in npm's top-level `overrides` object in `package.json`,
/// or (in a bun repository) in the Yarn-style top-level `resolutions` object
/// in `package.json`, whose target package is not declared in any workspace
/// `package.json` and is not present in `pnpm-lock.yaml`,
/// `package-lock.json`, or `bun.lock`. Projects without a readable lockfile
/// fall back to package manifest checks; the `hint` field flags that
/// conservative mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// The right-hand side of the entry: the version the package manager should force.
    pub version_range: String,
    /// File the override was declared in. Matches the value users write in
    /// `ignoreDependencyOverrides[].source`.
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
    /// Soft hint reminding consumers to verify the override before removal.
    /// Emitted on every unused-override finding (both bare-target and
    /// parent-chain shapes) because projects without a readable lockfile still
    /// use the conservative package-manifest fallback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Why a dependency-override entry is misconfigured. The active package
/// manager may fail at install time or silently no-op on these entries;
/// surfacing them statically catches the issue first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DependencyOverrideMisconfigReason {
    /// The override key could not be parsed into a recognised source shape
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
/// `error` because the active package manager may refuse to install or silently
/// produce a no-op override when it encounters these shapes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
    /// Classifier for the misconfiguration. 'unparsable-key' = the key is not a
    /// valid source shape; 'empty-value' = the value is missing, empty, or
    /// contains line breaks.
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestOnlyDependency {
    /// Production dependency that is only imported by test files, consider
    /// moving to devDependencies.
    pub package_name: String,
    /// Path to the package.json where the dependency is listed.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
}

/// A `devDependencies` package imported by production (non-test, non-config)
/// source code via a runtime/value import. Because a production-only install
/// (`pnpm install --prod`) omits devDependencies, it would break at runtime, so
/// the package should be promoted to `dependencies`. The promote-side mirror of
/// [`TestOnlyDependency`] / [`TypeOnlyDependency`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DevDependencyInProduction {
    /// devDependency imported at runtime from production code, consider moving
    /// to dependencies.
    pub package_name: String,
    /// Path to the package.json where the dependency is listed.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
}
