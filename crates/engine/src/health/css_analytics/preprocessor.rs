use fallow_extract::css_metrics::{
    MAX_DECLARATION_BLOCKS, MAX_NOTABLE_RULES, MAX_RAW_STYLE_VALUES,
};

/// Upper bound on a compiled (flat) parent selector. Each `&` copies the whole
/// parent, so repeated `& &` nesting doubles it per level; past this size the
/// parent is treated as unknown and suffixes below it stay as written. Real
/// compiled selectors are tens to a few hundred bytes, so 4 KiB only cuts off
/// pathological input while keeping every resolved selector cheap to copy.
const MAX_COMPILED_SELECTOR_BYTES: usize = 4096;

/// Lower a Sass/Less source into standard CSS whose rules and declarations sit
/// on their source lines and columns, so CSS metric positions map straight
/// back onto the preprocessor file (or the SFC it was padded from).
///
/// Returns one stylesheet per layer, main layer first. A rule whose Sass
/// parent-suffix selector (`&__element`, `&--modifier`) resolves against a
/// known parent is written flat, as Sass compiles it, at the top level of the
/// next layer: CSS nesting would otherwise prefix it with an implicit
/// descendant `&`. The layers are analysed separately and merged with
/// [`merge_css_analytics`].
pub(super) fn preprocessor_virtual_stylesheets(source: &str) -> Vec<String> {
    let clean = strip_preprocessor_comments(source);
    let mut out = LayeredWriter::new(&clean);
    let context = RenderContext {
        layer: 0,
        parent: ParentSelector::Root,
        at_rules: Vec::new(),
    };
    render_preprocessor_children(&clean, 0, clean.len(), &context, &mut out);
    out.layers
        .into_iter()
        .map(|layer| layer.output)
        .filter(|output| !output.trim().is_empty())
        .collect()
}

/// Fold the analytics of another layer of the same stylesheet into `into`.
pub(super) fn merge_css_analytics(
    into: &mut fallow_types::extract::CssAnalytics,
    from: fallow_types::extract::CssAnalytics,
) {
    let fallow_types::extract::CssAnalytics {
        total_declarations,
        important_declarations,
        rule_count,
        empty_rule_count,
        max_nesting_depth,
        notable_rules,
        notable_truncated,
        colors,
        font_sizes,
        z_indexes,
        box_shadows,
        border_radii,
        line_heights,
        raw_style_values,
        custom_property_definitions,
        defined_custom_properties,
        referenced_custom_properties,
        defined_keyframes,
        referenced_keyframes,
        registered_custom_properties,
        declared_layers,
        populated_layers,
        defined_font_faces,
        referenced_font_families,
        declaration_blocks,
    } = from;
    into.total_declarations = into.total_declarations.saturating_add(total_declarations);
    into.important_declarations = into
        .important_declarations
        .saturating_add(important_declarations);
    into.rule_count = into.rule_count.saturating_add(rule_count);
    into.empty_rule_count = into.empty_rule_count.saturating_add(empty_rule_count);
    into.max_nesting_depth = into.max_nesting_depth.max(max_nesting_depth);
    into.notable_rules.extend(notable_rules);
    into.notable_rules.sort_by_key(|rule| (rule.line, rule.col));
    into.notable_truncated |= notable_truncated || into.notable_rules.len() > MAX_NOTABLE_RULES;
    into.notable_rules.truncate(MAX_NOTABLE_RULES);
    union_sorted(&mut into.colors, colors);
    union_sorted(&mut into.font_sizes, font_sizes);
    union_sorted(&mut into.z_indexes, z_indexes);
    union_sorted(&mut into.box_shadows, box_shadows);
    union_sorted(&mut into.border_radii, border_radii);
    union_sorted(&mut into.line_heights, line_heights);
    merge_raw_style_values(&mut into.raw_style_values, raw_style_values);
    into.custom_property_definitions
        .extend(custom_property_definitions);
    union_sorted(
        &mut into.defined_custom_properties,
        defined_custom_properties,
    );
    union_sorted(
        &mut into.referenced_custom_properties,
        referenced_custom_properties,
    );
    union_sorted(&mut into.defined_keyframes, defined_keyframes);
    union_sorted(&mut into.referenced_keyframes, referenced_keyframes);
    union_sorted(
        &mut into.registered_custom_properties,
        registered_custom_properties,
    );
    union_sorted(&mut into.declared_layers, declared_layers);
    union_sorted(&mut into.populated_layers, populated_layers);
    union_sorted(&mut into.defined_font_faces, defined_font_faces);
    union_sorted(&mut into.referenced_font_families, referenced_font_families);
    into.declaration_blocks.extend(declaration_blocks);
    into.declaration_blocks.sort_by_key(|block| block.line);
    into.declaration_blocks.truncate(MAX_DECLARATION_BLOCKS);
}

