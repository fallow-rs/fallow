/// Lower a Sass/Less source into standard CSS whose rules and declarations sit
/// on their source lines and columns, so CSS metric positions map straight
/// back onto the preprocessor file (or the SFC it was padded from).
pub(super) fn preprocessor_virtual_stylesheet(source: &str) -> Option<String> {
    let clean = strip_preprocessor_comments(source);
    let mut out = SourceAlignedWriter::new(&clean);
    render_preprocessor_children(&clean, 0, clean.len(), &mut out);
    let output = out.output;
    (!output.trim().is_empty()).then_some(output)
}

struct SourceAlignedWriter {
    line_starts: Vec<usize>,
    output: String,
    line: usize,
}

impl SourceAlignedWriter {
    fn new(source: &str) -> Self {
        let line_starts = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter(|&(_, byte)| byte == b'\n')
                    .map(|(index, _)| index + 1),
            )
            .collect();
        Self {
            line_starts,
            output: String::new(),
            line: 0,
        }
    }

    /// Write `text` at the source position of `offset`. Output never moves
    /// backwards: when the writer is already past that position, `text`
    /// follows after one space.
    fn write_at(&mut self, offset: usize, text: &str) {
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let column = offset - self.line_starts[line];
        while self.line < line {
            self.output.push('\n');
            self.line += 1;
        }
        let current = self.output.len() - self.output.rfind('\n').map_or(0, |at| at + 1);
        if self.line == line && current < column {
            self.output.extend(std::iter::repeat_n(' ', column - current));
        } else if current > 0 {
            self.output.push(' ');
        }
        self.write(text);
    }

    fn write(&mut self, text: &str) {
        self.output.push_str(text);
        self.line += text.bytes().filter(|&byte| byte == b'\n').count();
    }

    fn checkpoint(&self) -> (usize, usize) {
        (self.output.len(), self.line)
    }

    fn rollback(&mut self, (len, line): (usize, usize)) {
        self.output.truncate(len);
        self.line = line;
    }
}

fn trimmed_start_offset(source: &str, start: usize, end: usize) -> usize {
    let raw = &source[start..end];
    start + (raw.len() - raw.trim_start().len())
}

fn strip_preprocessor_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'/') {
            out.push_str(&source[cursor..i]);
            out.push_str("  ");
            i += 2;
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(' ');
                i += 1;
            }
            cursor = i;
            continue;
        }
        i += 1;
    }
    out.push_str(&source[cursor..]);
    out
}

fn render_preprocessor_children(
    source: &str,
    start: usize,
    end: usize,
    out: &mut SourceAlignedWriter,
) {
    let bytes = source.as_bytes();
    let mut statement_start = start;
    let mut i = start;
    while i < end {
        if bytes[i] == b'{' {
            let Some(close) = find_matching_brace(source, i, end) else {
                return;
            };
            let block = PreprocessorBlock {
                prelude: source[statement_start..i].trim(),
                prelude_offset: trimmed_start_offset(source, statement_start, i),
                body_start: i + 1,
                body_end: close,
            };
            render_preprocessor_block(source, &block, out);
            i = close + 1;
            statement_start = i;
        } else if bytes[i] == b';' {
            i += 1;
            statement_start = i;
        } else {
            i += 1;
        }
    }
}

struct PreprocessorBlock<'a> {
    prelude: &'a str,
    prelude_offset: usize,
    body_start: usize,
    body_end: usize,
}

