//! Literal-search check for the consumers of an export.
//!
//! An agent finds the users of an export with a whole-word text search for
//! its name. The module graph knows every consumer, also the consumers that
//! such a search does not find: a default import under another name, a barrel
//! alias, a whole-namespace use. This module reads consumer sources on demand,
//! only for the exports that a caller checks, and compares the graph edge with
//! what the text search sees.

use std::path::Path;

use fallow_types::discover::FileId;
use fallow_types::extract::{ExportName, ImportedName};
use fallow_types::trace::{SearchGapReason, SearchVisibility};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::graph::{ExportNamespace, ModuleGraph, ReferenceKind, SymbolReference};

/// The result of the literal-search check for one consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchCheck {
    /// Whether a whole-word search for the export name finds the consumer.
    pub visibility: SearchVisibility,
    /// Why the search does not find the uses. `None` for `found`.
    pub reason: Option<SearchGapReason>,
    /// The name to search for instead, when one name finds the uses.
    pub search_as: Option<String>,
    /// 1-based line of the import statement in the consumer.
    pub line: Option<u32>,
}

/// One source file read from disk, with its line offsets.
struct Source {
    text: String,
    offsets: Vec<u32>,
}

/// Consumer sources, read once per file and only when a check needs them.
pub(crate) struct SourceCache<'g> {
    graph: &'g ModuleGraph,
    sources: FxHashMap<FileId, Option<Source>>,
    forwarded: FxHashMap<FileId, FxHashSet<String>>,
}

impl<'g> SourceCache<'g> {
    pub(crate) fn new(graph: &'g ModuleGraph) -> Self {
        Self {
            graph,
            sources: FxHashMap::default(),
            forwarded: FxHashMap::default(),
        }
    }

    /// Whether `file_id` only exports its import binding `local` again and
    /// has no other use of it. The file is parsed once.
    fn only_forwards(&mut self, file_id: FileId, local: &str) -> bool {
        if !self.forwarded.contains_key(&file_id) {
            let path = self
                .graph
                .modules
                .get(file_id.0 as usize)
                .map(|module| module.path.clone());
            let names = match (path, self.get(file_id)) {
                (Some(path), Some(source)) => forwarded_import_bindings(&path, &source.text),
                _ => FxHashSet::default(),
            };
            self.forwarded.insert(file_id, names);
        }
        self.forwarded
            .get(&file_id)
            .is_some_and(|names| names.contains(local))
    }

    fn get(&mut self, file_id: FileId) -> Option<&Source> {
        let graph = self.graph;
        self.sources
            .entry(file_id)
            .or_insert_with(|| {
                let module = graph.modules.get(file_id.0 as usize)?;
                let text = std::fs::read_to_string(&module.path).ok()?;
                let offsets = fallow_types::extract::compute_line_offsets(&text);
                Some(Source { text, offsets })
            })
            .as_ref()
    }
}

/// The export under check: the name a search uses and the modules that expose
/// the binding, each with the names it exposes.
pub(crate) struct SearchTarget {
    term: Option<String>,
    exposures: FxHashMap<FileId, Vec<String>>,
}

impl SearchTarget {
    /// Build the target for `export_name` of `file_id`.
    ///
    /// The search term of a named export is its name. The search term of a
    /// default export is the name of its declaration (`export default function
    /// Widget`), because nobody searches for `default`. An anonymous default
    /// export has no search term.
    pub(crate) fn new(
        graph: &ModuleGraph,
        sources: &mut SourceCache<'_>,
        file_id: FileId,
        export_name: &str,
        namespace: ExportNamespace,
    ) -> Self {
        let term = if export_name == "default" {
            default_export_term(graph, sources, file_id)
        } else {
            Some(export_name.to_string())
        };
        let mut exposures: FxHashMap<FileId, Vec<String>> = FxHashMap::default();
        exposures
            .entry(file_id)
            .or_default()
            .push(export_name.to_string());
        for route in graph.effective_re_export_routes(file_id, export_name, namespace) {
            let names = exposures.entry(route.barrel_file()).or_default();
            if !names.iter().any(|name| name == route.exported_name()) {
                names.push(route.exported_name().to_string());
            }
        }
        Self { term, exposures }
    }

