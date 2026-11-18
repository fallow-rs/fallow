//! Behavior tests for `fallow-ignore-start` / `fallow-ignore-end` ranges.

use std::path::Path;

use super::*;
use crate::discover::FileId;
use fallow_types::suppress::RangeMarkerProblemKind;

const START: &str = "// fallow-ignore-start code-duplication -- kept on purpose";
const END: &str = "// fallow-ignore-end code-duplication";

/// The function body that every repro function shares. `limit` is the
/// literal in the `if`, so the functions of one file are not one clone.
fn function(name: &str, limit: u32) -> String {
    format!(
        "export function {name}(items: number[], factor: number): number {{\n  let total = 0;\n  for (const item of items) {{\n    if (item > {limit}) {{\n      total += item * factor;\n    }} else {{\n      total -= item;\n    }}\n  }}\n  const scaled = total / items.length;\n  console.log(\"result\", scaled, factor);\n  return Math.round(scaled * 100) / 100;\n}}\n"
    )
}

/// `b.ts` of the issue #4706 repro: the three functions with one `export
/// const` line between them, plus two tail lines.
fn repro_b() -> String {
    format!(
        "{}export const sepA = 1;\n{}export const sepB = 2;\n{}\nexport const lonely = 1; // tail\n// fallow-ignore-next-line code-duplication\nexport const alsoLonely = 2;\n",
        function("one", 1),
        function("two", 2),
        function("three", 3),
    )
}

fn write_files(root: &Path, files: &[(&str, &str)]) -> Vec<DiscoveredFile> {
    files
        .iter()
        .enumerate()
        .map(|(index, (name, content))| {
            let path = root.join(name);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create parent dir");
            }
            std::fs::write(&path, content).expect("write fixture file");
            DiscoveredFile {
                id: FileId(u32::try_from(index).expect("small index")),
                path,
                size_bytes: content.len() as u64,
            }
        })
        .collect()
}

/// Every group as a sorted list of `file:start-end` strings.
fn group_lines(root: &Path, report: &DuplicationReport) -> Vec<Vec<String>> {
    let mut groups: Vec<Vec<String>> = report
        .clone_groups
        .iter()
        .map(|group| {
            let mut instances: Vec<String> = group
                .instances
                .iter()
                .map(|instance| {
                    let file = instance.file.strip_prefix(root).unwrap_or(&instance.file);
                    format!(
                        "{}:{}-{}",
                        file.to_string_lossy().replace('\\', "/"),
                        instance.start_line,
                        instance.end_line
                    )
                })
                .collect();
            instances.sort();
            instances
        })
        .collect();
    groups.sort();
    groups
}

fn problems(report: &DuplicationReport) -> Vec<(u32, RangeMarkerProblemKind, Option<String>)> {
    report
        .range_marker_problems
        .iter()
        .map(|entry| {
            (
                entry.problem.line,
                entry.problem.kind,
                entry.problem.token.clone(),
            )
        })
        .collect()
}

fn default_report(root: &Path, files: &[DiscoveredFile]) -> DuplicationReport {
    find_duplicates(root, files, &DuplicatesConfig::default())
}

#[test]
fn issue_repro_range_cuts_neighbor_clone_at_the_border() {
    let dir = tempfile::tempdir().expect("temp dir");
    let a = format!(
        "{}{START}\n{}{END}\n{}",
        function("one", 1),
        function("two", 2),
        function("three", 3),
    );
    let b = repro_b();
    let files = write_files(dir.path(), &[("src/a.ts", &a), ("src/b.ts", &b)]);

    let report = default_report(dir.path(), &files);

    // Without the range: a.ts:1-14 / b.ts:1-14, a.ts:14-27 / b.ts:15-28,
    // a.ts:27-38 / b.ts:29-40, and the b.ts-only group. The range removes
    // the two() group and cuts the one() group before line 13.
    assert_eq!(
        group_lines(dir.path(), &report),
        vec![
            vec!["src/a.ts:1-12".to_string(), "src/b.ts:1-12".to_string()],
            vec!["src/a.ts:29-40".to_string(), "src/b.ts:29-40".to_string()],
            vec![
                "src/b.ts:18-28".to_string(),
                "src/b.ts:32-43".to_string(),
                "src/b.ts:4-14".to_string(),
            ],
        ]
    );
    assert!(problems(&report).is_empty());
}

