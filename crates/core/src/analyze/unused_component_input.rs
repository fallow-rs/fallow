//! Detection of unused Angular component/directive inputs: an `@Input()` /
//! signal `input()` / `model()` declared input read NOWHERE inside its own
//! component (neither the inline/external template nor the class body).
//!
//! Single-file dead-input direction, the Angular analogue of the Vue
//! `unused-component-prop` rule. The harvest lives on
//! `ModuleInfo.angular_inputs` (set during extraction); this detector only
//! reads it, applies the `@angular/core` dep gate and the whole-component
//! extends-abstain, and emits one finding per genuinely-unused input.
//!
//! Usage is detected by over-crediting (every ambiguous shape credits toward
//! "used", so only false negatives can result, never false positives). An input
//! `foo` is USED if ANY hold:
//! - a typed Angular template member fact for `foo`, with older cached
//!   member accesses accepted only as a conservative parse-cache fallback; inline
//!   templates, host bindings, and `inputs:` / `outputs:` metadata arrays all
//!   emit this template member evidence in the component's own module;
//! - the component has `has_angular_component_template_url` and the linked
//!   external `.html` module (reached via the `SideEffect` import edge) has such
//!   template member evidence for `foo`;
//! - ANY `member_access` in this module with `member == foo` regardless of
//!   object (credits `this.foo`, `changes.foo` / `changes['foo']` in
//!   `ngOnChanges`, destructured reads); broad on purpose, to kill the
//!   ngOnChanges-by-name false positive without a blanket abstain.
//!
//! Whole-component ABSTAIN (skip ALL inputs for the component) when the
//! component class declares an `extends` heritage clause: a base class in
//! another file may read `this.foo`, and cross-file inheritance is invisible to
//! the per-module usage scan. The signal used is any `super_class` present on an
//! exported class (`ExportInfo.super_class`) or in `class_heritage`
//! (`ClassHeritageInfo.super_class`). This intentionally cannot tell a resolved
//! same-file base from an unresolved cross-file one, so it abstains whenever ANY
//! heritage `extends` is present (the conservative, zero-FP-leaning direction
//! documented in the design review).

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ModuleInfo, SemanticFactView};

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::results::UnusedComponentInput;

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Find Angular component/directive inputs read nowhere in their own component.
/// Returns empty unless the project declares `@angular/core`.
#[must_use]
pub fn find_unused_component_inputs(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnusedComponentInput> {
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
        collect_module_unused_component_inputs(
            node,
            module,
            graph,
            &modules_by_id,
            line_offsets_by_file,
            &mut findings,
        );
    }

    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.input_name.cmp(&b.input_name))
    });
    findings
}

fn collect_module_unused_component_inputs(
    node: &ModuleNode,
    module: &ModuleInfo,
    graph: &ModuleGraph,
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    findings: &mut Vec<UnusedComponentInput>,
) {
    if module.angular_inputs.is_empty() || component_abstains_inputs(module) {
        return;
    }

    // Collect linked external `templateUrl` module(s) so template evidence in
    // the `.html` file credits the input too.
    let external_templates = external_template_modules(graph, modules_by_id, node.file_id);
    let used = input_usage_set(module, &external_templates);

    let component_name = component_name_for(&node.path);
    for input in &module.angular_inputs {
        if used.contains(input.name.as_str()) || is_js_reserved_word(&input.name) {
            continue;
        }
        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, node.file_id, input.span_start);
        findings.push(UnusedComponentInput {
            path: node.path.clone(),
            component_name: component_name.clone(),
            input_name: input.name.clone(),
            line,
            col,
        });
    }
}

fn component_abstains_inputs(module: &ModuleInfo) -> bool {
    // A base class in another file may read the input through `this.foo`.
    if component_has_extends(module) {
        return true;
    }

    // `{ ...this }` forwards every input opaquely into a behavior pattern.
    component_spreads_this(module)
}

