//! The opt-in `constant-parameter` rule.
//!
//! The rule reads the function contract facts of each module. It joins the
//! calls of each function across the project, with each import resolved to
//! its origin export through re-export chains. It reports a parameter only
//! when the world is closed: each caller of the function is visible, and
//! each visible call passes the parameter in the same way. Each condition
//! that can hide a caller makes the rule abstain for the function, with one
//! [`Abstain`] reason.

use std::path::{Path, PathBuf};

use fallow_config::ResolvedConfig;
use fallow_types::extract::{
    CallSite, CallTarget, ExportName, FunctionContract, FunctionContractFacts,
    ImportBindingReferenceKind, ImportedName, ModuleInfo, ParamFact, ParamKind, VisibilityTag,
};
use fallow_types::suppress::IssueKind;
use rustc_hash::{FxHashMap, FxHashSet};

use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::discover::FileId;
use crate::graph::{
    EffectiveExportBinding, EffectiveExportResolution, ExportNamespace, ModuleGraph,
};
use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use crate::results::{
    ConstantParameter, ConstantParameterCallSite, ConstantParameterReason, MAX_INSPECTED_CALL_SITES,
};

/// The fewest calls that the rule reads for one parameter. With one call,
/// each literal argument is "constant", and the finding has no value. A
/// read of public projects found findings with two calls as useful as
/// findings with more calls.
const MIN_CALL_SITES: usize = 2;

const IDENTICAL_EXPLANATION: &str = "Each visible call passes the same value for this parameter. Review whether the parameter is needed; static analysis does not prove that no caller outside the project exists.";
const NEVER_SET_EXPLANATION: &str = "No visible call sets this option key, so it always has its default or is undefined. Review whether the option is needed; static analysis does not prove that no caller outside the project exists.";
const NEVER_PASSED_EXPLANATION: &str = "No visible call passes this optional parameter, so its default always applies. Review whether the parameter is needed; static analysis does not prove that no caller outside the project exists.";

/// Why the rule abstains for one function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(super) enum Abstain {
    /// A module of the project has a parse error or a parse panic.
    ParseIncomplete,
    /// The contract of the function is open, for example a read of
    /// `arguments` or a reassigned binding.
    OpenContract,
    /// The function is part of the public API of a package.
    PublicApi,
    /// The function is an export of an entry point, or a plugin or a
    /// side effect marks the export as used.
    EntryExport,
    /// A dynamic import or a dynamic import pattern reaches the module.
    DynamicImport,
    /// The module has CommonJS exports, or a `require` reads it.
    CommonJs,
    /// A namespace import of the module escapes as a value.
    NamespaceEscape,
    /// A reference to the function is not a plain call.
    ValueReference,
    /// A call target with the name of the function does not resolve, or
    /// resolves to more than one export.
    UnresolvedTarget,
    /// A template, an inline script, or a file that is not a JavaScript
    /// module can call the function.
    TemplateCallers,
    /// A direct `eval` in the module can call the function.
    DirectEval,
    /// The module can be a classic script, whose functions are globals.
    ClassicScript,
    /// A framework convention imports the module with no import statement.
    AutoImport,
    /// An import with a query, such as `?raw` or `?url`, or with a webpack
    /// inline loader reads the module as text or as a file. A copy of the
    /// code can run with callers that the project does not hold.
    AssetImport,
    /// A test file declares the function. A test helper exists to pass
    /// fixed values, so a constant argument is its purpose. A test file
    /// stays a caller of the functions of other files.
    TestFile,
    /// The function has fewer than [`MIN_CALL_SITES`] calls.
    TooFewCalls,
}

impl Abstain {
    /// Each reason, in declaration order.
    const ALL: [Self; 16] = [
        Self::ParseIncomplete,
        Self::OpenContract,
        Self::PublicApi,
        Self::EntryExport,
        Self::DynamicImport,
        Self::CommonJs,
        Self::NamespaceEscape,
        Self::ValueReference,
        Self::UnresolvedTarget,
        Self::TemplateCallers,
        Self::DirectEval,
        Self::ClassicScript,
        Self::AutoImport,
        Self::AssetImport,
        Self::TestFile,
        Self::TooFewCalls,
    ];

    const fn bit(self) -> u16 {
        1 << self as u16
    }

    /// A stable kebab-case code for the reason.
    #[must_use]
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::ParseIncomplete => "parse-incomplete",
            Self::OpenContract => "open-contract",
            Self::PublicApi => "public-api",
            Self::EntryExport => "entry-export",
            Self::DynamicImport => "dynamic-import",
            Self::CommonJs => "commonjs",
            Self::NamespaceEscape => "namespace-escape",
            Self::ValueReference => "value-reference",
            Self::UnresolvedTarget => "unresolved-target",
            Self::TemplateCallers => "template-callers",
            Self::DirectEval => "direct-eval",
            Self::ClassicScript => "classic-script",
            Self::AutoImport => "auto-import",
            Self::AssetImport => "asset-import",
            Self::TestFile => "test-file",
            Self::TooFewCalls => "too-few-calls",
        }
    }
}

// `Reasons` holds one bit for each reason.
const _: () = assert!(Abstain::ALL.len() <= u16::BITS as usize);

/// A function contract: the module and the index of its declaration.
type FnId = (FileId, u32);

/// Each [`Abstain`] reason that applies to one function, one bit for each
/// reason.
#[derive(Debug, Clone, Copy, Default)]
struct Reasons(u16);

impl Reasons {
    const fn insert(&mut self, reason: Abstain) {
        self.0 |= reason.bit();
    }

    const fn contains(self, reason: Abstain) -> bool {
        self.0 & reason.bit() != 0
    }

