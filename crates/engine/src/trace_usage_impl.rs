//! Per-specifier usage of a dependency (`fallow trace --dependency`).
//!
//! The walk is syntactic. It reads the import bindings, call sites and
//! binding references that extraction keeps on each `ModuleInfo`, and it
//! follows one hop from a project wrapper to the files that import the
//! wrapper. Each use that it cannot resolve to a call gets a named reason, so
//! the counts never hide a gap.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fallow_types::discover::FileId;
use fallow_types::extract::{
    ExportName, ImportBindingReference, ImportBindingReferenceKind, ImportedName, ModuleInfo,
    byte_offset_to_line_col,
};
use fallow_types::trace_usage::{
    ClosureFile, ConsumerClosure, DependencyUsage, DependencyUsageQuery,
    DependencyUsageSchemaVersion, FileLevelUnresolved, SpecifierUnresolved, SpecifierUsage,
    UsageConfidence, UsageSite, UsageSiteKind, UsageSitePage, UsageWrapper, WrapperShape,
};
use rustc_hash::{FxHashMap, FxHashSet};

use super::trace_impl::relativize;
use crate::graph::ModuleGraph;

/// The name of a bare namespace use.
const NAMESPACE_SPECIFIER: &str = "*";
/// The name of a default import.
const DEFAULT_SPECIFIER: &str = "default";
/// The version prefix of a site cursor.
const CURSOR_PREFIX: &str = "v1.";
/// The separator between the fields of a decoded cursor.
const CURSOR_SEPARATOR: char = '\0';
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

/// Why a usage query has no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    /// The cursor does not decode, or it belongs to another query.
    InvalidCursor,
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCursor => {
                f.write_str("the cursor is not valid for this query; start again without --cursor")
            }
        }
    }
}

impl std::error::Error for UsageError {}

/// Compute the usage of `package_name` across `modules`.
///
/// `imported_by` is the root-relative importer list of the dependency trace.
/// It seeds the closure and finds the files that import the package through
/// a specifier that does not name it.
///
/// # Errors
///
/// Returns [`UsageError::InvalidCursor`] when the query carries a cursor that
/// does not decode or that belongs to another query.
pub fn dependency_usage(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    root: &Path,
    package_name: &str,
    imported_by: &[PathBuf],
    query: &DependencyUsageQuery,
) -> Result<DependencyUsage, UsageError> {
    let ctx = UsageContext::new(graph, modules, root, package_name);
    let mut scan = Scan::default();
    for module in modules {
        scan.scan_module(&ctx, module);
    }
    scan.follow_wrappers(&ctx);

    let selected = selected_specifiers(&query.specifiers);
    let mut unresolved = scan.file_level_unresolved();
    unresolved.unattributed_file = imported_by
        .iter()
        .filter(|path| {
            ctx.file_id(&path_key(path))
                .is_none_or(|id| !scan.attributed_files.contains(&id))
        })
        .count();
    let specifiers = scan.specifier_usages(selected.as_deref());
    let sites = match &query.sites {
        Some(request) => Some(site_page(
            &scan.sites,
            selected.as_deref(),
            package_name,
            request.limit,
            request.cursor.as_deref(),
        )?),
        None => None,
    };
    let closure = query.closure_depth.map(|depth| {
        let seeds = closure_seeds(&ctx, &scan, imported_by, selected.as_deref());
        consumer_closure(graph, root, &seeds, depth)
    });
    Ok(DependencyUsage {
        schema_version: DependencyUsageSchemaVersion::V1,
        confidence: UsageConfidence::Syntactic,
        specifiers,
        unresolved,
        sites,
        closure,
    })
}

/// The sorted, deduplicated specifier filter, or `None` for all names.
fn selected_specifiers(specifiers: &[String]) -> Option<Vec<String>> {
    if specifiers.is_empty() {
        return None;
    }
    let mut selected = specifiers.to_vec();
    selected.sort_unstable();
    selected.dedup();
    Some(selected)
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

struct UsageContext<'a> {
    graph: &'a ModuleGraph,
    root: &'a Path,
    package_name: &'a str,
    modules_by_id: FxHashMap<FileId, &'a ModuleInfo>,
    files_by_path: FxHashMap<String, FileId>,
}