fn render_preprocessor_block(
    source: &str,
    block: &PreprocessorBlock<'_>,
    out: &mut SourceAlignedWriter,
) {
    let prelude = block.prelude;
    if prelude.is_empty()
        || prelude.contains("#{")
        || prelude.starts_with("@mixin")
        || prelude.starts_with("@function")
        || prelude.starts_with("@for")
        || prelude.starts_with("@each")
        || prelude.starts_with("@if")
        || prelude.starts_with("@else")
        || prelude.starts_with("@while")
    {
        return;
    }
    let checkpoint = out.checkpoint();
    if prelude.starts_with("@media")
        || prelude.starts_with("@supports")
        || prelude.starts_with("@container")
        || prelude.starts_with("@layer")
    {
        out.write_at(block.prelude_offset, &format!("{prelude} {{"));
        let body_start = out.output.len();
        render_preprocessor_children(source, block.body_start, block.body_end, out);
        if out.output[body_start..].trim().is_empty() {
            out.rollback(checkpoint);
            return;
        }
        out.write(" }");
        return;
    }
    if prelude.starts_with('@') || prelude.ends_with(':') {
        return;
    }

    let Some(selectors) = clean_preprocessor_selector_list(prelude) else {
        return;
    };
    out.write_at(block.prelude_offset, &format!("{selectors} {{"));
    let body_start = out.output.len();
    render_preprocessor_body(source, block.body_start, block.body_end, out);
    if out.output[body_start..].trim().is_empty() {
        out.rollback(checkpoint);
        return;
    }
    out.write(" }");
}

/// Declarations are emitted before nested rules, as CSS nesting expects. A
/// declaration written after a nested rule in the source therefore cannot keep
/// its own line and follows the ones before it.
fn render_preprocessor_body(
    source: &str,
    body_start: usize,
    body_end: usize,
    out: &mut SourceAlignedWriter,
) {
    let bytes = source.as_bytes();
    let mut declarations = Vec::new();
    let mut children = Vec::new();
    let mut statement_start = body_start;
    let mut i = body_start;
    while i < body_end {
        match bytes[i] {
            b'{' => {
                let Some(close) = find_matching_brace(source, i, body_end) else {
                    break;
                };
                children.push(PreprocessorBlock {
                    prelude: source[statement_start..i].trim(),
                    prelude_offset: trimmed_start_offset(source, statement_start, i),
                    body_start: i + 1,
                    body_end: close,
                });
                i = close + 1;
                statement_start = i;
            }
            b';' => {
                let statement = source[statement_start..=i].trim();
                if let Some(declaration) = normalize_preprocessor_declaration(statement) {
                    declarations.push((
                        trimmed_start_offset(source, statement_start, i),
                        declaration,
                    ));
                }
                i += 1;
                statement_start = i;
            }
            _ => i += 1,
        }
    }
    let first_child_offset = children
        .first()
        .map_or(usize::MAX, |child| child.prelude_offset);
    for (offset, declaration) in declarations {
        if offset < first_child_offset {
            out.write_at(offset, &declaration);
        } else {
            out.write(" ");
            out.write(&declaration.replace(['\r', '\n'], " "));
        }
    }
    for child in &children {
        render_preprocessor_block(source, child, out);
    }
}

fn clean_preprocessor_selector_list(prelude: &str) -> Option<String> {
    let children: Vec<&str> = prelude
        .split(',')
        .map(str::trim)
        .filter(|selector| {
            !selector.is_empty()
                && !selector.contains("#{")
                && !selector.starts_with('@')
                && !selector.ends_with(':')
        })
        .collect();
    if children.is_empty() {
        None
    } else {
        Some(children.join(", "))
    }
}

fn normalize_preprocessor_declaration(statement: &str) -> Option<String> {
    let statement = statement.trim().trim_end_matches(';').trim();
    if statement.is_empty()
        || statement.starts_with('$')
        || statement.starts_with("@include")
        || statement.starts_with("@extend")
        || statement.starts_with("@debug")
        || statement.starts_with("@warn")
        || statement.starts_with("@error")
        || statement.contains("#{")
    {
        return None;
    }
    let (property, value) = statement.split_once(':')?;
    let property = property.trim();
    let value = value.trim();
    if property.is_empty() || value.is_empty() || property.starts_with('@') {
        return None;
    }
    Some(format!(
        "{property}: {};",
        normalize_preprocessor_value(value)
    ))
}

