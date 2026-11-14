//! Uses of TypeScript namespace members that a plain member access does not
//! record.
//!
//! The visitor records `N.a` as a member access of `N`. This pass uses the
//! scope resolution of the semantic model to record three more uses:
//!
//! - a nested chain: the value `N.Inner.x` and the type `N.Inner.T`;
//! - a value that leaves its chain: `use(N)`, `[N.Inner]`, `{ ref: N }`.
//!   Every member below the chain then counts as used;
//! - a reference inside the namespace body: `export const b = a`.
//!
//! The pass follows a `const` alias (`const M = N.Inner; M.x`) and records
//! its reads as reads of the root.
//!
//! The analysis does not know at extraction time which binding is a
//! namespace or an enum, so the pass records these uses for every import
//! binding. Analysis reads them for namespace members, and reads the
//! escape of a whole enum (`use(E)`) for enum members.

use fallow_types::extract::{
    ExportInfo, ImportInfo, ImportedName, MemberKind, NamespaceMemberUseFact,
};
use oxc_ast::AstKind;
use oxc_ast::ast::{
    BinaryOperator, BindingPattern, Expression, PropertyKey, TSImportTypeQualifier,
    VariableDeclarationKind,
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
    let mut uses = Uses::default();
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
                prefix: String::new(),
                re_export_escapes,
                is_alias: false,
            };
            record_root_references(scoping, nodes, &root, &mut uses, &mut seen_roots);
        }
    }

    if imports
        .iter()
        .any(|import| import.local_name.is_empty() && import.is_type_only)
    {
        record_import_type_uses(nodes, &mut uses.chains);
    }

    for export in exports {
        let is_namespace = export
            .members
            .iter()
            .any(|member| member.kind == MemberKind::NamespaceMember);
        // An enum that the module declares can escape in the same module
        // (`takesEnum(Shape)`), as an imported enum can.
        let is_enum = export
            .members
            .iter()
            .any(|member| member.kind == MemberKind::EnumMember);
        if !is_namespace && !is_enum {
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
                prefix: String::new(),
                re_export_escapes: false,
                is_alias: false,
            };
            record_root_references(scoping, nodes, &root, &mut uses, &mut seen_roots);
        }
        if is_namespace {
            record_body_references(semantic, symbol, local, export, &mut uses.chains);
        }
    }

    let class_field_facts =
        uses.class_fields
            .into_iter()
            .map(|(object, path)| NamespaceMemberUseFact {
                object,
                path,
                whole: true,
                class_field: true,
            });
    let mut facts: Vec<NamespaceMemberUseFact> = uses
        .chains
        .into_iter()
        .map(|(object, path, whole)| NamespaceMemberUseFact {
            object,
            path,
            whole,
            class_field: false,
        })
        .chain(class_field_facts)
        .collect();
    facts.sort_unstable_by(|a, b| {
        (a.object.as_str(), a.path.as_str(), a.whole, a.class_field).cmp(&(
            b.object.as_str(),
            b.path.as_str(),
            b.whole,
            b.class_field,
        ))
    });
    facts
}

/// The uses that the pass collects before it makes them facts.
#[derive(Default)]
struct Uses {
    /// `(object, path, whole)` of each chain use.
    chains: FxHashSet<(String, String, bool)>,
    /// `(object, path)` of each value that goes into a class field.
    class_fields: FxHashSet<(String, String)>,
}

/// A binding whose references the pass records: a module-level root, or a
/// `const` alias of a root.
struct Root<'a> {
    symbol: SymbolId,
    object: &'a str,
    /// The member path that the binding holds under `object`: empty for the
    /// root itself and for `const A = N`, `Inner` for `const I = N.Inner`.
    prefix: String,
    /// A local re-export (`export { X }`) of the binding is an escape.
    re_export_escapes: bool,
    /// The binding is a `const` alias. The visitor does not always record a
    /// one-step read of an alias, so the pass records it.
    is_alias: bool,
}

/// Record the uses of a binding. A one-step chain `N.a` on a root is already
/// a member access of the visitor, so only a longer chain and an escape are
/// recorded for a root.
///
/// The pass follows a `const` alias (`const A = N; A.a`) and records the
/// reads of the alias as reads of the root, so the alias itself is not an
/// escape. An exported alias is an escape: other modules read it.
fn record_root_references(
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    root: &Root<'_>,
    uses: &mut Uses,
    seen: &mut FxHashSet<SymbolId>,
) {
    let object = root.object;
    for reference in scoping.get_resolved_references(root.symbol) {
        let node_id = reference.node_id();
        let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
            continue;
        };
        let chain = member_chain(nodes, node_id, identifier.span);
        let path = join_path(&root.prefix, &chain.segments.join("."));
        if let Some(alias) = const_alias_symbol(nodes, &chain) {
            if seen.insert(alias) {
                let alias_root = Root {
                    symbol: alias,
                    object,
                    prefix: path,
                    re_export_escapes: true,
                    is_alias: true,
                };
                record_root_references(scoping, nodes, &alias_root, uses, seen);
            }
            continue;
        }
        let chain_use = if root.re_export_escapes && is_export_specifier(nodes, &chain) {
            ChainUse::Escape
        } else {
            classify(nodes, &chain)
        };
        match chain_use {
            ChainUse::Access => {
                if chain.segments.len() > 1 || (root.is_alias && !path.is_empty()) {
                    uses.chains.insert((object.to_string(), path, false));
                }
            }
            ChainUse::Escape if is_class_field_value(nodes, &chain) => {
                uses.class_fields.insert((object.to_string(), path));
            }
            ChainUse::Escape => {
                uses.chains.insert((object.to_string(), path, true));
            }
            ChainUse::Destructure(keys) => {
                for key in keys {
                    uses.chains
                        .insert((object.to_string(), join_path(&path, &key), false));
                }
            }
        }
    }
}

