//! Bounded function-scoped near-miss clone detection.

use std::cmp::Reverse;
use std::path::{Path, PathBuf};

use fallow_config::{DetectionMode, NormalizationConfig, ResolvedNormalization};
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use super::TokenizedFile;
use super::normalize::normalize_and_hash_resolved;
use super::tokenize::{FragmentTokenizationStrategy, tokenize_fragment};
use super::types::{CloneGroup, CloneGroupKind, CloneInstance};

const SHINGLE_TOKENS: usize = 7;
const SHINGLE_BASE: u64 = 1_000_003;
const MINHASH_VALUES: usize = 64;
const MINHASH_BANDS: usize = 16;
const MINHASH_ROWS: usize = MINHASH_VALUES / MINHASH_BANDS;
const MAX_BUCKET_MEMBERS: usize = 256;
const MAX_CANDIDATES_PER_FUNCTION: usize = 256;
const MAX_CANDIDATE_CHECKS: usize = 1_000_000;
const MAX_CLUSTER_MEMBERS: usize = 64;
const MIN_SIMILARITY: f64 = 0.80;
const MINHASH_SEED_BASE: u64 = 0x243f_6a88_85a3_08d3;
const MINHASH_SEED_STEP: u64 = 0x9e37_79b9_7f4a_7c15;

/// Inputs for one near-miss detection run.
pub(super) struct NearDetectionInput<'a> {
    pub(super) files: &'a [TokenizedFile],
    pub(super) min_tokens: usize,
    pub(super) min_lines: usize,
    pub(super) skip_local: bool,
    /// When present, every emitted group must contain at least one focused file.
    pub(super) focus_files: Option<&'a FxHashSet<PathBuf>>,
}

/// Bounded near-miss results plus visible truncation metadata.
pub(super) struct NearDetectionResult {
    pub(super) clone_groups: Vec<NearCloneGroup>,
    pub(super) skipped_candidates: usize,
}

/// Internal near-clone representation with a required similarity invariant.
pub(super) struct NearCloneGroup {
    instances: Vec<CloneInstance>,
    token_count: usize,
    line_count: usize,
    similarity: f64,
}

impl NearCloneGroup {
    pub(super) fn into_clone_group(self) -> CloneGroup {
        CloneGroup {
            instances: self.instances,
            token_count: self.token_count,
            line_count: self.line_count,
            similarity: Some(self.similarity),
        }
    }
}

#[derive(Debug)]
struct FunctionCandidate {
    file: PathBuf,
    span: Span,
    semantic_tokens: Vec<u64>,
    shingles: Vec<u64>,
    signature: [u64; MINHASH_VALUES],
    start_line: usize,
    end_line: usize,
    start_col: usize,
    end_col: usize,
    fragment: String,
    focused: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BandKey([u64; MINHASH_ROWS]);

#[derive(Default)]
struct BandBucket {
    members: Vec<usize>,
    omitted: usize,
}

#[derive(Default)]
struct CheckBudget {
    used: usize,
    skipped: usize,
}

impl CheckBudget {
    fn charge(&mut self) -> bool {
        if self.used >= MAX_CANDIDATE_CHECKS {
            self.skipped = self.skipped.saturating_add(1);
            return false;
        }
        self.used += 1;
        true
    }

    fn skip(&mut self, count: usize) {
        self.skipped = self.skipped.saturating_add(count);
    }
}

struct UnionFind {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl UnionFind {
    fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
            size: vec![1; len],
        }
    }

    fn find(&mut self, value: usize) -> usize {
        if self.parent[value] != value {
            self.parent[value] = self.find(self.parent[value]);
        }
        self.parent[value]
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left_root = self.find(left);
        let mut right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        if self.size[left_root] < self.size[right_root] {
            std::mem::swap(&mut left_root, &mut right_root);
        }
        self.parent[right_root] = left_root;
        self.size[left_root] += self.size[right_root];
    }
}

