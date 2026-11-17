//! Credit for the handlers that the Cloudflare Workers runtime calls by name.
//!
//! A class that extends `DurableObject`, `WorkerEntrypoint` or
//! `WorkflowEntrypoint` from `cloudflare:workers` receives calls from the
//! runtime, not from project code. The handlers that the base class declares
//! are therefore used. The credit applies only when the base resolves through
//! an import of `cloudflare:workers`: a named import with the exact base name,
//! or a namespace import (`cf.DurableObject`). A local class or a package
//! class with the same name gets no credit.
//!
//! The credit does not check the `static` modifier, because the extraction
//! records no such flag. A static method with a handler name also gets credit.

use rustc_hash::FxHashMap;

use super::ExportKey;
use crate::discover::FileId;
use crate::extract::{ImportedName, ModuleInfo};
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

/// The `cloudflare:workers` imports of one module.
#[derive(Default)]
struct WorkersImports<'a> {
    /// Named imports of a runtime base, by local name.
    base_by_local: FxHashMap<&'a str, &'static [&'static str]>,
    /// Local names of namespace imports.
    namespaces: Vec<&'a str>,
}

impl<'a> WorkersImports<'a> {
    fn new(resolved: &'a ResolvedModule) -> Self {
        let mut imports = Self::default();
        for import in &resolved.resolved_imports {
            if import.info.source != CLOUDFLARE_WORKERS_SOURCE {
                continue;
            }
            let local = import.info.local_name.as_str();
            match &import.info.imported_name {
                ImportedName::Named(name) => {
                    if let Some(handlers) = runtime_handlers(name) {
                        imports.base_by_local.insert(local, handlers);
                    }
                }
                ImportedName::Namespace => imports.namespaces.push(local),
                ImportedName::Default | ImportedName::SideEffect => {}
            }
        }
        imports
    }

    fn is_empty(&self) -> bool {
        self.base_by_local.is_empty() && self.namespaces.is_empty()
    }

    /// The handlers of the base that an `extends` expression names: a local
    /// name of a named import, or `<namespace>.<base>`.
    fn handlers_of(&self, super_class: &str) -> Option<&'static [&'static str]> {
        if let Some(handlers) = self.base_by_local.get(super_class) {
            return Some(handlers);
        }
        let (namespace, base) = super_class.split_once('.')?;
        if !self.namespaces.contains(&namespace) {
            return None;
        }
        runtime_handlers(base)
    }
}

/// Map each exported class whose heritage chain reaches a Cloudflare runtime
/// base to the handlers of that base.
///
/// A direct subclass is found through the `super_class` local name and the
/// imports of its module. A class that extends a base class that its module
/// declares but does not export is found through the first `extends` target
/// past such bases (`extends_root`), also through a generic base. The heritage
/// is keyed by the export name, so a default export and a renamed export
/// (`export { Inner as Outer }`) find it too. A local subclass of a
/// credited exported class inherits the same handlers through
/// `parent_to_children`.
pub(super) fn build_cloudflare_runtime_subclass_handlers(
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    parent_to_children: &FxHashMap<ExportKey, Vec<ExportKey>>,
) -> FxHashMap<ExportKey, &'static [&'static str]> {
    let mut handlers_by_key: FxHashMap<ExportKey, &'static [&'static str]> = FxHashMap::default();
    let module_by_id: FxHashMap<FileId, &ModuleInfo> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();

    for resolved in resolved_modules {
        let imports = WorkersImports::new(resolved);
        if imports.is_empty() {
            continue;
        }
        let extends_root_by_export: FxHashMap<&str, &str> = module_by_id
            .get(&resolved.file_id)
            .into_iter()
            .flat_map(|module| module.class_heritage.iter())
            .filter_map(|heritage| {
                Some((
                    heritage.export_name.as_str(),
                    heritage.extends_root.as_deref()?,
                ))
            })
            .collect();

        for export in resolved.exports.iter() {
            let Some(super_class) = export.super_class.as_deref() else {
                continue;
            };
            let export_name = export.name.to_string();
            let handlers = imports
                .handlers_of(super_class)
                .or_else(|| imports.handlers_of(extends_root_by_export.get(export_name.as_str())?));
            if let Some(handlers) = handlers {
                handlers_by_key.insert(ExportKey::new(resolved.file_id, export_name), handlers);
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
