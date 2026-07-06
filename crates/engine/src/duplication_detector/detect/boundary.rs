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
