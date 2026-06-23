//! Heuristic template scanners for frameworks whose templates carry import
//! and class-member references the JavaScript AST cannot see. Intentionally
//! conservative: we support the common template constructs that can be
//! analysed reliably with lightweight scanning, without pretending to be a
//! full framework compiler.
//!
//! Four scanners live here, dispatched in two different ways:
//!
//! - **Vue (`vue`) / Svelte (`svelte`)**: `.vue` / `.svelte` files are not
//!   valid JS until their `<script>` blocks are extracted, so `crate::sfc`
//!   runs a per-SFC pipeline that builds a fresh `ModuleInfo`, then folds in
//!   per-script and per-style results. These two share the `SfcKind` enum
//!   and the `collect_template_usage_with_bound_targets` dispatcher below.
//!
//! - **Angular (`angular`)**: Angular components live in regular `.ts`
//!   files; templates appear as decorator metadata (`template: \`...\``) or
//!   external `.html` siblings (`templateUrl`). The scanner is invoked
//!   directly from `crate::visitor::visit_impl::visit_class` for inline
//!   templates and from `crate::html::parse_html_to_module_with_complexity`
//!   for external templates. Bare identifier references use an
//!   `ANGULAR_TPL_SENTINEL` object name so the analysis phase can bridge
//!   them to the importing component's class members.
//!
//! - **Glimmer (`glimmer`)**: Ember `.gts` / `.gjs` single-file components.
//!   The host file IS valid JS once `<template>...</template>` blocks are
//!   blanked by `crate::glimmer::strip_glimmer_templates`, so the standard
//!   `crate::parse::parse_source_to_module` pipeline handles parsing. The
//!   Glimmer scanner is invoked from `parse.rs::
//!   collect_glimmer_template_into_extractor` against the un-stripped source
//!   AFTER `extractor.visit_program(...)` but BEFORE
//!   `compute_import_binding_usage`. Results push directly onto
//!   `extractor.member_accesses` (matching Angular's flow) and feed
//!   `compute_import_binding_usage`'s `template_used` skip-set so
//!   template-only imports never enter the `unused` vector.
//!
//! Angular and Glimmer do not participate in `SfcKind` / the dispatcher
//! below because they don't need per-file `ModuleInfo` construction:
//! their host files are normal `.ts` / `.gts` / `.gjs` sources.

pub mod angular;
pub mod glimmer;
mod scanners;
mod shared;
mod svelte;
mod vue;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::template_usage::TemplateUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SfcKind {
    /// Vue single-file components.
    Vue,
    /// Svelte single-file components.
    Svelte,
}

/// Collect template-visible import usage from Vue or Svelte markup.
#[cfg(test)]
pub fn collect_template_usage(
    kind: SfcKind,
    source: &str,
    imported_bindings: &FxHashSet<String>,
) -> TemplateUsage {
    match kind {
        SfcKind::Vue => vue::collect_template_usage(source, imported_bindings),
        SfcKind::Svelte => svelte::collect_template_usage(source, imported_bindings),
    }
}

/// Collect Svelte custom-event listener names from template `on:<name>`
/// bindings on component tags (PascalCase). DOM-element `on:click` is excluded.
/// Feeds the `unused-svelte-event` detector's liberal project-wide listened set.
#[must_use]
pub fn collect_svelte_listened_events(source: &str) -> Vec<String> {
    svelte::collect_listened_events(source)
}

/// Collect template-visible usage, including framework template references to
/// script-local instance bindings such as `const counter = new Counter()`.
pub fn collect_template_usage_with_bound_targets(
    kind: SfcKind,
    source: &str,
    imported_bindings: &FxHashSet<String>,
    bound_targets: &FxHashMap<String, String>,
) -> TemplateUsage {
    match kind {
        SfcKind::Vue => {
            vue::collect_template_usage_with_bound_targets(source, imported_bindings, bound_targets)
        }
        SfcKind::Svelte => svelte::collect_template_usage_with_bound_targets(
            source,
            imported_bindings,
            bound_targets,
        ),
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    fn imported(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn svelte_template_usage_marks_named_imports_used() {
        let usage = collect_template_usage(
            SfcKind::Svelte,
            "<script>import { formatDate } from './utils';</script><p>{formatDate(value)}</p>",
            &imported(&["formatDate"]),
        );

        assert!(usage.used_bindings.contains("formatDate"));
    }

    #[test]
    fn svelte_template_usage_retains_namespace_members() {
        let usage = collect_template_usage(
            SfcKind::Svelte,
            "<script>import * as utils from './utils';</script><p>{utils.formatDate(value)}</p>",
            &imported(&["utils"]),
        );

        assert!(usage.used_bindings.contains("utils"));
        assert_eq!(usage.member_accesses.len(), 1);
        assert_eq!(usage.member_accesses[0].object, "utils");
        assert_eq!(usage.member_accesses[0].member, "formatDate");
    }

    #[test]
    fn vue_template_usage_marks_named_imports_used() {
        let usage = collect_template_usage(
            SfcKind::Vue,
            "<script setup>import { formatDate } from './utils';</script><template><p>{{ formatDate(value) }}</p></template>",
            &imported(&["formatDate"]),
        );

        assert!(usage.used_bindings.contains("formatDate"));
    }

    #[test]
    fn vue_template_usage_treats_event_handlers_as_statements() {
        let usage = collect_template_usage(
            SfcKind::Vue,
            "<script setup>import { increment } from './utils';</script><template><button @click=\"count += increment(step)\">Add</button></template>",
            &imported(&["increment"]),
        );

        assert!(usage.used_bindings.contains("increment"));
    }
}