/// Detect near-miss function clones with bounded deterministic candidate work.
pub(super) fn detect_near_clones(input: &NearDetectionInput<'_>) -> NearDetectionResult {
    if input.min_tokens == 0 || input.files.is_empty() {
        return NearDetectionResult {
            clone_groups: Vec::new(),
            skipped_candidates: 0,
        };
    }

    let candidates = build_candidates(input);
    if candidates.len() < 2 {
        return NearDetectionResult {
            clone_groups: Vec::new(),
            skipped_candidates: 0,
        };
    }

    let mut similarities = FxHashMap::default();
    let mut budget = CheckBudget::default();
    let mut union_find = candidate_components(&candidates, &mut similarities, &mut budget);
    let components = collect_components(&mut union_find, candidates.len());
    let clusters = complete_link_clusters(components, &candidates, &mut similarities, &mut budget);
    let mut clone_groups = clusters
        .into_iter()
        .filter(|cluster| cluster.len() >= 2)
        .filter(|cluster| {
            input.focus_files.is_none() || cluster.iter().any(|&id| candidates[id].focused)
        })
        .filter_map(|cluster| {
            build_clone_group(&cluster, &candidates, &similarities, input.skip_local)
        })
        .collect::<Vec<_>>();
    sort_groups(&mut clone_groups);

    tracing::debug!(
        functions = candidates.len(),
        near_groups = clone_groups.len(),
        candidate_checks = budget.used,
        skipped_candidates = budget.skipped,
        "bounded near-miss clone detection"
    );

    NearDetectionResult {
        clone_groups,
        skipped_candidates: budget.skipped,
    }
}

fn build_candidates(input: &NearDetectionInput<'_>) -> Vec<FunctionCandidate> {
    let semantic =
        ResolvedNormalization::resolve(DetectionMode::Semantic, &NormalizationConfig::default());
    let normalized_focus = input.focus_files.map(|files| {
        files
            .iter()
            .map(|path| dunce::simplified(path).to_path_buf())
            .collect::<FxHashSet<_>>()
    });
    let mut candidates = Vec::new();

    for file in input.files {
        let semantic_tokens = normalize_and_hash_resolved(&file.file_tokens.tokens, semantic);
        let mut spans = file.file_tokens.function_spans.clone();
        spans.sort_by_key(|span| (span.start, span.end));
        spans.dedup();
        let line_table = line_table(&file.file_tokens.source);
        let focused = normalized_focus
            .as_ref()
            .is_some_and(|focus| focus.contains(dunce::simplified(&file.path)));

        for span in spans {
            let hashes = semantic_tokens
                .iter()
                .filter_map(|token| {
                    let source = file.file_tokens.tokens.get(token.original_index)?;
                    (span.start <= source.span.start && source.span.end <= span.end)
                        .then_some(token.hash)
                })
                .collect::<Vec<_>>();
            if hashes.len() < input.min_tokens {
                continue;
            }
            let Some(location) = function_location(&file.file_tokens.source, span, &line_table)
            else {
                continue;
            };
            if location.end_line.saturating_sub(location.start_line) + 1 < input.min_lines {
                continue;
            }
            let shingles = rolling_shingles(&hashes);
            if shingles.is_empty() {
                continue;
            }
            candidates.push(FunctionCandidate {
                file: file.path.clone(),
                span,
                signature: minhash_signature(&shingles),
                shingles,
                semantic_tokens: hashes,
                start_line: location.start_line,
                end_line: location.end_line,
                start_col: location.start_col,
                end_col: location.end_col,
                fragment: location.fragment,
                focused,
            });
        }
    }

    candidates.sort_by(|left, right| {
        left.file
            .cmp(&right.file)
            .then(left.span.start.cmp(&right.span.start))
            .then(left.span.end.cmp(&right.span.end))
    });
    candidates
}

struct FunctionLocation {
    start_line: usize,
    end_line: usize,
    start_col: usize,
    end_col: usize,
    fragment: String,
}

fn function_location(source: &str, span: Span, lines: &[usize]) -> Option<FunctionLocation> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end
        || end > source.len()
        || !source.is_char_boundary(start)
        || !source.is_char_boundary(end)
    {
        return None;
    }
    let (start_line, start_col) = byte_offset_to_line_col(source, start, lines);
    let (end_line, end_col) = byte_offset_to_line_col(source, end, lines);
    Some(FunctionLocation {
        start_line,
        end_line,
        start_col,
        end_col,
        fragment: source[start..end].to_string(),
    })
}

fn line_table(source: &str) -> Vec<usize> {
    source
        .bytes()
        .enumerate()
        .filter_map(|(index, byte)| (byte == b'\n').then_some(index))
        .collect()
}