    const fn is_only(self, reason: Abstain) -> bool {
        self.0 == reason.bit()
    }
}

/// The counts of one reason over the functions in runtime-reachable modules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ReasonCount {
    /// Functions where the rule records this reason first.
    pub first: usize,
    /// Functions where this reason is one of the reasons.
    pub any: usize,
    /// Functions where this reason is the only reason. Each of them passes
    /// the gate when the condition does not apply.
    pub only: usize,
}

/// The counts of the gate for one run, for `RUST_LOG=fallow_core=debug`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct GateStats {
    /// Function contracts in each module.
    pub contracts: usize,
    /// Function contracts in runtime-reachable modules. The other counts
    /// cover only these.
    pub reachable: usize,
    /// Functions that pass the gate, with enough calls.
    pub passed: usize,
    /// Findings of the run.
    pub findings: usize,
    /// The counts of each reason that applies to at least one function.
    pub reasons: Vec<(Abstain, ReasonCount)>,
}

/// Inputs of the rule.
struct ConstantParameterInput<'a> {
    /// The project root. Test file patterns match the path below it.
    root: &'a Path,
    graph: &'a ModuleGraph,
    modules: &'a [ModuleInfo],
    resolved_modules: &'a [ResolvedModule],
    public_api_entry_points: &'a FxHashSet<FileId>,
    /// Modules whose exports a plugin `used_exports` rule names.
    plugin_used_export_files: &'a FxHashSet<FileId>,
    workspaces: &'a [fallow_config::WorkspaceInfo],
    /// The dependency names of each `package.json` of the project.
    declared_deps: &'a FxHashSet<String>,
    line_offsets_by_file: &'a LineOffsetsMap<'a>,
    /// `true` when a `constant-parameter` suppression covers the line.
    is_suppressed: &'a dyn Fn(FileId, u32) -> bool,
}

/// One parameter, or one option key of an object-pattern parameter, that
/// the rule reads.
struct Subject<'s> {
    index: u32,
    /// The parameter name, or the local binding of the key.
    name: &'s str,
    option_key: Option<&'s str>,
    /// `true` when the parameter has the TypeScript `?` marker.
    optional: bool,
    span_start: u32,
    has_default: bool,
    condition_reads: u32,
}

/// One call of a function, with the module that holds it.
#[derive(Clone, Copy)]
struct Call<'a> {
    file: FileId,
    site: &'a CallSite,
}

/// The result of one run: the findings and the reason of each function that
/// the rule abstained for.
struct Evaluation {
    findings: Vec<ConstantParameter>,
    #[cfg(test)]
    abstained: FxHashMap<FnId, Abstain>,
    /// Present when debug logging or a test reads it.
    stats: Option<GateStats>,
}

struct Gate<'a> {
    input: &'a ConstantParameterInput<'a>,
    modules: FxHashMap<FileId, &'a ModuleInfo>,
    resolved: FxHashMap<FileId, &'a ResolvedModule>,
    /// Each exported contract by its export name, for a target that does not
    /// resolve to one export.
    by_export_name: FxHashMap<&'a str, Vec<FnId>>,
    blocked: FxHashMap<FnId, Abstain>,
    reasons: FxHashMap<FnId, Reasons>,
    calls: FxHashMap<FnId, Vec<Call<'a>>>,
    /// `true` when a framework convention imports a module with no import
    /// statement somewhere in the project.
    has_auto_imports: bool,
    /// The module paths for specifier matching, built on first use.
    module_index: Option<specifiers::ModuleIndex>,
    /// The reasons for which each export is already blocked.
    all_exports_blocked: Reasons,
}

/// Run the rule over the project.
fn find_constant_parameters(input: &ConstantParameterInput<'_>) -> Evaluation {
    let mut gate = Gate::new(input);
    let broken: Vec<&ModuleInfo> = input
        .modules
        .iter()
        .filter(|module| module.parse_error_count > 0 || module.parse_panicked)
        .collect();
    log_broken_modules(input.graph, &broken);
    for module in broken {
        gate.block_broken_module(module.file_id);
    }
    gate.block_surfaces();
    for module in input.modules {
        gate.scan_module(module);
    }
    let findings = gate.emit();
    gate.finish(findings)
}

impl<'a> Gate<'a> {
    fn new(input: &'a ConstantParameterInput<'a>) -> Self {
        let modules: FxHashMap<FileId, &ModuleInfo> =
            input.modules.iter().map(|m| (m.file_id, m)).collect();
        let resolved = input
            .resolved_modules
            .iter()
            .map(|m| (m.file_id, m))
            .collect();
        let mut by_export_name: FxHashMap<&str, Vec<FnId>> = FxHashMap::default();
        for module in input.modules {
            for (contract, name) in exported_contracts(module) {
                by_export_name
                    .entry(name)
                    .or_default()
                    .push((module.file_id, contract));
            }
        }
        Self {
            input,
            modules,
            resolved,
            by_export_name,
            blocked: FxHashMap::default(),
            reasons: FxHashMap::default(),
            calls: FxHashMap::default(),
            has_auto_imports: input.resolved_modules.iter().any(|module| {
                module
                    .resolved_imports
                    .iter()
                    .any(|import| import.target.is_synthetic_auto_import())
            }),
            module_index: None,
            all_exports_blocked: Reasons::default(),
        }
    }

    fn finish(self, findings: Vec<ConstantParameter>) -> Evaluation {
        let stats = (cfg!(test) || tracing::enabled!(tracing::Level::DEBUG))
            .then(|| self.stats(findings.len()));
        Evaluation {
            findings,
            #[cfg(test)]
            abstained: self.blocked,
            stats,
        }
    }

