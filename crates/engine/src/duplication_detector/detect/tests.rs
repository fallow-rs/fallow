use std::path::PathBuf;

use oxc_span::Span;
use rustc_hash::FxHashMap;

use super::*;
use crate::duplicates::normalize::HashedToken;
use crate::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};

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

fn make_file_tokens(source: &str, count: usize) -> FileTokens {
    FileTokens {
        tokens: make_source_tokens(count),
        atomic_invocation_spans: Vec::new(),
        source: source.to_string(),
        line_count: source.lines().count().max(1),
    }
}

fn make_boundary_test_file(path: &str, has_boundary: bool) -> FileData {
    let kind = if has_boundary {
        TokenKind::Boundary("markup".to_string())
    } else {
        TokenKind::Identifier("value".to_string())
    };
    FileData {
        path: PathBuf::from(path),
        hashed_tokens: vec![HashedToken {
            hash: 1,
            original_index: 0,
        }],
        file_tokens: FileTokens {
            tokens: vec![SourceToken {
                kind,
                span: Span::new(0, 1),
            }],
            atomic_invocation_spans: Vec::new(),
            source: "x".to_string(),
            line_count: 1,
        },
        atomic_invocation_spans: Vec::new(),
    }
}

#[test]
fn component_heavy_corpus_detects_boundary_ratio_at_threshold() {
    let files = vec![
        make_boundary_test_file("a.astro", true),
        make_boundary_test_file("b.ts", false),
        make_boundary_test_file("c.ts", false),
        make_boundary_test_file("d.ts", false),
        make_boundary_test_file("e.ts", false),
    ];

    let summary = summarize_boundaries(&files);
    assert!(summary.is_component_heavy);
    assert!(summary.has_any_boundary);
}

#[test]
fn component_heavy_corpus_ignores_sparse_boundary_files() {
    let mut files = vec![make_boundary_test_file("a.mdx", true)];
    for index in 0..10 {
        files.push(make_boundary_test_file(&format!("file{index}.ts"), false));
    }

    let summary = summarize_boundaries(&files);
    assert!(!summary.is_component_heavy);
    assert!(summary.has_any_boundary);
}

#[test]
fn boundary_precheck_skips_plain_js_ts_corpus() {
    let files = vec![
        make_boundary_test_file("a.ts", false),
        make_boundary_test_file("b.tsx", false),
        make_boundary_test_file("c.js", false),
        make_boundary_test_file("d.jsx", false),
    ];

    assert!(!files_may_have_boundaries(&files));
}

#[test]
fn boundary_precheck_keeps_component_and_style_corpus() {
    for path in [
        "App.vue",
        "Counter.svelte",
        "page.astro",
        "style.css",
        "style.scss",
        "style.sass",
        "style.less",
    ] {
        let files = vec![make_boundary_test_file(path, false)];
        assert!(files_may_have_boundaries(&files), "{path}");
    }
}

#[test]
fn empty_input_produces_empty_report() {
    let detector = CloneDetector::new(5, 1, false);
    let report = detector.detect(vec![]);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 0);
}

#[test]
fn single_file_no_clones() {
    let detector = CloneDetector::new(3, 1, false);
    let hashed = make_hashed_tokens(&[1, 2, 3, 4, 5]);
    let ft = make_file_tokens("a b c d e", 5);
    let report = detector.detect(vec![(PathBuf::from("a.ts"), hashed, ft)]);
    assert!(report.clone_groups.is_empty());
}

#[test]
fn detects_exact_duplicate_across_files() {
    let detector = CloneDetector::new(3, 1, false);

    let hashes = vec![10, 20, 30, 40, 50];
    let source_a = "a\nb\nc\nd\ne";
    let source_b = "a\nb\nc\nd\ne";

    let hashed_a = make_hashed_tokens(&hashes);
    let hashed_b = make_hashed_tokens(&hashes);
    let ft_a = make_file_tokens(source_a, 5);
    let ft_b = make_file_tokens(source_b, 5);

    let report = detector.detect(vec![
        (PathBuf::from("a.ts"), hashed_a, ft_a),
        (PathBuf::from("b.ts"), hashed_b, ft_b),
    ]);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect at least one clone group"
    );
}

#[test]
fn no_detection_below_min_tokens() {
    let detector = CloneDetector::new(10, 1, false);

    let hashes = vec![10, 20, 30]; // Only 3 tokens, min is 10
    let hashed_a = make_hashed_tokens(&hashes);
    let hashed_b = make_hashed_tokens(&hashes);
    let ft_a = make_file_tokens("abc", 3);
    let ft_b = make_file_tokens("abc", 3);

    let report = detector.detect(vec![
        (PathBuf::from("a.ts"), hashed_a, ft_a),
        (PathBuf::from("b.ts"), hashed_b, ft_b),
    ]);

    assert!(report.clone_groups.is_empty());
}

#[test]
fn byte_offset_to_line_col_basic() {
    let source = "abc\ndef\nghi";
    assert_eq!(utils::byte_offset_to_line_col(source, 0), (1, 0));
    assert_eq!(utils::byte_offset_to_line_col(source, 4), (2, 0));
    assert_eq!(utils::byte_offset_to_line_col(source, 5), (2, 1));
    assert_eq!(utils::byte_offset_to_line_col(source, 8), (3, 0));
}

#[test]
fn byte_offset_beyond_source() {
    let source = "abc";
    let (line, col) = utils::byte_offset_to_line_col(source, 100);
    assert_eq!(line, 1);
    assert_eq!(col, 3);
}

#[test]
fn skip_local_filters_same_directory() {
    let detector = CloneDetector::new(3, 1, true);

    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";

    let hashed_a = make_hashed_tokens(&hashes);
    let hashed_b = make_hashed_tokens(&hashes);
    let ft_a = make_file_tokens(source, 5);
    let ft_b = make_file_tokens(source, 5);

    let report = detector.detect(vec![
        (PathBuf::from("src/a.ts"), hashed_a, ft_a),
        (PathBuf::from("src/b.ts"), hashed_b, ft_b),
    ]);

    assert!(
        report.clone_groups.is_empty(),
        "Same-directory clones should be filtered with skip_local"
    );
}

#[test]
fn skip_local_keeps_cross_directory() {
    let detector = CloneDetector::new(3, 1, true);

    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";

    let hashed_a = make_hashed_tokens(&hashes);
    let hashed_b = make_hashed_tokens(&hashes);
    let ft_a = make_file_tokens(source, 5);
    let ft_b = make_file_tokens(source, 5);

    let report = detector.detect(vec![
        (PathBuf::from("src/components/a.ts"), hashed_a, ft_a),
        (PathBuf::from("src/utils/b.ts"), hashed_b, ft_b),
    ]);

    assert!(
        !report.clone_groups.is_empty(),
        "Cross-directory clones should be kept with skip_local"
    );
}

