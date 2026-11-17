//! Credit for the handlers that the Cloudflare Workers runtime calls by name.
//!
//! A class that extends `DurableObject`, `WorkerEntrypoint` or
//! `WorkflowEntrypoint` from `cloudflare:workers` receives calls from the
//! runtime, not from project code. The handlers that the base class declares
//! are therefore used. The credit applies only when the base resolves through
//! an import of `cloudflare:workers` with the exact imported name, so a local
//! class or a package class with the same name gets no credit.

use rustc_hash::FxHashMap;

use super::ExportKey;
use crate::extract::ImportedName;
use crate::resolve::ResolvedModule;

const CLOUDFLARE_WORKERS_SOURCE: &str = "cloudflare:workers";

/// Optional handlers that `@cloudflare/workers-types` declares on the
/// `DurableObject` base class.
const DURABLE_OBJECT_HANDLERS: &[&str] = &[
    "alarm",
    "fetch",
    "connect",
    "webSocketMessage",
    "webSocketClose",
    "webSocketError",
];

/// Optional handlers that `@cloudflare/workers-types` declares on the
/// `WorkerEntrypoint` base class.
const WORKER_ENTRYPOINT_HANDLERS: &[&str] = &[
    "email",
    "fetch",
    "connect",
    "queue",
    "scheduled",
    "tail",
    "tailStream",
    "test",
    "trace",
];

/// The abstract method that `@cloudflare/workers-types` declares on the
/// `WorkflowEntrypoint` base class.
const WORKFLOW_ENTRYPOINT_HANDLERS: &[&str] = &["run"];

/// The handlers of a base class that `cloudflare:workers` exports, or `None`
/// for another export name.
fn runtime_handlers(imported_name: &str) -> Option<&'static [&'static str]> {
    match imported_name {
        "DurableObject" => Some(DURABLE_OBJECT_HANDLERS),
        "WorkerEntrypoint" => Some(WORKER_ENTRYPOINT_HANDLERS),
        "WorkflowEntrypoint" => Some(WORKFLOW_ENTRYPOINT_HANDLERS),
        _ => None,
    }
}

/// Map each exported class whose heritage chain reaches a Cloudflare runtime
/// base to the handlers of that base.
///
/// A direct subclass is found through the `super_class` local name and the
/// imports of its module. A local subclass of such a class inherits the same
/// handlers through `parent_to_children`.
pub(super) fn build_cloudflare_runtime_subclass_handlers(
    resolved_modules: &[ResolvedModule],
    parent_to_children: &FxHashMap<ExportKey, Vec<ExportKey>>,
) -> FxHashMap<ExportKey, &'static [&'static str]> {
    let mut handlers_by_key: FxHashMap<ExportKey, &'static [&'static str]> = FxHashMap::default();

    for resolved in resolved_modules {
        let base_by_local: FxHashMap<&str, &'static [&'static str]> = resolved
            .resolved_imports
            .iter()
            .filter(|import| import.info.source == CLOUDFLARE_WORKERS_SOURCE)
            .filter_map(|import| match &import.info.imported_name {
                ImportedName::Named(name) => runtime_handlers(name)
                    .map(|handlers| (import.info.local_name.as_str(), handlers)),
                _ => None,
            })
            .collect();
        if base_by_local.is_empty() {
            continue;
        }

        for export in resolved.exports.iter() {
            if let Some(handlers) = export
                .super_class
                .as_deref()
                .and_then(|super_local| base_by_local.get(super_local))
            {
                handlers_by_key.insert(
                    ExportKey::new(resolved.file_id, export.name.to_string()),
                    handlers,
                );
            }
        }
    }

    let mut stack: Vec<(ExportKey, &'static [&'static str])> = handlers_by_key
        .iter()
        .map(|(key, handlers)| (key.clone(), *handlers))
        .collect();
    while let Some((parent_key, handlers)) = stack.pop() {
        let Some(children) = parent_to_children.get(&parent_key) else {
            continue;
        };
        for child in children {
            if !handlers_by_key.contains_key(child) {
                handlers_by_key.insert(child.clone(), handlers);
                stack.push((child.clone(), handlers));
            }
        }
    }

    handlers_by_key
}

/// Whether the runtime calls `member_name` on the class of `export_key`.
pub(super) fn is_cloudflare_runtime_handler(
    member_name: &str,
    export_key: &ExportKey,
    handlers_by_key: &FxHashMap<ExportKey, &'static [&'static str]>,
) -> bool {
    handlers_by_key
        .get(export_key)
        .is_some_and(|handlers| handlers.contains(&member_name))
}
