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
//! A value under a static key of an object holder (`{ websocket: WebSocket }`)
//! can be read as `holder[name].prototype.member`. The pass records such a
//! read of a module-level `const` holder as a read of the value. For an
//! exported holder, it records the value as a [`HolderValueFact`], and it
//! records a read of an imported holder under its key. The member pass joins
//! the two across the module graph.
//!
//! The analysis does not know at extraction time which binding is a
//! namespace or an enum, so the pass records these uses for every import
//! binding. Analysis reads them for namespace members, reads the escape of
//! a whole enum (`use(E)`) for enum members, and reads the escape of a
//! class value (`use(C)`) and `C.prototype.member` for class members.

use fallow_types::extract::{
    ExportInfo, HolderValueFact, ImportInfo, ImportedName, MemberKind, NamespaceMemberUseFact,
    SemanticFact,
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

/// Collect the namespace member uses and the holder values of a module,
/// sorted and without repeats.
pub fn collect(
    semantic: &Semantic<'_>,
    imports: &[ImportInfo],
    exports: &[ExportInfo],
) -> Vec<SemanticFact> {
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
        // An enum or a class that the module declares can escape in the
        // same module (`takesEnum(Shape)`, `register(Adapter)`), as an
        // imported one can. A class can also have a member read through
        // `Class.prototype.member`.
        let has_value_members = export.members.iter().any(|member| {
            matches!(
                member.kind,
                MemberKind::EnumMember | MemberKind::ClassMethod | MemberKind::ClassProperty
            )
        });
        if !is_namespace && !has_value_members {
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

    let facts = namespace_use_facts(uses.chains, uses.class_fields);
    let holder_values = holder_value_facts(uses.holder_values, exports);
    facts
        .into_iter()
        .map(SemanticFact::NamespaceMemberUse)
        .chain(
            holder_values
                .into_iter()
                .map(|holder| SemanticFact::HolderValue(Box::new(holder))),
        )
        .collect()
}

/// The namespace member use facts of the collected chain and class field
/// uses, sorted.
fn namespace_use_facts(
    chains: FxHashSet<(String, String, bool)>,
    class_fields: FxHashSet<(String, String)>,
) -> Vec<NamespaceMemberUseFact> {
    let class_field_facts = class_fields
        .into_iter()
        .map(|(object, path)| NamespaceMemberUseFact {
            object,
            path,
            whole: true,
            class_field: true,
        });
    let mut facts: Vec<NamespaceMemberUseFact> = chains
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

/// The holder value facts of the collected holder values, one for each
/// export of the holder, sorted and without repeats.
fn holder_value_facts(
    values: FxHashSet<(Option<String>, String, String, String)>,
    exports: &[ExportInfo],
) -> Vec<HolderValueFact> {
    let mut holder_values: Vec<HolderValueFact> = values
        .into_iter()
        .flat_map(|(holder, key, object, path)| {
            holder_export_names(exports, holder.as_deref())
                .into_iter()
                .map(move |export_name| HolderValueFact {
                    export_name,
                    key: key.clone(),
                    object: object.clone(),
                    path: path.clone(),
                })
        })
        .collect();
    holder_values.sort_unstable_by(|a, b| {
        (&a.export_name, &a.key, &a.object, &a.path).cmp(&(
            &b.export_name,
            &b.key,
            &b.object,
            &b.path,
        ))
    });
    holder_values.dedup();
    holder_values
}

/// The export names of a holder: `default` for `export default { ... }`
/// (`None`), and each export of the `const` binding otherwise.
fn holder_export_names(exports: &[ExportInfo], holder: Option<&str>) -> Vec<String> {
    let Some(local) = holder else {
        return vec![DEFAULT_EXPORT.to_string()];
    };
    exports
        .iter()
        .filter(|export| export.local_name.as_deref() == Some(local))
        .map(|export| export.name.to_string())
        .collect()
}

const DEFAULT_EXPORT: &str = "default";

/// The uses that the pass collects before it makes them facts.
#[derive(Default)]
struct Uses {
    /// `(object, path, whole)` of each chain use.
    chains: FxHashSet<(String, String, bool)>,
    /// `(object, path)` of each value that goes into a class field.
    class_fields: FxHashSet<(String, String)>,
    /// `(holder, key, object, path)` of each value under a static key of an
    /// object holder that can be exported. `holder` is the local name of the
    /// `const`, or `None` for `export default { ... }`.
    holder_values: FxHashSet<(Option<String>, String, String, String)>,
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
        record_any_key_prototype_read(nodes, &chain, object, &path, uses);
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
                if let Some(holder) = object_holder(nodes, &chain) {
                    record_holder_value(scoping, nodes, holder, object, &path, uses);
                }
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

/// The first segment of a chain that reads an instance member through a
/// class value: `Class.prototype.member`.
const PROTOTYPE_SEGMENT: &str = "prototype";

/// An object literal that holds a value under a static key: a module-level
/// `const holder = { key: Value }`, which can be exported, or
/// `export default { key: Value }`.
struct Holder {
    /// The `const` binding, `None` for `export default { ... }`.
    symbol: Option<SymbolId>,
    /// The local name of the `const` binding, `None` for
    /// `export default { ... }`.
    name: Option<String>,
    key: String,
    /// The holder is a module-level declaration, so an export can name it.
    can_be_exported: bool,
}

/// The holder whose property value is the value at the end of `chain`.
fn object_holder(nodes: &AstNodes<'_>, chain: &Chain) -> Option<Holder> {
    let (outer, outer_span) = unwrap_upward(nodes, chain.node, chain.span);
    let property_id = nodes.parent_id(outer);
    let AstKind::ObjectProperty(property) = nodes.kind(property_id) else {
        return None;
    };
    if property.computed || property.value.span() != outer_span {
        return None;
    }
    let key = property.key.static_name()?.as_str()?.to_string();
    let object_id = nodes.parent_id(property_id);
    let AstKind::ObjectExpression(object) = nodes.kind(object_id) else {
        return None;
    };
    let object_chain = Chain {
        node: object_id,
        span: object.span,
        segments: Vec::new(),
    };
    if is_export_default_value(nodes, &object_chain) {
        return Some(Holder {
            symbol: None,
            name: None,
            key,
            can_be_exported: true,
        });
    }
    let declarator = const_declarator(nodes, &object_chain)?;
    Some(Holder {
        symbol: Some(declarator.symbol),
        name: Some(declarator.name),
        key,
        can_be_exported: declarator.is_module_level,
    })
}

/// Whether the value at the end of `chain` is the expression of
/// `export default`.
fn is_export_default_value(nodes: &AstNodes<'_>, chain: &Chain) -> bool {
    let (outer, outer_span) = unwrap_upward(nodes, chain.node, chain.span);
    matches!(
        nodes.parent_kind(outer),
        AstKind::ExportDefaultDeclaration(declaration)
            if declaration.declaration.span() == outer_span
    )
}

/// Record `chain[name].prototype.member` with a computed key that is not a
/// string literal as a read of `[].prototype.member` under `path`. The
/// value at the end of `chain` can be an imported holder. The member pass
/// credits the read on each value of that holder.
fn record_any_key_prototype_read(
    nodes: &AstNodes<'_>,
    chain: &Chain,
    object: &str,
    path: &str,
    uses: &mut Uses,
) {
    let parent = nodes.parent_id(chain.node);
    let AstKind::ComputedMemberExpression(member) = nodes.kind(parent) else {
        return;
    };
    if member.object.span() != chain.span
        || matches!(member.expression, Expression::StringLiteral(_))
    {
        return;
    }
    let value_chain = member_chain(nodes, parent, member.span);
    if let [first, name, ..] = value_chain.segments.as_slice()
        && first == PROTOTYPE_SEGMENT
    {
        let read = format!(
            "{}.{PROTOTYPE_SEGMENT}.{name}",
            NamespaceMemberUseFact::ANY_KEY
        );
        uses.chains
            .insert((object.to_string(), join_path(path, &read), false));
    }
}

/// Record the reads of a value that a holder holds: through the holder in
/// this module, and, for a holder that an export can name, through the
/// holder in another module.
fn record_holder_value(
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    holder: Holder,
    object: &str,
    path: &str,
    uses: &mut Uses,
) {
    record_holder_prototype_reads(scoping, nodes, &holder, object, path, uses);
    if holder.can_be_exported {
        uses.holder_values.insert((
            holder.name,
            holder.key,
            object.to_string(),
            path.to_string(),
        ));
    }
}

/// Record `holder[name].prototype.member` and `holder.key.prototype.member`
/// as reads of `prototype.member` on the value under `path`. A computed key
/// that is not a string literal can select any value of the holder.
fn record_holder_prototype_reads(
    scoping: &Scoping,
    nodes: &AstNodes<'_>,
    holder: &Holder,
    object: &str,
    path: &str,
    uses: &mut Uses,
) {
    let Some(symbol) = holder.symbol else {
        return;
    };
    for reference in scoping.get_resolved_references(symbol) {
        let node_id = reference.node_id();
        let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
            continue;
        };
        let Some((member_id, member_span)) =
            holder_value_access(nodes, node_id, identifier.span, &holder.key)
        else {
            continue;
        };
        let chain = member_chain(nodes, member_id, member_span);
        if let [first, member, ..] = chain.segments.as_slice()
            && first == PROTOTYPE_SEGMENT
        {
            let read = format!("{PROTOTYPE_SEGMENT}.{member}");
            uses.chains
                .insert((object.to_string(), join_path(path, &read), false));
        }
    }
}

/// The member expression over the holder reference at `node_id` that can
/// give the value under `key`.
fn holder_value_access(
    nodes: &AstNodes<'_>,
    node_id: NodeId,
    span: Span,
    key: &str,
) -> Option<(NodeId, Span)> {
    let parent = nodes.parent_id(node_id);
    let selects_key = match nodes.kind(parent) {
        AstKind::StaticMemberExpression(member) if member.object.span() == span => {
            member.property.name == key
        }
        AstKind::ComputedMemberExpression(member) if member.object.span() == span => {
            match &member.expression {
                Expression::StringLiteral(literal) => literal.value == key,
                _ => true,
            }
        }
        _ => false,
    };
    selects_key.then(|| (parent, nodes.kind(parent).span()))
}

/// The symbol of the `const` alias that `chain` initializes:
/// `const A = N` or `const I = N.Inner`. An exported declaration is not an
/// alias that the pass can follow, because other modules read it.
fn const_alias_symbol(nodes: &AstNodes<'_>, chain: &Chain) -> Option<SymbolId> {
    let declarator = const_declarator(nodes, chain)?;
    (!declarator.is_exported).then_some(declarator.symbol)
}

/// A `const` binding that the value at the end of a chain initializes.
struct ConstDeclarator {
    symbol: SymbolId,
    name: String,
    /// The declaration is `export const`.
    is_exported: bool,
    /// The declaration is at the top of the module, directly or behind
    /// `export`.
    is_module_level: bool,
}

/// The `const` binding that `chain` initializes: `const A = chain` or
/// `export const A = chain`.
fn const_declarator(nodes: &AstNodes<'_>, chain: &Chain) -> Option<ConstDeclarator> {
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
    if declaration.kind != VariableDeclarationKind::Const {
        return None;
    }
    let is_exported = matches!(
        nodes.parent_kind(declaration_id),
        AstKind::ExportNamedDeclaration(_) | AstKind::ExportDeclaration(_)
    );
    let is_module_level =
        is_exported || matches!(nodes.parent_kind(declaration_id), AstKind::Program(_));
    Some(ConstDeclarator {
        symbol: id.symbol_id.get()?,
        name: id.name.to_string(),
        is_exported,
        is_module_level,
    })
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
    if parent == outer
        || is_callee_use(nodes, parent, outer_span)
        || is_read_use(nodes.kind(parent), chain, outer_span)
    {
        return ChainUse::Access;
    }
    match nodes.kind(parent) {
        AstKind::VariableDeclarator(declarator)
            if declarator.init.as_ref().map(GetSpan::span) == Some(outer_span) =>
        {
            destructured_keys(&declarator.id).map_or(ChainUse::Escape, ChainUse::Destructure)
        }
        _ => ChainUse::Escape,
    }
}

/// Whether the parent calls, constructs, tags or reads a member of the
/// value at `outer_span`.
fn is_callee_use(nodes: &AstNodes<'_>, parent: NodeId, outer_span: Span) -> bool {
    match nodes.kind(parent) {
        AstKind::CallExpression(call) => {
            call.callee.span() == outer_span
                // `class Sub extends mixin(Base) {}`: the class factory builds
                // the superclass from `Base`. It does not give the value to
                // other code.
                || is_superclass_call(nodes, parent, call.span)
        }
        AstKind::NewExpression(new) => new.callee.span() == outer_span,
        AstKind::TaggedTemplateExpression(tagged) => tagged.tag.span() == outer_span,
        AstKind::StaticMemberExpression(member) => member.object.span() == outer_span,
        _ => false,
    }
}

/// Whether the parent only reads the value at `outer_span`: a type use, a
/// statement, an operator, or the target of an assignment.
fn is_read_use(parent: AstKind<'_>, chain: &Chain, outer_span: Span) -> bool {
    match parent {
        AstKind::TSTypeReference(_)
        | AstKind::TSClassImplements(_)
        | AstKind::TSInterfaceHeritage(_)
        | AstKind::Class(_)
        | AstKind::ExportSpecifier(_)
        | AstKind::ExpressionStatement(_)
        | AstKind::UnaryExpression(_)
        | AstKind::UpdateExpression(_)
        | AstKind::JSXOpeningElement(_)
        | AstKind::JSXClosingElement(_) => true,
        AstKind::TSImportEqualsDeclaration(_) => !chain.segments.is_empty(),
        AstKind::BinaryExpression(binary) => {
            !(binary.operator == BinaryOperator::In && binary.right.span() == outer_span)
        }
        AstKind::AssignmentExpression(assignment) => assignment.right.span() != outer_span,
        _ => false,
    }
}

/// Whether the call at `call_id` is the `extends` expression of a class.
fn is_superclass_call(nodes: &AstNodes<'_>, call_id: NodeId, call_span: Span) -> bool {
    matches!(
        nodes.parent_kind(call_id),
        AstKind::Class(class)
            if class.heritage.as_ref().map(|heritage| heritage.expression.span()) == Some(call_span)
    )
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

    /// The holder values of `source` as `export|key|object|path`.
    fn holder_values(source: &str) -> Vec<String> {
        parse_ts(source)
            .semantic_facts
            .iter()
            .filter_map(|fact| match fact {
                SemanticFact::HolderValue(holder) => Some(format!(
                    "{}|{}|{}|{}",
                    holder.export_name, holder.key, holder.object, holder.path
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn exported_holders_record_their_values() {
        assert_eq!(
            holder_values(
                "import { A } from './a';\n\
                 import * as ns from './ns';\n\
                 export default { a: A, 'b': ns.B, [A.name]: A };\n"
            ),
            ["default|a|A|", "default|b|ns|B"]
        );
        assert_eq!(
            holder_values(
                "import { A } from './a';\n\
                 export const named = { a: A };\n\
                 const local = { a: A };\n\
                 export { local as renamed };\n"
            ),
            ["named|a|A|", "renamed|a|A|"]
        );
    }

    #[test]
    fn holders_that_no_export_can_name_record_no_value() {
        assert!(
            holder_values(
                "import { A } from './a';\n\
                 const unexported = { a: A };\n\
                 export function make() { const inner = { a: A }; return inner; }\n\
                 export let reassignable = { a: A };\n"
            )
            .is_empty()
        );
    }

    #[test]
    fn computed_key_prototype_read_of_an_import_is_recorded() {
        let recorded = uses(
            "import holder from './holder';\n\
             export const a = (k: string) => holder[k].prototype.x;\n\
             export const b = () => holder['s'].prototype.y;\n",
        );
        assert!(
            recorded.contains(&"holder|[].prototype.x|false".to_string()),
            "{recorded:?}"
        );
        assert!(
            recorded
                .iter()
                .any(|use_| use_.starts_with("holder|s.prototype.y|")),
            "{recorded:?}"
        );
        assert!(
            !recorded
                .iter()
                .any(|use_| use_.starts_with("holder|[].prototype.y")),
            "{recorded:?}"
        );
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
