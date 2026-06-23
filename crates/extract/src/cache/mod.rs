//! Incremental parse cache with bitcode serialization.
//!
//! Stores parsed module information (exports, imports, re-exports) on disk so
//! unchanged files can skip AST parsing on subsequent runs. Uses xxh3 content
//! hashing to detect changes.

mod conversion;
mod store;
mod types;

#[cfg(all(test, not(miri)))]
mod tests;

pub use conversion::{
    cached_to_module, cached_to_module_opts, current_unix_seconds, module_to_cached,
};
pub use store::CacheStore;
pub use types::{
    CachedDynamicImport, CachedDynamicImportPattern, CachedExport, CachedImport, CachedMember,
    CachedModule, CachedReExport, CachedRequireCall, CachedSuppression, DEFAULT_CACHE_MAX_SIZE,
    DUPES_CACHE_VERSION,
};