    fn stats(&self, findings: usize) -> GateStats {
        let contracts = self.all_contracts();
        let reachable: Vec<FnId> = contracts
            .iter()
            .copied()
            .filter(|(file, _)| {
                self.input
                    .graph
                    .modules
                    .get(file.0 as usize)
                    .is_some_and(|node| node.is_runtime_reachable())
            })
            .collect();
        let mut counts = [ReasonCount::default(); Abstain::ALL.len()];
        let mut passed = 0;
        for id in &reachable {
            let Some(first) = self.blocked.get(id) else {
                passed += 1;
                continue;
            };
            let reasons = self.reasons.get(id).copied().unwrap_or_default();
            for (count, reason) in counts.iter_mut().zip(Abstain::ALL) {
                count.first += usize::from(*first == reason);
                count.any += usize::from(reasons.contains(reason));
                count.only += usize::from(reasons.is_only(reason));
            }
        }
        GateStats {
            contracts: contracts.len(),
            reachable: reachable.len(),
            passed,
            findings,
            reasons: Abstain::ALL
                .into_iter()
                .zip(counts)
                .filter(|(_, count)| count.any > 0)
                .collect(),
        }
    }

    fn all_contracts(&self) -> Vec<FnId> {
        self.input
            .modules
            .iter()
            .flat_map(|module| {
                let count = facts(module).map_or(0, |facts| facts.declarations.len());
                (0..count).filter_map(move |index| {
                    u32::try_from(index)
                        .ok()
                        .map(|index| (module.file_id, index))
                })
            })
            .collect()
    }

    /// Record a reason for a function. The first reason stays the reason
    /// of the function.
    fn block(&mut self, id: FnId, reason: Abstain) {
        self.blocked.entry(id).or_insert(reason);
        self.reasons.entry(id).or_default().insert(reason);
    }

    fn block_module(&mut self, file: FileId, reason: Abstain) {
        let count = self
            .modules
            .get(&file)
            .and_then(|module| facts(module))
            .map_or(0, |facts| facts.declarations.len());
        for index in 0..count {
            if let Ok(index) = u32::try_from(index) {
                self.block((file, index), reason);
            }
        }
    }

    fn block_all_exports(&mut self, reason: Abstain) {
        if self.all_exports_blocked.contains(reason) {
            return;
        }
        self.all_exports_blocked.insert(reason);
        let ids: Vec<FnId> = self.by_export_name.values().flatten().copied().collect();
        for id in ids {
            self.block(id, reason);
        }
    }

    fn block_name(&mut self, name: &str, reason: Abstain) {
        let ids = self.by_export_name.get(name).cloned().unwrap_or_default();
        for id in ids {
            self.block(id, reason);
        }
    }

    /// Block what a module with a parse error can call. Its facts can miss
    /// each import and each call, so the rule reads its import specifiers
    /// from the source text. A declaration file has no code that runs.
    fn block_broken_module(&mut self, file: FileId) {
        self.block_module(file, Abstain::ParseIncomplete);
        let Some(node) = self.input.graph.modules.get(file.0 as usize) else {
            self.block_all_exports(Abstain::ParseIncomplete);
            return;
        };
        if is_declaration_path(&node.path) {
            return;
        }
        // A framework auto-import, or a component template, can call a
        // function with no import specifier in the text.
        if self.has_auto_imports || !is_script_path(&node.path) {
            self.block_all_exports(Abstain::ParseIncomplete);
            return;
        }
        self.block_scanned_targets(file, Abstain::ParseIncomplete);
    }

    /// Block each function that an import specifier in the source text of
    /// `file` can reach. A text that the rule cannot read, or a specifier
    /// that can name any module, blocks each export.
    fn block_scanned_targets(&mut self, file: FileId, reason: Abstain) {
        let Some(node) = self.input.graph.modules.get(file.0 as usize) else {
            self.block_all_exports(reason);
            return;
        };
        let Ok(text) = std::fs::read_to_string(&node.path) else {
            self.block_all_exports(reason);
            return;
        };
        let specifiers::Scan::Specifiers(found) = specifiers::scan(&text) else {
            self.block_all_exports(reason);
            return;
        };
        let mut targets = Vec::new();
        for specifier in &found {
            let Some(files) = self.specifier_targets(&node.path, specifier) else {
                self.block_all_exports(reason);
                return;
            };
            targets.extend(files);
        }
        for target in targets {
            self.block_surface(target, reason);
        }
    }

    /// The modules that `specifier` in the module at `from` can name, or
    /// `None` when it can name each module.
    fn specifier_targets(&mut self, from: &Path, specifier: &str) -> Option<Vec<FileId>> {
        let graph = self.input.graph;
        let index = self.module_index.get_or_insert_with(|| {
            specifiers::ModuleIndex::new(
                graph
                    .modules
                    .iter()
                    .map(|node| (node.file_id, node.path.as_path())),
            )
        });
        match specifiers::target(from, specifier) {
            specifiers::Target::External => Some(Vec::new()),
            specifiers::Target::Unknown => None,
            specifiers::Target::Path(path) => Some(index.under_path(&path)),
            specifiers::Target::RootRelative(rooted) => Some(index.with_suffix(&rooted)),
            specifiers::Target::Bare { specifier, package } => {
                let workspaces = self.input.workspaces;
                let roots: Vec<PathBuf> = workspaces
                    .iter()
                    .filter(|ws| ws.name == package)
                    .map(|ws| ws.root.clone())
                    .chain(linked_package_dir(from, &package))
                    .collect();
                if !roots.is_empty() {
                    return Some(
                        roots
                            .iter()
                            .flat_map(|root| index.under_path(root))
                            .collect(),
                    );
                }
                let external = self.input.declared_deps.contains(&package)
                    || super::predicates::is_builtin_module(&package);
                // A path alias or a `baseUrl` import can reuse the name of a
                // dependency or a built-in module.
                (external && index.with_suffix(&specifier).is_empty()).then(Vec::new)
            }
        }
    }

