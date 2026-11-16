#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

//! Clone detection with `normalization.consistentIdentifiers` and template
//! literal text (issue #4626).

use fallow_config::DuplicatesConfig;
use fallow_engine::discover::{DiscoveredFile, FileId};
use fallow_engine::duplicates::DuplicationReport;

const BASE: &str = "export function computeInvoice(items, rate) {
  let total = 0;
  for (const item of items) {
    const price = item.amount * item.quantity;
    const tax = price * rate;
    total = total + price + tax;
  }
  const label = `Invoice total: ${total}`;
  console.log(label, items.length, rate);
  return total;
}
";

/// The settings of the issue: renamed copies are clones, literals are compared.
/// `BASE` has more than 55 tokens, and each change below splits it into parts
/// of fewer than 55 tokens.
fn jscpd_like_config(consistent: bool) -> DuplicatesConfig {
    serde_json::from_value(serde_json::json!({
        "minTokens": 55,
        "minLines": 3,
        "normalization": {
            "ignoreIdentifiers": true,
            "consistentIdentifiers": consistent,
        },
    }))
    .expect("valid duplicates config")
}

fn detect(sources: &[&str], config: &DuplicatesConfig) -> DuplicationReport {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("create src");
    let files: Vec<DiscoveredFile> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            let path = src.join(format!("file{index}.js"));
            std::fs::write(&path, source).expect("write source");
            DiscoveredFile {
                id: FileId(u32::try_from(index).unwrap()),
                path,
                size_bytes: source.len() as u64,
            }
        })
        .collect();
    fallow_engine::duplicates::find_duplicates(dir.path(), &files, config)
}

fn rename_consistently(source: &str) -> String {
    source
        .replace("computeInvoice", "buildBill")
        .replace("items", "rows")
        .replace("rate", "ratio")
        .replace("total", "sum")
        .replace("item.", "row.")
        .replace("const item ", "const row ")
        .replace("price", "cost")
        .replace("tax", "fee")
        .replace("label", "text")
}

#[test]
fn consistent_identifiers_keeps_exact_copy() {
    let report = detect(&[BASE, BASE], &jscpd_like_config(true));
    assert_eq!(report.clone_groups.len(), 1, "{:#?}", report.clone_groups);
}

#[test]
fn consistent_identifiers_keeps_consistent_rename() {
    let renamed = rename_consistently(BASE);
    assert_ne!(renamed, BASE);
    let report = detect(&[BASE, &renamed], &jscpd_like_config(true));
    assert_eq!(report.clone_groups.len(), 1, "{:#?}", report.clone_groups);
    assert_eq!(report.clone_groups[0].instances[0].start_line, 1);
}

#[test]
fn consistent_identifiers_rejects_literal_only_change() {
    let changed = BASE
        .replace("let total = 0;", "let total = 100;")
        .replace("`Invoice total: ${total}`", "`Order sum: ${total}`");
    let report = detect(&[BASE, &changed], &jscpd_like_config(true));
    assert!(
        report.clone_groups.is_empty(),
        "literal-only change must not be a clone: {:#?}",
        report.clone_groups
    );
}

#[test]
fn consistent_identifiers_rejects_inconsistent_rename() {
    // `tax` becomes `total`, a name that already stands for `total`.
    let changed = BASE.replace("total + price + tax", "total + price + total");
    let blind = detect(&[BASE, &changed], &jscpd_like_config(false));
    assert_eq!(
        blind.clone_groups.len(),
        1,
        "without the check, blind identifiers match: {:#?}",
        blind.clone_groups
    );
    let report = detect(&[BASE, &changed], &jscpd_like_config(true));
    assert!(
        report.clone_groups.is_empty(),
        "inconsistent rename must not be a clone: {:#?}",
        report.clone_groups
    );
}

#[test]
fn consistent_identifiers_splits_clone_at_mapping_conflict() {
    // Two long consistent halves around one inconsistent token.
    let first_half = "export function first(alpha, beta) {
  const gamma = alpha + beta;
  const delta = gamma * alpha - beta;
  const epsilon = delta / gamma + alpha;
  return [alpha, beta, gamma, delta, epsilon];
}
";
    let second_half = "export function second(one, two) {
  const three = one + two;
  const four = three * one - two;
  const five = four / three + one;
  return [one, two, three, four, five];
}
";
    let middle = "export const pivot = (left, right) => left + right;\n";
    let conflicted = "export const pivot = (left, right) => left + left;\n";
    let a = format!("{first_half}{middle}{second_half}");
    let b = format!("{first_half}{conflicted}{second_half}");
    let config: DuplicatesConfig = serde_json::from_value(serde_json::json!({
        "minTokens": 25,
        "minLines": 3,
        "normalization": { "ignoreIdentifiers": true, "consistentIdentifiers": true },
    }))
    .unwrap();
    let report = detect(&[&a, &b], &config);
    assert!(
        !report.clone_groups.is_empty(),
        "consistent parts must stay clones"
    );
    for group in &report.clone_groups {
        for instance in &group.instances {
            assert!(
                !instance.fragment.contains("left + left"),
                "a clone must not cross the mapping conflict: {}",
                instance.fragment
            );
        }
    }
    let lines: Vec<(usize, usize)> = report
        .clone_groups
        .iter()
        .map(|group| (group.instances[0].start_line, group.instances[0].end_line))
        .collect();
    assert!(
        lines.iter().any(|&(start, _)| start == 1),
        "the first half must be a clone: {lines:?}"
    );
    assert!(
        lines.iter().any(|&(_, end)| end >= 12),
        "the second half must be a clone: {lines:?}"
    );
}

