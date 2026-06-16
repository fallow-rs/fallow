//! Detection of unused Angular component/directive outputs: an `@Output()` /
//! signal `output()` declared output EMITTED nowhere inside its own component
//! (no `this.<output>.emit(...)`).
//!
//! Single-file dead-output direction, the Angular analogue of the Vue
//! `unused-component-emit` rule. The harvest lives on
//! `ModuleInfo.angular_outputs` (set during extraction; a `model()` is recorded
//! as an input only, so its framework-driven `update:` emit never appears here).
//! This detector reads the harvest, applies the `@angular/core` dep gate and the
//! whole-component extends-abstain, and emits one finding per genuinely-unused
//! output.
//!
//! Usage is detected by over-crediting (every ambiguous shape credits toward
//! "used", so only false negatives can result, never false positives). An output
//! `bar` is USED if ANY hold:
//! - a `member_access` with `object == "this.bar" && member == "emit"` (the
//!   `this.bar.emit(...)` call site; reading `.emit` without calling is a
//!   negligible shape treated as a call);
//! - a `member_access` with `object == "this" && member == bar` (the output read
//!   as a value, e.g. forwarded to a function that may emit it). Over-credit;
//! - an inline-template sentinel ref (`object == ANGULAR_TPL_SENTINEL &&
//!   member == bar`), which credits a template-handler emit such as
//!   `(click)="bar.emit(...)"` (Angular templates emit outputs directly off the
//!   bare name, with no `this.` prefix);
//! - the same sentinel ref in the linked external `templateUrl` `.html` module,
//!   reached via the `SideEffect` import edge.
//!
//! Whole-component ABSTAIN (skip ALL outputs for the component) when the
//! component class declares an `extends` heritage clause: a base class in another
//! file may emit `this.bar`, invisible to the per-module scan. Same conservative
//! `super_class`-present signal as the input detector.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_extract::ANGULAR_TPL_SENTINEL;
use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::UnusedComponentOutput;

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Find Angular component/directive outputs emitted nowhere in their own
/// component. Returns empty unless the project declares `@angular/core`.
#[must_use]
pub fn find_unused_component_outputs(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnusedComponentOutput> {
    if !declared_deps.contains("@angular/core") {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let mut findings = Vec::new();
    for node in &graph.modules {
        if !node.is_reachable() {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        if module.angular_outputs.is_empty() {
            continue;
        }
        // Whole-component abstain: a base class in another file may emit the
        // output through `this.bar.emit()` (zero-FP doctrine).
        if component_has_extends(module) {
            continue;
        }
        // Whole-component abstain: `{ ...this }` forwards every output opaquely
        // into a behavior pattern (the Angular headless-pattern convention).
        if super::unused_component_input::component_spreads_this(module) {
            continue;
        }

        // Linked external `templateUrl` module(s): a `(click)="bar.emit()"`
        // handler in the `.html` file emits the output too.
        let external_templates = external_template_modules(graph, &modules_by_id, node.file_id);
        let template_emitted = template_emitted_outputs(module, &external_templates);

        let component_name = component_name_for(&node.path);
        for output in &module.angular_outputs {
            if output_is_emitted(module, &output.name)
                || template_emitted.contains(output.name.as_str())
            {
                continue;
            }
            // A template emit off a JS reserved word (`(delete)="..."` ->
            // `delete.emit(...)`) is invisible to the JS-based expression scanner,
            // which reads `delete` as the operator. Abstain to stay zero-FP.
            if super::unused_component_input::is_js_reserved_word(&output.name) {
                continue;
            }
            let (line, col) =
                byte_offset_to_line_col(line_offsets_by_file, node.file_id, output.span_start);
            findings.push(UnusedComponentOutput {
                path: node.path.clone(),
                component_name: component_name.clone(),
                output_name: output.name.clone(),
                line,
                col,
            });
        }
    }

    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.output_name.cmp(&b.output_name))
    });
    findings
}

/// Whether the output `name` is emitted (or forwarded) somewhere in its own
/// component. Over-credits: `this.<name>.emit(...)` is the canonical emit, and a
/// bare `this.<name>` value read credits too (it may be forwarded to a function
/// that emits it).
fn output_is_emitted(component: &ModuleInfo, name: &str) -> bool {
    let emit_object = format!("this.{name}");
    component.member_accesses.iter().any(|access| {
        (access.object == emit_object && access.member == "emit")
            || (access.object == "this" && access.member == name)
    })
}

/// Build the set of output names emitted through a template handler. An Angular
/// template emits an output off the bare name (`(click)="bar.emit(...)"`), which
/// the template scanner records as an `ANGULAR_TPL_SENTINEL` member access for
/// `bar`. This covers the component's own inline template plus every linked
/// external `templateUrl` module. Over-credits by design.
fn template_emitted_outputs<'a>(
    component: &'a ModuleInfo,
    external_templates: &[&'a ModuleInfo],
) -> FxHashSet<&'a str> {
    let mut emitted: FxHashSet<&str> = FxHashSet::default();
    for access in &component.member_accesses {
        if access.object == ANGULAR_TPL_SENTINEL {
            emitted.insert(access.member.as_str());
        }
    }
    for template in external_templates {
        for access in &template.member_accesses {
            if access.object == ANGULAR_TPL_SENTINEL {
                emitted.insert(access.member.as_str());
            }
        }
    }
    emitted
}

