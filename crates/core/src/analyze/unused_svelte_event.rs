//! Detection of unused Svelte custom events: a `.svelte` component dispatching a
//! custom event via `dispatch('<name>')` (where `dispatch` is the binding from
//! `const dispatch = createEventDispatcher()`) whose event name is listened to
//! NOWHERE in the analyzed project. Cross-file dead-OUTPUT direction: the
//! component fires an event nothing handles. No native tool covers the listener
//! side (eslint-plugin-svelte / svelte-check are single-file / type-only).
//!
//! Mirrors `unprovided_inject`'s two-pass cross-file set-difference: pass 1
//! builds a liberal project-wide "listened" set (an `on:<name>` listener on ANY
//! component anywhere credits the name, the safe over-credit direction), pass 2
//! flags each reachable component's dispatched events whose name is in NO
//! listener.
//!
//! Zero-FP doctrine. The dispatched/listened harvest lives on `ModuleInfo`
//! (set during extraction); this detector only reads it, applies the dep gate,
//! and abstains on the whole-component signals:
//! - `has_dynamic_dispatch`: a `dispatch(<nonLiteral>)` call (event unknowable),
//!   or the `dispatch` binding used as a whole value (passed / returned).

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::UnusedSvelteEvent;

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Find Svelte custom events dispatched via `createEventDispatcher()` and
/// listened to nowhere project-wide. Returns empty unless the project declares
/// `svelte`.
#[must_use]
pub fn find_unused_svelte_events(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<UnusedSvelteEvent> {
    if !declared_deps.contains("svelte") {
        return Vec::new();
    }

    // Pass 1: union every module's listened events into a liberal project-wide
    // set. A listener on ANY component credits the name everywhere (the safe
    // over-credit direction: over-crediting suppresses a finding, never creates
    // one).
    let listened = collect_listened_events(modules);

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    // Pass 2: flag each reachable `.svelte` component's dispatched events whose
    // name is in no listener.
    let mut findings = Vec::new();
    for node in &graph.modules {
        if !node.is_reachable() || !is_svelte_file(&node.path) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        flag_unlistened_events(
            module,
            &node.path,
            &listened,
            line_offsets_by_file,
            &mut findings,
        );
    }

    findings.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(a.line.cmp(&b.line))
            .then(a.event_name.cmp(&b.event_name))
    });
    findings
}

/// Build the liberal project-wide set of listened custom-event names.
fn collect_listened_events(modules: &[ModuleInfo]) -> FxHashSet<&str> {
    modules
        .iter()
        .flat_map(|module| module.svelte_listened_events.iter().map(String::as_str))
        .collect()
}

/// Append one finding per dispatched event in `module` whose name is in no
/// listener. Abstains on the whole component when `has_dynamic_dispatch` is set
/// (a dynamic dispatch or whole-`dispatch`-value use can fire any event
/// opaquely). A no-op when the component dispatches nothing.
fn flag_unlistened_events(
    module: &ModuleInfo,
    path: &Path,
    listened: &FxHashSet<&str>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    findings: &mut Vec<UnusedSvelteEvent>,
) {
    if module.svelte_dispatched_events.is_empty() || module.has_dynamic_dispatch {
        return;
    }
    let component_name = component_name_for(path);
    for dispatched in &module.svelte_dispatched_events {
        if listened.contains(dispatched.name.as_str()) {
            continue;
        }
        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, module.file_id, dispatched.span_start);
        findings.push(UnusedSvelteEvent {
            path: path.to_path_buf(),
            component_name: component_name.clone(),
            event_name: dispatched.name.clone(),
            line,
            col,
        });
    }
}

/// Whether the path is a Svelte SFC (`.svelte`).
fn is_svelte_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()) == Some("svelte")
}

