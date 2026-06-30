//! Framework DI and Angular render-entry capture.

#[allow(clippy::wildcard_imports, reason = "many DI visitor AST types used")]
use oxc_ast::ast::*;

use fallow_types::extract::{DiFramework, DiKeySite, DiRole};

use super::{
    ModuleInfoExtractor, angular_inject_is_optional, angular_param_inject_token,
    angular_param_is_optional, object_has_any_key, then_callback_member_class,
};

impl ModuleInfoExtractor {
    /// Classify a call as a framework DI provide / inject site, gating each
    /// named-callee form on its import provenance. The app-level
    /// `*.provide(KEY, value)` member form is FN-preferring (no provenance gate),
    /// since a captured provide can only suppress a finding, never create one.
    fn classify_di_call_site(&self, expr: &CallExpression<'_>) -> Option<(DiFramework, DiRole)> {
        match &expr.callee {
            Expression::Identifier(callee) => {
                let name = callee.name.as_str();
                if self.nested_scope_shadows(name) {
                    None
                } else if name == "provide"
                    && (self.is_named_import_from(name, "vue", "provide")
                        || self.is_named_import_from(name, "@vue/runtime-core", "provide"))
                {
                    Some((DiFramework::Vue, DiRole::Provide))
                } else if name == "inject"
                    && (self.is_named_import_from(name, "vue", "inject")
                        || self.is_named_import_from(name, "@vue/runtime-core", "inject"))
                {
                    Some((DiFramework::Vue, DiRole::Inject))
                } else if name == "setContext"
                    && self.is_named_import_from(name, "svelte", "setContext")
                {
                    Some((DiFramework::Svelte, DiRole::Provide))
                } else if name == "getContext"
                    && self.is_named_import_from(name, "svelte", "getContext")
                {
                    Some((DiFramework::Svelte, DiRole::Inject))
                } else if name == "inject"
                    && self.is_named_import_from(name, "@angular/core", "inject")
                {
                    Some((DiFramework::Angular, DiRole::Inject))
                } else {
                    None
                }
            }
            Expression::StaticMemberExpression(member)
                if member.property.name == "provide" && expr.arguments.len() == 2 =>
            {
                Some((DiFramework::Vue, DiRole::Provide))
            }
            _ => None,
        }
    }

    pub(super) fn record_di_key_site(&mut self, expr: &CallExpression<'_>) {
        let Some((framework, role)) = self.classify_di_call_site(expr) else {
            return;
        };

        // Angular `inject(TOKEN, { optional: true })`: an optional inject is
        // designed to be unprovided (it returns null), so it is never a dead
        // link. Drop the site entirely. Other frameworks have no such 2nd-arg
        // shape, so this check is Angular-only.
        if framework == DiFramework::Angular
            && role == DiRole::Inject
            && angular_inject_is_optional(expr)
        {
            return;
        }

        let Some(first) = expr.arguments.first() else {
            return;
        };
        match first {
            Argument::Identifier(ident) if !self.nested_scope_shadows(ident.name.as_str()) => {
                self.di_key_sites.push(DiKeySite {
                    key_local: ident.name.to_string(),
                    role,
                    framework,
                    span_start: expr.span.start,
                });
            }
            Argument::StringLiteral(_) => {}
            Argument::TemplateLiteral(t) if t.expressions.is_empty() => {}
            _ => {
                if role == DiRole::Provide {
                    self.has_dynamic_provide = true;
                }
            }
        }
    }

    /// Record an Angular `@Inject(TOKEN)` constructor-parameter decorator as a
    /// `(Angular, Inject)` `DiKeySite` keyed on the TOKEN identifier. Abstains
    /// when the same parameter also carries an `@Optional()` decorator.
    pub(super) fn record_angular_param_inject(&mut self, param: &FormalParameter<'_>) {
        if param.decorators.is_empty() {
            return;
        }
        let is_named_import = |local: &str, source: &str, imported: &str| {
            self.is_named_import_from(local, source, imported)
        };
        if angular_param_is_optional(param, &is_named_import) {
            return;
        }
        let token = param
            .decorators
            .iter()
            .find_map(|decorator| angular_param_inject_token(decorator, &is_named_import));
        let Some(token) = token else {
            return;
        };
        self.di_key_sites.push(DiKeySite {
            key_local: token.to_string(),
            role: DiRole::Inject,
            framework: DiFramework::Angular,
            span_start: param.span.start,
        });
    }

