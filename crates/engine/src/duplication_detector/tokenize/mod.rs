use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};

mod lexical;

#[cfg(test)]
pub use super::token_types::KeywordType;
pub use super::token_types::{FileTokens, OperatorType, PunctuationType, SourceToken, TokenKind};
use super::token_visitor::TokenExtractor;

/// Tokenize a source file into a sequence of normalized tokens.
///
/// For Vue/Svelte SFC files, extracts `<script>` blocks first and tokenizes
/// their content, mirroring the main analysis pipeline's SFC handling.
/// For Astro files, extracts frontmatter. For MDX files, extracts import/export statements.
///
/// When `strip_types` is true, TypeScript type annotations, interfaces, and type
/// aliases are stripped from the token stream. This enables cross-language clone
/// detection between `.ts` and `.js` files.
///
/// When `skip_imports` is true, module-wiring declarations are excluded from the
/// token stream to reduce noise from import, re-export, and top-level static
/// require binding blocks.
#[must_use]
pub fn tokenize_file(path: &Path, source: &str, skip_imports: bool) -> FileTokens {
    tokenize_file_inner(path, source, false, skip_imports)
}

/// Tokenize a source file with optional type stripping for cross-language detection.
#[must_use]
pub fn tokenize_file_cross_language(
    path: &Path,
    source: &str,
    strip_types: bool,
    skip_imports: bool,
) -> FileTokens {
    tokenize_file_inner(path, source, strip_types, skip_imports)
}

fn tokenize_file_inner(
    path: &Path,
    source: &str,
    strip_types: bool,
    skip_imports: bool,
) -> FileTokens {
    use fallow_extract::{extract_astro_frontmatter, extract_mdx_statements, is_sfc_file};

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    if is_sfc_file(path) {
        return tokenize_sfc(source, strip_types, skip_imports);
    }
    if ext == "astro" {
        return tokenize_astro(source, strip_types, skip_imports, extract_astro_frontmatter);
    }
    if ext == "mdx" {
        return tokenize_mdx(source, strip_types, skip_imports, extract_mdx_statements);
    }
    if matches!(ext, "css" | "scss" | "sass" | "less") {
        return tokenize_style_source(source);
    }

    tokenize_js_ts(path, source, strip_types, skip_imports)
}

/// Tokenize Vue/Svelte SFC `<script>` blocks.
fn tokenize_sfc(source: &str, strip_types: bool, skip_imports: bool) -> FileTokens {
    let scripts = fallow_extract::extract_sfc_scripts(source);
    let mut sections = Vec::new();

    for script in &scripts {
        let source_type = match (script.is_typescript, script.is_jsx) {
            (true, true) => SourceType::tsx(),
            (true, false) => SourceType::ts(),
            (false, true) => SourceType::jsx(),
            (false, false) => SourceType::mjs(),
        };
        sections.push(tokenize_js_section(
            "js",
            &script.body,
            script.byte_offset,
            source_type,
            strip_types,
            skip_imports,
        ));
    }

    for region in fallow_extract::extract_sfc_template_regions(source) {
        sections.push(tokenize_lexical_section(
            "markup",
            &region.body,
            region.byte_offset,
        ));
    }

    for style in fallow_extract::extract_sfc_styles(source) {
        if style.src.is_none() {
            sections.push(tokenize_lexical_section(
                "style",
                &style.body,
                style.byte_offset,
            ));
        }
    }

    let (all_tokens, atomic_invocation_spans) = merge_sections(sections);

    FileTokens {
        tokens: all_tokens,
        atomic_invocation_spans,
        source: source.to_string(),
        line_count: source.lines().count().max(1),
    }
}

/// Tokenize Astro frontmatter between `---` delimiters.
fn tokenize_astro(
    source: &str,
    strip_types: bool,
    skip_imports: bool,
    extract_fn: fn(&str) -> Option<fallow_extract::sfc::SfcScript>,
) -> FileTokens {
    if let Some(script) = extract_fn(source) {
        let mut sections = vec![tokenize_js_section(
            "js",
            &script.body,
            script.byte_offset,
            SourceType::ts(),
            strip_types,
            skip_imports,
        )];
        for region in fallow_extract::extract_astro_template_regions(source) {
            sections.push(tokenize_lexical_section(
                "markup",
                &region.body,
                region.byte_offset,
            ));
        }
        for region in fallow_extract::extract_astro_style_regions(source) {
            sections.push(tokenize_lexical_section(
                "style",
                &region.body,
                region.byte_offset,
            ));
        }
        let (tokens, atomic_invocation_spans) = merge_sections(sections);
        return FileTokens {
            tokens,
            atomic_invocation_spans,
            source: source.to_string(),
            line_count: source.lines().count().max(1),
        };
    }
    let mut sections = Vec::new();
    for region in fallow_extract::extract_astro_template_regions(source) {
        sections.push(tokenize_lexical_section(
            "markup",
            &region.body,
            region.byte_offset,
        ));
    }
    for region in fallow_extract::extract_astro_style_regions(source) {
        sections.push(tokenize_lexical_section(
            "style",
            &region.body,
            region.byte_offset,
        ));
    }
    let (tokens, atomic_invocation_spans) = merge_sections(sections);
    FileTokens {
        tokens,
        atomic_invocation_spans,
        source: source.to_string(),
        line_count: source.lines().count().max(1),
    }
}