impl<'a> UsageContext<'a> {
    fn new(
        graph: &'a ModuleGraph,
        modules: &'a [ModuleInfo],
        root: &'a Path,
        package_name: &'a str,
    ) -> Self {
        let modules_by_id = modules
            .iter()
            .map(|module| (module.file_id, module))
            .collect();
        let files_by_path = graph
            .modules
            .iter()
            .map(|node| (relativize(&node.path, root), node.file_id))
            .collect();
        Self {
            graph,
            root,
            package_name,
            modules_by_id,
            files_by_path,
        }
    }

    fn file_id(&self, path: &str) -> Option<FileId> {
        self.files_by_path.get(path).copied()
    }

    fn file_path(&self, id: FileId) -> Option<String> {
        self.graph
            .modules
            .get(id.0 as usize)
            .map(|node| relativize(&node.path, self.root))
    }

    /// Whether an import specifier names the package, after the same `npm:`
    /// normalization that the resolver uses.
    fn names_package(&self, source: &str) -> bool {
        if let Some(rest) = source.strip_prefix("npm:") {
            return fallow_graph::resolve::extract_package_name(
                &fallow_graph::resolve::normalize_npm_specifier(rest),
            ) == self.package_name;
        }
        source.starts_with(self.package_name)
            && fallow_graph::resolve::extract_package_name(source) == self.package_name
    }
}

/// A runtime import binding of the package in one file.
#[derive(Clone)]
enum BindingName {
    /// A named or default import: the imported name.
    Named(String),
    /// A namespace import: the name comes from the first member.
    Namespace,
}

impl BindingName {
    /// The specifier and the remaining member of a use with `member_path`.
    fn split(&self, member_path: &str) -> (String, Option<String>) {
        match self {
            Self::Named(name) => (name.clone(), non_empty(member_path)),
            Self::Namespace => match member_path.split_once('.') {
                Some((first, rest)) => (first.to_owned(), non_empty(rest)),
                None if member_path.is_empty() => (NAMESPACE_SPECIFIER.to_owned(), None),
                None => (member_path.to_owned(), None),
            },
        }
    }
}

fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

/// What one file holds for one specifier.
#[derive(Default)]
struct FileSpecifier {
    runtime_binding: bool,
    type_binding: bool,
}

/// A wrapper found in the scan, with the totals of its consumer calls.
struct Wrapper {
    file_id: FileId,
    file: String,
    export: String,
    specifier: String,
    shape: WrapperShape,
    line: u32,
    consumer_files: FxHashSet<FileId>,
    call_site_count: usize,
}

/// One site with the file that holds it.
struct RawSite {
    file_id: FileId,
    site: UsageSite,
}

#[derive(Default)]
struct Scan {
    sites: Vec<RawSite>,
    /// Per file and specifier: the bindings that count toward `file_count`.
    file_specifiers: FxHashMap<(FileId, String), FileSpecifier>,
    /// Files and specifiers with at least one site.
    sited: FxHashSet<(FileId, String)>,
    wrappers: Vec<Wrapper>,
    /// Files with an import, re-export, dynamic import or `require` that
    /// names the package.
    attributed_files: FxHashSet<FileId>,
}

/// The fixed inputs of one module scan.
struct ModuleScan<'m> {
    module: &'m ModuleInfo,
    file: String,
    /// Runtime bindings of the package by import index.
    bindings: FxHashMap<u32, BindingName>,
    /// Import index by local binding name, for the runtime bindings.
    locals: FxHashMap<&'m str, u32>,
    /// The index of the first site of this module in `Scan::sites`.
    first_site: usize,
}

impl ModuleScan<'_> {
    fn position(&self, offset: u32) -> (u32, u32) {
        byte_offset_to_line_col(&self.module.line_offsets, offset)
    }
}

/// Fields of a site that the caller chooses.
struct SiteSpec {
    offset: u32,
    specifier: Option<String>,
    local_name: Option<String>,
    member: Option<String>,
    kind: UsageSiteKind,
    via: Option<String>,
}

impl Scan {
    fn push_site(&mut self, scan: &ModuleScan<'_>, spec: SiteSpec) {
        let (line, col) = scan.position(spec.offset);
        if let Some(specifier) = &spec.specifier {
            self.sited.insert((scan.module.file_id, specifier.clone()));
        }
        self.sites.push(RawSite {
            file_id: scan.module.file_id,
            site: UsageSite {
                file: scan.file.clone(),
                line,
                col,
                specifier: spec.specifier,
                local_name: spec.local_name,
                member: spec.member,
                kind: spec.kind,
                via: spec.via,
            },
        });
    }