#[test]
fn stats_computation() {
    use crate::duplicates::types::{CloneGroup, CloneInstance};

    let groups = vec![CloneGroup {
        instances: vec![
            CloneInstance {
                file: PathBuf::from("a.ts"),
                start_line: 1,
                end_line: 5,
                start_col: 0,
                end_col: 10,
                fragment: "...".to_string(),
            },
            CloneInstance {
                file: PathBuf::from("b.ts"),
                start_line: 10,
                end_line: 14,
                start_col: 0,
                end_col: 10,
                fragment: "...".to_string(),
            },
        ],
        token_count: 50,
        line_count: 5,
    }];

    let stats = statistics::compute_stats(&groups, 10, 200, 1000);
    assert_eq!(stats.total_files, 10);
    assert_eq!(stats.files_with_clones, 2);
    assert_eq!(stats.clone_groups, 1);
    assert_eq!(stats.clone_instances, 2);
    assert_eq!(stats.duplicated_lines, 10); // 5 lines in each of 2 instances
    assert!(stats.duplication_percentage > 0.0);
}

#[test]
fn sa_construction_basic() {
    let text: Vec<i64> = vec![1, 0, 2, 0, 2, 0];
    let sa = suffix_array::build_suffix_array(&text);

    assert_eq!(sa, vec![5, 3, 1, 0, 4, 2]);
}

#[test]
fn lcp_construction_basic() {
    let text: Vec<i64> = vec![1, 0, 2, 0, 2, 0];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    assert_eq!(lcp_arr, vec![0, 1, 3, 0, 0, 2]);
}

#[test]
fn lcp_stops_at_sentinels() {
    let text: Vec<i64> = vec![0, 1, 2, -1, 0, 1, 2];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let rank_0 = sa.iter().position(|&s| s == 0).expect("pos 0 in SA");
    let rank_4 = sa.iter().position(|&s| s == 4).expect("pos 4 in SA");
    let (lo, hi) = if rank_0 < rank_4 {
        (rank_0, rank_4)
    } else {
        (rank_4, rank_0)
    };

    let min_lcp = lcp_arr[(lo + 1)..=hi].iter().copied().min().unwrap_or(0);
    assert_eq!(
        min_lcp, 3,
        "LCP between identical sequences across sentinel should be 3"
    );
}

#[test]
fn rank_reduction_maps_correctly() {
    let files = vec![
        FileData {
            path: PathBuf::from("a.ts"),
            hashed_tokens: make_hashed_tokens(&[100, 200, 300]),
            file_tokens: make_file_tokens("a b c", 3),
            atomic_invocation_spans: Vec::new(),
        },
        FileData {
            path: PathBuf::from("b.ts"),
            hashed_tokens: make_hashed_tokens(&[200, 300, 400]),
            file_tokens: make_file_tokens("d e f", 3),
            atomic_invocation_spans: Vec::new(),
        },
    ];

    let ranked = ranking::rank_reduce(&files);

    assert_eq!(ranked[0], vec![0, 1, 2]);
    assert_eq!(ranked[1], vec![1, 2, 3]);
}

#[test]
fn three_file_grouping() {
    let detector = CloneDetector::new(3, 1, false);

    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";

    let data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)> = (0..3)
        .map(|i| {
            (
                PathBuf::from(format!("file{i}.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens(source, 5),
            )
        })
        .collect();

    let report = detector.detect(data);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect clones across 3 identical files"
    );

    let max_instances = report
        .clone_groups
        .iter()
        .map(|g| g.instances.len())
        .max()
        .unwrap_or(0);
    assert_eq!(
        max_instances, 3,
        "3 identical files should produce a group with 3 instances"
    );
}

#[test]
fn overlapping_clones_largest_wins() {
    let detector = CloneDetector::new(3, 1, false);

    let hashes: Vec<u64> = (1..=10).collect();
    let source = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj";

    let hashed_a = make_hashed_tokens(&hashes);
    let hashed_b = make_hashed_tokens(&hashes);
    let ft_a = make_file_tokens(source, 10);
    let ft_b = make_file_tokens(source, 10);

    let report = detector.detect(vec![
        (PathBuf::from("a.ts"), hashed_a, ft_a),
        (PathBuf::from("b.ts"), hashed_b, ft_b),
    ]);

    assert!(!report.clone_groups.is_empty());
    assert_eq!(
        report.clone_groups[0].token_count, 10,
        "Maximal clone should cover all 10 tokens"
    );
}

#[test]
fn no_self_overlap() {
    let detector = CloneDetector::new(3, 1, false);

    let hashes = vec![1, 2, 3, 1, 2, 3];
    let source = "aa\nbb\ncc\ndd\nee\nff\ngg";

    let hashed = make_hashed_tokens(&hashes);
    let ft = make_file_tokens(source, 6);

    let report = detector.detect(vec![(PathBuf::from("a.ts"), hashed, ft)]);

    for group in &report.clone_groups {
        let mut file_instances: FxHashMap<&PathBuf, Vec<(usize, usize)>> = FxHashMap::default();
        for inst in &group.instances {
            file_instances
                .entry(&inst.file)
                .or_default()
                .push((inst.start_line, inst.end_line));
        }
        for (_file, mut ranges) in file_instances {
            ranges.sort_unstable();
            for w in ranges.windows(2) {
                assert!(
                    w[1].0 > w[0].1,
                    "Clone instances in the same file should not overlap: {:?} and {:?}",
                    w[0],
                    w[1]
                );
            }
        }
    }
}

#[test]
fn empty_input_edge_case() {
    let detector = CloneDetector::new(0, 0, false);
    let report = detector.detect(vec![]);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 0);
}

#[test]
fn single_file_internal_duplication() {
    let detector = CloneDetector::new(3, 1, false);

    let hashes = vec![10, 20, 30, 99, 10, 20, 30];
    let source = "a\nb\nc\nx\na\nb\nc";

    let hashed = make_hashed_tokens(&hashes);
    let ft = make_file_tokens(source, 7);

    let report = detector.detect(vec![(PathBuf::from("a.ts"), hashed, ft)]);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect internal duplication within a single file"
    );
}

#[test]
fn sa_empty_input() {
    let sa = suffix_array::build_suffix_array(&[]);
    assert!(sa.is_empty());
}