    /// Check one graph reference against a text search for the export name.
    /// Returns `None` when the consumer source cannot be read.
    pub(crate) fn check(
        &self,
        graph: &ModuleGraph,
        sources: &mut SourceCache<'_>,
        reference: &SymbolReference,
    ) -> Option<SearchCheck> {
        let source = sources.get(reference.from_file)?;
        let found = self
            .term
            .as_deref()
            .is_some_and(|term| contains_whole_word(&source.text, term));
        let line = (reference.import_span.end > reference.import_span.start).then(|| {
            fallow_types::extract::byte_offset_to_line_col(
                &source.offsets,
                reference.import_span.start,
            )
            .0
        });
        let binding = self.binding(graph, reference);
        let forwards = match &binding {
            Some(Binding::ReExport) => true,
            Some(Binding::Named { imported, local }) => {
                let name = if local.is_empty() { imported } else { local };
                sources.only_forwards(reference.from_file, name)
            }
            Some(Binding::Default { local }) => {
                !local.is_empty() && sources.only_forwards(reference.from_file, local)
            }
            // The graph does not credit the importers of a forwarded
            // namespace binding, so the forwarding file stays a consumer.
            Some(Binding::Namespace { .. }) | None => false,
        };
        let (visibility, reason, search_as) = if forwards {
            (
                SearchVisibility::Found,
                Some(SearchGapReason::PassThrough),
                None,
            )
        } else if found {
            self.classify_found(binding.as_ref())
        } else {
            self.classify_missed(binding.as_ref(), reference.kind)
        };
        Some(SearchCheck {
            visibility,
            reason,
            search_as,
            line,
        })
    }

    fn is_term(&self, name: &str) -> bool {
        self.term.as_deref() == Some(name)
    }

    fn classify_found(&self, binding: Option<&Binding<'_>>) -> Classification {
        match binding {
            Some(Binding::Named { imported, local })
                if !local.is_empty() && !self.is_term(local) =>
            {
                let reason = if self.is_term(imported) {
                    SearchGapReason::ImportAlias
                } else {
                    SearchGapReason::ReExportAlias
                };
                renamed(reason, local)
            }
            Some(Binding::Default { local }) if !local.is_empty() && !self.is_term(local) => {
                renamed(SearchGapReason::DefaultImport, local)
            }
            _ => (SearchVisibility::Found, None, None),
        }
    }

    fn classify_missed(
        &self,
        binding: Option<&Binding<'_>>,
        kind: ReferenceKind,
    ) -> Classification {
        let (reason, search_as) = match binding {
            _ if kind == ReferenceKind::DynamicImport => (SearchGapReason::DynamicImport, None),
            Some(Binding::Named { imported, local }) => {
                let reason = if self.is_term(imported) {
                    SearchGapReason::ImportAlias
                } else {
                    SearchGapReason::ReExportAlias
                };
                let name = if local.is_empty() { imported } else { local };
                (reason, Some((*name).to_string()))
            }
            Some(Binding::Default { local }) => (SearchGapReason::DefaultImport, non_empty(local)),
            Some(Binding::Namespace { local }) => (SearchGapReason::NamespaceUse, non_empty(local)),
            Some(Binding::ReExport) | None => (reason_for_kind(kind), None),
        };
        (SearchVisibility::Missed, Some(reason), search_as)
    }