    fn file_level_site(&mut self, scan: &ModuleScan<'_>, offset: u32, kind: UsageSiteKind) {
        self.push_site(
            scan,
            SiteSpec {
                offset,
                specifier: None,
                local_name: None,
                member: None,
                kind,
                via: None,
            },
        );
    }

    fn note_binding(&mut self, file_id: FileId, specifier: &str, type_only: bool) {
        let entry = self
            .file_specifiers
            .entry((file_id, specifier.to_owned()))
            .or_default();
        if type_only {
            entry.type_binding = true;
        } else {
            entry.runtime_binding = true;
        }
    }

    fn scan_module(&mut self, ctx: &UsageContext<'_>, module: &ModuleInfo) {
        let Some(file) = ctx.file_path(module.file_id) else {
            return;
        };
        let mut scan = ModuleScan {
            module,
            file,
            bindings: FxHashMap::default(),
            locals: FxHashMap::default(),
            first_site: self.sites.len(),
        };
        self.scan_imports(ctx, &mut scan);
        self.scan_calls(&scan);
        self.scan_binding_references(&scan);
        self.scan_file_level_forms(ctx, &scan);
        self.close_namespace_bindings(&scan);
    }

    fn scan_imports(&mut self, ctx: &UsageContext<'_>, scan: &mut ModuleScan<'_>) {
        let file_id = scan.module.file_id;
        for (index, import) in scan.module.imports.iter().enumerate() {
            if !ctx.names_package(&import.source) {
                continue;
            }
            self.attributed_files.insert(file_id);
            let Ok(index) = u32::try_from(index) else {
                break;
            };
            let name = match &import.imported_name {
                ImportedName::SideEffect => {
                    self.file_level_site(scan, import.span.start, UsageSiteKind::SideEffectImport);
                    continue;
                }
                ImportedName::Named(name) => BindingName::Named(name.clone()),
                ImportedName::Default => BindingName::Named(DEFAULT_SPECIFIER.to_owned()),
                ImportedName::Namespace => BindingName::Namespace,
            };
            match &name {
                BindingName::Named(specifier) => {
                    let type_only = import.is_type_only
                        || is_type_position_only(scan.module, &import.local_name);
                    self.note_binding(file_id, specifier, type_only);
                }
                BindingName::Namespace if import.is_type_only => {
                    self.note_binding(file_id, NAMESPACE_SPECIFIER, true);
                }
                BindingName::Namespace => {}
            }
            if !import.is_type_only && !import.local_name.is_empty() {
                scan.bindings.insert(index, name);
                scan.locals.insert(import.local_name.as_str(), index);
            }
        }
    }

    /// The runtime binding of a local name.
    fn binding_of<'s>(scan: &'s ModuleScan<'_>, local_name: &str) -> Option<&'s BindingName> {
        scan.locals
            .get(local_name)
            .and_then(|index| scan.bindings.get(index))
    }

    fn scan_calls(&mut self, scan: &ModuleScan<'_>) {
        let wrapper_calls = wrapper_initializer_calls(scan);
        for call in scan.module.imported_call_sites.iter() {
            let Some(binding) = Self::binding_of(scan, &call.local_name) else {
                continue;
            };
            // The wrapper definition owns this offset (precedence rule).
            if wrapper_calls.contains(&call.span_start) {
                continue;
            }
            let (specifier, member) = binding.split(&call.member_path);
            self.note_namespace_use(scan, binding, &specifier);
            self.push_site(
                scan,
                SiteSpec {
                    offset: call.span_start,
                    specifier: Some(specifier),
                    local_name: Some(call.local_name.clone()),
                    member,
                    kind: UsageSiteKind::Call,
                    via: None,
                },
            );
        }
    }

    /// A namespace member use counts the file toward the member's
    /// `file_count`.
    fn note_namespace_use(&mut self, scan: &ModuleScan<'_>, binding: &BindingName, name: &str) {
        if matches!(binding, BindingName::Namespace) {
            self.note_binding(scan.module.file_id, name, false);
        }
    }

