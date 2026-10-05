//! Framework-neutral component input declarations and closed caller evidence.
//!
//! The existing semantic pass supplies symbol identities. Unknown syntax stays
//! explicit so analysis never interprets an uninspected consumer as an empty call.

mod classes;

use oxc_ast::{AstKind, ast::*};
use oxc_semantic::Semantic;
use oxc_span::GetSpan;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{
    ComponentContractFacts, ComponentFramework, ComponentInvocation, ComponentPropDeclaration,
    ComponentReference, ImportInfo,
};

#[derive(Clone)]
struct PropShape {
    name: String,
    span_start: u32,
    optional: bool,
    has_default: bool,
}

pub fn collect(
    semantic: &Semantic<'_>,
    imports: &[ImportInfo],
    exports: &[crate::ExportInfo],
) -> ComponentContractFacts {
    let framework = jsx_framework(imports);
    let mut collector = ContractCollector {
        semantic,
        imports,
        framework,
        facts: ComponentContractFacts::default(),
        types: FxHashMap::default(),
        aliases: FxHashMap::default(),
        objects: FxHashMap::default(),
        inspected_references: FxHashSet::default(),
        components: FxHashSet::default(),
    };
    collector.collect_shapes();
    collector.collect_alias_bindings();
    collector.collect_spread_bindings();
    collector.collect_declarations();
    collector.collect_callers();
    collector.collect_escapes();
    collector.collect_exports(exports);
    collector.facts
}

fn jsx_framework(imports: &[ImportInfo]) -> ComponentFramework {
    if imports.iter().any(|i| i.source.starts_with("preact")) {
        return ComponentFramework::Preact;
    }
    if imports.iter().any(|i| i.source.starts_with("solid-js")) {
        return ComponentFramework::Solid;
    }
    if imports
        .iter()
        .any(|i| i.source.starts_with("@builder.io/qwik") || i.source.starts_with("@qwik.dev/"))
    {
        return ComponentFramework::Qwik;
    }
    ComponentFramework::React
}

struct ContractCollector<'s, 'a> {
    semantic: &'s Semantic<'a>,
    imports: &'s [ImportInfo],
    framework: ComponentFramework,
    facts: ComponentContractFacts,
    types: FxHashMap<u32, Option<Vec<PropShape>>>,
    aliases: FxHashMap<u32, &'s IdentifierReference<'a>>,
    objects: FxHashMap<u32, &'s ObjectExpression<'a>>,
    inspected_references: FxHashSet<u32>,
    components: FxHashSet<u32>,
}

impl ContractCollector<'_, '_> {
    fn reference_span(&self, id: &IdentifierReference<'_>) -> Option<u32> {
        let reference = self
            .semantic
            .scoping()
            .get_reference(id.reference_id.get()?);
        Some(
            self.semantic
                .scoping()
                .symbol_span(reference.symbol_id()?)
                .start,
        )
    }

    fn binding_reference(&self, id: &IdentifierReference<'_>, depth: usize) -> ComponentReference {
        let Some(span_start) = self.reference_span(id) else {
            return ComponentReference::Unknown;
        };
        if depth > self.aliases.len() {
            return ComponentReference::Unknown;
        }
        if let Some(alias) = self.aliases.get(&span_start) {
            return self.binding_reference(alias, depth + 1);
        }
        let scoping = self.semantic.scoping();
        let Some(reference_id) = id.reference_id.get() else {
            return ComponentReference::Unknown;
        };
        let reference = scoping.get_reference(reference_id);
        let Some(symbol) = reference.symbol_id() else {
            return ComponentReference::Unknown;
        };
        let name = scoping.symbol_name(symbol).to_string();
        if scoping.symbol_scope_id(symbol) == scoping.root_scope_id()
            && self.imports.iter().any(|i| i.local_name == name)
        {
            return ComponentReference::Import {
                local: name,
                span_start,
            };
        }
        ComponentReference::Local { name, span_start }
    }

    fn collect_alias_bindings(&mut self) {
        let scoping = self.semantic.scoping();
        for symbol in scoping.symbol_ids() {
            if scoping.symbol_scope_id(symbol) != scoping.root_scope_id() {
                continue;
            }
            let Some(alias) = self.aliases.get(&scoping.symbol_span(symbol).start) else {
                continue;
            };
            let target = self.binding_reference(alias, 0);
            if !matches!(target, ComponentReference::Unknown) {
                self.facts
                    .aliases
                    .push(fallow_types::extract::ComponentAliasBinding {
                        local: scoping.symbol_name(symbol).to_string(),
                        target,
                    });
            }
        }
    }

