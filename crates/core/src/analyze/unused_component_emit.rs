//! Detection of unused Vue `<script setup>` `defineEmits` events: a declared
//! emit event EMITTED nowhere inside its own single-file component (no
//! `emit('<name>')` call). Script-only: emits are called in script via
//! `emit('x')`, never in the template (the template `@event` is the PARENT
//! listening, not an emit).
//!
//! Single-file finding, zero-FP doctrine. The harvest + usage flag lives on
//! `ModuleInfo.component_emits` (set during extraction); this detector only reads
//! it, applies the dep gate and the whole-file abstain ladder, and emits one
//! finding per genuinely-unused event.
//!
//! Abstain ladder (each abstains the WHOLE file's emit findings):
//! - not `<script setup>`: no `component_emits` harvested (the field is empty).
//! - `has_unharvestable_emits`: a type-reference `defineEmits<MyEmits>()` arg, a
//!   non-literal runtime form, or an unbound bare `defineEmits([...])`.
//! - `has_dynamic_emit`: an `emit(<nonLiteral>)` call (event unknowable).
//! - `has_emit_whole_object_use`: the emit binding passed / returned / spread.
//! - `has_define_model`: `defineModel` creates implicit `update:x` emits, so a
//!   file with `defineModel` must abstain emits too (reuses the props flag).

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::UnusedComponentEmit;

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Find Vue `<script setup>` `defineEmits` events emitted nowhere in their own
/// SFC. Returns empty unless the project declares `vue` / `@vue/runtime-core` /
/// `nuxt`.
#[must_use]
pub fn find_unused_component_emits(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnusedComponentEmit> {
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
        if module.component_emits.is_empty() {
            continue;
        }
        // Whole-file abstain ladder: any signal that an event could be emitted
        // indirectly skips the file (zero-FP doctrine). `defineModel` creates
        // implicit `update:x` emits.
        if module.has_unharvestable_emits
            || module.has_dynamic_emit
            || module.has_emit_whole_object_use
            || module.has_define_model
        {
            continue;
        }

        let component_name = component_name_for(&node.path);
        for emit in &module.component_emits {
            if emit.used {
                continue;
            }
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, node.file_id, emit.span_start);
            findings.push(UnusedComponentEmit {
                path: node.path.clone(),
                component_name: component_name.clone(),
                emit_name: emit.name.clone(),
                line,
                col,
            });
        }
    }

    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.emit_name.cmp(&b.emit_name))
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