    fn scan_binding_references(&mut self, scan: &ModuleScan<'_>) {
        for reference in scan.module.import_binding_references.iter() {
            let Some(binding) = scan.bindings.get(&reference.import_index) else {
                continue;
            };
            let Some(import) = scan.module.imports.get(reference.import_index as usize) else {
                continue;
            };
            let (specifier, member) = binding.split(&reference.member_path);
            let wrapper = wrapper_shape(reference, member.as_deref())
                .and_then(|shape| Some((shape, wrapper_export(scan.module, reference)?)));
            let kind = match (reference.kind, &wrapper) {
                (_, Some(_)) => UsageSiteKind::WrapperDefinition,
                // A call site covers an initializer call that is no wrapper.
                (ImportBindingReferenceKind::InitializerCall, None) => continue,
                (ImportBindingReferenceKind::ValueAlias, None) => UsageSiteKind::ValueAlias,
                (ImportBindingReferenceKind::JsxElement, None) => UsageSiteKind::JsxElement,
                (ImportBindingReferenceKind::Other, None) => UsageSiteKind::NonCallReference,
            };
            if let Some((shape, export)) = wrapper {
                let (line, _) = scan.position(reference.span_start);
                self.wrappers.push(Wrapper {
                    file_id: scan.module.file_id,
                    file: scan.file.clone(),
                    export,
                    specifier: specifier.clone(),
                    shape,
                    line,
                    consumer_files: FxHashSet::default(),
                    call_site_count: 0,
                });
            }
            self.note_namespace_use(scan, binding, &specifier);
            self.push_site(
                scan,
                SiteSpec {
                    offset: reference.span_start,
                    specifier: Some(specifier),
                    local_name: Some(import.local_name.clone()),
                    member,
                    kind,
                    via: None,
                },
            );
        }
    }

    fn scan_file_level_forms(&mut self, ctx: &UsageContext<'_>, scan: &ModuleScan<'_>) {
        let file_id = scan.module.file_id;
        for re_export in &scan.module.re_exports {
            if !ctx.names_package(&re_export.source) {
                continue;
            }
            self.attributed_files.insert(file_id);
            if re_export.imported_name == NAMESPACE_SPECIFIER {
                self.file_level_site(scan, re_export.span.start, UsageSiteKind::StarReExport);
                continue;
            }
            self.note_binding(file_id, &re_export.imported_name, re_export.is_type_only);
            if re_export.is_type_only {
                continue;
            }
            self.push_site(
                scan,
                SiteSpec {
                    offset: re_export.span.start,
                    specifier: Some(re_export.imported_name.clone()),
                    local_name: Some(re_export.imported_name.clone()),
                    member: None,
                    kind: UsageSiteKind::ReExport,
                    via: None,
                },
            );
        }
        for dynamic in &scan.module.dynamic_imports {
            if dynamic.is_speculative || !ctx.names_package(&dynamic.source) {
                continue;
            }
            self.attributed_files.insert(file_id);
            self.file_level_site(scan, dynamic.span.start, UsageSiteKind::DynamicImport);
        }
        for require in &scan.module.require_calls {
            if !ctx.names_package(&require.source) {
                continue;
            }
            self.attributed_files.insert(file_id);
            self.file_level_site(scan, require.span.start, UsageSiteKind::Require);
        }
    }

    /// A runtime namespace binding with no member use is a bare binding of
    /// `*`, so it counts toward `binding_without_site`.
    fn close_namespace_bindings(&mut self, scan: &ModuleScan<'_>) {
        let file_id = scan.module.file_id;
        for (index, binding) in &scan.bindings {
            if !matches!(binding, BindingName::Namespace) {
                continue;
            }
            let Some(import) = scan.module.imports.get(*index as usize) else {
                continue;
            };
            let used = self
                .sites
                .get(scan.first_site..)
                .unwrap_or_default()
                .iter()
                .any(|raw| raw.site.local_name.as_deref() == Some(import.local_name.as_str()));
            if !used {
                self.note_binding(file_id, NAMESPACE_SPECIFIER, false);
            }
        }
    }

    /// Follow one hop from each wrapper to the files that import it.
    fn follow_wrappers(&mut self, ctx: &UsageContext<'_>) {
        let mut wrappers = std::mem::take(&mut self.wrappers);
        for wrapper in &mut wrappers {
            for (consumer_id, local_name, prefix) in wrapper_consumers(ctx.graph, wrapper) {
                let Some(consumer) = ctx.modules_by_id.get(&consumer_id) else {
                    continue;
                };
                let Some(file) = ctx.file_path(consumer_id) else {
                    continue;
                };
                let scan = ModuleScan {
                    module: consumer,
                    file,
                    bindings: FxHashMap::default(),
                    locals: FxHashMap::default(),
                    first_site: self.sites.len(),
                };
                self.scan_consumer(&scan, wrapper, &local_name, prefix.as_deref());
            }
        }
        self.wrappers = wrappers;
    }

