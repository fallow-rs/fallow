//! Duplicate-analysis prefilters based on k-token shingles.

use rustc_hash::{FxHashSet, FxHasher};
use std::hash::Hasher;
use std::path::PathBuf;

use super::{TokenizedFile, normalize::HashedToken};

const DEFAULT_SHINGLE_TOKENS: usize = 7;
const GLOBAL_SHINGLE_BASE: u64 = 1_000_003;

pub(super) fn filter_to_duplicate_candidates(files: &mut Vec<TokenizedFile>, min_tokens: usize) {
    let window = min_tokens.max(1);
    let duplicate_shingles = find_duplicate_shingles(files, window);
    if duplicate_shingles.is_empty() {
        files.clear();
        return;
    }

    let before = files.len();
    files.retain(|file| {
        has_matching_rolling_shingle(&file.hashed_tokens, window, &duplicate_shingles)
    });
    tracing::debug!(
        candidates_kept = files.len(),
        candidates_skipped = before.saturating_sub(files.len()),
        shingle_window = window,
        "duplication shingle prefilter"
    );
}

pub(super) fn filter_to_focus_candidates(
    files: &mut Vec<TokenizedFile>,
    focus_files: &FxHashSet<PathBuf>,
    min_tokens: usize,
) {
    let window = min_tokens.clamp(1, DEFAULT_SHINGLE_TOKENS);
    let normalized_focus: FxHashSet<PathBuf> = focus_files
        .iter()
        .map(|p| dunce::simplified(p).to_path_buf())
        .collect();
    let path_is_focus = |path: &std::path::Path| normalized_focus.contains(dunce::simplified(path));
    let mut focus_shingles = FxHashSet::default();
    for file in files.iter().filter(|file| path_is_focus(&file.path)) {
        insert_shingles(&file.hashed_tokens, window, &mut focus_shingles);
    }
    if focus_shingles.is_empty() {
        return;
    }

    let mut candidates_kept = 0usize;
    let mut candidates_skipped = 0usize;
    files.retain(|file| {
        if path_is_focus(&file.path) {
            return true;
        }
        let keep = has_matching_shingle(&file.hashed_tokens, window, &focus_shingles);
        if keep {
            candidates_kept += 1;
        } else {
            candidates_skipped += 1;
        }
        keep
    });
    tracing::debug!(
        candidates_kept,
        candidates_skipped,
        shingle_window = window,
        "focused duplication shingle prefilter"
    );
}

fn insert_shingles(tokens: &[HashedToken], window: usize, out: &mut FxHashSet<u64>) {
    if tokens.len() < window {
        return;
    }
    for shingle in tokens.windows(window) {
        out.insert(hash_shingle(shingle));
    }
}

fn find_duplicate_shingles(files: &[TokenizedFile], window: usize) -> FxHashSet<u64> {
    let mut seen = FxHashSet::default();
    let mut duplicates = FxHashSet::default();

    for file in files {
        if file.hashed_tokens.len() < window {
            continue;
        }

        for_each_rolling_shingle_hash(&file.hashed_tokens, window, |hash| {
            if !seen.insert(hash) {
                duplicates.insert(hash);
            }
        });
    }

    duplicates
}

fn for_each_rolling_shingle_hash(
    tokens: &[HashedToken],
    window: usize,
    mut visit: impl FnMut(u64),
) {
    if tokens.len() < window {
        return;
    }

    let mut base_power = 1_u64;
    for _ in 1..window {
        base_power = base_power.wrapping_mul(GLOBAL_SHINGLE_BASE);
    }

    let mut hash = 0_u64;
    for token in &tokens[..window] {
        hash = hash
            .wrapping_mul(GLOBAL_SHINGLE_BASE)
            .wrapping_add(token.hash);
    }
    visit(hash);

    for i in window..tokens.len() {
        let outgoing = tokens[i - window].hash.wrapping_mul(base_power);
        hash = hash
            .wrapping_sub(outgoing)
            .wrapping_mul(GLOBAL_SHINGLE_BASE)
            .wrapping_add(tokens[i].hash);
        visit(hash);
    }
}

fn has_matching_shingle(
    tokens: &[HashedToken],
    window: usize,
    focus_shingles: &FxHashSet<u64>,
) -> bool {
    tokens.len() >= window
        && tokens
            .windows(window)
            .any(|shingle| focus_shingles.contains(&hash_shingle(shingle)))
}

