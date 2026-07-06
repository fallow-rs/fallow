//! Utility functions for clone instance construction and byte-to-line mapping.

use super::FileData;
use crate::duplicates::types::CloneInstance;

/// Build a `CloneInstance` using a pre-computed line offset table for fast lookup.
pub(super) fn build_clone_instance_fast(
    file: &FileData,
    token_offset: usize,
    token_length: usize,
    line_table: &[usize],
) -> Option<CloneInstance> {
    let tokens = &file.hashed_tokens;
    let source_tokens = &file.file_tokens.tokens;

    if token_offset + token_length > tokens.len() {
        return None;
    }

    let first_hashed = &tokens[token_offset];
    let last_hashed = &tokens[token_offset + token_length - 1];

    let first_source = &source_tokens[first_hashed.original_index];
    let last_source = &source_tokens[last_hashed.original_index];

    let start_byte = first_source.span.start as usize;
    let end_byte = last_source.span.end as usize;

    if start_byte > end_byte {
        return None;
    }

    let source = &file.file_tokens.source;
    let (start_line, start_col) = byte_offset_to_line_col_fast(source, start_byte, line_table);
    let (end_line, end_col) = byte_offset_to_line_col_fast(source, end_byte, line_table);

    let fragment = if end_byte <= source.len() {
        let mut sb = start_byte;
        while sb > 0 && !source.is_char_boundary(sb) {
            sb -= 1;
        }
        let mut eb = end_byte;
        while eb < source.len() && !source.is_char_boundary(eb) {
            eb += 1;
        }
        source[sb..eb].to_string()
    } else {
        String::new()
    };

    Some(CloneInstance {
        file: file.path.clone(),
        start_line,
        end_line,
        start_col,
        end_col,
        fragment,
    })
}

/// Convert a byte offset into a 1-based line number and 0-based character column
/// using a pre-computed table of newline positions for O(log L) lookup.
pub(super) fn byte_offset_to_line_col_fast(
    source: &str,
    byte_offset: usize,
    line_table: &[usize],
) -> (usize, usize) {
    let mut offset = byte_offset.min(source.len());
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let line_idx = line_table.partition_point(|&nl_pos| nl_pos < offset);
    let line = line_idx + 1; // 1-based
    let line_start = if line_idx == 0 {
        0
    } else {
        line_table[line_idx - 1] + 1
    };
    let col = source[line_start..offset].chars().count();
    (line, col)
}

/// Convert a byte offset into a 1-based line number and 0-based character column.
#[cfg(test)]
pub(super) fn byte_offset_to_line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let mut offset = byte_offset.min(source.len());
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |pos| pos + 1);
    let col = before[line_start..].chars().count();
    (line, col)
}