    fn scan_consumer(
        &mut self,
        scan: &ModuleScan<'_>,
        wrapper: &mut Wrapper,
        local_name: &str,
        prefix: Option<&str>,
    ) {
        let via = format!("{}:{}", wrapper.file, wrapper.export);
        let indexes: FxHashSet<u32> = scan
            .module
            .imports
            .iter()
            .enumerate()
            .filter(|(_, import)| import.local_name == local_name && !import.is_type_only)
            .filter_map(|(index, _)| u32::try_from(index).ok())
            .collect();
        let nested_calls: FxHashSet<u32> = scan
            .module
            .import_binding_references
            .iter()
            .filter(|reference| {
                indexes.contains(&reference.import_index)
                    && reference.kind == ImportBindingReferenceKind::InitializerCall
                    && exports_declared_value(scan.module, reference)
            })
            .map(|reference| reference.span_start)
            .collect();
        for call in scan.module.imported_call_sites.iter() {
            if call.local_name != local_name || nested_calls.contains(&call.span_start) {
                continue;
            }
            let PrefixMatch::Member(member) = match_member_prefix(&call.member_path, prefix) else {
                continue;
            };
            wrapper.call_site_count += 1;
            wrapper.consumer_files.insert(scan.module.file_id);
            self.push_site(
                scan,
                SiteSpec {
                    offset: call.span_start,
                    specifier: Some(wrapper.specifier.clone()),
                    local_name: Some(local_name.to_owned()),
                    member,
                    kind: UsageSiteKind::Call,
                    via: Some(via.clone()),
                },
            );
        }
        for reference in scan.module.import_binding_references.iter() {
            if !indexes.contains(&reference.import_index) {
                continue;
            }
            let PrefixMatch::Member(member) = match_member_prefix(&reference.member_path, prefix)
            else {
                continue;
            };
            let exported = exports_declared_value(scan.module, reference);
            let kind = match reference.kind {
                ImportBindingReferenceKind::InitializerCall if exported => {
                    UsageSiteKind::NestedWrapper
                }
                ImportBindingReferenceKind::InitializerCall => continue,
                ImportBindingReferenceKind::ValueAlias if exported => UsageSiteKind::NestedWrapper,
                ImportBindingReferenceKind::ValueAlias => UsageSiteKind::ValueAlias,
                ImportBindingReferenceKind::JsxElement => UsageSiteKind::JsxElement,
                ImportBindingReferenceKind::Other => UsageSiteKind::NonCallReference,
            };
            self.push_site(
                scan,
                SiteSpec {
                    offset: reference.span_start,
                    specifier: Some(wrapper.specifier.clone()),
                    local_name: Some(local_name.to_owned()),
                    member,
                    kind,
                    via: Some(via.clone()),
                },
            );
        }
    }

    fn file_level_unresolved(&self) -> FileLevelUnresolved {
        let mut unresolved = FileLevelUnresolved::default();
        for raw in &self.sites {
            match raw.site.kind {
                UsageSiteKind::DynamicImport => unresolved.dynamic_import += 1,
                UsageSiteKind::Require => unresolved.require += 1,
                UsageSiteKind::SideEffectImport => unresolved.side_effect_import += 1,
                UsageSiteKind::StarReExport => unresolved.star_re_export += 1,
                _ => {}
            }
        }
        unresolved
    }

    fn specifier_usages(&self, selected: Option<&[String]>) -> Vec<SpecifierUsage> {
        let mut usages: FxHashMap<String, SpecifierUsage> = FxHashMap::default();
        let entry = |usages: &mut FxHashMap<String, SpecifierUsage>, name: &str| {
            if usages.contains_key(name) {
                return;
            }
            usages.insert(name.to_owned(), empty_specifier_usage(name));
        };
        if let Some(selected) = selected {
            for name in selected {
                entry(&mut usages, name);
            }
        }
        let wanted = |name: &str| selected.is_none_or(|names| names.iter().any(|n| n == name));
        for ((file_id, name), state) in &self.file_specifiers {
            if !wanted(name) {
                continue;
            }
            entry(&mut usages, name);
            let Some(usage) = usages.get_mut(name) else {
                continue;
            };
            usage.file_count += 1;
            if state.type_binding && !state.runtime_binding {
                usage.type_only_file_count += 1;
            }
            if state.runtime_binding && !self.sited.contains(&(*file_id, name.clone())) {
                usage.unresolved.binding_without_site += 1;
            }
        }
        for raw in &self.sites {
            let Some(name) = raw.site.specifier.as_deref() else {
                continue;
            };
            if !wanted(name) {
                continue;
            }
            entry(&mut usages, name);
            if let Some(usage) = usages.get_mut(name) {
                count_site(usage, &raw.site);
            }
        }
        for wrapper in &self.wrappers {
            if let Some(usage) = usages.get_mut(&wrapper.specifier) {
                usage.wrappers.push(UsageWrapper {
                    file: wrapper.file.clone(),
                    export: wrapper.export.clone(),
                    shape: wrapper.shape,
                    line: wrapper.line,
                    consumer_file_count: wrapper.consumer_files.len(),
                    call_site_count: wrapper.call_site_count,
                });
            }
        }
        let mut usages: Vec<SpecifierUsage> = usages.into_values().collect();
        for usage in &mut usages {
            usage
                .wrappers
                .sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.export.cmp(&b.export)));
        }
        usages.sort_by(|a, b| a.name.cmp(&b.name));
        usages
    }
}