    /// Find the binding behind a graph reference. The import span and an
    /// exposing module identify the binding, the same way the graph credits
    /// the reference. When the span names no import, any import of the export
    /// in the same file wins over a re-export: a barrel that also uses the
    /// export is a consumer, and only a barrel without such an import passes
    /// the export through.
    fn binding<'g>(
        &self,
        graph: &'g ModuleGraph,
        reference: &SymbolReference,
    ) -> Option<Binding<'g>> {
        self.import_binding(graph, reference.from_file, Some(reference.import_span))
            .or_else(|| self.import_binding(graph, reference.from_file, None))
            .or_else(|| self.re_export_binding(graph, reference))
    }

    /// The import binding of the export in `from_file`, restricted to the
    /// import at `span` when one is given.
    fn import_binding<'g>(
        &self,
        graph: &'g ModuleGraph,
        from_file: FileId,
        span: Option<oxc_span::Span>,
    ) -> Option<Binding<'g>> {
        let mut namespace_binding = None;
        for (target, symbols) in graph.outgoing_symbol_edges(from_file) {
            let Some(names) = self.exposures.get(&target) else {
                continue;
            };
            for symbol in symbols
                .iter()
                .filter(|symbol| span.is_none_or(|span| symbol.import_span == span))
            {
                match &symbol.imported_name {
                    ImportedName::Named(imported) if names.contains(imported) => {
                        return Some(Binding::Named {
                            imported,
                            local: &symbol.local_name,
                        });
                    }
                    ImportedName::Default if names.iter().any(|name| name == "default") => {
                        return Some(Binding::Default {
                            local: &symbol.local_name,
                        });
                    }
                    ImportedName::Namespace => {
                        namespace_binding.get_or_insert(Binding::Namespace {
                            local: &symbol.local_name,
                        });
                    }
                    _ => {}
                }
            }
        }
        namespace_binding
    }

    fn re_export_binding<'g>(
        &self,
        graph: &'g ModuleGraph,
        reference: &SymbolReference,
    ) -> Option<Binding<'g>> {
        let module = graph.modules.get(reference.from_file.0 as usize)?;
        let forwards = |re_export: &&crate::graph::ReExportEdge| {
            self.exposures
                .get(&re_export.source_file)
                .is_some_and(|names| {
                    re_export.imported_name == "*" || names.contains(&re_export.imported_name)
                })
        };
        module
            .re_exports
            .iter()
            .any(|re_export| forwards(&re_export))
            .then_some(Binding::ReExport)
    }
}

type Classification = (SearchVisibility, Option<SearchGapReason>, Option<String>);

fn renamed(reason: SearchGapReason, local: &str) -> Classification {
    (
        SearchVisibility::Renamed,
        Some(reason),
        Some(local.to_string()),
    )
}

fn non_empty(name: &str) -> Option<String> {
    (!name.is_empty()).then(|| name.to_string())
}

const fn reason_for_kind(kind: ReferenceKind) -> SearchGapReason {
    match kind {
        ReferenceKind::DefaultImport => SearchGapReason::DefaultImport,
        ReferenceKind::NamespaceImport => SearchGapReason::NamespaceUse,
        ReferenceKind::DynamicImport | ReferenceKind::SideEffectImport => {
            SearchGapReason::DynamicImport
        }
        ReferenceKind::NamedImport | ReferenceKind::ReExport => SearchGapReason::ReExportAlias,
    }
}

/// How a consumer binds the export.
enum Binding<'g> {
    Named {
        imported: &'g str,
        local: &'g str,
    },
    Default {
        local: &'g str,
    },
    Namespace {
        local: &'g str,
    },
    /// The consumer forwards the export and has no import of it.
    ReExport,
}

/// The import bindings of a source file that the file only exports again
/// (`import { a } from './x'; export { a }`, `import b from './x'; export
/// default b`) and does not use in any other place.
///
/// Only plain script files are read. A Vue, Svelte, Astro or MDX file can use
/// a binding in its template, so it has no such bindings. A file that does not
/// parse has none.
#[must_use]
fn forwarded_import_bindings(path: &Path, source: &str) -> FxHashSet<String> {
    use oxc_ast::ast::{ImportDeclarationSpecifier, Statement};
    use oxc_ast_visit::Visit;

    let Ok(source_type) = oxc_span::SourceType::from_path(path) else {
        return FxHashSet::default();
    };
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    if parsed.fatal_error {
        return FxHashSet::default();
    }
    let imports: FxHashSet<&str> = parsed
        .program
        .body
        .iter()
        .filter_map(|statement| match statement {
            Statement::ImportDeclaration(import) => import.specifiers.as_ref(),
            _ => None,
        })
        .flatten()
        .map(|specifier| match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(named) => named.local.name.as_str(),
            ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                default.local.name.as_str()
            }
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                namespace.local.name.as_str()
            }
        })
        .collect();
    if imports.is_empty() {
        return FxHashSet::default();
    }
    let mut uses = BindingUses {
        imports,
        exported: FxHashSet::default(),
        used: FxHashSet::default(),
    };
    uses.visit_program(&parsed.program);
    uses.exported
        .difference(&uses.used)
        .map(|name| (*name).to_string())
        .collect()
}

