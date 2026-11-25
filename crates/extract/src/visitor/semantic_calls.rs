//! Store the call and reference facts that the semantic pass resolves, and
//! merge the call evidence across the script blocks of one host file.

use fallow_types::extract::{
    ArgFact, BindingRoot, CallTarget, FunctionContractFacts, ImportBindingReference, KeyValue,
};
use oxc_span::Span;
use rustc_hash::FxHashSet;

use super::{ModuleInfo, ModuleInfoExtractor};

/// The working state of the loads with an unknown source in one program.
#[derive(Default)]
pub(super) struct OpaqueLoadState {
    /// The span of each `import()` or `require` call whose uses the
    /// semantic pass reads.
    spans: FxHashSet<Span>,
    /// True when a template outside the program can read each top-level
    /// binding: a Vue `<script setup>` block, a Svelte script, an Astro
    /// frontmatter or a Glimmer file.
    template_reads_root: bool,
    /// True when the program has an ambient `declare` of `require`, which
    /// has no runtime binding and does not hide the module loader.
    pub(super) ambient_require: bool,
}

impl ModuleInfoExtractor {
    pub(crate) fn imported_call_reference_candidates(&self) -> FxHashSet<Span> {
        self.pending_imported_call_sites
            .iter()
            .map(|(span, _)| *span)
            .collect()
    }

    /// Record a bare reference to a namespace-like binding as a whole-object
    /// use. A reference to a namespace import local waits for the semantic
    /// pass, which keeps it only when it resolves to the import binding. A
    /// catch parameter or another local of the same name can shadow the
    /// import where the scope stack of the walk does not see it (issue #5064).
    pub(super) fn record_namespace_whole_object_use(&mut self, span: Span, name: &str) {
        // A nested namespace binding of the same name in scope here
        // (`const NS = require('./x')` in a function) is not a shadow.
        let walk_needs_name = name == "loaderData"
            || self.route_loader_data_bindings.contains(name)
            || self.walk_order_class_bindings.contains_key(name)
            || self
                .scoped_namespace_binding_names
                .iter()
                .any(|scope| scope.contains(name));
        if self.namespace_import_locals.contains(name) && !walk_needs_name {
            self.shadowed_reads
                .namespace_whole_uses
                .push((span, name.to_string()));
            return;
        }
        self.record_whole_object_identifier_use(name);
    }

    pub(crate) fn namespace_whole_object_use_candidates(&self) -> FxHashSet<Span> {
        self.shadowed_reads
            .namespace_whole_uses
            .iter()
            .map(|(span, _)| *span)
            .collect()
    }

    /// Record the pending namespace references in `import_references` as
    /// whole-object uses, and drop the others: a local of the same name in a
    /// nested scope declares their binding.
    pub(crate) fn resolve_namespace_whole_object_uses(
        &mut self,
        import_references: &FxHashSet<Span>,
    ) {
        for (span, name) in std::mem::take(&mut self.shadowed_reads.namespace_whole_uses) {
            if import_references.contains(&span) {
                self.push_whole_object_use(name);
            }
        }
    }

    /// Record each pending namespace reference as a whole-object use. An
    /// extractor that no semantic pass resolves keeps the conservative credit.
    pub(crate) fn flush_pending_namespace_whole_object_uses(&mut self) {
        for (_, name) in std::mem::take(&mut self.shadowed_reads.namespace_whole_uses) {
            self.push_whole_object_use(name);
        }
    }

    pub(super) fn begin_resolution_phase(&mut self) {
        self.flush_pending_namespace_whole_object_uses();
        self.resolve_typed_destructure_bindings();
    }

    pub(crate) fn resolve_imported_call_sites(&mut self, references: &FxHashSet<Span>) {
        self.imported_call_sites.extend(
            self.pending_imported_call_sites
                .drain(..)
                .filter(|(span, _)| references.contains(span))
                .map(|(_, call)| call),
        );
    }

    pub(crate) fn set_import_binding_references(
        &mut self,
        references: Vec<ImportBindingReference>,
    ) {
        self.import_binding_references = references;
    }

    /// Store the facts of the semantic pass, which reads the value of each
    /// load with an unknown source.
    pub(crate) fn set_function_contracts(&mut self, facts: FunctionContractFacts) {
        self.function_contracts = facts;
        self.opaque_loads.spans.clear();
    }

    pub(crate) const fn set_template_reads_root(&mut self, value: bool) {
        self.opaque_loads.template_reads_root = value;
    }

