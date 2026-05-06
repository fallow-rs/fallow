//! Astro component frontmatter and template-script extraction.
//!
//! Extracts the TypeScript code between `---` delimiters in `.astro` files,
//! and follows `<script src="...">` and inline `<script>` blocks in the
//! component template so the targets stay reachable from the .astro file
//! (Astro bundles them into the page output at build time).

use std::path::Path;
use std::sync::LazyLock;

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::{Span, SourceType};

use crate::asset_url::normalize_asset_url;
use crate::html::is_remote_url;
use crate::sfc::{SfcScript, extract_sfc_scripts};
use crate::visitor::ModuleInfoExtractor;
use crate::{ImportInfo, ImportedName, ModuleInfo};
use fallow_types::discover::FileId;


/// Regex to extract Astro frontmatter (content between `---` delimiters at file start).
static ASTRO_FRONTMATTER_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?s)\A\s*---[ \t]*\r?\n(?P<body>.*?\r?\n)---").expect("valid regex")
});

/// Extract frontmatter from an Astro component.
pub fn extract_astro_frontmatter(source: &str) -> Option<SfcScript> {
    ASTRO_FRONTMATTER_RE.captures(source).map(|cap| {
        let body_match = cap.name("body");
        SfcScript {
            body: body_match.map_or("", |m| m.as_str()).to_string(),
            is_typescript: true, // Astro frontmatter is always TS-compatible
            is_jsx: false,
            byte_offset: body_match.map_or(0, |m| m.start()),
            src: None,
            is_setup: false,
            is_context_module: false,
            generic_attr: None,
        }
    })
}

pub(crate) fn is_astro_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "astro")
}

/// Byte offset where the Astro component template begins (after the closing
/// frontmatter `---`). Returns `0` for files without frontmatter, so the whole
/// source is scanned.
fn template_start_offset(source: &str) -> usize {
    ASTRO_FRONTMATTER_RE
        .find(source)
        .map_or(0, |m| m.end())
}

/// Parse an Astro file by extracting the frontmatter section and any
/// `<script>` blocks in the component template.
pub(crate) fn parse_astro_to_module(
    file_id: FileId,
    source: &str,
    content_hash: u64,
) -> ModuleInfo {
    let suppressions = crate::suppress::parse_suppressions_from_source(source);
    let line_offsets = fallow_types::extract::compute_line_offsets(source);

    let mut info = if let Some(script) = extract_astro_frontmatter(source) {
        let source_type = SourceType::ts();
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &script.body, source_type).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);
        extractor.into_module_info(file_id, content_hash, suppressions)
    } else {
        ModuleInfoExtractor::new().into_module_info(file_id, content_hash, suppressions)
    };

    info.line_offsets = line_offsets;

    // Astro components mount per-page client JS via `<script src="...">` and
    // inline `<script>` blocks in the template body. Scan the post-frontmatter
    // section so the targets stay reachable.
    extract_template_script_imports(source, template_start_offset(source), &mut info);

    info
}

/// Scan the Astro component template (everything after the closing frontmatter
/// `---` delimiter) for `<script src="...">` and inline `<script>` blocks, and
/// merge their references into `info.imports`.
fn extract_template_script_imports(source: &str, template_offset: usize, info: &mut ModuleInfo) {
    if template_offset >= source.len() {
        return;
    }
    let template = &source[template_offset..];
    let scripts = extract_sfc_scripts(template);
    if scripts.is_empty() {
        return;
    }

    let allocator = Allocator::default();
    for script in &scripts {
        if let Some(src) = &script.src {
            let trimmed = src.trim();
            if !trimmed.is_empty() && !is_remote_url(trimmed) {
                info.imports.push(ImportInfo {
                    source: normalize_asset_url(trimmed),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::default(),
                    source_span: Span::default(),
                });
            }
            // <script src="..."> with a body is unusual; Astro treats the body
            // as ignored when src is set, so don't parse it.
            continue;
        }

        if script.body.trim().is_empty() {
            continue;
        }

        // Astro client `<script>` blocks default to TypeScript-compatible JS
        // (the build pipeline runs them through esbuild). Match the frontmatter
        // path and parse as TS unless an explicit `lang="*sx"` enables JSX.
        let source_type = if script.is_jsx {
            if script.is_typescript {
                SourceType::tsx()
            } else {
                SourceType::jsx()
            }
        } else {
            SourceType::ts()
        };
        let parser_return = Parser::new(&allocator, &script.body, source_type).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);
        extractor.merge_into(info);
    }
}

