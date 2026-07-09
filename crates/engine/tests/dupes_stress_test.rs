#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Adversarial stress tests for the suffix array + LCP clone detection engine.

use std::path::PathBuf;

use fallow_config::DetectionMode;
use fallow_engine::duplicates::detect::CloneDetector;
use fallow_engine::duplicates::normalize::{HashedToken, normalize_and_hash};
use fallow_engine::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};
use oxc_span::Span;

/// Build a `Vec<HashedToken>` from raw hash values.
fn make_hashed_tokens(hashes: &[u64]) -> Vec<HashedToken> {
    hashes
        .iter()
        .enumerate()
        .map(|(i, &hash)| HashedToken {
            hash,
            original_index: i,
        })
        .collect()
}

/// Build a `Vec<SourceToken>` with synthetic spans that won't panic during
/// fragment extraction. Each token occupies 3 bytes in the source.
#[expect(
    clippy::cast_possible_truncation,
    reason = "test span values are trivially small"
)]
fn make_source_tokens(count: usize) -> Vec<SourceToken> {
    (0..count)
        .map(|i| SourceToken {
            kind: TokenKind::Identifier(format!("t{i}")),
            span: Span::new((i * 3) as u32, (i * 3 + 2) as u32),
        })
        .collect()
}

/// Build a `FileTokens` struct. The `source` is a synthetic string with enough
/// bytes for `count` tokens (each spanning 3 bytes) spread across lines.
fn make_file_tokens_for(count: usize) -> FileTokens {
    let mut source = String::with_capacity(count * 4);
    for i in 0..count {
        source.push_str("xx");
        if i < count - 1 {
            source.push('\n');
        }
    }
    let line_count = source.lines().count().max(1);
    FileTokens {
        tokens: make_source_tokens(count),
        atomic_invocation_spans: Vec::new(),
        source,
        line_count,
    }
}

type DetectInput = Vec<(PathBuf, Vec<HashedToken>, FileTokens)>;

#[test]
fn two_identical_large_files_single_group_no_quadratic_blowup() {
    let count = 1000;
    let hashes: Vec<u64> = (1..=count as u64).collect();

    let data: DetectInput = (0..2)
        .map(|i| {
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(count),
            )
        })
        .collect();

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect at least one clone group"
    );

    let largest = &report.clone_groups[0];
    assert_eq!(
        largest.token_count, count,
        "Token count of largest group should be {count}"
    );
    assert_eq!(
        largest.instances.len(),
        2,
        "The group should have exactly 2 instances"
    );
}

#[test]
fn three_identical_files_single_group_three_instances() {
    let hashes: Vec<u64> = (1..=50).collect();

    let data: DetectInput = (0..3)
        .map(|i| {
            (
                PathBuf::from(format!("pkg{i}/file.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(50),
            )
        })
        .collect();

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect clone groups for 3 identical files"
    );

    let max_instances = report
        .clone_groups
        .iter()
        .map(|g| g.instances.len())
        .max()
        .unwrap_or(0);

    assert_eq!(
        max_instances, 3,
        "Largest group should have 3 instances (one per file)"
    );
}

#[test]
fn five_identical_files_single_group_five_instances() {
    let hashes: Vec<u64> = (1..=50).collect();

    let data: DetectInput = (0..5)
        .map(|i| {
            (
                PathBuf::from(format!("pkg{i}/file.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(50),
            )
        })
        .collect();

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(!report.clone_groups.is_empty());

    let max_instances = report
        .clone_groups
        .iter()
        .map(|g| g.instances.len())
        .max()
        .unwrap_or(0);

    assert_eq!(
        max_instances, 5,
        "Largest group should have 5 instances (one per file)"
    );
}

#[test]
fn partial_overlap_detects_shared_region() {
    let hashes_a: Vec<u64> = (1..=100).collect();
    let hashes_b: Vec<u64> = (50..=150).collect();

    let data: DetectInput = vec![
        (
            PathBuf::from("src/a.ts"),
            make_hashed_tokens(&hashes_a),
            make_file_tokens_for(100),
        ),
        (
            PathBuf::from("lib/b.ts"),
            make_hashed_tokens(&hashes_b),
            make_file_tokens_for(101),
        ),
    ];

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect the overlapping region"
    );

    let largest = &report.clone_groups[0];
    assert_eq!(
        largest.token_count, 51,
        "Overlap region should be 51 tokens (50..=100)"
    );
    assert_eq!(largest.instances.len(), 2);
}

#[test]
fn completely_different_files_produce_zero_groups() {
    let hashes_a: Vec<u64> = (1..=50).collect();
    let hashes_b: Vec<u64> = (1001..=1050).collect();

    let data: DetectInput = vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes_a),
            make_file_tokens_for(50),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes_b),
            make_file_tokens_for(50),
        ),
    ];

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        report.clone_groups.is_empty(),
        "Completely different files should produce zero clone groups, got {}",
        report.clone_groups.len()
    );
}

