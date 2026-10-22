//! Syntactic Effect facts for the Effect dead-code rules.
//!
//! The collector runs only for a module that imports `effect` or an
//! `@effect/*` package. It records:
//! - module-scope classes that extend a tagged error constructor,
//! - the tags that `catchTag` and `catchTags` handlers name,
//! - tags that a literal `_tag` property or a tagged constructor declares,
//! - layer definitions and the layer keys that each initializer reads,
//! - the keys that value references outside every layer definition read.
//!
//! A key is a binding name (`FooLive`) or a class static member
//! (`FooService.layer`). Import aliases resolve to the imported name, so the
//! analyze layer can join keys across modules by name.

use std::borrow::Cow;

use oxc_ast::AstKind;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BindingPattern, CallExpression, Class, ClassElement,
    Declaration, ExportDefaultDeclarationKind, Expression, ObjectPropertyKind, Program,
    PropertyKey, Statement,
};
use oxc_semantic::{AstNodes, NodeId, Semantic, SymbolId};
use oxc_span::{GetSpan, Span};
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{
    EffectCaughtTag, EffectFacts, EffectLayer, EffectTaggedError, ExportInfo, ExportName,
    ImportInfo, ImportedName,
};

use crate::js_text::JsText;

/// `Layer` functions that return an `Effect` or a runtime, not a layer.
const NON_LAYER_FUNCTIONS: &[&str] = &[
    "launch",
    "build",
    "buildWithScope",
    "buildWithMemoMap",
    "toRuntime",
    "toRuntimeWithMemoMap",
    "makeMemoMap",
    "isLayer",
    "isFresh",
];

/// Superclass constructors that declare a tagged error class.
const TAGGED_ERROR_CONSTRUCTORS: &[&str] = &["TaggedError", "TaggedErrorClass"];

/// Constructors whose first string argument declares a tag that a value of
/// the constructed type carries. Each one counts as a producer of its tag.
const TAG_DECLARING_CONSTRUCTORS: &[&str] = &[
    "TaggedClass",
    "TaggedRequest",
    "TaggedStruct",
    "tagged",
    "tag",
];

/// Return true when the specifier names the Effect core package or an
/// `@effect/*` package.
#[must_use]
pub fn is_effect_specifier(source: &str) -> bool {
    source == "effect" || source.starts_with("effect/") || source.starts_with("@effect/")
}

/// Collect the Effect facts of one module, or `None` when the module does not
/// import Effect or declares nothing of interest.
pub(crate) fn collect(
    semantic: &Semantic<'_>,
    program: &Program<'_>,
    imports: &[ImportInfo],
    exports: &[ExportInfo],
) -> Option<Box<EffectFacts>> {
    if !imports
        .iter()
        .any(|import| is_effect_specifier(&import.source))
    {
        return None;
    }
    let mut collector = Collector::new(semantic, imports, exports);
    collector.collect_declarations(program);
    collector.collect_layer_references();
    collector.collect_this_references();
    collector.collect_call_and_tag_sites();
    collector.finish()
}

struct LayerCandidate {
    key: String,
    span_start: u32,
    exported: bool,
    init_span: Span,
    dependencies: Vec<String>,
}

struct ErrorCandidate {
    symbol: SymbolId,
    class_span: Span,
    error: EffectTaggedError,
}

struct StaticSlot {
    class_name: String,
    value_span: Span,
}

struct Collector<'s, 'a> {
    semantic: &'s Semantic<'a>,
    imports: &'s [ImportInfo],
    /// Local names that the module exports (`export const`, `export { X }`).
    exported_locals: FxHashSet<String>,
    /// Local names that the module exports as `default`. An importer binds
    /// them under any name, so name joins cannot follow them.
    default_locals: FxHashSet<String>,
    /// Symbols of the `Layer` namespace binding (`import { Layer } from "effect"`).
    layer_namespaces: FxHashSet<SymbolId>,
    /// Import binding symbol to its import record.
    import_symbols: FxHashMap<SymbolId, usize>,
    layers: Vec<LayerCandidate>,
    errors: Vec<ErrorCandidate>,
    static_slots: Vec<StaticSlot>,
    root_keys: FxHashSet<String>,
    caught_tags: Vec<EffectCaughtTag>,
    literal_tags: FxHashSet<String>,
}