/// Tokenize MDX import/export statements.
fn tokenize_mdx(
    source: &str,
    strip_types: bool,
    skip_imports: bool,
    extract_fn: fn(&str) -> String,
) -> FileTokens {
    let statements = extract_fn(source);
    if !statements.is_empty() {
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &statements, SourceType::jsx()).parse();

        let mut extractor = TokenExtractor::new(strip_types, skip_imports);
        extractor.visit_program(&parser_return.program);

        return FileTokens {
            tokens: extractor.tokens,
            atomic_invocation_spans: extractor.atomic_invocation_spans,
            source: source.to_string(),
            line_count: source.lines().count().max(1),
        };
    }
    empty_tokens(source)
}

/// Return empty tokens for a source file that has no tokenized regions.
fn empty_tokens(source: &str) -> FileTokens {
    FileTokens {
        tokens: Vec::new(),
        atomic_invocation_spans: Vec::new(),
        source: source.to_string(),
        line_count: source.lines().count().max(1),
    }
}

fn tokenize_style_source(source: &str) -> FileTokens {
    let mut tokens = Vec::with_capacity(source.len().min(64));
    tokens.push(lexical::boundary_token("style", 0));
    tokens.extend(lexical::tokenize_lexical_region(source, 0, true));
    FileTokens {
        tokens,
        atomic_invocation_spans: Vec::new(),
        source: source.to_string(),
        line_count: source.lines().count().max(1),
    }
}

struct TokenSection {
    name: &'static str,
    start: usize,
    tokens: Vec<SourceToken>,
    atomic_invocation_spans: Vec<Span>,
}

fn tokenize_js_section(
    name: &'static str,
    source: &str,
    byte_offset: usize,
    source_type: SourceType,
    strip_types: bool,
    skip_imports: bool,
) -> TokenSection {
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, source, source_type).parse();

    let mut extractor = TokenExtractor::new(strip_types, skip_imports);
    extractor.visit_program(&parser_return.program);

    let offset = byte_offset as u32;
    for token in &mut extractor.tokens {
        token.span = Span::new(token.span.start + offset, token.span.end + offset);
    }
    for span in &mut extractor.atomic_invocation_spans {
        *span = Span::new(span.start + offset, span.end + offset);
    }

    TokenSection {
        name,
        start: byte_offset,
        tokens: extractor.tokens,
        atomic_invocation_spans: extractor.atomic_invocation_spans,
    }
}

fn tokenize_lexical_section(name: &'static str, source: &str, byte_offset: usize) -> TokenSection {
    // CSS value canonicalization is scoped to the `"style"` section so markup
    // tokens (and, transitively, JS) are provably untouched.
    let css = name == "style";
    TokenSection {
        name,
        start: byte_offset,
        tokens: lexical::tokenize_lexical_region(source, byte_offset, css),
        atomic_invocation_spans: Vec::new(),
    }
}

fn merge_sections(mut sections: Vec<TokenSection>) -> (Vec<SourceToken>, Vec<Span>) {
    sections.retain(|section| !section.tokens.is_empty());
    sections.sort_by_key(|section| section.start);

    let mut tokens = Vec::new();
    let mut atomic_invocation_spans = Vec::new();

    for section in sections {
        tokens.push(lexical::boundary_token(section.name, section.start));
        tokens.extend(section.tokens);
        atomic_invocation_spans.extend(section.atomic_invocation_spans);
    }

    (tokens, atomic_invocation_spans)
}

/// Tokenize a standard JS/TS file, with JSX fallback for parse errors.
fn tokenize_js_ts(path: &Path, source: &str, strip_types: bool, skip_imports: bool) -> FileTokens {
    let source_type = match path.extension().and_then(|ext| ext.to_str()) {
        Some("gts") => SourceType::ts(),
        Some("gjs") => SourceType::mjs(),
        _ => SourceType::from_path(path).unwrap_or_default(),
    };
    let stripped_glimmer_source = fallow_extract::is_glimmer_file(path)
        .then(|| fallow_extract::strip_glimmer_templates(source))
        .flatten();
    let parser_source = stripped_glimmer_source.as_deref().unwrap_or(source);
    let allocator = Allocator::default();
    let parser_return = Parser::new(&allocator, parser_source, source_type).parse();

    let mut extractor = TokenExtractor::new(strip_types, skip_imports);
    extractor.visit_program(&parser_return.program);

    if extractor.tokens.len() < 5 && source.len() > 100 && !source_type.is_jsx() {
        let jsx_type = if source_type.is_typescript() {
            SourceType::tsx()
        } else {
            SourceType::jsx()
        };
        let allocator2 = Allocator::default();
        let retry_return = Parser::new(&allocator2, parser_source, jsx_type).parse();
        let mut retry_extractor = TokenExtractor::new(strip_types, skip_imports);
        retry_extractor.visit_program(&retry_return.program);
        if retry_extractor.tokens.len() > extractor.tokens.len() {
            extractor = retry_extractor;
        }
    }

    FileTokens {
        tokens: extractor.tokens,
        atomic_invocation_spans: extractor.atomic_invocation_spans,
        source: source.to_string(),
        line_count: source.lines().count().max(1),
    }
}

#[cfg(test)]
mod tests;
