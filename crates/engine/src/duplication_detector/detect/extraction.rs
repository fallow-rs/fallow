use std::collections::BTreeSet;

use super::FileData;
use super::boundary::{build_boundary_prefixes, range_contains_boundary};

/// A raw clone group before conversion to `CloneGroup`.
pub(super) struct RawGroup {
    /// List of (`file_id`, `token_offset`) instances.
    pub(super) instances: Vec<(usize, usize)>,
    /// Clone length in tokens.
    pub(super) length: usize,
}

/// Extract clone groups from the suffix array and LCP array.
///
/// Uses a stack-based approach to find all maximal LCP intervals where the
/// minimum LCP value is >= `min_tokens`, and the interval contains suffixes
/// from at least two different positions (cross-file or non-overlapping
/// same-file).
pub(super) struct CloneGroupExtractionInput<'a> {
    pub(super) sa: &'a [usize],
    pub(super) lcp: &'a [usize],
    pub(super) file_of: &'a [usize],
    pub(super) file_offsets: &'a [usize],
    pub(super) min_tokens: usize,
    pub(super) files: &'a [FileData],
    pub(super) focus_file_ids: Option<&'a [bool]>,
    pub(super) may_have_boundaries: bool,
}

pub(super) fn extract_clone_groups(input: &CloneGroupExtractionInput<'_>) -> Vec<RawGroup> {
    let sa = input.sa;
    let lcp = input.lcp;
    let n = sa.len();
    if n < 2 {
        return vec![];
    }

    let context = CloneGroupScanContext::new(input);
    let mut stack: Vec<(usize, usize)> = Vec::new();
    // Closed large intervals that no closed large interval holds yet,
    // disjoint and in suffix array order.
    let mut large_intervals: Vec<LargeInterval> = Vec::new();
    let mut groups: Vec<RawGroup> = Vec::new();

    #[expect(
        clippy::needless_range_loop,
        reason = "i is used as a value, not just as an index"
    )]
    for i in 1..=n {
        let cur_lcp = if i < n { lcp[i] } else { 0 };
        let mut start = i;

        while let Some(&(top_lcp, top_start)) = stack.last() {
            if top_lcp <= cur_lcp {
                break;
            }
            stack.pop();
            start = top_start;

            if top_lcp >= input.min_tokens {
                let interval = CloneInterval {
                    begin: start - 1,
                    end: i,
                    length: top_lcp,
                };
                if interval.end - interval.begin > LARGE_INTERVAL_MIN_POSITIONS {
                    let children_from =
                        large_intervals.partition_point(|child| child.begin < interval.begin);
                    let children = large_intervals.split_off(children_from);
                    let large = LargeInterval::collect(sa, interval, children);
                    if let Some(group) = context.build_large_group(interval, &large.positions) {
                        groups.push(group);
                    }
                    large_intervals.push(large);
                } else if let Some(group) = context.build_group(interval) {
                    groups.push(group);
                }
            }
        }

        if i < n
            && cur_lcp >= input.min_tokens
            && stack.last().is_none_or(|&(last_lcp, _)| last_lcp < cur_lcp)
        {
            stack.push((cur_lcp, start));
        }
        if stack.is_empty() && !large_intervals.is_empty() {
            large_intervals.clear();
        }
    }

    groups
}

/// Intervals with more suffix positions than this keep their positions in an
/// ordered set. Smaller intervals copy and sort their positions, which is
/// cheaper at that size.
///
/// A long run of one repeated token gives a chain of nested intervals, one
/// per repeat length. A copy and sort per interval is quadratic in the run
/// length. With the ordered set, a parent takes over the set of its largest
/// child, adds the other children (smaller set into larger set), and inserts
/// only the positions that no large child holds. The non-overlap walk then
/// jumps through the set, so the chain costs O(n log^2 n).
const LARGE_INTERVAL_MIN_POSITIONS: usize = 128;

/// A closed interval with more than `LARGE_INTERVAL_MIN_POSITIONS` positions.
struct LargeInterval {
    begin: usize,
    end: usize,
    /// All suffix positions `sa[begin..end]`, in ascending text order.
    positions: BTreeSet<usize>,
}

impl LargeInterval {
    /// Collect the positions of `interval`. `children` are the closed large
    /// intervals inside it, disjoint and in suffix array order.
    fn collect(sa: &[usize], interval: CloneInterval, mut children: Vec<Self>) -> Self {
        let largest = children
            .iter()
            .enumerate()
            .max_by_key(|(_, child)| child.positions.len())
            .map(|(index, _)| index);
        let mut positions = largest
            .map(|index| std::mem::take(&mut children[index].positions))
            .unwrap_or_default();

        let mut cursor = interval.begin;
        for child in children {
            positions.extend(&sa[cursor..child.begin]);
            positions.extend(child.positions);
            cursor = child.end;
        }
        positions.extend(&sa[cursor..interval.end]);

        Self {
            begin: interval.begin,
            end: interval.end,
            positions,
        }
    }
}

