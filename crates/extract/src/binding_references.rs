//! Value references to runtime ESM import bindings.
//!
//! The semantic pass records each value use of an admitted import binding that
//! is not an admitted call site. It also records an admitted call that is the
//! whole initializer of a top-level declarator. `fallow trace --dependency`
//! reads these facts to count the uses of an imported name that it cannot
//! resolve, and to find one-hop project wrappers of a package export.

use fallow_types::extract::{ImportBindingReference, ImportBindingReferenceKind, ImportInfo};
use oxc_ast::AstKind;
use oxc_semantic::{AstNodes, NodeId, Semantic, SymbolId};
use oxc_span::{GetSpan, Span};
use rustc_hash::FxHashSet;

/// The root binding of `import` when it is one runtime ESM import declaration:
/// one declaration of kind `ImportSpecifier`, `ImportDefaultSpecifier` or
/// `ImportNamespaceSpecifier`, and not type-only.
pub fn admitted_import_symbol(semantic: &Semantic<'_>, import: &ImportInfo) -> Option<SymbolId> {
    if import.is_type_only || import.local_name.is_empty() {
        return None;
    }
    let scoping = semantic.scoping();
    let symbol = scoping.get_binding(
        scoping.root_scope_id(),
        oxc_str::Ident::from(import.local_name.as_str()),
    )?;
    let mut declarations = scoping.symbol_declarations(symbol);
    let declaration = declarations.next()?;
    if declarations.next().is_some() {
        return None;
    }
    matches!(
        semantic.nodes().kind(declaration),
        AstKind::ImportSpecifier(_)
            | AstKind::ImportDefaultSpecifier(_)
            | AstKind::ImportNamespaceSpecifier(_)
    )
    .then_some(symbol)
}

/// Collect the import binding references of a module, sorted by
/// `span_start`.
///
/// `admitted_calls` holds the root identifier spans of the admitted imported
/// call sites. Such a reference is a call site, not a binding reference,
/// unless the call is the whole initializer of a top-level declarator.
pub fn collect(
    semantic: &Semantic<'_>,
    imports: &[ImportInfo],
    admitted_calls: &FxHashSet<Span>,
) -> Vec<ImportBindingReference> {
    let scoping = semantic.scoping();
    let nodes = semantic.nodes();
    let mut references = Vec::new();
    for (index, import) in imports.iter().enumerate() {
        let Ok(import_index) = u32::try_from(index) else {
            break;
        };
        let Some(symbol) = admitted_import_symbol(semantic, import) else {
            continue;
        };
        for reference in scoping.get_resolved_references(symbol) {
            if !reference.is_value() {
                continue;
            }
            let node_id = reference.node_id();
            let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
                continue;
            };
            let is_call = admitted_calls.contains(&identifier.span);
            if let Some(classified) = classify(nodes, node_id, identifier.span, is_call) {
                references.push(ImportBindingReference {
                    import_index,
                    member_path: classified.member_path.into_boxed_str(),
                    kind: classified.kind,
                    span_start: classified.span_start,
                    declared_name: classified.declared_name,
                });
            }
        }
    }
    references.sort_by_key(|reference| reference.span_start);
    references
}

struct Classified {
    kind: ImportBindingReferenceKind,
    member_path: String,
    span_start: u32,
    declared_name: Option<Box<str>>,
}

/// The outermost static member chain over a root identifier.
struct MemberChain {
    node: NodeId,
    span: Span,
    path: String,
}

fn classify(
    nodes: &AstNodes<'_>,
    node_id: NodeId,
    identifier_span: Span,
    is_admitted_call: bool,
) -> Option<Classified> {
    let chain = member_chain(nodes, node_id, identifier_span);
    if is_admitted_call {
        return classify_admitted_call(nodes, &chain);
    }
    let parent = nodes.parent_kind(chain.node);
    let kind = match parent {
        AstKind::JSXOpeningElement(_) => ImportBindingReferenceKind::JsxElement,
        AstKind::JSXClosingElement(_) | AstKind::TSTypeQuery(_) | AstKind::TSQualifiedName(_) => {
            return None;
        }
        _ => {
            let (outer, outer_span) = unwrap_upward(nodes, chain.node, chain.span);
            match initialized_declarator(nodes, outer, outer_span) {
                Some(declarator) => {
                    return Some(Classified {
                        kind: ImportBindingReferenceKind::ValueAlias,
                        member_path: chain.path,
                        span_start: identifier_span.start,
                        declared_name: top_level_declared_name(nodes, declarator),
                    });
                }
                None => ImportBindingReferenceKind::Other,
            }
        }
    };
    Some(Classified {
        kind,
        member_path: chain.path,
        span_start: identifier_span.start,
        declared_name: None,
    })
}