// Astro tests exercise regex-based frontmatter extraction — no unsafe code,
// no Miri-specific value. Oxc parser tests are additionally ~1000x slower.
#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    // ── is_astro_file ────────────────────────────────────────────

    #[test]
    fn is_astro_file_positive() {
        assert!(is_astro_file(Path::new("Layout.astro")));
    }

    #[test]
    fn is_astro_file_rejects_vue() {
        assert!(!is_astro_file(Path::new("App.vue")));
    }

    #[test]
    fn is_astro_file_rejects_ts() {
        assert!(!is_astro_file(Path::new("utils.ts")));
    }

    #[test]
    fn is_astro_file_rejects_mdx() {
        assert!(!is_astro_file(Path::new("post.mdx")));
    }

    // ── extract_astro_frontmatter: basic extraction ──────────────

    #[test]
    fn extracts_frontmatter_body() {
        let source = "---\nimport Layout from '../layouts/Layout.astro';\nconst title = 'Hi';\n---\n<Layout />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let script = script.unwrap();
        assert!(script.body.contains("import Layout"));
        assert!(script.body.contains("const title"));
    }

    #[test]
    fn frontmatter_is_always_typescript() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(script.is_typescript);
    }

    #[test]
    fn frontmatter_is_not_jsx() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(!script.is_jsx);
    }

    #[test]
    fn frontmatter_has_no_src() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(script.src.is_none());
    }

    // ── No frontmatter ───────────────────────────────────────────

    #[test]
    fn no_frontmatter_returns_none() {
        let source = "<div>No frontmatter here</div>";
        assert!(extract_astro_frontmatter(source).is_none());
    }

    #[test]
    fn no_frontmatter_just_html() {
        let source = "<html><body><h1>Hello</h1></body></html>";
        assert!(extract_astro_frontmatter(source).is_none());
    }

    // ── Empty frontmatter ────────────────────────────────────────

    #[test]
    fn empty_frontmatter() {
        let source = "---\n\n---\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let body = script.unwrap().body;
        assert!(body.trim().is_empty());
    }

    // ── Multiple --- pairs: only first is extracted ──────────────

    #[test]
    fn only_first_frontmatter_pair() {
        let source = "---\nconst first = true;\n---\n<div />\n---\nconst second = true;\n---\n";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let body = script.unwrap().body;
        assert!(body.contains("first"));
        assert!(!body.contains("second"));
    }

    // ── Byte offset ──────────────────────────────────────────────

    #[test]
    fn byte_offset_points_to_body() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source).unwrap();
        let offset = script.byte_offset;
        assert!(source[offset..].starts_with("const x = 1;"));
    }

    // ── Leading whitespace before --- ────────────────────────────

    #[test]
    fn leading_whitespace_before_frontmatter() {
        let source = "  \n---\nconst x = 1;\n---\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        assert!(script.unwrap().body.contains("const x = 1;"));
    }

    // ── Frontmatter with TypeScript syntax ───────────────────────

    #[test]
    fn frontmatter_with_type_annotations() {
        let source = "---\ninterface Props { title: string; }\nconst { title } = Astro.props as Props;\n---\n<h1>{title}</h1>";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        let body = script.unwrap().body;
        assert!(body.contains("interface Props"));
        assert!(body.contains("Astro.props"));
    }

    // ── Additional coverage ─────────────────────────────────────

    #[test]
    fn frontmatter_with_multiline_imports() {
        let source = "---\nimport {\n  Component,\n  Fragment\n} from 'react';\n---\n<Component />";
        let script = extract_astro_frontmatter(source).unwrap();
        assert!(script.body.contains("Component"));
        assert!(script.body.contains("Fragment"));
    }

    #[test]
    fn frontmatter_with_crlf_line_endings() {
        // Windows: git checkout converts LF to CRLF
        let source = "---\r\nexport const x = 1;\r\n---\r\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        assert!(script.unwrap().body.contains("export const x = 1;"));
    }

    #[test]
    fn frontmatter_not_at_start_returns_none() {
        // --- not at the start of the file
        let source = "<div />\n---\nconst x = 1;\n---\n";
        assert!(extract_astro_frontmatter(source).is_none());
    }

    #[test]
    fn frontmatter_dashes_in_body_not_confused() {
        // Triple dashes inside the frontmatter body (as part of a comment or string)
        let source = "---\nconst x = '---';\nconst y = 2;\n---\n<div />";
        let script = extract_astro_frontmatter(source);
        assert!(script.is_some());
        // The body should end at the first --- after the opening, which is inside the string
        // Actually the regex is non-greedy, so it finds the first `\n---`
        let body = script.unwrap().body;
        assert!(body.contains("const x = '---';"));
    }

    // ── Full parse tests (Oxc parser ~1000x slower under Miri) ──

    #[test]
    fn parse_astro_to_module_no_frontmatter() {
        let info = parse_astro_to_module(FileId(0), "<div>Hello</div>", 42);
        assert!(info.imports.is_empty());
        assert!(info.exports.is_empty());
        assert_eq!(info.content_hash, 42);
        assert_eq!(info.file_id, FileId(0));
    }

    #[test]
    fn parse_astro_to_module_with_imports() {
        let source = "---\nimport { ref } from 'vue';\nconst x = ref(0);\n---\n<div />";
        let info = parse_astro_to_module(FileId(1), source, 99);
        assert_eq!(info.imports.len(), 1);
        assert_eq!(info.imports[0].source, "vue");
        assert_eq!(info.file_id, FileId(1));
        assert_eq!(info.content_hash, 99);
    }

    #[test]
    fn parse_astro_to_module_has_line_offsets() {
        let source = "---\nconst x = 1;\n---\n<div />";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(!info.line_offsets.is_empty());
    }

    #[test]
    fn parse_astro_to_module_has_suppressions() {
        let source = "---\n// fallow-ignore-file\nconst x = 1;\n---\n<div />";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(!info.suppressions.is_empty());
        assert_eq!(info.suppressions[0].line, 0);
    }

    #[test]
    fn is_astro_file_rejects_svelte() {
        assert!(!is_astro_file(Path::new("Component.svelte")));
    }

    #[test]
    fn is_astro_file_rejects_no_extension() {
        assert!(!is_astro_file(Path::new("Makefile")));
    }

    // ── Template <script> extraction (issue #295) ────────────────

    #[test]
    fn template_script_src_emits_side_effect_import() {
        let source = "---\n---\n<script src=\"../scripts/foo.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(
            info.imports.iter().any(|i| i.source == "../scripts/foo.ts"
                && matches!(i.imported_name, ImportedName::SideEffect)),
            "expected side-effect import for <script src>, got: {:?}",
            info.imports.iter().map(|i| &i.source).collect::<Vec<_>>()
        );
    }

    #[test]
    fn template_script_src_without_frontmatter() {
        let source = "<html><body><script src=\"./foo.ts\"></script></body></html>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.iter().any(|i| i.source == "./foo.ts"));
    }

    #[test]
    fn template_inline_script_imports_followed() {
        let source = "---\n---\n<script>\n  import '../scripts/bar';\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(
            info.imports.iter().any(|i| i.source == "../scripts/bar"),
            "expected inline-script import to be extracted, got: {:?}",
            info.imports.iter().map(|i| &i.source).collect::<Vec<_>>()
        );
    }

    #[test]
    fn template_script_src_remote_url_skipped() {
        let source = "---\n---\n<script src=\"https://cdn.example.com/x.js\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.is_empty());
    }

    #[test]
    fn template_script_src_bare_filename_normalized() {
        let source = "---\n---\n<script src=\"logic.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.iter().any(|i| i.source == "./logic.ts"));
    }

    #[test]
    fn template_script_inside_html_comment_skipped() {
        let source = "---\n---\n<!-- <script src=\"./bad.ts\"></script> -->\n<script src=\"./good.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.iter().any(|i| i.source == "./good.ts"));
        assert!(!info.imports.iter().any(|i| i.source == "./bad.ts"));
    }

    #[test]
    fn template_script_combined_with_frontmatter_imports() {
        let source = "---\nimport Layout from '../layouts/Layout.astro';\n---\n<script src=\"./client.ts\"></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.iter().any(|i| i.source == "../layouts/Layout.astro"));
        assert!(info.imports.iter().any(|i| i.source == "./client.ts"));
    }

    #[test]
    fn template_script_src_with_body_uses_src_only() {
        // Per Astro's HTML semantics, when `src` is set the body is ignored.
        let source = "---\n---\n<script src=\"./external.ts\">import './ignored';</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.iter().any(|i| i.source == "./external.ts"));
        assert!(!info.imports.iter().any(|i| i.source == "./ignored"));
    }

    #[test]
    fn template_inline_script_typescript_syntax() {
        // Inline <script> defaults to TS so type annotations parse cleanly.
        let source = "---\n---\n<script>\n  import { foo } from './bar';\n  const x: number = foo();\n</script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.iter().any(|i| i.source == "./bar"));
    }

    #[test]
    fn template_empty_script_block_no_panic() {
        let source = "---\n---\n<script></script>";
        let info = parse_astro_to_module(FileId(0), source, 0);
        assert!(info.imports.is_empty());
    }
}
