//! Text preparation for suffix-array duplicate detection.

use rustc_hash::FxHashMap;

use super::FileData;

pub(super) struct RankedText {
    pub(super) text: Vec<i64>,
    pub(super) file_of: Vec<usize>,
    pub(super) file_offsets: Vec<usize>,
    pub(super) unique_ranks: usize,
}

/// Rank token hashes and write the suffix-array text in one pass.
///
/// Ranks retain encounter order across files, while unique negative sentinels
/// preserve file boundaries. Writing the final buffers directly avoids the
/// intermediate per-file rank vectors used by the rolling detector.
pub(super) fn rank_and_concatenate(files: &[FileData]) -> RankedText {
    let token_count: usize = files.iter().map(|file| file.hashed_tokens.len()).sum();
    let total_len = token_count + files.len().saturating_sub(1);
    let mut hash_to_rank: FxHashMap<u64, u32> =
        FxHashMap::with_capacity_and_hasher(token_count / 2, rustc_hash::FxBuildHasher);
    let mut text = Vec::with_capacity(total_len);
    let mut file_of = Vec::with_capacity(total_len);
    let mut file_offsets = Vec::with_capacity(files.len());
    let mut next_rank = 0_u32;
    let mut sentinel = -1_i64;

    for (file_id, file) in files.iter().enumerate() {
        file_offsets.push(text.len());

        for token in &file.hashed_tokens {
            let rank = match hash_to_rank.entry(token.hash) {
                std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let rank = next_rank;
                    next_rank += 1;
                    *entry.insert(rank)
                }
            };
            text.push(i64::from(rank));
            file_of.push(file_id);
        }

        if file_id + 1 < files.len() {
            text.push(sentinel);
            file_of.push(usize::MAX);
            sentinel -= 1;
        }
    }

    RankedText {
        text,
        file_of,
        file_offsets,
        unique_ranks: next_rank as usize,
    }
}

/// Concatenate all ranked token sequences into a single `Vec<i64>`,
/// inserting unique negative sentinel values between files.
///
/// Returns `(text, file_of, file_offsets)` where:
/// - `text` is the concatenated sequence
/// - `file_of[pos]` maps a position in `text` to a file index
///   (`usize::MAX` for sentinel positions)
/// - `file_offsets[file_id]` is the starting position of file `file_id`
///   in `text`
#[cfg(test)]
pub(super) fn concatenate_with_sentinels(
    ranked_files: &[Vec<u32>],
) -> (Vec<i64>, Vec<usize>, Vec<usize>) {
    let sentinel_count = ranked_files.len().saturating_sub(1);
    let total_len: usize = ranked_files.iter().map(Vec::len).sum::<usize>() + sentinel_count;

    let mut text = Vec::with_capacity(total_len);
    let mut file_of = Vec::with_capacity(total_len);
    let mut file_offsets = Vec::with_capacity(ranked_files.len());

    let mut sentinel: i64 = -1;

    for (file_id, ranks) in ranked_files.iter().enumerate() {
        file_offsets.push(text.len());

        for &r in ranks {
            text.push(i64::from(r));
            file_of.push(file_id);
        }

        if file_id + 1 < ranked_files.len() {
            text.push(sentinel);
            file_of.push(usize::MAX);
            sentinel -= 1;
        }
    }

    (text, file_of, file_offsets)
}
