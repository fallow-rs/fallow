//! MDX import/export statement extraction.
//!
//! Extracts `import` and `export` lines from MDX files (Markdown with JSX),
//! handling multi-line imports via brace depth tracking, and scans the prose
//! body for the member accesses (`<NS.Card />`, `{NS.helper()}`) and
//! whole-object passes (`all={NS}`) of its import bindings so namespace-imported
//! exports used only there are credited. Fenced code and inline code spans are
//! documentation: they record no access, and a binding mentioned inside them
//! keeps its mark-all crediting; a bare mention of a binding on a statement
//! line the visitor does not record (`export const all = NS`) does the same
//! (issue #2355).

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_ast_visit::Visit;
use oxc_parser::Parser;
use oxc_span::SourceType;

use crate::source_map::ExtractionResult;
use crate::template_expression_scan::{
    TemplateScanMode, guarded_import_locals, import_declaration_ranges, narrowable_import_locals,
    record_unexplained_mentions, record_unexplained_script_mentions, recorded_member_pairs,
    scan_template_usage,
};
use crate::visitor::{ModuleInfoExtractor, extend_member_accesses, extend_whole_object_uses};
use crate::{ImportInfo, MemberAccess, ModuleInfo};
use fallow_types::discover::FileId;

/// Everything the MDX line scan yields: the source-mapped `import` / `export`
/// statements the JavaScript parser consumes (and the same lines with their
/// source offsets, for the script-side completeness guard), the prose lines
/// whose usage is scanned once the import bindings are known, and the
/// fenced-code lines (fence delimiters included) whose mentions only feed the
/// completeness guard (issue #2355).
struct MdxScan<'a> {
    statements: ExtractionResult,
    statement_lines: Vec<(usize, &'a str)>,
    prose_lines: Vec<&'a str>,
    code_lines: Vec<&'a str>,
}

/// Extract import/export statements from MDX content.
///
/// MDX files are Markdown with JSX. Only `import` and `export` lines are relevant
/// for dead code analysis. Multi-line imports (with unmatched braces) are handled
/// by tracking brace depth.
///
/// NOTE: CSS/SCSS `@apply` is handled in `parse_css_to_module()`, not here.
/// MDX import/export extraction only handles JS/TS `import`/`export` statements.
#[must_use]
pub fn extract_mdx_statements(source: &str) -> String {
    extract_mdx_source(source).statements.body
}

fn extract_mdx_source(source: &str) -> MdxScan<'_> {
    let mut scanner = MdxStatementScanner::default();
    for (line_start, line) in lines_with_offsets(source) {
        scanner.scan_line(line_start, line);
    }
    scanner.into_scan()
}

#[derive(Default)]
struct MdxStatementScanner<'a> {
    statements: Vec<(usize, &'a str)>,
    prose_lines: Vec<&'a str>,
    code_lines: Vec<&'a str>,
    in_multiline: bool,
    brace_depth: i32,
    code_fence: Option<CodeFence>,
}

impl<'a> MdxStatementScanner<'a> {
    fn scan_line(&mut self, line_start: usize, line: &'a str) {
        let trimmed = line.trim();

        if self.consume_code_fence(trimmed) {
            self.code_lines.push(line);
            return;
        }

        if self.in_multiline {
            self.push_multiline_line(line_start, line, trimmed);
            return;
        }

        if is_statement_start(trimmed) {
            self.push_statement_start(line_start, line, trimmed);
            return;
        }

        // Prose lines are the only place the statement parser never looks, so
        // a namespace member rendered in the body (`<NS.Card />`,
        // `{NS.helper()}`) is credited here or not at all (issue #2355).
        // Statement lines are excluded so a tag inside
        // `export const X = () => <NS.Card />` records once, through the JSX
        // visitor.
        self.prose_lines.push(line);
    }

    fn consume_code_fence(&mut self, trimmed: &str) -> bool {
        if let Some(fence) = self.code_fence {
            if fence.is_closing_line(trimmed) {
                self.code_fence = None;
            }
            return true;
        }

        if self.in_multiline {
            return false;
        }

        if let Some(fence) = CodeFence::opening(trimmed) {
            self.code_fence = Some(fence);
            return true;
        }

        false
    }

