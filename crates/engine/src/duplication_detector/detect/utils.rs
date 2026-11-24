//! Byte offsets and report coordinates for clone detection.

/// One source position in byte offsets and report coordinates.
#[derive(Clone, Copy)]
pub(super) struct SourcePosition {
    pub(super) offset: usize,
    pub(super) line: usize,
    pub(super) column: usize,
}

impl SourcePosition {
    pub(super) fn at(source: &str, byte_offset: usize, line_table: &[usize]) -> Self {
        let offset = source.floor_char_boundary(byte_offset);
        let line_idx = line_table.partition_point(|&newline| newline < offset);
        let line_start = if line_idx == 0 {
            0
        } else {
            line_table[line_idx - 1] + 1
        };
        Self {
            offset,
            line: line_idx + 1,
            column: offset - line_start,
        }
    }
}

#[cfg(test)]
pub(super) fn byte_offset_to_line_col_fast(
    source: &str,
    byte_offset: usize,
    line_table: &[usize],
) -> (usize, usize) {
    let position = SourcePosition::at(source, byte_offset, line_table);
    (position.line, position.column)
}

/// Convert a byte offset into a 1-based line number and 0-based byte column.
#[cfg(test)]
pub(super) fn byte_offset_to_line_col(source: &str, byte_offset: usize) -> (usize, usize) {
    let mut offset = byte_offset.min(source.len());
    while offset > 0 && !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |pos| pos + 1);
    let col = offset - line_start;
    (line, col)
}