#[test]
fn single_file_internal_repetition() {
    let hashes: Vec<u64> = vec![1, 2, 3, 4, 5, 99, 1, 2, 3, 4, 5];
    let count = hashes.len();

    let data: DetectInput = vec![(
        PathBuf::from("a.ts"),
        make_hashed_tokens(&hashes),
        make_file_tokens_for(count),
    )];

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect [1,2,3,4,5] duplicated within a single file"
    );

    let group = &report.clone_groups[0];
    assert_eq!(
        group.instances.len(),
        2,
        "Should have 2 non-overlapping instances"
    );
    assert_eq!(group.token_count, 5, "Duplicated block should be 5 tokens");

    assert_eq!(group.instances[0].file, group.instances[1].file);

    let first_end = group.instances[0].end_line;
    let second_start = group.instances[1].start_line;
    assert!(
        second_start > first_end,
        "Instances should not overlap: first ends at line {first_end}, second starts at {second_start}"
    );
}

#[test]
fn semantic_mode_detects_type2_clones() {
    let code_a = r#"
function processData(input) {
    const trimmed = input.trim();
    if (trimmed.length === 0) {
        return "";
    }
    const parts = trimmed.split(",");
    const filtered = parts.filter(p => p.length > 0);
    return filtered.join(", ");
}
"#;

    let code_b = r#"
function handlePayload(payload) {
    const cleaned = payload.trim();
    if (cleaned.length === 0) {
        return "";
    }
    const segments = cleaned.split(",");
    const valid = segments.filter(s => s.length > 0);
    return valid.join(", ");
}
"#;

    use fallow_engine::duplicates::tokenize::tokenize_file;

    let ft_a = tokenize_file(&PathBuf::from("a.ts"), code_a, false);
    let ft_b = tokenize_file(&PathBuf::from("b.ts"), code_b, false);

    let hashed_a = normalize_and_hash(&ft_a.tokens, DetectionMode::Semantic);
    let hashed_b = normalize_and_hash(&ft_b.tokens, DetectionMode::Semantic);

    assert!(
        !hashed_a.is_empty() && !hashed_b.is_empty(),
        "Tokenization should produce tokens"
    );

    let detector = CloneDetector::new(10, 1, false);
    let report = detector.detect(vec![
        (PathBuf::from("a.ts"), hashed_a, ft_a),
        (PathBuf::from("b.ts"), hashed_b, ft_b),
    ]);

    assert!(
        !report.clone_groups.is_empty(),
        "Semantic mode should detect Type-2 clones (renamed variables)"
    );

    let ft_a2 = tokenize_file(&PathBuf::from("a.ts"), code_a, false);
    let ft_b2 = tokenize_file(&PathBuf::from("b.ts"), code_b, false);
    let hashed_a2 = normalize_and_hash(&ft_a2.tokens, DetectionMode::Strict);
    let hashed_b2 = normalize_and_hash(&ft_b2.tokens, DetectionMode::Strict);

    let report_strict = detector.detect(vec![
        (PathBuf::from("a.ts"), hashed_a2, ft_a2),
        (PathBuf::from("b.ts"), hashed_b2, ft_b2),
    ]);

    let semantic_max = report
        .clone_groups
        .iter()
        .map(|g| g.token_count)
        .max()
        .unwrap_or(0);

    let strict_max = report_strict
        .clone_groups
        .iter()
        .map(|g| g.token_count)
        .max()
        .unwrap_or(0);

    assert!(
        semantic_max > strict_max,
        "Semantic mode should find larger clones ({semantic_max} tokens) \
         than strict mode ({strict_max} tokens) for renamed-variable code"
    );
}

#[test]
fn many_small_files_with_some_duplicates() {
    let shared_hashes: Vec<u64> = (1..=20).collect();
    let mut data: DetectInput = Vec::with_capacity(50);

    for i in 0..50 {
        let hashes = if i < 10 {
            shared_hashes.clone()
        } else {
            ((i * 1000 + 1)..=(i * 1000 + 20))
                .map(|v| v as u64)
                .collect()
        };

        data.push((
            PathBuf::from(format!("pkg{i}/file.ts")),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(20),
        ));
    }

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect clones among the first 10 identical files"
    );

    let max_instances = report
        .clone_groups
        .iter()
        .map(|g| g.instances.len())
        .max()
        .unwrap_or(0);

    assert_eq!(
        max_instances, 10,
        "Largest group should have 10 instances (the 10 identical files)"
    );

    assert_eq!(report.stats.total_files, 50);
}

