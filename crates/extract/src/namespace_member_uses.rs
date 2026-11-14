//! Uses of TypeScript namespace members that a plain member access does not
//! record.
//!
//! The visitor records `N.a` as a member access of `N`. This pass uses the
//! scope resolution of the semantic model to record three more uses:
//!
//! - a nested chain: the value `N.Inner.x` and the type `N.Inner.T`;
//! - a value that leaves its chain: `use(N)`, `const M = N.Inner`, `[N.Inner]`.
//!   Every member below the chain then counts as used;
//! - a reference inside the namespace body: `export const b = a`.
//!
//! The analysis does not know at extraction time which binding is a
//! namespace, so the pass records these uses for every import binding.
//! Analysis reads them only for namespace members.

use fallow_types::extract::{
    ExportInfo, ImportInfo, ImportedName, MemberKind, NamespaceMemberUseFact,
};
use oxc_ast::AstKind;
use oxc_ast::ast::{
    BinaryOperator, BindingPattern, Expression, PropertyKey, TSImportTypeQualifier,
};
use oxc_semantic::{AstNodes, NodeId, ScopeId, Scoping, Semantic, SymbolId};
use oxc_span::{GetSpan, Span};

use crate::js_text::JsText;
use rustc_hash::FxHashSet;

/// How the code uses the value at the end of a member chain.
enum ChainUse {
    /// A read, a call, a type use, or a re-export: only the chain itself.
    Access,
    /// The value leaves the chain, so every member below it can be read.
    Escape,
    /// `const { a, b } = chain`: the listed keys below the chain.
    Destructure(Vec<String>),
}

/// The outermost static member chain over a root identifier.
pub struct Chain {
    pub node: NodeId,
    pub span: Span,
    pub segments: Vec<String>,
}

/// Collect the namespace member uses of a module, sorted and without repeats.
pub fn collect(
    semantic: &Semantic<'_>,
    imports: &[ImportInfo],
    exports: &[ExportInfo],
) -> Vec<NamespaceMemberUseFact> {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let root_scope = scoping.root_scope_id();
    let mut uses: FxHashSet<(String, String, bool)> = FxHashSet::default();
    let mut seen_roots: FxHashSet<SymbolId> = FxHashSet::default();

    for import in imports {
        if import.local_name.is_empty() {
            continue;
        }
        let Some(symbol) =
            scoping.get_binding(root_scope, oxc_str::Ident::from(import.local_name.as_str()))
        else {
            continue;
        };
        if seen_roots.insert(symbol) {
            // `import * as X; export { X }` hands the whole module on. The
            // member pass does not follow that re-export, so it is an escape.
            let re_export_escapes = matches!(import.imported_name, ImportedName::Namespace);
            let root = Root {
                symbol,
                object: &import.local_name,
                re_export_escapes,
            };
            record_root_references(scoping, nodes, &root, &mut uses);
        }
    }

    if imports
        .iter()
        .any(|import| import.local_name.is_empty() && import.is_type_only)
    {
        record_import_type_uses(nodes, &mut uses);
    }

    for export in exports {
        if !export
            .members
            .iter()
            .any(|member| member.kind == MemberKind::NamespaceMember)
        {
            continue;
        }
        let Some(local) = export.local_name.as_deref() else {
            continue;
        };
        let Some(symbol) = scoping.get_binding(root_scope, oxc_str::Ident::from(local)) else {
            continue;
        };
        if seen_roots.insert(symbol) {
            let root = Root {
                symbol,
                object: local,
                re_export_escapes: false,
            };
            record_root_references(scoping, nodes, &root, &mut uses);
        }
        record_body_references(semantic, symbol, local, export, &mut uses);
    }

    let mut facts: Vec<NamespaceMemberUseFact> = uses
        .into_iter()
        .map(|(object, path, whole)| NamespaceMemberUseFact {
            object,
            path,
            whole,
        })
        .collect();
    facts.sort_unstable_by(|a, b| {
        (a.object.as_str(), a.path.as_str(), a.whole).cmp(&(
            b.object.as_str(),
            b.path.as_str(),
            b.whole,
        ))
    });
    facts
}