fn byte_offset_to_line_col(source: &str, offset: usize, lines: &[usize]) -> (usize, usize) {
    let line_index = lines.partition_point(|&newline| newline < offset);
    let line_start = line_index
        .checked_sub(1)
        .map_or(0, |previous| lines[previous] + 1);
    (line_index + 1, source[line_start..offset].chars().count())
}

fn rolling_shingles(tokens: &[u64]) -> Vec<u64> {
    if tokens.is_empty() {
        return Vec::new();
    }
    let window = SHINGLE_TOKENS.min(tokens.len());
    let mut power = 1_u64;
    for _ in 1..window {
        power = power.wrapping_mul(SHINGLE_BASE);
    }
    let mut hash = 0_u64;
    for token in &tokens[..window] {
        hash = hash.wrapping_mul(SHINGLE_BASE).wrapping_add(*token);
    }
    let mut shingles = vec![hash];
    for index in window..tokens.len() {
        let outgoing = tokens[index - window].wrapping_mul(power);
        hash = hash
            .wrapping_sub(outgoing)
            .wrapping_mul(SHINGLE_BASE)
            .wrapping_add(tokens[index]);
        shingles.push(hash);
    }
    shingles.sort_unstable();
    shingles.dedup();
    shingles
}

fn minhash_signature(shingles: &[u64]) -> [u64; MINHASH_VALUES] {
    std::array::from_fn(|index| {
        let seed = MINHASH_SEED_BASE.wrapping_add(
            u64::try_from(index)
                .unwrap_or(u64::MAX)
                .wrapping_mul(MINHASH_SEED_STEP),
        );
        shingles
            .iter()
            .map(|shingle| mix64(*shingle ^ seed))
            .min()
            .unwrap_or(u64::MAX)
    })
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn candidate_components(
    candidates: &[FunctionCandidate],
    similarities: &mut FxHashMap<(usize, usize), f64>,
    budget: &mut CheckBudget,
) -> UnionFind {
    let mut buckets: FxHashMap<BandKey, BandBucket> = FxHashMap::default();
    let mut union_find = UnionFind::new(candidates.len());

    for candidate_id in 0..candidates.len() {
        let candidate = &candidates[candidate_id];
        let mut collisions: FxHashMap<usize, usize> = FxHashMap::default();
        let keys = band_keys(&candidate.signature);
        for key in &keys {
            if let Some(bucket) = buckets.get(key) {
                budget.skip(bucket.omitted);
                for &other_id in &bucket.members {
                    *collisions.entry(other_id).or_insert(0) += 1;
                }
            }
        }
        let mut ranked = collisions.into_iter().collect::<Vec<_>>();
        ranked.sort_by_key(|(other_id, bands)| (Reverse(*bands), *other_id));
        if ranked.len() > MAX_CANDIDATES_PER_FUNCTION {
            budget.skip(ranked.len() - MAX_CANDIDATES_PER_FUNCTION);
            ranked.truncate(MAX_CANDIDATES_PER_FUNCTION);
        }

        for (other_id, _) in ranked {
            if spans_overlap(candidate, &candidates[other_id]) {
                continue;
            }
            let Some(similarity) =
                checked_similarity(candidate_id, other_id, candidates, similarities, budget)
            else {
                continue;
            };
            if similarity >= MIN_SIMILARITY {
                union_find.union(candidate_id, other_id);
            }
        }

        for key in keys {
            let bucket = buckets.entry(key).or_default();
            if bucket.members.len() < MAX_BUCKET_MEMBERS {
                bucket.members.push(candidate_id);
            } else {
                bucket.omitted = bucket.omitted.saturating_add(1);
            }
        }
    }
    union_find
}

fn band_keys(signature: &[u64; MINHASH_VALUES]) -> [BandKey; MINHASH_BANDS] {
    std::array::from_fn(|band| {
        let start = band * MINHASH_ROWS;
        BandKey([
            signature[start],
            signature[start + 1],
            signature[start + 2],
            signature[start + 3],
        ])
    })
}

fn spans_overlap(left: &FunctionCandidate, right: &FunctionCandidate) -> bool {
    left.file == right.file
        && left.span.start.max(right.span.start) < left.span.end.min(right.span.end)
}

fn checked_similarity(
    left: usize,
    right: usize,
    candidates: &[FunctionCandidate],
    similarities: &mut FxHashMap<(usize, usize), f64>,
    budget: &mut CheckBudget,
) -> Option<f64> {
    let key = ordered_pair(left, right);
    if let Some(similarity) = similarities.get(&key) {
        return Some(*similarity);
    }
    if !budget.charge() {
        return None;
    }
    let similarity = jaccard(&candidates[left].shingles, &candidates[right].shingles);
    similarities.insert(key, similarity);
    Some(similarity)
}

const fn ordered_pair(left: usize, right: usize) -> (usize, usize) {
    if left < right {
        (left, right)
    } else {
        (right, left)
    }
}

fn jaccard(left: &[u64], right: &[u64]) -> f64 {
    let mut left_index = 0;
    let mut right_index = 0;
    let mut intersection = 0_usize;
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => {
                intersection += 1;
                left_index += 1;
                right_index += 1;
            }
        }
    }
    let union = left.len() + right.len() - intersection;
    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

