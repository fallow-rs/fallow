#![no_main]
//! Properties of extraction and of the extraction cache.
//!
//! 1. Extraction is deterministic: two parses of the same source give the
//!    same cache bytes. A difference makes the cache key unstable and the
//!    output of two runs differ.
//! 2. The cache is a fixed point of a save and load cycle: a module loaded
//!    from the cache and saved again gives the same bytes. A difference means
//!    that a warm run sees other facts than the cold run that wrote the cache.

use std::path::Path;

use fallow_extract::cache::{CachedModule, cached_to_module, module_to_cached};
use fallow_types::discover::FileId;
use fallow_types::source_fingerprint::SourceFingerprint;
use libfuzzer_sys::fuzz_target;

#[path = "common/extract_paths.rs"]
mod extract_paths;

use extract_paths::PATHS;

#[path = "common/worker_stack.rs"]
mod worker_stack;

const FINGERPRINT: SourceFingerprint = SourceFingerprint {
    mtime_ns: 1,
    ctime_ns: 1,
    file_size: 1,
};

/// Encodes the cache entry of `module`. The entry stamps the wall-clock time
/// for eviction, so the stamp is cleared: two calls can fall in two seconds.
fn cache_bytes(module: &fallow_extract::ModuleInfo) -> Vec<u8> {
    let mut cached = module_to_cached(module, FINGERPRINT, true);
    cached.last_seen_secs = 0;
    bitcode::encode(&cached)
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    let Ok(source) = std::str::from_utf8(rest) else {
        return;
    };
    let path = Path::new(PATHS[usize::from(selector) % PATHS.len()]);

    worker_stack::on_worker_stack(|| {
        let first = cache_bytes(&fallow_extract::parse_from_content(FileId(0), path, source));
        let second = cache_bytes(&fallow_extract::parse_from_content(FileId(0), path, source));
        assert!(
            first == second,
            "two parses of the same {} source differ",
            path.display()
        );

        let cached: CachedModule = bitcode::decode(&first).expect("cache bytes decode");
        let reloaded = cache_bytes(&cached_to_module(&cached, FileId(0)));
        assert!(
            first == reloaded,
            "a cache save and load cycle changes the {} module",
            path.display()
        );
    });
});
