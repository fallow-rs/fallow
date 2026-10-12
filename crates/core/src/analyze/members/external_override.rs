//! Credit for `override` members of a base class outside the project.
//!
//! The `override` modifier tells that the base class declares the member.
//! When that base comes from a package or is a global constructor, the
//! package or the runtime calls the member, so it is used.
//!
//! The credit uses only syntactic, certain evidence: the `override` keyword
//! and a base whose root identifier resolves through an import to a target
//! outside the project, or names a global constructor that the module does
//! not shadow. A base that resolves to a project file gets no credit here,
//! because the inheritance pass tracks project contracts.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::MemberKind;

use super::ExportKey;
use crate::discover::FileId;
use crate::extract::ModuleInfo;
use crate::resolve::{ResolveResult, ResolvedModule};

/// Global constructors that a class can extend without an import. An
/// `override` member of a direct subclass is called by the runtime.
const GLOBAL_BASE_NAMES: &[&str] = &[
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "AggregateError",
    "Array",
    "Map",
    "Set",
    "WeakMap",
    "WeakSet",
    "Promise",
    "RegExp",
    "Date",
    "EventTarget",
    "Event",
    "CustomEvent",
    "HTMLElement",
    "ReadableStream",
    "WritableStream",
    "TransformStream",
    "Request",
    "Response",
    "Headers",
    "AbortController",
];

/// Add `override` members of external bases to `accessed_members`, before
/// the inheritance pass so subclasses that override them get the credit too.
pub(super) fn credit_external_override_members(
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let module_by_id: FxHashMap<FileId, &ModuleInfo> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    for resolved in resolved_modules {
        if !resolved.exports.iter().any(|export| {
            export
                .members
                .iter()
                .any(|member| member.override_base.is_some())
        }) {
            continue;
        }
        let names = ModuleNames::new(resolved, module_by_id.get(&resolved.file_id).copied());
        for export in resolved.exports.iter() {
            let credited: Vec<String> = export
                .members
                .iter()
                .filter(|member| {
                    matches!(
                        member.kind,
                        MemberKind::ClassMethod | MemberKind::ClassProperty
                    ) && member
                        .override_base
                        .as_deref()
                        .is_some_and(|base| names.is_external_base(base))
                })
                .map(|member| member.name.clone())
                .collect();
            if credited.is_empty() {
                continue;
            }
            accessed_members
                .entry(ExportKey::new(resolved.file_id, export.name.to_string()))
                .or_default()
                .extend(credited);
        }
    }
}

/// Whether an import target is code outside the project. An unresolved
/// specifier counts only with a scheme (`cloudflare:workers`), because a
/// broken relative import or path alias can name a project file.
fn is_external_target(target: &ResolveResult, source: &str) -> bool {
    match target {
        ResolveResult::NpmPackage(_)
        | ResolveResult::CommonJsNpmPackage(_)
        | ResolveResult::ExternalFile(_) => true,
        ResolveResult::Unresolvable(_) => has_url_scheme(source),
        _ => false,
    }
}

/// Whether a specifier starts with a URL scheme such as `cloudflare:` or
/// `node:`. A one-letter scheme is a Windows drive letter, not a scheme.
fn has_url_scheme(source: &str) -> bool {
    let Some((scheme, _)) = source.split_once(':') else {
        return false;
    };
    scheme.len() > 1
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Names of one module: import locals marked as outside the project or not,
/// and the names that the module itself declares.
struct ModuleNames<'a> {
    external_by_local: FxHashMap<&'a str, bool>,
    declared: FxHashSet<&'a str>,
}

impl<'a> ModuleNames<'a> {
    fn new(resolved: &'a ResolvedModule, module: Option<&'a ModuleInfo>) -> Self {
        let mut external_by_local: FxHashMap<&'a str, bool> = FxHashMap::default();
        for import in &resolved.resolved_imports {
            let external = is_external_target(&import.target, &import.info.source);
            external_by_local
                .entry(import.info.local_name.as_str())
                .and_modify(|all_external| *all_external &= external)
                .or_insert(external);
        }
        let declared = resolved
            .exports
            .iter()
            .filter_map(|export| export.local_name.as_deref())
            .chain(
                module
                    .into_iter()
                    .flat_map(|module| module.local_type_declarations.iter())
                    .map(|declaration| declaration.name.as_str()),
            )
            .collect();
        Self {
            external_by_local,
            declared,
        }
    }

    /// Whether the root of a class `extends` expression names code outside
    /// the project: an external import, or a global constructor that no
    /// import and no declaration of the module shadows.
    fn is_external_base(&self, root: &str) -> bool {
        match self.external_by_local.get(root) {
            Some(external) => *external,
            None => GLOBAL_BASE_NAMES.contains(&root) && !self.declared.contains(root),
        }
    }
}