    fn push_statement_start(&mut self, line_start: usize, line: &'a str, trimmed: &str) {
        self.statements.push((line_start, line));
        self.brace_depth = brace_delta(trimmed);
        if self.brace_depth > 0 && !has_spaced_source_clause(trimmed) {
            self.in_multiline = true;
        }
    }

    fn push_multiline_line(&mut self, line_start: usize, line: &'a str, trimmed: &str) {
        self.statements.push((line_start, line));
        self.brace_depth += brace_delta(trimmed);
        if self.brace_depth <= 0 || trimmed.ends_with(';') || has_any_source_clause(trimmed) {
            self.in_multiline = false;
            self.brace_depth = 0;
        }
    }

    fn into_scan(self) -> MdxScan<'a> {
        let mut statements = ExtractionResult::default();
        for &(line_start, line) in &self.statements {
            statements.push_mapped(line, line_start);
        }
        MdxScan {
            statements,
            statement_lines: self.statements,
            prose_lines: self.prose_lines,
            code_lines: self.code_lines,
        }
    }
}

/// Scan the body for the member accesses (`<NS.Card />`, `{NS.helper()}`,
/// `icon={NS.Moon}`) and whole-object passes (`all={NS}`) of the parsed import
/// bindings. The prose scan is line-based and brace-agnostic on purpose: a
/// `{ ... }` expression may span lines, so a dotted chain records wherever it
/// appears and a bare mention of an import binding (inside an expression or in
/// prose) records a whole-object use that keeps the binding on the mark-all
/// path.
///
/// Completeness guard (issue #2355): every line that is not a parsed `import`
/// / `export` statement is either a prose line, where the scan classifies
/// every identifier and hands the inline code spans it skips to the guard, or
/// a fenced-code line, which records no access and goes to the guard whole. A
/// binding mentioned in documentation code (or in a template literal the line
/// scanner cannot tell apart from a code span) therefore keeps its mark-all
/// crediting, so narrowing applies only when every mention of the binding was
/// structurally understood. Statement lines are parsed by the JSX visitor and
/// guarded separately by `collect_unexplained_statement_mentions`.
fn collect_body_usage(
    scan: &MdxScan<'_>,
    imports: &[ImportInfo],
) -> (Vec<MemberAccess>, Vec<String>) {
    let mut accesses = Vec::new();
    let mut whole_object_uses = Vec::new();
    let import_locals = guarded_import_locals(imports);
    for line in &scan.prose_lines {
        scan_template_usage(
            line,
            &import_locals,
            TemplateScanMode::MdxProse,
            &mut accesses,
            &mut whole_object_uses,
        );
    }
    for line in &scan.code_lines {
        record_unexplained_mentions(line, &import_locals, &[], &mut whole_object_uses);
    }
    (accesses, whole_object_uses)
}

/// Script-side completeness guard for the `import` / `export` statement lines
/// (issue #2355). The JSX visitor parsed them, but it records a bare import
/// binding as a whole-object use only for an allow-list of positions, so
/// `export const all = NS`, `export const Demo = () => <Callout all={NS} />`,
/// or `export default (props) => <Callout all={NS} {...props} />` would leave
/// the binding free to narrow to its dotted tags. Every mention of a namespace
/// or CSS-module binding (the bindings the graph narrows exports for) on a
/// statement line outside its own import declaration that is not a dotted
/// access the visitor recorded (or a JSX tag root) records a whole-object use.
/// Runs after the prose scan so the recorded pairs cover the whole file.
fn collect_unexplained_statement_mentions(scan: &MdxScan<'_>, info: &ModuleInfo) -> Vec<String> {
    let mut whole_object_uses = Vec::new();
    let guarded = narrowable_import_locals(&info.imports);
    if guarded.is_empty() {
        return whole_object_uses;
    }
    let recorded = recorded_member_pairs(&info.member_accesses, &guarded);
    let excluded = import_declaration_ranges(&info.imports);
    for &(line_start, line) in &scan.statement_lines {
        record_unexplained_script_mentions(
            line,
            line_start,
            &guarded,
            &excluded,
            &recorded,
            &mut whole_object_uses,
        );
    }
    whole_object_uses
}

