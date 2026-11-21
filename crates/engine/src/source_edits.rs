//! The byte spans of the top-level export statements of a file, and the
//! edits that remove their `export` keyword.
//!
//! `fallow fix`, the language server and the CI review comments change
//! source text. A line rule cannot tell which `export` keyword of a line
//! belongs to a name, which names one keyword exports, or where a
//! declaration ends. This module parses the file and answers these
//! questions from the syntax tree. A file that does not parse gets no
//! index, and so no edit.

use std::path::Path;

use oxc_ast::ast::{
    ClassBody, Declaration, ExportDefaultDeclarationKind, Program, Statement, TSEnumBody,
};
use oxc_ast_visit::Visit;
use oxc_span::{GetSpan, SourceType};

/// One replacement of a byte range of the source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpanEdit {
    /// The byte offset of the first replaced byte.
    pub start: u32,
    /// The byte offset after the last replaced byte.
    pub end: u32,
    /// The new text. Empty for a deletion.
    pub replacement: String,
}

/// Why the index gives no statement for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRefusal {
    /// No export statement on the line binds the name.
    NameNotFound,
    /// More than one export statement matches, and the column selects none.
    Ambiguous,
}

/// The form of one export statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportStatementKind {
    /// `export const`, `export let`, `export var`.
    Variable,
    /// `export enum`, `export const enum`, `export declare enum`.
    Enum,
    /// Each other `export <declaration>`.
    Declaration,
    /// `export default function name` or `export default class Name`.
    NamedDefault,
    /// Each other `export default`: no name, an interface, or an expression.
    OtherDefault,
}

/// One top-level `export <declaration>` or `export default` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportStatement {
    /// The byte offset of the `export` keyword.
    pub start: u32,
    /// The byte offset after the end of the statement.
    pub end: u32,
    /// The form of the statement.
    pub kind: ExportStatementKind,
    /// Each name that the statement exports, in source order. A default
    /// export has the one name `default`.
    pub names: Vec<String>,
    /// The end of the text that the removal of the export deletes, or `None`
    /// when the statement has no safe removal.
    keyword_end: Option<u32>,
}

impl ExportStatement {
    /// The edit that removes the export and keeps the declaration.
    #[must_use]
    pub fn keyword_edit(&self) -> Option<SpanEdit> {
        self.keyword_end.map(|end| SpanEdit {
            start: self.start,
            end,
            replacement: String::new(),
        })
    }
}

/// One class element or enum member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemberSpan {
    /// The byte offset of the first byte of the member.
    pub start: u32,
    /// The byte offset after the last byte of the member.
    pub end: u32,
    /// False for an enum member whose deletion changes the value of the next
    /// member: the next member has no initializer, so its value counts on
    /// from this member. Always true for a class element.
    pub deletion_keeps_values: bool,
    /// True for a class element whose static key another element of the same
    /// class also has, such as an overload signature, its implementation, or
    /// one half of a getter and setter pair. Always false for an enum member.
    pub shares_its_key: bool,
}

/// The export statements and the class and enum members of one file text.
pub struct SourceEditIndex {
    statements: Vec<ExportStatement>,
    members: Vec<MemberSpan>,
    source: String,
    line_starts: Vec<u32>,
}

impl SourceEditIndex {
    /// Parse `source`, the text of the file at `path`.
    ///
    /// A JavaScript or TypeScript file is parsed as a whole. A Vue or Svelte
    /// file is parsed per inline `<script>` block and an Astro file per
    /// frontmatter block. Each offset is an offset into `source`.
    ///
    /// Returns `None` for a file type that the engine does not parse here,
    /// or when a block does not parse without an error.
    #[must_use]
    pub fn parse(path: &Path, source: &str) -> Option<Self> {
        let Parsed {
            statements,
            members,
        } = parse_file(path, source)?;
        let line_starts = std::iter::once(0)
            .chain(
                source
                    .match_indices('\n')
                    .map(|(offset, _)| u32::try_from(offset + 1).unwrap_or(u32::MAX)),
            )
            .collect();
        Some(Self {
            statements,
            members,
            source: source.to_owned(),
            line_starts,
        })
    }