/// The uses of the import bindings of one file, split in export positions
/// and all other positions.
struct BindingUses<'s> {
    imports: FxHashSet<&'s str>,
    exported: FxHashSet<&'s str>,
    used: FxHashSet<&'s str>,
}

impl BindingUses<'_> {
    fn record(&mut self, name: &str, exported: bool) {
        let Some(&name) = self.imports.get(name) else {
            return;
        };
        if exported {
            self.exported.insert(name);
        } else {
            self.used.insert(name);
        }
    }
}

impl<'a> oxc_ast_visit::Visit<'a> for BindingUses<'_> {
    fn visit_identifier_reference(&mut self, it: &oxc_ast::ast::IdentifierReference<'a>) {
        self.record(it.name.as_str(), false);
    }

    fn visit_export_named_declaration(&mut self, it: &oxc_ast::ast::ExportNamedDeclaration<'a>) {
        for specifier in &it.specifiers {
            self.record(specifier.local.name().as_str(), true);
        }
    }

    fn visit_export_default_declaration(
        &mut self,
        it: &oxc_ast::ast::ExportDefaultDeclaration<'a>,
    ) {
        if let oxc_ast::ast::ExportDefaultDeclarationKind::Identifier(identifier) = &it.declaration
        {
            self.record(identifier.name.as_str(), true);
        } else {
            oxc_ast_visit::walk::walk_export_default_declaration(self, it);
        }
    }

    fn visit_ts_export_assignment(&mut self, it: &oxc_ast::ast::TSExportAssignment<'a>) {
        if let oxc_ast::ast::Expression::Identifier(identifier) = &it.expression {
            self.record(identifier.name.as_str(), true);
        } else {
            oxc_ast_visit::walk::walk_ts_export_assignment(self, it);
        }
    }
}

/// The search term of the default export of `file_id`: the name of the
/// declaration, when it has one.
fn default_export_term(
    graph: &ModuleGraph,
    sources: &mut SourceCache<'_>,
    file_id: FileId,
) -> Option<String> {
    let export = graph
        .modules
        .get(file_id.0 as usize)?
        .exports
        .iter()
        .find(|export| matches!(export.name, ExportName::Default))?;
    let source = sources.get(file_id)?;
    default_declaration_name(&source.text, export.span.start as usize).map(str::to_string)
}

/// Whether `byte` belongs to a word, with the word characters of `grep -w`:
/// letters, digits and the underscore. Non-ASCII bytes count as letters.
const fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Whether `source` contains `word` as a whole-word token, as `grep -w` sees
/// it. Matches in comments and strings count, because a text search finds
/// them too.
#[must_use]
pub fn contains_whole_word(source: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let bytes = source.as_bytes();
    source.match_indices(word).any(|(start, _)| {
        let end = start + word.len();
        let before = start.checked_sub(1).map(|index| bytes[index]);
        let after = bytes.get(end).copied();
        !before.is_some_and(is_word_byte) && !after.is_some_and(is_word_byte)
    })
}

fn identifier_prefix(text: &str) -> &str {
    let end = text
        .char_indices()
        .find(|(_, ch)| !(ch.is_alphanumeric() || *ch == '_' || *ch == '$'))
        .map_or(text.len(), |(index, _)| index);
    &text[..end]
}

