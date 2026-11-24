//! Compare health counts with the path, function and line keys of an older save.

use super::{
    BTreeMap, HEALTH_FILE_COUNTS, HEALTH_IDENTITY_COUNTS, Map, Value, bucket_count, display_key,
    has_entries,
};

pub(super) struct Migration<'a> {
    keys: &'a [Value],
    category: &'static str,
    buckets: &'a Map<String, Value>,
    separate_file_lines: bool,
}

impl<'a> Migration<'a> {
    pub(super) fn from_maps(
        base: &'a Map<String, Value>,
        head: &'a Map<String, Value>,
    ) -> Option<Self> {
        let keys = base.get("findings")?.as_array()?;
        if keys.is_empty()
            || has_entries(base, HEALTH_FILE_COUNTS)
            || has_entries(base, HEALTH_IDENTITY_COUNTS)
        {
            return None;
        }
        let category = if has_entries(head, HEALTH_IDENTITY_COUNTS) {
            HEALTH_IDENTITY_COUNTS
        } else {
            HEALTH_FILE_COUNTS
        };
        let buckets = head.get(category)?.as_object()?;
        if buckets.is_empty() {
            return None;
        }

        Some(Self {
            keys,
            category,
            buckets,
            separate_file_lines: head
                .get(super::HEALTH_FILE_LINES_MARKER)
                .and_then(Value::as_bool)
                == Some(true),
        })
    }

    pub(super) fn compare(self, grown: &mut BTreeMap<String, Vec<String>>) {
        let Self {
            keys,
            category,
            buckets,
            separate_file_lines,
        } = self;
        let mut known = BTreeMap::new();
        for key in keys.iter().filter_map(Value::as_str) {
            if let Some((path_name, line)) = key.rsplit_once(':')
                && line.parse::<u32>().is_ok()
            {
                *known.entry(path_name).or_default() += 1;
            }
        }
        // A legacy path and name share a colon separator. Match the most specific
        // path first when two candidate paths share a prefix.
        let ordered: BTreeMap<_, _> = buckets.iter().collect();
        let mut added = Vec::new();
        for (bucket, counts) in ordered.into_iter().rev() {
            let Some(counts) = counts.as_object() else {
                continue;
            };
            let minimum = minimum_distinct_findings(counts);
            let uncovered = if category == HEALTH_IDENTITY_COUNTS {
                consume(known.get_mut(display_key(bucket).as_str()), minimum)
            } else {
                match_file_counts(&mut known, bucket, counts, minimum, separate_file_lines)
            };
            if uncovered > 0 {
                added.push(format!(
                    "{} (at least {minimum} needed, {} covered by old keys)",
                    display_key(bucket),
                    minimum - uncovered
                ));
            }
        }
        if !added.is_empty() {
            added.reverse();
            grown.insert(category.to_owned(), added);
        }
    }
}

/// A legacy file-length finding has the reserved function name `<file>`.
/// Other names can cover only function counts in a save with separate
/// file-length categories.
fn match_file_counts(
    known: &mut BTreeMap<&str, u64>,
    path: &str,
    counts: &Map<String, Value>,
    minimum: u64,
    separate_file_lines: bool,
) -> u64 {
    let prefix = format!("{path}:");
    let file_key = format!("{prefix}{}", fallow_output::FILE_LINES_FINDING_NAME);
    let file_lines = category_total(counts, super::HEALTH_FILE_LINES_PREFIX);
    let missing_files = consume(known.get_mut(file_key.as_str()), file_lines);
    let mut missing_functions = minimum - file_lines;
    if !separate_file_lines {
        // Older counts put file length under complexity. CRAP always needs a function.
        let possible_files = missing_functions.saturating_sub(category_total(counts, "crap_"));
        let unmatched = consume(known.get_mut(file_key.as_str()), possible_files);
        missing_functions -= possible_files - unmatched;
    }
    for (key, available) in known
        .range_mut::<str, _>((
            std::ops::Bound::Included(prefix.as_str()),
            std::ops::Bound::Unbounded,
        ))
        .take_while(|(key, _)| key.starts_with(&prefix))
    {
        if missing_functions == 0 {
            break;
        }
        if *key != file_key {
            missing_functions = consume(Some(available), missing_functions);
        }
    }
    missing_files + missing_functions
}

fn category_total(counts: &Map<String, Value>, prefix: &str) -> u64 {
    counts
        .iter()
        .filter(|(category, _)| category.starts_with(prefix))
        .fold(0u64, |sum, (_, bucket)| {
            sum.saturating_add(bucket_count(Some(bucket)))
        })
}

/// Return the part of the demand the old key could not cover.
fn consume(available: Option<&mut u64>, needed: u64) -> u64 {
    let Some(available) = available else {
        return needed;
    };
    let matched = needed.min(*available);
    *available -= matched;
    needed - matched
}

/// A function can exceed both complexity and CRAP at the same severity.
/// Those counts can overlap. File-length findings each need a separate row.
fn minimum_distinct_findings(counts: &Map<String, Value>) -> u64 {
    counts.iter().fold(0u64, |sum, (category, bucket)| {
        let count = bucket_count(Some(bucket));
        let overlap = category.strip_prefix("crap_").map_or(0, |severity| {
            bucket_count(counts.get(&format!("complexity_{severity}")))
        });
        sum.saturating_add(count.saturating_sub(overlap))
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test fixtures contain JSON objects")]

    use super::*;
    use serde_json::json;

    #[test]
    fn count_migration_keeps_file_length_and_function_allowances_separate() {
        for (old, category, separate, grows) in [
            ("parse", "file_lines_high", true, true),
            ("<file>", "complexity_high", true, true),
            ("<file>", "complexity_high", false, false),
            ("<file>", "crap_high", false, true),
            ("<file>", "file_lines_high", true, false),
        ] {
            let base = json!({ "findings": [format!("src/a.ts:{old}:1")] });
            let head = json!({ "file_lines_categories": separate, "finding_counts": {
                "src/a.ts": {category: {"count": 1}}
            }});
            let mut grown = BTreeMap::new();
            let head = head.as_object().unwrap();
            Migration::from_maps(base.as_object().unwrap(), head)
                .unwrap()
                .compare(&mut grown);
            assert_eq!(
                grown.contains_key(HEALTH_FILE_COUNTS),
                grows,
                "{old} / {category}"
            );
        }
    }

    #[test]
    fn a_legacy_key_with_a_colon_cannot_pay_for_two_files() {
        let base = json!({"findings": ["src/a.ts:b.ts:run:1"]});
        let head = json!({"finding_counts": {
            "src/a.ts": {"complexity_high": {"count": 1}},
            "src/a.ts:b.ts": {"complexity_high": {"count": 1}}
        }});
        let mut grown = BTreeMap::new();
        let head = head.as_object().unwrap();
        Migration::from_maps(base.as_object().unwrap(), head)
            .unwrap()
            .compare(&mut grown);
        assert_eq!(grown[HEALTH_FILE_COUNTS].len(), 1);
    }
}