fn merge_raw_style_values(
    into: &mut Vec<fallow_types::extract::CssRawStyleValue>,
    from: Vec<fallow_types::extract::CssRawStyleValue>,
) {
    into.extend(from);
    into.sort_by_key(|value| value.line);
    let mut seen = rustc_hash::FxHashSet::default();
    into.retain(|value| {
        seen.insert((
            value.axis.clone(),
            value.property.clone(),
            value.value.clone(),
            value.line,
        ))
    });
    into.truncate(MAX_RAW_STYLE_VALUES);
}

fn union_sorted(into: &mut Vec<String>, from: Vec<String>) {
    into.extend(from);
    into.sort_unstable();
    into.dedup();
}

struct LayeredWriter {
    line_starts: Vec<usize>,
    layers: Vec<LayerOutput>,
}

#[derive(Default)]
struct LayerOutput {
    output: String,
    line: usize,
}

impl LayeredWriter {
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
            layers: Vec::new(),
        }
    }

    fn layer(&mut self, layer: usize) -> &mut LayerOutput {
        if self.layers.len() <= layer {
            self.layers.resize_with(layer + 1, LayerOutput::default);
        }
        &mut self.layers[layer]
    }

    /// Write `text` at the source position of `offset`. Output never moves
    /// backwards: when the layer is already past that position, `text`
    /// follows after one space.
    fn write_at(&mut self, layer: usize, offset: usize, text: &str) {
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let column = offset - self.line_starts[line];
        let out = self.layer(layer);
        while out.line < line {
            out.output.push('\n');
            out.line += 1;
        }
        let current = out.output.len() - out.output.rfind('\n').map_or(0, |at| at + 1);
        if out.line == line && current < column {
            out.output
                .extend(std::iter::repeat_n(' ', column - current));
        } else if current > 0 {
            out.output.push(' ');
        }
        self.write(layer, text);
    }

    /// Start of the source line above `offset`, when `layer` has not reached
    /// that line yet and can take text there without moving `offset`'s line.
    fn free_line_above(&mut self, layer: usize, offset: usize) -> Option<usize> {
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let above = line.checked_sub(1)?;
        (self.layer(layer).line <= above).then(|| self.line_starts[above])
    }

    fn write(&mut self, layer: usize, text: &str) {
        let out = self.layer(layer);
        out.output.push_str(text);
        out.line += text.bytes().filter(|&byte| byte == b'\n').count();
    }

    fn len(&mut self, layer: usize) -> usize {
        self.layer(layer).output.len()
    }

    fn written_since(&mut self, layer: usize, start: usize) -> bool {
        !self.layer(layer).output[start..].trim().is_empty()
    }

    fn checkpoint(&mut self, layer: usize) -> (usize, usize) {
        let out = self.layer(layer);
        (out.output.len(), out.line)
    }

    fn rollback(&mut self, layer: usize, (len, line): (usize, usize)) {
        let out = self.layer(layer);
        out.output.truncate(len);
        out.line = line;
    }
}

/// The compiled (flat) Sass selector of the enclosing rule, when it is known.
#[derive(Clone)]
enum ParentSelector {
    Root,
    Known(String),
    Unknown,
}

struct RenderContext<'a> {
    layer: usize,
    parent: ParentSelector,
    at_rules: Vec<&'a str>,
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

fn render_preprocessor_children<'a>(
    source: &'a str,
    start: usize,
    end: usize,
    context: &RenderContext<'a>,
    out: &mut LayeredWriter,
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
            render_preprocessor_block(source, &block, context, out);
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

