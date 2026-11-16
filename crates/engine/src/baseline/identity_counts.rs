//! Identity counts of baseline entries with placeholder allowance.

use super::*;

/// Baseline identity counts where a placeholder bucket (`<anonymous>`,
/// `<arrow>`) gives its unused allowance to new function names in the same
/// file.
///
/// A function without a declared name can get a context name in a later
/// version (`Command.make("setup")`, an `Effect.fn` span name). Its findings
/// then move from the placeholder bucket to a named bucket that has no
/// allowance for them. This transfer keeps such a baseline valid. The
/// allowance moves per category, only to named buckets in the same file whose
/// current count is above their own allowance, and only up to the count that
/// the placeholder bucket no longer uses. After a re-save, a placeholder bucket has unused allowance only when
/// an unnamed finding was fixed. Count mode has the same tolerance per file.
///
/// Returns `None` when no allowance moves, so callers can keep borrowing the
/// original map.
pub(super) fn identity_counts_with_placeholder_allowance(
    baseline_counts: &HealthFindingCountMap,
    current_counts: &HealthFindingCountMap,
) -> Option<HealthFindingCountMap> {
    let mut named_by_path: FxHashMap<&str, Vec<NamedBucket<'_>>> = FxHashMap::default();
    for (key, counts) in current_counts {
        if let Some((path, name)) = identity_bucket_parts(key)
            && !is_anonymous_function_name(name)
        {
            named_by_path.entry(path).or_default().push((key, counts));
        }
    }
    let mut transferred: HealthFindingCountMap = BTreeMap::new();
    for (key, placeholder_counts) in baseline_counts {
        let Some((path, name)) = identity_bucket_parts(key) else {
            continue;
        };
        if !is_anonymous_function_name(name) {
            continue;
        }
        let Some(recipients) = named_by_path.get(path) else {
            continue;
        };
        for (category, placeholder_count) in placeholder_counts {
            let mut unused = placeholder_count
                .count
                .saturating_sub(bucket_category_count(current_counts, key, category));
            for (recipient_key, recipient_counts) in recipients {
                if unused == 0 {
                    break;
                }
                let current = recipient_counts
                    .get(category)
                    .map_or(0, |entry| entry.count);
                let own_allowance = bucket_category_count(baseline_counts, recipient_key, category);
                let given = transferred
                    .entry((*recipient_key).clone())
                    .or_default()
                    .entry(category.clone())
                    .or_insert(HealthBaselineCount { count: 0 });
                let take = current
                    .saturating_sub(own_allowance + given.count)
                    .min(unused);
                given.count += take;
                unused -= take;
            }
        }
    }
    transferred.retain(|_, counts| {
        counts.retain(|_, entry| entry.count > 0);
        !counts.is_empty()
    });
    if transferred.is_empty() {
        return None;
    }
    let mut effective = baseline_counts.clone();
    for (key, counts) in transferred {
        let bucket = effective.entry(key).or_default();
        for (category, given) in counts {
            bucket
                .entry(category)
                .and_modify(|entry| entry.count += given.count)
                .or_insert(given);
        }
    }
    Some(effective)
}