#[test]
fn consistent_identifiers_drops_only_the_inconsistent_instance() {
    let inconsistent = BASE.replace("total + price + tax", "total + price + total");
    let renamed = rename_consistently(BASE);
    let report = detect(&[BASE, &renamed, &inconsistent], &jscpd_like_config(true));
    assert!(
        report
            .clone_groups
            .iter()
            .any(|group| group.instances.len() == 2
                && group
                    .instances
                    .iter()
                    .all(|i| !i.file.ends_with("file2.js"))),
        "the consistent pair must stay a full clone: {:#?}",
        report.clone_groups
    );
}

#[test]
fn template_literal_text_is_compared() {
    let changed = BASE.replace("`Invoice total: ${total}`", "`Order sum: ${total}`");
    let config: DuplicatesConfig = serde_json::from_value(serde_json::json!({
        "minTokens": 60,
        "minLines": 3,
    }))
    .unwrap();
    let report = detect(&[BASE, &changed], &config);
    assert!(
        report.clone_groups.is_empty(),
        "different template text must not match: {:#?}",
        report.clone_groups
    );
    let exact = detect(&[BASE, BASE], &config);
    assert_eq!(exact.clone_groups.len(), 1);
}

#[test]
fn template_literal_text_is_blind_with_ignore_string_values() {
    let changed = BASE.replace("`Invoice total: ${total}`", "`Order sum: ${total}`");
    let config: DuplicatesConfig = serde_json::from_value(serde_json::json!({
        "minTokens": 60,
        "minLines": 3,
        "normalization": { "ignoreStringValues": true },
    }))
    .unwrap();
    let report = detect(&[BASE, &changed], &config);
    assert_eq!(report.clone_groups.len(), 1, "{:#?}", report.clone_groups);
}

/// Two calls of 30 arguments each: `head(arg, ...); tail(arg, ...);`.
fn two_calls(head: &str, tail: &str, args: &[&str]) -> String {
    let list = |offset: usize| -> String {
        (0..30)
            .map(|index| args[(index + offset) % args.len()])
            .collect::<Vec<_>>()
            .join(",\n  ")
    };
    format!("{head}(\n  {});\n{tail}(\n  {});\n", list(0), list(1))
}

fn calls_config() -> DuplicatesConfig {
    serde_json::from_value(serde_json::json!({
        "minTokens": 100,
        "minLines": 1,
        "normalization": { "ignoreIdentifiers": true, "consistentIdentifiers": true },
    }))
    .unwrap()
}

fn longest_clone_tokens(report: &DuplicationReport, instances: usize) -> usize {
    report
        .clone_groups
        .iter()
        .filter(|group| group.instances.len() == instances)
        .map(|group| group.token_count)
        .max()
        .unwrap_or(0)
}

#[test]
fn consistent_identifiers_finds_the_longest_window_after_a_conflict() {
    // `a` maps to `x`, then to `z`. Without the first `a`, the rest maps
    // one-to-one and is longer than `minTokens`.
    let first = two_calls("a", "a", &["b"]);
    let second = two_calls("x", "z", &["y"]);
    let report = detect(&[&first, &second], &calls_config());
    assert!(
        longest_clone_tokens(&report, 2) >= 100,
        "the window after the first conflicting name must stay a clone: {:#?}",
        report.clone_groups
    );
}

#[test]
fn consistent_identifiers_keeps_a_pair_when_a_third_instance_breaks_often() {
    let first = two_calls("a", "a", &["b"]);
    let second = two_calls("x", "z", &["y"]);
    let third = two_calls("p", "p", &["q", "r", "q", "s"]);
    let report = detect(&[&first, &second, &third], &calls_config());
    let pair = report.clone_groups.iter().find(|group| {
        group.instances.len() == 2
            && group
                .instances
                .iter()
                .all(|instance| !instance.file.ends_with("file2.js"))
    });
    assert!(
        pair.is_some_and(|group| group.token_count >= 100),
        "the first two instances must keep their shared clone: {:#?}",
        report.clone_groups
    );
}

#[test]
fn template_literal_escapes_compare_as_in_strings() {
    let escaped = BASE.replace("`Invoice total: ${total}`", "`Invoice \\x41: ${total}`");
    let plain = BASE.replace("`Invoice total: ${total}`", "`Invoice A: ${total}`");
    let config: DuplicatesConfig = serde_json::from_value(serde_json::json!({
        "minTokens": 60,
        "minLines": 3,
    }))
    .unwrap();
    let report = detect(&[&escaped, &plain], &config);
    assert_eq!(report.clone_groups.len(), 1, "{:#?}", report.clone_groups);
}
