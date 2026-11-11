//! Store the call and reference facts that the semantic pass resolves, and
//! merge the call evidence across the script blocks of one host file.

use fallow_types::extract::{
    ArgFact, BindingArg, BindingRoot, CallTarget, FunctionContractFacts, ImportBindingReference,
    KeyValue,
};
use oxc_span::Span;
use rustc_hash::FxHashSet;

use super::{ModuleInfo, ModuleInfoExtractor};

impl ModuleInfoExtractor {
    pub(crate) fn imported_call_reference_candidates(&self) -> FxHashSet<Span> {
        self.pending_imported_call_sites
            .iter()
            .map(|(span, _)| *span)
            .collect()
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

    pub(crate) fn set_function_contracts(&mut self, facts: FunctionContractFacts) {
        self.function_contracts = facts;
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
                    shift_binding_imports(&mut site.args, import_base);
                }
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
        facts
            .calls
            .sort_by_key(|calls| calls.sites.first().map_or(0, |site| site.span_start));
        facts.escapes.sort_by_key(|escape| escape.span_start);
    }
}

/// Move each import index of a binding argument past `import_base`.
fn shift_binding_imports(args: &mut [ArgFact], import_base: u32) {
    let shift = |binding: &mut BindingArg| {
        if let BindingRoot::Import(index) = &mut binding.root {
            *index = index.saturating_add(import_base);
        }
    };
    for arg in args {
        match arg {
            ArgFact::Binding(binding) => shift(binding),
            ArgFact::Object(keys) => {
                for key in keys {
                    if let KeyValue::Binding(binding) = &mut key.value {
                        shift(binding);
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
