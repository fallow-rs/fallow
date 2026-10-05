//! Decorator, signal, reactive property and typed Glimmer argument contracts.
use super::*;

impl ContractCollector<'_, '_> {
    pub(super) fn vue_options_props(&mut self, object: &ObjectExpression<'_>) {
        let runtime = object.properties.iter().find_map(|property| {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return None;
            };
            if static_key(&property.key, property.computed).as_deref() != Some("props") {
                return None;
            }
            let Expression::ObjectExpression(props) = &property.value else {
                return None;
            };
            Some(props)
        });
        let Some(runtime) = runtime else { return };
        let has_unknown_options = object.properties.iter().any(|property| !matches!(property, ObjectPropertyKind::ObjectProperty(property) if !property.computed && property.kind == PropertyKind::Init))
            || runtime.properties.iter().any(|property| !matches!(property, ObjectPropertyKind::ObjectProperty(property) if !property.computed && property.kind == PropertyKind::Init));
        for property in &runtime.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let Some(name) = static_key(&property.key, property.computed) else {
                continue;
            };
            let mut required = false;
            let mut has_default = false;
            let mut incomplete = has_unknown_options;
            if let Expression::ObjectExpression(options) = &property.value {
                for option in &options.properties {
                    let ObjectPropertyKind::ObjectProperty(option) = option else {
                        incomplete = true;
                        continue;
                    };
                    match static_key(&option.key, option.computed).as_deref() {
                        Some("required") => {
                            if let Expression::BooleanLiteral(value) = &option.value {
                                required = value.value;
                            } else {
                                incomplete = true;
                            }
                        }
                        Some("default") => has_default = true,
                        None => incomplete = true,
                        _ => {}
                    }
                }
            }
            self.facts.declarations.push(ComponentPropDeclaration {
                component: String::new(), component_span: 0, name: name.clone(), local: name.clone(), aliases: Vec::new(), span_start: property.span.start,
                optional: !required, has_default, is_used: self.semantic.nodes().iter().any(|node| matches!(node.kind(), AstKind::StaticMemberExpression(member) if member.property.name == name.as_str() && matches!(member.object, Expression::ThisExpression(_)) && member.span.start >= object.span.start && member.span.end <= object.span.end && self.member_is_read(node.id(), member.span))),
                framework: ComponentFramework::Vue, incomplete,
            });
        }
    }

    pub(super) fn class_props(&mut self, class: &Class<'_>) {
        let Some(id) = &class.id else { return };
        let angular = class.decorators.iter().find_map(|decorator| {
            let Expression::CallExpression(call) = &decorator.expression else {
                return None;
            };
            self.imported_api(&call.callee, "@angular/core", &["Component", "Directive"])
                .then_some(call)
        });
        if let Some(component) = angular {
            self.angular_class_props(class, id, component);
            return;
        }
        let Some(heritage) = &class.heritage else {
            return;
        };
        if self.imported_api(&heritage.expression, "@glimmer/component", &["default"]) {
            self.glimmer_class_props(class, id, heritage);
        } else if self.imported_api(
            &heritage.expression,
            "lit",
            &["LitElement", "ReactiveElement"],
        ) || self.imported_api(&heritage.expression, "lit-element", &["LitElement"])
        {
            self.lit_class_props(class, id);
        }
    }

    pub(super) fn imported_api(
        &self,
        expression: &Expression<'_>,
        source: &str,
        names: &[&str],
    ) -> bool {
        let (id, member) = match expression {
            Expression::Identifier(id) => (id, None),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(id) = &member.object else {
                    return false;
                };
                (id, Some(member.property.name.as_str()))
            }
            _ => return false,
        };
        let ComponentReference::Import { local, .. } = self.binding_reference(id, 0) else {
            return false;
        };
        self.imports.iter().any(|import| {
            import.local_name == local
                && (import.source == source
                    || (source == "lit" && import.source.starts_with("lit/")))
                && match &import.imported_name {
                    fallow_types::extract::ImportedName::Named(name) => {
                        member.is_none() && names.contains(&name.as_str())
                    }
                    fallow_types::extract::ImportedName::Default => {
                        member.is_none() && names.contains(&"default")
                    }
                    fallow_types::extract::ImportedName::Namespace => {
                        member.is_some_and(|member| names.contains(&member))
                    }
                    fallow_types::extract::ImportedName::SideEffect => false,
                }
        })
    }

    fn angular_class_props(
        &mut self,
        class: &Class<'_>,
        id: &BindingIdentifier<'_>,
        component: &CallExpression<'_>,
    ) {
        self.components.insert(id.span.start);
        self.angular_external_templates(id, component);
        let (template, incomplete) = angular_metadata(
            self.semantic.source_text(),
            class.heritage.is_some(),
            component,
        );
        if let Some((source, offset)) = &template {
            self.angular_template_callers(source, *offset);
        }
        for element in &class.body.body {
            let ClassElement::PropertyDefinition(property) = element else {
                continue;
            };
            let Some(local) = static_key(&property.key, property.computed) else {
                continue;
            };
            let mut name = local.clone();
            let mut required = false;
            let mut found = false;
            let mut model = false;
            let mut has_default = property.value.is_some();
            let mut input_incomplete = incomplete;
            for decorator in &property.decorators {
                let Expression::CallExpression(call) = &decorator.expression else {
                    continue;
                };
                if !self.imported_api(&call.callee, "@angular/core", &["Input"]) {
                    continue;
                }
                found = true;
                if let Some(argument) = call.arguments.first().and_then(Argument::as_expression) {
                    if let Expression::StringLiteral(alias) = argument {
                        name = alias.value.to_string();
                    } else {
                        read_input_options(
                            argument,
                            &mut name,
                            &mut required,
                            &mut input_incomplete,
                        );
                    }
                }
            }
            if let Some(Expression::CallExpression(call)) = &property.value {
                let (callee, explicitly_required) = match &call.callee {
                    Expression::StaticMemberExpression(member)
                        if member.property.name == "required" =>
                    {
                        (&member.object, true)
                    }
                    callee => (callee, false),
                };
                if self.imported_api(callee, "@angular/core", &["input", "model"]) {
                    found = true;
                    required = explicitly_required;
                    model = self.imported_api(callee, "@angular/core", &["model"]);
                    has_default = !explicitly_required && !call.arguments.is_empty();
                    let option = if explicitly_required {
                        call.arguments.first()
                    } else {
                        call.arguments.get(1)
                    };
                    if let Some(option) = option.and_then(Argument::as_expression) {
                        read_input_options(option, &mut name, &mut required, &mut input_incomplete);
                    }
                }
            }
            if !found {
                continue;
            }
            let used = self.class_member_read(class, &local)
                || template.as_ref().is_some_and(|(template, _)| {
                    crate::sfc_template::angular::collect_angular_template_refs(template)
                        .identifiers
                        .iter()
                        .any(|member| member == &local)
                });
            self.facts.declarations.push(ComponentPropDeclaration {
                component: id.name.to_string(),
                component_span: id.span.start,
                name,
                local,
                aliases: Vec::new(),
                span_start: property.key.span().start,
                optional: !required,
                has_default,
                is_used: used,
                framework: ComponentFramework::Angular,
                incomplete: input_incomplete || model,
            });
        }
    }

    fn angular_external_templates(
        &mut self,
        id: &BindingIdentifier<'_>,
        component: &CallExpression<'_>,
    ) {
        if let Some(Expression::ObjectExpression(metadata)) = component
            .arguments
            .first()
            .and_then(Argument::as_expression)
        {
            for property in &metadata.properties {
                let ObjectPropertyKind::ObjectProperty(property) = property else {
                    continue;
                };
                if static_key(&property.key, property.computed).as_deref() == Some("templateUrl")
                    && let Expression::StringLiteral(url) = &property.value
                {
                    self.facts.external_templates.push(
                        fallow_types::extract::ComponentExternalTemplate {
                            component_span: id.span.start,
                            source: url.value.to_string(),
                        },
                    );
                }
            }
        }
    }

    fn angular_template_callers(&mut self, source: &str, offset: u32) {
        let callers = crate::sfc_template::component_contracts::collect(
            source,
            self.imports,
            ComponentFramework::Angular,
            offset,
            &self.facts.spread_bindings,
            &self.facts.aliases,
        );
        self.facts.invocations.extend(callers.invocations);
        self.facts.escapes.extend(callers.escapes);
        self.facts
            .incomplete_frameworks
            .extend(callers.incomplete_frameworks);
    }

    fn class_member_read(&self, class: &Class<'_>, name: &str) -> bool {
        self.semantic.nodes().iter().any(|node| {
            let AstKind::StaticMemberExpression(member) = node.kind() else {
                return false;
            };
            if member.property.name != name
                || !matches!(&member.object, Expression::ThisExpression(_))
                || member.span.start < class.span.start
                || member.span.end > class.span.end
            {
                return false;
            }
            self.member_is_read(node.id(), member.span)
        })
    }

    fn lit_class_props(&mut self, class: &Class<'_>, id: &BindingIdentifier<'_>) {
        self.components.insert(id.span.start);
        for element in &class.body.body {
            let ClassElement::PropertyDefinition(property) = element else {
                continue;
            };
            if property.r#static {
                continue;
            }
            let Some(name) = static_key(&property.key, property.computed) else {
                continue;
            };
            if !property.decorators.iter().any(|decorator| {
                let Expression::CallExpression(call) = &decorator.expression else {
                    return false;
                };
                self.imported_api(&call.callee, "lit", &["property"])
            }) {
                continue;
            }
            let aliases = self.lit_attribute_names(property, &name);
            let incomplete = aliases.is_none();
            self.facts.declarations.push(ComponentPropDeclaration {
                component: id.name.to_string(),
                component_span: id.span.start,
                name: name.clone(),
                local: name.clone(),
                aliases: aliases.unwrap_or_default(),
                span_start: property.key.span().start,
                optional: property.optional || property.value.is_some(),
                has_default: property.value.is_some(),
                is_used: self.class_member_read(class, &name),
                framework: ComponentFramework::Lit,
                incomplete,
            });
        }
    }

    fn lit_attribute_names(
        &self,
        property: &PropertyDefinition<'_>,
        name: &str,
    ) -> Option<Vec<String>> {
        let mut aliases = Vec::new();
        let mut attribute_disabled = false;
        for decorator in &property.decorators {
            let Expression::CallExpression(call) = &decorator.expression else {
                continue;
            };
            if !self.imported_api(&call.callee, "lit", &["property"]) {
                continue;
            }
            let Some(argument) = call.arguments.first().and_then(Argument::as_expression) else {
                continue;
            };
            let Expression::ObjectExpression(options) = argument else {
                return None;
            };
            for option in &options.properties {
                let ObjectPropertyKind::ObjectProperty(option) = option else {
                    return None;
                };
                if option.computed {
                    return None;
                }
                if static_key(&option.key, option.computed).as_deref() == Some("attribute") {
                    match &option.value {
                        Expression::StringLiteral(alias) => {
                            aliases.push(alias.value.to_ascii_lowercase());
                        }
                        Expression::BooleanLiteral(value) if !value.value => {
                            attribute_disabled = true;
                        }
                        Expression::BooleanLiteral(_) => {}
                        _ => return None,
                    }
                }
            }
        }
        if aliases.is_empty() && !attribute_disabled {
            aliases.push(name.to_ascii_lowercase());
        }
        Some(aliases)
    }

    fn glimmer_class_props(
        &mut self,
        class: &Class<'_>,
        id: &BindingIdentifier<'_>,
        heritage: &ClassHeritage<'_>,
    ) {
        let Some(TSType::TSTypeReference(signature)) = heritage
            .type_arguments
            .as_ref()
            .and_then(|arguments| arguments.params.first())
        else {
            return;
        };
        let TSTypeName::IdentifierReference(signature) = &signature.type_name else {
            return;
        };
        let Some(span) = self.reference_span(signature) else {
            return;
        };
        let shapes = self
            .semantic
            .nodes()
            .iter()
            .find_map(|node| match node.kind() {
                AstKind::TSInterfaceDeclaration(declaration)
                    if declaration.id.span.start == span && declaration.extends.is_empty() =>
                {
                    argument_shapes(&declaration.body.body)
                }
                AstKind::TSTypeAliasDeclaration(declaration)
                    if declaration.id.span.start == span =>
                {
                    let TSType::TSTypeLiteral(literal) = &declaration.type_annotation else {
                        return None;
                    };
                    argument_shapes(&literal.members)
                }
                _ => None,
            });
        let Some(shapes) = shapes else { return };
        self.components.insert(id.span.start);
        self.facts
            .template_owners
            .push(fallow_types::extract::ComponentTemplateOwner {
                component_span: id.span.start,
                start: class.span.start,
                end: class.span.end,
            });
        let incomplete = self.semantic.nodes().iter().any(|node| {
            let AstKind::StaticMemberExpression(args) = node.kind() else { return false };
            if args.property.name != "args" || !matches!(&args.object, Expression::ThisExpression(_)) || args.span.start < class.span.start || args.span.end > class.span.end { return false; }
            !matches!(self.semantic.nodes().parent_kind(node.id()), AstKind::StaticMemberExpression(member) if member.object.span() == args.span && self.member_is_read(self.semantic.nodes().parent_id(node.id()), member.span))
        });
        for shape in shapes {
            let used = self.semantic.nodes().iter().any(|node| {
                matches!(node.kind(), AstKind::StaticMemberExpression(member) if member.property.name == shape.name.as_str() && matches!(&member.object, Expression::StaticMemberExpression(args) if args.property.name == "args" && matches!(&args.object, Expression::ThisExpression(_))) && member.span.start >= class.span.start && member.span.end <= class.span.end && self.member_is_read(node.id(), member.span))
            });
            self.facts.declarations.push(ComponentPropDeclaration {
                component: id.name.to_string(),
                component_span: id.span.start,
                name: shape.name.clone(),
                local: shape.name,
                aliases: Vec::new(),
                span_start: shape.span_start,
                optional: shape.optional,
                has_default: false,
                is_used: used,
                framework: ComponentFramework::Ember,
                incomplete,
            });
        }
    }

    pub(super) fn tagged_template_callers(&mut self, tagged: &TaggedTemplateExpression<'_>) {
        if self.imported_api(&tagged.tag, "lit/static-html.js", &["html", "svg"])
            && !tagged.quasi.expressions.is_empty()
        {
            self.facts
                .incomplete_frameworks
                .push(ComponentFramework::Lit);
        }
        if !self.imported_api(&tagged.tag, "lit", &["html"]) {
            return;
        }
        // Mask interpolation expressions without moving source byte anchors.
        let source = self.semantic.source_text();
        let start = tagged.quasi.span.start as usize + 1;
        let end = tagged.quasi.span.end.saturating_sub(1) as usize;
        let Some(body) = source.get(start..end) else {
            return;
        };
        let mut body = body.as_bytes().to_vec();
        for expression in &tagged.quasi.expressions {
            let span = expression.span();
            let from = (span.start as usize).saturating_sub(start + 2);
            let to = (span.end as usize + 1)
                .saturating_sub(start)
                .min(body.len());
            if from <= to {
                body[from..to].fill(b'x');
            }
        }
        let Ok(body) = std::str::from_utf8(&body) else {
            return;
        };
        let callers = crate::sfc_template::component_contracts::collect(
            body,
            self.imports,
            ComponentFramework::Lit,
            start as u32,
            &self.facts.spread_bindings,
            &self.facts.aliases,
        );
        self.facts.invocations.extend(callers.invocations);
        self.facts.escapes.extend(callers.escapes);
        self.facts
            .incomplete_frameworks
            .extend(callers.incomplete_frameworks);
    }
}