    /// Record Angular provider wiring as `(Angular, Provide)` DI sites from an
    /// object literal. Dynamic provider shapes set the project-wide provide
    /// abstain because they may supply any token.
    pub(super) fn record_angular_provider_object(&mut self, obj: &ObjectExpression<'_>) {
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                continue;
            };
            if p.key.static_name().as_deref() == Some("providers")
                && let Expression::ArrayExpression(arr) = &p.value
                && arr
                    .elements
                    .iter()
                    .any(|elem| matches!(elem, ArrayExpressionElement::SpreadElement(_)))
            {
                self.has_dynamic_provide = true;
            }
        }

        if !object_has_any_key(obj, &["useClass", "useValue", "useFactory", "useExisting"]) {
            return;
        }
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                continue;
            };
            if p.key.static_name().as_deref() != Some("provide") {
                continue;
            }
            match &p.value {
                Expression::Identifier(ident)
                    if !self.nested_scope_shadows(ident.name.as_str()) =>
                {
                    self.di_key_sites.push(DiKeySite {
                        key_local: ident.name.to_string(),
                        role: DiRole::Provide,
                        framework: DiFramework::Angular,
                        span_start: p.span.start,
                    });
                }
                _ => {
                    self.has_dynamic_provide = true;
                }
            }
        }
    }

    /// Flag `importProvidersFrom(...)` / `makeEnvironmentProviders(...)` calls
    /// as a project-wide provide abstain: both build an opaque provider bundle
    /// that can supply any token.
    pub(super) fn record_angular_dynamic_providers(&mut self, expr: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &expr.callee else {
            return;
        };
        let name = callee.name.as_str();
        if (name == "importProvidersFrom"
            && self.is_named_import_from(name, "@angular/core", "importProvidersFrom"))
            || (name == "makeEnvironmentProviders"
                && self.is_named_import_from(name, "@angular/core", "makeEnvironmentProviders"))
        {
            self.has_dynamic_provide = true;
        }
    }

    /// Drop `di_key_sites` whose key is a module-scope const bound to a string
    /// literal (string identity, abstain). Run at finalize so a const declared
    /// after the inject/provide call is still resolved.
    pub(in crate::visitor) fn finalize_di_key_sites(&mut self) {
        if self.string_keyed_di_consts.is_empty() {
            return;
        }
        let sites = std::mem::take(&mut self.di_key_sites);
        self.di_key_sites = sites
            .into_iter()
            .filter(|site| !self.string_keyed_di_consts.contains(&site.key_local))
            .collect();
    }

    /// Capture Angular route / bootstrap component class references from an
    /// object property. These are render-equivalent entry points the Angular
    /// `unrendered-component` detector abstains on.
    pub(super) fn record_angular_entry_component_refs(&mut self, prop: &ObjectProperty<'_>) {
        let Some(key) = prop.key.static_name() else {
            return;
        };
        match key.as_ref() {
            "component" => {
                if let Expression::Identifier(ident) = &prop.value {
                    self.angular_entry_component_refs
                        .push(ident.name.to_string());
                }
            }
            "loadComponent" => {
                if let Some(name) = then_callback_member_class(&prop.value) {
                    self.angular_entry_component_refs.push(name);
                }
            }
            "bootstrap" => {
                if let Expression::ArrayExpression(arr) = &prop.value {
                    for elem in &arr.elements {
                        if let ArrayExpressionElement::Identifier(ident) = elem {
                            self.angular_entry_component_refs
                                .push(ident.name.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Flag a dynamic Angular component render (`*.createComponent(...)` or a
    /// bare `createComponent(...)`). This drives the Angular
    /// `unrendered-component` detector's project-wide abstain.
    pub(super) fn record_angular_dynamic_component_render(&mut self, expr: &CallExpression<'_>) {
        let is_create_component = match &expr.callee {
            Expression::StaticMemberExpression(member) => {
                member.property.name.as_str() == "createComponent"
            }
            Expression::Identifier(ident) => ident.name.as_str() == "createComponent",
            _ => false,
        };
        if is_create_component {
            self.has_dynamic_component_render = true;
        }
    }

    /// Capture the bootstrapped component class from `bootstrapApplication(Foo,
    /// ...)` (the standalone Angular bootstrap entry).
    pub(super) fn record_angular_bootstrap_call(&mut self, expr: &CallExpression<'_>) {
        let Expression::Identifier(callee) = &expr.callee else {
            return;
        };
        if callee.name.as_str() != "bootstrapApplication" {
            return;
        }
        if let Some(Argument::Identifier(ident)) = expr.arguments.first() {
            self.angular_entry_component_refs
                .push(ident.name.to_string());
        }
    }
}