#[test]
fn sa_single_element() {
    let sa = suffix_array::build_suffix_array(&[42]);
    assert_eq!(sa, vec![0]);
}

#[test]
fn sa_two_elements_sorted() {
    let sa = suffix_array::build_suffix_array(&[0, 1]);
    assert_eq!(sa, vec![0, 1]);
}

#[test]
fn sa_two_elements_reversed() {
    let sa = suffix_array::build_suffix_array(&[1, 0]);
    assert_eq!(sa, vec![1, 0]);
}

#[test]
fn sa_all_identical() {
    let sa = suffix_array::build_suffix_array(&[3, 3, 3, 3]);
    assert_eq!(sa, vec![3, 2, 1, 0]);
}

#[test]
fn sa_already_sorted() {
    let sa = suffix_array::build_suffix_array(&[0, 1, 2, 3]);
    assert_eq!(sa, vec![0, 1, 2, 3]);
}

#[test]
fn sa_reverse_sorted() {
    let sa = suffix_array::build_suffix_array(&[3, 2, 1, 0]);
    assert_eq!(sa, vec![3, 2, 1, 0]);
}

#[test]
fn sa_with_negative_sentinels() {
    let text: Vec<i64> = vec![5, 10, -1, 5, 10];
    let sa = suffix_array::build_suffix_array(&text);

    let mut sorted_sa = sa.clone();
    sorted_sa.sort_unstable();
    assert_eq!(sorted_sa, vec![0, 1, 2, 3, 4]);

    assert_eq!(sa[0], 2, "Sentinel position should be first in SA");
}

#[test]
fn sa_ordering_invariant() {
    let text: Vec<i64> = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5];
    let sa = suffix_array::build_suffix_array(&text);

    for i in 0..sa.len() - 1 {
        let s1 = &text[sa[i]..];
        let s2 = &text[sa[i + 1]..];
        assert!(
            s1 < s2,
            "SA ordering violated at position {i}: suffix at {} ({s1:?}) >= suffix at {} ({s2:?})",
            sa[i],
            sa[i + 1]
        );
    }
}

#[test]
fn sa_is_valid_permutation() {
    let text: Vec<i64> = vec![1, 0, 2, 0, 2, 0];
    let mut sa = suffix_array::build_suffix_array(&text);
    let n = text.len();
    sa.sort_unstable();
    let expected: Vec<usize> = (0..n).collect();
    assert_eq!(sa, expected, "SA must be a permutation of 0..n");
}

#[test]
fn sa_multiple_sentinels() {
    let text: Vec<i64> = vec![1, 2, -1, 3, 4, -2, 5, 6];
    let sa = suffix_array::build_suffix_array(&text);

    assert_eq!(sa[0], 5, "Most negative sentinel should be first");
    assert_eq!(sa[1], 2, "Second sentinel should be second");
}

#[test]
fn lcp_empty_input() {
    let lcp_arr = lcp::build_lcp(&[], &[]);
    assert!(lcp_arr.is_empty());
}

#[test]
fn lcp_single_element() {
    let text: Vec<i64> = vec![42];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);
    assert_eq!(lcp_arr, vec![0]);
}

#[test]
fn lcp_no_common_prefixes() {
    let text: Vec<i64> = vec![0, 1, 2, 3];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    assert_eq!(lcp_arr[0], 0);

    for v in &lcp_arr {
        assert_eq!(*v, 0);
    }
}

#[test]
fn lcp_all_identical() {
    let text: Vec<i64> = vec![5, 5, 5, 5];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);
    assert_eq!(lcp_arr, vec![0, 1, 2, 3]);
}

#[test]
fn lcp_sentinel_prevents_cross_file_extension() {
    let text: Vec<i64> = vec![1, 2, 3, -1, 1, 2, 3, 4];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let rank_0 = sa.iter().position(|&s| s == 0).unwrap();
    let rank_4 = sa.iter().position(|&s| s == 4).unwrap();
    let (lo, hi) = if rank_0 < rank_4 {
        (rank_0, rank_4)
    } else {
        (rank_4, rank_0)
    };
    let min_lcp = lcp_arr[(lo + 1)..=hi].iter().copied().min().unwrap();
    assert_eq!(min_lcp, 3, "LCP should stop at sentinel, not extend to 4");
}

#[test]
fn lcp_multiple_sentinels_between_files() {
    let text: Vec<i64> = vec![10, 20, -1, 10, 20, -2, 10, 20];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let positions = [0usize, 3, 6];
    let mut ranks: Vec<usize> = positions
        .iter()
        .map(|&p| sa.iter().position(|&s| s == p).unwrap())
        .collect();
    ranks.sort_unstable();

    for w in ranks.windows(2) {
        let min_lcp = lcp_arr[(w[0] + 1)..=w[1]].iter().copied().min().unwrap();
        assert_eq!(
            min_lcp, 2,
            "LCP between identical sequences across sentinels should be 2"
        );
    }
}

#[test]
fn lcp_sentinel_at_start() {
    let text: Vec<i64> = vec![-1, 5, 10];
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    assert_eq!(lcp_arr[0], 0);
}

#[test]
fn concat_empty_files_list() {
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&[]);
    assert!(text.is_empty());
    assert!(file_of.is_empty());
    assert!(file_offsets.is_empty());
}

#[test]
fn concat_single_file_no_sentinel() {
    let files = vec![vec![1u32, 2, 3]];
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&files);

    assert_eq!(text, vec![1i64, 2, 3]);
    assert_eq!(file_of, vec![0, 0, 0]);
    assert_eq!(file_offsets, vec![0]);
}

#[test]
fn concat_two_files_one_sentinel() {
    let files = vec![vec![1u32, 2], vec![3u32, 4]];
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&files);

    assert_eq!(text, vec![1i64, 2, -1, 3, 4]);
    assert_eq!(file_of, vec![0, 0, usize::MAX, 1, 1]);
    assert_eq!(file_offsets, vec![0, 3]);
}

#[test]
fn concat_three_files_unique_sentinels() {
    let files = vec![vec![10u32], vec![20u32], vec![30u32]];
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&files);

    assert_eq!(text, vec![10i64, -1, 20, -2, 30]);
    assert_eq!(file_of, vec![0, usize::MAX, 1, usize::MAX, 2]);
    assert_eq!(file_offsets, vec![0, 2, 4]);
}