    fn collect_shapes(&mut self) {
        for node in self.semantic.nodes().iter() {
            match node.kind() {
                AstKind::TSInterfaceDeclaration(decl) => {
                    self.types.insert(
                        decl.id.span.start,
                        if decl.extends.is_empty() && decl.type_parameters.is_none() {
                            signature_shapes(&decl.body.body)
                        } else {
                            None
                        },
                    );
                }
                AstKind::TSTypeAliasDeclaration(decl) => {
                    self.types.insert(
                        decl.id.span.start,
                        type_literal_shapes(&decl.type_annotation),
                    );
                }
                AstKind::VariableDeclarator(decl) => {
                    let BindingPattern::BindingIdentifier(id) = &decl.id else {
                        continue;
                    };
                    if !self.is_immutable_binding(id) {
                        continue;
                    }
                    match decl.init.as_ref().map(Expression::without_parentheses) {
                        Some(Expression::Identifier(alias)) => {
                            self.aliases.insert(id.span.start, alias);
                        }
                        Some(Expression::ObjectExpression(object)) => {
                            self.objects.insert(id.span.start, object);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    fn is_immutable_binding(&self, id: &BindingIdentifier<'_>) -> bool {
        let Some(symbol) = id.symbol_id.get() else {
            return false;
        };
        let scoping = self.semantic.scoping();
        let declaration = scoping.symbol_declaration(symbol);
        let AstKind::VariableDeclarator(_) = self.semantic.nodes().kind(declaration) else {
            return false;
        };
        if !matches!(self.semantic.nodes().parent_kind(declaration), AstKind::VariableDeclaration(decl) if decl.kind == VariableDeclarationKind::Const)
        {
            return false;
        }
        !scoping
            .get_resolved_references(symbol)
            .any(|reference| reference.is_write())
    }

    fn collect_spread_bindings(&mut self) {
        for (&span, object) in &self.objects {
            let Some(symbol) = self
                .semantic
                .scoping()
                .symbol_ids()
                .find(|symbol| self.semantic.scoping().symbol_span(*symbol).start == span)
            else {
                continue;
            };
            if self.semantic.scoping().symbol_scope_id(symbol)
                != self.semantic.scoping().root_scope_id()
                || !self.object_symbol_safe(symbol)
            {
                continue;
            }
            let Some(mut keys) = self.object_keys(object, 0) else {
                continue;
            };
            keys.sort_unstable();
            keys.dedup();
            self.facts
                .spread_bindings
                .push(fallow_types::extract::ComponentSpreadBinding {
                    local: self.semantic.scoping().symbol_name(symbol).to_string(),
                    span_start: span,
                    keys,
                });
        }
    }

    fn resolve_type(&self, ty: &TSType<'_>) -> Option<Vec<PropShape>> {
        if let TSType::TSTypeReference(reference) = ty {
            if reference.type_arguments.is_some() {
                return None;
            }
            let TSTypeName::IdentifierReference(id) = &reference.type_name else {
                return None;
            };
            return self.types.get(&self.reference_span(id)?).cloned().flatten();
        }
        type_literal_shapes(ty)
    }

    fn collect_declarations(&mut self) {
        for node in self.semantic.nodes().iter() {
            match node.kind() {
                AstKind::Function(function) => {
                    if let Some(id) = &function.id
                        && is_component_name(id.name.as_str())
                        && function.body.is_some()
                        && crate::visitor::react::function_body_returns_jsx(
                            function.body.as_deref(),
                        )
                    {
                        self.function_props(
                            id.name.as_str(),
                            id.span.start,
                            &function.params,
                            false,
                            None,
                        );
                    }
                }
                AstKind::VariableDeclarator(decl) => {
                    self.astro_props(decl);
                    if let Some(Expression::CallExpression(call)) =
                        decl.init.as_ref().map(Expression::without_parentheses)
                    {
                        self.macro_props(&decl.id, decl.type_annotation.as_deref(), call);
                    }
                    let BindingPattern::BindingIdentifier(id) = &decl.id else {
                        continue;
                    };
                    if self
                        .semantic
                        .nodes()
                        .ancestor_kinds(node.id())
                        .take(2)
                        .any(|kind| matches!(kind, AstKind::ExportDeclaration(_)))
                        && matches!(self.semantic.nodes().parent_kind(node.id()), AstKind::VariableDeclaration(variable) if variable.kind == VariableDeclarationKind::Let)
                    {
                        self.legacy_svelte_prop(id, decl);
                    }
                    let Some(init) = &decl.init else { continue };
                    if is_component_name(id.name.as_str()) {
                        match init.without_parentheses() {
                            Expression::ArrowFunctionExpression(arrow)
                                if crate::visitor::react::arrow_returns_jsx(arrow) =>
                            {
                                self.function_props(
                                    id.name.as_str(),
                                    id.span.start,
                                    &arrow.params,
                                    false,
                                    None,
                                );
                            }
                            Expression::FunctionExpression(function)
                                if crate::visitor::react::function_body_returns_jsx(
                                    function.body.as_deref(),
                                ) =>
                            {
                                self.function_props(
                                    id.name.as_str(),
                                    id.span.start,
                                    &function.params,
                                    false,
                                    None,
                                );
                            }
                            Expression::CallExpression(call) => self.wrapper_props(id, call),
                            _ => {}
                        }
                    }
                }
                AstKind::ObjectExpression(object) => {
                    let supported = matches!(
                        self.semantic.nodes().parent_kind(node.id()),
                        AstKind::ExportDefaultDeclaration(_)
                    ) || matches!(self.semantic.nodes().parent_kind(node.id()), AstKind::CallExpression(call) if self.imported_api(&call.callee, "vue", &["defineComponent"]));
                    if supported {
                        self.vue_options_props(object);
                    }
                }
                AstKind::Class(class) => self.class_props(class),
                AstKind::TaggedTemplateExpression(tagged) => self.tagged_template_callers(tagged),
                AstKind::CallExpression(call) => {
                    self.dynamic_dom_call(call);
                    if matches!(
                        self.semantic.nodes().parent_kind(node.id()),
                        AstKind::ExpressionStatement(_)
                    ) {
                        self.macro_props_empty(call);
                    }
                }
                _ => {}
            }
        }
        // `interface Props` is the declaration contract of an Astro component.
        // It is retained provisionally and assigned to Astro only by the SFC path.
    }

    fn legacy_svelte_prop(&mut self, id: &BindingIdentifier<'_>, decl: &VariableDeclarator<'_>) {
        self.facts.declarations.push(ComponentPropDeclaration {
            component: String::new(),
            component_span: 0,
            name: id.name.to_string(),
            local: id.name.to_string(),
            aliases: Vec::new(),
            span_start: id.span.start,
            optional: decl.init.is_some(),
            has_default: decl.init.is_some(),
            is_used: self.binding_read(id),
            framework: ComponentFramework::Svelte,
            incomplete: false,
        });
    }

    fn astro_props(&mut self, decl: &VariableDeclarator<'_>) {
        let Some(Expression::StaticMemberExpression(member)) = &decl.init else {
            return;
        };
        if member.property.name != "props"
            || !matches!(&member.object, Expression::Identifier(id) if id.name == "Astro")
        {
            return;
        }
        let scoping = self.semantic.scoping();
        let shapes = scoping
            .symbol_ids()
            .find(|symbol| {
                scoping.symbol_name(*symbol) == "Props"
                    && scoping.symbol_scope_id(*symbol) == scoping.root_scope_id()
            })
            .and_then(|symbol| self.types.get(&scoping.symbol_span(symbol).start))
            .cloned()
            .flatten();
        let Some(shapes) = shapes else { return };
        let mut locals = FxHashMap::default();
        let mut defaults = FxHashSet::default();
        let mut incomplete;
        if let BindingPattern::ObjectPattern(object) = &decl.id {
            incomplete = object.rest.is_some();
            for property in &object.properties {
                let Some(name) = static_key(&property.key, property.computed) else {
                    incomplete = true;
                    continue;
                };
                let (local, default) = binding_with_default(&property.value);
                if let Some(local) = local {
                    locals.insert(name.clone(), local);
                }
                if default {
                    defaults.insert(name);
                }
            }
        } else {
            incomplete = true;
        }
        for shape in shapes {
            self.facts.declarations.push(ComponentPropDeclaration {
                component: String::new(),
                component_span: 0,
                name: shape.name.clone(),
                local: locals
                    .get(&shape.name)
                    .map_or_else(|| shape.name.clone(), |local| local.name.to_string()),
                aliases: Vec::new(),
                span_start: shape.span_start,
                optional: shape.optional,
                has_default: defaults.contains(&shape.name),
                is_used: locals
                    .get(&shape.name)
                    .is_some_and(|local| self.binding_read(local)),
                framework: ComponentFramework::Astro,
                incomplete,
            });
        }
    }

    fn wrapper_props(&mut self, id: &BindingIdentifier<'_>, call: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &call.callee else {
            return;
        };
        let ComponentReference::Import { local, .. } = self.binding_reference(callee, 0) else {
            return;
        };
        let Some(import) = self
            .imports
            .iter()
            .find(|import| import.local_name == local)
        else {
            return;
        };
        let fallow_types::extract::ImportedName::Named(imported) = &import.imported_name else {
            return;
        };
        let recognized = (matches!(import.source.as_str(), "react" | "preact/compat")
            && matches!(imported.as_str(), "memo" | "forwardRef"))
            || ((import.source.starts_with("@builder.io/qwik")
                || import.source.starts_with("@qwik.dev/"))
                && imported == "component$");
        let Some(first) = call.arguments.first().and_then(Argument::as_expression) else {
            return;
        };
        match first.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => self.function_props(
                id.name.as_str(),
                id.span.start,
                &arrow.params,
                !recognized,
                call.type_arguments
                    .as_ref()
                    .and_then(|args| args.params.get(usize::from(imported == "forwardRef"))),
            ),
            Expression::FunctionExpression(function) => self.function_props(
                id.name.as_str(),
                id.span.start,
                &function.params,
                !recognized,
                call.type_arguments
                    .as_ref()
                    .and_then(|args| args.params.get(usize::from(imported == "forwardRef"))),
            ),
            _ => {}
        }
        self.inspected_references.insert(callee.span.start);
    }

    fn function_props(
        &mut self,
        name: &str,
        component_span: u32,
        params: &FormalParameters<'_>,
        wrapper_incomplete: bool,
        wrapper_type: Option<&TSType<'_>>,
    ) {
        let framework = self.framework;
        let Some(param) = params.items.first() else {
            return;
        };
        let declared_type = param
            .type_annotation
            .as_ref()
            .map(|annotation| &annotation.type_annotation)
            .or(wrapper_type);
        let shape = declared_type.and_then(|ty| self.resolve_type(ty));
        let typed_unknown = declared_type.is_some() && shape.is_none();
        let typed = shape.is_some();
        let mut props = shape.unwrap_or_default();
        let mut defaults = FxHashSet::default();
        let mut locals = FxHashMap::default();
        let mut incomplete = wrapper_incomplete || typed_unknown || params.rest.is_some();
        match &param.pattern {
            BindingPattern::ObjectPattern(object) => {
                incomplete |= object.rest.is_some();
                for property in &object.properties {
                    let Some(public) = static_key(&property.key, property.computed) else {
                        incomplete = true;
                        continue;
                    };
                    let (binding, has_default) = binding_with_default(&property.value);
                    if has_default {
                        defaults.insert(public.clone());
                    }
                    if let Some(binding) = binding {
                        locals.insert(public.clone(), binding);
                    }
                    if !props.iter().any(|prop| prop.name == public) {
                        props.push(PropShape {
                            name: public,
                            span_start: property.span.start,
                            optional: has_default,
                            has_default,
                        });
                    }
                }
            }
            BindingPattern::BindingIdentifier(binding) => {
                for prop in &props {
                    locals.insert(prop.name.clone(), binding);
                }
                incomplete |= self.binding_used_whole(binding);
            }
            _ => {
                incomplete = true;
            }
        }
        self.components.insert(component_span);
        for prop in props {
            let used = locals
                .get(&prop.name)
                .is_some_and(|binding| match &param.pattern {
                    BindingPattern::BindingIdentifier(_) => {
                        self.binding_member_used(binding, &prop.name)
                    }
                    _ => self.binding_read(binding),
                });
            self.facts.declarations.push(ComponentPropDeclaration {
                component: name.to_string(),
                component_span,
                name: prop.name.clone(),
                local: locals
                    .get(&prop.name)
                    .map_or_else(|| prop.name.clone(), |local| local.name.to_string()),
                aliases: Vec::new(),
                span_start: prop.span_start,
                optional: prop.optional || (!typed && defaults.contains(&prop.name)),
                has_default: prop.has_default || defaults.contains(&prop.name),
                is_used: used,
                framework,
                incomplete,
            });
        }
    }

    fn binding_read(&self, binding: &BindingIdentifier<'_>) -> bool {
        binding.symbol_id.get().is_some_and(|symbol| {
            self.semantic
                .scoping()
                .get_resolved_references(symbol)
                .any(|reference| reference.is_read() && reference.is_value())
        })
    }

    fn binding_member_used(&self, binding: &BindingIdentifier<'_>, name: &str) -> bool {
        binding.symbol_id.get().is_some_and(|symbol| {
            self.semantic
                .scoping()
                .get_resolved_references(symbol)
                .any(|reference| {
                    let nodes = self.semantic.nodes();
                    let AstKind::StaticMemberExpression(member) =
                        nodes.parent_kind(reference.node_id())
                    else {
                        return false;
                    };
                    if member.property.name != name {
                        return false;
                    }
                    self.member_is_read(nodes.parent_id(reference.node_id()), member.span)
                })
        })
    }

    fn member_is_read(&self, node: oxc_semantic::NodeId, span: Span) -> bool {
        match self.semantic.nodes().parent_kind(node) {
            AstKind::AssignmentExpression(assignment)
                if assignment.operator.is_assign() && assignment.left.span() == span =>
            {
                false
            }
            AstKind::UnaryExpression(unary) if unary.operator.as_str() == "delete" => false,
            _ => true,
        }
    }

    fn binding_used_whole(&self, binding: &BindingIdentifier<'_>) -> bool {
        binding.symbol_id.get().is_some_and(|symbol| {
            self.semantic
                .scoping()
                .get_resolved_references(symbol)
                .any(|reference| {
                    reference.is_value()
                        && !matches!(
                            self.semantic.nodes().parent_kind(reference.node_id()),
                            AstKind::StaticMemberExpression(_)
                        )
                })
        })
    }

    fn macro_props_empty(&mut self, call: &CallExpression<'_>) {
        if call_name(call) == Some("defineProps") {
            self.add_macro_shapes(call, ComponentFramework::Vue, None, None, false);
        }
    }

    fn macro_props(
        &mut self,
        pattern: &BindingPattern<'_>,
        annotation: Option<&TSTypeAnnotation<'_>>,
        call: &CallExpression<'_>,
    ) {
        match call_name(call) {
            Some("defineProps") => self.add_macro_shapes(
                call,
                ComponentFramework::Vue,
                Some(pattern),
                annotation,
                false,
            ),
            Some("withDefaults") => {
                if let Some(Expression::CallExpression(inner)) =
                    call.arguments.first().and_then(Argument::as_expression)
                    && call_name(inner) == Some("defineProps")
                {
                    let start = self.facts.declarations.len();
                    self.add_macro_shapes(
                        inner,
                        ComponentFramework::Vue,
                        Some(pattern),
                        annotation,
                        false,
                    );
                    if let Some(Expression::ObjectExpression(defaults)) =
                        call.arguments.get(1).and_then(Argument::as_expression)
                    {
                        let keys = self.object_keys(defaults, 0);
                        for prop in &mut self.facts.declarations[start..] {
                            if keys.as_ref().is_some_and(|keys| keys.contains(&prop.name)) {
                                prop.has_default = true;
                            }
                        }
                    } else {
                        for prop in &mut self.facts.declarations[start..] {
                            prop.incomplete = true;
                        }
                    }
                }
            }
            Some("$props") => self.add_macro_shapes(
                call,
                ComponentFramework::Svelte,
                Some(pattern),
                annotation,
                false,
            ),
            _ => {}
        }
    }

    fn add_macro_shapes(
        &mut self,
        call: &CallExpression<'_>,
        framework: ComponentFramework,
        pattern: Option<&BindingPattern<'_>>,
        annotation: Option<&TSTypeAnnotation<'_>>,
        incomplete: bool,
    ) {
        let ty = call
            .type_arguments
            .as_ref()
            .and_then(|args| args.params.first())
            .or_else(|| annotation.map(|annotation| &annotation.type_annotation));
        let mut shapes = ty.and_then(|ty| self.resolve_type(ty)).unwrap_or_default();
        let mut unresolved = ty.is_some() && shapes.is_empty();
        if framework == ComponentFramework::Vue
            && ty.is_none()
            && let Some(Expression::ObjectExpression(object)) =
                call.arguments.first().and_then(Argument::as_expression)
        {
            shapes.extend(runtime_vue_shapes(object, &mut unresolved));
        }
        let mut defaults = FxHashSet::default();
        let mut locals = FxHashMap::default();
        let mut rest = false;
        if let Some(BindingPattern::ObjectPattern(object)) = pattern {
            rest = object.rest.is_some();
            for property in &object.properties {
                let Some(name) = static_key(&property.key, property.computed) else {
                    unresolved = true;
                    continue;
                };
                let (local, has_default) = binding_with_default(&property.value);
                if framework == ComponentFramework::Svelte
                    && matches!(&property.value, BindingPattern::AssignmentPattern(assignment) if matches!(&assignment.right, Expression::CallExpression(call) if call_name(call) == Some("$bindable")))
                {
                    unresolved = true;
                }
                if has_default {
                    defaults.insert(name.clone());
                }
                if let Some(local) = local {
                    locals.insert(name.clone(), local);
                }
                if framework == ComponentFramework::Svelte
                    && !shapes.iter().any(|shape| shape.name == name)
                {
                    shapes.push(PropShape {
                        name,
                        span_start: property.span.start,
                        optional: has_default,
                        has_default,
                    });
                }
            }
        }
        for shape in shapes {
            let used = locals
                .get(&shape.name)
                .is_some_and(|local| self.binding_read(local))
                || match pattern {
                    Some(BindingPattern::BindingIdentifier(local)) => {
                        self.binding_member_used(local, &shape.name)
                    }
                    _ => false,
                };
            self.facts.declarations.push(ComponentPropDeclaration {
                component: String::new(),
                component_span: 0,
                name: shape.name.clone(),
                local: locals.get(&shape.name).map_or_else(
                    || match pattern {
                        Some(BindingPattern::BindingIdentifier(local)) => local.name.to_string(),
                        _ => shape.name.clone(),
                    },
                    |local| local.name.to_string(),
                ),
                aliases: Vec::new(),
                span_start: shape.span_start,
                optional: shape.optional,
                has_default: shape.has_default || defaults.contains(&shape.name),
                is_used: used,
                framework,
                incomplete: incomplete || rest || unresolved,
            });
        }
    }

    fn collect_callers(&mut self) {
        for node in self.semantic.nodes().iter() {
            let AstKind::JSXElement(element) = node.kind() else {
                continue;
            };
            let opening = &element.opening_element;
            let target = match &opening.name {
                JSXElementName::IdentifierReference(id) if is_component_name(id.name.as_str()) => {
                    self.inspected_references.insert(id.span.start);
                    self.binding_reference(id, 0)
                }
                JSXElementName::Identifier(id) if id.name.contains('-') => {
                    ComponentReference::Selector(id.name.to_ascii_lowercase())
                }
                JSXElementName::MemberExpression(member) => {
                    if let JSXMemberExpressionObject::IdentifierReference(id) = &member.object {
                        if let ComponentReference::Import { local, span_start } =
                            self.binding_reference(id, 0)
                            && self.imports.iter().any(|import| {
                                import.local_name == local
                                    && matches!(
                                        import.imported_name,
                                        fallow_types::extract::ImportedName::Namespace
                                    )
                            })
                        {
                            self.inspected_references.insert(id.span.start);
                            ComponentReference::NamespaceMember {
                                local,
                                span_start,
                                member: member.property.name.to_string(),
                            }
                        } else {
                            self.facts.escapes.push(self.binding_reference(id, 0));
                            ComponentReference::Unknown
                        }
                    } else {
                        self.facts.incomplete_frameworks.push(self.framework);
                        ComponentReference::Unknown
                    }
                }
                _ => continue,
            };
            if let Some(closing) = &element.closing_element {
                match &closing.name {
                    JSXElementName::IdentifierReference(id) => {
                        self.inspected_references.insert(id.span.start);
                    }
                    JSXElementName::MemberExpression(member) => {
                        if let JSXMemberExpressionObject::IdentifierReference(id) = &member.object {
                            self.inspected_references.insert(id.span.start);
                        }
                    }
                    _ => {}
                }
            }
            let mut supplied = Vec::new();
            let mut unknown_props = false;
            for attribute in &opening.attributes {
                match attribute {
                    JSXAttributeItem::Attribute(attribute) => match &attribute.name {
                        JSXAttributeName::Identifier(id) => supplied.push(id.name.to_string()),
                        JSXAttributeName::NamespacedName(_) => unknown_props = true,
                    },
                    JSXAttributeItem::SpreadAttribute(spread) => {
                        if let Some(keys) = self.expression_keys(&spread.argument, 0) {
                            supplied.extend(keys);
                        } else {
                            unknown_props = true;
                        }
                    }
                }
            }
            if element.children.iter().any(|child| match child {
                JSXChild::Text(text) => !text.value.trim().is_empty(),
                JSXChild::ExpressionContainer(container) => {
                    !matches!(container.expression, JSXExpression::EmptyExpression(_))
                }
                _ => true,
            }) {
                supplied.push("children".to_string());
            }
            supplied.sort_unstable();
            supplied.dedup();
            self.facts.invocations.push(ComponentInvocation {
                target,
                span_start: element.span.start,
                supplied,
                supplied_properties: Vec::new(),
                unknown_props,
                framework: self.framework,
            });
        }
    }

    fn expression_keys(&self, expression: &Expression<'_>, depth: usize) -> Option<Vec<String>> {
        if depth > self.objects.len() + 16 {
            return None;
        }
        match expression.without_parentheses() {
            Expression::ObjectExpression(object) => self.object_keys(object, depth + 1),
            Expression::Identifier(id) => {
                let span = self.reference_span(id)?;
                let object = self.objects.get(&span)?;
                if !self.object_binding_safe(id) {
                    return None;
                }
                self.object_keys(object, depth + 1)
            }
            Expression::TSAsExpression(cast) => self.expression_keys(&cast.expression, depth + 1),
            Expression::TSSatisfiesExpression(cast) => {
                self.expression_keys(&cast.expression, depth + 1)
            }
            _ => None,
        }
    }

    fn object_keys(&self, object: &ObjectExpression<'_>, depth: usize) -> Option<Vec<String>> {
        let mut keys = Vec::new();
        for property in &object.properties {
            match property {
                ObjectPropertyKind::SpreadProperty(spread) => {
                    keys.extend(self.expression_keys(&spread.argument, depth + 1)?);
                }
                ObjectPropertyKind::ObjectProperty(property) => {
                    if property.kind != PropertyKind::Init || property.method {
                        return None;
                    }
                    keys.push(static_key(&property.key, property.computed)?);
                }
            }
        }
        Some(keys)
    }

    fn object_binding_safe(&self, id: &IdentifierReference<'_>) -> bool {
        let Some(reference_id) = id.reference_id.get() else {
            return false;
        };
        let Some(symbol) = self
            .semantic
            .scoping()
            .get_reference(reference_id)
            .symbol_id()
        else {
            return false;
        };
        self.object_symbol_safe(symbol)
    }

    fn object_symbol_safe(&self, symbol: oxc_semantic::SymbolId) -> bool {
        let declaration = self.semantic.scoping().symbol_declaration(symbol);
        if self
            .semantic
            .nodes()
            .ancestor_kinds(declaration)
            .any(|parent| {
                matches!(
                    parent,
                    AstKind::ExportDeclaration(_)
                        | AstKind::ExportNamedDeclaration(_)
                        | AstKind::ExportDefaultDeclaration(_)
                )
            })
        {
            return false;
        }
        self.semantic
            .scoping()
            .get_resolved_references(symbol)
            .all(|reference| {
                if !reference.is_value() {
                    return true;
                }
                match self.semantic.nodes().parent_kind(reference.node_id()) {
                    AstKind::JSXSpreadAttribute(_) => true,
                    AstKind::SpreadElement(_) => matches!(
                        self.semantic
                            .nodes()
                            .parent_kind(self.semantic.nodes().parent_id(reference.node_id())),
                        AstKind::ObjectExpression(_)
                    ),
                    _ => false,
                }
            })
    }

    fn collect_escapes(&mut self) {
        for node in self.semantic.nodes().iter() {
            if matches!(node.kind(), AstKind::StaticMemberExpression(member) if matches!(member.property.name.as_str(), "innerHTML" | "outerHTML"))
            {
                self.facts
                    .incomplete_frameworks
                    .push(ComponentFramework::Lit);
            }
            let AstKind::IdentifierReference(id) = node.kind() else {
                continue;
            };
            let Some(reference_id) = id.reference_id.get() else {
                continue;
            };
            let reference = self.semantic.scoping().get_reference(reference_id);
            if !reference.is_value() || self.inspected_references.contains(&id.span.start) {
                continue;
            }
            let Some(span) = self.reference_span(id) else {
                continue;
            };
            let target = self.binding_reference(id, 0);
            let ComponentReference::Local { span_start, .. } = &target else {
                if matches!(target, ComponentReference::Import { .. })
                    && !self.static_angular_registry(node.id())
                    && !self
                        .safe_reference_parent(self.semantic.nodes().parent_kind(node.id()), span)
                {
                    self.facts.escapes.push(target);
                }
                continue;
            };
            if self.components.contains(span_start)
                && !self.static_angular_registry(node.id())
                && !self.safe_reference_parent(self.semantic.nodes().parent_kind(node.id()), span)
            {
                self.facts.escapes.push(target);
            }
        }
        self.facts
            .escapes
            .sort_by_key(|reference| format!("{reference:?}"));
        self.facts.escapes.dedup();
        self.facts
            .incomplete_frameworks
            .sort_by_key(|framework| *framework as u8);
        self.facts.incomplete_frameworks.dedup();
    }

    fn collect_exports(&mut self, exports: &[crate::ExportInfo]) {
        let scoping = self.semantic.scoping();
        for export in exports {
            if export.is_type_only {
                continue;
            }
            let export_name = export.name.to_string();
            if export_name == "default" && export.local_name.is_none() {
                let target = self.semantic.nodes().iter().find_map(|node| {
                    let AstKind::ExportDefaultDeclaration(declaration) = node.kind() else {
                        return None;
                    };
                    let ExportDefaultDeclarationKind::Identifier(id) = &declaration.declaration
                    else {
                        return None;
                    };
                    Some(self.binding_reference(id, 0))
                });
                if let Some(target) =
                    target.filter(|target| !matches!(target, ComponentReference::Unknown))
                {
                    self.facts
                        .exports
                        .push(fallow_types::extract::ComponentExportBinding {
                            export_name,
                            export_span: export.span.start,
                            target,
                        });
                    continue;
                }
            }
            let local = export.local_name.as_deref().unwrap_or(&export_name);
            let Some(symbol) = scoping.symbol_ids().find(|symbol| {
                scoping.symbol_scope_id(*symbol) == scoping.root_scope_id()
                    && scoping.symbol_name(*symbol) == local
            }) else {
                continue;
            };
            let span_start = scoping.symbol_span(symbol).start;
            let target = if let Some(alias) = self.aliases.get(&span_start) {
                self.binding_reference(alias, 0)
            } else if self.imports.iter().any(|import| import.local_name == local) {
                ComponentReference::Import {
                    local: local.to_string(),
                    span_start,
                }
            } else if self.components.contains(&span_start) {
                ComponentReference::Local {
                    name: local.to_string(),
                    span_start,
                }
            } else {
                continue;
            };
            self.facts
                .exports
                .push(fallow_types::extract::ComponentExportBinding {
                    export_name,
                    export_span: export.span.start,
                    target,
                });
        }
    }

    fn static_angular_registry(&self, node: oxc_semantic::NodeId) -> bool {
        let ancestors = self.semantic.nodes().ancestor_kinds(node);
        let imports = ancestors.clone().any(|kind| matches!(kind, AstKind::ObjectProperty(property) if static_key(&property.key, property.computed).as_deref() == Some("imports")));
        imports && ancestors.clone().any(|kind| matches!(kind, AstKind::CallExpression(call) if self.imported_api(&call.callee, "@angular/core", &["Component"])) )
    }

    fn dynamic_dom_call(&mut self, call: &CallExpression<'_>) {
        if self.imported_api(
            &call.callee,
            "lit/directives/unsafe-html.js",
            &["unsafeHTML"],
        ) || self.imported_api(&call.callee, "lit/directives/unsafe-svg.js", &["unsafeSVG"])
        {
            self.facts
                .incomplete_frameworks
                .push(ComponentFramework::Lit);
            return;
        }
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return;
        };
        if !matches!(
            member.property.name.as_str(),
            "querySelector"
                | "querySelectorAll"
                | "getElementById"
                | "getElementsByTagName"
                | "createElement"
                | "setAttribute"
                | "removeAttribute"
        ) {
            return;
        }
        if matches!(
            member.property.name.as_str(),
            "createElement" | "querySelector" | "querySelectorAll" | "getElementsByTagName"
        ) && let Some(Expression::StringLiteral(tag)) =
            call.arguments.first().and_then(Argument::as_expression)
            && tag.value.contains('-')
            && tag
                .value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            self.facts
                .escapes
                .push(ComponentReference::Selector(tag.value.to_string()));
            return;
        }
        self.facts
            .incomplete_frameworks
            .push(ComponentFramework::Lit);
    }

    fn safe_reference_parent(&self, parent: AstKind<'_>, _span: u32) -> bool {
        match parent {
            AstKind::ExportSpecifier(_) | AstKind::ExportDefaultDeclaration(_) => true,
            AstKind::VariableDeclarator(decl) => {
                matches!(&decl.id, BindingPattern::BindingIdentifier(id) if self.aliases.contains_key(&id.span.start))
            }

            _ => false,
        }
    }
}

fn signature_shapes(signatures: &[TSSignature<'_>]) -> Option<Vec<PropShape>> {
    signatures
        .iter()
        .map(|signature| {
            let TSSignature::TSPropertySignature(property) = signature else {
                return None;
            };
            Some(PropShape {
                name: static_key(&property.key, property.computed)?,
                span_start: property.span.start,
                optional: property.optional,
                has_default: false,
            })
        })
        .collect()
}

fn type_literal_shapes(ty: &TSType<'_>) -> Option<Vec<PropShape>> {
    let TSType::TSTypeLiteral(literal) = ty else {
        return None;
    };
    signature_shapes(&literal.members)
}

fn static_key(key: &PropertyKey<'_>, computed: bool) -> Option<String> {
    if computed {
        return None;
    }
    match key {
        PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        PropertyKey::StringLiteral(value) => Some(value.value.to_string()),
        _ => None,
    }
}

fn binding_with_default<'a, 'b>(
    pattern: &'b BindingPattern<'a>,
) -> (Option<&'b BindingIdentifier<'a>>, bool) {
    match pattern {
        BindingPattern::BindingIdentifier(binding) => (Some(binding), false),
        BindingPattern::AssignmentPattern(assignment) => match &assignment.left {
            BindingPattern::BindingIdentifier(binding) => (Some(binding), true),
            _ => (None, true),
        },
        _ => (None, false),
    }
}

fn is_component_name(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
}

fn call_name<'b>(call: &'b CallExpression<'_>) -> Option<&'b str> {
    match &call.callee {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

/// Fold original-source offsets and declaration usage into SFC contracts.
pub fn merge(
    target: &mut Option<Box<ComponentContractFacts>>,
    mut facts: ComponentContractFacts,
    offset: u32,
    framework: Option<ComponentFramework>,
) {
    for declaration in &mut facts.declarations {
        declaration.span_start += offset;
        if !declaration.component.is_empty() {
            declaration.component_span += offset;
        }
        if let Some(framework) = framework {
            declaration.framework = framework;
        }
    }
    for invocation in &mut facts.invocations {
        invocation.span_start += offset;
        remap_reference(&mut invocation.target, offset);
    }
    for escape in &mut facts.escapes {
        remap_reference(escape, offset);
    }
    let target = target.get_or_insert_with(Default::default);
    for export in &mut facts.exports {
        export.export_span += offset;
        remap_reference(&mut export.target, offset);
    }
    target.exports.extend(facts.exports);
    for alias in &mut facts.aliases {
        remap_reference(&mut alias.target, offset);
    }
    target.aliases.extend(facts.aliases);
    for binding in &mut facts.spread_bindings {
        binding.span_start += offset;
    }
    for owner in &mut facts.template_owners {
        owner.component_span += offset;
        owner.start += offset;
        owner.end += offset;
    }
    target.template_owners.extend(facts.template_owners);
    for template in &mut facts.external_templates {
        template.component_span += offset;
    }
    target.external_templates.extend(facts.external_templates);
    target.spread_bindings.extend(facts.spread_bindings);
    target.declarations.extend(facts.declarations);
    target.invocations.extend(facts.invocations);
    target.escapes.extend(facts.escapes);
    target
        .incomplete_frameworks
        .extend(facts.incomplete_frameworks);
}

fn remap_reference(reference: &mut ComponentReference, offset: u32) {
    match reference {
        ComponentReference::Local { span_start, .. }
        | ComponentReference::Import { span_start, .. }
        | ComponentReference::NamespaceMember { span_start, .. } => *span_start += offset,
        _ => {}
    }
}

fn runtime_vue_shapes(object: &ObjectExpression<'_>, unresolved: &mut bool) -> Vec<PropShape> {
    let mut shapes = Vec::new();
    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            *unresolved = true;
            continue;
        };
        let Some(name) = static_key(&property.key, property.computed) else {
            *unresolved = true;
            continue;
        };
        let mut required = false;
        let mut has_default = false;
        if let Expression::ObjectExpression(options) = &property.value {
            for option in &options.properties {
                let ObjectPropertyKind::ObjectProperty(option) = option else {
                    *unresolved = true;
                    continue;
                };
                match static_key(&option.key, option.computed).as_deref() {
                    Some("required") => match &option.value {
                        Expression::BooleanLiteral(value) => required = value.value,
                        _ => *unresolved = true,
                    },
                    Some("default") => has_default = true,
                    None => *unresolved = true,
                    _ => {}
                }
            }
        }
        shapes.push(PropShape {
            name,
            span_start: property.span.start,
            optional: !required,
            has_default,
        });
    }
    shapes
}

/// Bind a single-file component's implicit default export to its own contract.
pub fn attach_sfc_default_export(module: &mut crate::ModuleInfo) {
    let Some(facts) = &mut module.component_contracts else {
        return;
    };
    if !facts
        .declarations
        .iter()
        .any(|prop| prop.component.is_empty())
        || facts
            .exports
            .iter()
            .any(|export| export.export_name == "default")
    {
        return;
    }
    facts
        .exports
        .push(fallow_types::extract::ComponentExportBinding {
            export_name: "default".to_string(),
            export_span: module
                .exports
                .iter()
                .find(|export| export.name.matches_str("default"))
                .map_or(0, |export| export.span.start),
            target: ComponentReference::Local {
                name: String::new(),
                span_start: 0,
            },
        });
}