struct CloneGroupScanContext<'a> {
    sa: &'a [usize],
    file_of: &'a [usize],
    file_offsets: &'a [usize],
    files: &'a [FileData],
    focus_prefix: Option<Vec<usize>>,
    boundary_prefixes: Vec<Option<Vec<u32>>>,
    has_boundaries: bool,
}

impl<'a> CloneGroupScanContext<'a> {
    fn new(input: &CloneGroupExtractionInput<'a>) -> Self {
        let boundary_prefixes = if input.may_have_boundaries {
            build_boundary_prefixes(input.files)
        } else {
            Vec::new()
        };
        let has_boundaries =
            input.may_have_boundaries && boundary_prefixes.iter().any(Option::is_some);

        Self {
            sa: input.sa,
            file_of: input.file_of,
            file_offsets: input.file_offsets,
            files: input.files,
            focus_prefix: input
                .focus_file_ids
                .map(|ids| build_focus_prefix(input.sa, input.file_of, ids)),
            boundary_prefixes,
            has_boundaries,
        }
    }

    fn build_group(&self, interval: CloneInterval) -> Option<RawGroup> {
        if let Some(prefix) = self.focus_prefix.as_deref()
            && !interval_has_focus(prefix, interval.begin, interval.end)
        {
            return None;
        }

        build_raw_group(&RawGroupInput {
            sa: self.sa,
            file_of: self.file_of,
            file_offsets: self.file_offsets,
            files: self.files,
            boundary_prefixes: &self.boundary_prefixes,
            has_boundaries: self.has_boundaries,
            interval_begin: interval.begin,
            interval_end: interval.end,
            length: interval.length,
        })
    }

    /// Build the group of a large interval from its ordered position set.
    ///
    /// This keeps the result of `build_group`: the instances in ascending
    /// (`file_id`, offset) order, without an instance that overlaps the
    /// previous kept instance in the same file. Text order equals
    /// (`file_id`, offset) order, because files are concatenated in `file_id`
    /// order. After a kept instance at `pos`, the next candidate is at
    /// `pos + length` or at the start of the next file, whichever comes first.
    /// The walk jumps there, so it does not visit the overlapped positions.
    fn build_large_group(
        &self,
        interval: CloneInterval,
        positions: &BTreeSet<usize>,
    ) -> Option<RawGroup> {
        if let Some(prefix) = self.focus_prefix.as_deref()
            && !interval_has_focus(prefix, interval.begin, interval.end)
        {
            return None;
        }

        let length = interval.length;
        let mut instances = Vec::new();
        let mut from = 0;
        while let Some(&pos) = positions.range(from..).next() {
            from = pos + 1;
            let fid = self.file_of[pos];
            if fid == usize::MAX {
                continue;
            }
            let offset = pos - self.file_offsets[fid];
            if offset + length > self.files[fid].hashed_tokens.len()
                || (self.has_boundaries
                    && range_contains_boundary(
                        self.boundary_prefixes[fid].as_ref(),
                        offset,
                        length,
                    ))
            {
                continue;
            }

            instances.push((fid, offset));
            let next_file_start = self
                .file_offsets
                .get(fid + 1)
                .copied()
                .unwrap_or(usize::MAX);
            from = (pos + length).min(next_file_start);
        }

        (instances.len() >= 2).then_some(RawGroup { instances, length })
    }
}

#[derive(Clone, Copy)]
struct CloneInterval {
    begin: usize,
    end: usize,
    length: usize,
}

fn build_focus_prefix(sa: &[usize], file_of: &[usize], focus_file_ids: &[bool]) -> Vec<usize> {
    let mut prefix = Vec::with_capacity(sa.len() + 1);
    prefix.push(0);
    for &pos in sa {
        let focused = file_of
            .get(pos)
            .copied()
            .filter(|&file_id| file_id != usize::MAX)
            .and_then(|file_id| focus_file_ids.get(file_id))
            .copied()
            .unwrap_or(false);
        prefix.push(prefix.last().copied().unwrap_or(0) + usize::from(focused));
    }
    prefix
}

fn interval_has_focus(focus_prefix: &[usize], begin: usize, end: usize) -> bool {
    focus_prefix[end] > focus_prefix[begin]
}

/// Build a `RawGroup` from an LCP interval, filtering to non-overlapping
/// instances.
struct RawGroupInput<'a> {
    sa: &'a [usize],
    file_of: &'a [usize],
    file_offsets: &'a [usize],
    files: &'a [FileData],
    boundary_prefixes: &'a [Option<Vec<u32>>],
    has_boundaries: bool,
    interval_begin: usize,
    interval_end: usize,
    length: usize,
}

