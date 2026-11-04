use std::path::{Path, PathBuf};

use fallow_types::duplicates::{
    CloneFamily, CloneGroup, CloneInstance, DuplicationReport, DuplicationStats,
};

use super::build_duplication_markdown;

fn capped_report(shown: usize, corpus_groups: usize, corpus_families: usize) -> DuplicationReport {
    let group = |n: usize| CloneGroup {
        instances: vec![CloneInstance {
            is_symlink: false,
            file: PathBuf::from(format!("/project/src/a{n}.ts")),
            start_line: 1,
            end_line: 4,
            start_col: 0,
            end_col: 10,
            fragment: "const a = 1;".to_string(),
        }],
        token_count: 8,
        line_count: 4,
        similarity: None,
    };
    let family = |n: usize| CloneFamily {
        files: vec![PathBuf::from(format!("/project/src/a{n}.ts"))],
        groups: vec![group(n)],
        total_duplicated_lines: 4,
        total_duplicated_tokens: 8,
        suggestions: Vec::new(),
    };
    DuplicationReport {
        clone_groups: (0..shown).map(group).collect(),
        clone_families: (0..shown.min(corpus_families)).map(family).collect(),
        mirrored_directories: Vec::new(),
        stats: DuplicationStats {
            clone_groups: corpus_groups,
            clone_families: corpus_families,
            clone_instances: corpus_groups,
            total_files: 40,
            files_with_clones: 12,
            total_lines: 1000,
            duplicated_lines: 252,
            total_tokens: 5000,
            duplicated_tokens: 1200,
            duplication_percentage: 25.2,
            ..DuplicationStats::default()
        },
    }
}

// The heading and the duplication rate beside it are read as one sentence
// about one scope, so a capped listing must not rewrite the heading.
#[test]
fn a_capped_listing_still_names_the_measured_corpus() {
    let md = build_duplication_markdown(&capped_report(3, 251, 163), Path::new("/project"));
    assert!(
        md.starts_with("## Fallow: 251 clone groups found (25.2% duplication)"),
        "got: {md}"
    );
    assert!(
            md.contains("_Listing 3 of them; 248 more clone groups and 160 more clone families withheld by a display limit._"),
            "got: {md}"
        );
}

// An untruncated run must render exactly as before, note included.
#[test]
fn an_untruncated_listing_carries_no_omission_note() {
    let md = build_duplication_markdown(&capped_report(3, 3, 0), Path::new("/project"));
    assert!(
        md.starts_with("## Fallow: 3 clone groups found (25.2% duplication)"),
        "got: {md}"
    );
    assert!(!md.contains("withheld by a display limit"), "got: {md}");
}
