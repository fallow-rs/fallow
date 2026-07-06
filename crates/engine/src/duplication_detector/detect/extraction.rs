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
                let interval_begin = start - 1;
                let interval_end = i;
                if let Some(group) = context.build_group(CloneInterval {
                    begin: interval_begin,
                    end: interval_end,
                    length: top_lcp,
                }) {
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
    }

    groups
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