#[test]
fn clone_completely_inside_a_range_is_gone() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let wrapped = format!("{START}\n{body}{END}\n");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &wrapped)]);

    let report = default_report(dir.path(), &files);

    assert!(
        report.clone_groups.is_empty(),
        "{:?}",
        group_lines(dir.path(), &report)
    );
}

#[test]
fn cut_part_below_min_tokens_is_not_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let lead = "export const lead = [1, 2, 3];\n";
    let body = function("shared", 1);
    let plain = format!("{lead}{body}");
    let wrapped = format!("{lead}{START}\n{body}{END}\n");
    let files = write_files(dir.path(), &[("a.ts", &plain), ("b.ts", &wrapped)]);

    let report = default_report(dir.path(), &files);

    assert!(
        report.clone_groups.is_empty(),
        "{:?}",
        group_lines(dir.path(), &report)
    );
}

#[test]
fn range_in_one_of_three_instances_keeps_the_other_two() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let wrapped = format!("{START}\n{body}{END}\n");
    let files = write_files(
        dir.path(),
        &[("a.ts", &body), ("b.ts", &body), ("c.ts", &wrapped)],
    );

    let report = default_report(dir.path(), &files);

    assert_eq!(
        group_lines(dir.path(), &report),
        vec![vec!["a.ts:1-12".to_string(), "b.ts:1-12".to_string()]]
    );
}

#[test]
fn range_in_sfc_template_with_html_comments() {
    let dir = tempfile::tempdir().expect("temp dir");
    let rows = (0..8).fold(String::new(), |mut rows, index| {
        use std::fmt::Write as _;
        let _ = writeln!(
            rows,
            "    <li class=\"row\" :data-index=\"{index}\"><span>{{{{ item.label }}}}</span><button @click=\"select(item)\">Pick</button></li>"
        );
        rows
    });
    let script = "<script setup lang=\"ts\">\nconst select = (item: unknown): void => {\n  console.log(item);\n};\n</script>\n";
    let plain = format!("<template>\n  <ul>\n{rows}  </ul>\n</template>\n{script}");
    let wrapped = format!(
        "<template>\n  <ul>\n    <!-- fallow-ignore-start code-duplication -- kept -->\n{rows}    <!-- fallow-ignore-end code-duplication -->\n  </ul>\n</template>\n{script}"
    );
    let config = DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        ..DuplicatesConfig::default()
    };

    let baseline = find_duplicates(
        dir.path(),
        &write_files(dir.path(), &[("A.vue", &plain), ("B.vue", &plain)]),
        &config,
    );
    assert!(
        !baseline.clone_groups.is_empty(),
        "the template rows form a clone without a range"
    );

    let files = write_files(dir.path(), &[("A.vue", &plain), ("B.vue", &wrapped)]);
    let report = find_duplicates(dir.path(), &files, &config);

    for instance in report
        .clone_groups
        .iter()
        .flat_map(|group| &group.instances)
    {
        assert!(
            !(instance.file.ends_with("B.vue")
                && instance.start_line <= 11
                && instance.end_line >= 4),
            "no B.vue instance may overlap the range (lines 4 to 11): {:?}",
            group_lines(dir.path(), &report)
        );
    }
    assert!(
        baseline
            .clone_groups
            .iter()
            .flat_map(|group| &group.instances)
            .any(|instance| {
                instance.file.ends_with("B.vue")
                    && instance.start_line <= 11
                    && instance.end_line >= 4
            }),
        "the baseline has a B.vue instance in the rows"
    );
    assert!(problems(&report).is_empty());
}

#[test]
fn marker_inside_template_literal_is_ignored() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let with_literal =
        format!("export const doc = `\n{START}\n`;\n{body}export const doc2 = `\n{END}\n`;\n");
    let plain =
        format!("export const doc = `\nnothing\n`;\n{body}export const doc2 = `\nnothing\n`;\n");
    let files = write_files(dir.path(), &[("a.ts", &plain), ("b.ts", &with_literal)]);

    let report = default_report(dir.path(), &files);

    assert!(
        !report.clone_groups.is_empty(),
        "markers in a template literal must not mask code"
    );
    assert!(problems(&report).is_empty(), "{:?}", problems(&report));
}

