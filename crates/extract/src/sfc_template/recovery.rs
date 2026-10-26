use super::scanners::{scan_curly_section, scan_html_tag};

/// Reuse suffix results when malformed markup makes a caller retry at later bytes.
pub(super) struct RecoveryScanner<'a> {
    source: &'a str,
    ends: Option<Vec<SectionEnds>>,
}

impl<'a> RecoveryScanner<'a> {
    pub(super) fn new(source: &'a str) -> Self {
        Self { source, ends: None }
    }

    pub(super) fn html_tag(&mut self, start: usize) -> Option<(&'a str, usize)> {
        if let Some(ends) = &self.ends {
            let end = ends[start + 1].tag;
            return (end < self.source.len()).then(|| (&self.source[start..=end], end + 1));
        }
        let result = scan_html_tag(self.source, start);
        if result.is_none() {
            self.ends = Some(section_ends(self.source));
        }
        result
    }

    pub(super) fn curly_section(
        &mut self,
        start: usize,
        brace_len: usize,
    ) -> Option<(&'a str, usize)> {
        if let Some(ends) = &self.ends {
            let body_start = start + brace_len;
            let end = if brace_len == 1 {
                ends[body_start].curly
            } else {
                ends[body_start].interpolation
            };
            return (end < self.source.len())
                .then(|| (&self.source[body_start..end], end + brace_len));
        }
        let result = scan_curly_section(self.source, start, brace_len, brace_len);
        if result.is_none() {
            self.ends = Some(section_ends(self.source));
        }
        result
    }
}

#[derive(Clone, Copy)]
struct SectionEnds {
    curly: usize,
    interpolation: usize,
    tag: usize,
}

impl SectionEnds {
    const fn missing(end: usize) -> Self {
        Self {
            curly: end,
            interpolation: end,
            tag: end,
        }
    }
}

// Recovery can restart inside an earlier quote or comment. Resolve suffixes
// from a fresh lexical state so those retries keep their original boundaries.
fn section_ends(source: &str) -> Vec<SectionEnds> {
    let bytes = source.as_bytes();
    let missing = source.len();
    let mut ends = vec![SectionEnds::missing(missing); source.len() + 2];
    let mut quotes = [missing; 3];
    let mut escaped_quotes = quotes;
    let mut newline = missing;
    let mut block_end = missing;
    let mut block_after_next = missing;

    for index in (0..bytes.len()).rev() {
        let byte = bytes[index];
        let next = ends[index + 1];
        let quote = match byte {
            b'\'' => Some(quotes[0]),
            b'"' => Some(quotes[1]),
            b'`' => Some(quotes[2]),
            _ => None,
        };
        let expression = if let Some(close) = quote {
            ends[close + 1]
        } else if bytes[index..].starts_with(b"//") {
            ends[newline + 1]
        } else if bytes[index..].starts_with(b"/*") {
            ends[block_after_next]
        } else {
            match byte {
                b'{' => ends[next.curly + 1],
                b'}' => SectionEnds {
                    curly: index,
                    interpolation: if bytes.get(index + 1) == Some(&b'}') {
                        index
                    } else {
                        next.interpolation
                    },
                    tag: missing,
                },
                _ => next,
            }
        };
        let tag = match byte {
            b'\'' => ends[quotes[0] + 1].tag,
            b'"' => ends[quotes[1] + 1].tag,
            b'{' => ends[next.curly + 1].tag,
            b'>' => index,
            _ => next.tag,
        };
        ends[index] = SectionEnds { tag, ..expression };

        let previous_quotes = quotes;
        for (slot, marker) in [b'\'', b'"', b'`'].into_iter().enumerate() {
            if byte == b'\\' {
                quotes[slot] = escaped_quotes[slot];
            } else if byte == marker {
                quotes[slot] = index;
            }
        }
        escaped_quotes = previous_quotes;
        if byte == b'\n' {
            newline = index;
        }
        let previous_block = block_end;
        if bytes[index..].starts_with(b"*/") {
            block_end = index + 2;
        }
        block_after_next = previous_block;
    }
    ends
}

#[cfg(test)]
mod tests {
    use super::RecoveryScanner;

    #[test]
    fn recovery_preserves_nested_sections_quotes_and_comments() {
        for (section, braces, expression) in [
            ("{ ready }", 1, " ready "),
            ("{ /* } */ ready }", 1, " /* } */ ready "),
            ("{ /*/ } */ ready }", 1, " /*/ } */ ready "),
            ("{ // }\n ready }", 1, " // }\n ready "),
            ("{ '\\'' + `}` + \"é\" }", 1, " '\\'' + `}` + \"é\" "),
            ("{{ {nested: \"}\"} }}", 2, " {nested: \"}\"} "),
            ("{{ } ready }}", 2, " } ready "),
        ] {
            let prefix = "<broken{{{{ ";
            let tag = "<Used value=\"a > b\"/>";
            let source = format!("{prefix}{section}{tag}");
            let mut scanner = RecoveryScanner::new(&source);
            assert!(scanner.html_tag(0).is_none());
            assert_eq!(
                scanner.curly_section(prefix.len(), braces),
                Some((expression, prefix.len() + section.len()))
            );
            assert_eq!(
                scanner.html_tag(prefix.len() + section.len()),
                Some((tag, source.len()))
            );
        }
    }
}
