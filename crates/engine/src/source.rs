//! Source parsing contracts owned by the engine boundary.

use fallow_types::discover::DiscoveredFile;
#[cfg(test)]
pub use fallow_types::extract::{ExportName, MemberKind, VisibilityTag};
pub use fallow_types::extract::{ModuleInfo, ParseResult};

type CacheStore = fallow_extract::cache::CacheStore;

/// Source inventory walking for coverage and upload surfaces.
pub mod inventory {
    use std::path::Path;

    use rustc_hash::FxHashMap;

    /// A single static-inventory entry for one function.
    ///
    /// This is the engine-owned inventory contract exposed to CLI upload
    /// surfaces. The extractor owns AST traversal; the engine owns the public
    /// shape that downstream crates construct and upload.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct InventoryEntry {
        /// Beacon-compatible function name.
        pub name: String,
        /// 1-based source line of the function declaration.
        pub line: u32,
        /// 1-indexed UTF-16 column of the function node start.
        pub start_column: u32,
        /// 1-based source line where the function node ends.
        pub end_line: u32,
        /// 1-indexed UTF-16 column of the function node end.
        pub end_column: u32,
        /// Content digest of the function's full-span source slice.
        pub source_hash: String,
    }

    impl From<fallow_extract::inventory::InventoryEntry> for InventoryEntry {
        fn from(entry: fallow_extract::inventory::InventoryEntry) -> Self {
            Self {
                name: entry.name,
                line: entry.line,
                start_column: entry.start_column,
                end_line: entry.end_line,
                end_column: entry.end_column,
                source_hash: entry.source_hash,
            }
        }
    }

    /// Per-function static complexity collected alongside the inventory walk.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct InventoryComplexity {
        /// `McCabe` cyclomatic complexity (1 + decision points).
        pub cyclomatic: u16,
        /// `SonarSource` cognitive complexity (structural + nesting penalty).
        pub cognitive: u16,
    }

    impl From<fallow_extract::inventory::InventoryComplexity> for InventoryComplexity {
        fn from(complexity: fallow_extract::inventory::InventoryComplexity) -> Self {
            Self {
                cyclomatic: complexity.cyclomatic,
                cognitive: complexity.cognitive,
            }
        }
    }

    /// Walk source and emit engine-owned function inventory entries.
    #[must_use]
    pub fn walk_source(path: &Path, source: &str) -> Vec<InventoryEntry> {
        fallow_extract::inventory::walk_source(path, source)
            .into_iter()
            .map(InventoryEntry::from)
            .collect()
    }

    /// Walk source once and emit inventory entries plus static complexity by source hash.
    #[must_use]
    pub fn walk_source_with_complexity(
        path: &Path,
        source: &str,
    ) -> (Vec<InventoryEntry>, FxHashMap<String, InventoryComplexity>) {
        let (entries, complexity) =
            fallow_extract::inventory::walk_source_with_complexity(path, source);
        let entries = entries.into_iter().map(InventoryEntry::from).collect();
        let complexity = complexity
            .into_iter()
            .map(|(hash, metrics)| (hash, InventoryComplexity::from(metrics)))
            .collect();
        (entries, complexity)
    }
}

/// Parse discovered source files into typed module facts.
///
/// Keeping parsing behind the engine boundary lets sessions and future
/// incremental runners choose cache policy without exposing the extract crate
/// as the public orchestration layer.
#[must_use]
pub fn parse_all_files(
    files: &[DiscoveredFile],
    cache: Option<&CacheStore>,
    need_complexity: bool,
) -> ParseResult {
    fallow_extract::parse_all_files(files, cache, need_complexity)
}