#[test]
fn file_marker_inside_template_literal_is_ignored() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let with_literal =
        format!("{body}export const docs = `\n// fallow-ignore-file code-duplication\n`;\n");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &with_literal)]);

    let report = default_report(dir.path(), &files);

    assert_eq!(
        report.clone_groups.len(),
        1,
        "a file marker in a template literal must not suppress the file"
    );
}

#[test]
fn next_line_marker_inside_template_literal_is_ignored() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let marker = "// fallow-ignore-next-line code-duplication";
    // The line after the marker is the first line of the shared function.
    let plain = format!("export const docs = `\nnothing\n`; {body}");
    let with_literal = format!("export const docs = `\n{marker}\n`; {body}");
    let files = write_files(dir.path(), &[("a.ts", &plain), ("b.ts", &with_literal)]);

    let report = default_report(dir.path(), &files);

    assert_eq!(
        report.clone_groups.len(),
        1,
        "a next-line marker in a template literal must not suppress the clone: {:?}",
        group_lines(dir.path(), &report)
    );
}

#[test]
fn real_file_marker_after_template_literal_still_suppresses() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let suppressed = format!(
        "export const docs = `\n// fallow-ignore-file code-duplication\n`;\n// fallow-ignore-file code-duplication\n{body}"
    );
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &suppressed)]);

    let report = default_report(dir.path(), &files);

    assert!(report.clone_groups.is_empty());
}

#[test]
fn unclosed_start_has_no_effect_and_is_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let unclosed = format!("{START}\n{body}");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &unclosed)]);

    let report = default_report(dir.path(), &files);

    assert_eq!(
        report.clone_groups.len(),
        1,
        "an unclosed start masks nothing"
    );
    assert_eq!(
        problems(&report),
        vec![(1, RangeMarkerProblemKind::UnclosedStart, None)]
    );
    assert!(report.range_marker_problems[0].path.ends_with("b.ts"));
}

#[test]
fn end_without_start_is_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let stray = format!("{body}{END}\n");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &stray)]);

    let report = default_report(dir.path(), &files);

    assert_eq!(report.clone_groups.len(), 1);
    assert_eq!(
        problems(&report),
        vec![(14, RangeMarkerProblemKind::UnmatchedEnd, None)]
    );
}

#[test]
fn nested_start_is_reported_and_outer_range_applies() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let nested = format!("{START}\n{START}\n{body}{END}\n");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &nested)]);

    let report = default_report(dir.path(), &files);

    assert!(report.clone_groups.is_empty());
    assert_eq!(
        problems(&report),
        vec![(2, RangeMarkerProblemKind::NestedStart, None)]
    );
}

#[test]
fn blanket_start_is_rejected() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let blanket = format!("// fallow-ignore-start -- everything\n{body}// fallow-ignore-end\n");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &blanket)]);

    let report = default_report(dir.path(), &files);

    assert_eq!(
        report.clone_groups.len(),
        1,
        "a blanket range masks nothing"
    );
    assert_eq!(
        problems(&report),
        vec![
            (1, RangeMarkerProblemKind::MissingKind, None),
            (15, RangeMarkerProblemKind::MissingKind, None),
        ]
    );
}

#[test]
fn mixed_kind_list_applies_dupes_and_reports_the_other_kind() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let mixed =
        format!("// fallow-ignore-start code-duplication, unused-export -- kept\n{body}{END}\n");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &mixed)]);

    let report = default_report(dir.path(), &files);

    assert!(report.clone_groups.is_empty());
    assert_eq!(
        problems(&report),
        vec![(
            1,
            RangeMarkerProblemKind::UnsupportedKind,
            Some("unused-export".to_string())
        )]
    );
}

