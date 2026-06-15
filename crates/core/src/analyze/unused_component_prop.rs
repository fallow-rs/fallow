//! Detection of unused Vue `<script setup>` `defineProps` props: a declared prop
//! referenced NOWHERE inside its own single-file component (neither `<script>`
//! nor `<template>`).
//!
//! Single-file finding, zero-FP doctrine. The harvest + usage flags live on
//! `ModuleInfo.component_props` (set during extraction); this detector only reads
//! them, applies the dep gate and the whole-file abstain ladder, and emits one
//! finding per genuinely-unused prop.
//!
//! Abstain ladder (each abstains the WHOLE file's prop findings):
//! - `has_unharvestable_props`: a type-reference `defineProps<Props>()` arg.
//! - `has_props_attrs_fallthrough`: `v-bind="$attrs"/$props/props"` or a
//!   rest-destructure of the props return.
//! - `has_define_expose`: a prop may be re-exposed.
//! - `has_define_model`: two-way model props are out of scope for v1.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::UnusedComponentProp;

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Find Vue `<script setup>` `defineProps` props referenced nowhere in their own
/// SFC. Returns empty unless the project declares `vue` / `@vue/runtime-core` /
/// `nuxt`.
#[must_use]
pub fn find_unused_component_props(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnusedComponentProp> {
    let gated = declared_deps.contains("vue")
        || declared_deps.contains("@vue/runtime-core")
        || declared_deps.contains("nuxt");
    if !gated {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let mut findings = Vec::new();
    for node in &graph.modules {
        if !node.is_reachable() {
            continue;
        }
        if !is_vue_file(&node.path) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        if module.component_props.is_empty() {
            continue;
        }
        // Whole-file abstain ladder: any signal that a prop could be consumed
        // indirectly skips the file (zero-FP doctrine).
        if module.has_unharvestable_props
            || module.has_props_attrs_fallthrough
            || module.has_define_expose
            || module.has_define_model
        {
            continue;
        }

        let component_name = component_name_for(&node.path);
        for prop in &module.component_props {
            if prop.used_in_script || prop.used_in_template {
                continue;
            }
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, node.file_id, prop.span_start);
            findings.push(UnusedComponentProp {
                path: node.path.clone(),
                component_name: component_name.clone(),
                prop_name: prop.name.clone(),
                line,
                col,
            });
        }
    }

    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.prop_name.cmp(&b.prop_name))
    });
    findings
}

/// Whether the path is a Vue SFC (`.vue`).
fn is_vue_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("vue")
}

/// The component name: the `.vue` file stem.
fn component_name_for(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}