#[test]
fn concat_sentinels_are_unique() {
    let files = vec![vec![0u32; 3]; 5]; // 5 files of 3 tokens each
    let (text, _file_of, _file_offsets) = concatenation::concatenate_with_sentinels(&files);

    let sentinels: Vec<i64> = text.iter().copied().filter(|&v| v < 0).collect();
    assert_eq!(sentinels.len(), 4, "4 sentinels between 5 files");

    let unique: rustc_hash::FxHashSet<i64> = sentinels.iter().copied().collect();
    assert_eq!(
        unique.len(),
        sentinels.len(),
        "All sentinels must be unique"
    );
}

#[test]
fn concat_file_of_maps_correctly() {
    let files = vec![vec![1u32, 2, 3], vec![4u32, 5]];
    let (text, file_of, _) = concatenation::concatenate_with_sentinels(&files);

    for (pos, &fid) in file_of.iter().enumerate() {
        if text[pos] < 0 {
            assert_eq!(
                fid,
                usize::MAX,
                "Sentinel positions should map to usize::MAX"
            );
        } else if pos < 3 {
            assert_eq!(fid, 0, "Position {pos} should belong to file 0");
        } else {
            assert_eq!(fid, 1, "Position {pos} should belong to file 1");
        }
    }
}

#[test]
fn concat_file_offsets_are_correct() {
    let files = vec![vec![1u32, 2, 3], vec![4u32, 5, 6, 7], vec![8u32]];
    let (_text, _file_of, file_offsets) = concatenation::concatenate_with_sentinels(&files);

    assert_eq!(file_offsets[0], 0, "First file starts at 0");
    assert_eq!(
        file_offsets[1], 4,
        "Second file starts after 3 tokens + 1 sentinel"
    );
    assert_eq!(file_offsets[2], 9, "Third file starts after 3+1+4+1 = 9");
}

#[test]
fn concat_empty_file_in_middle() {
    let files = vec![vec![1u32, 2], vec![], vec![3u32, 4]];
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&files);

    assert_eq!(file_offsets.len(), 3);
    assert_eq!(text.len(), 6);
    assert_eq!(
        file_offsets[1], 3,
        "Empty file offset is after first sentinel"
    );

    assert_eq!(text[2], -1);
    assert_eq!(text[3], -2);
    assert_eq!(file_of[2], usize::MAX);
    assert_eq!(file_of[3], usize::MAX);
}

fn make_file_data(path: &str, source: &str, num_tokens: usize) -> FileData {
    FileData {
        path: PathBuf::from(path),
        hashed_tokens: make_hashed_tokens(&(0..num_tokens as u64).collect::<Vec<_>>()),
        file_tokens: make_file_tokens(source, num_tokens),
        atomic_invocation_spans: Vec::new(),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "test helper; thin wrapper bundling fixture inputs for clone-group extraction"
)]
fn extract_clone_groups_for_test(
    sa: &[usize],
    lcp: &[usize],
    file_of: &[usize],
    file_offsets: &[usize],
    min_tokens: usize,
    files: &[FileData],
    focus_file_ids: Option<&[bool]>,
) -> Vec<extraction::RawGroup> {
    extraction::extract_clone_groups(&extraction::CloneGroupExtractionInput {
        sa,
        lcp,
        file_of,
        file_offsets,
        min_tokens,
        files,
        focus_file_ids,
        may_have_boundaries: files_may_have_boundaries(files),
    })
}

#[test]
fn extraction_empty_sa() {
    let groups = extract_clone_groups_for_test(&[], &[], &[], &[], 3, &[], None);
    assert!(groups.is_empty());
}

#[test]
fn extraction_single_suffix_no_groups() {
    let groups = extract_clone_groups_for_test(&[0], &[0], &[0], &[0], 1, &[], None);
    assert!(groups.is_empty());
}

#[test]
fn extraction_below_min_tokens_no_groups() {
    let files = vec![
        make_file_data("a.ts", "ab", 2),
        make_file_data("b.ts", "ab", 2),
    ];
    let ranked = ranking::rank_reduce(&files);
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&ranked);
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let groups = extract_clone_groups_for_test(
        &sa,
        &lcp_arr,
        &file_of,
        &file_offsets,
        10, // min_tokens > file length
        &files,
        None,
    );
    assert!(groups.is_empty());
}

#[test]
fn extraction_skips_sentinel_positions() {
    let files = vec![
        make_file_data("a.ts", "aa\nbb\ncc", 3),
        make_file_data("b.ts", "aa\nbb\ncc", 3),
    ];
    let ranked = ranking::rank_reduce(&files);
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&ranked);
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let groups =
        extract_clone_groups_for_test(&sa, &lcp_arr, &file_of, &file_offsets, 2, &files, None);

    for group in &groups {
        for &(fid, _offset) in &group.instances {
            assert_ne!(
                fid,
                usize::MAX,
                "Sentinel positions must not appear in instances"
            );
        }
    }
}

#[test]
fn extraction_produces_valid_offsets() {
    let files = vec![
        make_file_data("a.ts", "aa\nbb\ncc\ndd\nee", 5),
        make_file_data("b.ts", "aa\nbb\ncc\ndd\nee", 5),
    ];
    let ranked = ranking::rank_reduce(&files);
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&ranked);
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let groups =
        extract_clone_groups_for_test(&sa, &lcp_arr, &file_of, &file_offsets, 2, &files, None);

    for group in &groups {
        for &(fid, offset) in &group.instances {
            assert!(
                offset + group.length <= files[fid].hashed_tokens.len(),
                "Instance offset {offset} + length {} exceeds file {fid} token count {}",
                group.length,
                files[fid].hashed_tokens.len()
            );
        }
    }
}

#[test]
fn extraction_removes_overlapping_same_file() {
    let hashed = make_hashed_tokens(&[1, 2, 1, 2, 1]);
    let file = FileData {
        path: PathBuf::from("a.ts"),
        hashed_tokens: hashed,
        file_tokens: make_file_tokens("aa\nbb\ncc\ndd\nee", 5),
        atomic_invocation_spans: Vec::new(),
    };
    let files = vec![file];
    let ranked = ranking::rank_reduce(&files);
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&ranked);
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let groups =
        extract_clone_groups_for_test(&sa, &lcp_arr, &file_of, &file_offsets, 2, &files, None);

    for group in &groups {
        let mut same_file: Vec<(usize, usize)> = group
            .instances
            .iter()
            .filter(|&&(fid, _)| fid == 0)
            .map(|&(_, offset)| (offset, offset + group.length))
            .collect();
        same_file.sort_unstable();
        for w in same_file.windows(2) {
            assert!(
                w[1].0 >= w[0].1,
                "Overlapping instances: [{}, {}) and [{}, {})",
                w[0].0,
                w[0].1,
                w[1].0,
                w[1].1
            );
        }
    }
}

