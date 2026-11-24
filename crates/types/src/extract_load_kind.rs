//! When the target of an import edge loads, relative to the importing module.

/// When the target of an import edge loads, relative to the importing module.
///
/// The startup weight report follows only `Static` edges to find the code that
/// loads before the entry module runs. The other kinds load later, or outside
/// the importing thread, except `PathReference` and `AssetReference`. These
/// two edges keep their target in use but do not run it: see
/// [`Self::loads_target`]. Only an asset reference also stops reachability:
/// see [`Self::runs_target_code`]. A `BuildTime` edge runs its target only in
/// the bundler: see [`Self::runs_at_runtime`].
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum ImportLoadKind {
    /// The target loads before the importer runs: `import`, `export ... from`,
    /// `require()`, `require.context` and `import.meta.glob(..., { eager: true })`.
    #[default]
    Static = 0,
    /// The target loads on demand through `import('./literal')`.
    Dynamic = 1,
    /// The target is one match of an on-demand pattern: a template `import()`
    /// or a lazy `import.meta.glob`.
    DynamicPattern = 2,
    /// The target runs on another thread or in another process: a
    /// `new URL(..., import.meta.url)` reference (for example a worker URL),
    /// a webpack worker loader request (`worker-loader!./work.js`),
    /// `child_process.fork`, a pino transport or a `module.register` hook.
    OutOfThread = 3,
    /// The importer gets only the path of the target and does not load it:
    /// `require.resolve('./file')` or ``require.resolve(`./file`)``. The edge
    /// keeps the target in use. The consumer of the path can run the target
    /// later, so the imports of the target stay reachable.
    PathReference = 4,
    /// The importer reads the target as text, bytes or a URL through a
    /// webpack asset loader, such as `raw-loader!./file.js`. The bundle never
    /// runs the target as code, so the imports of the target are not
    /// reachable through this edge. The graph sets this kind; extraction
    /// never records it.
    AssetReference = 5,
    /// The bundler runs the target at build time and inlines the result:
    /// `import { x } from './x.ts' with { type: 'macro' }`. The target and its
    /// imports stay in use, but they do not run in the shipped program.
    BuildTime = 6,
}

impl ImportLoadKind {
    /// Whether the target loads before the importing module runs.
    #[must_use]
    pub const fn is_eager(self) -> bool {
        matches!(self, Self::Static)
    }

    /// Whether the target loads on demand on the importing thread.
    #[must_use]
    pub const fn is_deferred(self) -> bool {
        matches!(self, Self::Dynamic | Self::DynamicPattern)
    }

    /// Whether the edge runs its target. A path reference and an asset
    /// reference keep the target in use but do not run it, so the edge does
    /// not close a cycle, is not in the entry load closure, does not cross an
    /// architecture boundary and does not carry server code into a client
    /// bundle.
    #[must_use]
    pub const fn loads_target(self) -> bool {
        !matches!(self, Self::PathReference | Self::AssetReference)
    }

    /// Whether the target can run as code at all, through this edge or
    /// through the consumer of a path reference. Only an asset reference never
    /// runs its target, so reachability does not continue through it.
    #[must_use]
    pub const fn runs_target_code(self) -> bool {
        !matches!(self, Self::AssetReference)
    }

    /// Whether the target can run as code in the shipped program. A build-time
    /// import runs its target only in the bundler, so production reachability
    /// does not continue through it.
    #[must_use]
    pub const fn runs_at_runtime(self) -> bool {
        self.runs_target_code() && !matches!(self, Self::BuildTime)
    }

    /// Whether an import with this load kind puts a runtime value of the
    /// target on the startup path: the target loads before the importer runs
    /// and the import is not type-only.
    #[must_use]
    pub const fn is_eager_value(self, is_type_only: bool) -> bool {
        self.is_eager() && !is_type_only
    }
}