impl<'s, 'a> Collector<'s, 'a> {
    fn new(semantic: &'s Semantic<'a>, imports: &'s [ImportInfo], exports: &[ExportInfo]) -> Self {
        let mut exported_locals = FxHashSet::default();
        let mut default_locals = FxHashSet::default();
        for export in exports {
            if let (ExportName::Default, Some(local)) = (&export.name, &export.local_name) {
                default_locals.insert(local.clone());
            }
            if let Some(local) = &export.local_name {
                exported_locals.insert(local.clone());
            } else if let ExportName::Named(name) = &export.name {
                exported_locals.insert(name.clone());
            }
        }
        let mut layer_namespaces = FxHashSet::default();
        let mut import_symbols = FxHashMap::default();
        for (index, import) in imports.iter().enumerate() {
            let Some(symbol) = crate::binding_references::admitted_import_symbol(semantic, import)
            else {
                continue;
            };
            import_symbols.insert(symbol, index);
            let is_layer_namespace = match &import.imported_name {
                ImportedName::Named(name) => name == "Layer" && is_effect_specifier(&import.source),
                ImportedName::Namespace => import.source == "effect/Layer",
                ImportedName::Default | ImportedName::SideEffect => false,
            };
            if is_layer_namespace {
                layer_namespaces.insert(symbol);
            }
        }
        Self {
            semantic,
            imports,
            exported_locals,
            default_locals,
            layer_namespaces,
            import_symbols,
            layers: Vec::new(),
            errors: Vec::new(),
            static_slots: Vec::new(),
            root_keys: FxHashSet::default(),
            caught_tags: Vec::new(),
            literal_tags: FxHashSet::default(),
        }
    }

    fn finish(self) -> Option<Box<EffectFacts>> {
        let mut layer_root_keys: Vec<String> = self.root_keys.into_iter().collect();
        layer_root_keys.sort_unstable();
        let mut literal_tags: Vec<String> = self.literal_tags.into_iter().collect();
        literal_tags.sort_unstable();
        let facts = EffectFacts {
            tagged_errors: self.errors.into_iter().map(|error| error.error).collect(),
            caught_tags: self.caught_tags,
            literal_tags,
            layers: self
                .layers
                .into_iter()
                .map(|layer| {
                    let mut dependencies = layer.dependencies;
                    dependencies.sort_unstable();
                    dependencies.dedup();
                    EffectLayer {
                        key: layer.key,
                        span_start: layer.span_start,
                        exported: layer.exported,
                        dependencies,
                    }
                })
                .collect(),
            layer_root_keys,
        };
        let empty = facts.tagged_errors.is_empty()
            && facts.caught_tags.is_empty()
            && facts.literal_tags.is_empty()
            && facts.layers.is_empty()
            && facts.layer_root_keys.is_empty();
        (!empty).then(|| Box::new(facts))
    }

    // ── Declarations ────────────────────────────────────────────────

    fn collect_declarations(&mut self, program: &Program<'a>) {
        // Pass 1 finds direct `Layer.*` initializers. Later passes admit
        // `.pipe(...)` chains on a known layer const until no change. The cap
        // only drops very deep chains, and a missed layer is never reported.
        let mut known: FxHashSet<String> = FxHashSet::default();
        for _ in 0..4 {
            let before = known.len();
            for statement in &program.body {
                self.scan_statement_for_layer_names(statement, &mut known);
            }
            if known.len() == before {
                break;
            }
        }
        for statement in &program.body {
            self.record_statement(statement, &known);
        }
    }