#[test]
fn extraction_at_least_two_instances() {
    let files = vec![
        make_file_data("a.ts", "aa\nbb\ncc\ndd\nee", 5),
        make_file_data("b.ts", "aa\nbb\ncc\ndd\nee", 5),
    ];
    let ranked = ranking::rank_reduce(&files);
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&ranked);
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let groups =
        extract_clone_groups_for_test(&sa, &lcp_arr, &file_of, &file_offsets, 2, &files, None);

    for group in &groups {
        assert!(
            group.instances.len() >= 2,
            "Group with length {} has only {} instance(s)",
            group.length,
            group.instances.len()
        );
    }
}

#[test]
fn rank_reduce_empty_files() {
    let ranked = ranking::rank_reduce(&[]);
    assert!(ranked.is_empty());
}

#[test]
fn rank_reduce_single_empty_file() {
    let files = vec![FileData {
        path: PathBuf::from("a.ts"),
        hashed_tokens: vec![],
        file_tokens: make_file_tokens("", 0),
        atomic_invocation_spans: Vec::new(),
    }];
    let ranked = ranking::rank_reduce(&files);
    assert_eq!(ranked.len(), 1);
    assert!(ranked[0].is_empty());
}

#[test]
fn rank_reduce_all_same_hash() {
    let files = vec![FileData {
        path: PathBuf::from("a.ts"),
        hashed_tokens: make_hashed_tokens(&[42, 42, 42]),
        file_tokens: make_file_tokens("a b c", 3),
        atomic_invocation_spans: Vec::new(),
    }];
    let ranked = ranking::rank_reduce(&files);
    assert_eq!(ranked[0][0], ranked[0][1]);
    assert_eq!(ranked[0][1], ranked[0][2]);
}

#[test]
fn rank_reduce_preserves_equality() {
    let files = vec![
        FileData {
            path: PathBuf::from("a.ts"),
            hashed_tokens: make_hashed_tokens(&[10, 20, 30]),
            file_tokens: make_file_tokens("a b c", 3),
            atomic_invocation_spans: Vec::new(),
        },
        FileData {
            path: PathBuf::from("b.ts"),
            hashed_tokens: make_hashed_tokens(&[30, 20, 10]),
            file_tokens: make_file_tokens("d e f", 3),
            atomic_invocation_spans: Vec::new(),
        },
    ];
    let ranked = ranking::rank_reduce(&files);

    assert_eq!(ranked[0][0], ranked[1][2], "Hash 10 must map to same rank");
    assert_eq!(ranked[0][1], ranked[1][1], "Hash 20 must map to same rank");
    assert_eq!(ranked[0][2], ranked[1][0], "Hash 30 must map to same rank");
}

#[test]
fn rank_reduce_distinct_hashes_get_distinct_ranks() {
    let files = vec![FileData {
        path: PathBuf::from("a.ts"),
        hashed_tokens: make_hashed_tokens(&[100, 200, 300, 400]),
        file_tokens: make_file_tokens("a b c d", 4),
        atomic_invocation_spans: Vec::new(),
    }];
    let ranked = ranking::rank_reduce(&files);

    let mut ranks = ranked[0].clone();
    ranks.sort_unstable();
    ranks.dedup();
    assert_eq!(
        ranks.len(),
        4,
        "4 distinct hashes should produce 4 distinct ranks"
    );
}