    /// The statement that exports `name` on the 1-based `line`.
    ///
    /// `col` is the 0-based byte column of the name. It decides only between
    /// two statements on one line that both export `name`.
    ///
    /// # Errors
    ///
    /// [`EditRefusal::NameNotFound`] when no statement of the line exports
    /// `name`, and [`EditRefusal::Ambiguous`] when the column does not select
    /// one of several such statements.
    pub fn export_statement(
        &self,
        name: &str,
        line: u32,
        col: u32,
    ) -> Result<&ExportStatement, EditRefusal> {
        let line_idx = line.saturating_sub(1) as usize;
        let candidates: Vec<&ExportStatement> = self
            .statements
            .iter()
            .filter(|statement| {
                let (first, last) = self.line_range(statement.start, statement.end);
                (first..=last).contains(&line_idx)
                    && statement.names.iter().any(|bound| bound == name)
            })
            .collect();
        match candidates.as_slice() {
            [] => Err(EditRefusal::NameNotFound),
            [only] => Ok(only),
            _ => {
                let offset = self
                    .line_starts
                    .get(line_idx)
                    .map(|start| start.saturating_add(col));
                let mut containing = candidates.iter().filter(|statement| {
                    offset.is_some_and(|offset| statement.start <= offset && offset < statement.end)
                });
                match (containing.next(), containing.next()) {
                    (Some(statement), None) => Ok(statement),
                    _ => Err(EditRefusal::Ambiguous),
                }
            }
        }
    }

    /// Each export statement of the file, in source order.
    #[must_use]
    pub fn statements(&self) -> &[ExportStatement] {
        &self.statements
    }

    /// The statements whose `export` keyword is on the 1-based `line`.
    pub fn export_statements_on_line(&self, line: u32) -> impl Iterator<Item = &ExportStatement> {
        let line_idx = line.saturating_sub(1) as usize;
        self.statements
            .iter()
            .filter(move |statement| self.line_of(statement.start) == line_idx)
    }

    /// The class elements and enum members whose first byte is on the
    /// 1-based `line`, at each depth of the file.
    pub fn members_on_line(&self, line: u32) -> impl Iterator<Item = &MemberSpan> {
        let line_idx = line.saturating_sub(1) as usize;
        self.members
            .iter()
            .filter(move |member| self.line_of(member.start) == line_idx)
    }

    /// Each class element and enum member of the file. The elements of one
    /// class body or enum body are in source order, and each body comes
    /// before the members nested inside it.
    #[must_use]
    pub fn members(&self) -> &[MemberSpan] {
        &self.members
    }

    /// The statement whose `export` keyword is at byte `start`.
    #[must_use]
    pub fn statement_at(&self, start: u32) -> Option<&ExportStatement> {
        self.statements
            .iter()
            .find(|statement| statement.start == start)
    }

    /// The 0-based first and last line of the bytes `start..end`.
    #[must_use]
    pub fn line_range(&self, start: u32, end: u32) -> (usize, usize) {
        (
            self.line_of(start),
            self.line_of(end.saturating_sub(1).max(start)),
        )
    }

    fn line_of(&self, offset: u32) -> usize {
        self.line_starts
            .partition_point(|line_start| *line_start <= offset)
            .saturating_sub(1)
    }

    /// True when no other text shares the lines of `start..end`: only white
    /// space is before `start` on its line, and only white space and `;`
    /// are after `end` on its line.
    #[must_use]
    pub fn owns_its_lines(&self, start: u32, end: u32) -> bool {
        let (first, _) = self.line_range(start, end);
        let line_start = self.line_starts[first] as usize;
        let Some(before) = self.source.get(line_start..start as usize) else {
            return false;
        };
        let Some(after) = self.source.get(end as usize..) else {
            return false;
        };
        let after = after.split('\n').next().unwrap_or("");
        before.trim().is_empty() && after.trim().trim_start_matches(';').trim().is_empty()
    }
}