    fn scan_statement_for_layer_names(
        &self,
        statement: &Statement<'a>,
        known: &mut FxHashSet<String>,
    ) {
        let declaration = match statement {
            Statement::VariableDeclaration(declaration) => declaration,
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => declaration,
                _ => return,
            },
            _ => return,
        };
        for declarator in &declaration.declarations {
            let (BindingPattern::BindingIdentifier(id), Some(init)) =
                (&declarator.id, &declarator.init)
            else {
                continue;
            };
            if self.is_layer_expression(init, known) {
                known.insert(id.name.to_string());
            }
        }
    }

    fn record_statement(&mut self, statement: &Statement<'a>, known: &FxHashSet<String>) {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                self.record_variables(declaration, false, known);
            }
            Statement::ClassDeclaration(class) => self.record_class(class, false, known),
            Statement::ExportDeclaration(export) => match &export.declaration {
                Declaration::VariableDeclaration(declaration) => {
                    self.record_variables(declaration, true, known);
                }
                Declaration::ClassDeclaration(class) => self.record_class(class, true, known),
                _ => {}
            },
            Statement::ExportDefaultDeclaration(export) => {
                // A default-exported class keeps its layers (the class is the
                // public value) but is never a tagged error candidate: an
                // importer binds it under any local name.
                if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &export.declaration {
                    if let Some(id) = &class.id {
                        self.default_locals.insert(id.name.to_string());
                    }
                    self.record_class_layers(class, true, known);
                }
            }
            _ => {}
        }
    }

    fn record_variables(
        &mut self,
        declaration: &oxc_ast::ast::VariableDeclaration<'a>,
        exported: bool,
        known: &FxHashSet<String>,
    ) {
        for declarator in &declaration.declarations {
            let (BindingPattern::BindingIdentifier(id), Some(init)) =
                (&declarator.id, &declarator.init)
            else {
                continue;
            };
            if !self.is_layer_expression(init, known) {
                continue;
            }
            let name = id.name.to_string();
            let exported = exported || self.exported_locals.contains(&name);
            if self.default_locals.contains(&name) {
                self.root_keys.insert(name.clone());
            }
            self.layers.push(LayerCandidate {
                key: name,
                span_start: id.span.start,
                exported,
                init_span: init.span(),
                dependencies: Vec::new(),
            });
        }
    }

    fn record_class(&mut self, class: &Class<'a>, exported: bool, known: &FxHashSet<String>) {
        self.record_class_layers(class, exported, known);
        let Some(id) = &class.id else {
            return;
        };
        let Some(tag) = class
            .heritage
            .as_ref()
            .and_then(|heritage| tagged_error_tag(&heritage.expression))
        else {
            return;
        };
        let name = id.name.to_string();
        if self.default_locals.contains(&name) {
            return;
        }
        let exported = exported || self.exported_locals.contains(&name);
        self.errors.push(ErrorCandidate {
            symbol: id.symbol_id(),
            class_span: class.span,
            error: EffectTaggedError {
                name,
                tag,
                span_start: id.span.start,
                exported,
                local_value_reference: false,
                local_type_reference: false,
            },
        });
    }

    fn record_class_layers(
        &mut self,
        class: &Class<'a>,
        exported: bool,
        known: &FxHashSet<String>,
    ) {
        let Some(id) = &class.id else {
            return;
        };
        let class_name = id.name.to_string();
        let exported = exported || self.exported_locals.contains(&class_name);
        let default_exported = self.default_locals.contains(&class_name);
        for element in &class.body.body {
            let ClassElement::PropertyDefinition(property) = element else {
                continue;
            };
            if !property.r#static {
                continue;
            }
            let (PropertyKey::StaticIdentifier(key), Some(value)) =
                (&property.key, &property.value)
            else {
                continue;
            };
            self.static_slots.push(StaticSlot {
                class_name: class_name.clone(),
                value_span: value.span(),
            });
            if !self.is_layer_expression(value, known) {
                continue;
            }
            let layer_key = format!("{class_name}.{}", key.name);
            if default_exported {
                self.root_keys.insert(layer_key.clone());
            }
            self.layers.push(LayerCandidate {
                key: layer_key,
                span_start: key.span.start,
                exported,
                init_span: value.span(),
                dependencies: Vec::new(),
            });
        }
    }

    // ── Layer expression shape ──────────────────────────────────────

    fn is_layer_expression(&self, expression: &Expression<'a>, known: &FxHashSet<String>) -> bool {
        let expression = expression.without_parentheses();
        let expression = match expression {
            Expression::TSAsExpression(node) => &node.expression,
            Expression::TSSatisfiesExpression(node) => &node.expression,
            _ => expression,
        };
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        if self.is_layer_namespace_call(call) {
            return true;
        }
        let Expression::StaticMemberExpression(member) = call.callee.without_parentheses() else {
            return false;
        };
        if member.property.name != "pipe" || call.arguments.is_empty() {
            return false;
        }
        if self.is_layer_expression(&member.object, known) {
            return true;
        }
        if let Expression::Identifier(object) = member.object.without_parentheses()
            && known.contains(object.name.as_str())
        {
            return true;
        }
        call.arguments.iter().all(|argument| {
            argument.as_expression().is_some_and(|expression| {
                matches!(
                    expression.without_parentheses(),
                    Expression::CallExpression(inner) if self.is_layer_namespace_call(inner)
                )
            })
        })
    }

    /// `Layer.<fn>(...)` where `Layer` is the Effect layer namespace and `<fn>`
    /// returns a layer.
    fn is_layer_namespace_call(&self, call: &CallExpression<'a>) -> bool {
        let Expression::StaticMemberExpression(member) = call.callee.without_parentheses() else {
            return false;
        };
        let Expression::Identifier(object) = member.object.without_parentheses() else {
            return false;
        };
        if NON_LAYER_FUNCTIONS.contains(&member.property.name.as_str()) {
            return false;
        }
        let Some(reference_id) = object.reference_id.get() else {
            return false;
        };
        self.semantic
            .scoping()
            .get_reference(reference_id)
            .symbol_id()
            .is_some_and(|symbol| self.layer_namespaces.contains(&symbol))
    }

    // ── References ──────────────────────────────────────────────────

    fn collect_layer_references(&mut self) {
        let scoping = self.semantic.scoping();
        let nodes = self.semantic.nodes();
        let root_scope = scoping.root_scope_id();
        let mut keyed: Vec<(Span, String)> = Vec::new();
        let mut error_references: Vec<(usize, bool, bool)> = Vec::new();
        // A local binding can only name a layer of this module: a layer const
        // or the class of a layer static. Other local bindings give no key.
        let local_layer_heads: FxHashSet<String> = self
            .layers
            .iter()
            .map(|layer| {
                layer
                    .key
                    .split_once('.')
                    .map_or(layer.key.as_str(), |(head, _)| head)
                    .to_string()
            })
            .collect();
        for symbol in scoping.symbol_ids() {
            if scoping.symbol_scope_id(symbol) != root_scope {
                continue;
            }
            let import = self
                .import_symbols
                .get(&symbol)
                .map(|&index| &self.imports[index]);
            if import.is_some_and(|import| is_effect_specifier(&import.source)) {
                continue;
            }
            let error_index = self.errors.iter().position(|error| error.symbol == symbol);
            let name = scoping.symbol_name(symbol);
            let wants_key = import.is_some() || local_layer_heads.contains(name);
            if error_index.is_none() && !wants_key {
                continue;
            }
            for reference in scoping.get_resolved_references(symbol) {
                let node_id = reference.node_id();
                let AstKind::IdentifierReference(identifier) = nodes.kind(node_id) else {
                    continue;
                };
                if matches!(nodes.parent_kind(node_id), AstKind::ExportSpecifier(_)) {
                    continue;
                }
                if let Some(index) = error_index {
                    let error = &self.errors[index];
                    if !contains(error.class_span, identifier.span) {
                        error_references.push((index, reference.is_value(), reference.is_type()));
                    }
                }
                if !wants_key || !reference.is_value() {
                    continue;
                }
                let path = member_path(nodes, node_id, identifier.span);
                let Some(key) = reference_key(import, name, &path) else {
                    continue;
                };
                keyed.push((identifier.span, key));
            }
        }
        for (index, is_value, is_type) in error_references {
            let error = &mut self.errors[index].error;
            error.local_value_reference |= is_value;
            error.local_type_reference |= is_type;
        }
        for (span, key) in keyed {
            self.attach_key(span, key);
        }
    }

    /// `this.<member>` inside a class static property initializer reads the
    /// static `<Class>.<member>`.
    fn collect_this_references(&mut self) {
        let nodes = self.semantic.nodes();
        let mut keyed: Vec<(Span, String)> = Vec::new();
        for node in nodes.iter() {
            let AstKind::StaticMemberExpression(member) = node.kind() else {
                continue;
            };
            if !matches!(member.object, Expression::ThisExpression(_)) {
                continue;
            }
            let Some(slot) = self
                .static_slots
                .iter()
                .find(|slot| contains(slot.value_span, member.span))
            else {
                continue;
            };
            keyed.push((
                member.span,
                format!("{}.{}", slot.class_name, member.property.name),
            ));
        }
        for (span, key) in keyed {
            self.attach_key(span, key);
        }
    }

    fn attach_key(&mut self, span: Span, key: String) {
        // The innermost layer initializer that contains the reference owns it.
        let owner = self
            .layers
            .iter_mut()
            .filter(|layer| contains(layer.init_span, span))
            .min_by_key(|layer| layer.init_span.size());
        match owner {
            Some(layer) if layer.key != key => layer.dependencies.push(key),
            Some(_) => {}
            None => {
                self.root_keys.insert(key);
            }
        }
    }

    // ── Calls and tags ──────────────────────────────────────────────

    fn collect_call_and_tag_sites(&mut self) {
        let nodes = self.semantic.nodes();
        for node in nodes.iter() {
            match node.kind() {
                AstKind::CallExpression(call) => self.inspect_call(call),
                AstKind::ObjectProperty(property) => {
                    if property_key_name(&property.key).as_deref() == Some("_tag")
                        && let Some(tag) = literal_or_first_string_argument(&property.value)
                    {
                        self.literal_tags.insert(tag);
                    }
                }
                AstKind::PropertyDefinition(property) => {
                    if property_key_name(&property.key).as_deref() == Some("_tag")
                        && let Some(tag) = property
                            .value
                            .as_ref()
                            .and_then(literal_or_first_string_argument)
                    {
                        self.literal_tags.insert(tag);
                    }
                }
                _ => {}
            }
        }
    }

    fn inspect_call(&mut self, call: &CallExpression<'a>) {
        let property = match call.callee.without_parentheses() {
            Expression::StaticMemberExpression(member) => member.property.name.as_str(),
            Expression::Identifier(identifier) => identifier.name.as_str(),
            _ => return,
        };
        match property {
            "catchTag" => self.record_catch_tag(call),
            "catchTags" => self.record_catch_tags(call),
            _ if TAG_DECLARING_CONSTRUCTORS.contains(&property) => {
                if let Some(tag) = first_string_argument(call) {
                    self.literal_tags.insert(tag);
                }
            }
            _ => {}
        }
    }

    fn record_catch_tag(&mut self, call: &CallExpression<'a>) {
        for argument in &call.arguments {
            let Some(expression) = argument.as_expression() else {
                return;
            };
            match expression.without_parentheses() {
                Expression::StringLiteral(literal) => self.caught_tags.push(EffectCaughtTag {
                    tag: literal.value.text().into_owned(),
                    span_start: literal.span.start + 1,
                }),
                Expression::ArrayExpression(array) => {
                    for element in &array.elements {
                        if let ArrayExpressionElement::StringLiteral(literal) = element {
                            self.caught_tags.push(EffectCaughtTag {
                                tag: literal.value.text().into_owned(),
                                span_start: literal.span.start + 1,
                            });
                        }
                    }
                }
                Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                    return;
                }
                _ => {}
            }
        }
    }

    fn record_catch_tags(&mut self, call: &CallExpression<'a>) {
        for argument in &call.arguments {
            let Some(Expression::ObjectExpression(object)) = argument
                .as_expression()
                .map(Expression::without_parentheses)
            else {
                continue;
            };
            for property in &object.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                if property.computed {
                    continue;
                }
                if let Some(tag) = property_key_name(&property.key) {
                    let quoted = matches!(property.key, PropertyKey::StringLiteral(_));
                    self.caught_tags.push(EffectCaughtTag {
                        tag: tag.to_string(),
                        span_start: property.key.span().start + u32::from(quoted),
                    });
                }
            }
        }
    }
}

