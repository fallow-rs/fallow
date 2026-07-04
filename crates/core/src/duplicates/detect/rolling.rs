use rustc_hash::FxHashMap;

use super::boundary::{build_boundary_prefixes, range_contains_boundary};
use super::extraction::RawGroup;
use super::{CorpusTotals, FileData, filtering, ranking, statistics};
use crate::duplicates::types::DuplicationReport;

const HASH_BASE: u64 = 0x0000_0100_0000_01b3;
const MAX_ADJACENCY_LOOKAHEAD_PAIRS: usize = 512;
const CHILD_BUCKET_PREALLOC_LIMIT: usize = 64;

#[derive(Clone, Copy)]
struct Occurrence {
    file_id: usize,
    offset: usize,
}

struct SeedClass {
    key_file_id: usize,
    key_offset: usize,
    occurrences: SeedOccurrences,
}

enum SeedOccurrences {
    One(Occurrence),
    Many(Vec<Occurrence>),
}

enum SeedBucket {
    One(SeedClass),
    Many(Vec<SeedClass>),
}

#[derive(Clone, Copy)]
pub(super) struct RollingDetectInput<'a> {
    pub(super) files: &'a [FileData],
    pub(super) min_tokens: usize,
    pub(super) min_lines: usize,
    pub(super) skip_local: bool,
    pub(super) totals: CorpusTotals,
    pub(super) may_have_boundaries: bool,
}

impl SeedOccurrences {
    fn push(&mut self, occurrence: Occurrence) {
        match self {
            Self::One(first) => {
                *self = Self::Many(vec![*first, occurrence]);
            }
            Self::Many(occurrences) => occurrences.push(occurrence),
        }
    }

    fn into_many(self) -> Option<Vec<Occurrence>> {
        match self {
            Self::One(_) => None,
            Self::Many(occurrences) => Some(occurrences),
        }
    }
}

impl SeedBucket {
    fn insert_or_append(
        &mut self,
        ranked_files: &[Vec<u32>],
        file_id: usize,
        offset: usize,
        min_tokens: usize,
    ) {
        match self {
            Self::One(class) => {
                if same_window(
                    ranked_files,
                    class.key_file_id,
                    class.key_offset,
                    file_id,
                    offset,
                    min_tokens,
                ) {
                    class.occurrences.push(Occurrence { file_id, offset });
                } else {
                    let first = std::mem::replace(
                        class,
                        SeedClass {
                            key_file_id: file_id,
                            key_offset: offset,
                            occurrences: SeedOccurrences::One(Occurrence { file_id, offset }),
                        },
                    );
                    *self = Self::Many(vec![
                        first,
                        SeedClass {
                            key_file_id: file_id,
                            key_offset: offset,
                            occurrences: SeedOccurrences::One(Occurrence { file_id, offset }),
                        },
                    ]);
                }
            }
            Self::Many(classes) => {
                if let Some(class) = classes.iter_mut().find(|class| {
                    same_window(
                        ranked_files,
                        class.key_file_id,
                        class.key_offset,
                        file_id,
                        offset,
                        min_tokens,
                    )
                }) {
                    class.occurrences.push(Occurrence { file_id, offset });
                } else {
                    classes.push(SeedClass {
                        key_file_id: file_id,
                        key_offset: offset,
                        occurrences: SeedOccurrences::One(Occurrence { file_id, offset }),
                    });
                }
            }
        }
    }
}