    /// Block each function that the value surface of `file` exposes, also
    /// through namespace re-exports.
    fn block_surface(&mut self, file: FileId, reason: Abstain) {
        let graph = self.input.graph;
        let mut stack: Vec<_> = graph
            .unique_export_bindings(file, ExportNamespace::Value)
            .into_iter()
            .collect();
        let mut visited = FxHashSet::default();
        while let Some(binding) = stack.pop() {
            if !visited.insert(binding) {
                continue;
            }
            if let Some(source) = binding.namespace_source() {
                stack.extend(graph.unique_export_bindings(source, ExportNamespace::Value));
            } else if let Some(id) = self.binding_contract(binding) {
                self.block(id, reason);
            }
        }
    }

    /// The function contract that a unique export binding names.
    fn binding_contract(&self, binding: EffectiveExportBinding) -> Option<FnId> {
        let origin = self.input.graph.export_binding_origin(binding)?;
        let file = binding.origin_file();
        let module = self.modules.get(&file)?;
        let symbol = origin.export();
        let export = module
            .exports
            .iter()
            .find(|export| export.name == symbol.name && export.span.start == symbol.span.start)?;
        if export.is_type_only {
            return None;
        }
        let local = export_local_name(export)?;
        Some((file, local_contract(module, local)?))
    }

    /// Block the surfaces that callers outside the facts can reach: package
    /// public API, entry points, dynamic imports, `require` reads and
    /// framework auto-imports.
    fn block_surfaces(&mut self) {
        let public: Vec<FileId> = self.input.public_api_entry_points.iter().copied().collect();
        for file in public {
            self.block_surface(file, Abstain::PublicApi);
        }
        let graph = self.input.graph;
        let entries: Vec<FileId> = graph
            .modules
            .iter()
            .filter(|node| node.is_entry_point())
            .map(|node| node.file_id)
            .collect();
        for file in entries {
            self.block_surface(file, Abstain::EntryExport);
        }
        for resolved in self.input.resolved_modules {
            self.block_hidden_imports(resolved);
        }
    }

    fn block_hidden_imports(&mut self, resolved: &ResolvedModule) {
        let imports = resolved
            .resolved_imports
            .iter()
            .chain(&resolved.resolved_dynamic_imports)
            .filter(|import| !import.info.is_type_only && reads_as_asset(&import.info.source))
            .filter_map(|import| import.target.internal_file_id());
        let re_exports = resolved
            .re_exports
            .iter()
            .filter(|re_export| {
                !re_export.info.is_type_only && reads_as_asset(&re_export.info.source)
            })
            .filter_map(|re_export| re_export.target.internal_file_id());
        for target in imports.chain(re_exports) {
            self.block_module(target, Abstain::AssetImport);
        }
        for import in &resolved.resolved_dynamic_imports {
            if let Some(target) = import.target.internal_file_id() {
                self.block_surface(target, Abstain::DynamicImport);
            }
        }
        for (_, targets) in &resolved.resolved_dynamic_patterns {
            for target in targets {
                self.block_surface(*target, Abstain::DynamicImport);
            }
        }
        let static_count = self
            .modules
            .get(&resolved.file_id)
            .map_or(0, |module| module.imports.len());
        for import in resolved.resolved_imports.iter().skip(static_count) {
            let Some(target) = import.target.internal_file_id() else {
                continue;
            };
            let reason = if import.target.is_synthetic_auto_import() {
                Abstain::AutoImport
            } else {
                Abstain::CommonJs
            };
            self.block_surface(target, reason);
        }
    }

    fn scan_module(&mut self, module: &'a ModuleInfo) {
        let file = module.file_id;
        let node = self.input.graph.modules.get(file.0 as usize);
        let has_cjs = node.is_some_and(|node| node.has_cjs_exports());
        let facts = facts(module);
        let script = node.is_none_or(|node| is_script_path(&node.path));
        if node.is_some_and(|node| self.is_test_file(&node.path)) {
            self.block_module(file, Abstain::TestFile);
        }
        if let Some(reason) = module_reason(module, facts, has_cjs, script) {
            self.block_module(file, reason);
            if matches!(reason, Abstain::TemplateCallers | Abstain::DirectEval) {
                self.block_import_targets(module, reason);
            }
        }
        let html = node.is_some_and(|node| node.path.extension().is_some_and(|ext| ext == "html"));
        if html && facts.is_some_and(|facts| facts.template_callers) {
            // Extraction records no import of an inline script.
            self.block_scanned_targets(file, Abstain::TemplateCallers);
        }
        self.block_exports(module);
        self.block_import_references(module);
        let Some(facts) = facts else {
            self.block_unrecorded_imports(module, None);
            return;
        };
        for (index, contract) in facts.declarations.iter().enumerate() {
            if contract.incomplete.is_some()
                && let Ok(index) = u32::try_from(index)
            {
                self.block((file, index), Abstain::OpenContract);
            }
        }
        for escape in &facts.escapes {
            self.block((file, escape.contract), Abstain::ValueReference);
        }
        self.collect_calls(module, facts);
        self.block_unrecorded_imports(module, Some(facts));
    }

    /// `true` when `path` matches a test pattern of production mode, below
    /// the project root.
    fn is_test_file(&self, path: &Path) -> bool {
        let relative = path.strip_prefix(self.input.root).unwrap_or(path);
        super::predicates::is_test_or_spec_file(relative)
    }

