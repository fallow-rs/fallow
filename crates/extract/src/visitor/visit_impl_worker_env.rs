//! Receivers of the Cloudflare Worker `Env` that a function reads without a
//! typed parameter (issue #4548).
//!
//! - `import { env } from "cloudflare:workers"` binds the global `Env`.
//! - In a class that extends `DurableObject<E>`, `WorkerEntrypoint<E>` or
//!   `WorkflowEntrypoint<E, ...>` of `cloudflare:workers`, `this.env` has the
//!   type `E`.
//!
//! The structural collector tracks each receiver as a typed parameter, so a
//! Durable Object stub of a binding (`env.ROOM.getByName(name)`) credits the
//! method on the class of the binding.

use oxc_ast::ast::{Class, ClassElement, Expression, FormalParameters};
use rustc_hash::{FxHashMap, FxHashSet};

use super::ModuleInfoExtractor;
use super::helpers::extract_type_reference_name;
use crate::function_body::BodyRef;
use fallow_types::extract::ImportedName;

const CLOUDFLARE_WORKERS: &str = "cloudflare:workers";

/// The receiver name of `this.env` in a Cloudflare Worker class. The
/// structural collector reads `this.env.<path>` as the path of this receiver.
pub(super) const THIS_ENV_RECEIVER: &str = "this.env";

/// The type of `this.env` in each method body of a Worker class, keyed by the
/// body start.
pub(super) type WorkerEnvBodies = FxHashMap<u32, String>;

/// The type of the global `env` of `cloudflare:workers` (`Cloudflare.Env`),
/// which `wrangler types` also declares as the global `Env`.
const GLOBAL_ENV_TYPE: &str = "Env";

/// Base classes of `cloudflare:workers` whose first type argument is the type
/// of `this.env`.
const WORKER_BASE_CLASSES: &[&str] = &["DurableObject", "WorkerEntrypoint", "WorkflowEntrypoint"];

impl ModuleInfoExtractor {
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
                    .insert(body.span().start, env_type.clone());
            }
        }
    }

    /// The untyped `Env` receivers of a function body, as (receiver, type).
    /// A parameter of the same name hides the `env` import. Only an outermost
    /// function reads the import: the collector of that function also walks
    /// each nested function.
    pub(super) fn implicit_env_receivers(
        &self,
        params: &FormalParameters<'_>,
        body: BodyRef<'_, '_>,
    ) -> Vec<(String, String)> {
        let mut receivers = Vec::new();
        if let Some(env_type) = self.worker_env_bodies.get(&body.span().start) {
            receivers.push((THIS_ENV_RECEIVER.to_string(), env_type.clone()));
        }
        if self.function_depth > 0 {
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
