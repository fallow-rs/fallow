//! Import and package cycle findings.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::serde_path;

/// One import hop in a circular dependency: the file containing the import
/// and where that import statement sits.
///
/// `edges[i]` is the import in `path` (the hop source, equal to the cycle's
/// `files[i]`) that points to the next file in the cycle
/// (`files[(i + 1) % files.len()]`); the target is not repeated here to keep
/// the wire compact. Enables a per-file diagnostic squiggly anchored under
/// the offending import rather than a single squiggly on the first file.
///
/// `col` is a 0-based byte column, matching the cycle's top-level `col`;
/// converting it to a UTF-16 code-unit column for LSP clients is a tracked
/// follow-up shared with the existing field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CircularDependencyEdge {
    /// The file containing the import (the hop source; equal to `files[i]`).
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the import statement pointing to the next file.
    pub line: u32,
    /// 0-based byte column offset of the import statement.
    pub col: u32,
}

/// A circular dependency chain detected in the module graph.
///
/// The `line` and `col` fields carry `#[serde(default)]` so callers reading
/// historical baseline JSON without these fields can still deserialize the
/// struct, but the JSON output layer always emits them (u32 always
/// serializes, never via `skip_serializing_if`). The schemars derive sees
/// the serde defaults and marks both fields optional in the generated
/// schema; the explicit `extend("required" = ...)` override here keeps the
/// schema's `required` array honest about what the JSON output actually
/// contains.
///
/// `edges` is deliberately kept OUT of the `required` extend: it is
/// `#[serde(default)]` (so historical baseline JSON without it still
/// deserializes) and the output layer always emits it, but listing it in
/// `required` would make pre-upgrade JSON fail validation against the new
/// schema. It is a normal additive field: always present in current output,
/// optional for backward compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(extend("required" = ["files", "length", "line", "col"])))]
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
    /// Per-file import anchors, one entry per hop in cycle order: `edges[i]`
    /// is the import in `files[i]` pointing to `files[(i + 1) % len]`. Always
    /// the same length as `files`. Drives the per-file LSP diagnostic
    /// squiggly. `#[serde(default)]` so pre-`edges` baselines deserialize;
    /// always emitted on output but intentionally not in the schema's
    /// `required` set (see the struct doc).
    #[serde(default)]
    pub edges: Vec<CircularDependencyEdge>,
    /// Whether this cycle crosses workspace package boundaries.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_cross_package: bool,
}

/// A cycle or self-loop in the re-export edge subgraph, found by a Tarjan SCC
/// pass in `crates/graph/src/graph/re_exports/`. Chain propagation through the
/// loop is a no-op, so a symbol re-exported through it does not resolve.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReExportCycle {
    /// Member files, sorted lexicographically. A self-loop has one entry.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// Which structural shape this finding describes.
    pub kind: ReExportCycleKind,
    /// 1-based line of the re-export in `files[0]` that closes the cycle, or `0` if unknown.
    #[serde(default)]
    pub line: u32,
    /// 0-based byte column of that re-export.
    #[serde(default)]
    pub col: u32,
}

/// Discriminator for [`ReExportCycle`]: which structural shape was detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ReExportCycleKind {
    /// Two or more barrel files re-export from each other in a loop
    /// (SCC of size >= 2).
    MultiNode,
    /// A single barrel file re-exports from itself.
    SelfLoop,
}

/// One package hop in a [`PackageCycle`]: `from_package` imports
/// `to_package`, and `path` holds one example import for that hop.
///
/// The example import is the first runtime import by `(path, line)`. When
/// every import on the hop is type-only, it is the first type-only import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PackageCycleEdge {
    /// Label of the importing workspace package, as in
    /// [`PackageCycle::packages`].
    pub from_package: String,
    /// Label of the imported workspace package, as in
    /// [`PackageCycle::packages`].
    pub to_package: String,
    /// File in `from_package` that holds the example import.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// File in `to_package` that the example import resolves to.
    #[serde(serialize_with = "serde_path::serialize")]
    pub target_path: PathBuf,
    /// 1-based line number of the example import.
    pub line: u32,
    /// 0-based byte column offset of the example import.
    pub col: u32,
    /// True when every import from `from_package` to `to_package` is
    /// type-only. A type-only hop has no runtime effect, but it can still
    /// force a build order (for example with declaration builds).
    pub type_only: bool,
}

/// A dependency cycle between workspace packages.
///
/// Each workspace package is a node. A resolved import from a file in one
/// package to a file in another package is an edge. Declared `package.json`
/// dependencies are not edges, and imports from test, spec, story, fixture
/// and tooling config files are not edges. A package cycle can exist when no
/// file-level cycle exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PackageCycle {
    /// Workspace package labels in cycle order. The first entry is the
    /// lexicographically smallest label; the last entry imports the first.
    /// A label is the package name. When two or more workspace packages
    /// share a name, the label is `name (root)` with the project-relative
    /// package root, so that each label names one package.
    pub packages: Vec<String>,
    /// Package root directories in cycle order: `package_roots[i]` is the
    /// root of `packages[i]`.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub package_roots: Vec<PathBuf>,
    /// Number of packages in the cycle.
    pub length: usize,
    /// One example import per hop, in cycle order: `edges[i]` goes from
    /// `packages[i]` to `packages[(i + 1) % length]`.
    pub edges: Vec<PackageCycleEdge>,
    /// True when the group of packages that holds this cycle has more
    /// cycles than fallow lists. The listing stops at 20 cycles per group,
    /// or earlier on a very dense package graph. Break a listed cycle and
    /// run again to see the rest.
    pub group_truncated: bool,
}

impl PackageCycle {
    /// The note that every output format shows for a cycle with
    /// [`PackageCycle::group_truncated`] set.
    pub const GROUP_TRUNCATED_NOTE: &'static str = "this package group has more cycles than listed";

    /// The package labels in cycle order, with the first label repeated at
    /// the end, joined with `separator`.
    #[must_use]
    pub fn chain(&self, separator: &str) -> String {
        let mut chain: Vec<&str> = self.packages.iter().map(String::as_str).collect();
        if let Some(first) = chain.first().copied() {
            chain.push(first);
        }
        chain.join(separator)
    }
}