/// The tag of a tagged error superclass expression:
/// `Data.TaggedError("X")`, `Schema.TaggedError<T>()("X", ...)` and
/// `Schema.TaggedErrorClass<T>()("X", ...)`.
fn tagged_error_tag(super_class: &Expression<'_>) -> Option<String> {
    let Expression::CallExpression(call) = super_class.without_parentheses() else {
        return None;
    };
    match call.callee.without_parentheses() {
        Expression::StaticMemberExpression(member)
            if TAGGED_ERROR_CONSTRUCTORS.contains(&member.property.name.as_str()) =>
        {
            first_string_argument(call)
        }
        Expression::CallExpression(inner) => {
            let Expression::StaticMemberExpression(member) = inner.callee.without_parentheses()
            else {
                return None;
            };
            if !TAGGED_ERROR_CONSTRUCTORS.contains(&member.property.name.as_str()) {
                return None;
            }
            first_string_argument(call)
        }
        _ => None,
    }
}

fn first_string_argument(call: &CallExpression<'_>) -> Option<String> {
    match call.arguments.first()? {
        Argument::StringLiteral(literal) => Some(literal.value.text().into_owned()),
        _ => None,
    }
}

fn literal_or_first_string_argument(expression: &Expression<'_>) -> Option<String> {
    match unwrap_type_wrappers(expression) {
        Expression::StringLiteral(literal) => Some(literal.value.text().into_owned()),
        Expression::CallExpression(call) => first_string_argument(call),
        _ => None,
    }
}