    /// Block exports that a caller outside the import graph can use.
    fn block_exports(&mut self, module: &ModuleInfo) {
        let file = module.file_id;
        let plugin = self.input.plugin_used_export_files.contains(&file);
        for export in module.exports.iter() {
            let Some(contract) =
                export_local_name(export).and_then(|local| local_contract(module, local))
            else {
                continue;
            };
            if plugin || export.is_side_effect_used {
                self.block((file, contract), Abstain::EntryExport);
            } else if export.visibility != VisibilityTag::None {
                self.block((file, contract), Abstain::PublicApi);
            }
        }
    }

    /// Block each function that an import of `module` can name.
    fn block_import_targets(&mut self, module: &ModuleInfo, reason: Abstain) {
        let Some(resolved) = self.resolved.get(&module.file_id).copied() else {
            return;
        };
        for import in &resolved.resolved_imports {
            let Some(target) = import.target.internal_file_id() else {
                continue;
            };
            match import_export_name(&import.info.imported_name) {
                Some(name) => self.block_export(target, name, reason),
                None => {
                    self.block_surface(target, reason);
                    self.block_module(target, reason);
                }
            }
        }
    }

    /// Block the function that `target` exports as `name`, or each function
    /// with that export name when the name does not resolve to one export.
    fn block_export(&mut self, target: FileId, name: &str, reason: Abstain) {
        match self
            .input
            .graph
            .resolve_export(target, name, ExportNamespace::Value)
        {
            EffectiveExportResolution::Unique(binding) => {
                if let Some(source) = binding.namespace_source() {
                    self.block_surface(source, reason);
                } else if let Some(id) = self.binding_contract(binding) {
                    self.block(id, reason);
                }
            }
            EffectiveExportResolution::Ambiguous | EffectiveExportResolution::Missing => {
                self.block_name(name, Abstain::UnresolvedTarget);
            }
        }
    }

