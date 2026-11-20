//! Loans of placeholder buckets in an identity health baseline.

use super::*;

/// A slot that a placeholder bucket lends to a named bucket of the same file.
pub(super) struct PlaceholderLoan {
    /// The placeholder bucket that owns the slot.
    pub(super) from: String,
    /// The current bucket of the named function that uses the slot.
    pub(super) to: String,
    /// The category of the slot.
    pub(super) category: &'static str,
    /// The number of lent slots.
    pub(super) count: usize,
}

/// The slots that placeholder buckets (`<anonymous>`, `<arrow>`) lend to
/// function names in the same file.
///
/// A function without a declared name can get a context name in a later
/// version (`Command.make("setup")`, an `Effect.fn` span name). Its findings
/// then move from the placeholder bucket to a named bucket that has no
/// slot for them. A loan keeps such a baseline valid. A slot is lent per
/// category, only to named buckets in the same file whose current count is
/// above their own slots, and only when the match of the placeholder bucket
/// does not use it. A slot that hides a finding of a lower severity in the
/// placeholder bucket is in use. Count mode has the same tolerance per file.
///
/// `saved_counts` has the keys of the current run. Returns the loans in the
/// order of the placeholder buckets.
pub(super) fn placeholder_loans(
    saved_counts: &HealthFindingCountMap,
    current_counts: &HealthFindingCountMap,
) -> Vec<PlaceholderLoan> {
    let mut named_by_path: FxHashMap<&str, Vec<NamedBucket<'_>>> = FxHashMap::default();
    for (key, counts) in current_counts {
        if let Some((path, name)) = identity_bucket_parts(key)
            && !is_anonymous_function_name(name)
        {
            named_by_path.entry(path).or_default().push((key, counts));
        }
    }
    let mut book = LoanBook {
        saved_counts,
        given: FxHashMap::default(),
        loans: Vec::new(),
    };
    for (key, placeholder_counts) in saved_counts {
        let Some((path, name)) = identity_bucket_parts(key) else {
            continue;
        };
        if !is_anonymous_function_name(name) {
            continue;
        }
        let Some(recipients) = named_by_path.get(path) else {
            continue;
        };
        for dimension in HEALTH_FINDING_DIMENSIONS {
            let saved = severity_counts_for_dimension(Some(placeholder_counts), dimension);
            let used = match_severity_slots(
                severity_counts_for_dimension(current_counts.get(key), dimension),
                saved,
            )
            .used;
            for severity in [
                fallow_output::FindingSeverity::Moderate,
                fallow_output::FindingSeverity::High,
                fallow_output::FindingSeverity::Critical,
            ] {
                let index = severity_index(severity);
                let category = HealthFindingCategory {
                    dimension,
                    severity,
                }
                .key();
                book.lend(key, category, saved[index] - used[index], recipients);
            }
        }
    }
    book.loans
}

/// The loans of one run and the slots that each named bucket got so far.
struct LoanBook<'a> {
    saved_counts: &'a HealthFindingCountMap,
    given: FxHashMap<(&'a str, &'static str), usize>,
    loans: Vec<PlaceholderLoan>,
}

impl<'a> LoanBook<'a> {
    /// Lend up to `unused` slots of `category` from the placeholder bucket
    /// `from` to the named buckets that have more findings than slots.
    fn lend(
        &mut self,
        from: &str,
        category: &'static str,
        mut unused: usize,
        recipients: &[NamedBucket<'a>],
    ) {
        for (recipient_key, recipient_counts) in recipients {
            if unused == 0 {
                break;
            }
            let current = recipient_counts
                .get(category)
                .map_or(0, |entry| entry.count);
            let own = bucket_category_count(self.saved_counts, recipient_key, category);
            let given = self
                .given
                .entry((recipient_key.as_str(), category))
                .or_default();
            let take = current.saturating_sub(own + *given).min(unused);
            if take == 0 {
                continue;
            }
            *given += take;
            unused -= take;
            self.loans.push(PlaceholderLoan {
                from: from.to_owned(),
                to: (*recipient_key).clone(),
                category,
                count: take,
            });
        }
    }
}

/// A current identity bucket with a function name: the key and its counts.
type NamedBucket<'a> = (&'a String, &'a BTreeMap<String, HealthBaselineCount>);

fn bucket_category_count(counts: &HealthFindingCountMap, key: &str, category: &str) -> usize {
    counts
        .get(key)
        .and_then(|bucket| bucket.get(category))
        .map_or(0, |entry| entry.count)
}
