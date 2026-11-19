//! Trim the leading `;` tokens of each raw clone group.
//!
//! A `;` ends the statement or declaration before it. When that statement
//! differs between the instances, a clone that starts on the `;` reports the
//! line of the different statement as its first line.

use super::FileData;
use super::extraction::RawGroup;
use crate::duplicates::tokenize::{PunctuationType, TokenKind};

/// Drop the leading `;` tokens of each group, and each group that then has
/// fewer than `min_tokens` tokens.
pub(super) fn trim_leading_semicolons(
    mut raw_groups: Vec<RawGroup>,
    files: &[FileData],
    min_tokens: usize,
) -> Vec<RawGroup> {
    raw_groups.retain_mut(|group| {
        let Some(&(file_id, offset)) = group.instances.first() else {
            return true;
        };
        let leading = (0..group.length)
            .take_while(|&index| is_semicolon(&files[file_id], offset + index))
            .count();
        if leading == 0 {
            return true;
        }
        group.length -= leading;
        for instance in &mut group.instances {
            instance.1 += leading;
        }
        group.length >= min_tokens
    });
    raw_groups
}

/// Whether the hashed token at `index` of `file` is a `;`.
fn is_semicolon(file: &FileData, index: usize) -> bool {
    file.hashed_tokens
        .get(index)
        .and_then(|hashed| file.file_tokens.tokens.get(hashed.original_index))
        .is_some_and(|token| {
            matches!(
                token.kind,
                TokenKind::Punctuation(PunctuationType::Semicolon)
            )
        })
}
