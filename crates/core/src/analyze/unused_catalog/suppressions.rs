//! Recognize suppression comments using the same YAML parser as catalog discovery.

use std::fmt::Write;

use fallow_config::PnpmCatalogData;
use fallow_types::suppress::IssueKind;
use rustc_hash::FxHashSet;
use serde_yaml_ng::Value;

const MARKER_PREFIX: &str = "fallow_yaml_comment_";

pub(super) fn suppressed_yaml_catalog_entries(
    source: &str,
    data: &PnpmCatalogData,
) -> FxHashSet<u32> {
    let candidates = suppression_candidates(source, data);
    if candidates.is_empty() {
        return candidates;
    }
    let Ok(original) = serde_yaml_ng::from_str::<Value>(source) else {
        return FxHashSet::default();
    };
    let mut used_nonces = value_marker_ids(&original, MARKER_PREFIX);
    used_nonces.extend(marker_ids(source, MARKER_PREFIX));
    let nonce = (0..=used_nonces.len())
        .find(|nonce| !used_nonces.contains(nonce))
        .unwrap_or(used_nonces.len());
    let prefix = format!("{MARKER_PREFIX}{nonce}_");

    // Actual comments disappear during YAML parsing; marker text inside any
    // scalar survives. One instrumented parse handles every candidate without
    // per-directive reparsing or a separate YAML grammar.
    let marked = mark_candidates(source, &candidates, &prefix);
    let Ok(parsed) = serde_yaml_ng::from_str::<Value>(&marked) else {
        return FxHashSet::default();
    };
    let scalar_lines = value_marker_ids(&parsed, &prefix);
    candidates
        .into_iter()
        .filter(|line| !scalar_lines.contains(&(*line as usize)))
        .collect()
}

fn suppression_candidates(source: &str, data: &PnpmCatalogData) -> FxHashSet<u32> {
    let lines: Vec<&str> = source.lines().collect();
    data.catalogs
        .iter()
        .flat_map(|catalog| &catalog.entries)
        .filter_map(|entry| {
            let comment_index = (entry.line as usize).checked_sub(2)?;
            let comment = lines.get(comment_index)?.trim_start().strip_prefix('#')?;
            let parsed =
                fallow_extract::suppress::parse_suppressions_from_source(&format!("//{comment}"));
            parsed
                .suppressions
                .iter()
                .any(|suppression| {
                    suppression.line != 0
                        && suppression
                            .matches_issue_kind(suppression.line, IssueKind::PnpmCatalogEntry)
                })
                .then_some(entry.line)
        })
        .collect()
}

fn mark_candidates(source: &str, candidates: &FxHashSet<u32>, prefix: &str) -> String {
    let mut marked = String::with_capacity(source.len());
    for (index, line) in source.split_inclusive('\n').enumerate() {
        let entry_line = u32::try_from(index)
            .ok()
            .and_then(|line| line.checked_add(2));
        if let Some(entry_line) = entry_line
            && candidates.contains(&entry_line)
            && let Some(hash) = line.find('#')
        {
            marked.push_str(&line[..=hash]);
            let _ = write!(marked, "{prefix}{entry_line}_");
            marked.push_str(&line[hash + 1..]);
        } else {
            marked.push_str(line);
        }
    }
    marked
}

fn marker_ids<'a>(text: &'a str, prefix: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(prefix)
        .filter_map(move |(index, _)| text[index + prefix.len()..].split_once('_')?.0.parse().ok())
}

fn value_marker_ids(value: &Value, prefix: &str) -> FxHashSet<usize> {
    let mut ids = FxHashSet::default();
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::String(text) => ids.extend(marker_ids(text, prefix)),
            Value::Sequence(values) => pending.extend(values),
            Value::Mapping(mapping) => {
                pending.extend(mapping.iter().flat_map(<[&Value; 2]>::from));
            }
            Value::Tagged(tagged) => pending.push(&tagged.value),
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_can_be_more_indented_than_the_catalog_entry() {
        let source =
            "catalog:\n    # fallow-ignore-next-line unused-catalog-entry\n  target: ^1.0.0\n";
        let data = fallow_config::parse_pnpm_catalog_data(source).expect("valid YAML");
        assert_eq!(
            suppressed_yaml_catalog_entries(source, &data),
            FxHashSet::from_iter([3])
        );
    }

    #[test]
    fn multiline_quoted_scalar_text_does_not_suppress_the_next_entry() {
        for quote in ['\'', '"'] {
            let source = format!(
                "catalog:\n  scalar: {quote}hello\n  # fallow-ignore-next-line unused-catalog-entry -- reason{quote}\n  victim: ^1.0.0\n"
            );
            let data = fallow_config::parse_pnpm_catalog_data(&source).expect("valid YAML");
            assert!(suppressed_yaml_catalog_entries(&source, &data).is_empty());
        }
    }

    #[test]
    fn escaped_marker_text_cannot_collide_with_instrumented_comments() {
        let source = concat!(
            "metadata: \"\\x66allow_yaml_comment_0_4_\"\n",
            "catalog:\n",
            "  # fallow-ignore-next-line unused-catalog-entry\n",
            "  target: ^1.0.0\n",
        );
        let data = fallow_config::parse_pnpm_catalog_data(source).expect("valid YAML");
        assert_eq!(
            suppressed_yaml_catalog_entries(source, &data),
            FxHashSet::from_iter([4])
        );
    }

    #[test]
    fn marker_scan_covers_mapping_keys_sequences_and_tagged_values() {
        let value: Value = serde_yaml_ng::from_str(
            "fallow_yaml_comment_0_: [fallow_yaml_comment_1_, !custom fallow_yaml_comment_2_]\n",
        )
        .expect("valid YAML");
        assert_eq!(
            value_marker_ids(&value, MARKER_PREFIX),
            FxHashSet::from_iter([0, 1, 2])
        );
    }
}
