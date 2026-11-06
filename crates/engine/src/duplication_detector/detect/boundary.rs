use super::FileData;
use crate::duplicates::tokenize::TokenKind;

pub(super) fn build_boundary_prefixes(files: &[FileData]) -> Vec<Option<Vec<u32>>> {
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

/// Replace each section boundary token in `text` with a unique negative
/// sentinel, below every value already in `text`.
///
/// A component file (Vue, Svelte, Astro) holds one boundary token before each
/// section. With equal boundary values, the suffix array finds repeats that
/// run through several sections. The extractor drops each instance that holds
/// a boundary, and a shorter repeat inside one section is not maximal, so it
/// gets no group. Unique values stop each repeat at the section border, so
/// each section of identical files forms its own maximal repeat.
pub(super) fn isolate_boundaries(text: &mut [i64], files: &[FileData], file_offsets: &[usize]) {
    let mut sentinel = text.iter().copied().min().unwrap_or(0).min(0) - 1;
    for (file, &start) in files.iter().zip(file_offsets) {
        for (offset, token) in file.hashed_tokens.iter().enumerate() {
            if hashed_token_is_boundary(file, token.original_index) {
                text[start + offset] = sentinel;
                sentinel -= 1;
            }
        }
    }
}

fn hashed_token_is_boundary(file: &FileData, original_index: usize) -> bool {
    file.file_tokens
        .tokens
        .get(original_index)
        .is_some_and(|source| matches!(source.kind, TokenKind::Boundary(_)))
}

pub(super) fn range_contains_boundary(
    prefix: Option<&Vec<u32>>,
    offset: usize,
    length: usize,
) -> bool {
    prefix.is_some_and(|prefix| prefix[offset] != prefix[offset + length])
}