fn argument_shapes(signatures: &[TSSignature<'_>]) -> Option<Vec<PropShape>> {
    signatures.iter().find_map(|signature| {
        let TSSignature::TSPropertySignature(property) = signature else {
            return None;
        };
        if static_key(&property.key, property.computed).as_deref() != Some("Args") {
            return None;
        }
        type_literal_shapes(&property.type_annotation.as_ref()?.type_annotation)
    })
}

fn read_input_options(
    expression: &Expression<'_>,
    name: &mut String,
    required: &mut bool,
    incomplete: &mut bool,
) {
    let Expression::ObjectExpression(options) = expression else {
        *incomplete = true;
        return;
    };
    for option in &options.properties {
        let ObjectPropertyKind::ObjectProperty(option) = option else {
            *incomplete = true;
            continue;
        };
        match static_key(&option.key, option.computed).as_deref() {
            Some("alias") => {
                if let Expression::StringLiteral(alias) = &option.value {
                    *name = alias.value.to_string();
                } else {
                    *incomplete = true;
                }
            }
            Some("required") => {
                if let Expression::BooleanLiteral(value) = &option.value {
                    *required = value.value;
                } else {
                    *incomplete = true;
                }
            }
            Some("transform") => *incomplete = true,
            _ => {}
        }
    }
}

fn original_literal<'s>(source: &'s str, expression: &Expression<'_>) -> Option<(&'s str, u32)> {
    if !matches!(
        expression,
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_)
    ) {
        return None;
    }
    if matches!(expression, Expression::TemplateLiteral(template) if !template.expressions.is_empty())
    {
        return None;
    }
    let span = expression.span();
    let start = span.start as usize + 1;
    let end = span.end as usize - 1;
    let body = source.get(start..end)?;
    if body.contains('\\') {
        return None;
    }
    Some((body, start as u32))
}

fn angular_metadata<'s>(
    source: &'s str,
    inherited: bool,
    component: &CallExpression<'_>,
) -> (Option<(&'s str, u32)>, bool) {
    let mut template = None;
    let mut incomplete = inherited;
    if let Some(Expression::ObjectExpression(metadata)) = component
        .arguments
        .first()
        .and_then(Argument::as_expression)
    {
        for property in &metadata.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                incomplete = true;
                continue;
            };
            match static_key(&property.key, property.computed).as_deref() {
                Some("template") => {
                    template = original_literal(source, &property.value);
                }
                Some("hostDirectives") => incomplete = true,
                Some("selector") => {
                    incomplete |= !matches!(&property.value, Expression::StringLiteral(value) if value.value.split(',').all(|selector| selector.trim().bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')));
                }
                _ => {}
            }
        }
    } else {
        incomplete = true;
    }
    (template, incomplete)
}
