//! Unused, unlisted and type-only dependencies, unused members, unresolved
//! imports, and duplicate exports.

use super::*;

/// A dependency that is listed in package.json but never imported.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
        default,
        serialize_with = "serde_path::serialize_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub used_in_workspaces: Vec<PathBuf>,
    /// Workspace roots whose package.json declares this package for the files
    /// that import it. Only a root finding fills this field: these imports use
    /// the nearer workspace declaration, so the root declaration stays unused.
    #[serde(
        default,
        serialize_with = "serde_path::serialize_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    #[cfg_attr(feature = "schema", schemars(default))]
    pub declared_and_imported_in: Vec<PathBuf>,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum DependencyLocation {
    /// Listed in `dependencies`.
    Dependencies,
    /// Listed in `devDependencies`.
    DevDependencies,
    /// Listed in `optionalDependencies`.
    OptionalDependencies,
}

/// An unused enum, class, store, or namespace member.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedMember {
    /// File containing the unused member.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the parent enum, class, store, or exported namespace.
    pub parent_name: String,
    /// Name of the unused member. A member of a nested namespace has the dotted
    /// path from the exported namespace (`Inner.x`).
    pub member_name: String,
    /// The kind of member: enum member, class method, class property, store
    /// member, or namespace member.
    pub kind: MemberKind,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// An import that could not be resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnlistedDependency {
    /// Package name, including internal workspace package names, that is
    /// imported but not listed in package.json.
    pub package_name: String,
    /// Import sites where this unlisted dependency is used (file path, line, column).
    pub imported_from: Vec<ImportSite>,
}

/// A binary that a package.json script or a process call runs, but that no
/// installed or listed package provides.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnlistedBinary {
    /// The binary name as the command runs it (for example `ffmpeg`).
    pub binary_name: String,
    /// The sites that run this binary.
    pub used_in: Vec<BinaryUseSite>,
}

/// A location that runs a binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct BinaryUseSite {
    /// The `package.json` that declares the script, or the source file that
    /// makes the process call.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// The name of the package.json script that runs the binary. Absent for
    /// a process call in source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_name: Option<String>,
}

/// A location where an import occurs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicateExport {
    /// The duplicated export name.
    pub export_name: String,
    /// Locations where this export name appears.
    pub locations: Vec<DuplicateLocation>,
}

/// A location where a duplicate export appears.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicateLocation {
    /// File containing the duplicate export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// One module that exports one binding under two or more names, for example
/// `export const a = 1; export default a;` or `export { a, a as b }`. The
/// finding is about the export surface of one file, not about usage, so an
/// entry file can have it too. Value and type-only exports of one name are
/// separate bindings. Two bindings with the same initializer are not one
/// binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicateExportAlias {
    /// The module that exports the binding.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The local name of the binding. For a re-export, the name that the
    /// source module exports.
    pub local_name: String,
    /// The export names of the binding, in source order.
    pub export_names: Vec<String>,
    /// 1-based line number of the first export of the binding.
    pub line: u32,
    /// 0-based byte column offset of the first export of the binding.
    pub col: u32,
    /// One location for each export name, in source order.
    pub locations: Vec<DuplicateExportAliasLocation>,
}

/// One export name of a [`DuplicateExportAlias`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicateExportAliasLocation {
    /// The export name at this location (`default` for a default export).
    pub export_name: String,
    /// 1-based line number.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// A production dependency that is only used via type-only imports.
/// In production builds, type imports are erased, so this dependency
/// is not needed at runtime and could be moved to devDependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeOnlyDependency {
    /// Production dependency that is only used via type-only imports.
    pub package_name: String,
    /// Path to the package.json where the dependency is listed.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the dependency entry in package.json.
    pub line: u32,
}
