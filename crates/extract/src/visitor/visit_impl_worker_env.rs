//! Receivers of the Cloudflare Worker `Env` that a function reads without a
//! typed parameter (issue #4548).
//!
//! - `import { env } from "cloudflare:workers"` binds the global `Env`.
//! - In a class that extends `DurableObject<E>`, `WorkerEntrypoint<E>` or
//!   `WorkflowEntrypoint<E, ...>` of `cloudflare:workers`, `this.env` has the
//!   type `E`.
//! - In an object of the type `ExportedHandler<E>` (through `satisfies`, `as`
//!   or a variable annotation), the untyped `env` parameter of a handler has
//!   the type `E`.
//!
//!   The match reads the type name `ExportedHandler`, the global type of
//!   `@cloudflare/workers-types`, and does not check for a local type of the
//!   same name. Only a handler that the object literal declares (a method, a
//!   function or an arrow function) gets the type. A shorthand property that
//!   names a separate function (`{ fetch }`) gets no type.
//!
//! The structural collector tracks each receiver as a typed parameter, so a
//! Durable Object stub of a binding (`env.ROOM.getByName(name)`) credits the
//! method on the class of the binding.

use oxc_ast::ast::{
    BindingPattern, Class, ClassElement, Expression, FormalParameters, ObjectPropertyKind, TSType,
    TSTypeName, VariableDeclarator,
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::ModuleInfoExtractor;
use super::helpers::extract_type_reference_name;
use crate::function_body::BodyRef;
use crate::js_text::JsText;
use fallow_types::extract::ImportedName;

const CLOUDFLARE_WORKERS: &str = "cloudflare:workers";

/// The receiver name of `this.env` in a Cloudflare Worker class. The
/// structural collector reads `this.env.<path>` as the path of this receiver.
pub(super) const THIS_ENV_RECEIVER: &str = "this.env";

/// The Worker `Env` state of one module.
#[derive(Default)]
pub(super) struct WorkerEnvBodies {
    /// The type of `this.env` in each method body of a Worker class, keyed by
    /// the body start.
    env_types: FxHashMap<u32, String>,
    /// The untyped `env` parameter of each handler of an `ExportedHandler<E>`
    /// object, as (parameter name, `E`), keyed by the handler body start.
    handler_env_params: FxHashMap<u32, (String, String)>,
    /// The module imports `cloudflare:workers`.
    imports_workers: bool,
    /// The number of imports that `imports_workers` has seen.
    seen_imports: usize,
}

/// The type of the global `env` of `cloudflare:workers` (`Cloudflare.Env`),
/// which `wrangler types` also declares as the global `Env`.
const GLOBAL_ENV_TYPE: &str = "Env";

/// Base classes of `cloudflare:workers` whose first type argument is the type
/// of `this.env`.
const WORKER_BASE_CLASSES: &[&str] = &["DurableObject", "WorkerEntrypoint", "WorkflowEntrypoint"];

/// The handlers that `ExportedHandler` of `@cloudflare/workers-types`
/// declares. Each handler receives the `Env` as its second parameter.
const EXPORTED_HANDLER_NAMES: &[&str] = &[
    "fetch",
    "connect",
    "tail",
    "trace",
    "tailStream",
    "scheduled",
    "test",
    "email",
    "queue",
];

/// The `E` of an `ExportedHandler<E, ...>` type.
fn exported_handler_env_type(ty: &TSType<'_>) -> Option<String> {
    let TSType::TSTypeReference(reference) = ty else {
        return None;
    };
    let TSTypeName::IdentifierReference(name) = &reference.type_name else {
        return None;
    };
    if name.name != "ExportedHandler" {
        return None;
    }
    extract_type_reference_name(reference.type_arguments.as_deref()?.params.first()?)
}

/// The parameters and body of a handler value: a method, a function
/// expression or an arrow function.
fn handler_function<'a, 'b>(
    value: &'b Expression<'a>,
) -> Option<(&'b FormalParameters<'a>, BodyRef<'b, 'a>)> {
    match value {
        Expression::FunctionExpression(function) => {
            Some((&function.params, BodyRef::Block(function.body.as_deref()?)))
        }
        Expression::ArrowFunctionExpression(arrow) => {
            Some((&arrow.params, BodyRef::arrow(&arrow.body)))
        }
        _ => None,
    }
}

impl ModuleInfoExtractor {
    /// The module imports `cloudflare:workers`. Each call reads only the
    /// imports after the previous call.
    fn imports_cloudflare_workers(&mut self) -> bool {
        let state = &mut self.worker_env_bodies;
        if !state.imports_workers {
            state.imports_workers = self.imports[state.seen_imports.min(self.imports.len())..]
                .iter()
                .any(|import| import.source == CLOUDFLARE_WORKERS);
        }
        state.seen_imports = self.imports.len();
        state.imports_workers
    }

    /// The name that `local` imports from `cloudflare:workers`.
    fn cloudflare_workers_import(&self, local: &str) -> Option<&str> {
        self.imports.iter().find_map(|import| {
            if import.source != CLOUDFLARE_WORKERS || import.local_name != local {
                return None;
            }
            match &import.imported_name {
                ImportedName::Named(name) => Some(name.as_str()),
                _ => None,
            }
        })
    }

    /// The type of `this.env` in `class`: the first type argument of a Worker
    /// base class of `cloudflare:workers`.
    fn worker_env_type(&self, class: &Class<'_>) -> Option<String> {
        let heritage = class.heritage.as_ref()?;
        let Expression::Identifier(base) = &heritage.expression else {
            return None;
        };
        let imported = self.cloudflare_workers_import(base.name.as_str())?;
        if !WORKER_BASE_CLASSES.contains(&imported) {
            return None;
        }
        let env_type = heritage.type_arguments.as_deref()?.params.first()?;
        extract_type_reference_name(env_type)
    }

    /// Records the body of each instance method and arrow field of a Worker
    /// class, with the type of `this.env` in it.
    pub(super) fn record_worker_env_bodies(&mut self, class: &Class<'_>) {
        let Some(env_type) = self.worker_env_type(class) else {
            return;
        };
        for element in &class.body.body {
            let body = match element {
                ClassElement::MethodDefinition(method) if !method.r#static => {
                    method.value.body.as_deref().map(BodyRef::Block)
                }
                ClassElement::PropertyDefinition(property) if !property.r#static => {
                    match &property.value {
                        Some(Expression::ArrowFunctionExpression(arrow)) => {
                            Some(BodyRef::arrow(&arrow.body))
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(body) = body {
                self.worker_env_bodies
                    .env_types
                    .insert(body.span().start, env_type.clone());
            }
        }
    }

    /// Records the handlers of `const handler: ExportedHandler<E> = { ... }`.
    pub(super) fn record_exported_handler_declarator(
        &mut self,
        declarator: &VariableDeclarator<'_>,
    ) {
        if let Some(annotation) = declarator.type_annotation.as_deref()
            && let Some(init) = &declarator.init
        {
            self.record_exported_handler_env(init, &annotation.type_annotation);
        }
    }

    /// Records the untyped `env` parameter of each handler in the object
    /// literal `value` when `ty` is `ExportedHandler<E>`.
    pub(super) fn record_exported_handler_env(&mut self, value: &Expression<'_>, ty: &TSType<'_>) {
        let Expression::ObjectExpression(object) = value.without_parentheses() else {
            return;
        };
        let Some(env_type) = exported_handler_env_type(ty) else {
            return;
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let Some(name) = property.key.static_name() else {
                continue;
            };
            if !EXPORTED_HANDLER_NAMES.contains(&&*name.text()) {
                continue;
            }
            let Some((params, body)) = handler_function(&property.value) else {
                continue;
            };
            let Some(param) = params.items.get(1) else {
                continue;
            };
            let BindingPattern::BindingIdentifier(id) = &param.pattern else {
                continue;
            };
            if param.type_annotation.is_some() {
                continue;
            }
            self.worker_env_bodies
                .handler_env_params
                .insert(body.span().start, (id.name.to_string(), env_type.clone()));
        }
    }

    /// The untyped `Env` receivers of a function body, as (receiver, type).
    /// A parameter of the same name hides the `env` import. Only an outermost
    /// function reads the import: the collector of that function also walks
    /// each nested function.
    pub(super) fn implicit_env_receivers(
        &mut self,
        params: &FormalParameters<'_>,
        body: BodyRef<'_, '_>,
    ) -> Vec<(String, String)> {
        let mut receivers = Vec::new();
        if let Some(env_type) = self.worker_env_bodies.env_types.get(&body.span().start) {
            receivers.push((THIS_ENV_RECEIVER.to_string(), env_type.clone()));
        }
        if let Some(param) = self
            .worker_env_bodies
            .handler_env_params
            .get(&body.span().start)
        {
            receivers.push(param.clone());
        }
        if self.function_depth > 0 || !self.imports_cloudflare_workers() {
            return receivers;
        }
        let param_names: FxHashSet<&str> = params
            .items
            .iter()
            .flat_map(|param| param.pattern.get_binding_identifiers())
            .map(|id| id.name.as_str())
            .collect();
        receivers.extend(
            self.imports
                .iter()
                .filter(|import| {
                    import.source == CLOUDFLARE_WORKERS
                        && matches!(&import.imported_name, ImportedName::Named(name) if name == "env")
                        && !import.local_name.is_empty()
                        && !param_names.contains(import.local_name.as_str())
                })
                .map(|import| (import.local_name.clone(), GLOBAL_ENV_TYPE.to_string())),
        );
        receivers
    }
}
