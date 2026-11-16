//! The imports of a React Router route module that only its server-only
//! exports use.
//!
//! For the client build, React Router removes the `loader`, `action`,
//! `middleware` and `headers` exports of each route module. Dead-code
//! elimination then removes each top-level declaration and import binding
//! that only the removed code used. This module applies the same rule to the
//! syntax tree:
//!
//! 1. Each top-level statement is a client root, a server-only export, an
//!    import, or a local declaration. A local declaration is live only when
//!    live code uses it.
//! 2. Liveness spreads from the client roots through the value references to
//!    top-level bindings.
//! 3. An import binding with at least one value reference, but no reference
//!    from live code, is server-only. Dead-code elimination keeps an import
//!    that nothing used before the export removal, so such an import stays.
//!
//! The result holds the start offsets of the server-only import specifiers
//! and of the `import()` expressions in dead code. These offsets are the
//! `import_span` starts of the graph edges of the module. It also holds the
//! env reads (`process.env.X`, `import.meta.env.X`) that only dead code
//! makes, so that a secret read in a `loader` does not make the route module
//! a secret source.

use std::path::Path;

use oxc_ast::AstKind;
use oxc_ast::ast::{Declaration, ImportDeclarationSpecifier, Program, Statement};
use oxc_ast_visit::Visit;
use oxc_semantic::{Semantic, SemanticBuilder, SymbolId};
use oxc_span::{GetSpan, SourceType, Span};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::plugins::react_router::SERVER_ONLY_ROUTE_EXPORTS;

/// The UTF-8 byte order mark. Extraction measures offsets after it.
const BOM: char = '\u{FEFF}';

/// The `import_span` start that the graph gives to the edge symbol of every
/// re-export (`export { x } from './y'`) and of every dynamic import pattern.
const RE_EXPORT_SPAN_START: u32 = 0;

/// The env sources whose reads the split tracks, as `member_accesses` names
/// them.
const PROCESS_ENV: &str = "process.env";
const IMPORT_META_ENV: &str = "import.meta.env";

/// What the client build of one route module drops.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct RouteModuleSplit {
    /// The `import_span` starts of the edges that only server-only code uses.
    pub(super) server_only_spans: FxHashSet<u32>,
    /// The env reads, such as `process.env.SESSION_SECRET`, that only dead code
    /// makes. A read in another form stays counted.
    pub(super) dead_only_env_reads: FxHashSet<String>,
}

/// Read and split one route module. `None` when the file cannot be read or
/// parsed cleanly: the caller then keeps every import and env read.
pub(super) fn split_route_module(path: &Path) -> Option<RouteModuleSplit> {
    let source_type = SourceType::from_path(path).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let source = text.strip_prefix(BOM).unwrap_or(&text);
    fallow_extract::parse_bounded_or_default(source, source_type, |parsed| {
        if parsed.fatal_error || parsed.diagnostics.has_errors() {
            return None;
        }
        Some(split_program(&parsed.program))
    })
}

/// The role of one top-level statement in the client build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatementRole {
    /// Code that ships to the client: a client export, a side effect, or a
    /// re-export list.
    ClientRoot,
    /// A `loader`, `action`, `middleware` or `headers` export declaration.
    ServerExport,
    /// A local declaration, live only when live code uses it.
    Local,
    /// An import declaration or a type-only declaration.
    Inert,
}