/// Whether a value import binding is read only in type positions, for example
/// `import { PayloadAction } from "pkg"` used as `action: PayloadAction<T>`.
/// TypeScript erases such a binding, so it counts as a type-only binding. A
/// binding with no reference at all (a template use in Vue, Svelte or Astro)
/// stays a runtime binding.
fn is_type_position_only(module: &ModuleInfo, local_name: &str) -> bool {
    let has = |names: &[String]| names.iter().any(|name| name == local_name);
    has(&module.type_referenced_import_bindings) && !has(&module.value_referenced_import_bindings)
}

fn empty_specifier_usage(name: &str) -> SpecifierUsage {
    SpecifierUsage {
        name: name.to_owned(),
        file_count: 0,
        type_only_file_count: 0,
        call_site_count: 0,
        unresolved: SpecifierUnresolved::default(),
        wrappers: Vec::new(),
    }
}

fn count_site(usage: &mut SpecifierUsage, site: &UsageSite) {
    let unresolved = &mut usage.unresolved;
    match site.kind {
        UsageSiteKind::Call if site.via.is_none() => usage.call_site_count += 1,
        UsageSiteKind::ValueAlias => unresolved.value_alias += 1,
        UsageSiteKind::NonCallReference => unresolved.non_call_reference += 1,
        UsageSiteKind::JsxElement => unresolved.jsx_element += 1,
        UsageSiteKind::ReExport => unresolved.re_export += 1,
        UsageSiteKind::NestedWrapper => unresolved.nested_wrapper += 1,
        _ => {}
    }
}

/// The start offsets of the initializer calls that define a wrapper.
fn wrapper_initializer_calls(scan: &ModuleScan<'_>) -> FxHashSet<u32> {
    scan.module
        .import_binding_references
        .iter()
        .filter(|reference| {
            reference.kind == ImportBindingReferenceKind::InitializerCall
                && scan.bindings.contains_key(&reference.import_index)
                && wrapper_export(scan.module, reference).is_some()
        })
        .map(|reference| reference.span_start)
        .collect()
}

/// The wrapper shape that a reference can define. An alias wrapper is the
/// bare binding or one static namespace member.
const fn wrapper_shape(
    reference: &ImportBindingReference,
    member: Option<&str>,
) -> Option<WrapperShape> {
    match reference.kind {
        ImportBindingReferenceKind::InitializerCall => Some(WrapperShape::Call),
        ImportBindingReferenceKind::ValueAlias if member.is_none() => Some(WrapperShape::Alias),
        _ => None,
    }
}

/// The exported name of the top-level declarator that `reference`
/// initializes, when the module exports it as a value.
fn wrapper_export(module: &ModuleInfo, reference: &ImportBindingReference) -> Option<String> {
    let declared = reference.declared_name.as_deref()?;
    module
        .exports
        .iter()
        .find(|export| {
            !export.is_type_only
                && match &export.local_name {
                    Some(local) => local == declared,
                    None => export.name.matches_str(declared),
                }
        })
        .map(|export| match &export.name {
            ExportName::Named(name) => name.clone(),
            ExportName::Default => DEFAULT_SPECIFIER.to_owned(),
        })
}

fn exports_declared_value(module: &ModuleInfo, reference: &ImportBindingReference) -> bool {
    wrapper_export(module, reference).is_some()
}

/// How a member path relates to a required namespace prefix.
enum PrefixMatch {
    /// The path does not start with the prefix.
    Outside,
    /// The path starts with the prefix, with this member after it.
    Member(Option<String>),
}

