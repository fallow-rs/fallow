//! Clone-group helpers of the I4 expected split: the text, the size, the
//! spans and the added lines of the instances in a clone-group symbol, and
//! the base clone groups that the edit check of the audit compares with.

use std::collections::BTreeMap;

use fallow_api::diff_risk::clones::{BaseCloneGroup, CloneSpan};

use crate::keys::{self, FindingKey};

/// The `duplicates.minLines` of a case: the value in its `.fallowrc.json`,
/// else the default.
pub fn case_min_lines(files: &BTreeMap<String, String>) -> usize {
    files
        .get(".fallowrc.json")
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|config| config["duplicates"]["minLines"].as_u64())
        .and_then(|lines| usize::try_from(lines).ok())
        .unwrap_or_else(|| fallow_config::DuplicatesConfig::default().min_lines)
}

/// The root-relative spans of a clone-group symbol. `None` when an instance
/// does not parse.
pub fn clone_spans(symbol: &str) -> Option<Vec<CloneSpan>> {
    keys::clone_instances(symbol)
        .map(|instance| {
            let instance = instance?;
            Some(CloneSpan {
                path: instance.path.to_string(),
                start_line: u32::try_from(instance.start_line).ok()?,
                end_line: u32::try_from(instance.end_line).ok()?,
            })
        })
        .collect()
}

/// The clone groups of the base keys, at base paths.
pub fn base_clone_groups(base_keys: &[FindingKey]) -> Vec<BaseCloneGroup> {
    base_keys
        .iter()
        .filter(|key| key.kind == keys::DUPLICATION_KIND)
        .filter_map(|key| {
            Some(BaseCloneGroup {
                instances: clone_spans(&key.symbol)?,
            })
        })
        .collect()
}

/// Whether an instance of a clone-group symbol holds an added line of its
/// file.
pub fn clone_holds_added_line(symbol: &str, added: &BTreeMap<String, Vec<(u64, u64)>>) -> bool {
    keys::clone_instances(symbol).any(|instance| {
        let Some(instance) = instance else {
            return true;
        };
        added.get(instance.path).is_some_and(|hunks| {
            hunks
                .iter()
                .any(|&(first, last)| first <= instance.end_line && instance.start_line <= last)
        })
    })
}

/// The sorted source text of the instances in a clone-group symbol, read
/// from `contents`. The first line starts at the start column and the last
/// line ends at the end column, because a clone can start or end inside a
/// line; text outside the clone does not change its identity.
pub fn clone_texts(symbol: &str, contents: &BTreeMap<String, String>) -> String {
    let mut texts: Vec<String> = keys::clone_instances(symbol)
        .filter_map(|instance| {
            let instance = instance?;
            let start = usize::try_from(instance.start_line).ok()?.checked_sub(1)?;
            let end = usize::try_from(instance.end_line).ok()?;
            let lines: Vec<&str> = contents.get(instance.path)?.lines().collect();
            let mut span: Vec<String> = lines
                .get(start..end.min(lines.len()))?
                .iter()
                .map(|line| (*line).to_string())
                .collect();
            let start_col = usize::try_from(instance.start_col).ok()?;
            let end_col = usize::try_from(instance.end_col).ok()?;
            if let Some(last) = span.last_mut() {
                *last = last.get(..end_col)?.to_string();
            }
            if let Some(first) = span.first_mut() {
                *first = first.get(start_col..)?.to_string();
            }
            Some(span.join("\n"))
        })
        .collect();
    texts.sort_unstable();
    texts.join("\u{1f}")
}

/// The sorted line counts of the instances in a clone-group symbol, without
/// the paths and positions.
pub fn clone_line_counts(symbol: &str) -> String {
    let mut counts: Vec<u64> = keys::clone_instances(symbol)
        .filter_map(|instance| {
            let instance = instance?;
            Some(instance.end_line + 1 - instance.start_line)
        })
        .collect();
    counts.sort_unstable();
    counts
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",")
}