/// An admitted call is a call site. Record it only when the call is the whole
/// initializer of a top-level declarator, so a wrapper can be found.
fn classify_admitted_call(nodes: &AstNodes<'_>, chain: &MemberChain) -> Option<Classified> {
    let mut callee = chain.node;
    let mut callee_span = chain.span;
    let call = loop {
        let parent = nodes.parent_id(callee);
        if parent == callee {
            return None;
        }
        match nodes.kind(parent) {
            AstKind::ParenthesizedExpression(paren) => {
                callee = parent;
                callee_span = paren.span;
            }
            AstKind::CallExpression(call) if call.callee.span() == callee_span => break call,
            _ => return None,
        }
    };
    let call_id = nodes.parent_id(callee);
    let (outer, outer_span) = unwrap_upward(nodes, call_id, call.span);
    let declarator = initialized_declarator(nodes, outer, outer_span)?;
    let declared_name = top_level_declared_name(nodes, declarator)?;
    Some(Classified {
        kind: ImportBindingReferenceKind::InitializerCall,
        member_path: chain.path.clone(),
        span_start: call.span.start,
        declared_name: Some(declared_name),
    })
}

fn member_chain(nodes: &AstNodes<'_>, node_id: NodeId, identifier_span: Span) -> MemberChain {
    let mut current = node_id;
    let mut span = identifier_span;
    let mut path = String::new();
    loop {
        let parent = nodes.parent_id(current);
        if parent == current {
            break;
        }
        let (property, parent_span) = match nodes.kind(parent) {
            AstKind::StaticMemberExpression(member) if member.object.span() == span => {
                (member.property.name.as_str(), member.span)
            }
            AstKind::JSXMemberExpression(member) if member.object.span() == span => {
                (member.property.name.as_str(), member.span)
            }
            _ => break,
        };
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(property);
        current = parent;
        span = parent_span;
    }
    MemberChain {
        node: current,
        span,
        path,
    }
}

/// Walk up through parentheses and the TypeScript expression wrappers that
/// do not change the runtime value.
fn unwrap_upward(nodes: &AstNodes<'_>, node_id: NodeId, span: Span) -> (NodeId, Span) {
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

/// The declarator whose whole initializer is the node at `span`.
fn initialized_declarator(nodes: &AstNodes<'_>, node_id: NodeId, span: Span) -> Option<NodeId> {
    let parent = nodes.parent_id(node_id);
    match nodes.kind(parent) {
        AstKind::VariableDeclarator(declarator)
            if declarator.init.as_ref().map(GetSpan::span) == Some(span) =>
        {
            Some(parent)
        }
        _ => None,
    }
}

/// The declared name when the declarator is top level:
/// `VariableDeclarator -> VariableDeclaration -> (export declaration ->) Program`,
/// and its binding is a plain identifier.
fn top_level_declared_name(nodes: &AstNodes<'_>, declarator_id: NodeId) -> Option<Box<str>> {
    let AstKind::VariableDeclarator(declarator) = nodes.kind(declarator_id) else {
        return None;
    };
    let identifier = declarator.id.get_binding_identifier()?;
    let declaration = nodes.parent_id(declarator_id);
    if !matches!(nodes.kind(declaration), AstKind::VariableDeclaration(_)) {
        return None;
    }
    let mut owner = nodes.parent_id(declaration);
    if matches!(
        nodes.kind(owner),
        AstKind::ExportNamedDeclaration(_) | AstKind::ExportDeclaration(_)
    ) {
        owner = nodes.parent_id(owner);
    }
    matches!(nodes.kind(owner), AstKind::Program(_)).then(|| identifier.name.as_str().into())
}
