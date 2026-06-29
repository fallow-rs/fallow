//! Detection of unused Vue `<script setup>` `defineProps` props, Svelte 5
//! `$props()` props, and Astro `interface Props` fields: a declared prop
//! referenced NOWHERE inside its own single-file component (neither `<script>` /
//! frontmatter nor `<template>` / markup). Astro props are harvested from the
//! frontmatter `interface Props { ... }` and credited via `Astro.props`
//! destructure / member access / template `{prop}` usage.
//!
//! Single-file finding, zero-FP doctrine. The harvest + usage flags live on
//! `ModuleInfo.component_props` (set during extraction); this detector only reads
//! them, applies the dep gate and the whole-file abstain ladder, and emits one
//! finding per genuinely-unused prop.
//!
//! Abstain ladder (each abstains the WHOLE file's prop findings):
//! - `has_unharvestable_props`: opaque or imported prop declarations.
//! - `has_props_attrs_fallthrough`: `v-bind="$attrs"/$props/props"` or a
//!   rest-destructure of the props return.
//! - `has_define_expose`: a prop may be re-exposed.
//! - `has_define_model`: two-way model props are out of scope for v1.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::results::UnusedComponentProp;

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Result of the SFC (Vue/Svelte/Astro) prop scan: the findings plus the count
/// of props exempted by `unusedComponentProps.ignorePattern` (drives the
/// human-output note so a typo'd pattern matching nothing is not a silent no-op).
#[derive(Debug, Default)]
pub struct SfcPropScan {
    pub findings: Vec<UnusedComponentProp>,
    pub exempted: usize,
}

/// Find Vue `<script setup>` `defineProps` and Svelte 5 `$props()` props
/// referenced nowhere in their own SFC. Returns framework findings only when
/// the matching framework dependency is declared.
#[must_use]
pub fn find_unused_component_props(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    ignore_pattern: Option<&regex::Regex>,
) -> SfcPropScan {
    let vue_gated = declared_deps.contains("vue")
        || declared_deps.contains("@vue/runtime-core")
        || declared_deps.contains("nuxt");
    let svelte_gated = declared_deps.contains("svelte") || declared_deps.contains("@sveltejs/kit");
    let astro_gated = declared_deps.contains("astro");
    if !vue_gated && !svelte_gated && !astro_gated {
        return SfcPropScan::default();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let mut scan = SfcPropScan::default();
    for node in &graph.modules {
        if !node.is_reachable() {
            continue;
        }
        let Some(framework) = component_prop_framework(&node.path) else {
            continue;
        };
        match framework {
            ComponentPropFramework::Vue if !vue_gated => continue,
            ComponentPropFramework::Svelte if !svelte_gated => continue,
            ComponentPropFramework::Astro if !astro_gated => continue,
            _ => {}
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        collect_module_unused_sfc_props(
            node,
            module,
            line_offsets_by_file,
            ignore_pattern,
            &mut scan,
        );
    }

    scan.findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.prop_name.cmp(&b.prop_name))
    });
    scan
}

fn collect_module_unused_sfc_props(
    node: &ModuleNode,
    module: &ModuleInfo,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    ignore_pattern: Option<&regex::Regex>,
    scan: &mut SfcPropScan,
) {
    if module.component_props.is_empty() {
        return;
    }
    // Whole-file abstain ladder: any signal that a prop could be consumed
    // indirectly skips the file (zero-FP doctrine).
    if module.has_unharvestable_props
        || module.has_props_attrs_fallthrough
        || module.has_define_expose
        || module.has_define_model
    {
        return;
    }

    let component_name = component_name_for(&node.path);
    for prop in &module.component_props {
        if prop.used_in_script || prop.used_in_template {
            continue;
        }
        // Opt-in `unusedComponentProps.ignorePattern`: exempt props whose LOCAL
        // destructure binding name matches (the ESLint `varsIgnorePattern`
        // convention keys on the local binding, e.g. `_stage`, which falls back
        // to the public name when there is no alias).
        if matches_ignore_pattern(ignore_pattern, &prop.local) {
            scan.exempted += 1;
            continue;
        }
        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, node.file_id, prop.span_start);
        scan.findings.push(UnusedComponentProp {
            path: node.path.clone(),
            component_name: component_name.clone(),
            prop_name: prop.name.clone(),
            line,
            col,
        });
    }
}

/// Whether a prop's local destructure binding name matches the opt-in
/// `unusedComponentProps.ignorePattern`. Matching is unanchored (`is_match`,
/// substring), mirroring ESLint's `RegExp.test`; users anchor with `^_`.
fn matches_ignore_pattern(ignore_pattern: Option<&regex::Regex>, local: &str) -> bool {
    ignore_pattern.is_some_and(|re| re.is_match(local))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ComponentPropFramework {
    Vue,
    Svelte,
    Astro,
}

/// Whether the path is an SFC whose props feed `unused-component-prop`. `.astro`
/// joins `.vue` / `.svelte`: its `interface Props` declaration + `Astro.props`
/// usage are harvested into the same `ComponentProp` IR + abstain flags during
/// extraction.
fn component_prop_framework(path: &Path) -> Option<ComponentPropFramework> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("vue") => Some(ComponentPropFramework::Vue),
        Some("svelte") => Some(ComponentPropFramework::Svelte),
        Some("astro") => Some(ComponentPropFramework::Astro),
        _ => None,
    }
}

