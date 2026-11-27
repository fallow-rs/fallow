use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{FlagPatterns, ParseResult};
use fallow_types::shared_state::ExtractionMode;
use fallow_types::source_fingerprint::SourceFingerprint;

use crate::cache::CacheStore;
use crate::{ParseNeeds, parse_all_files, parse_all_files_with_mode};

const NO_FACTS: ParseNeeds = ParseNeeds {
    complexity: false,
    call_facts: false,
    flags: false,
    component_contracts: false,
};

fn source_file(root: &std::path::Path, name: &str, source: &str) -> DiscoveredFile {
    let path = root.join(name);
    std::fs::write(&path, source).expect("write source");
    DiscoveredFile {
        id: FileId(0),
        path,
        size_bytes: u64::try_from(source.len()).expect("source length fits u64"),
    }
}

/// Store the module of a first parse without complexity and call facts, with
/// the fingerprint that `fingerprint_of` builds from the file metadata. Then
/// parse again with that store and `needs`.
fn second_parse(
    fingerprint_of: fn(&std::fs::Metadata) -> SourceFingerprint,
    needs: ParseNeeds,
) -> ParseResult {
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
        ParseNeeds {
            flags: true,
            component_contracts: true,
            ..NO_FACTS
        },
    );

    parse_all_files_with_mode(
        &files,
        Some(&store),
        needs,
        None,
        &FlagPatterns::default(),
        ExtractionMode::Standard,
    )
}

// Only Unix reports a ctime. Without it no fingerprint is trusted without
// the content, so a hit on Windows always compares the content hash.
#[cfg(unix)]
#[test]
fn a_hit_on_a_trusted_fingerprint_is_a_metadata_hit() {
    let second = second_parse(SourceFingerprint::from_metadata, NO_FACTS);
    assert_eq!(second.cache_hits, 1);
    assert_eq!(second.metadata_hits, vec![FileId(0)]);
}

#[cfg(not(unix))]
#[test]
fn a_hit_without_a_ctime_is_not_a_metadata_hit() {
    let second = second_parse(SourceFingerprint::from_metadata, NO_FACTS);
    assert_eq!(second.cache_hits, 1);
    assert!(second.metadata_hits.is_empty());
}

#[test]
fn a_hit_that_compared_the_content_hash_is_not_a_metadata_hit() {
    let second = second_parse(
        |metadata| {
            let full = SourceFingerprint::from_metadata(metadata);
            SourceFingerprint::new(full.mtime_ns, full.file_size)
        },
        NO_FACTS,
    );
    assert_eq!(second.cache_hits, 1);
    assert!(second.metadata_hits.is_empty());
}

#[test]
fn an_entry_without_the_complexity_of_the_run_is_not_a_metadata_hit() {
    let needs = ParseNeeds {
        complexity: true,
        ..NO_FACTS
    };
    let second = second_parse(SourceFingerprint::from_metadata, needs);
    assert!(second.metadata_hits.is_empty());
}

#[test]
fn an_entry_without_the_call_facts_of_the_run_is_not_a_metadata_hit() {
    let needs = ParseNeeds {
        call_facts: true,
        ..NO_FACTS
    };
    let second = second_parse(SourceFingerprint::from_metadata, needs);
    assert!(second.metadata_hits.is_empty());
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
