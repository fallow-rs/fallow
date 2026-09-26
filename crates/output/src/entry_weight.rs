//! Startup import weight carried by `fallow list --entry-weight`.
//!
//! The unit is source bytes on disk. The value includes types and comments and
//! ignores tree shaking and bundler chunks, so it is not a bundle size. It is a
//! repeatable count that goes down when an import moves behind `import()`.

use serde::{Deserialize, Serialize};

/// Unit of every byte count in [`EntryWeightListing`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EntryWeightUnit {
    /// On-disk bytes of project source files, before any build step.
    SourceBytes,
}

/// `entry_weight` block of `fallow list --entry-weight --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntryWeightListing {
    /// Unit of every byte count in this block.
    pub unit: EntryWeightUnit,
    /// Number of entries in `entries`.
    pub entry_count: usize,
    /// One row per runtime entry point, heaviest `eager_bytes` first. A
    /// declaration file (`.d.ts`) is not a row, because nothing loads it.
    pub entries: Vec<EntryWeightOutput>,
    /// Comparison with a saved regression baseline; present when a baseline
    /// with entry weights was loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regression: Option<EntryWeightRegression>,
}

/// Startup import weight of one runtime entry point.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntryWeightOutput {
    /// Entry file, relative to the analysed root.
    pub path: String,
    /// What declared the entry point, e.g. a plugin or `package.json main`.
    pub source: String,
    /// Project modules that load before the entry runs, the entry included.
    /// Only static imports with a runtime value count; `import type` does not.
    pub eager_modules: usize,
    /// Source bytes of `eager_modules`.
    pub eager_bytes: u64,
    /// The part of `eager_bytes` that stylesheets (CSS, Sass, Less)
    /// contribute.
    pub eager_css_bytes: u64,
    /// Project modules that load only on demand through `import()` or a lazy
    /// glob or template pattern.
    pub deferred_modules: usize,
    /// Source bytes of `deferred_modules`.
    pub deferred_bytes: u64,
    /// Project modules that only a `new URL(..., import.meta.url)` reference
    /// (for example a worker URL), `child_process.fork`, a pino transport or
    /// a `module.register` hook reaches. They do not load on the thread of
    /// the entry.
    pub out_of_thread_modules: usize,
    /// Source bytes of `out_of_thread_modules`.
    pub out_of_thread_bytes: u64,
    /// Number of packages in `eager_packages`.
    pub eager_package_count: usize,
    /// Packages that eager modules import statically, sorted by name.
    /// Platform built-ins such as `node:fs` are not listed. Package bytes are
    /// not measured.
    pub eager_packages: Vec<EagerPackageOutput>,
    /// Single imports that each keep a part of the eager modules eager,
    /// heaviest first. This is evidence for a review, not a fix: a lazy load of
    /// code that the first screen needs can make startup slower.
    pub dominating_imports: Vec<DominatingImportOutput>,
}

/// One package on the eager path of an entry.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EagerPackageOutput {
    /// Package name, e.g. `lodash` or `@scope/pkg`.
    pub name: String,
    /// Specifiers as written, e.g. `lodash/debounce`, sorted.
    pub specifiers: Vec<String>,
}

/// One import that alone keeps a subtree of the eager modules eager.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DominatingImportOutput {
    /// File that contains the import, relative to the analysed root.
    pub importer: String,
    /// 1-based line of the import binding; absent when the edge has no
    /// binding span, such as an `export ... from` re-export or an eager glob
    /// match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// Imported file, relative to the analysed root.
    pub target: String,
    /// Source bytes that leave the eager modules if this import becomes an
    /// `import()`.
    pub exclusive_bytes: u64,
    /// Modules that leave the eager modules if this import becomes an
    /// `import()`.
    pub exclusive_modules: usize,
}

/// Comparison of the current entry weights with a regression baseline.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntryWeightRegression {
    /// Allowed growth of `eager_bytes` per entry, as spelled: `"5%"` or a
    /// byte count such as `"1024"`.
    pub tolerance: String,
    /// Whether `--fail-on-regression` makes an exceeded entry fail the run.
    /// Without it the comparison is report-only.
    pub enforced: bool,
    /// Whether at least one entry grew more than the tolerance allows.
    pub exceeded: bool,
    /// One row per entry in the baseline or in the current run, sorted by path.
    pub entries: Vec<EntryWeightDelta>,
}

/// Change of one entry against the regression baseline.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntryWeightDelta {
    /// Entry file, relative to the analysed root.
    pub path: String,
    /// Baseline `eager_bytes`; absent for an entry that is new in this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_eager_bytes: Option<u64>,
    /// Current `eager_bytes`; absent for an entry that is gone in this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_eager_bytes: Option<u64>,
    /// Baseline `eager_modules`; absent for an entry that is new in this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_eager_modules: Option<usize>,
    /// Current `eager_modules`; absent for an entry that is gone in this run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_eager_modules: Option<usize>,
    /// Packages on the eager path now that the baseline did not have.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub new_eager_packages: Vec<String>,
    /// Whether the growth of `eager_bytes` is larger than the tolerance.
    pub exceeded: bool,
}