fn render_preprocessor_block<'a>(
    source: &'a str,
    block: &PreprocessorBlock<'a>,
    context: &RenderContext<'a>,
    out: &mut LayeredWriter,
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
    let layer = context.layer;
    let checkpoint = out.checkpoint(layer);
    if prelude.starts_with("@media")
        || prelude.starts_with("@supports")
        || prelude.starts_with("@container")
        || prelude.starts_with("@layer")
    {
        out.write_at(layer, block.prelude_offset, &format!("{prelude} {{"));
        let body_start = out.len(layer);
        let mut at_rules = context.at_rules.clone();
        at_rules.push(prelude);
        let inner = RenderContext {
            layer,
            parent: context.parent.clone(),
            at_rules,
        };
        render_preprocessor_children(source, block.body_start, block.body_end, &inner, out);
        if !out.written_since(layer, body_start) {
            out.rollback(layer, checkpoint);
            return;
        }
        out.write(layer, " }");
        return;
    }
    if prelude.starts_with('@') || prelude.ends_with(':') {
        return;
    }

    let Some(selectors) = clean_preprocessor_selector_list(prelude) else {
        return;
    };
    let compiled = compiled_selector(&selectors, &context.parent);
    if has_parent_suffix(&selectors) {
        if let ParentSelector::Known(flat) = &compiled {
            let parent = ParentSelector::Known(flat.clone());
            render_hoisted_rule(source, block, flat, parent, context, out);
            return;
        }
        if let Some(flat) = compiled_selector_list(&selectors, &context.parent) {
            // A list cannot be pasted into a descendant's `&`, so descendants
            // stay unresolved.
            render_hoisted_rule(source, block, &flat, ParentSelector::Unknown, context, out);
            return;
        }
    }
    out.write_at(layer, block.prelude_offset, &format!("{selectors} {{"));
    let body_start = out.len(layer);
    let inner = RenderContext {
        layer,
        parent: compiled,
        at_rules: context.at_rules.clone(),
    };
    render_preprocessor_body(source, block.body_start, block.body_end, &inner, out);
    if !out.written_since(layer, body_start) {
        out.rollback(layer, checkpoint);
        return;
    }
    out.write(layer, " }");
}

/// Write a resolved parent-suffix rule flat at the top level of the next
/// layer, re-entering its enclosing at-rules. Its nesting depth restarts at 0,
/// as in the compiled stylesheet, where suffix concatenation adds no nesting.
fn render_hoisted_rule<'a>(
    source: &'a str,
    block: &PreprocessorBlock<'a>,
    flat: &str,
    parent: ParentSelector,
    context: &RenderContext<'a>,
    out: &mut LayeredWriter,
) {
    let layer = context.layer + 1;
    let checkpoint = out.checkpoint(layer);
    let wrapper_offset = if context.at_rules.is_empty() {
        block.prelude_offset
    } else {
        out.free_line_above(layer, block.prelude_offset)
            .unwrap_or(block.prelude_offset)
    };
    for at_rule in &context.at_rules {
        out.write_at(
            layer,
            wrapper_offset,
            &format!("{} {{", single_line(at_rule)),
        );
    }
    out.write_at(
        layer,
        block.prelude_offset,
        &format!("{} {{", single_line(flat)),
    );
    let body_start = out.len(layer);
    let inner = RenderContext {
        layer,
        parent,
        at_rules: context.at_rules.clone(),
    };
    render_preprocessor_body(source, block.body_start, block.body_end, &inner, out);
    if !out.written_since(layer, body_start) {
        out.rollback(layer, checkpoint);
        return;
    }
    out.write(layer, " }");
    for _ in &context.at_rules {
        out.write(layer, " }");
    }
}

/// Replayed at-rule preludes and inherited selector text belong to other source
/// lines, so they must not advance the writer's line.
fn single_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