/// Strip parentheses and the TypeScript wrappers that keep the runtime value
/// (`"X" as const`, `"X" satisfies Tag`, `x!`).
fn unwrap_type_wrappers<'e, 'a>(expression: &'e Expression<'a>) -> &'e Expression<'a> {
    let mut current = expression.without_parentheses();
    loop {
        current = match current {
            Expression::TSAsExpression(node) => node.expression.without_parentheses(),
            Expression::TSSatisfiesExpression(node) => node.expression.without_parentheses(),
            Expression::TSNonNullExpression(node) => node.expression.without_parentheses(),
            _ => return current,
        };
    }
}

fn property_key_name<'k>(key: &'k PropertyKey<'_>) -> Option<Cow<'k, str>> {
    match key {
        PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str().into()),
        PropertyKey::StringLiteral(literal) => Some(literal.value.text()),
        _ => None,
    }
}

const fn contains(outer: Span, inner: Span) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}

/// Static member names read off the identifier, outermost last
/// (`Ns.Foo.layer` gives `["Foo", "layer"]`).
fn member_path(nodes: &AstNodes<'_>, node_id: NodeId, identifier_span: Span) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = node_id;
    let mut span = identifier_span;
    while path.len() < 2 {
        let parent = nodes.parent_id(current);
        if parent == current {
            break;
        }
        match nodes.kind(parent) {
            AstKind::StaticMemberExpression(member) if member.object.span() == span => {
                path.push(member.property.name.to_string());
                current = parent;
                span = member.span;
            }
            _ => break,
        }
    }
    path
}

/// The layer key for a value reference. A namespace import reads its member,
/// a named import reads its imported name, and a local binding reads itself.
/// One member step is kept for a static (`Foo.layer`).
fn reference_key(import: Option<&ImportInfo>, local_name: &str, path: &[String]) -> Option<String> {
    let (base, rest) = match import.map(|import| &import.imported_name) {
        Some(ImportedName::Namespace) => {
            let (first, rest) = path.split_first()?;
            (first.as_str(), rest)
        }
        Some(ImportedName::Named(imported)) => (imported.as_str(), path),
        Some(ImportedName::Default | ImportedName::SideEffect) => return None,
        None => (local_name, path),
    };
    Some(match rest.first() {
        Some(member) => format!("{base}.{member}"),
        None => base.to_string(),
    })
}