pub(super) fn detect(input: RollingDetectInput<'_>) -> DuplicationReport {
    let RollingDetectInput {
        files,
        min_tokens,
        min_lines,
        skip_local,
        totals,
        may_have_boundaries,
    } = input;
    let t0 = std::time::Instant::now();
    let ranked_files = ranking::rank_reduce(files);
    let rank_time = t0.elapsed();

    let t0 = std::time::Instant::now();
    let (boundary_prefixes, has_boundaries) =
        boundary_prefixes_for_detection(files, may_have_boundaries);
    let seed_buckets = build_seed_buckets(
        &ranked_files,
        &boundary_prefixes,
        has_boundaries,
        min_tokens,
    );
    let seed_time = t0.elapsed();

    let t0 = std::time::Instant::now();
    let (raw_groups, seed_class_count) = build_raw_groups(
        &ranked_files,
        &boundary_prefixes,
        has_boundaries,
        seed_buckets,
        min_tokens,
    );
    let raw_time = t0.elapsed();
    tracing::debug!(
        elapsed_us = raw_time.as_micros(),
        raw_groups = raw_groups.len(),
        seed_classes = seed_class_count,
        "rolling_step3_raw_groups"
    );

    let t0 = std::time::Instant::now();
    let clone_groups = filtering::build_groups(raw_groups, files, min_lines, skip_local);
    let build_time = t0.elapsed();

    let t0 = std::time::Instant::now();
    let stats = statistics::compute_stats(&clone_groups, totals.files, totals.lines, totals.tokens);
    let stats_time = t0.elapsed();

    tracing::info!(
        total_us = (rank_time + seed_time + raw_time + build_time + stats_time).as_micros(),
        rank_us = rank_time.as_micros(),
        seed_us = seed_time.as_micros(),
        raw_us = raw_time.as_micros(),
        build_us = build_time.as_micros(),
        stats_us = stats_time.as_micros(),
        total_tokens = totals.tokens,
        clone_groups = clone_groups.len(),
        "rolling clone detection complete"
    );

    DuplicationReport {
        clone_groups,
        clone_families: vec![],
        mirrored_directories: vec![],
        stats,
    }
}

fn boundary_prefixes_for_detection(
    files: &[FileData],
    may_have_boundaries: bool,
) -> (Vec<Option<Vec<u32>>>, bool) {
    let boundary_prefixes = if may_have_boundaries {
        build_boundary_prefixes(files)
    } else {
        std::iter::repeat_with(|| None).take(files.len()).collect()
    };
    let has_boundaries = may_have_boundaries && boundary_prefixes.iter().any(Option::is_some);
    (boundary_prefixes, has_boundaries)
}

fn build_seed_buckets(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    min_tokens: usize,
) -> FxHashMap<u64, SeedBucket> {
    let mut buckets: FxHashMap<u64, SeedBucket> = FxHashMap::default();
    let base_power = hash_base_power(min_tokens);

    for (file_id, tokens) in ranked_files.iter().enumerate() {
        if tokens.len() < min_tokens {
            continue;
        }

        let last_offset = tokens.len() - min_tokens;
        let mut hash = initial_window_hash(tokens, min_tokens);
        for offset in 0..=tokens.len() - min_tokens {
            if has_boundaries
                && range_contains_boundary(boundary_prefixes[file_id].as_ref(), offset, min_tokens)
            {
                if offset < last_offset {
                    hash = roll_window_hash(
                        hash,
                        tokens[offset],
                        tokens[offset + min_tokens],
                        base_power,
                    );
                }
                continue;
            }

            match buckets.get_mut(&hash) {
                Some(bucket) => {
                    bucket.insert_or_append(ranked_files, file_id, offset, min_tokens);
                }
                None => {
                    buckets.insert(
                        hash,
                        SeedBucket::One(SeedClass {
                            key_file_id: file_id,
                            key_offset: offset,
                            occurrences: SeedOccurrences::One(Occurrence { file_id, offset }),
                        }),
                    );
                }
            }

            if offset < last_offset {
                hash = roll_window_hash(
                    hash,
                    tokens[offset],
                    tokens[offset + min_tokens],
                    base_power,
                );
            }
        }
    }

    buckets
}

fn build_raw_groups(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    seed_buckets: FxHashMap<u64, SeedBucket>,
    min_tokens: usize,
) -> (Vec<RawGroup>, usize) {
    let mut groups = Vec::new();
    let mut seed_class_count = 0_usize;

    {
        let mut ctx = SeedClassContext {
            ranked_files,
            boundary_prefixes,
            has_boundaries,
            min_tokens,
            seed_class_count: &mut seed_class_count,
            groups: &mut groups,
        };

        for bucket in seed_buckets.into_values() {
            match bucket {
                SeedBucket::One(class) => {
                    process_seed_class(&mut ctx, class);
                }
                SeedBucket::Many(classes) => {
                    for class in classes {
                        process_seed_class(&mut ctx, class);
                    }
                }
            }
        }
    }

    (groups, seed_class_count)
}

struct SeedClassContext<'a, 'b> {
    ranked_files: &'a [Vec<u32>],
    boundary_prefixes: &'a [Option<Vec<u32>>],
    has_boundaries: bool,
    min_tokens: usize,
    seed_class_count: &'b mut usize,
    groups: &'b mut Vec<RawGroup>,
}