/// The statements and members of a file or of its script blocks.
#[derive(Default)]
struct Parsed {
    statements: Vec<ExportStatement>,
    members: Vec<MemberSpan>,
}

fn parse_file(path: &Path, source: &str) -> Option<Parsed> {
    if fallow_extract::is_sfc_file(path) {
        let scripts = fallow_extract::extract_sfc_scripts(source);
        return parse_blocks(source, scripts.iter().filter(|script| script.src.is_none()));
    }
    if path
        .extension()
        .is_some_and(|extension| extension == "astro")
    {
        let frontmatter = fallow_extract::astro::extract_astro_frontmatter(source);
        return parse_blocks(source, frontmatter.iter());
    }
    let source_type = SourceType::from_path(path).ok()?;
    parse_block(source, source, source_type, 0)
}

fn parse_blocks<'a>(
    file: &str,
    scripts: impl Iterator<Item = &'a fallow_extract::sfc::SfcScript>,
) -> Option<Parsed> {
    let mut parsed = Parsed::default();
    for script in scripts {
        let source_type = match (script.is_typescript, script.is_jsx) {
            (true, true) => SourceType::tsx(),
            (true, false) => SourceType::ts(),
            (false, true) => SourceType::jsx(),
            (false, false) => SourceType::mjs(),
        };
        let offset = u32::try_from(script.byte_offset).ok()?;
        let block = parse_block(file, &script.body, source_type, offset)?;
        parsed.statements.extend(block.statements);
        parsed.members.extend(block.members);
    }
    Some(parsed)
}

/// Parse one block of script text that starts at byte `offset` of `file`.
fn parse_block(file: &str, source: &str, source_type: SourceType, offset: u32) -> Option<Parsed> {
    fallow_extract::parse_bounded(source, || {
        let allocator = oxc_allocator::Allocator::default();
        let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
        if parsed.fatal_error || parsed.diagnostics.has_errors() {
            return None;
        }
        let mut members = MemberCollector {
            offset,
            members: Vec::new(),
        };
        members.visit_program(&parsed.program);
        Some(Parsed {
            statements: collect(&parsed.program, file, offset),
            members: members.members,
        })
    })
    .flatten()
}

/// Collects the span of each class element and each enum member.
struct MemberCollector {
    offset: u32,
    members: Vec<MemberSpan>,
}

impl MemberCollector {
    fn push(&mut self, span: oxc_span::Span, deletion_keeps_values: bool, shares_its_key: bool) {
        self.members.push(MemberSpan {
            start: span.start + self.offset,
            end: span.end + self.offset,
            deletion_keeps_values,
            shares_its_key,
        });
    }
}

impl<'a> Visit<'a> for MemberCollector {
    fn visit_class_body(&mut self, it: &ClassBody<'a>) {
        let names: Vec<_> = it
            .body
            .iter()
            .map(|element| element.static_name())
            .collect();
        for (position, element) in it.body.iter().enumerate() {
            let shares_its_key = names[position].as_ref().is_some_and(|name| {
                names.iter().enumerate().any(|(other, other_name)| {
                    other != position && other_name.as_ref() == Some(name)
                })
            });
            self.push(element.span(), true, shares_its_key);
        }
        oxc_ast_visit::walk::walk_class_body(self, it);
    }

    fn visit_ts_enum_body(&mut self, it: &TSEnumBody<'a>) {
        for (position, member) in it.members.iter().enumerate() {
            let next_has_value = it
                .members
                .get(position + 1)
                .is_none_or(|next| next.initializer.is_some());
            self.push(member.span, next_has_value, false);
        }
        oxc_ast_visit::walk::walk_ts_enum_body(self, it);
    }
}

