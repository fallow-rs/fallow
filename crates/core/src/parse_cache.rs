//! Write the modules of a parse into the persisted parse cache.

use fallow_types::discover::FileId;
use fallow_types::source_fingerprint::SourceFingerprint;

use crate::{cache, discover, extract};

/// The modules of one parse to store in the cache.
pub struct CacheUpdate<'a> {
    pub modules: &'a [extract::ModuleInfo],
    pub files: &'a [discover::DiscoveredFile],
    /// Files that the parse served from cache metadata, sorted by id.
    pub metadata_hits: &'a [FileId],
    /// The optional facts that the modules hold.
    pub needs: extract::ParseNeeds,
    /// The time before the parse read the sources.
    pub read_started_ns: u64,
}

/// The optional facts that a parse of `config` must give. A fact without a
/// reader in the run is not collected.
pub fn parse_needs(
    config: &fallow_config::ResolvedConfig,
    need_complexity: bool,
) -> extract::ParseNeeds {
    extract::ParseNeeds {
        complexity: need_complexity,
        call_facts: config.reads_call_facts(),
        flags: config.reads_feature_flags(),
        component_contracts: config.reads_component_contracts(),
    }
}

/// Update cache: write freshly parsed modules, refresh stale metadata entries,
/// and upgrade entries that a run without an optional fact wrote earlier.
/// [`cache::CacheStore::record_module`] keeps each optional fact that an
/// earlier run stored and this run did not extract.
///
/// A file in `metadata_hits` keeps its entry without a metadata read (see
/// [`fallow_extract::is_metadata_hit`]).
///
/// A fingerprint that is not older than `read_started_ns` by the settle window
/// is stored without its ctime, so the next run compares content for that
/// file. The metadata is read here, after the parse. A save between the parse
/// read and this read gives a ctime newer than `read_started_ns`, so that
/// fingerprint never settles and is stored without its ctime.
pub fn update_cache(store: &mut cache::CacheStore, update: &CacheUpdate<'_>) -> bool {
    let mut dirty = false;
    for module in update.modules {
        if fallow_extract::is_metadata_hit(update.metadata_hits, module.file_id) {
            continue;
        }
        if let Some(file) = update.files.get(module.file_id.0 as usize) {
            let fingerprint =
                file_fingerprint(&file.path).for_content_read_at(update.read_started_ns);
            dirty |= store.record_module(&file.path, module, fingerprint, update.needs);
        }
    }
    let removed_stale_paths = store.retain_paths(update.files);
    dirty || removed_stale_paths
}

/// Extract source fingerprint metadata from a path.
fn file_fingerprint(path: &std::path::Path) -> SourceFingerprint {
    std::fs::metadata(path).map_or(SourceFingerprint::new(0, 0), |metadata| {
        SourceFingerprint::from_metadata(&metadata)
    })
}

/// The content hash of each parsed file, by path.
pub fn collect_file_hashes(
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
) -> rustc_hash::FxHashMap<std::path::PathBuf, u64> {
    modules
        .iter()
        .filter_map(|module| {
            files
                .get(module.file_id.0 as usize)
                .map(|file| (file.path.clone(), module.content_hash))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::source_fingerprint::SourceFingerprint;

    use super::{CacheUpdate, update_cache};
    use crate::cache::CacheStore;

    const SOURCE: &str = "export const alpha = 1;\n";
    const STORED: SourceFingerprint = SourceFingerprint::with_ctime(10, 20, 24);

    /// Store the module of a path that does not exist, then write that module
    /// again. A metadata read of the path gives an empty fingerprint, so a
    /// changed entry shows that the writer read the metadata. Returns whether
    /// the write changed the store, and the stored fingerprint.
    fn rewrite_missing_file(metadata_hits: &[FileId]) -> (bool, SourceFingerprint) {
        let root = std::env::temp_dir().join("fallow-parse-cache-no-such-project");
        let path = root.join("api.ts");
        let module = fallow_extract::parse_source_to_module(FileId(0), &path, SOURCE, 0, false);
        let mut store = CacheStore::new(&root);
        store.record_module(
            &path,
            &module,
            STORED,
            fallow_extract::ParseNeeds::default(),
        );
        let files = [DiscoveredFile {
            id: FileId(0),
            path: path.clone(),
            size_bytes: STORED.file_size,
        }];
        let changed = update_cache(
            &mut store,
            &CacheUpdate {
                modules: std::slice::from_ref(&module),
                files: &files,
                metadata_hits,
                needs: fallow_extract::ParseNeeds::default(),
                read_started_ns: fallow_types::source_fingerprint::now_ns(),
            },
        );
        let stored = store
            .get_by_path_only(&path)
            .expect("entry stays in the store")
            .source_fingerprint();
        (changed, stored)
    }

    #[test]
    #[cfg_attr(miri, ignore = "the metadata read is blocked by Miri isolation")]
    fn update_cache_skips_the_stat_of_a_metadata_hit() {
        assert_eq!(rewrite_missing_file(&[FileId(0)]), (false, STORED));
    }

    #[test]
    #[cfg_attr(miri, ignore = "the metadata read is blocked by Miri isolation")]
    fn update_cache_reads_the_metadata_of_any_other_module() {
        let (changed, stored) = rewrite_missing_file(&[]);
        assert!(changed);
        assert_eq!(stored, SourceFingerprint::new(0, 0));
    }
}