fn is_statement_start(trimmed: &str) -> bool {
    trimmed.starts_with("import ")
        || trimmed.starts_with("import{")
        || trimmed.starts_with("export ")
        || trimmed.starts_with("export{")
}

fn brace_delta(line: &str) -> i32 {
    let opens = line.chars().filter(|&ch| ch == '{').count();
    let closes = line.chars().filter(|&ch| ch == '}').count();
    let opens = i32::try_from(opens).unwrap_or(i32::MAX);
    let closes = i32::try_from(closes).unwrap_or(i32::MAX);
    opens.saturating_sub(closes)
}

fn has_spaced_source_clause(line: &str) -> bool {
    line.contains(" from ")
}

fn has_any_source_clause(line: &str) -> bool {
    has_spaced_source_clause(line) || line.contains(" from'") || line.contains(" from\"")
}

fn lines_with_offsets(source: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut offset = 0usize;
    source.split_inclusive('\n').map(move |line| {
        let start = offset;
        offset += line.len();
        (start, line)
    })
}

#[derive(Clone, Copy)]
struct CodeFence {
    marker: char,
    len: usize,
}

impl CodeFence {
    fn opening(line: &str) -> Option<Self> {
        let marker = line.chars().next()?;
        if marker != '`' && marker != '~' {
            return None;
        }

        let len = line.chars().take_while(|&c| c == marker).count();
        (len >= 3).then_some(Self { marker, len })
    }

    fn is_closing_line(self, line: &str) -> bool {
        if !line.starts_with(self.marker) {
            return false;
        }

        let len = line.chars().take_while(|&c| c == self.marker).count();
        len >= self.len && line[len..].trim().is_empty()
    }
}

pub(crate) fn is_mdx_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext == "mdx")
}