fn collect(program: &Program<'_>, file: &str, offset: u32) -> Vec<ExportStatement> {
    program
        .body
        .iter()
        .filter_map(|statement| {
            let (span, kind, names) = match statement {
                Statement::ExportDeclaration(export) => {
                    let (kind, names) = declaration_names(&export.declaration)?;
                    (export.span(), kind, names)
                }
                Statement::ExportDefaultDeclaration(export) => {
                    let named = match &export.declaration {
                        ExportDefaultDeclarationKind::FunctionDeclaration(function) => {
                            function.id.is_some()
                        }
                        ExportDefaultDeclarationKind::ClassDeclaration(class) => class.id.is_some(),
                        _ => false,
                    };
                    let kind = if named {
                        ExportStatementKind::NamedDefault
                    } else {
                        ExportStatementKind::OtherDefault
                    };
                    (export.span(), kind, vec!["default".to_owned()])
                }
                _ => return None,
            };
            let start = span.start + offset;
            let end = span.end + offset;
            Some(ExportStatement {
                start,
                end,
                kind,
                names,
                keyword_end: keyword_end(file, start, kind),
            })
        })
        .collect()
}

/// The kind and the exported names of an `export <declaration>`, or `None`
/// for a declaration without a name.
fn declaration_names(declaration: &Declaration<'_>) -> Option<(ExportStatementKind, Vec<String>)> {
    let single = |name: &str| Some((ExportStatementKind::Declaration, vec![name.to_owned()]));
    match declaration {
        Declaration::VariableDeclaration(variables) => {
            let names: Vec<String> = variables
                .declarations
                .iter()
                .flat_map(|declarator| declarator.id.get_binding_identifiers())
                .map(|identifier| identifier.name.to_string())
                .collect();
            (!names.is_empty()).then_some((ExportStatementKind::Variable, names))
        }
        Declaration::FunctionDeclaration(function) => single(function.id.as_ref()?.name.as_str()),
        Declaration::ClassDeclaration(class) => single(class.id.as_ref()?.name.as_str()),
        Declaration::TSTypeAliasDeclaration(alias) => single(alias.id.name.as_str()),
        Declaration::TSInterfaceDeclaration(interface) => single(interface.id.name.as_str()),
        Declaration::TSEnumDeclaration(enumeration) => Some((
            ExportStatementKind::Enum,
            vec![enumeration.id.name.to_string()],
        )),
        Declaration::TSNamespaceDeclaration(namespace) => single(namespace.id.name.as_str()),
        Declaration::TSImportEqualsDeclaration(import) => single(import.id.name.as_str()),
        Declaration::TSExternalModuleDeclaration(_) | Declaration::TSGlobalDeclaration(_) => None,
    }
}

/// The end of the removable `export` prefix of the statement at `start`.
fn keyword_end(file: &str, start: u32, kind: ExportStatementKind) -> Option<u32> {
    let text = file.get(start as usize..)?.strip_prefix("export")?;
    let after_export = skip_blank(text)?;
    let removed = match kind {
        ExportStatementKind::Variable
        | ExportStatementKind::Enum
        | ExportStatementKind::Declaration => text.len() - after_export.len(),
        ExportStatementKind::NamedDefault => {
            let after_default = skip_blank(after_export.strip_prefix("default")?)?;
            text.len() - after_default.len()
        }
        ExportStatementKind::OtherDefault => return None,
    };
    let removed = u32::try_from("export".len() + removed).ok()?;
    Some(start + removed)
}