/// Strip `keyword` from the front of `text` when a non-word character follows.
fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = text.strip_prefix(keyword)?;
    let next = rest.chars().next();
    if next.is_some_and(|ch| ch.is_alphanumeric() || ch == '_' || ch == '$') {
        return None;
    }
    Some(rest.trim_start())
}

/// The declaration name of a default export whose source starts at `start`.
///
/// Handles `export default function Name`, `export default class Name`,
/// `export default Name;` and the `Name as default` export specifier.
/// Returns `None` for an anonymous declaration or an expression.
#[must_use]
pub fn default_declaration_name(source: &str, start: usize) -> Option<&str> {
    let text = source.get(start..)?.trim_start();
    let Some(after_export) = strip_keyword(text, "export") else {
        let name = identifier_prefix(text);
        let rest = text[name.len()..].trim_start();
        return (!name.is_empty() && strip_keyword(rest, "as")?.starts_with("default"))
            .then_some(name);
    };
    let mut rest = strip_keyword(after_export, "default")?;
    let mut declaration = false;
    loop {
        if let Some(next) = ["async", "function", "abstract", "class"]
            .iter()
            .find_map(|keyword| strip_keyword(rest, keyword))
        {
            declaration = true;
            rest = next;
        } else if let Some(next) = rest.strip_prefix('*') {
            rest = next.trim_start();
        } else {
            break;
        }
    }
    let name = identifier_prefix(rest);
    if name.is_empty() {
        return None;
    }
    if declaration {
        return Some(name);
    }
    let after = rest[name.len()..].trim_start();
    (after.is_empty() || after.starts_with(';') || after.starts_with('\n')).then_some(name)
}

/// The 1-based line range of the top-level statement that declares each
/// export of a source file, keyed by the byte offset of the export span.
///
/// An `export { name }` specifier maps to the statement that declares `name`
/// in the same file. For a Vue or Svelte SFC, an Astro component or an MDX
/// document, only the script sections are parsed, and the statement offsets
/// map back to the file. A file that does not parse maps to nothing, and the
/// caller falls back to the export line.
#[must_use]
pub fn declaration_line_ranges(
    path: &Path,
    source: &str,
    export_starts: &[u32],
) -> FxHashMap<u32, (u32, u32)> {
    let statements = match fallow_extract::embedded_script_sections(path, source) {
        Some(sections) => sections
            .iter()
            .flat_map(|section| {
                top_level_statements(&section.extraction.body, section.source_type)
                    .into_iter()
                    .map(|statement| statement.remapped(&section.extraction))
            })
            .collect(),
        None => oxc_span::SourceType::from_path(path)
            .map(|source_type| top_level_statements(source, source_type))
            .unwrap_or_default(),
    };
    let offsets = fallow_types::extract::compute_line_offsets(source);
    let line = |offset: u32| fallow_types::extract::byte_offset_to_line_col(&offsets, offset).0;
    let mut ranges = FxHashMap::default();
    for &start in export_starts {
        let Some(containing) = statements
            .iter()
            .find(|statement| statement.start <= start && start < statement.end)
        else {
            continue;
        };
        let statement = if containing.is_specifier_list {
            let local = source
                .get(start as usize..)
                .map(identifier_prefix)
                .unwrap_or_default();
            statements
                .iter()
                .find(|statement| statement.declared.iter().any(|name| name == local))
                .unwrap_or(containing)
        } else {
            containing
        };
        ranges.insert(
            start,
            (line(statement.start), line(statement.end.saturating_sub(1))),
        );
    }
    ranges
}

struct TopLevelStatement {
    start: u32,
    end: u32,
    declared: Vec<String>,
    is_specifier_list: bool,
}

impl TopLevelStatement {
    /// Map the statement span from the extracted script to the file.
    fn remapped(self, extraction: &fallow_extract::ExtractionResult) -> Self {
        let span = extraction.remap_span(oxc_span::Span::new(self.start, self.end));
        Self {
            start: span.start,
            end: span.end,
            ..self
        }
    }
}