fn split_program(program: &Program<'_>) -> RouteModuleSplit {
    let semantic = SemanticBuilder::new()
        .with_build_nodes(true)
        .build(program)
        .semantic;
    let statements: Vec<Span> = program.body.iter().map(GetSpan::span).collect();
    let roles: Vec<StatementRole> = program.body.iter().map(statement_role).collect();
    let index_of = |offset: u32| statement_index(&statements, offset);
    let references = TopLevelReferences::collect(&semantic, &index_of);
    let live = live_statements(&roles, &references);

    let mut spans = FxHashSet::default();
    let mut live_env = EnvReads::default();
    let mut dead_env = EnvReads::default();
    for (index, statement) in program.body.iter().enumerate() {
        if let Statement::ImportDeclaration(import) = statement {
            add_server_only_specifiers(import, &semantic, &references, &live, &mut spans);
        } else if live[index] {
            live_env.visit_statement(statement);
        } else {
            spans.extend(dynamic_import_starts(statement));
            dead_env.visit_statement(statement);
        }
    }
    if re_exports_only_server_exports(program) && !has_other_offset_zero_edge(program) {
        spans.insert(RE_EXPORT_SPAN_START);
    }
    RouteModuleSplit {
        server_only_spans: spans,
        dead_only_env_reads: dead_env.0.difference(&live_env.0).cloned().collect(),
    }
}

/// The static env reads in a statement, as `process.env.X` or
/// `import.meta.env.X`.
#[derive(Default)]
struct EnvReads(FxHashSet<String>);

impl<'a> Visit<'a> for EnvReads {
    fn visit_static_member_expression(
        &mut self,
        member: &oxc_ast::ast::StaticMemberExpression<'a>,
    ) {
        if let Some(source) = env_source(&member.object) {
            self.0.insert(format!("{source}.{}", member.property.name));
        }
        oxc_ast_visit::walk::walk_static_member_expression(self, member);
    }
}

/// `process.env` or `import.meta.env` when `object` is one of them.
fn env_source(object: &oxc_ast::ast::Expression<'_>) -> Option<&'static str> {
    let oxc_ast::ast::Expression::StaticMemberExpression(env) = object else {
        return None;
    };
    if env.property.name != "env" {
        return None;
    }
    match &env.object {
        oxc_ast::ast::Expression::Identifier(id) if id.name == "process" => Some(PROCESS_ENV),
        oxc_ast::ast::Expression::ImportMeta(_) => Some(IMPORT_META_ENV),
        _ => None,
    }
}

/// Whether the module has a re-export and every re-export names only
/// server-only exports (`export { action } from './note.server'`). React
/// Router removes such a specifier from the client build. All re-export edges
/// share one span start, so a module with a mix of client and server-only
/// re-exports keeps them all.
fn re_exports_only_server_exports(program: &Program<'_>) -> bool {
    let mut found = false;
    for statement in &program.body {
        match statement {
            Statement::ExportFromDeclaration(export) => {
                if !export
                    .specifiers
                    .iter()
                    .all(|specifier| is_server_export_name(&specifier.exported.name()))
                {
                    return false;
                }
                found = true;
            }
            Statement::ExportAllDeclaration(_) => return false,
            _ => {}
        }
    }
    found
}

/// Whether another edge of the module can have the re-export span start: a
/// side-effect import at the very start of the file, or a dynamic import
/// pattern (`import(\`./${x}\`)`, `import.meta.glob`, `require.context`).
fn has_other_offset_zero_edge(program: &Program<'_>) -> bool {
    #[derive(Default)]
    struct Finder(bool);
    impl<'a> Visit<'a> for Finder {
        fn visit_import_expression(&mut self, expression: &oxc_ast::ast::ImportExpression<'a>) {
            if !matches!(
                expression.source,
                oxc_ast::ast::Expression::StringLiteral(_)
            ) {
                self.0 = true;
            }
            oxc_ast_visit::walk::walk_import_expression(self, expression);
        }
        fn visit_static_member_expression(
            &mut self,
            member: &oxc_ast::ast::StaticMemberExpression<'a>,
        ) {
            let pattern = match &member.object {
                oxc_ast::ast::Expression::ImportMeta(_) => member.property.name == "glob",
                oxc_ast::ast::Expression::Identifier(id) => {
                    id.name == "require" && member.property.name == "context"
                }
                _ => false,
            };
            self.0 |= pattern;
            oxc_ast_visit::walk::walk_static_member_expression(self, member);
        }
    }
    if let Some(Statement::ImportDeclaration(import)) = program.body.first()
        && import.span.start == RE_EXPORT_SPAN_START
        && import
            .specifiers
            .as_ref()
            .is_none_or(|specifiers| specifiers.is_empty())
    {
        // A side-effect import records the span of its whole declaration.
        return true;
    }
    let mut finder = Finder::default();
    finder.visit_program(program);
    finder.0
}

/// The index of the top-level statement that holds `offset`.
fn statement_index(statements: &[Span], offset: u32) -> Option<usize> {
    let index = statements.partition_point(|span| span.start <= offset);
    let index = index.checked_sub(1)?;
    (offset < statements[index].end).then_some(index)
}

fn statement_role(statement: &Statement<'_>) -> StatementRole {
    match statement {
        Statement::ImportDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_) => StatementRole::Inert,
        Statement::ExportDeclaration(export)
            if declares_only_server_exports(&export.declaration) =>
        {
            StatementRole::ServerExport
        }
        Statement::ExportDeclaration(export) if export.declaration.is_typescript_syntax() => {
            StatementRole::Inert
        }
        Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_)
        | Statement::VariableDeclaration(_) => StatementRole::Local,
        _ => StatementRole::ClientRoot,
    }
}