/// The selector Sass compiles `selectors` to under `parent`, when it can be
/// derived without expanding selector lists.
fn compiled_selector(selectors: &str, parent: &ParentSelector) -> ParentSelector {
    let tokens = SelectorTokens::scan(selectors);
    if tokens.has_list_comma {
        return ParentSelector::Unknown;
    }
    match parent {
        ParentSelector::Unknown => ParentSelector::Unknown,
        ParentSelector::Root if !tokens.ampersands.is_empty() => ParentSelector::Unknown,
        ParentSelector::Root if selectors.len() > MAX_COMPILED_SELECTOR_BYTES => {
            ParentSelector::Unknown
        }
        ParentSelector::Root => ParentSelector::Known(selectors.to_owned()),
        ParentSelector::Known(parent) if tokens.ampersands.is_empty() => {
            let within_budget = parent
                .len()
                .checked_add(1 + selectors.len())
                .is_some_and(|len| len <= MAX_COMPILED_SELECTOR_BYTES);
            if !within_budget {
                return ParentSelector::Unknown;
            }
            ParentSelector::Known(format!("{parent} {selectors}"))
        }
        ParentSelector::Known(parent) => substitute_parent(selectors, &tokens, parent),
    }
}

/// The compiled list for a selector list under a known single parent, such as
/// BEM siblings `&__a, &__b`. Lists with brackets or quotes are left alone,
/// because a comma inside them does not separate list items.
fn compiled_selector_list(selectors: &str, parent: &ParentSelector) -> Option<String> {
    if !matches!(parent, ParentSelector::Known(_))
        || selectors.contains(['(', '[', '"', '\'', '\\'])
    {
        return None;
    }
    let mut items = Vec::new();
    for item in selectors.split(',') {
        match compiled_selector(item.trim(), parent) {
            ParentSelector::Known(flat) => items.push(flat),
            _ => return None,
        }
    }
    let flat = items.join(", ");
    (flat.len() <= MAX_COMPILED_SELECTOR_BYTES).then_some(flat)
}

fn substitute_parent(selectors: &str, tokens: &SelectorTokens, parent: &str) -> ParentSelector {
    let within_budget = parent
        .len()
        .checked_mul(tokens.ampersands.len())
        .and_then(|copies| copies.checked_add(selectors.len() - tokens.ampersands.len()))
        .is_some_and(|len| len <= MAX_COMPILED_SELECTOR_BYTES);
    if !within_budget {
        return ParentSelector::Unknown;
    }
    let suffixable = parent_accepts_suffix(parent);
    let mut resolved = String::with_capacity(selectors.len() + parent.len());
    let mut cursor = 0;
    for &position in &tokens.ampersands {
        if is_suffix_ampersand(selectors, position) && !suffixable {
            return ParentSelector::Unknown;
        }
        resolved.push_str(&selectors[cursor..position]);
        resolved.push_str(parent);
        cursor = position + 1;
    }
    resolved.push_str(&selectors[cursor..]);
    ParentSelector::Known(resolved)
}

/// Parent-selector `&` positions and list commas in a selector, skipping
/// quoted strings and backslash escapes, where both are literal text.
struct SelectorTokens {
    ampersands: Vec<usize>,
    has_list_comma: bool,
}

impl SelectorTokens {
    fn scan(selectors: &str) -> Self {
        let bytes = selectors.as_bytes();
        let mut ampersands = Vec::new();
        let mut has_list_comma = false;
        let mut quote = None;
        let mut i = 0;
        while i < bytes.len() {
            let byte = bytes[i];
            if byte == b'\\' {
                i += 2;
                continue;
            }
            if let Some(open) = quote {
                if byte == open {
                    quote = None;
                }
            } else {
                match byte {
                    b'"' | b'\'' => quote = Some(byte),
                    b'&' => ampersands.push(i),
                    b',' => has_list_comma = true,
                    _ => {}
                }
            }
            i += 1;
        }
        Self {
            ampersands,
            has_list_comma,
        }
    }
}

fn is_suffix_ampersand(selectors: &str, position: usize) -> bool {
    selectors[position + 1..]
        .chars()
        .next()
        .is_some_and(is_selector_ident_char)
}

/// Sass appends a suffix to the parent's last simple selector, which must be a
/// class, id or element name.
fn parent_accepts_suffix(parent: &str) -> bool {
    let compound = parent
        .rsplit(|ch: char| ch.is_whitespace() || matches!(ch, '>' | '+' | '~'))
        .next()
        .unwrap_or_default();
    if !compound.ends_with(is_selector_ident_char) {
        return false;
    }
    let last_simple = compound
        .rfind(['.', '#', ':', '['])
        .map_or(compound, |index| &compound[index..]);
    !last_simple.starts_with([':', '['])
}