/// Build the set of input names that are USED by the component, unioning the
/// component's ordinary member accesses (`this.foo`-style reads) with the
/// template member evidence of the component and linked external templates.
///
/// Over-credits by design: any `member_access` whose `member` matches an input
/// counts as a use regardless of object, so destructures and `ngOnChanges`
/// reads never produce a false positive.
fn input_usage_set<'a>(
    component: &'a ModuleInfo,
    external_templates: &[&'a ModuleInfo],
) -> FxHashSet<&'a str> {
    let mut used: FxHashSet<&str> = FxHashSet::default();
    for access in SemanticFactView::new(&component.semantic_facts, &component.member_accesses)
        .ordinary_member_accesses()
    {
        // Any ordinary member access naming the input (`this.foo`,
        // `changes.foo`, a destructured read) credits it.
        used.insert(access.member.as_str());
    }
    insert_angular_template_members(component, &mut used);
    for template in external_templates {
        insert_angular_template_members(template, &mut used);
    }
    used
}

pub(super) fn insert_angular_template_members<'a>(
    module: &'a ModuleInfo,
    out: &mut FxHashSet<&'a str>,
) {
    out.extend(
        SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .angular_template_member_names(),
    );
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

/// Whether the module spreads `this` into an object literal (`{ ...this }`).
/// Every input/output is then consumed opaquely, so the whole component abstains.
pub(super) fn component_spreads_this(module: &ModuleInfo) -> bool {
    SemanticFactView::new(&module.semantic_facts, &module.member_accesses).has_angular_this_spread()
}

/// The `.ts` modules reached from `from` by a `SideEffect`-shaped edge that hold
/// an external Angular `templateUrl`. The component sets
/// `has_angular_component_template_url`, and the external `.html` file is reached
/// via a `SideEffect` import edge; we follow every outgoing edge and keep the
/// targets whose module carries template member evidence.
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
        // The external template module carries typed template facts. Older
        // parse-cache payloads may still carry conservative member accesses.
        if SemanticFactView::new(
            &target_module.semantic_facts,
            &target_module.member_accesses,
        )
        .has_angular_template_members()
        {
            out.push(*target_module);
        }
    }
    out
}

/// The component name: the file stem (e.g. `user-card` for `user-card.ts`).
fn component_name_for(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Whether `name` is a JavaScript reserved word that fallow's JS-based template
/// expression scanner would read as an operator/keyword rather than an
/// identifier (so a template read of an input/output named this is invisible).
/// Angular's template grammar permits these as property names, so a member named
/// `delete` / `in` / `new` is idiomatic; abstaining keeps the detector zero-FP.
pub(super) fn is_js_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "delete"
            | "void"
            | "typeof"
            | "new"
            | "in"
            | "instanceof"
            | "yield"
            | "await"
            | "super"
            | "this"
            | "null"
            | "true"
            | "false"
            | "function"
            | "class"
            | "return"
            | "if"
            | "else"
            | "for"
            | "while"
            | "do"
            | "switch"
            | "case"
            | "throw"
            | "try"
            | "catch"
            | "finally"
            | "var"
            | "let"
            | "const"
            | "import"
            | "export"
            | "default"
            | "extends"
            | "enum"
    )
}

#[cfg(test)]
mod tests {
    use fallow_types::discover::FileId;
    use fallow_types::extract::{
        AngularInputMember, AngularTemplateMemberAccessFact, AngularThisSpreadFact,
        ClassHeritageInfo, MemberAccess, SemanticFact,
    };
    use rustc_hash::FxHashSet;

    use super::*;
    use crate::analyze::test_support::empty_module;

    fn input(name: &str, span: u32) -> AngularInputMember {
        AngularInputMember {
            name: name.to_string(),
            span_start: span,
        }
    }

    fn tpl_fact(member: &str) -> SemanticFact {
        SemanticFact::AngularTemplateMemberAccess(AngularTemplateMemberAccessFact {
            member: member.to_string(),
        })
    }