fn normalize_preprocessor_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut cursor = 0;
    let mut i = 0;
    while i < bytes.len() {
        if (bytes[i] == b'$' || bytes[i] == b'@') && is_preprocessor_ident_start(bytes.get(i + 1)) {
            out.push_str(&value[cursor..i]);
            out.push_str("var(--fallow-preprocessor-var)");
            i += 2;
            while i < bytes.len() && is_preprocessor_ident_continue(bytes[i]) {
                i += 1;
            }
            cursor = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&value[cursor..]);
    out
}

fn is_preprocessor_ident_start(byte: Option<&u8>) -> bool {
    byte.is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_' || *b == b'-')
}

fn is_preprocessor_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn find_matching_brace(source: &str, open: usize, limit: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < limit {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "tests use unwrap to keep fixtures concise"
)]
mod tests {
    use super::*;

    fn line_of(output: &str, needle: &str) -> usize {
        output
            .lines()
            .position(|line| line.contains(needle))
            .map(|index| index + 1)
            .unwrap()
    }

    #[test]
    fn rules_and_declarations_keep_source_lines_and_columns() {
        let source = "// a\n\n.card {\n\n  .title {\n    color: red;\n  }\n}\n";
        let output = preprocessor_virtual_stylesheet(source).unwrap();
        assert_eq!(line_of(&output, ".card"), 3);
        assert_eq!(line_of(&output, ".title"), 5);
        assert_eq!(line_of(&output, "color: red"), 6);
        assert_eq!(output.lines().nth(4).unwrap().find(".title"), Some(2));
    }

    #[test]
    fn media_blocks_keep_source_lines() {
        let source = ".a {\n  color: red;\n\n  @media (min-width: 1px) {\n    .b {\n      color: blue;\n    }\n  }\n}\n";
        let output = preprocessor_virtual_stylesheet(source).unwrap();
        assert_eq!(line_of(&output, "@media"), 4);
        assert_eq!(line_of(&output, ".b"), 5);
    }

    #[test]
    fn empty_blocks_leave_no_output() {
        let source = "$x: 1px;\n@media (min-width: 1px) {\n  .a {\n    @include m;\n  }\n}\n";
        assert_eq!(preprocessor_virtual_stylesheet(source), None);
    }

    #[test]
    fn declaration_after_nested_rule_stays_in_its_rule() {
        let source =
            ".a {\n  .b {\n    color: red;\n  }\n  margin: 0;\n}\n.c {\n  color: blue;\n}\n";
        let output = preprocessor_virtual_stylesheet(source).unwrap();
        let analytics = fallow_extract::compute_css_analytics(&output).unwrap();
        assert_eq!(analytics.total_declarations, 3, "{output}");
        assert_eq!(line_of(&output, ".c"), 7);
    }

    #[test]
    fn multiline_declaration_after_nested_rule_keeps_rule_lines() {
        let source = ".a {\n  .b { color: red; }\n  background: linear-gradient(\n    red,\n    blue\n  );\n}\n.c {\n  color: blue;\n}\n";
        let output = preprocessor_virtual_stylesheet(source).unwrap();
        assert_eq!(line_of(&output, ".b"), 2, "{output}");
        assert_eq!(line_of(&output, ".c"), 8, "{output}");
        let analytics = fallow_extract::compute_css_analytics(&output).unwrap();
        assert_eq!(analytics.total_declarations, 3, "{output}");
    }

    #[test]
    fn later_rule_on_a_line_keeps_its_source_column() {
        let source = ".a { color: red; }      .b { color: blue; }\n";
        let output = preprocessor_virtual_stylesheet(source).unwrap();
        assert_eq!(output.find(".b"), source.find(".b"), "{output}");
    }

    #[test]
    fn block_comment_before_a_selector_keeps_the_selector_line() {
        let source = "/* Header */\n.header {\n  color: red;\n}\n";
        let output = preprocessor_virtual_stylesheet(source).unwrap();
        assert_eq!(line_of(&output, ".header"), 2, "{output}");
        assert_eq!(line_of(&output, "color: red"), 3, "{output}");
    }
}