    /// The resolved import at `index`, when it matches the static import.
    fn resolved_import(&self, module: &ModuleInfo, index: usize) -> Option<&'a ResolvedImport> {
        let import = module.imports.get(index)?;
        let resolved = self
            .resolved
            .get(&module.file_id)?
            .resolved_imports
            .get(index)?;
        (resolved.info.local_name == import.local_name && resolved.info.span == import.span)
            .then_some(resolved)
    }

    /// Each import binding reference that is not a plain call is an escape.
    fn block_import_references(&mut self, module: &ModuleInfo) {
        for reference in module.import_binding_references.iter() {
            if reference.through.is_some()
                || matches!(
                    reference.kind,
                    ImportBindingReferenceKind::InitializerCall | ImportBindingReferenceKind::Call
                )
            {
                continue;
            }
            let index = reference.import_index as usize;
            let Some(import) = module.imports.get(index) else {
                continue;
            };
            let first = reference.member_path.split('.').next().unwrap_or("");
            let name = match &import.imported_name {
                ImportedName::Namespace if first.is_empty() => {
                    self.block_import_surface(module, index, Abstain::NamespaceEscape);
                    continue;
                }
                ImportedName::Namespace => first,
                other => match import_export_name(other) {
                    Some(name) => name,
                    None => continue,
                },
            };
            self.block_import_name(module, index, name, Abstain::ValueReference);
        }
    }

    fn block_import_surface(&mut self, module: &ModuleInfo, index: usize, reason: Abstain) {
        match self.resolved_import(module, index) {
            Some(resolved) => {
                if let Some(target) = resolved.target.internal_file_id() {
                    self.block_surface(target, reason);
                }
            }
            None => self.block_module_imports(module, reason),
        }
    }

    fn block_import_name(
        &mut self,
        module: &ModuleInfo,
        index: usize,
        name: &str,
        reason: Abstain,
    ) {
        let Some(resolved) = self.resolved_import(module, index) else {
            self.block_name(name, Abstain::UnresolvedTarget);
            return;
        };
        match &resolved.target {
            ResolveResult::Unresolvable(_) => self.block_name(name, Abstain::UnresolvedTarget),
            target => {
                if let Some(target) = target.internal_file_id() {
                    self.block_export(target, name, reason);
                }
            }
        }
    }

    /// Block each import of a module whose resolution does not line up with
    /// its facts.
    fn block_module_imports(&mut self, module: &ModuleInfo, reason: Abstain) {
        for (index, import) in module.imports.iter().enumerate() {
            match import_export_name(&import.imported_name) {
                Some(name) => self.block_import_name(module, index, name, reason),
                None if import.imported_name == ImportedName::Namespace => {
                    self.block_import_surface(module, index, reason);
                }
                None => {}
            }
        }
    }

    fn collect_calls(&mut self, module: &'a ModuleInfo, facts: &'a FunctionContractFacts) {
        let file = module.file_id;
        for group in &facts.calls {
            match group.target {
                CallTarget::Local(contract) => self
                    .calls
                    .entry((file, contract))
                    .or_default()
                    .extend(group.sites.iter().map(|site| Call { file, site })),
                CallTarget::Import(index) => {
                    self.collect_import_calls(
                        module,
                        index as usize,
                        &group.member_path,
                        &group.sites,
                    );
                }
            }
        }
    }

    fn collect_import_calls(
        &mut self,
        module: &ModuleInfo,
        index: usize,
        member_path: &str,
        sites: &'a [CallSite],
    ) {
        let Some(import) = module.imports.get(index) else {
            return;
        };
        let (name, rest) = match &import.imported_name {
            ImportedName::Namespace => {
                let (head, rest) = member_path.split_once('.').unwrap_or((member_path, ""));
                if head.is_empty() {
                    return;
                }
                (head, rest)
            }
            other => match import_export_name(other) {
                Some(name) => (name, member_path),
                None => return,
            },
        };
        let Some(resolved) = self.resolved_import(module, index) else {
            self.block_name(name, Abstain::UnresolvedTarget);
            return;
        };
        let target = match &resolved.target {
            ResolveResult::Unresolvable(_) => {
                self.block_name(name, Abstain::UnresolvedTarget);
                return;
            }
            target => match target.internal_file_id() {
                Some(target) => target,
                None => return,
            },
        };
        let binding = match self
            .input
            .graph
            .resolve_export(target, name, ExportNamespace::Value)
        {
            EffectiveExportResolution::Unique(binding) => binding,
            EffectiveExportResolution::Ambiguous | EffectiveExportResolution::Missing => {
                self.block_name(name, Abstain::UnresolvedTarget);
                return;
            }
        };
        if let Some(source) = binding.namespace_source() {
            self.block_surface(source, Abstain::NamespaceEscape);
            return;
        }
        let Some(id) = self.binding_contract(binding) else {
            return;
        };
        if rest.is_empty() {
            let file = module.file_id;
            self.calls
                .entry(id)
                .or_default()
                .extend(sites.iter().map(|site| Call { file, site }));
        } else {
            // `f.call(...)`, `f.apply(...)` or `f.bind(...)` shift or hide
            // the arguments.
            self.block(id, Abstain::ValueReference);
        }
    }

    /// Block the target of an import whose uses the facts do not hold.
    ///
    /// The facts record a call or a reference only for an admitted import
    /// binding. A binding that is used but has no recorded use, and a call
    /// that the visitor saw but the call facts do not hold, hide a caller.
    fn block_unrecorded_imports(
        &mut self,
        module: &ModuleInfo,
        facts: Option<&FunctionContractFacts>,
    ) {
        let Some(resolved) = self.resolved.get(&module.file_id).copied() else {
            return;
        };
        let recorded: FxHashSet<(u32, u32)> = facts
            .into_iter()
            .flat_map(FunctionContractFacts::call_sites)
            .filter_map(|(target, _, site)| match target {
                CallTarget::Import(index) => Some((index, site.span_start)),
                CallTarget::Local(_) => None,
            })
            .collect();
        let referenced: FxHashSet<u32> = module
            .import_binding_references
            .iter()
            .map(|reference| reference.import_index)
            .chain(recorded.iter().map(|(index, _)| *index))
            .collect();
        for (index, import) in module.imports.iter().enumerate() {
            let Ok(index_u32) = u32::try_from(index) else {
                break;
            };
            if import.is_type_only || import.local_name.is_empty() {
                continue;
            }
            let used = !resolved.unused_import_bindings.contains(&import.local_name);
            let missed_call = module.imported_call_sites.iter().any(|call| {
                call.local_name == import.local_name
                    && !recorded.contains(&(index_u32, call.span_start))
            });
            if (used && !referenced.contains(&index_u32)) || missed_call {
                self.block_module_import(module, index, Abstain::ValueReference);
            }
        }
    }

    fn block_module_import(&mut self, module: &ModuleInfo, index: usize, reason: Abstain) {
        let Some(import) = module.imports.get(index) else {
            return;
        };
        match import_export_name(&import.imported_name) {
            Some(name) => self.block_import_name(module, index, name, reason),
            None => self.block_import_surface(module, index, reason),
        }
    }

    fn emit(&mut self) -> Vec<ConstantParameter> {
        let mut findings = Vec::new();
        for module in self.input.modules {
            let Some(facts) = facts(module) else {
                continue;
            };
            let reachable = self
                .input
                .graph
                .modules
                .get(module.file_id.0 as usize)
                .is_some_and(|node| node.is_runtime_reachable());
            if !reachable {
                continue;
            }
            for (index, contract) in facts.declarations.iter().enumerate() {
                let Ok(index) = u32::try_from(index) else {
                    break;
                };
                let id = (module.file_id, index);
                if self.blocked.contains_key(&id) {
                    continue;
                }
                let calls = self.calls.get(&id).map_or(&[][..], Vec::as_slice);
                if calls.len() < MIN_CALL_SITES {
                    self.block(id, Abstain::TooFewCalls);
                    continue;
                }
                findings.extend(self.contract_findings(module, contract, calls));
            }
        }
        findings
    }

    fn contract_findings(
        &self,
        module: &ModuleInfo,
        contract: &FunctionContract,
        calls: &[Call<'a>],
    ) -> Vec<ConstantParameter> {
        let mut findings = Vec::new();
        for (index, param) in contract.params.iter().enumerate() {
            let Ok(index) = u32::try_from(index) else {
                break;
            };
            if param.is_rest {
                continue;
            }
            match (param.kind, param.name.as_deref()) {
                (ParamKind::Identifier, Some(name)) if !param.written => {
                    let counted: Vec<Call<'a>> = calls
                        .iter()
                        .copied()
                        .filter(|call| !values::is_pass_through(call.site, index))
                        .collect();
                    let verdict = self.parameter_verdict(param, index, &counted);
                    let subject = Subject {
                        index,
                        name,
                        option_key: None,
                        optional: param.optional,
                        span_start: param.span_start,
                        has_default: param.has_default,
                        condition_reads: param.condition_reads,
                    };
                    findings.extend(self.checked_finding(
                        module, contract, param, &subject, verdict, &counted, calls,
                    ));
                }
                (ParamKind::Object, _) => {
                    for key in param.keys.iter().filter(|key| !key.written) {
                        let counted: Vec<Call<'a>> = calls
                            .iter()
                            .copied()
                            .filter(|call| !values::is_key_pass_through(call.site, index, &key.key))
                            .collect();
                        let verdict = self.key_verdict(param, key, index, &counted);
                        let subject = Subject {
                            index,
                            name: &key.binding,
                            option_key: Some(&key.key),
                            optional: param.optional,
                            span_start: key.span_start,
                            has_default: key.has_default,
                            condition_reads: key.condition_reads,
                        };
                        findings.extend(self.checked_finding(
                            module, contract, param, &subject, verdict, &counted, calls,
                        ));
                    }
                }
                _ => {}
            }
        }
        findings
    }

    /// The finding for one subject, unless too few calls count, a self-call
    /// passes a never-passed subject on, or a suppression covers it.
    #[expect(
        clippy::too_many_arguments,
        reason = "the subject, its verdict and both call lists belong together"
    )]
    fn checked_finding(
        &self,
        module: &ModuleInfo,
        contract: &FunctionContract,
        param: &ParamFact,
        subject: &Subject<'_>,
        verdict: Option<(ConstantParameterReason, Option<values::Value<'a>>)>,
        counted: &[Call<'a>],
        calls: &[Call<'a>],
    ) -> Option<ConstantParameter> {
        if counted.len() < MIN_CALL_SITES {
            return None;
        }
        let (reason, value) = verdict?;
        // A self-call that passes the subject on carries a value that the
        // function made, such as a cycle cache, so the default is not the
        // only value.
        if reason == ConstantParameterReason::NeverPassed && counted.len() < calls.len() {
            return None;
        }
        if self.suppressed(module.file_id, subject.span_start)
            || self.suppressed(module.file_id, param.span_start)
            || self.suppressed(module.file_id, contract.span_start)
        {
            return None;
        }
        Some(self.finding(module, contract, subject, (reason, value), counted))
    }

    fn suppressed(&self, file: FileId, offset: u32) -> bool {
        let (line, _) = byte_offset_to_line_col(self.input.line_offsets_by_file, file, offset);
        (self.input.is_suppressed)(file, line)
    }

    fn finding(
        &self,
        module: &ModuleInfo,
        contract: &FunctionContract,
        subject: &Subject<'_>,
        (reason, value): (ConstantParameterReason, Option<values::Value<'a>>),
        calls: &[Call<'_>],
    ) -> ConstantParameter {
        let offsets = self.input.line_offsets_by_file;
        let (line, col) = byte_offset_to_line_col(offsets, module.file_id, subject.span_start);
        let mut sites: Vec<ConstantParameterCallSite> = calls
            .iter()
            .filter_map(|call| {
                let node = self.input.graph.modules.get(call.file.0 as usize)?;
                let (line, col) = byte_offset_to_line_col(offsets, call.file, call.site.span_start);
                Some(ConstantParameterCallSite {
                    path: node.path.clone(),
                    line,
                    col,
                })
            })
            .collect();
        sites.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
        });
        sites.dedup();
        sites.truncate(MAX_INSPECTED_CALL_SITES);
        let path = self
            .input
            .graph
            .modules
            .get(module.file_id.0 as usize)
            .map_or_else(|| Path::new("").to_path_buf(), |node| node.path.clone());
        ConstantParameter {
            path,
            function_name: contract.binding_name.to_string(),
            parameter_name: subject.name.to_string(),
            parameter_index: subject.index,
            option_key: subject.option_key.map(str::to_string),
            line,
            col,
            reason,
            value: value.map(values::Value::wire),
            optional: subject.optional,
            has_default: subject.has_default,
            condition_reads: subject.condition_reads,
            call_site_count: u32::try_from(calls.len()).unwrap_or(u32::MAX),
            inspected_call_sites: sites,
            explanation: match (reason, subject.option_key) {
                (ConstantParameterReason::NeverPassed, Some(_)) => NEVER_SET_EXPLANATION,
                (ConstantParameterReason::NeverPassed, None) => NEVER_PASSED_EXPLANATION,
                _ => IDENTICAL_EXPLANATION,
            }
            .to_string(),
        }
    }
}

/// `true` when an import specifier can read its module as text, as a file
/// or through a loader: a query such as `./op.ts?raw`, or a webpack inline
/// loader request such as `raw-loader!./op.ts`. A `!` can also be part of a
/// plain file name, so the check can only hide a finding, never add one.
fn reads_as_asset(specifier: &str) -> bool {
    specifier.contains(['?', '!'])
}

fn facts(module: &ModuleInfo) -> Option<&FunctionContractFacts> {
    module.function_contracts.as_deref()
}

/// The real directory of an installed package that is local source, such as
/// a `link:` or `file:` dependency, from the nearest `node_modules` above
/// `from`.
fn linked_package_dir(from: &Path, package: &str) -> Option<PathBuf> {
    let installed = from
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("node_modules").join(package))
        .find(|candidate| candidate.exists())?;
    let real = dunce::canonicalize(installed).ok()?;
    let in_node_modules = real
        .components()
        .any(|component| component.as_os_str() == "node_modules");
    (!in_node_modules).then_some(real)
}

/// `true` for a TypeScript declaration file, which holds no code that runs.
fn is_declaration_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [".d.ts", ".d.mts", ".d.cts"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

/// The reason that opens each contract of a module.
fn module_reason(
    module: &ModuleInfo,
    facts: Option<&FunctionContractFacts>,
    has_cjs: bool,
    script: bool,
) -> Option<Abstain> {
    if facts.is_some_and(|facts| facts.template_callers) || !script {
        return Some(Abstain::TemplateCallers);
    }
    if facts.is_some_and(|facts| facts.direct_eval) {
        return Some(Abstain::DirectEval);
    }
    if has_cjs {
        return Some(Abstain::CommonJs);
    }
    if module.imports.is_empty() && module.exports.is_empty() && module.re_exports.is_empty() {
        return Some(Abstain::ClassicScript);
    }
    None
}

/// `true` for a file that holds only JavaScript or TypeScript module code.
/// Another host, such as an HTML page, can call a function from markup or
/// an inline script that the facts do not see.
fn is_script_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension,
                "js" | "jsx" | "mjs" | "cjs" | "ts" | "tsx" | "mts" | "cts"
            )
        })
}

