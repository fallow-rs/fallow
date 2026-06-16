pub(super) fn scan_curly_section(
    source: &str,
    start: usize,
    opening_len: usize,
    closing_len: usize,
) -> Option<(&str, usize)> {
    debug_assert!(opening_len == 1 || opening_len == 2);
    debug_assert!(closing_len == 1 || closing_len == 2);

    scan_delimited_section(source, start, opening_len, closing_len, b'{', b'}')
}

pub(super) fn scan_bracket_section(source: &str, start: usize) -> Option<(&str, usize)> {
    scan_delimited_section(source, start, 1, 1, b'[', b']')
}

fn scan_delimited_section(
    source: &str,
    start: usize,
    opening_len: usize,
    closing_len: usize,
    open_byte: u8,
    close_byte: u8,
) -> Option<(&str, usize)> {
    debug_assert_eq!(source.as_bytes().get(start), Some(&open_byte));

    let bytes = source.as_bytes();
    let mut index = start + opening_len;
    let mut nested_delimiters = 0_u32;
    let mut state = DelimitedScanState::default();

    while index < bytes.len() {
        let byte = bytes[index];

        if let Some(next_index) = state.consume_context(bytes, index, byte) {
            index = next_index;
            continue;
        }

        if let Some(next_index) = state.start_comment(bytes, index, byte) {
            index = next_index;
            continue;
        }

        match byte {
            b'\'' => {
                state.in_single = true;
            }
            b'"' => {
                state.in_double = true;
            }
            b'`' => {
                state.in_backtick = true;
            }
            b if b == open_byte => nested_delimiters += 1,
            b if b == close_byte => {
                if nested_delimiters == 0 {
                    if closing_len == 1 {
                        return Some((&source[start + opening_len..index], index + 1));
                    }
                    if bytes.get(index + 1) == Some(&close_byte) {
                        return Some((&source[start + opening_len..index], index + 2));
                    }
                } else {
                    nested_delimiters -= 1;
                }
            }
            _ => {}
        }

        index += 1;
    }

    None
}

#[derive(Default)]
struct DelimitedScanState {
    in_single: bool,
    in_double: bool,
    in_backtick: bool,
    escape: bool,
    line_comment: bool,
    block_comment: bool,
}

impl DelimitedScanState {
    fn consume_context(&mut self, bytes: &[u8], index: usize, byte: u8) -> Option<usize> {
        if self.line_comment {
            if byte == b'\n' {
                self.line_comment = false;
            }
            return Some(index + 1);
        }
        if self.block_comment {
            return Some(self.consume_block_comment(bytes, index, byte));
        }
        if self.escape {
            self.escape = false;
            return Some(index + 1);
        }
        if self.in_single {
            return Some(self.consume_single_quote(index, byte));
        }
        if self.in_double {
            return Some(self.consume_double_quote(index, byte));
        }
        if self.in_backtick {
            return Some(self.consume_backtick(index, byte));
        }
        None
    }

    fn start_comment(&mut self, bytes: &[u8], index: usize, byte: u8) -> Option<usize> {
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            self.line_comment = true;
            return Some(index + 2);
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            self.block_comment = true;
            return Some(index + 2);
        }
        None
    }

    fn consume_block_comment(&mut self, bytes: &[u8], index: usize, byte: u8) -> usize {
        if byte == b'*' && bytes.get(index + 1) == Some(&b'/') {
            self.block_comment = false;
            index + 2
        } else {
            index + 1
        }
    }

    fn consume_single_quote(&mut self, index: usize, byte: u8) -> usize {
        if byte == b'\\' {
            self.escape = true;
        } else if byte == b'\'' {
            self.in_single = false;
        }
        index + 1
    }

    fn consume_double_quote(&mut self, index: usize, byte: u8) -> usize {
        if byte == b'\\' {
            self.escape = true;
        } else if byte == b'"' {
            self.in_double = false;
        }
        index + 1
    }

    fn consume_backtick(&mut self, index: usize, byte: u8) -> usize {
        if byte == b'\\' {
            self.escape = true;
        } else if byte == b'`' {
            self.in_backtick = false;
        }
        index + 1
    }
}