/// The component name: the `.svelte` file stem.
fn component_name_for(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::extract::DispatchedEvent;

    use super::super::test_support::empty_module;
    use super::*;

    /// A `.svelte` `ModuleInfo` with the given dispatched/listened events.
    fn svelte_module(id: u32, dispatched: &[&str], listened: &[&str], dynamic: bool) -> ModuleInfo {
        ModuleInfo {
            file_id: FileId(id),
            // A single line-offset entry (start of line 1) so the
            // byte-offset-to-line-col helper resolves without panicking on the
            // synthetic spans.
            line_offsets: vec![0],
            svelte_dispatched_events: dispatched
                .iter()
                .enumerate()
                .map(|(i, name)| DispatchedEvent {
                    name: (*name).to_string(),
                    span_start: i as u32,
                })
                .collect(),
            svelte_listened_events: listened.iter().map(|s| (*s).to_string()).collect(),
            has_dynamic_dispatch: dynamic,
            ..empty_module()
        }
    }

    /// Run pass 1 + pass 2 directly against a set of modules, treating each
    /// `.svelte` module as a reachable component named `Comp<id>.svelte`. This
    /// exercises the real set-difference + abstain logic without building a full
    /// `ModuleGraph` (which needs resolver fixtures); the graph wrapper only adds
    /// the reachable + `.svelte`-extension gate, covered by integration tests.
    fn run(modules: &[ModuleInfo]) -> Vec<UnusedSvelteEvent> {
        let listened = collect_listened_events(modules);
        let offsets: LineOffsetsMap<'_> = modules
            .iter()
            .map(|m| (m.file_id, m.line_offsets.as_slice()))
            .collect();
        let mut findings = Vec::new();
        for module in modules {
            let path = PathBuf::from(format!("src/Comp{}.svelte", module.file_id.0));
            flag_unlistened_events(module, &path, &listened, &offsets, &mut findings);
        }
        findings.sort_by(|a, b| a.path.cmp(&b.path).then(a.event_name.cmp(&b.event_name)));
        findings
    }

    #[test]
    fn dispatched_and_listened_is_not_flagged() {
        let child = svelte_module(0, &["save"], &[], false);
        let parent = svelte_module(1, &[], &["save"], false);
        let findings = run(&[child, parent]);
        assert!(
            findings.is_empty(),
            "listened event must not flag: {findings:?}"
        );
    }

    #[test]
    fn dispatched_and_unlistened_is_flagged() {
        let child = svelte_module(0, &["save"], &[], false);
        let parent = svelte_module(1, &[], &["close"], false);
        let findings = run(&[child, parent]);
        assert_eq!(
            findings.len(),
            1,
            "unlistened event must flag: {findings:?}"
        );
        assert_eq!(findings[0].event_name, "save");
    }

    #[test]
    fn dynamic_dispatch_abstains_whole_component() {
        let child = svelte_module(0, &["save"], &[], true);
        let findings = run(&[child]);
        assert!(
            findings.is_empty(),
            "dynamic dispatch must abstain the whole component: {findings:?}"
        );
    }

    #[test]
    fn dom_on_click_does_not_credit_listener() {
        // A DOM `on:click` is excluded from `svelte_listened_events` at harvest
        // time, so a dispatched `click` with no component listener still flags.
        let child = svelte_module(0, &["click"], &[], false);
        let findings = run(&[child]);
        assert_eq!(
            findings.len(),
            1,
            "DOM on:click must not credit: {findings:?}"
        );
        assert_eq!(findings[0].event_name, "click");
    }

    #[test]
    fn forwarding_listener_credits_the_event() {
        // Event forwarding (`on:save` with no value) is harvested as a listen,
        // so a dispatched `save` is credited and not flagged.
        let child = svelte_module(0, &["save"], &[], false);
        let mid = svelte_module(1, &[], &["save"], false);
        let findings = run(&[child, mid]);
        assert!(
            findings.is_empty(),
            "forwarded event must be credited: {findings:?}"
        );
    }

    #[test]
    fn no_dep_gate_returns_empty() {
        let child = svelte_module(0, &["save"], &[], false);
        let graph = ModuleGraph::build(&[], &[], &[]);
        let modules = [child];
        let offsets: LineOffsetsMap<'_> = modules
            .iter()
            .map(|m| (m.file_id, m.line_offsets.as_slice()))
            .collect();
        let findings = find_unused_svelte_events(&graph, &modules, &FxHashSet::default(), &offsets);
        assert!(
            findings.is_empty(),
            "no `svelte` dep must abstain: {findings:?}"
        );
    }
}