#[test]
fn near_instances_that_overlap_a_range_are_dropped() {
    let dir = tempfile::tempdir().expect("temp dir");
    let shared_prefix = "export function calculate(input: number): number {\n  const doubled = input * 2;\n  const normalized = doubled + input;\n";
    let shared_suffix = "\n  const bounded = Math.max(adjusted, 0);\n  const rounded = Math.round(bounded);\n  const weighted = rounded * normalized;\n  const clamped = Math.min(weighted, 1000);\n  const staged = clamped + doubled;\n  const balanced = staged - input;\n  const projected = balanced * 2;\n  const limited = Math.min(projected, 2000);\n  const restored = limited + normalized;\n  const checked = Math.max(restored, doubled);\n  const combined = checked + bounded;\n  const smoothed = Math.round(combined / 2);\n  const finalized = smoothed + staged;\n  return finalized + normalized;\n}\n";
    let first = format!("{shared_prefix}  const adjusted = normalized + 2;{shared_suffix}");
    let second = format!(
        "{START}\n{shared_prefix}  const offset = normalized > 10 ? 3 : 2;\n  const adjusted = normalized + offset;{shared_suffix}{END}\n"
    );
    let config = DuplicatesConfig {
        near: true,
        min_tokens: 20,
        min_lines: 3,
        ..DuplicatesConfig::default()
    };

    let unwrapped = format!(
        "\n{shared_prefix}  const offset = normalized > 10 ? 3 : 2;\n  const adjusted = normalized + offset;{shared_suffix}\n"
    );
    let baseline_files = write_files(dir.path(), &[("a.ts", &first), ("b.ts", &unwrapped)]);
    let baseline = find_duplicates(dir.path(), &baseline_files, &config);
    assert!(
        baseline
            .clone_groups
            .iter()
            .any(|group| group.similarity.is_some()),
        "the fixture gives a near clone without a range"
    );

    let files = write_files(dir.path(), &[("a.ts", &first), ("b.ts", &second)]);
    let report = find_duplicates(dir.path(), &files, &config);

    assert!(
        report.clone_groups.is_empty(),
        "{:?}",
        group_lines(dir.path(), &report)
    );
}

#[test]
fn warm_token_cache_keeps_ranges_and_problems() {
    let dir = tempfile::tempdir().expect("temp dir");
    let cache = tempfile::tempdir().expect("cache dir");
    let body = function("shared", 1);
    let wrapped = format!("{START}\n{body}{END}\n{END}\n");
    let files = write_files(
        dir.path(),
        &[("a.ts", &body), ("b.ts", &body), ("c.ts", &wrapped)],
    );
    let config = DuplicatesConfig {
        min_corpus_size_for_token_cache: 0,
        ..DuplicatesConfig::default()
    };

    let cold = detect_duplicates(dir.path(), &files, &config, None, Some(cache.path())).report;
    let warm = detect_duplicates(dir.path(), &files, &config, None, Some(cache.path())).report;

    for report in [&cold, &warm] {
        assert_eq!(
            group_lines(dir.path(), report),
            vec![vec!["a.ts:1-12".to_string(), "b.ts:1-12".to_string()]]
        );
        assert_eq!(
            problems(report),
            vec![(16, RangeMarkerProblemKind::UnmatchedEnd, None)]
        );
    }
}

#[test]
fn trailing_range_marker_is_reported_on_its_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let trailing = format!("const zz = 1; // fallow-ignore-start code-duplication\n{body}{END}\n");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &trailing)]);

    let report = default_report(dir.path(), &files);

    assert_eq!(
        report.clone_groups.len(),
        1,
        "a trailing marker masks nothing"
    );
    assert_eq!(
        problems(&report),
        vec![
            (1, RangeMarkerProblemKind::NotOwnLine, None),
            (15, RangeMarkerProblemKind::UnmatchedEnd, None),
        ]
    );
}

#[test]
fn trailing_marker_text_in_a_string_is_not_reported() {
    let dir = tempfile::tempdir().expect("temp dir");
    let body = function("shared", 1);
    let quoted =
        format!("export const hint = \"x // fallow-ignore-start code-duplication\";\n{body}");
    let files = write_files(dir.path(), &[("a.ts", &body), ("b.ts", &quoted)]);

    let report = default_report(dir.path(), &files);

    assert!(problems(&report).is_empty(), "{:?}", problems(&report));
}