/// The symbol of the `const` alias that `chain` initializes:
/// `const A = N` or `const I = N.Inner`. An exported declaration is not an
/// alias that the pass can follow, because other modules read it.
fn const_alias_symbol(nodes: &AstNodes<'_>, chain: &Chain) -> Option<SymbolId> {
    let (outer, outer_span) = unwrap_upward(nodes, chain.node, chain.span);
    let declarator_id = nodes.parent_id(outer);
    let AstKind::VariableDeclarator(declarator) = nodes.kind(declarator_id) else {
        return None;
    };
    if declarator.init.as_ref().map(GetSpan::span) != Some(outer_span) {
        return None;
    }
    let BindingPattern::BindingIdentifier(id) = &declarator.id else {
        return None;
    };
    let declaration_id = nodes.parent_id(declarator_id);
    let AstKind::VariableDeclaration(declaration) = nodes.kind(declaration_id) else {
        return None;
    };
    if declaration.kind != VariableDeclarationKind::Const
        || matches!(
            nodes.parent_kind(declaration_id),
            AstKind::ExportNamedDeclaration(_) | AstKind::ExportDeclaration(_)
        )
    {
        return None;
    }
    id.symbol_id.get()
}

/// Whether the value at the end of `chain` initializes an instance field
/// with a static name (`levels = Level`). The visitor follows the reads of
/// such a field through `this.levels` and through the component template.
fn is_class_field_value(nodes: &AstNodes<'_>, chain: &Chain) -> bool {
    let (outer, outer_span) = unwrap_upward(nodes, chain.node, chain.span);
    matches!(
        nodes.parent_kind(outer),
        AstKind::PropertyDefinition(property)
            if !property.r#static
                && !property.computed
                && property.value.as_ref().map(GetSpan::span) == Some(outer_span)
    )
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
                    "{}|{}|{}{}",
                    namespace_use.object,
                    namespace_use.path,
                    namespace_use.whole,
                    if namespace_use.class_field {
                        "|field"
                    } else {
                        ""
                    }
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

    #[test]
    fn whole_binding_escapes_are_recorded() {
        assert_eq!(
            uses(
                "import { Shape, Size } from './e';\n\
                 declare function takesEnum(value: object): void;\n\
                 takesEnum(Shape);\n\
                 console.log({ ref: Size });\n"
            ),
            ["Shape||true", "Size||true"]
        );
    }

    #[test]
    fn const_alias_reads_are_reads_of_the_root() {
        assert_eq!(
            uses(
                "import { Color, Hue } from './e';\n\
                 const Alias = Color;\n\
                 if (Alias.Red === 'red') {}\n\
                 function f() { const local = Hue; return local.Dark; }\n"
            ),
            ["Color|Red|false", "Hue|Dark|true"]
        );
    }

    #[test]
    fn escape_of_a_const_alias_is_an_escape_of_the_root() {
        assert_eq!(
            uses(
                "import { Color } from './e';\n\
                 const Alias = Color;\n\
                 console.log([Alias]);\n"
            ),
            ["Color||true"]
        );
    }

    #[test]
    fn exported_alias_is_an_escape() {
        for source in [
            "import { Color } from './e';\nexport const Alias = Color;\n",
            "import { Color } from './e';\nconst Alias = Color;\nexport { Alias };\n",
        ] {
            assert_eq!(uses(source), ["Color||true"], "{source}");
        }
    }

    #[test]
    fn reassignable_alias_is_an_escape() {
        assert_eq!(
            uses("import { Color } from './e';\nlet Alias = Color;\nAlias.Red;\n"),
            ["Color||true"]
        );
    }

    #[test]
    fn class_field_alias_is_marked() {
        assert_eq!(
            uses(
                "import { Level, Kind } from './e';\n\
                 class C { levels = Level; static kinds = Kind; }\n"
            ),
            ["Kind||true", "Level||true|field"]
        );
    }

    #[test]
    fn local_enum_escape_is_recorded() {
        assert_eq!(
            uses(
                "export enum Dir { Up = 'up', Down = 'down' }\n\
                 export const registered: object[] = [];\n\
                 registered.push(Dir);\n"
            ),
            ["Dir||true"]
        );
    }
}