#[test]
fn stats_empty_groups() {
    let stats = statistics::compute_stats(&[], 5, 100, 500);
    assert_eq!(stats.total_files, 5);
    assert_eq!(stats.files_with_clones, 0);
    assert_eq!(stats.clone_groups, 0);
    assert_eq!(stats.clone_instances, 0);
    assert_eq!(stats.duplicated_lines, 0);
    assert_eq!(stats.duplicated_tokens, 0);
    assert!((stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
}

#[test]
fn stats_zero_total_lines() {
    use crate::duplicates::types::{CloneGroup, CloneInstance};

    let groups = vec![CloneGroup {
        instances: vec![
            CloneInstance {
                file: PathBuf::from("a.ts"),
                start_line: 1,
                end_line: 1,
                start_col: 0,
                end_col: 5,
                fragment: String::new(),
            },
            CloneInstance {
                file: PathBuf::from("b.ts"),
                start_line: 1,
                end_line: 1,
                start_col: 0,
                end_col: 5,
                fragment: String::new(),
            },
        ],
        token_count: 10,
        line_count: 1,
    }];

    let stats = statistics::compute_stats(&groups, 2, 0, 100);
    assert!((stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
}

#[test]
fn stats_duplicated_tokens_capped() {
    use crate::duplicates::types::{CloneGroup, CloneInstance};

    let groups = vec![CloneGroup {
        instances: vec![
            CloneInstance {
                file: PathBuf::from("a.ts"),
                start_line: 1,
                end_line: 10,
                start_col: 0,
                end_col: 10,
                fragment: String::new(),
            },
            CloneInstance {
                file: PathBuf::from("b.ts"),
                start_line: 1,
                end_line: 10,
                start_col: 0,
                end_col: 10,
                fragment: String::new(),
            },
            CloneInstance {
                file: PathBuf::from("c.ts"),
                start_line: 1,
                end_line: 10,
                start_col: 0,
                end_col: 10,
                fragment: String::new(),
            },
        ],
        token_count: 100,
        line_count: 10,
    }];

    let stats = statistics::compute_stats(&groups, 3, 30, 50);
    assert_eq!(
        stats.duplicated_tokens, 50,
        "duplicated_tokens must be capped to total_tokens"
    );
}

#[test]
fn stats_multiple_groups_same_file() {
    use crate::duplicates::types::{CloneGroup, CloneInstance};

    let groups = vec![
        CloneGroup {
            instances: vec![
                CloneInstance {
                    file: PathBuf::from("a.ts"),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 10,
                    fragment: String::new(),
                },
                CloneInstance {
                    file: PathBuf::from("b.ts"),
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 10,
                    fragment: String::new(),
                },
            ],
            token_count: 20,
            line_count: 5,
        },
        CloneGroup {
            instances: vec![
                CloneInstance {
                    file: PathBuf::from("a.ts"),
                    start_line: 3,
                    end_line: 8,
                    start_col: 0,
                    end_col: 10,
                    fragment: String::new(),
                },
                CloneInstance {
                    file: PathBuf::from("b.ts"),
                    start_line: 3,
                    end_line: 8,
                    start_col: 0,
                    end_col: 10,
                    fragment: String::new(),
                },
            ],
            token_count: 30,
            line_count: 6,
        },
    ];

    let stats = statistics::compute_stats(&groups, 2, 100, 500);
    assert_eq!(stats.files_with_clones, 2);
    assert_eq!(stats.clone_groups, 2);
    assert_eq!(stats.clone_instances, 4);
    assert_eq!(stats.duplicated_lines, 16);
}

#[test]
fn stats_single_instance_no_duplicated_tokens() {
    use crate::duplicates::types::{CloneGroup, CloneInstance};

    let groups = vec![CloneGroup {
        instances: vec![CloneInstance {
            file: PathBuf::from("a.ts"),
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 10,
            fragment: String::new(),
        }],
        token_count: 50,
        line_count: 5,
    }];

    let stats = statistics::compute_stats(&groups, 1, 100, 500);
    assert_eq!(stats.duplicated_tokens, 0);
}

#[test]
fn byte_offset_to_line_col_fast_matches_simple() {
    let source = "abc\ndef\nghi";
    let line_table: Vec<usize> = source
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| if b == b'\n' { Some(i) } else { None })
        .collect();

    for offset in 0..source.len() {
        let fast = utils::byte_offset_to_line_col_fast(source, offset, &line_table);
        let simple = utils::byte_offset_to_line_col(source, offset);
        assert_eq!(fast, simple, "Mismatch at offset {offset}");
    }
}

#[test]
fn byte_offset_to_line_col_fast_beyond_source() {
    let source = "abc\ndef";
    let line_table: Vec<usize> = source
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| if b == b'\n' { Some(i) } else { None })
        .collect();

    let (line, col) = utils::byte_offset_to_line_col_fast(source, 1000, &line_table);
    let (line_s, col_s) = utils::byte_offset_to_line_col(source, 1000);
    assert_eq!((line, col), (line_s, col_s));
}

#[test]
fn byte_offset_to_line_col_fast_empty_source() {
    let source = "";
    let line_table: Vec<usize> = vec![];
    let (line, col) = utils::byte_offset_to_line_col_fast(source, 0, &line_table);
    assert_eq!(line, 1);
    assert_eq!(col, 0);
}

#[test]
fn byte_offset_to_line_col_fast_at_newlines() {
    let source = "a\nb\nc";
    let line_table: Vec<usize> = vec![1, 3]; // newlines at byte 1 and 3

    let (line, col) = utils::byte_offset_to_line_col_fast(source, 1, &line_table);
    assert_eq!(line, 1, "Newline byte belongs to line 1");
    assert_eq!(col, 1, "Column should be 1 (after 'a')");

    let (line, col) = utils::byte_offset_to_line_col_fast(source, 2, &line_table);
    assert_eq!(line, 2, "Byte after first newline is line 2");
    assert_eq!(col, 0, "Column should be 0 at start of line");
}

#[test]
fn byte_offset_to_line_col_multibyte_chars() {
    let source = "\u{1F600}\n\u{1F601}"; // 4 bytes + \n + 4 bytes
    let (line, col) = utils::byte_offset_to_line_col(source, 0);
    assert_eq!(line, 1);
    assert_eq!(col, 0);

    let (line, col) = utils::byte_offset_to_line_col(source, 4);
    assert_eq!(line, 1);
    assert_eq!(col, 1); // one character before the newline

    let (line, col) = utils::byte_offset_to_line_col(source, 5);
    assert_eq!(line, 2);
    assert_eq!(col, 0);
}

#[test]
fn byte_offset_to_line_col_inside_multibyte() {
    let source = "\u{1F600}abc"; // 4-byte emoji + 3 ASCII
    let (line, col) = utils::byte_offset_to_line_col(source, 2);
    assert_eq!(line, 1);
    assert_eq!(col, 0, "Should snap to character boundary");
}

#[test]
fn pipeline_rank_concat_sa_lcp_roundtrip() {
    let files = vec![
        FileData {
            path: PathBuf::from("a.ts"),
            hashed_tokens: make_hashed_tokens(&[10, 20, 30, 40]),
            file_tokens: make_file_tokens("a\nb\nc\nd", 4),
            atomic_invocation_spans: Vec::new(),
        },
        FileData {
            path: PathBuf::from("b.ts"),
            hashed_tokens: make_hashed_tokens(&[10, 20, 30, 50]),
            file_tokens: make_file_tokens("e\nf\ng\nh", 4),
            atomic_invocation_spans: Vec::new(),
        },
    ];

    let ranked = ranking::rank_reduce(&files);
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&ranked);
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let mut sorted_sa = sa.clone();
    sorted_sa.sort_unstable();
    let expected: Vec<usize> = (0..text.len()).collect();
    assert_eq!(sorted_sa, expected);

    assert_eq!(lcp_arr[0], 0);

    let groups =
        extract_clone_groups_for_test(&sa, &lcp_arr, &file_of, &file_offsets, 3, &files, None);
    assert!(
        !groups.is_empty(),
        "Should find clone group for shared [10,20,30]"
    );

    let has_len_3 = groups.iter().any(|g| g.length == 3);
    assert!(has_len_3, "Should have a group of length 3");
}

#[test]
fn pipeline_no_false_positives_with_different_files() {
    let files = vec![
        FileData {
            path: PathBuf::from("a.ts"),
            hashed_tokens: make_hashed_tokens(&[1, 2, 3, 4, 5]),
            file_tokens: make_file_tokens("a\nb\nc\nd\ne", 5),
            atomic_invocation_spans: Vec::new(),
        },
        FileData {
            path: PathBuf::from("b.ts"),
            hashed_tokens: make_hashed_tokens(&[6, 7, 8, 9, 10]),
            file_tokens: make_file_tokens("f\ng\nh\ni\nj", 5),
            atomic_invocation_spans: Vec::new(),
        },
    ];

    let ranked = ranking::rank_reduce(&files);
    let (text, file_of, file_offsets) = concatenation::concatenate_with_sentinels(&ranked);
    let sa = suffix_array::build_suffix_array(&text);
    let lcp_arr = lcp::build_lcp(&text, &sa);

    let groups =
        extract_clone_groups_for_test(&sa, &lcp_arr, &file_of, &file_offsets, 2, &files, None);
    assert!(
        groups.is_empty(),
        "Completely different files should produce no clone groups"
    );
}

#[test]
fn min_tokens_zero_returns_empty() {
    let detector = CloneDetector::new(0, 1, false);
    let hashes = vec![10, 20, 30];
    let report = detector.detect(vec![(
        PathBuf::from("a.ts"),
        make_hashed_tokens(&hashes),
        make_file_tokens("abc", 3),
    )]);
    assert!(report.clone_groups.is_empty());
}

#[test]
fn detector_stats_are_consistent() {
    let detector = CloneDetector::new(3, 1, false);
    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";

    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
    ]);

    let stats = &report.stats;
    assert_eq!(stats.total_files, 2);
    assert_eq!(stats.total_lines, 10); // 5 lines per file
    assert_eq!(stats.total_tokens, 10); // 5 tokens per file
    assert_eq!(stats.clone_groups, report.clone_groups.len());
    assert!(stats.duplication_percentage >= 0.0);
    assert!(stats.duplication_percentage <= 100.0);
    assert!(stats.duplicated_tokens <= stats.total_tokens);
    assert!(stats.duplicated_lines <= stats.total_lines);
    assert!(stats.files_with_clones <= stats.total_files);
}