/// Match a member path against the namespace prefix that names a wrapper.
fn match_member_prefix(member_path: &str, prefix: Option<&str>) -> PrefixMatch {
    let Some(prefix) = prefix else {
        return PrefixMatch::Member(non_empty(member_path));
    };
    if member_path == prefix {
        return PrefixMatch::Member(None);
    }
    member_path
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_prefix('.'))
        .map_or(PrefixMatch::Outside, |rest| {
            PrefixMatch::Member(non_empty(rest))
        })
}

/// The files that import a wrapper, with the local binding and, for a
/// namespace import, the member that names the wrapper.
fn wrapper_consumers(
    graph: &ModuleGraph,
    wrapper: &Wrapper,
) -> Vec<(FileId, String, Option<String>)> {
    let mut consumers = Vec::new();
    let mut seen: FxHashSet<(FileId, &str)> = FxHashSet::default();
    for &importer in graph.importers_of(wrapper.file_id) {
        for (target, symbols) in graph.outgoing_symbol_edges(importer) {
            if target != wrapper.file_id {
                continue;
            }
            for symbol in symbols {
                if symbol.is_type_only {
                    continue;
                }
                let prefix = match &symbol.imported_name {
                    ImportedName::Named(name) if *name == wrapper.export => None,
                    ImportedName::Namespace => Some(wrapper.export.clone()),
                    _ => continue,
                };
                if seen.insert((importer, symbol.local_name.as_str())) {
                    consumers.push((importer, symbol.local_name.clone(), prefix));
                }
            }
        }
    }
    consumers
}

/// The sort key of a site: `file`, `line`, `col`, kind, `specifier`, `via`.
type SiteKey<'a> = (&'a str, u32, u32, UsageSiteKind, &'a str, &'a str);

fn site_key(site: &UsageSite) -> SiteKey<'_> {
    (
        site.file.as_str(),
        site.line,
        site.col,
        site.kind,
        site.specifier.as_deref().unwrap_or(""),
        site.via.as_deref().unwrap_or(""),
    )
}

/// A decoded cursor: the key of the last item of the previous page.
struct CursorKey {
    file: String,
    line: u32,
    col: u32,
    kind: UsageSiteKind,
    specifier: String,
    via: String,
}

impl CursorKey {
    fn key(&self) -> SiteKey<'_> {
        (
            self.file.as_str(),
            self.line,
            self.col,
            self.kind,
            self.specifier.as_str(),
            self.via.as_str(),
        )
    }
}

fn site_page(
    sites: &[RawSite],
    selected: Option<&[String]>,
    package_name: &str,
    limit: u16,
    cursor: Option<&str>,
) -> Result<UsageSitePage, UsageError> {
    let hash = query_hash(package_name, selected.unwrap_or(&[]));
    let mut matching: Vec<&UsageSite> = sites
        .iter()
        .map(|raw| &raw.site)
        .filter(|site| match selected {
            None => true,
            Some(names) => site
                .specifier
                .as_deref()
                .is_some_and(|name| names.iter().any(|n| n == name)),
        })
        .collect();
    matching.sort_by(|a, b| site_key(a).cmp(&site_key(b)));
    let total = matching.len();
    let start = match cursor {
        Some(token) => {
            let after = decode_cursor(token, hash)?;
            let after = after.key();
            matching.partition_point(|site| site_key(site) <= after)
        }
        None => 0,
    };
    let end = start.saturating_add(usize::from(limit)).min(total);
    let items: Vec<UsageSite> = matching
        .get(start..end)
        .unwrap_or_default()
        .iter()
        .map(|site| (*site).clone())
        .collect();
    let next_cursor = (end < total)
        .then(|| items.last().map(|last| encode_cursor(hash, last)))
        .flatten();
    Ok(UsageSitePage {
        items,
        total,
        limit,
        next_cursor,
    })
}

/// A 64-bit FNV-1a hash of the package name and the sorted specifier filter.
fn query_hash(package_name: &str, selected: &[String]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    let mut feed = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    };
    feed(package_name.as_bytes());
    for name in selected {
        feed(&[0]);
        feed(name.as_bytes());
    }
    hash
}

fn encode_cursor(hash: u64, site: &UsageSite) -> String {
    let payload = [
        format!("{hash:016x}"),
        site.file.clone(),
        site.line.to_string(),
        site.col.to_string(),
        site.kind.as_str().to_owned(),
        site.specifier.clone().unwrap_or_default(),
        site.via.clone().unwrap_or_default(),
    ]
    .join(&CURSOR_SEPARATOR.to_string());
    let mut token = String::with_capacity(CURSOR_PREFIX.len() + payload.len() * 2);
    token.push_str(CURSOR_PREFIX);
    for byte in payload.as_bytes() {
        let _ = write!(token, "{byte:02x}");
    }
    token
}