    fn this_spread_fact() -> SemanticFact {
        SemanticFact::AngularThisSpread(AngularThisSpreadFact)
    }

    fn this_access(member: &str) -> MemberAccess {
        MemberAccess {
            object: "this".to_string(),
            member: member.to_string(),
        }
    }

    #[test]
    fn unread_input_is_not_in_usage_set() {
        let component = ModuleInfo {
            angular_inputs: vec![input("label", 10)],
            ..empty_module()
        };
        let used = input_usage_set(&component, &[]);
        assert!(
            !used.contains("label"),
            "an input read nowhere is absent from the usage set (so it is flagged)"
        );
    }

    #[test]
    fn template_credited_input_is_used() {
        let component = ModuleInfo {
            angular_inputs: vec![input("label", 10)],
            semantic_facts: vec![tpl_fact("label")].into(),
            ..empty_module()
        };
        let used = input_usage_set(&component, &[]);
        assert!(
            used.contains("label"),
            "an inline-template ref credits the input"
        );
    }

    #[test]
    fn typed_template_fact_credits_input() {
        let component = ModuleInfo {
            angular_inputs: vec![input("label", 10)],
            semantic_facts: vec![tpl_fact("label")].into(),
            ..empty_module()
        };
        let used = input_usage_set(&component, &[]);
        assert!(
            used.contains("label"),
            "a typed Angular template fact credits the input"
        );
    }

    #[test]
    fn script_credited_input_is_used() {
        let component = ModuleInfo {
            angular_inputs: vec![input("count", 10)],
            member_accesses: vec![this_access("count")],
            ..empty_module()
        };
        let used = input_usage_set(&component, &[]);
        assert!(
            used.contains("count"),
            "a `this.count` read credits the input"
        );
    }

    #[test]
    fn external_template_credited_input_is_used() {
        let component = ModuleInfo {
            angular_inputs: vec![input("title", 10)],
            has_angular_component_template_url: true,
            ..empty_module()
        };
        let external = ModuleInfo {
            file_id: FileId(2),
            semantic_facts: vec![tpl_fact("title")].into(),
            ..empty_module()
        };
        let used = input_usage_set(&component, &[&external]);
        assert!(
            used.contains("title"),
            "a typed fact in the linked external template credits the input"
        );
    }

    #[test]
    fn extends_abstain_holds() {
        let component = ModuleInfo {
            angular_inputs: vec![input("label", 10)],
            class_heritage: vec![ClassHeritageInfo {
                export_name: "Foo".to_string(),
                super_class: Some("Base".to_string()),
                implements: Vec::new(),
                instance_bindings: Vec::new(),
                super_class_type_args: Vec::new(),
                generic_instance_bindings: Vec::new(),
            }],
            ..empty_module()
        };
        assert!(
            component_has_extends(&component),
            "an `extends` clause abstains the whole component"
        );
    }

    #[test]
    fn no_extends_does_not_abstain() {
        let component = ModuleInfo {
            angular_inputs: vec![input("label", 10)],
            ..empty_module()
        };
        assert!(
            !component_has_extends(&component),
            "a component with no heritage `extends` does not abstain"
        );
    }

    #[test]
    fn typed_this_spread_fact_abstains_component() {
        let component = ModuleInfo {
            angular_inputs: vec![input("label", 10)],
            semantic_facts: vec![this_spread_fact()].into(),
            ..empty_module()
        };
        assert!(
            component_spreads_this(&component),
            "a typed Angular this-spread fact abstains the whole component"
        );
    }

    #[test]
    fn dep_gate_returns_empty_without_angular_core() {
        let graph = ModuleGraph::build(&[], &[], &[]);
        let modules = Vec::new();
        let declared: FxHashSet<String> = std::iter::once("react".to_string()).collect();
        let offsets = LineOffsetsMap::default();
        let findings = find_unused_component_inputs(&graph, &modules, &declared, &offsets);
        assert!(
            findings.is_empty(),
            "no `@angular/core` dependency means no findings"
        );
    }
}