fn collect_components(union_find: &mut UnionFind, len: usize) -> Vec<Vec<usize>> {
    let mut by_root: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for member in 0..len {
        by_root
            .entry(union_find.find(member))
            .or_default()
            .push(member);
    }
    let mut components = by_root
        .into_values()
        .filter(|component| component.len() >= 2)
        .collect::<Vec<_>>();
    components.sort_by_key(|component| component[0]);
    components
}

fn complete_link_clusters(
    components: Vec<Vec<usize>>,
    candidates: &[FunctionCandidate],
    similarities: &mut FxHashMap<(usize, usize), f64>,
    budget: &mut CheckBudget,
) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    for component in components {
        let members = remove_nested_members(component, candidates);
        let mut clusters: Vec<Vec<usize>> = Vec::new();
        for member in members {
            let mut target = None;
            for (index, cluster) in clusters.iter().enumerate() {
                if cluster.len() >= MAX_CLUSTER_MEMBERS {
                    budget.skip(cluster.len());
                    continue;
                }
                if cluster.iter().all(|&other| {
                    checked_similarity(member, other, candidates, similarities, budget)
                        .is_some_and(|similarity| similarity >= MIN_SIMILARITY)
                }) {
                    target = Some(index);
                    break;
                }
            }
            if let Some(index) = target {
                clusters[index].push(member);
            } else {
                clusters.push(vec![member]);
            }
        }
        result.extend(clusters.into_iter().filter(|cluster| cluster.len() >= 2));
    }
    result
}