struct GroupCollectionContext<'a, 'b> {
    ranked_files: &'a [Vec<u32>],
    boundary_prefixes: &'a [Option<Vec<u32>>],
    has_boundaries: bool,
    groups: &'b mut Vec<RawGroup>,
}

fn process_seed_class(ctx: &mut SeedClassContext<'_, '_>, class: SeedClass) {
    let Some(occurrences) = class.occurrences.into_many() else {
        return;
    };
    *ctx.seed_class_count += 1;

    let is_left_maximal = is_left_maximal(
        ctx.ranked_files,
        ctx.boundary_prefixes,
        ctx.has_boundaries,
        &occurrences,
        ctx.min_tokens,
    );

    if is_left_maximal {
        let mut group_ctx = GroupCollectionContext {
            ranked_files: ctx.ranked_files,
            boundary_prefixes: ctx.boundary_prefixes,
            has_boundaries: ctx.has_boundaries,
            groups: ctx.groups,
        };
        collect_extended_groups(&mut group_ctx, &occurrences, ctx.min_tokens);
    }
}

fn same_window(
    ranked_files: &[Vec<u32>],
    left_file_id: usize,
    left_offset: usize,
    right_file_id: usize,
    right_offset: usize,
    length: usize,
) -> bool {
    ranked_files[left_file_id][left_offset..left_offset + length]
        == ranked_files[right_file_id][right_offset..right_offset + length]
}

fn is_left_maximal(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    occurrences: &[Occurrence],
    length: usize,
) -> bool {
    let Some(first) = occurrences.first() else {
        return false;
    };
    if first.offset == 0 {
        return true;
    }

    let previous = ranked_files[first.file_id][first.offset - 1];
    if occurrences.iter().any(|occurrence| {
        occurrence.offset == 0
            || ranked_files[occurrence.file_id][occurrence.offset - 1] != previous
            || (has_boundaries
                && range_contains_boundary(
                    boundary_prefixes[occurrence.file_id].as_ref(),
                    occurrence.offset - 1,
                    length + 1,
                ))
    }) {
        return true;
    }

    if has_same_file_pair_at_or_extending_to_adjacency(
        ranked_files,
        boundary_prefixes,
        has_boundaries,
        occurrences,
        length,
    ) {
        return true;
    }

    let current_count = count_non_overlapping_instances(occurrences, length, false);
    let extended_count = count_non_overlapping_instances(occurrences, length + 1, true);
    current_count >= 2 && extended_count < current_count
}

fn collect_extended_groups(
    ctx: &mut GroupCollectionContext<'_, '_>,
    occurrences: &[Occurrence],
    mut length: usize,
) {
    if occurrences.len() == 2 {
        let length = extend_pair_length(
            ctx.ranked_files,
            ctx.boundary_prefixes,
            ctx.has_boundaries,
            occurrences[0],
            occurrences[1],
            length,
        );
        push_raw_group(occurrences, length, ctx.groups);
        return;
    }

    length = extend_uniform_length(
        ctx.ranked_files,
        ctx.boundary_prefixes,
        ctx.has_boundaries,
        occurrences,
        length,
    );

    let children = collect_child_occurrences(
        ctx.ranked_files,
        ctx.boundary_prefixes,
        ctx.has_boundaries,
        occurrences,
        length,
    );

    push_raw_group(occurrences, length, ctx.groups);

    let Some(children) = children else {
        return;
    };

    for child in children
        .into_values()
        .filter_map(SeedOccurrences::into_many)
    {
        if is_left_maximal(
            ctx.ranked_files,
            ctx.boundary_prefixes,
            ctx.has_boundaries,
            &child,
            length + 1,
        ) {
            collect_extended_groups(ctx, &child, length + 1);
        }
    }
}

fn collect_child_occurrences(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    occurrences: &[Occurrence],
    length: usize,
) -> Option<FxHashMap<u32, SeedOccurrences>> {
    let mut first_child: Option<(u32, Occurrence)> = None;
    let mut children: Option<FxHashMap<u32, SeedOccurrences>> = None;
    for &occurrence in occurrences {
        let Some(token) = child_token_after_range(
            ranked_files,
            boundary_prefixes,
            has_boundaries,
            occurrence,
            length,
        ) else {
            continue;
        };

        if let Some(children) = children.as_mut() {
            insert_child_occurrence(children, token, occurrence);
        } else if let Some((first_token, first_occurrence)) = first_child.take() {
            let mut child_map = FxHashMap::default();
            child_map.reserve(occurrences.len().min(CHILD_BUCKET_PREALLOC_LIMIT));
            insert_child_occurrence(&mut child_map, first_token, first_occurrence);
            insert_child_occurrence(&mut child_map, token, occurrence);
            children = Some(child_map);
        } else {
            first_child = Some((token, occurrence));
        }
    }

    children
}