/// A module-level binding whose references the pass records.
struct Root<'a> {
    symbol: SymbolId,
    object: &'a str,
    /// A local re-export (`export { X }`) of the binding is an escape.
    re_export_escapes: bool,
}

/// Record the uses of a module-level root binding. A one-step chain `N.a` is
/// already a member access of the visitor, so only a longer chain and an
/// escape are recorded here.
fn record_root_references(
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    root: &Root<'_>,
    uses: &mut FxHashSet<(String, String, bool)>,
) {
    let object = root.object;
    for reference in scoping.get_resolved_references(root.symbol) {
        let node_id = reference.node_id();
        let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
            continue;
        };
        let chain = member_chain(nodes, node_id, identifier.span);
        let path = chain.segments.join(".");
        let chain_use = if root.re_export_escapes && is_export_specifier(nodes, &chain) {
            ChainUse::Escape
        } else {
            classify(nodes, &chain)
        };
        match chain_use {
            ChainUse::Access => {
                if chain.segments.len() > 1 {
                    uses.insert((object.to_string(), path, false));
                }
            }
            ChainUse::Escape => {
                uses.insert((object.to_string(), path, true));
            }
            ChainUse::Destructure(keys) => {
                for key in keys {
                    uses.insert((object.to_string(), join_path(&path, &key), false));
                }
            }
        }
    }
}

/// Record the qualifier of each inline import type as a use under the key of
/// its source: `import("./x").N.T` reads `N.T`. The import has no local
/// binding, so the reference walk above cannot see it. A `typeof` hands the
/// value on, as `typeof N.a` does for a binding.
fn record_import_type_uses(nodes: &AstNodes<'_>, uses: &mut FxHashSet<(String, String, bool)>) {
    for node in nodes.iter() {
        let AstKind::TSImportType(import_type) = node.kind() else {
            continue;
        };
        let Some(qualifier) = &import_type.qualifier else {
            continue;
        };
        let mut segments = Vec::new();
        qualifier_segments(qualifier, &mut segments);
        let whole = matches!(nodes.parent_kind(node.id()), AstKind::TSTypeQuery(_));
        if segments.len() < 2 && !whole {
            continue;
        }
        uses.insert((
            NamespaceMemberUseFact::import_type_object(&import_type.source.value.text()),
            segments.join("."),
            whole,
        ));
    }
}

fn qualifier_segments(qualifier: &TSImportTypeQualifier<'_>, segments: &mut Vec<String>) {
    match qualifier {
        TSImportTypeQualifier::Identifier(identifier) => {
            segments.push(identifier.name.to_string());
        }
        TSImportTypeQualifier::QualifiedName(name) => {
            qualifier_segments(&name.left, segments);
            segments.push(name.right.name.to_string());
        }
    }
}

/// Record the references to each namespace member inside the namespace body,
/// as uses of the member path under the root `object`.
fn record_body_references(
    semantic: &Semantic<'_>,
    root_symbol: SymbolId,
    object: &str,
    export: &ExportInfo,
    uses: &mut FxHashSet<(String, String, bool)>,
) {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let unresolved = scoping.root_unresolved_references();
    for member in &export.members {
        if member.kind != MemberKind::NamespaceMember {
            continue;
        }
        let leaf = member.name.rsplit('.').next().unwrap_or(&member.name);
        // A member that another block of a merged namespace reads without a
        // qualifier stays unresolved in the semantic model.
        if unresolved.contains_key(leaf) {
            uses.insert((object.to_string(), member.name.clone(), false));
        }
        let Some(symbol) = member_symbol(semantic, root_symbol, &member.name) else {
            continue;
        };
        for reference in scoping.get_resolved_references(symbol) {
            let node_id = reference.node_id();
            let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
                continue;
            };
            let chain = member_chain(nodes, node_id, identifier.span);
            let path = join_path(&member.name, &chain.segments.join("."));
            uses.insert((object.to_string(), path.clone(), false));
            match classify(nodes, &chain) {
                ChainUse::Access => {}
                ChainUse::Escape => {
                    uses.insert((object.to_string(), path, true));
                }
                ChainUse::Destructure(keys) => {
                    for key in keys {
                        uses.insert((object.to_string(), join_path(&path, &key), false));
                    }
                }
            }
        }
    }
}