fn has_matching_rolling_shingle(
    tokens: &[HashedToken],
    window: usize,
    duplicate_shingles: &FxHashSet<u64>,
) -> bool {
    let mut matches = false;
    for_each_rolling_shingle_hash(tokens, window, |hash| {
        matches |= duplicate_shingles.contains(&hash);
    });
    matches
}

fn hash_shingle(tokens: &[HashedToken]) -> u64 {
    let mut hasher = FxHasher::default();
    for token in tokens {
        hasher.write_u64(token.hash);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::tokenize::FileTokens;
    use proptest::prelude::*;

    fn file(path: &str, hashes: &[u64]) -> TokenizedFile {
        TokenizedFile {
            path: PathBuf::from(path),
            hashed_tokens: hashes
                .iter()
                .enumerate()
                .map(|(original_index, &hash)| HashedToken {
                    hash,
                    original_index,
                })
                .collect(),
            file_tokens: FileTokens {
                tokens: Vec::new(),
                atomic_invocation_spans: Vec::new(),
                source: String::new(),
                line_count: 1,
            },
            metadata: None,
            cache_hit: false,
            suppressions: Vec::new(),
        }
    }

    #[test]
    fn keeps_focus_and_matching_candidates_only() {
        let mut files = vec![
            file("focus.ts", &[1, 2, 3, 4, 5]),
            file("candidate.ts", &[9, 1, 2, 3, 8]),
            file("unrelated.ts", &[10, 11, 12, 13, 14]),
        ];
        let focus = FxHashSet::from_iter([PathBuf::from("focus.ts")]);

        filter_to_focus_candidates(&mut files, &focus, 3);

        let paths = files
            .into_iter()
            .map(|file| file.path)
            .collect::<FxHashSet<_>>();
        assert!(paths.contains(&PathBuf::from("focus.ts")));
        assert!(paths.contains(&PathBuf::from("candidate.ts")));
        assert!(!paths.contains(&PathBuf::from("unrelated.ts")));
    }

    #[test]
    fn keeps_only_files_with_duplicate_shingles() {
        let mut files = vec![
            file("a.ts", &[1, 2, 3, 4, 9]),
            file("b.ts", &[8, 1, 2, 3, 7]),
            file("unique.ts", &[10, 11, 12, 13, 14]),
        ];

        filter_to_duplicate_candidates(&mut files, 3);

        let paths = files
            .into_iter()
            .map(|file| file.path)
            .collect::<FxHashSet<_>>();
        assert!(paths.contains(&PathBuf::from("a.ts")));
        assert!(paths.contains(&PathBuf::from("b.ts")));
        assert!(!paths.contains(&PathBuf::from("unique.ts")));
    }

    #[test]
    fn keeps_file_with_internal_duplicate_shingles() {
        let mut files = vec![
            file("self.ts", &[1, 2, 3, 8, 1, 2, 3]),
            file("unique.ts", &[10, 11, 12, 13, 14]),
        ];

        filter_to_duplicate_candidates(&mut files, 3);

        let paths = files
            .into_iter()
            .map(|file| file.path)
            .collect::<FxHashSet<_>>();
        assert!(paths.contains(&PathBuf::from("self.ts")));
        assert!(!paths.contains(&PathBuf::from("unique.ts")));
    }

    proptest! {
        #[test]
        fn keeps_files_that_share_a_focus_shingle(
            shared in prop::collection::vec(1_u64..1_000, 5..20),
            focus_prefix in prop::collection::vec(10_000_u64..20_000, 0..8),
            focus_suffix in prop::collection::vec(20_000_u64..30_000, 0..8),
            match_prefix in prop::collection::vec(30_000_u64..40_000, 0..8),
            match_suffix in prop::collection::vec(40_000_u64..50_000, 0..8),
            noise in prop::collection::vec(50_000_u64..60_000, 5..20),
        ) {
            let mut focus_hashes = focus_prefix;
            focus_hashes.extend(shared.iter().copied());
            focus_hashes.extend(focus_suffix);

            let mut match_hashes = match_prefix;
            match_hashes.extend(shared);
            match_hashes.extend(match_suffix);

            let mut files = vec![
                file("focus.ts", &focus_hashes),
                file("matching.ts", &match_hashes),
                file("noise.ts", &noise),
            ];
            let focus = FxHashSet::from_iter([PathBuf::from("focus.ts")]);

            filter_to_focus_candidates(&mut files, &focus, 5);

            let kept = files
                .into_iter()
                .map(|file| file.path)
                .collect::<FxHashSet<_>>();
            prop_assert!(kept.contains(&PathBuf::from("focus.ts")));
            prop_assert!(kept.contains(&PathBuf::from("matching.ts")));
        }
    }
}
