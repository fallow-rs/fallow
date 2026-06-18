use super::FileData;
use crate::duplicates::tokenize::TokenKind;

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
}

pub(super) fn extract_clone_groups(input: &CloneGroupExtractionInput<'_>) -> Vec<RawGroup> {
    let sa = input.sa;
    let lcp = input.lcp;
    let file_of = input.file_of;
    let file_offsets = input.file_offsets;
    let files = input.files;
    let n = sa.len();
    if n < 2 {
        return vec![];
    }

    let mut stack: Vec<(usize, usize)> = Vec::new();
    let mut groups: Vec<RawGroup> = Vec::new();
    let focus_prefix = input
        .focus_file_ids
        .map(|ids| build_focus_prefix(sa, file_of, ids));
    let boundary_prefixes = build_boundary_prefixes(files);

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
                if let Some(prefix) = focus_prefix.as_deref()
                    && !interval_has_focus(prefix, interval_begin, interval_end)
                {
                    continue;
                }

                if let Some(group) = build_raw_group(&RawGroupInput {
                    sa,
                    file_of,
                    file_offsets,
                    files,
                    boundary_prefixes: &boundary_prefixes,
                    interval_begin,
                    interval_end,
                    length: top_lcp,
                }) {
                    groups.push(group);
                }
            }
        }

        if i < n {
            stack.push((cur_lcp, start));
        }
    }

    groups
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

fn build_boundary_prefixes(files: &[FileData]) -> Vec<Option<Vec<u32>>> {
    files
        .iter()
        .map(|file| {
            if !file
                .hashed_tokens
                .iter()
                .any(|token| hashed_token_is_boundary(file, token.original_index))
            {
                return None;
            }

            let mut prefix = Vec::with_capacity(file.hashed_tokens.len() + 1);
            let mut count = 0_u32;
            prefix.push(count);
            for token in &file.hashed_tokens {
                if hashed_token_is_boundary(file, token.original_index) {
                    count += 1;
                }
                prefix.push(count);
            }
            Some(prefix)
        })
        .collect()
}

fn hashed_token_is_boundary(file: &FileData, original_index: usize) -> bool {
    file.file_tokens
        .tokens
        .get(original_index)
        .is_some_and(|source| matches!(source.kind, TokenKind::Boundary(_)))
}

/// Build a `RawGroup` from an LCP interval, filtering to non-overlapping
/// instances.
struct RawGroupInput<'a> {
    sa: &'a [usize],
    file_of: &'a [usize],
    file_offsets: &'a [usize],
    files: &'a [FileData],
    boundary_prefixes: &'a [Option<Vec<u32>>],
    interval_begin: usize,
    interval_end: usize,
    length: usize,
}

fn build_raw_group(input: &RawGroupInput<'_>) -> Option<RawGroup> {
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
        if range_contains_boundary(boundary_prefixes[fid].as_ref(), offset_in_file, length) {
            continue;
        }

        instances.push((fid, offset_in_file));
    }

    if instances.len() < 2 {
        return None;
    }

    instances.sort_unstable();
    let mut deduped: Vec<(usize, usize)> = Vec::with_capacity(instances.len());
    for &(fid, offset) in &instances {
        if let Some(&(last_fid, last_offset)) = deduped.last()
            && fid == last_fid
            && offset < last_offset + length
        {
            continue;
        }
        deduped.push((fid, offset));
    }

    if deduped.len() < 2 {
        return None;
    }

    Some(RawGroup {
        instances: deduped,
        length,
    })
}

fn range_contains_boundary(prefix: Option<&Vec<u32>>, offset: usize, length: usize) -> bool {
    prefix.is_some_and(|prefix| prefix[offset] != prefix[offset + length])
}