/// The symbol that the dotted member path declares inside the namespace
/// `root`.
fn member_symbol(semantic: &Semantic<'_>, root: SymbolId, path: &str) -> Option<SymbolId> {
    let scoping = semantic.scoping();
    let mut owner = root;
    for segment in path.split('.') {
        owner = namespace_scopes(semantic, owner)
            .into_iter()
            .find_map(|scope| scoping.get_binding(scope, oxc_str::Ident::from(segment)))?;
    }
    Some(owner)
}

/// The body scopes of every namespace declaration of `symbol`.
fn namespace_scopes(semantic: &Semantic<'_>, symbol: SymbolId) -> Vec<ScopeId> {
    semantic
        .scoping()
        .symbol_declarations(symbol)
        .filter_map(|node_id| match semantic.nodes().kind(node_id) {
            AstKind::TSNamespaceDeclaration(declaration) => declaration.scope_id.get(),
            _ => None,
        })
        .collect()
}

fn join_path(prefix: &str, suffix: &str) -> String {
    match (prefix.is_empty(), suffix.is_empty()) {
        (true, _) => suffix.to_string(),
        (_, true) => prefix.to_string(),
        _ => format!("{prefix}.{suffix}"),
    }
}

/// Follow static member links up from a root identifier: `N.a.b`, the type
/// `N.a.T`, `N["a"]` and the JSX tag `<N.a />`.
pub fn member_chain(nodes: &AstNodes<'_>, node_id: NodeId, identifier_span: Span) -> Chain {
    let mut current = node_id;
    let mut span = identifier_span;
    let mut segments = Vec::new();
    loop {
        let parent = nodes.parent_id(current);
        if parent == current {
            break;
        }
        let (segment, parent_span) = match nodes.kind(parent) {
            AstKind::StaticMemberExpression(member) if member.object.span() == span => {
                (member.property.name.to_string(), member.span)
            }
            AstKind::ComputedMemberExpression(member) if member.object.span() == span => {
                match &member.expression {
                    Expression::StringLiteral(literal) => {
                        (literal.value.text().into_owned(), member.span)
                    }
                    _ => break,
                }
            }
            AstKind::TSQualifiedName(name) if name.left.span() == span => {
                (name.right.name.to_string(), name.span)
            }
            AstKind::JSXMemberExpression(member) if member.object.span() == span => {
                (member.property.name.to_string(), member.span)
            }
            _ => break,
        };
        segments.push(segment);
        current = parent;
        span = parent_span;
    }
    Chain {
        node: current,
        span,
        segments,
    }
}

/// Walk up through parentheses and the TypeScript expression wrappers that
/// do not change the runtime value.
pub fn unwrap_upward(nodes: &AstNodes<'_>, node_id: NodeId, span: Span) -> (NodeId, Span) {
    let mut current = node_id;
    let mut current_span = span;
    loop {
        let parent = nodes.parent_id(current);
        if parent == current {
            break;
        }
        let parent_span = match nodes.kind(parent) {
            AstKind::ParenthesizedExpression(node) => node.span,
            AstKind::TSAsExpression(node) if node.expression.span() == current_span => node.span,
            AstKind::TSSatisfiesExpression(node) if node.expression.span() == current_span => {
                node.span
            }
            AstKind::TSNonNullExpression(node) => node.span,
            AstKind::TSInstantiationExpression(node) if node.expression.span() == current_span => {
                node.span
            }
            _ => break,
        };
        current = parent;
        current_span = parent_span;
    }
    (current, current_span)
}

fn is_export_specifier(nodes: &AstNodes<'_>, chain: &Chain) -> bool {
    matches!(nodes.parent_kind(chain.node), AstKind::ExportSpecifier(_))
}

