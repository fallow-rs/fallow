use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::source_fingerprint::SourceFingerprint;

use crate::cache::CacheStore;
use crate::parse_all_files;

fn source_file(root: &std::path::Path, name: &str, source: &str) -> DiscoveredFile {
    let path = root.join(name);
    std::fs::write(&path, source).expect("write source");
    DiscoveredFile {
        id: FileId(0),
        path,
        size_bytes: u64::try_from(source.len()).expect("source length fits u64"),
    }
}

/// Store the module of a first parse with the fingerprint that
/// `fingerprint_of` builds from the file metadata, then parse again
/// with that store.
fn second_parse_metadata_hits(
    fingerprint_of: fn(&std::fs::Metadata) -> SourceFingerprint,
) -> Vec<FileId> {
    let project = tempfile::tempdir().expect("create project");
    let file = source_file(project.path(), "api.ts", "export const alpha = 1;\n");
    let files = [file];
    let first = parse_all_files(&files, None, false);
    let module = first.modules.first().expect("first parse produces module");
    let metadata = std::fs::metadata(&files[0].path).expect("read metadata");
    let mut store = CacheStore::new(project.path());
    store.record_module(
        &files[0].path,
        module,
        fingerprint_of(&metadata),
        false,
        false,
    );

    let second = parse_all_files(&files, Some(&store), false);
    assert_eq!(second.cache_hits, 1);
    second.metadata_hits
}

#[test]
fn a_hit_on_a_trusted_fingerprint_is_a_metadata_hit() {
    let hits = second_parse_metadata_hits(SourceFingerprint::from_metadata);
    assert_eq!(hits, vec![FileId(0)]);
}

#[test]
fn a_hit_that_compared_the_content_hash_is_not_a_metadata_hit() {
    let hits = second_parse_metadata_hits(|metadata| {
        let full = SourceFingerprint::from_metadata(metadata);
        SourceFingerprint::new(full.mtime_ns, full.file_size)
    });
    assert!(hits.is_empty(), "content-hash hit listed: {hits:?}");
}

#[test]
fn a_parse_without_a_cache_has_no_metadata_hits() {
    let project = tempfile::tempdir().expect("create project");
    let files = [source_file(
        project.path(),
        "api.ts",
        "export const alpha = 1;\n",
    )];
    assert!(
        parse_all_files(&files, None, false)
            .metadata_hits
            .is_empty()
    );
}