fn child_token_after_range(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    occurrence: Occurrence,
    length: usize,
) -> Option<u32> {
    let tokens = &ranked_files[occurrence.file_id];
    if occurrence.offset + length >= tokens.len()
        || (has_boundaries
            && range_contains_boundary(
                boundary_prefixes[occurrence.file_id].as_ref(),
                occurrence.offset,
                length + 1,
            ))
    {
        return None;
    }

    Some(tokens[occurrence.offset + length])
}

fn insert_child_occurrence(
    children: &mut FxHashMap<u32, SeedOccurrences>,
    token: u32,
    occurrence: Occurrence,
) {
    children
        .entry(token)
        .and_modify(|occurrences| occurrences.push(occurrence))
        .or_insert(SeedOccurrences::One(occurrence));
}

fn extend_uniform_length(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    occurrences: &[Occurrence],
    mut length: usize,
) -> usize {
    let Some(first) = occurrences.first() else {
        return length;
    };

    loop {
        let Some(next) = next_extension_token(
            ranked_files,
            boundary_prefixes,
            has_boundaries,
            *first,
            length,
        ) else {
            return length;
        };

        if occurrences[1..].iter().all(|&occurrence| {
            next_extension_token(
                ranked_files,
                boundary_prefixes,
                has_boundaries,
                occurrence,
                length,
            ) == Some(next)
        }) {
            length += 1;
            continue;
        }

        return length;
    }
}

fn next_extension_token(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    occurrence: Occurrence,
    length: usize,
) -> Option<u32> {
    let tokens = &ranked_files[occurrence.file_id];
    if occurrence.offset + length >= tokens.len()
        || (has_boundaries
            && range_contains_boundary(
                boundary_prefixes[occurrence.file_id].as_ref(),
                occurrence.offset,
                length + 1,
            ))
    {
        return None;
    }

    Some(tokens[occurrence.offset + length])
}

fn extend_pair_length(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    left: Occurrence,
    right: Occurrence,
    length: usize,
) -> usize {
    let mut extended = length;
    let left_tokens = &ranked_files[left.file_id];
    let right_tokens = &ranked_files[right.file_id];
    let left_boundary = has_boundaries
        .then(|| boundary_prefixes[left.file_id].as_ref())
        .flatten();
    let right_boundary = has_boundaries
        .then(|| boundary_prefixes[right.file_id].as_ref())
        .flatten();

    if left_boundary.is_none() && right_boundary.is_none() {
        while left.offset + extended < left_tokens.len()
            && right.offset + extended < right_tokens.len()
            && left_tokens[left.offset + extended] == right_tokens[right.offset + extended]
        {
            extended += 1;
        }

        return extended;
    }

    while left.offset + extended < left_tokens.len()
        && right.offset + extended < right_tokens.len()
        && left_tokens[left.offset + extended] == right_tokens[right.offset + extended]
        && !range_contains_boundary(left_boundary, left.offset, extended + 1)
        && !range_contains_boundary(right_boundary, right.offset, extended + 1)
    {
        extended += 1;
    }

    extended
}

fn count_non_overlapping_instances(
    occurrences: &[Occurrence],
    length: usize,
    shift_left: bool,
) -> usize {
    debug_assert_occurrences_sorted(occurrences);

    let mut count = 0_usize;
    let mut last: Option<(usize, usize)> = None;
    for occurrence in occurrences {
        let file_id = occurrence.file_id;
        let offset = if shift_left {
            occurrence.offset - 1
        } else {
            occurrence.offset
        };
        if let Some((last_file_id, last_offset)) = last
            && file_id == last_file_id
            && offset < last_offset + length
        {
            continue;
        }
        count += 1;
        last = Some((file_id, offset));
    }

    count
}