#[test]
fn detector_groups_sorted_by_token_count_desc() {
    let detector = CloneDetector::new(3, 1, false);

    let hashes_a: Vec<u64> = vec![10, 20, 30, 99, 40, 50, 60, 70];
    let hashes_b: Vec<u64> = vec![10, 20, 30, 88, 40, 50, 60, 70];
    let source = "a\nb\nc\nd\ne\nf\ng\nh";

    let report = detector.detect(vec![
        (
            PathBuf::from("dir_a/a.ts"),
            make_hashed_tokens(&hashes_a),
            make_file_tokens(source, 8),
        ),
        (
            PathBuf::from("dir_b/b.ts"),
            make_hashed_tokens(&hashes_b),
            make_file_tokens(source, 8),
        ),
    ]);

    for w in report.clone_groups.windows(2) {
        assert!(
            w[0].token_count >= w[1].token_count,
            "Groups should be sorted by token_count desc: {} < {}",
            w[0].token_count,
            w[1].token_count
        );
    }
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Suffix array is always a permutation of 0..n.
        #[test]
        fn suffix_array_is_permutation(values in prop::collection::vec(-100i64..100i64, 1..100)) {
            let sa = suffix_array::build_suffix_array(&values);
            let n = values.len();
            prop_assert_eq!(sa.len(), n, "SA length should equal input length");
            let mut sorted = sa;
            sorted.sort_unstable();
            let expected: Vec<usize> = (0..n).collect();
            prop_assert_eq!(sorted, expected, "SA should be a permutation of 0..n");
        }

        /// Suffix array of empty input is empty.
        #[test]
        fn suffix_array_empty_input(_unused in Just(())) {
            let sa = suffix_array::build_suffix_array(&[]);
            prop_assert!(sa.is_empty());
        }

        /// LCP values are always non-negative (they are usize, so >= 0).
        /// Also: LCP array has the same length as the suffix array.
        #[test]
        fn lcp_values_same_length_as_sa(values in prop::collection::vec(0i64..50i64, 1..80)) {
            let sa = suffix_array::build_suffix_array(&values);
            let lcp_arr = lcp::build_lcp(&values, &sa);
            prop_assert_eq!(lcp_arr.len(), sa.len(), "LCP array should have same length as SA");
            if !lcp_arr.is_empty() {
                prop_assert_eq!(lcp_arr[0], 0, "LCP[0] should always be 0");
            }
        }

        /// LCP values should never exceed the remaining text length.
        #[test]
        fn lcp_values_bounded_by_text_length(values in prop::collection::vec(0i64..20i64, 1..60)) {
            let n = values.len();
            let sa = suffix_array::build_suffix_array(&values);
            let lcp_arr = lcp::build_lcp(&values, &sa);
            for (i, &lcp_val) in lcp_arr.iter().enumerate() {
                if i > 0 {
                    let remaining_curr = n - sa[i];
                    let remaining_prev = n - sa[i - 1];
                    let max_possible = remaining_curr.min(remaining_prev);
                    prop_assert!(
                        lcp_val <= max_possible,
                        "LCP[{}]={} exceeds max possible {} (suffixes at {} and {})",
                        i, lcp_val, max_possible, sa[i], sa[i - 1]
                    );
                }
            }
        }

        /// Detected clones always have >= min_tokens tokens.
        #[test]
        fn clones_respect_min_tokens(
            min_tokens in 3..15usize,
            hash_values in prop::collection::vec(1u64..20u64, 5..30),
        ) {
            let detector = CloneDetector::new(min_tokens, 1, false);
            let source_a = (0..hash_values.len()).fold(String::new(), |mut acc, i| {
                use std::fmt::Write;
                let _ = writeln!(acc, "t{i}");
                acc
            });
            let source_b = source_a.clone();

            let hashed_a = make_hashed_tokens(&hash_values);
            let hashed_b = make_hashed_tokens(&hash_values);
            let ft_a = make_file_tokens(&source_a, hash_values.len());
            let ft_b = make_file_tokens(&source_b, hash_values.len());

            let report = detector.detect(vec![
                (PathBuf::from("dir_a/a.ts"), hashed_a, ft_a),
                (PathBuf::from("dir_b/b.ts"), hashed_b, ft_b),
            ]);

            for group in &report.clone_groups {
                prop_assert!(
                    group.token_count >= min_tokens,
                    "Clone group has {} tokens, but min is {}",
                    group.token_count, min_tokens
                );
            }
        }

        /// Clone groups should always have at least 2 instances.
        #[test]
        fn clone_groups_have_at_least_two_instances(
            hash_values in prop::collection::vec(1u64..10u64, 5..20),
        ) {
            let detector = CloneDetector::new(3, 1, false);
            let source = (0..hash_values.len()).fold(String::new(), |mut acc, i| {
                use std::fmt::Write;
                let _ = writeln!(acc, "t{i}");
                acc
            });

            let hashed_a = make_hashed_tokens(&hash_values);
            let hashed_b = make_hashed_tokens(&hash_values);
            let ft_a = make_file_tokens(&source, hash_values.len());
            let ft_b = make_file_tokens(&source, hash_values.len());

            let report = detector.detect(vec![
                (PathBuf::from("dir_a/a.ts"), hashed_a, ft_a),
                (PathBuf::from("dir_b/b.ts"), hashed_b, ft_b),
            ]);

            for group in &report.clone_groups {
                prop_assert!(
                    group.instances.len() >= 2,
                    "Clone group should have at least 2 instances, got {}",
                    group.instances.len()
                );
            }
        }
    }
}