fn decode_cursor(token: &str, expected_hash: u64) -> Result<CursorKey, UsageError> {
    let hex = token
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(UsageError::InvalidCursor)?;
    if hex.len() % 2 != 0 {
        return Err(UsageError::InvalidCursor);
    }
    let bytes = (0..hex.len())
        .step_by(2)
        .map(|index| {
            hex.get(index..index + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()
        .ok_or(UsageError::InvalidCursor)?;
    let payload = String::from_utf8(bytes).map_err(|_| UsageError::InvalidCursor)?;
    let fields: Vec<&str> = payload.split(CURSOR_SEPARATOR).collect();
    let [hash, file, line, col, kind, specifier, via] = fields.as_slice() else {
        return Err(UsageError::InvalidCursor);
    };
    if u64::from_str_radix(hash, 16).ok() != Some(expected_hash) {
        return Err(UsageError::InvalidCursor);
    }
    Ok(CursorKey {
        file: (*file).to_owned(),
        line: line.parse().map_err(|_| UsageError::InvalidCursor)?,
        col: col.parse().map_err(|_| UsageError::InvalidCursor)?,
        kind: site_kind_from_str(kind).ok_or(UsageError::InvalidCursor)?,
        specifier: (*specifier).to_owned(),
        via: (*via).to_owned(),
    })
}

fn site_kind_from_str(value: &str) -> Option<UsageSiteKind> {
    [
        UsageSiteKind::Call,
        UsageSiteKind::WrapperDefinition,
        UsageSiteKind::ValueAlias,
        UsageSiteKind::NonCallReference,
        UsageSiteKind::JsxElement,
        UsageSiteKind::ReExport,
        UsageSiteKind::NestedWrapper,
        UsageSiteKind::DynamicImport,
        UsageSiteKind::Require,
        UsageSiteKind::SideEffectImport,
        UsageSiteKind::StarReExport,
    ]
    .into_iter()
    .find(|kind| kind.as_str() == value)
}

/// The seed files of the closure: the importers of the package, or with a
/// specifier filter, the files with a site of a selected name.
fn closure_seeds(
    ctx: &UsageContext<'_>,
    scan: &Scan,
    imported_by: &[PathBuf],
    selected: Option<&[String]>,
) -> FxHashSet<FileId> {
    match selected {
        None => imported_by
            .iter()
            .filter_map(|path| ctx.file_id(&path_key(path)))
            .collect(),
        Some(names) => scan
            .sites
            .iter()
            .filter(|raw| {
                raw.site
                    .specifier
                    .as_deref()
                    .is_some_and(|name| names.iter().any(|n| n == name))
            })
            .map(|raw| raw.file_id)
            .collect(),
    }
}

/// Walk the importers of the seeds up to `depth` edges. The seeds are not
/// listed.
fn consumer_closure(
    graph: &ModuleGraph,
    root: &Path,
    seeds: &FxHashSet<FileId>,
    depth: u32,
) -> ConsumerClosure {
    let mut visited = seeds.clone();
    let mut frontier: Vec<FileId> = seeds.iter().copied().collect();
    frontier.sort_unstable_by_key(|id| id.0);
    let mut found: Vec<(FileId, u32)> = Vec::new();
    let mut reached = 0;
    for level in 1..=depth {
        let mut next = Vec::new();
        for file in &frontier {
            for &importer in graph.importers_of(*file) {
                if visited.insert(importer) {
                    next.push(importer);
                    found.push((importer, level));
                }
            }
        }
        frontier = next;
        if frontier.is_empty() {
            break;
        }
        reached = level;
    }
    let truncated = reached == depth
        && frontier.iter().any(|file| {
            graph
                .importers_of(*file)
                .iter()
                .any(|importer| !visited.contains(importer))
        });
    let mut files: Vec<ClosureFile> = found
        .into_iter()
        .filter_map(|(id, level)| {
            graph.modules.get(id.0 as usize).map(|node| ClosureFile {
                file: relativize(&node.path, root),
                depth: level,
            })
        })
        .collect();
    files.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.file.cmp(&b.file)));
    ConsumerClosure {
        depth,
        file_count: files.len(),
        files,
        truncated,
    }
}

#[cfg(test)]
#[path = "trace_usage_impl_tests.rs"]
mod tests;