    pub(crate) const fn template_reads_root(&self) -> bool {
        self.opaque_loads.template_reads_root
    }

    /// Record a declared name. An ambient `declare` of `require` adds no
    /// runtime binding, so the global loader stays visible.
    pub(super) fn record_declaration_name(&mut self, name: &str, ambient: bool) {
        self.opaque_loads.ambient_require |= ambient && name == "require";
        self.record_local_declaration_name(name);
    }

    /// The span of each `import()` or `require` call with an unknown source,
    /// for the semantic pass.
    pub(crate) fn opaque_load_candidates(&self) -> FxHashSet<Span> {
        self.opaque_loads.spans.clone()
    }

    /// Record an `import()` or a `require` call whose source gives no edge
    /// and no pattern. The call can load any module. Until the semantic pass
    /// reads the uses of the value, the value counts as an escape.
    pub(super) fn mark_opaque_module_load(&mut self, span: Span) {
        self.opaque_loads.spans.insert(span);
        self.function_contracts.opaque_loads.escapes = true;
    }

    /// Record a value that can load any module, such as `const r = require`,
    /// whose uses the facts do not follow. No node has the empty span, so
    /// the semantic pass cannot read this load and records an escape.
    pub(super) fn mark_opaque_load_escape(&mut self) {
        self.mark_opaque_module_load(Span::default());
    }

    /// Append the facts of this script block to `info`. Each index moves past
    /// the imports and contracts that earlier blocks added, so call this
    /// before the imports of the block are appended.
    pub(super) fn merge_function_contracts(&mut self, info: &mut ModuleInfo) {
        let incoming = std::mem::take(&mut self.function_contracts);
        if incoming.is_empty() {
            return;
        }
        let import_base = u32::try_from(info.imports.len()).unwrap_or(u32::MAX);
        let facts = info.function_contracts.get_or_insert_with(Box::default);
        let local_base = u32::try_from(facts.declarations.len()).unwrap_or(u32::MAX);
        facts.declarations.extend(incoming.declarations);
        facts
            .calls
            .extend(incoming.calls.into_iter().map(|mut calls| {
                calls.target = match calls.target {
                    CallTarget::Import(index) => {
                        CallTarget::Import(index.saturating_add(import_base))
                    }
                    CallTarget::Local(index) => CallTarget::Local(index.saturating_add(local_base)),
                };
                for site in &mut calls.sites {
                    merge_binding_args(&mut site.args, import_base);
                }
                calls.results.shift_loaders(local_base);
                calls
            }));
        facts.literal_consts.extend(incoming.literal_consts);
        facts
            .escapes
            .extend(incoming.escapes.into_iter().map(|mut escape| {
                escape.contract = escape.contract.saturating_add(local_base);
                escape
            }));
        facts.template_callers |= incoming.template_callers;
        facts.direct_eval |= incoming.direct_eval;
        let mut opaque_loads = incoming.opaque_loads;
        opaque_loads.shift_loaders(local_base);
        facts.opaque_loads.merge(opaque_loads);
        facts
            .calls
            .sort_by_key(|calls| calls.sites.first().map_or(0, |site| site.span_start));
        facts.escapes.sort_by_key(|escape| escape.span_start);
    }
}

/// Move each import index of a binding argument past `import_base`, and
/// make each local binding argument unknown.
///
/// Two script blocks of one host can each declare a `const` with the same
/// name and another value, so the name no longer identifies one binding
/// after the merge.
fn merge_binding_args(args: &mut [ArgFact], import_base: u32) {
    for arg in args {
        match arg {
            ArgFact::Binding(binding) => match &mut binding.root {
                BindingRoot::Import(index) => *index = index.saturating_add(import_base),
                BindingRoot::Local => *arg = ArgFact::Unknown,
            },
            ArgFact::Object(keys) => {
                for key in keys {
                    if let KeyValue::Binding(binding) = &mut key.value {
                        match &mut binding.root {
                            BindingRoot::Import(index) => {
                                *index = index.saturating_add(import_base);
                            }
                            BindingRoot::Local => key.value = KeyValue::Unknown,
                        }
                    }
                }
            }
            ArgFact::Literal(_) | ArgFact::Param(_) | ArgFact::Unknown => {}
        }
    }
}

/// Mark that a template of the host file can call a script binding, so the
/// facts do not hold each call.
pub fn mark_template_callers(info: &mut ModuleInfo) {
    info.function_contracts
        .get_or_insert_with(Box::default)
        .template_callers = true;
}