/// The component name: the SFC file stem.
fn component_name_for(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Whether the path is a React/Preact JSX module (`.jsx` / `.tsx`). `.js` / `.ts`
/// files re-parsed through the JSX retry path also carry React IR, but the prop
/// arm scopes to the canonical JSX extensions to keep the surface tight in v1.
fn is_react_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jsx" | "tsx")
    )
}

/// Result of the React prop scan: the findings plus the number of React
/// components inspected (for the "React detected, N components scanned"
/// diagnostic, so a silent dep-gate or silent abstain is observable).
#[derive(Debug, Default)]
pub struct ReactPropScan {
    /// Per-prop unused-component-prop findings.
    pub findings: Vec<UnusedComponentProp>,
    /// React components inspected across all reachable JSX modules.
    pub components_scanned: usize,
    /// Props exempted by `unusedComponentProps.ignorePattern` (drives the
    /// human-output note; see [`SfcPropScan::exempted`]).
    pub exempted: usize,
}

/// Find React/Preact component props declared on a component but read NOWHERE in
/// that component's body (the inline-destructured-literal-prop v1 scope). Returns
/// an empty scan unless the project declares `react` / `react-dom` / `next` /
/// `preact`.
///
/// React is just another producer of the SAME `unused-component-prop` finding:
/// it emits into `results.unused_component_props` alongside the Vue arm.
///
/// Abstain ladder (zero-FP, mirrors Vue's whole-file `has_unharvestable_props`,
/// but PER-COMPONENT because a `.tsx` file can declare several components):
/// - `ComponentFunction.has_unharvestable_props`: a rest/spread param
///   (`{ ...rest }` / a trailing rest parameter), a bare-identifier props param
///   (the `forwardRef<T, Props>` / `memo` imported-interface case, ADR-001), an
///   array-pattern param, a computed prop key, or a nested destructure fallow
///   cannot flatten with confidence. The whole component abstains.
/// - `ComponentFunction.is_exported`: a prop on an EXPORTED component is part of
///   its public contract (consumers pass it; the component need not read it).
///   The whole component abstains, reusing the public-API posture the
///   `unrendered-component` / entry-export logic takes.
#[must_use]
pub fn find_unused_react_props(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    ignore_pattern: Option<&regex::Regex>,
) -> ReactPropScan {
    if !has_react_runtime_dep(declared_deps) {
        return ReactPropScan::default();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let mut scan = ReactPropScan::default();
    for node in &graph.modules {
        if !node.is_reachable() {
            continue;
        }
        if !is_react_file(&node.path) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        collect_module_unused_react_props(
            node,
            module,
            line_offsets_by_file,
            ignore_pattern,
            &mut scan,
        );
    }

    scan.findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.component_name.cmp(&b.component_name))
            .then(a.prop_name.cmp(&b.prop_name))
    });
    scan
}

fn has_react_runtime_dep(declared_deps: &FxHashSet<String>) -> bool {
    declared_deps.contains("react")
        || declared_deps.contains("react-dom")
        || declared_deps.contains("next")
        || declared_deps.contains("preact")
}

fn collect_module_unused_react_props(
    node: &ModuleNode,
    module: &ModuleInfo,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    ignore_pattern: Option<&regex::Regex>,
    scan: &mut ReactPropScan,
) {
    if module.component_functions.is_empty() {
        return;
    }
    scan.components_scanned += module.component_functions.len();
    if module.react_props.is_empty() {
        return;
    }

    let abstained = react_prop_abstained_components(module);
    for prop in &module.react_props {
        if prop.used_in_script {
            continue;
        }
        if abstained.contains(prop.component.as_str()) {
            continue;
        }
        // Opt-in `unusedComponentProps.ignorePattern`: exempt props whose local
        // binding name matches (the React arm's `ComponentProp.local` is the
        // destructure alias, e.g. `_props`).
        if matches_ignore_pattern(ignore_pattern, &prop.local) {
            scan.exempted += 1;
            continue;
        }
        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, node.file_id, prop.span_start);
        scan.findings.push(UnusedComponentProp {
            path: node.path.clone(),
            component_name: prop.component.clone(),
            prop_name: prop.name.clone(),
            line,
            col,
        });
    }
}

fn react_prop_abstained_components(module: &ModuleInfo) -> FxHashSet<&str> {
    // Per-component abstain set: a component whose props are unharvestable
    // (rest/spread, bare props param, computed/nested key) or that is exported
    // (public contract) flags no props. Built once per file.
    module
        .component_functions
        .iter()
        .filter(|c| c.has_unharvestable_props || c.is_exported)
        .map(|c| c.name.as_str())
        .collect()
}