/// Parse an MDX file by extracting import/export statements, then append the
/// member accesses and whole-object uses scanned from the prose body.
pub(crate) fn parse_mdx_to_module(file_id: FileId, source: &str, content_hash: u64) -> ModuleInfo {
    let parsed_suppressions = crate::suppress::parse_suppressions_from_source(source);
    let line_offsets = fallow_types::extract::compute_line_offsets(source);
    let scan = extract_mdx_source(source);

    let mut info = if scan.statements.body.is_empty() {
        ModuleInfoExtractor::new().into_module_info(file_id, content_hash, parsed_suppressions)
    } else {
        let source_type = SourceType::jsx();
        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &scan.statements.body, source_type).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);
        extractor.remap_spans_with(|span| scan.statements.remap_span(span));
        extractor.into_module_info(file_id, content_hash, parsed_suppressions)
    };
    let (body_accesses, body_whole_object_uses) = collect_body_usage(&scan, &info.imports);
    extend_member_accesses(&mut info, body_accesses);
    extend_whole_object_uses(&mut info, body_whole_object_uses);
    let statement_whole_object_uses = collect_unexplained_statement_mentions(&scan, &info);
    extend_whole_object_uses(&mut info, statement_whole_object_uses);
    info.line_offsets = line_offsets;
    info
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn is_mdx_file_positive() {
        assert!(is_mdx_file(Path::new("post.mdx")));
    }

    #[test]
    fn is_mdx_file_rejects_md() {
        assert!(!is_mdx_file(Path::new("readme.md")));
    }

    #[test]
    fn is_mdx_file_rejects_tsx() {
        assert!(!is_mdx_file(Path::new("component.tsx")));
    }

    #[test]
    fn is_mdx_file_rejects_jsx() {
        assert!(!is_mdx_file(Path::new("component.jsx")));
    }

    #[test]
    fn extracts_single_import() {
        let result = extract_mdx_statements("import { Chart } from './Chart'\n\n# Title\n");
        assert!(result.contains("import { Chart } from './Chart'"));
    }

    #[test]
    fn extracts_default_import() {
        let result = extract_mdx_statements("import Button from './Button'\n\n# Title\n");
        assert!(result.contains("import Button from './Button'"));
    }

    #[test]
    fn extracts_multiple_imports() {
        let source = "import { A } from './a'\nimport { B } from './b'\n\n# Title\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("import { A } from './a'"));
        assert!(result.contains("import { B } from './b'"));
    }

    #[test]
    fn extracts_import_no_space() {
        let result = extract_mdx_statements("import{ Chart } from './Chart'\n\n# Title\n");
        assert!(result.contains("import{ Chart }"));
    }

    #[test]
    fn extracts_export_const() {
        let result = extract_mdx_statements("export const meta = { title: 'Hello' }\n\n# Title\n");
        assert!(result.contains("export const meta"));
    }

    #[test]
    fn multiline_export_const_with_object_literal() {
        let result = extract_mdx_statements(
            "export const meta = {\n  title: 'Hello',\n  draft: false\n};\n\n# Title\n",
        );
        assert!(result.contains("export const meta"));
        assert!(result.contains("title: 'Hello'"));
        assert!(!result.contains("# Title"));
    }

    #[test]
    fn multiline_export_const_closed_by_bare_brace() {
        let result =
            extract_mdx_statements("export const meta = {\n  title: 'Hello'\n}\n\n# Title\n");
        assert!(result.contains("export const meta"));
        assert!(result.contains("title: 'Hello'"));
        assert!(!result.contains("# Title"));
    }

    #[test]
    fn extracts_export_no_space() {
        let result = extract_mdx_statements("export{ foo } from './foo'\n\n# Title\n");
        assert!(result.contains("export{ foo }"));
    }

    #[test]
    fn multiline_import_with_braces() {
        let source =
            "import {\n  Chart,\n  Table,\n  Graph\n} from './components'\n\n# Dashboard\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Chart"));
        assert!(result.contains("Table"));
        assert!(result.contains("Graph"));
        assert!(result.contains("from './components'"));
    }

    #[test]
    fn multiline_import_closed_by_from() {
        let source = "import {\n  Foo,\n  Bar\n} from './mod'\n\n# Content\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Foo"));
        assert!(result.contains("Bar"));
    }

    #[test]
    fn imports_between_prose() {
        let source = "import { Header } from './Header'\n\n# Section 1\n\nSome content.\n\nimport { Footer } from './Footer'\n\n## Section 2\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Header"));
        assert!(result.contains("Footer"));
    }

    #[test]
    fn prose_lines_excluded() {
        let source =
            "import { A } from './a'\n\n# Title\n\nSome **markdown** text.\n\n- List item\n";
        let result = extract_mdx_statements(source);
        assert!(!result.contains("Title"));
        assert!(!result.contains("markdown"));
        assert!(!result.contains("List item"));
    }

    #[test]
    fn fenced_import_is_ignored() {
        let source = r"import { Live } from './Live'

# Example

```ts
file: exampleSlice.ts
import exampleSliceReducer from './exampleSlice'
```
";
        let result = extract_mdx_statements(source);
        assert!(result.contains("import { Live } from './Live'"));
        assert!(!result.contains("./exampleSlice"));
        assert_eq!(result.lines().count(), 1);
    }

    #[test]
    fn fenced_export_is_ignored() {
        let source = r"# Example

```tsx
export const Example = () => null
```
";
        let result = extract_mdx_statements(source);
        assert!(result.is_empty());
    }

    #[test]
    fn tilde_fenced_import_is_ignored() {
        let source = r"import { Live } from './Live'

~~~ts
import virtual from './virtual'
~~~

export { Live }
";
        let result = extract_mdx_statements(source);
        assert!(result.contains("import { Live } from './Live'"));
        assert!(result.contains("export { Live }"));
        assert!(!result.contains("./virtual"));
        assert_eq!(result.lines().count(), 2);
    }

    #[test]
    fn longer_matching_fence_closes_code_block() {
        let source = r"````ts
import hidden from './hidden'
````

import { Visible } from './Visible'
";
        let result = extract_mdx_statements(source);
        assert!(!result.contains("./hidden"));
        assert!(result.contains("import { Visible } from './Visible'"));
    }

    #[test]
    fn shorter_matching_fence_does_not_close_code_block() {
        let source = r"````ts
import hidden from './hidden'
```
import stillHidden from './still-hidden'
````

import { Visible } from './Visible'
";
        let result = extract_mdx_statements(source);
        assert!(!result.contains("./hidden"));
        assert!(!result.contains("./still-hidden"));
        assert!(result.contains("import { Visible } from './Visible'"));
    }

    #[test]
    fn empty_source() {
        let result = extract_mdx_statements("");
        assert!(result.is_empty());
    }

    #[test]
    fn no_imports_or_exports() {
        let result = extract_mdx_statements("# Just Markdown\n\nNo imports here.\n");
        assert!(result.is_empty());
    }

    #[test]
    fn import_like_text_not_extracted() {
        let result = extract_mdx_statements("This is an important note.\n");
        assert!(result.is_empty());
    }

    #[test]
    fn export_like_text_not_extracted() {
        let result = extract_mdx_statements("We are exporting goods overseas.\n");
        assert!(result.is_empty());
    }

    #[test]
    fn side_effect_import() {
        let result = extract_mdx_statements("import './global.css'\n\n# Title\n");
        assert!(result.contains("import './global.css'"));
    }

    #[test]
    fn namespace_import() {
        let result = extract_mdx_statements("import * as utils from './utils'\n\n# Title\n");
        assert!(result.contains("import * as utils from './utils'"));
    }

    #[test]
    fn single_line_import_with_braces_balanced() {
        let source = "import { A } from './a'\n# Title\n";
        let result = extract_mdx_statements(source);
        assert_eq!(result.lines().count(), 1);
    }

    #[test]
    fn multiline_import_with_braces_extracted_as_one() {
        let source = "import {\n  Foo,\n  Bar\n} from './module'\n\n# Title\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Foo"), "Foo should be in the result");
        assert!(result.contains("Bar"), "Bar should be in the result");
        assert!(
            result.contains("from './module'"),
            "from clause should be in the result"
        );
    }

    #[test]
    fn export_with_braces_from_module() {
        let source = "export { Foo, Bar } from './module'\n\n# Title\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("export { Foo, Bar } from './module'"));
    }

    #[test]
    fn non_import_lines_between_imports_ignored() {
        let source = "import { A } from './a'\n\n# Some heading\n\nA paragraph of text.\n\nimport { B } from './b'\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("import { A } from './a'"));
        assert!(result.contains("import { B } from './b'"));
        assert!(!result.contains("heading"), "prose should not be extracted");
        assert!(
            !result.contains("paragraph"),
            "prose should not be extracted"
        );
        assert_eq!(result.lines().count(), 2);
    }

    #[test]
    fn multiline_import_terminated_by_semicolon() {
        let source = "import {\n  Foo,\n  Bar\n};\n\n# Content\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Foo"));
        assert!(result.contains("Bar"));
    }

    #[test]
    fn multiline_import_terminated_by_from_no_space_single_quote() {
        let source = "import {\n  Foo\n} from'./module'\n\n# Content\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Foo"));
        assert!(result.contains("from'./module'"));
    }

    #[test]
    fn multiline_import_terminated_by_from_no_space_double_quote() {
        let source = "import {\n  Foo\n} from\"./module\"\n\n# Content\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Foo"));
        assert!(result.contains("from\"./module\""));
    }

    #[test]
    fn multiline_export_with_braces() {
        let source = "export {\n  Foo,\n  Bar\n} from './module'\n\n# Content\n";
        let result = extract_mdx_statements(source);
        assert!(result.contains("Foo"));
        assert!(result.contains("Bar"));
        assert!(result.contains("from './module'"));
    }

    #[test]
    fn import_with_from_on_same_line_not_multiline() {
        let source = "import { A } from './a'\nimport { B } from './b'\n";
        let result = extract_mdx_statements(source);
        assert_eq!(result.lines().count(), 2);
    }

    #[test]
    fn mdx_empty_source_returns_empty_module() {
        let info = parse_mdx_to_module(fallow_types::discover::FileId(0), "", 0);
        assert!(info.imports.is_empty());
        assert!(info.exports.is_empty());
    }

    #[test]
    fn mdx_only_prose_returns_empty_module() {
        let info = parse_mdx_to_module(
            fallow_types::discover::FileId(0),
            "# Title\n\nSome text.\n",
            0,
        );
        assert!(info.imports.is_empty());
        assert!(info.exports.is_empty());
    }

    fn body_member_accesses(source: &str) -> Vec<(String, String)> {
        parse_mdx_to_module(fallow_types::discover::FileId(0), source, 0)
            .member_accesses
            .iter()
            .map(|access| (access.object.clone(), access.member.clone()))
            .collect()
    }

    /// Issue #2355: member-expression tags in the MDX body (`<NS.Card />`,
    /// paired `<NS.Panel>`) record the same per-tag member access the JSX
    /// visitor records for `.tsx`, once per opening tag.
    #[test]
    fn mdx_body_member_tag_records_member_access() {
        let accesses = body_member_accesses(
            "import * as NS from './ns'\n\n# Title\n\n<NS.Card />\n\n<NS.Panel>\ntext\n</NS.Panel>\n",
        );
        assert_eq!(
            accesses,
            vec![
                ("NS".to_string(), "Card".to_string()),
                ("NS".to_string(), "Panel".to_string()),
            ],
            "body member tags should each record once; got {accesses:?}"
        );
    }

    /// Issue #2355: `<A.B.C>` records one access per nesting level, matching
    /// the plain-expression walk of `A.B.C` (`{A.B, C}` then `{A, B}`).
    #[test]
    fn mdx_body_nested_member_tag_records_each_level() {
        let accesses = body_member_accesses("import * as A from './a'\n\n<A.B.C>nested</A.B.C>\n");
        assert_eq!(
            accesses,
            vec![
                ("A.B".to_string(), "C".to_string()),
                ("A".to_string(), "B".to_string()),
            ],
            "<A.B.C> should record both nesting levels; got {accesses:?}"
        );
    }

    fn body_whole_object_uses(source: &str) -> Vec<String> {
        parse_mdx_to_module(fallow_types::discover::FileId(0), source, 0)
            .whole_object_uses
            .to_vec()
    }

    /// Issue #2355: fenced code blocks (backtick or tilde fences) are
    /// documentation, so member tags inside them never record an access; the
    /// mention is unexplained, so the binding keeps its mark-all crediting
    /// instead of narrowing to the visible tag.
    #[test]
    fn mdx_fenced_code_member_tag_records_no_access_but_keeps_mark_all() {
        let source = "import * as NS from './ns'\nimport * as Only from './only'\n\n```tsx\n<NS.Fenced />\n```\n\n~~~\n<NS.Tilde />\n~~~\n\n<NS.Visible />\n<Only.Visible />\n";
        let accesses = body_member_accesses(source);
        assert_eq!(
            accesses,
            vec![
                ("NS".to_string(), "Visible".to_string()),
                ("Only".to_string(), "Visible".to_string()),
            ],
            "fenced member tags must not record; got {accesses:?}"
        );
        assert_eq!(
            body_whole_object_uses(source),
            vec!["NS".to_string()],
            "a fenced mention must keep NS on the mark-all path while Only stays precise"
        );
    }

    /// Issue #2355: inline code spans (single or double backtick runs) are
    /// prose, so member tags inside them never record an access, while an
    /// unmatched backtick stays literal and does not hide the rest of the
    /// line. The mention inside the span is unexplained and keeps the binding
    /// on the mark-all path.
    #[test]
    fn mdx_inline_code_member_tag_records_no_access_but_keeps_mark_all() {
        let source = "import * as NS from './ns'\n\nUse `<NS.Single />` or ``<NS.Double />`` but render <NS.Visible />.\n\nA stray ` backtick before <NS.Literal /> is not a span.\n";
        let accesses = body_member_accesses(source);
        assert_eq!(
            accesses,
            vec![
                ("NS".to_string(), "Visible".to_string()),
                ("NS".to_string(), "Literal".to_string()),
            ],
            "inline-code member tags must not record; got {accesses:?}"
        );
        assert_eq!(body_whole_object_uses(source), vec!["NS".to_string()]);
    }

    /// Issue #2355: lowercase intrinsic tags (`<div>`, `<a>`, `<br/>`) are
    /// host elements and record nothing, while a dotted tag with a lowercase
    /// root (`<ui.Card />`) is still a member expression in JSX and records.
    #[test]
    fn mdx_lowercase_intrinsic_tag_records_nothing() {
        let source = "import * as ui from './ui'\n\n<div>\n  <a href=\"x\">link</a>\n  <br/>\n  <ui.Card />\n</div>\n";
        let accesses = body_member_accesses(source);
        assert_eq!(
            accesses,
            vec![("ui".to_string(), "Card".to_string())],
            "only the dotted tag should record; got {accesses:?}"
        );
    }

    /// Issue #2355: an `export` line containing a member tag is parsed by the
    /// JSX visitor, so the body scan must not record it a second time while
    /// still recording the tags in the prose body.
    #[test]
    fn mdx_export_line_member_tag_is_not_double_recorded() {
        let accesses = body_member_accesses(
            "import * as NS from './ns'\nexport const Demo = () => <NS.Card />\n\n<NS.Body />\n",
        );
        assert_eq!(
            accesses,
            vec![
                ("NS".to_string(), "Card".to_string()),
                ("NS".to_string(), "Body".to_string()),
            ],
            "a statement-line member tag should record exactly once; got {accesses:?}"
        );
    }

    /// Issue #2355: a namespace member read inside a body expression (attribute
    /// value, text expression, call) records the same access a dotted tag does,
    /// so `<NS.Star />` next to `icon={NS.Moon}` narrows to a complete set.
    #[test]
    fn mdx_body_expression_member_access_records() {
        let accesses = body_member_accesses(
            "import * as NS from './ns'\n\n<NS.Star />\n\n<Callout icon={NS.Moon}>x</Callout>\n\n{NS.Sun()}\n",
        );
        assert_eq!(
            accesses,
            vec![
                ("NS".to_string(), "Star".to_string()),
                ("NS".to_string(), "Moon".to_string()),
                ("NS".to_string(), "Sun".to_string()),
            ],
            "expression member accesses should record; got {accesses:?}"
        );
    }

    /// Issue #2355: a `{ ... }` expression that spans lines still records the
    /// dotted access on its own line, because the scan is brace-agnostic.
    #[test]
    fn mdx_body_multiline_expression_records_member_access() {
        let accesses = body_member_accesses(
            "import * as NS from './ns'\n\n<NS.Star />\n\n{\n  NS.Moon()\n}\n",
        );
        assert!(
            accesses.contains(&("NS".to_string(), "Moon".to_string())),
            "a dotted access inside a multi-line expression should record; got {accesses:?}"
        );
    }

    /// Issue #2355: passing the namespace whole (`all={NS}`) records a
    /// whole-object use so the graph keeps every export credited, while a
    /// namespace used only through dotted tags records none and narrows.
    #[test]
    fn mdx_body_whole_namespace_pass_records_whole_object_use() {
        let whole = body_whole_object_uses(
            "import * as NS from './ns'\nimport * as Tags from './tags'\n\n<NS.Star />\n<Tags.Only />\n\n<Callout all={NS}>x</Callout>\n",
        );
        assert_eq!(
            whole,
            vec!["NS".to_string()],
            "only the namespace passed whole should record a whole-object use; got {whole:?}"
        );
    }

    /// Issue #2355: the conservative fallback. A bare mention of an import
    /// binding anywhere in prose keeps it on the mark-all path, and so does
    /// the same mention inside an inline code span (unexplained by the scan),
    /// while a binding that is not mentioned records nothing.
    #[test]
    fn mdx_prose_mention_of_import_binding_is_whole_object_use() {
        let whole = body_whole_object_uses(
            "import * as NS from './ns'\nimport Layout from './Layout'\n\nNS is documented here.\n\n<NS.Star />\n",
        );
        assert_eq!(whole, vec!["NS".to_string()], "got {whole:?}");

        let whole = body_whole_object_uses(
            "import * as NS from './ns'\nimport Layout from './Layout'\n\nOnly `NS` in code.\n\n<NS.Star />\n",
        );
        assert_eq!(
            whole,
            vec!["NS".to_string()],
            "a code-span mention is unexplained; got {whole:?}"
        );
    }

    /// Issue #2355 completeness guard: a backtick inside a JSX expression is a
    /// template literal, which the line scanner cannot tell apart from a code
    /// span, so the mention it swallows keeps the binding on the mark-all path
    /// (a namespace interpolated in an attribute, a namespace passed whole
    /// inside the literal, and a CSS module class read through a literal).
    #[test]
    fn mdx_template_literal_inside_expression_keeps_binding_on_mark_all() {
        let source = "import * as NS from './ns'\nimport * as Whole from './whole'\nimport styles from './doc.module.css'\n\n\
             <NS.Star />\n<Whole.Star />\n<div className={styles.root}></div>\n\n\
             <Callout title={`Moon: ${NS.Moon}`}>x</Callout>\n\n\
             {`${JSON.stringify(Whole)}`}\n\n\
             <div className={`${styles.spare} extra`}></div>\n";
        let accesses = body_member_accesses(source);
        assert!(
            accesses.contains(&("NS".to_string(), "Star".to_string()))
                && accesses.contains(&("styles".to_string(), "root".to_string())),
            "structured accesses still record; got {accesses:?}"
        );
        let mut whole = body_whole_object_uses(source);
        whole.sort();
        assert_eq!(
            whole,
            vec!["NS".to_string(), "Whole".to_string(), "styles".to_string()],
            "every binding mentioned inside a template literal must keep mark-all"
        );
    }

    /// Issue #2355 precision: when every mention of a binding is a dotted tag
    /// (single, paired, nested in a `<div>`), a dotted chain in an expression
    /// (attribute, text, multi-line block), or a rendered default import, no
    /// whole-object use is recorded and narrowing stays precise.
    #[test]
    fn mdx_fully_understood_body_records_no_whole_object_use() {
        let source = "import * as NS from './ns'\nimport Layout from './Layout'\nimport styles from './doc.module.css'\n\n\
             # Title\n\n<Layout>\n  <NS.Star />\n  <NS.Panel icon={NS.Moon} className={styles.root}>\n    {NS.helper()}\n  </NS.Panel>\n</Layout>\n\n\
             {\n  NS.Sun()\n}\n";
        let whole = body_whole_object_uses(source);
        assert!(
            whole.is_empty(),
            "fully understood mentions must not record a whole-object use; got {whole:?}"
        );
        let accesses = body_member_accesses(source);
        for member in ["Star", "Panel", "Moon", "helper", "Sun"] {
            assert!(
                accesses.contains(&("NS".to_string(), member.to_string())),
                "NS.{member} should be recorded; got {accesses:?}"
            );
        }
    }

    /// Issue #2355 script guard: a statement-line mention of a namespace the
    /// JSX visitor does not record as a whole-object use (`export const all =
    /// NS`, a JSX attribute on an exported component, a default-export
    /// layout) keeps the binding on the mark-all path, while a dotted-only
    /// statement use (`export const moon = SP.Moon`) records no whole-object
    /// use and stays precise.
    #[test]
    fn mdx_statement_line_bare_mention_keeps_binding_on_mark_all() {
        let source = "import * as SW from './sw';\nimport * as SA from './sa';\n\
             import * as SD from './sd';\nimport * as SP from './sp';\n\
             import Callout from './Callout.astro';\n\n\
             export const all = SW;\n\n\
             export const Demo = () => <Callout all={SA} />;\n\n\
             export default (props) => <Callout all={SD} {...props} />;\n\n\
             export const moon = SP.Moon;\n\n\
             <SW.Star /><SA.Star /><SD.Star /><SP.Star />\n";
        let mut whole = body_whole_object_uses(source);
        whole.sort();
        assert_eq!(
            whole,
            vec!["SA".to_string(), "SD".to_string(), "SW".to_string()],
            "every bare statement-line mention must keep its namespace on mark-all"
        );
        let accesses = body_member_accesses(source);
        assert!(
            accesses.contains(&("SP".to_string(), "Moon".to_string())),
            "the dotted statement access should be recorded; got {accesses:?}"
        );
    }

    /// Issue #2355: a prose chain rooted outside the import locals
    /// (`process.env.API_KEY` in a setup sentence) records no member access,
    /// so documentation never turns the MDX module into a secret source, while
    /// a chain rooted at an import local still records.
    #[test]
    fn mdx_prose_env_mention_records_no_member_access() {
        let source = "import * as NS from './ns'\n\n# Setup\n\n\
             Set process.env.API_KEY and import.meta.env.SECRET before running {NS.helper()}.\n";
        let accesses = body_member_accesses(source);
        assert_eq!(
            accesses,
            vec![("NS".to_string(), "helper".to_string())],
            "only import-local roots record; got {accesses:?}"
        );
        assert!(body_whole_object_uses(source).is_empty());
    }
}
