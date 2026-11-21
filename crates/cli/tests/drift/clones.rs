//! Clone-group helpers of the I4 expected split: the text, the size and the
//! added lines of the instances in a clone-group symbol.

use std::collections::BTreeMap;

use crate::keys;

/// The default `duplicates.minLines`: the fewest lines of a clone.
const MIN_CLONE_LINES: u64 = 5;

/// Whether each instance of a clone-group symbol holds fewer added lines in a
/// row than a clone has. Such an edit cannot write a new clone alone, so the
/// audit grades the group of an old clone `inherited-edited`.
pub fn clone_edit_is_small(symbol: &str, added: &BTreeMap<String, Vec<(u64, u64)>>) -> bool {
    keys::clone_instances(symbol).all(|instance| {
        let Some(instance) = instance else {
            return false;
        };
        added.get(instance.path).is_none_or(|hunks| {
            hunks.iter().all(|&(first, last)| {
                let overlap_start = first.max(instance.start_line);
                let overlap_end = last.min(instance.end_line);
                overlap_end < overlap_start || overlap_end - overlap_start + 1 < MIN_CLONE_LINES
            })
        })
    })
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