fn is_server_export_name(name: &str) -> bool {
    SERVER_ONLY_ROUTE_EXPORTS.contains(&name)
}

fn declares_only_server_exports(declaration: &Declaration<'_>) -> bool {
    match declaration {
        Declaration::FunctionDeclaration(function) => function
            .id
            .as_ref()
            .is_some_and(|id| is_server_export_name(&id.name)),
        Declaration::VariableDeclaration(variable) => {
            let mut names = variable
                .declarations
                .iter()
                .flat_map(|declarator| declarator.id.get_binding_identifiers())
                .peekable();
            names.peek().is_some() && names.all(|id| is_server_export_name(&id.name))
        }
        _ => false,
    }
}

/// The value references between the top-level statements of a module.
struct TopLevelReferences {
    /// For each top-level binding: the statement that declares it.
    declared_in: Vec<(SymbolId, usize)>,
    /// For each top-level binding: the statements that read it in the client
    /// build. A `export { x as loader }` specifier does not count.
    read_from: FxHashMap<SymbolId, Vec<usize>>,
    /// The statements that declare a binding that another statement reads
    /// before the export removal, a server-only export specifier included.
    read_before_removal: FxHashSet<usize>,
}

impl TopLevelReferences {
    fn collect(semantic: &Semantic<'_>, index_of: &impl Fn(u32) -> Option<usize>) -> Self {
        let scoping = semantic.scoping();
        let nodes = semantic.nodes();
        let mut declared_in = Vec::new();
        let mut read_from = FxHashMap::default();
        let mut read_before_removal = FxHashSet::default();
        for symbol in scoping.iter_bindings_in(scoping.root_scope_id()) {
            let declaration = nodes.get_node(scoping.symbol_declaration(symbol)).span();
            let Some(declaring) = index_of(declaration.start) else {
                continue;
            };
            declared_in.push((symbol, declaring));
            let mut readers = Vec::new();
            for reference in scoping.get_resolved_references(symbol) {
                let node_id = reference.node_id();
                let Some(reader) = index_of(nodes.get_node(node_id).span().start) else {
                    continue;
                };
                if !reference.is_value() {
                    continue;
                }
                if reader != declaring {
                    read_before_removal.insert(declaring);
                }
                if !is_server_export_specifier(semantic, node_id) {
                    readers.push(reader);
                }
            }
            read_from.insert(symbol, readers);
        }
        Self {
            declared_in,
            read_from,
            read_before_removal,
        }
    }

    fn readers(&self, symbol: SymbolId) -> &[usize] {
        self.read_from.get(&symbol).map_or(&[], Vec::as_slice)
    }
}