/// The `.ts` modules reached from `from` by a `SideEffect`-shaped edge that hold
/// an external Angular `templateUrl`. Mirrors the input detector's helper: the
/// component sets `has_angular_component_template_url`, and the external `.html`
/// file is reached via a `SideEffect` import edge whose target carries template
/// sentinel member accesses.
fn external_template_modules<'a>(
    graph: &ModuleGraph,
    modules_by_id: &FxHashMap<FileId, &'a ModuleInfo>,
    from: FileId,
) -> Vec<&'a ModuleInfo> {
    let Some(component) = modules_by_id.get(&from) else {
        return Vec::new();
    };
    if !component.has_angular_component_template_url {
        return Vec::new();
    }
    let mut out = Vec::new();
    for target in graph.edges_for(from) {
        let Some(target_module) = modules_by_id.get(&target) else {
            continue;
        };
        if target_module
            .member_accesses
            .iter()
            .any(|a| a.object == ANGULAR_TPL_SENTINEL)
        {
            out.push(*target_module);
        }
    }
    out
}

/// Whether the component declares an `extends` heritage clause anywhere in its
/// module. Conservative: any exported class with a `super_class`, or any
/// `class_heritage` entry with a `super_class`, abstains the whole component.
fn component_has_extends(module: &ModuleInfo) -> bool {
    module.exports.iter().any(|e| e.super_class.is_some())
        || module
            .class_heritage
            .iter()
            .any(|h| h.super_class.is_some())
}

/// The component name: the file stem (e.g. `user-card` for `user-card.ts`).
fn component_name_for(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use fallow_types::extract::{AngularOutputMember, ClassHeritageInfo, MemberAccess};
    use rustc_hash::FxHashSet;

    use super::*;
    use crate::analyze::test_support::empty_module;

    fn output(name: &str, span: u32) -> AngularOutputMember {
        AngularOutputMember {
            name: name.to_string(),
            span_start: span,
        }
    }

    fn access(object: &str, member: &str) -> MemberAccess {
        MemberAccess {
            object: object.to_string(),
            member: member.to_string(),
        }
    }

    #[test]
    fn unemitted_output_is_not_emitted() {
        let component = ModuleInfo {
            angular_outputs: vec![output("changed", 10)],
            ..empty_module()
        };
        assert!(
            !output_is_emitted(&component, "changed"),
            "an output emitted nowhere is reported"
        );
    }

    #[test]
    fn emitted_output_is_credited() {
        let component = ModuleInfo {
            angular_outputs: vec![output("changed", 10)],
            member_accesses: vec![access("this.changed", "emit")],
            ..empty_module()
        };
        assert!(
            output_is_emitted(&component, "changed"),
            "a `this.changed.emit(...)` call credits the output"
        );
    }

    #[test]
    fn inline_template_emit_credits_output() {
        // `(click)="changed.emit(...)"` records a sentinel member access for the
        // bare output name (no `this.`); it must credit the output as emitted.
        let component = ModuleInfo {
            angular_outputs: vec![output("changed", 10)],
            member_accesses: vec![access(ANGULAR_TPL_SENTINEL, "changed")],
            ..empty_module()
        };
        let emitted = template_emitted_outputs(&component, &[]);
        assert!(
            emitted.contains("changed"),
            "an inline-template handler emit must credit the output"
        );
        assert!(
            !output_is_emitted(&component, "changed"),
            "the template sentinel is not a `this.changed.emit` script call"
        );
    }

    #[test]
    fn forwarded_output_value_read_is_credited() {
        let component = ModuleInfo {
            angular_outputs: vec![output("changed", 10)],
            member_accesses: vec![access("this", "changed")],
            ..empty_module()
        };
        assert!(
            output_is_emitted(&component, "changed"),
            "a `this.changed` value read (forwarded) over-credits the output"
        );
    }

    #[test]
    fn extends_abstain_holds() {
        let component = ModuleInfo {
            angular_outputs: vec![output("changed", 10)],
            class_heritage: vec![ClassHeritageInfo {
                export_name: "Foo".to_string(),
                super_class: Some("Base".to_string()),
                implements: Vec::new(),
                instance_bindings: Vec::new(),
            }],
            ..empty_module()
        };
        assert!(
            component_has_extends(&component),
            "an `extends` clause abstains the whole component"
        );
    }

    #[test]
    fn dep_gate_returns_empty_without_angular_core() {
        let graph = ModuleGraph::build(&[], &[], &[]);
        let modules = Vec::new();
        let declared: FxHashSet<String> = std::iter::once("react".to_string()).collect();
        let offsets = LineOffsetsMap::default();
        let findings = find_unused_component_outputs(&graph, &modules, &declared, &offsets);
        assert!(
            findings.is_empty(),
            "no `@angular/core` dependency means no findings"
        );
    }
}