/// The top-level statements of a script. A script that does not parse has
/// none.
fn top_level_statements(source: &str, source_type: oxc_span::SourceType) -> Vec<TopLevelStatement> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, source_type).parse();
    if parsed.fatal_error {
        return Vec::new();
    }
    parsed
        .program
        .body
        .iter()
        .map(top_level_statement)
        .collect()
}

fn top_level_statement(statement: &oxc_ast::ast::Statement<'_>) -> TopLevelStatement {
    use oxc_ast::ast::Statement;
    use oxc_span::GetSpan;

    let span = statement.span();
    let (declared, is_specifier_list) = match statement {
        Statement::ExportDeclaration(export) => (declared_names(&export.declaration), false),
        Statement::ExportNamedDeclaration(_) => (Vec::new(), true),
        _ => (
            statement
                .as_declaration()
                .map(declared_names)
                .unwrap_or_default(),
            false,
        ),
    };
    TopLevelStatement {
        start: span.start,
        end: span.end,
        declared,
        is_specifier_list,
    }
}

fn declared_names(declaration: &oxc_ast::ast::Declaration<'_>) -> Vec<String> {
    use oxc_ast::ast::Declaration;

    match declaration {
        Declaration::VariableDeclaration(variable) => variable
            .declarations
            .iter()
            .filter_map(|declarator| declarator.id.get_binding_identifier())
            .map(|identifier| identifier.name.to_string())
            .collect(),
        Declaration::FunctionDeclaration(function) => function
            .id
            .iter()
            .map(|identifier| identifier.name.to_string())
            .collect(),
        Declaration::ClassDeclaration(class) => class
            .id
            .iter()
            .map(|identifier| identifier.name.to_string())
            .collect(),
        Declaration::TSTypeAliasDeclaration(alias) => vec![alias.id.name.to_string()],
        Declaration::TSInterfaceDeclaration(interface) => vec![interface.id.name.to_string()],
        Declaration::TSEnumDeclaration(enumeration) => vec![enumeration.id.name.to_string()],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_word_matches_like_grep_w() {
        assert!(contains_whole_word("import { foo } from './x'", "foo"));
        assert!(contains_whole_word("ns.foo()", "foo"));
        assert!(contains_whole_word("$foo", "foo"));
        assert!(contains_whole_word("// foo is used", "foo"));
        assert!(!contains_whole_word("foobar()", "foo"));
        assert!(!contains_whole_word("my_foo()", "foo"));
        assert!(!contains_whole_word("foo2", "foo"));
        assert!(!contains_whole_word("", "foo"));
        assert!(!contains_whole_word("foo", ""));
        assert!(contains_whole_word("a foobar foo", "foo"));
    }

    fn forwarded(path: &str, source: &str) -> Vec<String> {
        let mut names: Vec<String> = forwarded_import_bindings(Path::new(path), source)
            .into_iter()
            .collect();
        names.sort();
        names
    }

    #[test]
    fn forwarded_bindings_are_only_exported_again() {
        assert_eq!(
            forwarded("a.ts", "import { a } from './x';\nexport { a };\n"),
            ["a"]
        );
        assert_eq!(
            forwarded("a.ts", "import b from './x';\nexport default b;\n"),
            ["b"]
        );
        assert_eq!(
            forwarded(
                "a.ts",
                "import { a as c } from './x';\nexport { c as d };\n"
            ),
            ["c"]
        );
        assert_eq!(
            forwarded(
                "a.ts",
                "import b = require('./x');\nimport c from './y';\nexport = c;\n"
            ),
            ["c"]
        );
        assert_eq!(
            forwarded(
                "a.ts",
                "import type { T } from './x';\nexport type { T };\n"
            ),
            ["T"]
        );
    }

    #[test]
    fn forwarded_bindings_exclude_local_uses_and_unused_imports() {
        let empty: [&str; 0] = [];
        assert_eq!(
            forwarded(
                "a.ts",
                "import { a } from './x';\nexport { a };\nexport const b = a * 2;\n"
            ),
            empty
        );
        assert_eq!(
            forwarded(
                "a.tsx",
                "import A from './x';\nexport { A };\nconst v = <A />;\n"
            ),
            empty
        );
        assert_eq!(
            forwarded("a.ts", "import { a } from './x';\nexport default a + 1;\n"),
            empty
        );
        assert_eq!(forwarded("a.ts", "import { a } from './x';\n"), empty);
        assert_eq!(
            forwarded(
                "a.ts",
                "import { a } from './x';\nexport { a } from './y';\n"
            ),
            empty
        );
        assert_eq!(
            forwarded(
                "A.vue",
                "<script>\nimport a from './x';\nexport { a };\n</script>\n"
            ),
            empty
        );
    }

    #[test]
    fn default_declaration_name_reads_named_declarations() {
        let name = |source: &'static str| default_declaration_name(source, 0);
        assert_eq!(name("export default function Widget() {}"), Some("Widget"));
        assert_eq!(
            name("export default async function load() {}"),
            Some("load")
        );
        assert_eq!(name("export default function* gen() {}"), Some("gen"));
        assert_eq!(name("export default class Store {}"), Some("Store"));
        assert_eq!(name("export default abstract class Base {}"), Some("Base"));
        assert_eq!(name("export default config;"), Some("config"));
        assert_eq!(name("export default config\n"), Some("config"));
        assert_eq!(name("Widget as default"), Some("Widget"));
    }

    #[test]
    fn default_declaration_name_rejects_anonymous_and_expressions() {
        let name = |source: &'static str| default_declaration_name(source, 0);
        assert_eq!(name("export default function () {}"), None);
        assert_eq!(name("export default class {}"), None);
        assert_eq!(name("export default defineConfig({});"), None);
        assert_eq!(name("export default { a: 1 };"), None);
        assert_eq!(name("Widget as Other"), None);
    }

    #[test]
    fn declaration_ranges_cover_the_whole_statement() {
        let source = "export const a = 1;\n\nexport function b() {\n  return 2;\n}\n\nconst c = 3;\nexport { c };\n";
        let a = source.find("a =").unwrap() as u32;
        let b = source.find("b()").unwrap() as u32;
        let c = source.rfind('c').unwrap() as u32;
        let ranges = declaration_line_ranges(Path::new("x.ts"), source, &[a, b, c]);
        assert_eq!(ranges.get(&a), Some(&(1, 1)));
        assert_eq!(ranges.get(&b), Some(&(3, 5)));
        assert_eq!(ranges.get(&c), Some(&(7, 7)));
    }

    fn range_of(path: &str, source: &str, needle: &str) -> Option<(u32, u32)> {
        let start = source.find(needle)? as u32;
        declaration_line_ranges(Path::new(path), source, &[start])
            .get(&start)
            .copied()
    }

    #[test]
    fn declaration_ranges_read_the_script_blocks_of_an_sfc() {
        let vue = "<template>\n  <div />\n</template>\n\n<script lang=\"ts\">\nexport function scale(n: number): number {\n  return n * 2;\n}\n</script>\n";
        assert_eq!(range_of("Gauge.vue", vue, "scale("), Some((6, 8)));

        let svelte = "<script context=\"module\">\nexport const size = {\n  width: 1,\n};\n</script>\n\n<script>\n  let open = false;\n</script>\n\n<div />\n";
        assert_eq!(range_of("Box.svelte", svelte, "size ="), Some((2, 4)));
    }

    #[test]
    fn declaration_ranges_read_astro_frontmatter_and_mdx_statements() {
        let astro =
            "---\nexport function title(): string {\n  return 'a';\n}\n---\n<h1>{title()}</h1>\n";
        assert_eq!(range_of("Page.astro", astro, "title()"), Some((2, 4)));

        let mdx =
            "# Post\n\nSome prose.\n\nexport const meta = {\n  title: 'Post',\n};\n\nMore prose.\n";
        assert_eq!(range_of("post.mdx", mdx, "meta ="), Some((5, 7)));
    }
}