/// Whether a reference is the local side of `export { x as loader }`. React
/// Router removes that specifier from the client build.
fn is_server_export_specifier(semantic: &Semantic<'_>, node_id: oxc_syntax::node::NodeId) -> bool {
    semantic.nodes().ancestor_kinds(node_id).any(|kind| {
        matches!(kind, AstKind::ExportSpecifier(specifier)
            if is_server_export_name(&specifier.exported.name()))
    })
}

/// Spread liveness from the client roots to the local declarations that
/// live code reads. Dead-code elimination removes only a declaration that
/// other code read before the export removal, so a declaration that no other
/// statement reads stays live.
fn live_statements(roles: &[StatementRole], references: &TopLevelReferences) -> Vec<bool> {
    let mut live: Vec<bool> = roles
        .iter()
        .map(|role| *role == StatementRole::ClientRoot)
        .collect();
    for (index, role) in roles.iter().enumerate() {
        if *role == StatementRole::Local && !references.read_before_removal.contains(&index) {
            live[index] = true;
        }
    }
    loop {
        let mut changed = false;
        for &(symbol, declaring) in &references.declared_in {
            if live[declaring] || roles[declaring] == StatementRole::Inert {
                continue;
            }
            if references
                .readers(symbol)
                .iter()
                .any(|&reader| reader != declaring && live[reader])
            {
                live[declaring] = true;
                changed = true;
            }
        }
        if !changed {
            return live;
        }
    }
}

fn add_server_only_specifiers(
    import: &oxc_ast::ast::ImportDeclaration<'_>,
    semantic: &Semantic<'_>,
    references: &TopLevelReferences,
    live: &[bool],
    spans: &mut FxHashSet<u32>,
) {
    let Some(specifiers) = &import.specifiers else {
        return;
    };
    for specifier in specifiers {
        let (local, span) = match specifier {
            ImportDeclarationSpecifier::ImportSpecifier(s) => (&s.local, s.span),
            ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => (&s.local, s.span),
            ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => (&s.local, s.span),
        };
        let Some(symbol) = local.symbol_id.get() else {
            continue;
        };
        let readers = references.readers(symbol);
        let has_value_reference = semantic
            .scoping()
            .get_resolved_references(symbol)
            .any(|reference| reference.is_value());
        if has_value_reference && !readers.iter().any(|&reader| live[reader]) {
            spans.insert(span.start);
        }
    }
}