pub(super) fn scan_html_tag(source: &str, start: usize) -> Option<(&str, usize)> {
    debug_assert_eq!(source.as_bytes().get(start), Some(&b'<'));

    let bytes = source.as_bytes();
    let mut index = start + 1;
    let mut in_single = false;
    let mut in_double = false;
    let mut escape = false;

    while index < bytes.len() {
        let byte = bytes[index];
        if escape {
            escape = false;
            index += 1;
            continue;
        }

        if in_single {
            if byte == b'\\' {
                escape = true;
            } else if byte == b'\'' {
                in_single = false;
            }
            index += 1;
            continue;
        }

        if in_double {
            if byte == b'\\' {
                escape = true;
            } else if byte == b'"' {
                in_double = false;
            }
            index += 1;
            continue;
        }

        if byte == b'{' {
            let (_, next_index) = scan_curly_section(source, index, 1, 1)?;
            index = next_index;
            continue;
        }

        match byte {
            b'\'' => {
                in_single = true;
                index += 1;
            }
            b'"' => {
                in_double = true;
                index += 1;
            }
            b'>' => return Some((&source[start..=index], index + 1)),
            _ => index += 1,
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{scan_bracket_section, scan_curly_section, scan_html_tag};

    #[test]
    fn scans_svelte_brace_section_with_nested_literals() {
        let source = "{handler({ key: `}` })}";
        let (inner, next_index) = scan_curly_section(source, 0, 1, 1).expect("brace section");
        assert_eq!(inner, "handler({ key: `}` })");
        assert_eq!(next_index, source.len());
    }

    #[test]
    fn scans_vue_interpolation_with_nested_comments() {
        let source = "{{ format(/* } */ value) }}";
        let (inner, next_index) = scan_curly_section(source, 0, 2, 2).expect("interpolation");
        assert_eq!(inner, " format(/* } */ value) ");
        assert_eq!(next_index, source.len());
    }

    #[test]
    fn scans_curly_sections_with_quoted_braces() {
        let source = r#"{format("}")}"#;
        let (inner, next_index) = scan_curly_section(source, 0, 1, 1).expect("expression");
        assert_eq!(inner, r#"format("}")"#);
        assert_eq!(next_index, source.len());
    }

    #[test]
    fn scans_curly_sections_with_ternary_empty_string_branch() {
        let source = "{cond ? inTernary() : ''}</p>";
        let (inner, next_index) = scan_curly_section(source, 0, 1, 1).expect("expression");
        assert_eq!(inner, "cond ? inTernary() : ''");
        assert_eq!(next_index, "{cond ? inTernary() : ''}".len());
    }

    #[test]
    fn scans_bracket_sections_with_nested_indexing() {
        let source = r#"[fieldMap[current["name"]]]"#;
        let (inner, next_index) = scan_bracket_section(source, 0).expect("bracket section");
        assert_eq!(inner, r#"fieldMap[current["name"]]"#);
        assert_eq!(next_index, source.len());
    }

    #[test]
    fn scans_html_tags_with_quoted_angle_brackets() {
        let source = r#"<Comp title="a > b" data-id='x>y'>"#;
        let (tag, next_index) = scan_html_tag(source, 0).expect("tag");
        assert_eq!(tag, source);
        assert_eq!(next_index, source.len());
    }

    #[test]
    fn scans_html_tags_with_braced_expressions() {
        let source = r"<button disabled={count > limit}>{label}</button>";
        let (tag, next_index) = scan_html_tag(source, 0).expect("tag");
        assert_eq!(tag, r"<button disabled={count > limit}>");
        assert_eq!(next_index, r"<button disabled={count > limit}>".len());
    }
}