#[test]
fn all_identical_tokens_does_not_hang() {
    let hashes = vec![42u64; 100];

    let data: DetectInput = vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(100),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(100),
        ),
    ];

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect clones in files with all-identical tokens"
    );
}

#[test]
fn empty_files_mixed_with_normal_files_do_not_crash() {
    let normal_hashes: Vec<u64> = (1..=20).collect();

    let data: DetectInput = vec![
        (
            PathBuf::from("empty.ts"),
            make_hashed_tokens(&[]),
            FileTokens {
                tokens: vec![],
                atomic_invocation_spans: Vec::new(),
                source: String::new(),
                line_count: 0,
            },
        ),
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&normal_hashes),
            make_file_tokens_for(20),
        ),
        (
            PathBuf::from("empty2.ts"),
            make_hashed_tokens(&[]),
            FileTokens {
                tokens: vec![],
                atomic_invocation_spans: Vec::new(),
                source: String::new(),
                line_count: 0,
            },
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&normal_hashes),
            make_file_tokens_for(20),
        ),
    ];

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect clones between the two non-empty identical files"
    );
}

#[test]
fn min_tokens_threshold_filters_small_clones() {
    let hashes: Vec<u64> = (1..=100).collect();

    let data: DetectInput = vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(100),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(100),
        ),
    ];

    let detector = CloneDetector::new(500, 1, false);
    let report = detector.detect(data);

    assert!(
        report.clone_groups.is_empty(),
        "min_tokens=500 should filter out all clones from 100-token files, got {} groups",
        report.clone_groups.len()
    );
}

#[test]
fn skip_local_filters_same_directory_keeps_cross_directory() {
    let hashes: Vec<u64> = (1..=50).collect();

    let data_same_dir: DetectInput = vec![
        (
            PathBuf::from("src/utils/a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(50),
        ),
        (
            PathBuf::from("src/utils/b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(50),
        ),
    ];

    let detector_skip = CloneDetector::new(5, 1, true);
    let report_same = detector_skip.detect(data_same_dir);

    assert!(
        report_same.clone_groups.is_empty(),
        "skip_local should filter same-directory clones"
    );

    let data_cross_dir: DetectInput = vec![
        (
            PathBuf::from("src/components/a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(50),
        ),
        (
            PathBuf::from("src/utils/b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(50),
        ),
    ];

    let report_cross = detector_skip.detect(data_cross_dir);

    assert!(
        !report_cross.clone_groups.is_empty(),
        "skip_local should keep cross-directory clones"
    );
}

#[test]
fn duplication_percentage_computation() {
    let hashes: Vec<u64> = (1..=20).collect();

    let data: DetectInput = vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(20),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(20),
        ),
    ];

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    assert!(
        report.stats.duplication_percentage > 0.0,
        "Duplication percentage should be > 0 for identical files"
    );
    assert!(
        report.stats.duplication_percentage <= 100.0,
        "Duplication percentage should not exceed 100%"
    );
    assert!(
        report.stats.duplicated_lines > 0,
        "Should have duplicated lines"
    );
    assert!(
        report.stats.files_with_clones >= 2,
        "Both files should be flagged as having clones"
    );
    assert_eq!(report.stats.total_files, 2);
}

#[test]
fn duplication_report_serializes_to_valid_json() {
    let hashes: Vec<u64> = (1..=30).collect();

    let data: DetectInput = vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(30),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens_for(30),
        ),
    ];

    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(data);

    let json = serde_json::to_string_pretty(&report).expect("Report should serialize to JSON");

    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("Serialized JSON should be valid");

    assert!(
        parsed.get("clone_groups").is_some(),
        "JSON should contain 'clone_groups' key"
    );
    assert!(
        parsed.get("stats").is_some(),
        "JSON should contain 'stats' key"
    );

    let stats = parsed.get("stats").unwrap();
    assert!(stats.get("total_files").is_some());
    assert!(stats.get("duplication_percentage").is_some());
    assert!(stats.get("duplicated_lines").is_some());
    assert!(stats.get("clone_groups").is_some());

    let groups = parsed.get("clone_groups").unwrap().as_array().unwrap();
    assert!(
        !groups.is_empty(),
        "Should have clone groups in JSON output"
    );

    for group in groups {
        assert!(group.get("instances").is_some());
        assert!(group.get("token_count").is_some());
        assert!(group.get("line_count").is_some());

        let instances = group.get("instances").unwrap().as_array().unwrap();
        for instance in instances {
            assert!(instance.get("file").is_some());
            assert!(instance.get("start_line").is_some());
            assert!(instance.get("end_line").is_some());
            assert!(instance.get("fragment").is_some());
        }
    }
}
