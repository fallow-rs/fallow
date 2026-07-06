//! Step 2: Text preparation, concatenate ranked token sequences with sentinels.

/// Concatenate all ranked token sequences into a single `Vec<i64>`,
/// inserting unique negative sentinel values between files.
///
/// Returns `(text, file_of, file_offsets)` where:
/// - `text` is the concatenated sequence
/// - `file_of[pos]` maps a position in `text` to a file index
///   (`usize::MAX` for sentinel positions)
/// - `file_offsets[file_id]` is the starting position of file `file_id`
///   in `text`
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