/// `text` without its leading spaces and tabs, or `None` when it has none.
fn skip_blank(text: &str) -> Option<&str> {
    let rest = text.trim_start_matches([' ', '\t']);
    (rest.len() < text.len()).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(path: &str, source: &str) -> SourceEditIndex {
        SourceEditIndex::parse(Path::new(path), source).expect("the source parses")
    }

    fn apply(source: &str, edit: &SpanEdit) -> String {
        let mut result = source.to_owned();
        result.replace_range(edit.start as usize..edit.end as usize, &edit.replacement);
        result
    }

    fn summary(index: &SourceEditIndex) -> Vec<(ExportStatementKind, Vec<String>)> {
        index
            .statements
            .iter()
            .map(|statement| (statement.kind, statement.names.clone()))
            .collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn finds_the_names_and_kind_of_each_statement_form() {
        use ExportStatementKind::{Declaration, Enum, NamedDefault, Variable};
        let source = "export const single = 1;\nexport var g = 1, h = 2;\nexport const { a = 1, b: { c }, ...rest } = x;\nexport let [d, , [e], ...f] = y;\nexport function fn1() {}\nexport async function fn2() {}\nexport class C1 {}\nexport abstract class C2 {}\nexport type T = 1;\nexport interface I {}\nexport enum E { A }\nexport const enum CE { A }\nexport namespace N {}\n";
        assert_eq!(
            summary(&index("store.ts", source)),
            vec![
                (Variable, names(&["single"])),
                (Variable, names(&["g", "h"])),
                (Variable, names(&["a", "c", "rest"])),
                (Variable, names(&["d", "e", "f"])),
                (Declaration, names(&["fn1"])),
                (Declaration, names(&["fn2"])),
                (Declaration, names(&["C1"])),
                (Declaration, names(&["C2"])),
                (Declaration, names(&["T"])),
                (Declaration, names(&["I"])),
                (Enum, names(&["E"])),
                (Enum, names(&["CE"])),
                (Declaration, names(&["N"])),
            ]
        );
        let defaults = [
            ("export default function named() {}\n", NamedDefault),
            ("export default class Named {}\n", NamedDefault),
            (
                "export default function () {}\n",
                ExportStatementKind::OtherDefault,
            ),
            (
                "export default class {}\n",
                ExportStatementKind::OtherDefault,
            ),
            ("export default thing;\n", ExportStatementKind::OtherDefault),
            (
                "export default { a: 1, b: 2 };\n",
                ExportStatementKind::OtherDefault,
            ),
        ];
        for (source, kind) in defaults {
            assert_eq!(
                summary(&index("store.ts", source)),
                vec![(kind, names(&["default"]))],
                "{source}"
            );
        }
    }

    #[test]
    fn skips_lists_re_exports_and_assignments() {
        let source =
            "const a = 1;\nexport { a };\nexport { b } from './b';\nexport * from './c';\n";
        assert!(index("store.ts", source).statements.is_empty());
        assert!(
            index("store.ts", "const a = 1;\nexport = a;\n")
                .statements
                .is_empty()
        );
    }

    #[test]
    fn keyword_edit_keeps_the_declaration() {
        let cases = [
            ("export const a = 1;\n", "const a = 1;\n"),
            ("export\tconst a = 1, b = 2;\n", "const a = 1, b = 2;\n"),
            (
                "export /* values */ const a = 1;\n",
                "/* values */ const a = 1;\n",
            ),
            ("export async function f() {}\n", "async function f() {}\n"),
            ("export abstract class C {}\n", "abstract class C {}\n"),
            ("export enum E { A }\n", "enum E { A }\n"),
            (
                "export default function named() {}\n",
                "function named() {}\n",
            ),
            (
                "export default async function named() {}\n",
                "async function named() {}\n",
            ),
            ("export default class Named {}\n", "class Named {}\n"),
            (
                "export default abstract class Named {}\n",
                "abstract class Named {}\n",
            ),
        ];
        for (source, expected) in cases {
            let index = index("store.ts", source);
            let edit = index.statements[0].keyword_edit().expect(source);
            assert_eq!(apply(source, &edit), expected, "{source}");
        }
        for source in [
            "export default function () {}\n",
            "export default class {}\n",
            "export default thing;\n",
            "export default { a: 1, b: 2 };\n",
        ] {
            assert_eq!(index("store.ts", source).statements[0].keyword_edit(), None);
        }
    }

    #[test]
    fn offsets_count_a_leading_byte_order_mark() {
        let source = "\u{feff}export const a = 1;\n";
        let index = index("store.ts", source);
        let statement = &index.statements[0];
        assert_eq!(statement.start, 3);
        assert_eq!(
            apply(source, &statement.keyword_edit().unwrap()),
            "\u{feff}const a = 1;\n"
        );
    }

    #[test]
    fn export_statement_selects_by_line_and_column() {
        let source = "export function live() {} export function dead() {}\nexport const x = 1;\n";
        let index = index("store.ts", source);
        let dead = index.export_statement("dead", 1, 42).unwrap();
        assert_eq!(
            &source[dead.start as usize..dead.end as usize],
            "export function dead() {}"
        );
        let dead = index.export_statement("dead", 1, 0).unwrap();
        assert_eq!(dead.start, 26, "one candidate needs no column");
        assert_eq!(
            index.export_statement("x", 1, 0),
            Err(EditRefusal::NameNotFound)
        );
        assert_eq!(index.export_statement("x", 2, 13).unwrap().start, 52);

        let twice = "export namespace N {} export namespace N {}\n";
        let index = super::tests::index("store.ts", twice);
        assert_eq!(index.export_statement("N", 1, 0).unwrap().start, 0);
        assert_eq!(index.export_statement("N", 1, 39).unwrap().start, 22);
    }

    #[test]
    fn a_statement_over_several_lines_holds_each_of_its_lines() {
        let source = "const x = {};\nexport const {\n  a,\n} = x;\nexport function f() {}\n";
        let index = index("store.ts", source);
        let statement = index.export_statement("a", 3, 2).unwrap();
        assert_eq!(
            &source[statement.start as usize..statement.end as usize],
            "export const {\n  a,\n} = x;"
        );
        assert_eq!(index.line_range(statement.start, statement.end), (1, 3));
        assert!(index.owns_its_lines(statement.start, statement.end));
        let f = index.export_statement("f", 5, 16).unwrap();
        assert_eq!(index.line_range(f.start, f.end), (4, 4));
    }

    #[test]
    fn owns_its_lines_sees_other_text_on_the_line() {
        let source =
            "export enum E { A };\nexport enum F { A } const x = 1;\n  export enum G {\n  A,\n}\n";
        let index = index("store.ts", source);
        let statements = &index.statements;
        assert!(index.owns_its_lines(statements[0].start, statements[0].end));
        assert!(!index.owns_its_lines(statements[1].start, statements[1].end));
        assert!(index.owns_its_lines(statements[2].start, statements[2].end));
    }

    /// The text of each statement that `source` holds.
    fn texts(path: &str, source: &str) -> Vec<String> {
        index(path, source)
            .statements
            .into_iter()
            .map(|statement| source[statement.start as usize..statement.end as usize].to_owned())
            .collect()
    }

    #[test]
    fn reads_each_script_block_of_a_vue_file() {
        let source = "<template><div /></template>\n<script lang=\"ts\">\nexport const c = 1, d = 2;\n</script>\n<script setup lang=\"ts\">\nconst e = 1;\n</script>\n";
        assert_eq!(
            texts("store.vue", source),
            vec!["export const c = 1, d = 2;"]
        );
    }

    #[test]
    fn reads_the_module_script_of_a_svelte_file() {
        let source =
            "<script context=\"module\">\n  export const { a, b } = x;\n</script>\n<p>hi</p>\n";
        assert_eq!(
            texts("store.svelte", source),
            vec!["export const { a, b } = x;"]
        );
    }

    #[test]
    fn reads_the_frontmatter_of_an_astro_file() {
        let source = "---\nexport const [a, b] = [1, 2];\n---\n<p>{a}</p>\n";
        assert_eq!(
            texts("page.astro", source),
            vec!["export const [a, b] = [1, 2];"]
        );
    }

    /// The text of each member that starts on `line`.
    fn members(source: &str, line: u32) -> Vec<String> {
        let index = index("store.ts", source);
        index
            .members_on_line(line)
            .map(|member| source[member.start as usize..member.end as usize].to_owned())
            .collect()
    }

    #[test]
    fn members_on_line_gives_class_and_enum_members() {
        let class = "class C {\n  one() { return 1; }\n  legacy() {\n    return null;\n  }\n  value = 1;\n}\n";
        assert_eq!(members(class, 2), vec!["one() { return 1; }"]);
        assert_eq!(members(class, 3), vec!["legacy() {\n    return null;\n  }"]);
        assert_eq!(members(class, 6), vec!["value = 1;"]);
        assert!(members(class, 4).is_empty());

        let enumeration = "enum E {\n  A,\n  B, C,\n  D = \"x,y\"\n}\n";
        assert_eq!(members(enumeration, 2), vec!["A"]);
        assert_eq!(members(enumeration, 3), vec!["B", "C"]);
        assert_eq!(members(enumeration, 4), vec!["D = \"x,y\""]);
    }

    #[test]
    fn a_class_element_knows_whether_it_shares_its_key() {
        let source = "class K {\n  m(a: string): void;\n  m(a: any) {}\n  get x() { return 1; }\n  set x(v) {}\n  only() {}\n}\n";
        let index = index("store.ts", source);
        let shares = |line| {
            index
                .members_on_line(line)
                .map(|member| member.shares_its_key)
                .collect::<Vec<_>>()
        };
        assert_eq!(shares(2), vec![true]);
        assert_eq!(shares(3), vec![true]);
        assert_eq!(shares(4), vec![true]);
        assert_eq!(shares(5), vec![true]);
        assert_eq!(shares(6), vec![false]);
    }

    #[test]
    fn an_enum_member_knows_whether_its_deletion_shifts_a_value() {
        let source = "enum E {\n  A,\n  B,\n  C = 5,\n  D,\n}\nclass K {\n  m() {}\n}\n";
        let index = index("store.ts", source);
        let keeps = |line| {
            index
                .members_on_line(line)
                .map(|member| member.deletion_keeps_values)
                .collect::<Vec<_>>()
        };
        assert_eq!(keeps(2), vec![false], "B counts on from A");
        assert_eq!(keeps(3), vec![true], "C has its own value");
        assert_eq!(keeps(4), vec![false], "D counts on from C");
        assert_eq!(keeps(5), vec![true], "D is the last member");
        assert_eq!(keeps(8), vec![true], "a class element");
    }

    #[test]
    fn members_on_line_reads_a_nested_class() {
        let source = "export function f() {\n  return class {\n    inner() {}\n  };\n}\n";
        assert_eq!(members(source, 3), vec!["inner() {}"]);
    }

    #[test]
    fn export_statements_on_line_gives_each_keyword_of_the_line() {
        let source = "export const a = 1; export const b = 2;\nexport function f() {\n}\n";
        let index = index("store.ts", source);
        assert_eq!(index.export_statements_on_line(1).count(), 2);
        assert_eq!(index.export_statements_on_line(2).count(), 1);
        assert_eq!(index.export_statements_on_line(3).count(), 0);
    }

    #[test]
    fn gives_none_for_another_file_type_or_a_syntax_error() {
        assert!(SourceEditIndex::parse(Path::new("notes.md"), "export const a = 1;\n").is_none());
        assert!(
            SourceEditIndex::parse(
                Path::new("store.ts"),
                "export const [a, b] = x;\nexport const = ;\n"
            )
            .is_none()
        );
    }
}