fn has_parent_suffix(selectors: &str) -> bool {
    SelectorTokens::scan(selectors)
        .ampersands
        .into_iter()
        .any(|position| is_suffix_ampersand(selectors, position))
}

fn is_selector_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')
}

/// Declarations are emitted before nested rules, as CSS nesting expects. A
/// declaration written after a nested rule in the source therefore cannot keep
/// its own line and follows the ones before it.
fn render_preprocessor_body<'a>(
    source: &'a str,
    body_start: usize,
    body_end: usize,
    context: &RenderContext<'a>,
    out: &mut LayeredWriter,
) {
    let layer = context.layer;
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
            out.write_at(layer, offset, &declaration);
        } else {
            out.write(layer, " ");
            out.write(layer, &declaration.replace(['\r', '\n'], " "));
        }
    }
    for child in &children {
        render_preprocessor_block(source, child, context, out);
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

    fn main_layer(source: &str) -> String {
        preprocessor_virtual_stylesheets(source)
            .into_iter()
            .next()
            .unwrap()
    }

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
        let output = main_layer(source);
        assert_eq!(line_of(&output, ".card"), 3);
        assert_eq!(line_of(&output, ".title"), 5);
        assert_eq!(line_of(&output, "color: red"), 6);
        assert_eq!(output.lines().nth(4).unwrap().find(".title"), Some(2));
    }

    #[test]
    fn media_blocks_keep_source_lines() {
        let source = ".a {\n  color: red;\n\n  @media (min-width: 1px) {\n    .b {\n      color: blue;\n    }\n  }\n}\n";
        let output = main_layer(source);
        assert_eq!(line_of(&output, "@media"), 4);
        assert_eq!(line_of(&output, ".b"), 5);
    }

    #[test]
    fn empty_blocks_leave_no_output() {
        let source = "$x: 1px;\n@media (min-width: 1px) {\n  .a {\n    @include m;\n  }\n}\n";
        assert!(preprocessor_virtual_stylesheets(source).is_empty());
    }

    #[test]
    fn declaration_after_nested_rule_stays_in_its_rule() {
        let source =
            ".a {\n  .b {\n    color: red;\n  }\n  margin: 0;\n}\n.c {\n  color: blue;\n}\n";
        let output = main_layer(source);
        let analytics = fallow_extract::compute_css_analytics(&output).unwrap();
        assert_eq!(analytics.total_declarations, 3, "{output}");
        assert_eq!(line_of(&output, ".c"), 7);
    }

    #[test]
    fn multiline_declaration_after_nested_rule_keeps_rule_lines() {
        let source = ".a {\n  .b { color: red; }\n  background: linear-gradient(\n    red,\n    blue\n  );\n}\n.c {\n  color: blue;\n}\n";
        let output = main_layer(source);
        assert_eq!(line_of(&output, ".b"), 2, "{output}");
        assert_eq!(line_of(&output, ".c"), 8, "{output}");
        let analytics = fallow_extract::compute_css_analytics(&output).unwrap();
        assert_eq!(analytics.total_declarations, 3, "{output}");
    }

    fn lowered_analytics(source: &str) -> fallow_types::extract::CssAnalytics {
        let mut layers = preprocessor_virtual_stylesheets(source)
            .into_iter()
            .map(|layer| fallow_extract::compute_css_analytics(&layer).unwrap());
        let mut analytics = layers.next().unwrap();
        for layer in layers {
            merge_css_analytics(&mut analytics, layer);
        }
        analytics
    }

    type RuleShape = (usize, u8, u16, u16, u16, u16);

    fn rule_at(analytics: &fallow_types::extract::CssAnalytics, line: usize) -> RuleShape {
        let rule = analytics
            .notable_rules
            .iter()
            .find(|rule| rule.line as usize == line)
            .unwrap_or_else(|| panic!("no notable rule on line {line}: {analytics:?}"));
        (
            rule.line as usize,
            rule.nesting_depth,
            rule.specificity_a,
            rule.specificity_b,
            rule.specificity_c,
            rule.complexity,
        )
    }

    fn compiled_shape(flat_selector: &str, line: usize) -> RuleShape {
        let css = format!(
            "{}{flat_selector} {{ color: red !important; }}",
            "\n".repeat(line - 1)
        );
        let analytics = fallow_extract::compute_css_analytics(&css).unwrap();
        rule_at(&analytics, line)
    }

    #[test]
    fn standalone_suffix_scores_like_compiled_sass() {
        let source = ".card {\n  &__body {\n    color: red !important;\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 2),
            compiled_shape(".card__body", 2)
        );
    }

    #[test]
    fn suffix_with_pseudo_classes_scores_like_compiled_sass() {
        let source = ".card {\n  &__body:hover:focus {\n    color: red !important;\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 2),
            compiled_shape(".card__body:hover:focus", 2)
        );
    }

    #[test]
    fn chained_suffixes_score_like_compiled_sass() {
        let source = ".card {\n  &__body {\n    &--wide .x .y {\n      color: red !important;\n    }\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 3),
            compiled_shape(".card__body--wide .x .y", 3)
        );
    }

    #[test]
    fn suffix_keeps_every_compound_of_the_parent() {
        let source = "#app .card {\n  &__body {\n    color: red !important;\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 2),
            compiled_shape("#app .card__body", 2)
        );
    }

    #[test]
    fn suffix_state_to_element_scores_like_compiled_sass() {
        let source = ".b {\n  &:focus-within &__copy {\n    color: red !important;\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 2),
            compiled_shape(".b:focus-within .b__copy", 2)
        );
    }

    #[test]
    fn suffix_inside_media_keeps_its_line_and_wrapper() {
        let source = ".card {\n  @media (min-width: 1px) {\n\n    &__body {\n      color: red !important;\n    }\n  }\n}\n";
        let analytics = lowered_analytics(source);
        assert_eq!(rule_at(&analytics, 4), compiled_shape(".card__body", 4));
    }

    #[test]
    fn suffix_inside_media_keeps_its_source_column() {
        let source = ".card {\n  @media (min-width: 1px) {\n\n    &__body {\n      color: red !important;\n    }\n  }\n}\n";
        let rule = lowered_analytics(source)
            .notable_rules
            .into_iter()
            .find(|rule| rule.line == 4)
            .unwrap();
        assert_eq!(rule.col, 5);
    }

    #[test]
    fn suffix_list_scores_like_compiled_sass() {
        let source = ".card {\n  &__a, &__b {\n    color: red !important;\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 2),
            compiled_shape(".card__a, .card__b", 2)
        );
    }

    #[test]
    fn descendants_of_a_suffix_list_stay_unresolved() {
        let source = ".card {\n  &__a, &__b {\n    &--x {\n      color: red;\n    }\n  }\n}\n";
        let output = preprocessor_virtual_stylesheets(source).join("\n");
        assert!(output.contains("&--x {"), "{output}");
        assert!(!output.contains("card__a--x"), "{output}");
    }

    #[test]
    fn hoisted_rule_inside_a_layer_populates_it() {
        let source = "@layer ui {\n  .card {\n    &__body {\n      color: red;\n    }\n  }\n}\n";
        let analytics = lowered_analytics(source);
        assert_eq!(analytics.populated_layers, vec!["ui".to_owned()]);
        assert_eq!(analytics.total_declarations, 1);
    }

    #[test]
    fn parent_with_only_suffix_children_is_not_emitted() {
        let source =
            ".card {\n  &__body {\n    color: red;\n  }\n  &__foot {\n    color: blue;\n  }\n}\n";
        let analytics = lowered_analytics(source);
        assert_eq!(analytics.rule_count, 2);
        assert_eq!(analytics.empty_rule_count, 0);
    }

    #[test]
    fn suffix_under_a_parent_list_stays_unresolved() {
        let source = ".a, .b {\n  &__x {\n    color: red !important;\n  }\n}\n";
        let output = main_layer(source);
        assert!(output.contains("&__x {"), "{output}");
    }

    #[test]
    fn non_suffix_rules_keep_their_nested_metrics() {
        let source = ".card {\n  .title .a .b {\n    color: red !important;\n  }\n  &__body {\n    color: blue;\n  }\n}\n";
        let analytics = lowered_analytics(source);
        let nested = fallow_extract::compute_css_analytics(
            ".card {\n  .title .a .b {\n    color: red !important;\n  }\n}\n",
        )
        .unwrap();
        assert_eq!(rule_at(&analytics, 2), rule_at(&nested, 2));
        assert_eq!(analytics.total_declarations, 2);
    }

    fn assert_stays_nested(source: &str, line: usize) {
        let analytics = lowered_analytics(source);
        let nested = fallow_extract::compute_css_analytics(source).unwrap();
        assert_eq!(rule_at(&analytics, line), rule_at(&nested, line));
        assert_eq!(preprocessor_virtual_stylesheets(source).len(), 1);
    }

    #[test]
    fn double_quoted_ampersand_is_not_a_parent_suffix() {
        assert_stays_nested(
            ".card {\n  [data-label=\"&__x\"] .a .b .c {\n    color: red !important;\n  }\n}\n",
            2,
        );
    }

    #[test]
    fn single_quoted_ampersand_is_not_a_parent_suffix() {
        assert_stays_nested(
            ".card {\n  [data-label='&--y'] .a .b .c {\n    color: red !important;\n  }\n}\n",
            2,
        );
    }

    #[test]
    fn escaped_ampersand_is_not_a_parent_suffix() {
        assert_stays_nested(
            ".card {\n  .a\\&__x .b .c .d {\n    color: red !important;\n  }\n}\n",
            2,
        );
    }

    #[test]
    fn multiline_media_prelude_keeps_hoisted_rule_line() {
        let source = ".card {\n  @media (min-width: 1px)\n    and (max-width: 2px)\n    and (orientation: landscape) {\n    &__body .a .b .c {\n      color: red !important;\n    }\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 5),
            compiled_shape(".card__body .a .b .c", 5)
        );
    }

    #[test]
    fn multiline_supports_prelude_keeps_hoisted_rule_line() {
        let source = ".card {\n  @supports (display: grid)\n    and (gap: 1px) {\n    &__body .a .b .c {\n      color: red !important;\n    }\n  }\n}\n";
        assert_eq!(
            rule_at(&lowered_analytics(source), 4),
            compiled_shape(".card__body .a .b .c", 4)
        );
    }

    #[test]
    fn multiline_ancestor_selector_keeps_hoisted_rule_line() {
        let source = ".card\n  .body\n  .x {\n  &__a .a .b { color: red !important; }\n  &__b .a .b { color: red !important; }\n}\n";
        let analytics = lowered_analytics(source);
        assert_eq!(
            rule_at(&analytics, 4),
            compiled_shape(".card .body .x__a .a .b", 4)
        );
        assert_eq!(
            rule_at(&analytics, 5),
            compiled_shape(".card .body .x__b .a .b", 5)
        );
    }

    fn capped_source(main_rules: usize, hoisted_rules: usize, important: bool) -> String {
        let flag = if important { " !important" } else { "" };
        let declarations = |index: usize| {
            format!("color: #{index:06x}{flag}; margin: 1px; padding: 2px; top: 3px;")
        };
        let main =
            (0..main_rules).map(|index| format!(".m{index} {{ {} }}\n", declarations(index)));
        let hoisted = (0..hoisted_rules).map(|index| {
            format!(
                ".h{index} {{\n  &__x {{ {} }}\n}}\n",
                declarations(main_rules + index)
            )
        });
        main.chain(hoisted).collect()
    }

    #[test]
    fn merged_layers_keep_the_notable_rule_cap() {
        let analytics = lowered_analytics(&capped_source(300, 300, true));
        let cap = fallow_extract::css_metrics::MAX_NOTABLE_RULES;
        assert_eq!(analytics.notable_rules.len(), cap);
        assert!(analytics.notable_truncated);
        assert!(
            analytics
                .notable_rules
                .windows(2)
                .all(|pair| pair[0].line <= pair[1].line)
        );
        assert_eq!(analytics.notable_rules[0].line, 1);
    }

    #[test]
    fn merged_layers_keep_the_raw_style_value_cap() {
        let analytics = lowered_analytics(&capped_source(150, 150, false));
        let cap = fallow_extract::css_metrics::MAX_RAW_STYLE_VALUES;
        assert_eq!(analytics.raw_style_values.len(), cap);
        assert!(
            analytics
                .raw_style_values
                .windows(2)
                .all(|pair| pair[0].line <= pair[1].line)
        );
    }

    #[test]
    fn merged_layers_keep_the_declaration_block_cap() {
        let analytics = lowered_analytics(&capped_source(1100, 1100, false));
        let cap = fallow_extract::css_metrics::MAX_DECLARATION_BLOCKS;
        assert_eq!(analytics.declaration_blocks.len(), cap);
        assert!(
            analytics
                .declaration_blocks
                .windows(2)
                .all(|pair| pair[0].line <= pair[1].line)
        );
    }

    #[test]
    fn merged_layers_deduplicate_raw_style_values_on_one_line() {
        let analytics = lowered_analytics(".a { color: #123456; &__x { color: #123456; } }\n");
        assert_eq!(
            analytics.raw_style_values.len(),
            1,
            "{:?}",
            analytics.raw_style_values
        );
    }

    fn repeated_ampersand_source(levels: usize) -> String {
        let mut source = String::from(".a {\n");
        for _ in 0..levels {
            source.push_str("& & {\n");
        }
        source.push_str("&__x { color: red; }\n");
        for _ in 0..=levels {
            source.push_str("}\n");
        }
        source
    }

    #[test]
    fn repeated_ampersand_nesting_keeps_output_bounded() {
        let source = repeated_ampersand_source(16);
        let layers = preprocessor_virtual_stylesheets(&source);
        let output_bytes: usize = layers.iter().map(String::len).sum();
        assert!(
            output_bytes <= source.len() + 2 * MAX_COMPILED_SELECTOR_BYTES,
            "{output_bytes} bytes of output for {} bytes of source",
            source.len()
        );
        assert_eq!(lowered_analytics(&source).total_declarations, 1);
    }

    #[test]
    fn deep_repeated_ampersand_nesting_stays_unresolved() {
        let source = repeated_ampersand_source(40);
        let layers = preprocessor_virtual_stylesheets(&source);
        assert_eq!(layers.len(), 1);
        assert!(layers[0].contains("&__x {"));
        assert_eq!(lowered_analytics(&source).total_declarations, 1);
    }

    #[test]
    fn root_selector_budget_is_inclusive() {
        let at_budget = format!(".{}", "a".repeat(MAX_COMPILED_SELECTOR_BYTES - 1));
        let over_budget = format!(".{}", "a".repeat(MAX_COMPILED_SELECTOR_BYTES));
        assert!(matches!(
            compiled_selector(&at_budget, &ParentSelector::Root),
            ParentSelector::Known(_)
        ));
        assert!(matches!(
            compiled_selector(&over_budget, &ParentSelector::Root),
            ParentSelector::Unknown
        ));
    }

    #[test]
    fn oversized_root_under_nested_at_rules_stays_unresolved() {
        let root = format!(".{}", "a".repeat(MAX_COMPILED_SELECTOR_BYTES));
        let mut source = format!("{root} {{\n");
        for _ in 0..8 {
            source.push_str("@media (min-width: 1px) {\n");
        }
        source.push_str("&__x { color: red; }\n");
        for _ in 0..8 {
            source.push_str("}\n");
        }
        source.push_str("}\n");
        let layers = preprocessor_virtual_stylesheets(&source);
        assert_eq!(layers.len(), 1);
        assert!(layers[0].contains("&__x {"));
        assert_eq!(lowered_analytics(&source).total_declarations, 1);
    }

    #[test]
    fn later_rule_on_a_line_keeps_its_source_column() {
        let source = ".a { color: red; }      .b { color: blue; }\n";
        let output = main_layer(source);
        assert_eq!(output.find(".b"), source.find(".b"), "{output}");
    }

    #[test]
    fn block_comment_before_a_selector_keeps_the_selector_line() {
        let source = "/* Header */\n.header {\n  color: red;\n}\n";
        let output = main_layer(source);
        assert_eq!(line_of(&output, ".header"), 2, "{output}");
        assert_eq!(line_of(&output, "color: red"), 3, "{output}");
    }
}