/// The export name that a named or default import reads.
fn import_export_name(imported: &ImportedName) -> Option<&str> {
    match imported {
        ImportedName::Named(name) => Some(name),
        ImportedName::Default => Some("default"),
        ImportedName::Namespace | ImportedName::SideEffect => None,
    }
}

/// The local binding that an export names.
fn export_local_name(export: &fallow_types::extract::ExportInfo) -> Option<&str> {
    export.local_name.as_deref().or(match &export.name {
        ExportName::Named(name) => Some(name.as_str()),
        ExportName::Default => None,
    })
}

fn local_contract(module: &ModuleInfo, local: &str) -> Option<u32> {
    let index = facts(module)?
        .declarations
        .iter()
        .position(|contract| &*contract.binding_name == local)?;
    u32::try_from(index).ok()
}

/// Each exported contract of a module, with its export name.
fn exported_contracts(module: &ModuleInfo) -> Vec<(u32, &str)> {
    module
        .exports
        .iter()
        .filter(|export| !export.is_type_only)
        .filter_map(|export| {
            let contract = local_contract(module, export_local_name(export)?)?;
            let name = match &export.name {
                ExportName::Named(name) => name.as_str(),
                ExportName::Default => "default",
            };
            Some((contract, name))
        })
        .collect()
}

