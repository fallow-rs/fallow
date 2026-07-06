//! Step 1: Rank reduction, map u64 hashes to consecutive u32 ranks.

use rustc_hash::FxHashMap;

use super::FileData;

/// Map all unique token hashes (u64) to consecutive integer ranks (u32).
///
/// Returns `ranked_files` where `ranked_files[i]` contains the rank
/// sequence for `files[i]`.
///
/// Uses `u32` ranks, which limits the number of distinct tokens to ~4.3
/// billion. This is safe for JS/TS codebases (even very large monorepos
/// have at most tens of millions of tokens).
pub(super) fn rank_reduce(files: &[FileData]) -> Vec<Vec<u32>> {
    let total: usize = files.iter().map(|f| f.hashed_tokens.len()).sum();
    let mut hash_to_rank: FxHashMap<u64, u32> =
        FxHashMap::with_capacity_and_hasher(total / 2, rustc_hash::FxBuildHasher);
    let mut next_rank: u32 = 0;

    files
        .iter()
        .map(|file| {
            file.hashed_tokens
                .iter()
                .map(|ht| match hash_to_rank.entry(ht.hash) {
                    std::collections::hash_map::Entry::Occupied(e) => *e.get(),
                    std::collections::hash_map::Entry::Vacant(e) => {
                        let r = next_rank;
                        next_rank += 1;
                        *e.insert(r)
                    }
                })
                .collect()
        })
        .collect()
}