/// Recompute the derived metrics of a near group after instance filtering.
pub(super) fn refresh_near_group_metrics(group: &mut CloneGroup) {
    if !matches!(group.kind(), CloneGroupKind::Near { .. }) || group.instances.len() < 2 {
        return;
    }

    let semantic =
        ResolvedNormalization::resolve(DetectionMode::Semantic, &NormalizationConfig::default());
    let token_sequences = group
        .instances
        .iter()
        .map(|instance| {
            let tokens = tokenize_fragment(
                &instance.file,
                &instance.fragment,
                FragmentTokenizationStrategy::Function,
            );
            normalize_and_hash_resolved(&tokens.tokens, semantic)
                .into_iter()
                .map(|token| token.hash)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let shingles = token_sequences
        .iter()
        .map(|tokens| rolling_shingles(tokens))
        .collect::<Vec<_>>();

    group.token_count = token_sequences.iter().map(Vec::len).min().unwrap_or(0);
    group.line_count = group
        .instances
        .iter()
        .map(|instance| {
            instance
                .end_line
                .saturating_sub(instance.start_line)
                .saturating_add(1)
        })
        .max()
        .unwrap_or(0);
    group.similarity = shingles
        .iter()
        .enumerate()
        .flat_map(|(index, left)| {
            shingles[index + 1..]
                .iter()
                .map(move |right| jaccard(left, right))
        })
        .reduce(f64::min);
}

fn remove_nested_members(mut members: Vec<usize>, candidates: &[FunctionCandidate]) -> Vec<usize> {
    members.sort_by_key(|&id| (Reverse(candidates[id].semantic_tokens.len()), id));
    let mut kept = Vec::new();
    for member in members {
        if kept
            .iter()
            .all(|&other| !spans_overlap(&candidates[member], &candidates[other]))
        {
            kept.push(member);
        }
    }
    kept.sort_unstable();
    kept
}

fn build_clone_group(
    cluster: &[usize],
    candidates: &[FunctionCandidate],
    similarities: &FxHashMap<(usize, usize), f64>,
    skip_local: bool,
) -> Option<NearCloneGroup> {
    if skip_local {
        let directories = cluster
            .iter()
            .filter_map(|&id| candidates[id].file.parent())
            .map(Path::to_path_buf)
            .collect::<FxHashSet<_>>();
        if directories.len() < 2 {
            return None;
        }
    }
    let similarity = cluster
        .iter()
        .enumerate()
        .flat_map(|(index, &left)| {
            cluster[index + 1..]
                .iter()
                .map(move |&right| *similarities.get(&ordered_pair(left, right)).unwrap_or(&0.0))
        })
        .reduce(f64::min)?;
    if similarity < MIN_SIMILARITY {
        return None;
    }
    let token_count = cluster
        .iter()
        .map(|&id| candidates[id].semantic_tokens.len())
        .min()?;
    let line_count = cluster
        .iter()
        .map(|&id| {
            candidates[id]
                .end_line
                .saturating_sub(candidates[id].start_line)
                + 1
        })
        .max()?;
    let instances = cluster
        .iter()
        .map(|&id| {
            let candidate = &candidates[id];
            CloneInstance {
                file: candidate.file.clone(),
                start_line: candidate.start_line,
                end_line: candidate.end_line,
                start_col: candidate.start_col,
                end_col: candidate.end_col,
                fragment: candidate.fragment.clone(),
            }
        })
        .collect();
    Some(NearCloneGroup {
        instances,
        token_count,
        line_count,
        similarity,
    })
}

fn sort_groups(groups: &mut [NearCloneGroup]) {
    groups.sort_by(
        |left, right| match (left.instances.first(), right.instances.first()) {
            (Some(left), Some(right)) => left
                .file
                .cmp(&right.file)
                .then(left.start_line.cmp(&right.start_line))
                .then(left.start_col.cmp(&right.start_col)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        },
    );
}

/// Remove near groups already represented by one exact group covering every
/// near instance. Partial exact sub-clones do not suppress a gapped function.
pub(super) fn suppress_exact_covered_near_groups(
    near_groups: &mut Vec<NearCloneGroup>,
    exact_groups: &[CloneGroup],
) {
    near_groups.retain(|near| {
        !exact_groups.iter().any(|exact| {
            near.instances.iter().all(|near_instance| {
                exact.instances.iter().any(|exact_instance| {
                    exact_instance.file == near_instance.file
                        && location_starts_before(exact_instance, near_instance)
                        && location_ends_after(exact_instance, near_instance)
                })
            })
        })
    });
}

fn location_starts_before(left: &CloneInstance, right: &CloneInstance) -> bool {
    (left.start_line, left.start_col) <= (right.start_line, right.start_col)
}

fn location_ends_after(left: &CloneInstance, right: &CloneInstance) -> bool {
    (left.end_line, left.end_col) >= (right.end_line, right.end_col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::normalize::normalize_and_hash_resolved;
    use crate::duplicates::tokenize::tokenize_file;

    fn tokenized(path: &str, source: &str, exact_semantic: bool) -> TokenizedFile {
        let file_tokens = tokenize_file(Path::new(path), source, false);
        let mode = if exact_semantic {
            DetectionMode::Semantic
        } else {
            DetectionMode::Mild
        };
        let normalization = ResolvedNormalization::resolve(mode, &NormalizationConfig::default());
        let hashed_tokens = normalize_and_hash_resolved(&file_tokens.tokens, normalization);
        TokenizedFile {
            path: PathBuf::from(path),
            hashed_tokens,
            file_tokens,
            metadata: None,
            cache_hit: false,
            suppressions: Vec::new(),
        }
    }

    fn run(files: &[TokenizedFile], min_tokens: usize) -> NearDetectionResult {
        detect_near_clones(&NearDetectionInput {
            files,
            min_tokens,
            min_lines: 3,
            skip_local: false,
            focus_files: None,
        })
    }

    fn function(name: &str, changed: bool) -> String {
        let middle = if changed {
            "const offset = normalized > 10 ? 3 : 2;\n  const adjusted = normalized + offset;"
        } else {
            "const adjusted = normalized + 2;"
        };
        format!(
            "export function {name}(input: number): number {{\n  const doubled = input * 2;\n  const normalized = doubled + input;\n  {middle}\n  const bounded = Math.max(adjusted, 0);\n  const rounded = Math.round(bounded);\n  const weighted = rounded * normalized;\n  const clamped = Math.min(weighted, 1000);\n  const staged = clamped + doubled;\n  const balanced = staged - input;\n  const projected = balanced * 2;\n  const limited = Math.min(projected, 2000);\n  const restored = limited + normalized;\n  const checked = Math.max(restored, doubled);\n  const combined = checked + bounded;\n  const smoothed = Math.round(combined / 2);\n  const finalized = smoothed + staged;\n  return finalized + normalized;\n}}\n"
        )
    }

    #[test]
    fn detects_semantic_gapped_functions_under_mild_exact_mode() {
        let files = vec![
            tokenized("src/a.ts", &function("calculate", false), false),
            tokenized("src/b.ts", &function("compute", true), false),
        ];
        let result = run(&files, 20);

        assert_eq!(result.clone_groups.len(), 1);
        let group = &result.clone_groups[0];
        assert_eq!(group.instances.len(), 2);
        assert!(group.similarity >= MIN_SIMILARITY);
    }

    #[test]
    fn rejects_functions_below_exact_jaccard_threshold() {
        let left = function("calculate", false);
        let right = "export function unrelated(value: string): string {\n  const pieces = value.split(',');\n  pieces.reverse();\n  const joined = pieces.join(':');\n  console.log(joined);\n  return joined.toUpperCase();\n}\n";
        let files = vec![
            tokenized("src/a.ts", &left, false),
            tokenized("src/b.ts", right, false),
        ];

        assert!(run(&files, 20).clone_groups.is_empty());
    }

    #[test]
    fn requires_a_focus_anchor_without_dropping_siblings() {
        let files = vec![
            tokenized("src/a.ts", &function("a", false), false),
            tokenized("src/b.ts", &function("b", true), false),
        ];
        let unrelated = FxHashSet::from_iter([PathBuf::from("src/unrelated.ts")]);
        let no_anchor = detect_near_clones(&NearDetectionInput {
            files: &files,
            min_tokens: 20,
            min_lines: 3,
            skip_local: false,
            focus_files: Some(&unrelated),
        });
        assert!(no_anchor.clone_groups.is_empty());

        let focus = FxHashSet::from_iter([PathBuf::from("src/a.ts")]);
        let anchored = detect_near_clones(&NearDetectionInput {
            files: &files,
            min_tokens: 20,
            min_lines: 3,
            skip_local: false,
            focus_files: Some(&focus),
        });
        assert_eq!(anchored.clone_groups[0].instances.len(), 2);
    }

    #[test]
    fn complete_link_splits_transitive_similarity() {
        let candidate = |file: &str, values: &[u64]| FunctionCandidate {
            file: PathBuf::from(file),
            span: Span::new(0, 10),
            semantic_tokens: values.to_vec(),
            shingles: values.to_vec(),
            signature: minhash_signature(values),
            start_line: 1,
            end_line: 5,
            start_col: 0,
            end_col: 1,
            fragment: String::new(),
            focused: false,
        };
        let candidates = vec![
            candidate("a.ts", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            candidate("b.ts", &[1, 2, 3, 4, 5, 6, 7, 8, 9, 11]),
            candidate("c.ts", &[1, 2, 3, 4, 5, 6, 7, 8, 11, 12]),
        ];
        let mut similarities = FxHashMap::default();
        similarities.insert((0, 1), 0.8);
        similarities.insert((1, 2), 0.8);
        let mut budget = CheckBudget::default();
        let clusters = complete_link_clusters(
            vec![vec![0, 1, 2]],
            &candidates,
            &mut similarities,
            &mut budget,
        );

        assert_eq!(clusters, vec![vec![0, 1]]);
        assert!(similarities[&(0, 2)] < MIN_SIMILARITY);
    }

    #[test]
    fn cluster_member_cap_reports_skipped_pair_work() {
        let candidates = (0..=MAX_CLUSTER_MEMBERS)
            .map(|index| {
                let shingles = vec![1, 2, 3, 4, 5, 6, 7];
                FunctionCandidate {
                    file: PathBuf::from(format!("src/{index}.ts")),
                    span: Span::new(0, 10),
                    semantic_tokens: shingles.clone(),
                    signature: minhash_signature(&shingles),
                    shingles,
                    start_line: 1,
                    end_line: 5,
                    start_col: 0,
                    end_col: 1,
                    fragment: String::new(),
                    focused: false,
                }
            })
            .collect::<Vec<_>>();
        let mut similarities = FxHashMap::default();
        let mut budget = CheckBudget::default();
        let clusters = complete_link_clusters(
            vec![(0..candidates.len()).collect()],
            &candidates,
            &mut similarities,
            &mut budget,
        );

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].len(), MAX_CLUSTER_MEMBERS);
        assert_eq!(budget.skipped, MAX_CLUSTER_MEMBERS);
    }

    #[test]
    fn refresh_near_metrics_uses_surviving_instances() {
        let source = function("same", false);
        let mut group = CloneGroup {
            instances: vec![
                CloneInstance {
                    file: PathBuf::from("src/a.ts"),
                    start_line: 1,
                    end_line: 20,
                    start_col: 0,
                    end_col: 1,
                    fragment: source.clone(),
                },
                CloneInstance {
                    file: PathBuf::from("src/b.ts"),
                    start_line: 3,
                    end_line: 18,
                    start_col: 0,
                    end_col: 1,
                    fragment: source,
                },
            ],
            token_count: 0,
            line_count: 0,
            similarity: Some(MIN_SIMILARITY),
        };

        refresh_near_group_metrics(&mut group);

        assert!(group.token_count > 0);
        assert_eq!(group.line_count, 20);
        assert_eq!(group.similarity, Some(1.0));
    }

    #[test]
    fn refresh_near_metrics_parses_embedded_language_fragments() {
        let source = function("same", false);
        for extension in ["vue", "svelte", "astro"] {
            let mut group = CloneGroup {
                instances: ["a", "b"]
                    .into_iter()
                    .map(|name| CloneInstance {
                        file: PathBuf::from(format!("src/{name}.{extension}")),
                        start_line: 1,
                        end_line: 20,
                        start_col: 0,
                        end_col: 1,
                        fragment: source.clone(),
                    })
                    .collect(),
                token_count: 0,
                line_count: 0,
                similarity: Some(MIN_SIMILARITY),
            };

            refresh_near_group_metrics(&mut group);

            assert!(group.token_count > 0, "{extension} fragment must tokenize");
            assert_eq!(group.similarity, Some(1.0));
        }
    }

    #[test]
    fn exact_group_must_cover_every_near_instance() {
        let instance = |file: &str, start_line, end_line| CloneInstance {
            file: PathBuf::from(file),
            start_line,
            end_line,
            start_col: 0,
            end_col: 20,
            fragment: String::new(),
        };
        let near = NearCloneGroup {
            instances: vec![instance("a.ts", 2, 8), instance("b.ts", 12, 18)],
            token_count: 30,
            line_count: 7,
            similarity: 0.9,
        };
        let partial = CloneGroup {
            instances: vec![instance("a.ts", 2, 8), instance("b.ts", 13, 18)],
            token_count: 20,
            line_count: 6,
            similarity: None,
        };
        let covering = CloneGroup {
            instances: vec![instance("a.ts", 1, 9), instance("b.ts", 11, 19)],
            token_count: 40,
            line_count: 9,
            similarity: None,
        };

        let mut groups = vec![near];
        suppress_exact_covered_near_groups(&mut groups, &[partial]);
        assert_eq!(groups.len(), 1);
        suppress_exact_covered_near_groups(&mut groups, &[covering]);
        assert!(groups.is_empty());
    }

    #[test]
    fn candidate_work_is_bounded_for_common_buckets() {
        let source = function("same", false);
        let files = (0..300)
            .map(|index| tokenized(&format!("src/{index}.ts"), &source, false))
            .collect::<Vec<_>>();
        let result = run(&files, 20);

        assert!(result.skipped_candidates > 0);
        assert!(
            result
                .clone_groups
                .iter()
                .all(|group| group.instances.len() <= MAX_CLUSTER_MEMBERS)
        );
    }
}
