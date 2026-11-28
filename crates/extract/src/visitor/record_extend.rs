//! Merge of template-scanned facts into a finished module record, and the
//! script binding context and script-only facts of a markup snippet merge.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use fallow_types::extract::SemanticFact;

use super::ModuleInfoExtractor;
use crate::template_usage::ScriptBindingContext;
use crate::{MemberAccess, ModuleInfo};

/// Append template-scanned member accesses to a finished module record,
/// skipping pairs the record already holds: a dotted tag inside an Astro
/// expression region is seen by both the template tag scan and the region
/// visitor, and no consumer counts repeats. The accesses live behind an
/// `Arc<[_]>` once the extractor is consumed, so the slice is rebuilt only when
/// there is something to add.
pub fn extend_member_accesses(info: &mut ModuleInfo, extra: Vec<MemberAccess>) {
    if extra.is_empty() {
        return;
    }
    let mut accesses = std::mem::take(&mut info.member_accesses).to_vec();
    let mut seen: FxHashSet<(String, String)> = accesses
        .iter()
        .map(|access| (access.object.clone(), access.member.clone()))
        .collect();
    for access in extra {
        if seen.insert((access.object.clone(), access.member.clone())) {
            accesses.push(access);
        }
    }
    info.member_accesses = accesses.into();
}

/// Append template-scanned whole-object uses to a finished module record, the
/// `whole_object_uses` counterpart of `extend_member_accesses`: names the
/// record already holds are skipped, so the region pass, the template guard,
/// and the script guard can each report the same binding without the
/// persisted record growing duplicates.
pub fn extend_whole_object_uses(info: &mut ModuleInfo, extra: Vec<String>) {
    if extra.is_empty() {
        return;
    }
    let mut uses = std::mem::take(&mut info.whole_object_uses).to_vec();
    for name in extra {
        if !uses.contains(&name) {
            uses.push(name);
        }
    }
    info.whole_object_uses = uses.into();
}

/// Append `added` to a shared list. The list is copied only when `added` has
/// items: a component merges one extractor per markup snippet, so an
/// unconditional copy costs the size of the list for each snippet.
pub(super) fn append_shared<T: Clone>(shared: &mut Arc<[T]>, added: &mut Vec<T>) {
    if added.is_empty() {
        return;
    }
    let mut merged = std::mem::take(shared).to_vec();
    merged.append(added);
    *shared = merged.into();
}

impl ModuleInfoExtractor {
    /// Give a markup snippet extractor the bindings that the script pass of
    /// the same component declares. A markup snippet has no declarations of
    /// its own, so without them a `dispatch("save")` call in markup is an
    /// ordinary call. The extractor reads the shared context and copies none
    /// of its bindings. A template local in `locals` (`{#each items as
    /// dispatch}`) shadows the script binding of the same name.
    pub(crate) fn use_script_binding_context(
        &mut self,
        context: Arc<ScriptBindingContext>,
        locals: &[&str],
    ) {
        self.markup.script = Some(context);
        self.markup.locals = locals.iter().map(|local| (*local).to_string()).collect();
    }

    /// Add the bindings of this script that its markup can call or read.
    pub(crate) fn extend_script_binding_context(&self, context: &mut ScriptBindingContext) {
        context
            .event_dispatch_bindings
            .extend(self.event_dispatch_bindings.iter().cloned());
        context.static_sink_literals.extend(
            self.module_static_sink_literals
                .iter()
                .map(|(name, value)| (name.clone(), value.clone())),
        );
    }

    /// Mark each declared emit that a markup snippet calls as used.
    pub(crate) fn merge_template_emits(&mut self, info: &mut ModuleInfo) {
        info.has_dynamic_emit |= self.markup.dynamic;
        if self.markup.emitted.is_empty() {
            return;
        }
        for emit in &mut info.component_emits {
            if self.markup.emitted.contains(&emit.name) {
                emit.used = true;
            }
        }
        self.markup.emitted.clear();
    }

    /// Keep only the imports whose specifier passes `keep`. Without
    /// `static_imports`, every static import goes: a classic HTML script with
    /// an `import` declaration is a syntax error in the browser.
    pub(crate) fn retain_import_specifiers(
        &mut self,
        static_imports: bool,
        keep: impl Fn(&str) -> bool,
    ) {
        if static_imports {
            self.imports.retain(|import| keep(&import.source));
        } else {
            self.imports.clear();
        }
        self.dynamic_imports.retain(|import| keep(&import.source));
        self.dynamic_import_patterns
            .retain(|pattern| keep(&pattern.prefix));
    }

    /// Clear the facts that apply only in a module script, before a markup
    /// snippet or an inline HTML script merges into its host through
    /// [`Self::merge_into`]. This is the one list of such facts: every other
    /// fact goes to the host.
    ///
    /// - Markup and an HTML page cannot export. A top-level declaration of an
    ///   inline HTML script is a page global.
    /// - The snippet wrapper declares the template locals as `const` bindings
    ///   and wraps a statement in a function. The declaration facts of these
    ///   wrappers (local types, value aliases, function contracts, class
    ///   heritage) describe the wrapper, not the component.
    /// - Module directives, top-level runtime statements, server actions and
    ///   SvelteKit `load()` returns are facts of a module program only. The
    ///   wrapper of a snippet is a top-level statement of its own program.
    /// - Markup and an HTML page do not run as CommonJS modules. A `require()`
    ///   call there (a UMD guard in an inline script) loads no module, and
    ///   `module.exports` exports nothing.
    pub(crate) fn clear_script_only_facts(&mut self) {
        self.require_calls.clear();
        self.has_cjs_exports = false;
        self.cjs_single_static_object_map = false;
        self.semantic_facts
            .retain(|fact| !matches!(fact, SemanticFact::TopLevelRuntimeStatement));
        self.exports.clear();
        self.re_exports.clear();
        self.exported_import_equals_names.clear();
        self.inert_function_exports.clear();
        self.local_type_declarations.clear();
        self.public_signature_type_references.clear();
        self.local_value_aliases.clear();
        self.function_contracts = fallow_types::extract::FunctionContractFacts::default();
        self.class_heritage.clear();
        self.directives.clear();
        self.misplaced_directives.clear();
        self.inline_server_action_exports.clear();
        self.load_return_keys.clear();
        self.has_unharvestable_load = false;
    }
}