fn has_same_file_pair_at_or_extending_to_adjacency(
    ranked_files: &[Vec<u32>],
    boundary_prefixes: &[Option<Vec<u32>>],
    has_boundaries: bool,
    occurrences: &[Occurrence],
    length: usize,
) -> bool {
    debug_assert_occurrences_sorted(occurrences);

    let mut checked_pairs = 0_usize;
    for (index, occurrence) in occurrences.iter().enumerate() {
        let file_id = occurrence.file_id;
        let left = occurrence.offset;
        for candidate in &occurrences[index + 1..] {
            let right_file_id = candidate.file_id;
            let right = candidate.offset;
            if file_id != right_file_id {
                break;
            }
            if right == left + length {
                return true;
            }
            if right < left + length {
                continue;
            }
            checked_pairs += 1;
            if checked_pairs > MAX_ADJACENCY_LOOKAHEAD_PAIRS {
                return false;
            }
            if pair_extends_to_adjacency(AdjacentPairInput {
                ranked_files,
                boundary_prefixes,
                has_boundaries,
                file_id,
                left,
                right,
                length,
            }) {
                return true;
            }
        }
    }

    false
}

#[derive(Clone, Copy)]
struct AdjacentPairInput<'a> {
    ranked_files: &'a [Vec<u32>],
    boundary_prefixes: &'a [Option<Vec<u32>>],
    has_boundaries: bool,
    file_id: usize,
    left: usize,
    right: usize,
    length: usize,
}

fn pair_extends_to_adjacency(input: AdjacentPairInput<'_>) -> bool {
    let AdjacentPairInput {
        ranked_files,
        boundary_prefixes,
        has_boundaries,
        file_id,
        left,
        right,
        length,
    } = input;
    let tokens = &ranked_files[file_id];
    let distance = right - left;
    let mut extended = length;
    let boundary = has_boundaries
        .then(|| boundary_prefixes[file_id].as_ref())
        .flatten();

    if boundary.is_none() {
        while extended < distance
            && right + extended < tokens.len()
            && tokens[left + extended] == tokens[right + extended]
        {
            extended += 1;
        }

        return extended == distance;
    }

    while extended < distance
        && right + extended < tokens.len()
        && tokens[left + extended] == tokens[right + extended]
        && !range_contains_boundary(boundary, left, extended + 1)
        && !range_contains_boundary(boundary, right, extended + 1)
    {
        extended += 1;
    }

    extended == distance
}

fn push_raw_group(occurrences: &[Occurrence], length: usize, groups: &mut Vec<RawGroup>) {
    debug_assert_occurrences_sorted(occurrences);

    let mut deduped = Vec::with_capacity(occurrences.len());
    for occurrence in occurrences {
        let file_id = occurrence.file_id;
        let offset = occurrence.offset;
        if let Some(&(last_file_id, last_offset)) = deduped.last()
            && file_id == last_file_id
            && offset < last_offset + length
        {
            continue;
        }
        deduped.push((file_id, offset));
    }

    if deduped.len() >= 2 {
        groups.push(RawGroup {
            instances: deduped,
            length,
        });
    }
}

fn debug_assert_occurrences_sorted(occurrences: &[Occurrence]) {
    debug_assert!(occurrences.windows(2).all(|pair| {
        pair[0].file_id < pair[1].file_id
            || (pair[0].file_id == pair[1].file_id && pair[0].offset <= pair[1].offset)
    }));
}

fn hash_base_power(length: usize) -> u64 {
    let mut power = 1_u64;
    for _ in 1..length {
        power = power.wrapping_mul(HASH_BASE);
    }
    power
}

fn initial_window_hash(tokens: &[u32], length: usize) -> u64 {
    let mut hash = 0_u64;
    for &token in &tokens[..length] {
        hash = hash.wrapping_mul(HASH_BASE).wrapping_add(u64::from(token));
    }
    hash
}