/// `true` when the rule is on for the project or for an override.
fn rule_enabled(config: &ResolvedConfig) -> bool {
    config.reads_call_facts()
}

/// Release the extraction payload that resolution copied, before analysis.
///
/// The rule reads `import_binding_references` to find each import of a
/// function that is not a plain call, so the references stay while the rule
/// is on.
pub fn release_resolution_payloads(modules: &mut [ModuleInfo], config: &ResolvedConfig) {
    let keep_references = rule_enabled(config);
    for module in modules {
        let references =
            keep_references.then(|| std::sync::Arc::clone(&module.import_binding_references));
        module.release_resolution_payload();
        if let Some(references) = references {
            module.import_binding_references = references;
        }
    }
}

/// The most modules with a parse error that the debug log names.
const MAX_LOGGED_BROKEN_MODULES: usize = 20;

/// Log the modules whose parse error blocks each function, for
/// `RUST_LOG=fallow_core=debug`.
fn log_broken_modules(graph: &ModuleGraph, broken: &[&ModuleInfo]) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    tracing::debug!(
        modules = broken.len(),
        "constant-parameter blocked by parse errors"
    );
    for module in broken.iter().take(MAX_LOGGED_BROKEN_MODULES) {
        let Some(node) = graph.modules.get(module.file_id.0 as usize) else {
            continue;
        };
        tracing::debug!(
            path = %node.path.display(),
            errors = module.parse_error_count,
            panicked = module.parse_panicked,
            "constant-parameter parse error"
        );
    }
}

/// Log the counts of the gate, for `RUST_LOG=fallow_core=debug`.
fn log_stats(stats: Option<&GateStats>) {
    let Some(stats) = stats else {
        return;
    };
    tracing::debug!(
        contracts = stats.contracts,
        reachable = stats.reachable,
        passed = stats.passed,
        findings = stats.findings,
        "constant-parameter gate"
    );
    for (reason, count) in &stats.reasons {
        tracing::debug!(
            reason = reason.code(),
            first = count.first,
            any = count.any,
            only = count.only,
            "constant-parameter abstained"
        );
    }
}

/// Run the rule when it is on for any path, and store the findings.
pub(super) fn populate(input: &mut super::PostDetectionInput<'_, '_>) {
    let config = input.config;
    if !rule_enabled(config) {
        return;
    }
    let matchers = super::unused_exports::compile_plugin_matchers(input.plugin_result);
    let plugin_used_export_files: FxHashSet<FileId> = if matchers.is_empty() {
        FxHashSet::default()
    } else {
        input
            .graph
            .modules
            .iter()
            .filter(|node| {
                let relative = node.path.strip_prefix(&config.root).unwrap_or(&node.path);
                let relative = relative.to_string_lossy().replace('\\', "/");
                matchers.iter().any(|rule| rule.matches(&relative))
            })
            .map(|node| node.file_id)
            .collect()
    };
    let suppressions = input.suppressions;
    let is_suppressed = |file: FileId, line: u32| {
        suppressions.is_suppressed(file, line, IssueKind::ConstantParameter)
    };
    let evaluation = find_constant_parameters(&ConstantParameterInput {
        root: &config.root,
        graph: input.graph,
        modules: input.modules,
        resolved_modules: input.resolved_modules,
        public_api_entry_points: input.public_api_entry_points,
        plugin_used_export_files: &plugin_used_export_files,
        workspaces: input.workspaces,
        declared_deps: input.declared_deps,
        line_offsets_by_file: input.line_offsets_by_file,
        is_suppressed: &is_suppressed,
    });
    #[cfg(test)]
    tests::record(
        input.graph,
        input.modules,
        &evaluation.abstained,
        evaluation.stats.as_ref(),
    );
    log_stats(evaluation.stats.as_ref());
    input.results.constant_parameters = evaluation
        .findings
        .into_iter()
        .map(crate::results::ConstantParameterFinding::with_actions)
        .collect();
}

#[path = "constant_parameter_specifiers.rs"]
mod specifiers;

#[path = "constant_parameter_values.rs"]
mod values;

#[cfg(test)]
#[path = "constant_parameter_tests.rs"]
mod tests;