fn build_raw_group(input: &RawGroupInput<'_>) -> Option<RawGroup> {
    let instances = collect_raw_group_instances(input);
    let instances = filter_overlapping_instances(instances, input.length)?;
    Some(RawGroup {
        instances,
        length: input.length,
    })
}

fn collect_raw_group_instances(input: &RawGroupInput<'_>) -> Vec<(usize, usize)> {
    let sa = input.sa;
    let file_of = input.file_of;
    let file_offsets = input.file_offsets;
    let files = input.files;
    let boundary_prefixes = input.boundary_prefixes;
    let interval_begin = input.interval_begin;
    let interval_end = input.interval_end;
    let length = input.length;
    let mut instances: Vec<(usize, usize)> = Vec::with_capacity(interval_end - interval_begin);

    for &pos in &sa[interval_begin..interval_end] {
        let fid = file_of[pos];
        if fid == usize::MAX {
            continue;
        }
        let offset_in_file = pos - file_offsets[fid];

        if offset_in_file + length > files[fid].hashed_tokens.len() {
            continue;
        }
        if input.has_boundaries
            && range_contains_boundary(boundary_prefixes[fid].as_ref(), offset_in_file, length)
        {
            continue;
        }

        instances.push((fid, offset_in_file));
    }

    instances
}

fn filter_overlapping_instances(
    mut instances: Vec<(usize, usize)>,
    length: usize,
) -> Option<Vec<(usize, usize)>> {
    if instances.len() < 2 {
        return None;
    }

    if instances.len() == 2 {
        return filter_pair_instances(instances, length);
    }

    instances.sort_unstable();
    let deduped = dedupe_overlapping_instances(&instances, length);
    if deduped.len() < 2 {
        return None;
    }

    Some(deduped)
}

fn filter_pair_instances(
    mut instances: Vec<(usize, usize)>,
    length: usize,
) -> Option<Vec<(usize, usize)>> {
    if instances[1] < instances[0] {
        instances.swap(0, 1);
    }
    let first = instances[0];
    let second = instances[1];

    (first.0 != second.0 || second.1 >= first.1 + length).then_some(instances)
}