/// How the code uses the value at the end of `chain`. An unknown context is
/// an escape, so an unclear use never leaves a live member reported.
fn classify(nodes: &AstNodes<'_>, chain: &Chain) -> ChainUse {
    let (outer, outer_span) = unwrap_upward(nodes, chain.node, chain.span);
    let parent = nodes.parent_id(outer);
    if parent == outer {
        return ChainUse::Access;
    }
    match nodes.kind(parent) {
        AstKind::CallExpression(call) if call.callee.span() == outer_span => ChainUse::Access,
        AstKind::NewExpression(new) if new.callee.span() == outer_span => ChainUse::Access,
        AstKind::TaggedTemplateExpression(tagged) if tagged.tag.span() == outer_span => {
            ChainUse::Access
        }
        AstKind::StaticMemberExpression(member) if member.object.span() == outer_span => {
            ChainUse::Access
        }
        AstKind::TSTypeReference(_)
        | AstKind::TSClassImplements(_)
        | AstKind::TSInterfaceHeritage(_)
        | AstKind::Class(_)
        | AstKind::ExportSpecifier(_)
        | AstKind::ExpressionStatement(_)
        | AstKind::UnaryExpression(_)
        | AstKind::UpdateExpression(_)
        | AstKind::JSXOpeningElement(_)
        | AstKind::JSXClosingElement(_) => ChainUse::Access,
        AstKind::TSImportEqualsDeclaration(_) if !chain.segments.is_empty() => ChainUse::Access,
        AstKind::BinaryExpression(binary)
            if !(binary.operator == BinaryOperator::In && binary.right.span() == outer_span) =>
        {
            ChainUse::Access
        }
        AstKind::AssignmentExpression(assignment) if assignment.right.span() != outer_span => {
            ChainUse::Access
        }
        AstKind::VariableDeclarator(declarator)
            if declarator.init.as_ref().map(GetSpan::span) == Some(outer_span) =>
        {
            destructured_keys(&declarator.id).map_or(ChainUse::Escape, ChainUse::Destructure)
        }
        _ => ChainUse::Escape,
    }
}

/// The keys of `const { a, b: c } = ...` when every key is static and the
/// pattern has no rest element.
pub fn destructured_keys(pattern: &BindingPattern<'_>) -> Option<Vec<String>> {
    let BindingPattern::ObjectPattern(object) = pattern else {
        return None;
    };
    if object.rest.is_some() {
        return None;
    }
    object
        .properties
        .iter()
        .map(|property| match &property.key {
            PropertyKey::StaticIdentifier(key) if !property.computed => Some(key.name.to_string()),
            PropertyKey::StringLiteral(key) => Some(key.value.text().into_owned()),
            _ => None,
        })
        .collect()
}

#[cfg(all(test, not(miri)))]
mod tests {
    use fallow_types::extract::SemanticFact;

    use crate::tests::parse_ts;

    /// The namespace member uses of `source` as `object|path|whole`.
    fn uses(source: &str) -> Vec<String> {
        parse_ts(source)
            .semantic_facts
            .iter()
            .filter_map(|fact| match fact {
                SemanticFact::NamespaceMemberUse(namespace_use) => Some(format!(
                    "{}|{}|{}",
                    namespace_use.object, namespace_use.path, namespace_use.whole
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn inline_import_type_records_its_qualifier() {
        assert_eq!(
            uses(
                "export type A = typeof import('./x').N.a;\n\
                 export type B = import('./x').N.M.T;\n\
                 export type C = typeof import('./x').N;\n\
                 export type D = import('./x').T;\n"
            ),
            [
                "import(./x)|N|true",
                "import(./x)|N.M.T|false",
                "import(./x)|N.a|true"
            ]
        );
    }

    #[test]
    fn re_export_of_a_namespace_import_is_an_escape() {
        assert_eq!(
            uses("import * as X from './ns';\nexport { X };\n"),
            ["X||true"]
        );
    }
}