fn roll_window_hash(hash: u64, outgoing: u32, incoming: u32, base_power: u64) -> u64 {
    hash.wrapping_sub(u64::from(outgoing).wrapping_mul(base_power))
        .wrapping_mul(HASH_BASE)
        .wrapping_add(u64::from(incoming))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use oxc_span::Span;

    use super::*;
    use crate::duplicates::normalize::HashedToken;
    use crate::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test span values are trivially small"
    )]
    fn make_file(path: &str, hashes: &[u64]) -> FileData {
        let source = std::iter::repeat_n("x", hashes.len())
            .collect::<Vec<_>>()
            .join("\n");
        FileData {
            path: PathBuf::from(path),
            hashed_tokens: hashes
                .iter()
                .enumerate()
                .map(|(index, &hash)| HashedToken {
                    hash,
                    original_index: index,
                })
                .collect(),
            file_tokens: FileTokens {
                tokens: (0..hashes.len())
                    .map(|index| SourceToken {
                        kind: TokenKind::Identifier(format!("t{index}")),
                        span: Span::new((index * 2) as u32, (index * 2 + 1) as u32),
                    })
                    .collect(),
                atomic_invocation_spans: Vec::new(),
                source,
                line_count: hashes.len(),
            },
            atomic_invocation_spans: Vec::new(),
        }
    }

    fn totals(files: &[FileData]) -> CorpusTotals {
        CorpusTotals {
            files: files.len(),
            lines: files.iter().map(|file| file.file_tokens.line_count).sum(),
            tokens: files.iter().map(|file| file.hashed_tokens.len()).sum(),
        }
    }

    fn detect_files(files: &[FileData], min_tokens: usize) -> DuplicationReport {
        detect(RollingDetectInput {
            files,
            min_tokens,
            min_lines: 1,
            skip_local: false,
            totals: totals(files),
            may_have_boundaries: false,
        })
    }

    #[test]
    fn rolling_detects_exact_duplicate_across_files() {
        let files = vec![
            make_file("a.ts", &[1, 2, 3, 4, 5]),
            make_file("b.ts", &[1, 2, 3, 4, 5]),
        ];

        let report = detect_files(&files, 3);

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(report.clone_groups[0].instances.len(), 2);
        assert_eq!(report.clone_groups[0].token_count, 5);
    }

    #[test]
    fn rolling_keeps_long_subgroup_when_seed_has_extra_occurrence() {
        let files = vec![
            make_file("a.ts", &[1, 2, 3, 4, 5, 6, 7, 8]),
            make_file("b.ts", &[1, 2, 3, 4, 5, 6, 7, 8]),
            make_file("c.ts", &[1, 2, 3, 9, 10, 11]),
        ];

        let report = detect_files(&files, 3);

        assert!(
            report
                .clone_groups
                .iter()
                .any(|group| group.token_count == 8 && group.instances.len() == 2),
            "expected the long pair clone to survive alongside the broad short seed"
        );
    }

    #[test]
    fn rolling_keeps_adjacent_same_file_clone_that_extends_from_seed() {
        let files = vec![make_file("a.ts", &[5, 1, 2, 3, 4, 5, 1, 2, 3, 4, 5])];

        let report = detect_files(&files, 3);

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(report.clone_groups[0].instances.len(), 2);
        assert_eq!(report.clone_groups[0].token_count, 5);
    }

    #[test]
    fn adjacency_lookahead_checks_past_intervening_occurrences() {
        let ranked_files = vec![vec![1, 2, 1, 2, 1, 1, 2, 1, 2, 1]];
        let occurrences = vec![
            Occurrence {
                file_id: 0,
                offset: 0,
            },
            Occurrence {
                file_id: 0,
                offset: 2,
            },
            Occurrence {
                file_id: 0,
                offset: 5,
            },
        ];

        assert!(has_same_file_pair_at_or_extending_to_adjacency(
            &ranked_files,
            &[None],
            false,
            &occurrences,
            3,
        ));
    }

    #[test]
    fn rolling_extends_long_multi_occurrence_clone_iteratively() {
        let tokens: Vec<u32> = (0..20_000).collect();
        let ranked_files = vec![tokens.clone(), tokens.clone(), tokens];
        let occurrences = vec![
            Occurrence {
                file_id: 0,
                offset: 0,
            },
            Occurrence {
                file_id: 1,
                offset: 0,
            },
            Occurrence {
                file_id: 2,
                offset: 0,
            },
        ];
        let mut groups = Vec::new();
        let boundary_prefixes = vec![None, None, None];
        let mut ctx = GroupCollectionContext {
            ranked_files: &ranked_files,
            boundary_prefixes: &boundary_prefixes,
            has_boundaries: false,
            groups: &mut groups,
        };

        collect_extended_groups(&mut ctx, &occurrences, 3);

        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].length, 20_000);
        assert_eq!(groups[0].instances, vec![(0, 0), (1, 0), (2, 0)]);
    }
}