/// The start offsets of the `import()` expressions in a statement.
fn dynamic_import_starts(statement: &Statement<'_>) -> Vec<u32> {
    #[derive(Default)]
    struct Collector(Vec<u32>);
    impl<'a> Visit<'a> for Collector {
        fn visit_import_expression(&mut self, expression: &oxc_ast::ast::ImportExpression<'a>) {
            self.0.push(expression.span.start);
            oxc_ast_visit::walk::walk_import_expression(self, expression);
        }
    }
    let mut collector = Collector::default();
    collector.visit_statement(statement);
    collector.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(source: &str) -> Vec<String> {
        fallow_extract::parse_bounded_or_default(source, SourceType::tsx(), |parsed| {
            let spans = split_program(&parsed.program).server_only_spans;
            let mut names: Vec<String> = spans
                .iter()
                .map(|&start| {
                    let rest = &source[start as usize..];
                    rest.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '(')
                        .next()
                        .unwrap_or_default()
                        .to_owned()
                })
                .collect();
            names.sort();
            names
        })
    }

    #[test]
    fn an_import_that_only_the_loader_reads_is_server_only() {
        let names = split(
            "import { db } from './db'\nimport { ui } from './ui'\n\
             export function loader() { return db }\n\
             export default function Page() { return ui }\n",
        );
        assert_eq!(names, ["db"]);
    }

    #[test]
    fn an_import_that_a_client_export_also_reads_stays() {
        let names = split(
            "import { db } from './db'\n\
             export const loader = () => db\n\
             export function meta() { return [db] }\n",
        );
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_local_helper_of_the_action_is_dead_with_its_imports() {
        let names = split(
            "import { sign } from './sign'\nimport { fmt } from './fmt'\n\
             const helper = () => sign()\n\
             function shared() { return fmt() }\n\
             export async function action() { return helper() + shared() }\n\
             export default function Page() { return shared() }\n",
        );
        assert_eq!(names, ["sign"]);
    }

    #[test]
    fn an_unused_import_and_a_side_effect_import_stay() {
        let names = split(
            "import './polyfill'\nimport { unused } from './unused'\n\
             export function loader() { return 1 }\n",
        );
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_dynamic_import_in_the_loader_is_server_only() {
        let names = split(
            "export async function loader() { return import('./db.server') }\n\
             export default function Page() { return null }\n",
        );
        assert_eq!(names, ["import("]);
    }

    #[test]
    fn a_renamed_server_export_specifier_does_not_keep_its_binding() {
        let names = split(
            "import { load } from './load'\n\
             const loadData = () => load()\n\
             export { loadData as loader }\n\
             export default function Page() { return null }\n",
        );
        assert_eq!(names, ["load"]);
    }

    #[test]
    fn a_local_declaration_that_nothing_reads_keeps_its_imports() {
        let names = split(
            "import { db } from './db'\nconst probe = db\n\
             export function loader() { return 1 }\n",
        );
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_server_only_re_export_is_skipped() {
        let names = split(
            "export { action } from './note.server'\n\
             export default function Page() { return null }\n",
        );
        // The re-export edge span starts at offset 0, which reads `export`.
        assert_eq!(names, ["export"]);
    }

    #[test]
    fn a_named_import_at_the_file_start_does_not_block_the_re_export_skip() {
        let names = split(
            "import { a } from './a'\nexport { action } from './note.server'\n\
             export default function Page() { return a }\n",
        );
        assert_eq!(names, ["import"]);
    }

    #[test]
    fn a_side_effect_import_at_the_file_start_blocks_the_re_export_skip() {
        let names = split(
            "import './polyfill'\nexport { action } from './note.server'\n\
             export default function Page() { return null }\n",
        );
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_mix_of_client_and_server_only_re_exports_stays() {
        let names = split(
            "export { action } from './note.server'\n\
             export { default } from './page'\n",
        );
        assert!(names.is_empty(), "{names:?}");
    }

    #[test]
    fn a_re_export_with_a_glob_pattern_in_the_module_stays() {
        let names = split(
            "export { action } from './note.server'\n\
             const pages = import.meta.glob('./pages/*.tsx')\n\
             export default function Page() { return pages }\n",
        );
        assert!(names.is_empty(), "{names:?}");
    }

    fn dead_env(source: &str) -> Vec<String> {
        fallow_extract::parse_bounded_or_default(source, SourceType::tsx(), |parsed| {
            let mut reads: Vec<String> = split_program(&parsed.program)
                .dead_only_env_reads
                .into_iter()
                .collect();
            reads.sort();
            reads
        })
    }

    #[test]
    fn an_env_read_that_only_the_loader_makes_is_dead() {
        let reads = dead_env(
            "const secret = () => process.env.SESSION_SECRET\n\
             export function loader() { return [secret(), import.meta.env.API_KEY] }\n\
             export default function Page() { return process.env.PUBLIC_NAME }\n",
        );
        assert_eq!(
            reads,
            ["import.meta.env.API_KEY", "process.env.SESSION_SECRET"]
        );
    }

    #[test]
    fn an_env_read_in_client_code_stays() {
        let reads = dead_env(
            "export function loader() { return process.env.TOKEN }\n\
             export default function Page() { return process.env.TOKEN }\n",
        );
        assert!(reads.is_empty(), "{reads:?}");
    }

    #[test]
    fn a_module_without_server_exports_has_no_server_only_imports() {
        let names =
            split("import { db } from './db'\nexport default function Page() { return db }\n");
        assert!(names.is_empty(), "{names:?}");
    }
}