#[test]
fn all_files_empty_tokens_returns_empty_report() {
    let detector = CloneDetector::new(3, 1, false);
    let ft_a = make_file_tokens("", 0);
    let ft_b = make_file_tokens("", 0);
    let report = detector.detect(vec![
        (PathBuf::from("a.ts"), vec![], ft_a),
        (PathBuf::from("b.ts"), vec![], ft_b),
    ]);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 2);
    assert_eq!(report.stats.total_tokens, 0);
    assert_eq!(report.stats.total_lines, 2);
}

#[test]
fn single_empty_token_file_returns_empty_report() {
    let detector = CloneDetector::new(3, 1, false);
    let ft = make_file_tokens("", 0);
    let report = detector.detect(vec![(PathBuf::from("a.ts"), vec![], ft)]);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 1);
}

#[test]
fn mixed_empty_and_nonempty_files() {
    let detector = CloneDetector::new(3, 1, false);
    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";
    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
        (PathBuf::from("b.ts"), vec![], make_file_tokens("", 0)),
    ]);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 2);
    assert_eq!(report.stats.total_tokens, 5);
    assert_eq!(report.stats.total_lines, 6);
}

#[test]
fn min_lines_filters_short_clones() {
    let detector = CloneDetector::new(3, 10, false);
    let hashes = vec![10, 20, 30];
    let source = "aa\nbb\ncc";
    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 3),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 3),
        ),
    ]);
    assert!(
        report.clone_groups.is_empty(),
        "Clones spanning fewer lines than min_lines should be filtered"
    );
}

#[test]
fn min_lines_allows_long_enough_clones() {
    let detector = CloneDetector::new(3, 3, false);
    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";
    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
    ]);
    assert!(
        !report.clone_groups.is_empty(),
        "Clones meeting min_lines should be retained"
    );
}

#[test]
fn many_files_with_shared_prefix() {
    let detector = CloneDetector::new(3, 1, false);
    let data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)> = (0..5)
        .map(|i| {
            let mut hashes: Vec<u64> = vec![10, 20, 30, 40];
            hashes.push(100 + i); // unique suffix per file
            let source = "a\nb\nc\nd\ne";
            (
                PathBuf::from(format!("dir{i}/file.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens(source, 5),
            )
        })
        .collect();

    let report = detector.detect(data);
    assert!(!report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 5);
    assert_eq!(report.stats.total_tokens, 25);
    assert_eq!(report.stats.total_lines, 25);
    let max_tokens = report
        .clone_groups
        .iter()
        .map(|g| g.token_count)
        .max()
        .unwrap_or(0);
    assert!(max_tokens >= 4);
}

#[test]
fn three_empty_files_early_return() {
    let detector = CloneDetector::new(5, 1, false);
    let data: Vec<(PathBuf, Vec<HashedToken>, FileTokens)> = (0..3)
        .map(|i| {
            (
                PathBuf::from(format!("f{i}.ts")),
                vec![],
                make_file_tokens("", 0),
            )
        })
        .collect();
    let report = detector.detect(data);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 3);
    assert_eq!(report.stats.total_tokens, 0);
    assert_eq!(report.stats.total_lines, 3);
    assert!((report.stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
}

#[test]
fn skip_local_with_root_level_files() {
    let detector = CloneDetector::new(3, 1, true);
    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";
    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
    ]);
    assert!(
        report.clone_groups.is_empty(),
        "Root-level files with skip_local should be filtered (same implicit directory)"
    );
}

#[test]
fn partial_overlap_between_two_files() {
    let detector = CloneDetector::new(3, 1, false);
    let hashes_a: Vec<u64> = vec![1, 2, 10, 20, 30, 40, 7, 8];
    let hashes_b: Vec<u64> = vec![3, 4, 10, 20, 30, 40, 9, 11];
    let source = "a\nb\nc\nd\ne\nf\ng\nh";
    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes_a),
            make_file_tokens(source, 8),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes_b),
            make_file_tokens(source, 8),
        ),
    ]);
    assert!(!report.clone_groups.is_empty());
    let has_shared = report.clone_groups.iter().any(|g| g.token_count >= 4);
    assert!(has_shared, "Should detect the shared [10,20,30,40] block");
}

#[test]
fn report_clone_families_and_mirrored_directories_empty() {
    let detector = CloneDetector::new(3, 1, false);
    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";
    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
    ]);
    assert!(report.clone_families.is_empty());
    assert!(report.mirrored_directories.is_empty());
}

#[test]
fn large_min_tokens_no_clones() {
    let detector = CloneDetector::new(100, 1, false);
    let hashes = vec![10, 20, 30];
    let source = "a\nb\nc";
    let report = detector.detect(vec![
        (
            PathBuf::from("a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 3),
        ),
        (
            PathBuf::from("b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 3),
        ),
    ]);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 2);
    assert_eq!(report.stats.total_tokens, 6);
}

#[test]
fn unique_ranks_computation_single_file() {
    let detector = CloneDetector::new(3, 1, false);
    let hashes = vec![10, 20, 30];
    let source = "a\nb\nc";
    let report = detector.detect(vec![(
        PathBuf::from("a.ts"),
        make_hashed_tokens(&hashes),
        make_file_tokens(source, 3),
    )]);
    assert!(report.clone_groups.is_empty());
    assert_eq!(report.stats.total_files, 1);
}

#[test]
fn skip_local_false_keeps_same_directory() {
    let detector = CloneDetector::new(3, 1, false);
    let hashes = vec![10, 20, 30, 40, 50];
    let source = "a\nb\nc\nd\ne";
    let report = detector.detect(vec![
        (
            PathBuf::from("src/a.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
        (
            PathBuf::from("src/b.ts"),
            make_hashed_tokens(&hashes),
            make_file_tokens(source, 5),
        ),
    ]);
    assert!(
        !report.clone_groups.is_empty(),
        "Same-directory clones should be kept when skip_local is false"
    );
}

#[test]
fn stats_duplication_percentage_within_bounds() {
    for min_tokens in [1, 3, 5] {
        let detector = CloneDetector::new(min_tokens, 1, false);
        let hashes: Vec<u64> = (1..=10).collect();
        let source = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj";
        let report = detector.detect(vec![
            (
                PathBuf::from("dir_a/a.ts"),
                make_hashed_tokens(&hashes),
                make_file_tokens(source, 10),
            ),
            (
                PathBuf::from("dir_b/b.ts"),
                make_hashed_tokens(&hashes),
                make_file_tokens(source, 10),
            ),
        ]);
        assert!(report.stats.duplication_percentage >= 0.0);
        assert!(report.stats.duplication_percentage <= 100.0);
    }
}