fn dedupe_overlapping_instances(
    instances: &[(usize, usize)],
    length: usize,
) -> Vec<(usize, usize)> {
    let mut deduped: Vec<(usize, usize)> = Vec::with_capacity(instances.len());
    for &(fid, offset) in instances {
        if let Some(&(last_fid, last_offset)) = deduped.last()
            && fid == last_fid
            && offset < last_offset + length
        {
            continue;
        }
        deduped.push((fid, offset));
    }

    deduped
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use oxc_span::Span;
    use proptest::prelude::*;

    use super::super::concatenation::concatenate_with_sentinels;
    use super::super::lcp::build_lcp;
    use super::super::suffix_array::build_suffix_array;
    use super::*;
    use crate::duplicates::normalize::HashedToken;
    use crate::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};

    /// Token value that the test files mark as a boundary token.
    const BOUNDARY_TOKEN: u32 = 0;

    fn make_file(tokens: &[u32]) -> FileData {
        let source_tokens = tokens
            .iter()
            .map(|&token| SourceToken {
                kind: if token == BOUNDARY_TOKEN {
                    TokenKind::Boundary("section".to_string())
                } else {
                    TokenKind::Identifier(format!("t{token}"))
                },
                span: Span::new(0, 1),
            })
            .collect();
        FileData {
            path: PathBuf::from("file.ts"),
            hashed_tokens: tokens
                .iter()
                .enumerate()
                .map(|(index, &token)| HashedToken {
                    hash: u64::from(token),
                    original_index: index,
                })
                .collect(),
            file_tokens: FileTokens {
                tokens: source_tokens,
                function_spans: Vec::new(),
                atomic_invocation_spans: Vec::new(),
                source: String::new(),
                line_count: 1,
            },
            atomic_invocation_spans: Vec::new(),
        }
    }

    /// The extractor before the ordered-set rewrite: every interval copies
    /// and sorts its suffix positions again. The tests use it as the oracle.
    fn reference_extract(input: &CloneGroupExtractionInput<'_>) -> Vec<RawGroup> {
        let (sa, lcp, n) = (input.sa, input.lcp, input.sa.len());
        if n < 2 {
            return vec![];
        }
        let boundary_prefixes = build_boundary_prefixes(input.files);
        let focus_prefix = input
            .focus_file_ids
            .map(|ids| build_focus_prefix(sa, input.file_of, ids));
        let mut stack: Vec<(usize, usize)> = Vec::new();
        let mut groups = Vec::new();
        #[expect(
            clippy::needless_range_loop,
            reason = "i is used as a value, not just as an index"
        )]
        for i in 1..=n {
            let cur_lcp = if i < n { lcp[i] } else { 0 };
            let mut start = i;
            while let Some(&(top_lcp, top_start)) = stack.last() {
                if top_lcp <= cur_lcp {
                    break;
                }
                stack.pop();
                start = top_start;
                let (begin, end, length) = (start - 1, i, top_lcp);
                if focus_prefix
                    .as_deref()
                    .is_some_and(|prefix| !interval_has_focus(prefix, begin, end))
                {
                    continue;
                }
                let mut instances: Vec<(usize, usize)> = sa[begin..end]
                    .iter()
                    .filter_map(|&pos| {
                        let fid = input.file_of[pos];
                        if fid == usize::MAX {
                            return None;
                        }
                        let offset = pos - input.file_offsets[fid];
                        let fits = offset + length <= input.files[fid].hashed_tokens.len();
                        (fits
                            && !range_contains_boundary(
                                boundary_prefixes[fid].as_ref(),
                                offset,
                                length,
                            ))
                        .then_some((fid, offset))
                    })
                    .collect();
                instances.sort_unstable();
                let mut kept: Vec<(usize, usize)> = Vec::new();
                for (fid, offset) in instances {
                    if kept
                        .last()
                        .is_some_and(|&(last_fid, last)| last_fid == fid && offset < last + length)
                    {
                        continue;
                    }
                    kept.push((fid, offset));
                }
                if kept.len() >= 2 {
                    groups.push(RawGroup {
                        instances: kept,
                        length,
                    });
                }
            }
            if i < n
                && cur_lcp >= input.min_tokens
                && stack.last().is_none_or(|&(last_lcp, _)| last_lcp < cur_lcp)
            {
                stack.push((cur_lcp, start));
            }
        }
        groups
    }

    /// Each raw group as (`length`, `instances`).
    type GroupTuples = Vec<(usize, Vec<(usize, usize)>)>;

    fn as_tuples(groups: &[RawGroup]) -> GroupTuples {
        groups
            .iter()
            .map(|group| (group.length, group.instances.clone()))
            .collect()
    }

    /// Run the production extractor and the oracle on the same corpus and
    /// return both results.
    fn extract_both(
        corpus: &[Vec<u32>],
        min_tokens: usize,
        focus: Option<&[bool]>,
    ) -> (GroupTuples, GroupTuples) {
        let files: Vec<FileData> = corpus.iter().map(|tokens| make_file(tokens)).collect();
        let (text, file_of, file_offsets) = concatenate_with_sentinels(corpus);
        let sa = build_suffix_array(&text);
        let lcp = build_lcp(&text, &sa);
        let input = CloneGroupExtractionInput {
            sa: &sa,
            lcp: &lcp,
            file_of: &file_of,
            file_offsets: &file_offsets,
            min_tokens,
            files: &files,
            focus_file_ids: focus,
            may_have_boundaries: true,
        };
        (
            as_tuples(&extract_clone_groups(&input)),
            as_tuples(&reference_extract(&input)),
        )
    }

    #[test]
    fn long_repeated_token_runs_match_reference() {
        let run: Vec<u32> = std::iter::repeat_n(7, 3_000).collect();
        let corpus: Vec<Vec<u32>> = (0..5)
            .map(|file| {
                let mut tokens = run.clone();
                tokens.push(100 + file);
                tokens
            })
            .collect();

        let (actual, expected) = extract_both(&corpus, 50, None);

        assert_ne!(expected, [] as [(usize, Vec<(usize, usize)>); 0]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn periodic_runs_with_boundaries_match_reference() {
        let corpus: Vec<Vec<u32>> = (0..3_u32)
            .map(|file| {
                (0..900_u32)
                    .map(|index| match index % 97 {
                        0 if file == 1 => BOUNDARY_TOKEN,
                        _ => 1 + index % 3,
                    })
                    .collect()
            })
            .collect();

        let (actual, expected) = extract_both(&corpus, 5, None);

        assert_ne!(expected, [] as [(usize, Vec<(usize, usize)>); 0]);
        assert_eq!(actual, expected);
    }

    proptest! {
        #[test]
        fn extraction_matches_reference(
            corpus in prop::collection::vec(prop::collection::vec(0_u32..4, 0..250), 1..6),
            min_tokens in 1_usize..8,
            focus in prop::option::of(prop::collection::vec(any::<bool>(), 6)),
        ) {
            let focus = focus.map(|mask| mask[..corpus.len()].to_vec());
            let (actual, expected) = extract_both(&corpus, min_tokens, focus.as_deref());
            prop_assert_eq!(actual, expected);
        }
    }
}
