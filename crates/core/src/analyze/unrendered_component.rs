//! Detection of Vue/Svelte single-file components that are reachable in the
//! module graph but rendered NOWHERE in the project (the
//! imported-but-never-rendered dead-half).
//!
//! A `.vue`/`.svelte` SFC's default export is the component. It is "rendered"
//! when some file instantiates it: a `<Tag>` in a template, a `:is`/`this=`
//! binding, a `components: {}` / `app.component()` registration, an `h()` call,
//! a Nuxt auto-import, or a lazy `() => import('./X.vue')`. All of those make the
//! importing file REFERENCE the component binding, which fallow records (the
//! binding is removed from `unused_import_bindings`, and Nuxt auto-imports add a
//! synthetic resolved import). Only a bare barrel re-export
//! (`export { default as Foo } from './Foo.vue'`) keeps a component reachable
//! WITHOUT referencing it, which is exactly the rot this detector surfaces: a
//! component refactored out of every template but left re-exported.
//!
//! Built to never false-flag (degrade by abstaining):
//! - **Dep-gated** on `vue` / `@vue/runtime-core` / `nuxt` (for `.vue`) and
//!   `svelte` / `@sveltejs/kit` (for `.svelte`).
//! - The "rendered/used" set is built LIBERALLY (any reference, auto-import,
//!   dynamic import, side-effect import, through barrel chains): over-crediting a
//!   component can only suppress a finding, never create one.
//! - **Barrel-gated**: a component is only eligible when it is re-exported by a
//!   reachable barrel. A component reachable only through a DEAD direct import is
//!   left to `unused-import`; a component reachable through nothing is left to
//!   `unused-file`.
//! - **Entry-point abstain**: a component that is itself an entry point (route
//!   page, layout, `App.vue`, Nuxt `app.vue`/`error.vue`) is rendered by the
//!   framework, not flagged.
//! - **Public-API abstain**: a component re-exported from a non-private package
//!   entry point is rendered by a downstream consumer, not flagged.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{ImportedName, ModuleInfo};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::{ResolvedImport, ResolvedModule};
use crate::results::UnrenderedComponent;
use crate::suppress::{IssueKind, SuppressionContext};

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// 1-based line the finding anchors at. An SFC's default export is the file
/// itself; there is no explicit default-export statement to point at, so the
/// finding (and its inline suppression) anchors at the file head.
const COMPONENT_LINE: u32 = 1;

/// Framework a component file belongs to, derived from its extension + the
/// project's declared dependencies.
#[derive(Clone, Copy)]
enum SfcFramework {
    Vue,
    Svelte,
}

impl SfcFramework {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Vue => "vue",
            Self::Svelte => "svelte",
        }
    }
}

/// Classify a path as a dependency-gated SFC, or `None` if it is not an SFC or
/// the owning framework is not a declared dependency.
fn sfc_framework(path: &Path, vue: bool, svelte: bool) -> Option<SfcFramework> {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("vue") if vue => Some(SfcFramework::Vue),
        Some("svelte") if svelte => Some(SfcFramework::Svelte),
        _ => None,
    }
}

fn is_sfc_extension(path: &Path) -> bool {
    // Extension comparison without allocation; `.vue` / `.svelte` only.
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("vue") | Some("svelte")
    )
}

/// Find Vue/Svelte components that are reachable but rendered nowhere.
///
/// Returns empty unless the project declares `vue` / `@vue/runtime-core` /
/// `nuxt` or `svelte` / `@sveltejs/kit`.
#[must_use]
pub fn find_unrendered_components(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    public_api_entry_points: &FxHashSet<FileId>,
    suppressions: &SuppressionContext<'_>,
) -> Vec<UnrenderedComponent> {
    let vue = declared_deps.contains("vue")
        || declared_deps.contains("@vue/runtime-core")
        || declared_deps.contains("nuxt");
    let svelte = declared_deps.contains("svelte") || declared_deps.contains("@sveltejs/kit");
    if !vue && !svelte {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    let used = build_rendered_sfc_used_set(graph, resolved_modules, &modules_by_id);
    let reexported = build_barrel_reexported_sfcs(graph);
    // Public-API abstain set: every SFC reachable through ANY re-export chain
    // from a non-private package entry point. A component library re-exports its
    // components for downstream consumers to render, often through MULTI-HOP
    // barrels (entry -> `export *` -> sub-barrel -> `export { default as X } from
    // './X.vue'`), so a shallow one-hop check leaves deep leaves wrongly
    // eligible. Over-abstaining here only suppresses findings (zero-FP), never
    // creates them.
    let public_api = public_api_reexported_sfcs(graph, public_api_entry_points);

    // Pass 3: emit.
    let scan = SfcScanContext {
        graph,
        used: &used,
        reexported: &reexported,
        public_api: &public_api,
        public_api_entry_points,
        suppressions,
    };
    let mut findings = Vec::new();
    for module in &graph.modules {
        let Some(framework) = sfc_framework(&module.path, vue, svelte) else {
            continue;
        };
        if let Some(finding) = evaluate_unrendered_sfc(&scan, module, framework) {
            findings.push(finding);
        }
    }

    findings
}

/// Pass 1: the set of SFC files that some file actually renders/uses, built
/// liberally (a real reference, an auto-import, a dynamic import, a side-effect
/// import) and followed through barrel re-export chains.
fn build_rendered_sfc_used_set(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules_by_id: &FxHashMap<FileId, &ModuleInfo>,
) -> FxHashSet<FileId> {
    let mut used: FxHashSet<FileId> = FxHashSet::default();
    for resolved in resolved_modules {
        let referenced: &[String] = modules_by_id
            .get(&resolved.file_id)
            .map_or(&[], |m| m.referenced_import_bindings.as_slice());
        for import in &resolved.resolved_imports {
            credit_static_import(graph, import, referenced, &mut used);
        }
        for import in &resolved.resolved_dynamic_imports {
            // A dynamic `import('./X.vue')` is always a use (lazy component).
            if let Some(target) = import.target.internal_file_id() {
                credit_rendered_sfc_chain(graph, target, "default", &mut used);
            }
        }
    }
    used
}

/// Pass 2: map each SFC default-re-exported by a REACHABLE barrel to its barrel.
/// A component is only eligible when a barrel keeps it alive; otherwise
/// `unused-file` / `unused-import` owns it.
fn build_barrel_reexported_sfcs(graph: &ModuleGraph) -> FxHashMap<FileId, FileId> {
    let mut reexported: FxHashMap<FileId, FileId> = FxHashMap::default();
    for barrel in &graph.modules {
        if !barrel.is_reachable() {
            continue;
        }
        for re in &barrel.re_exports {
            if re.imported_name == "default" && is_sfc_extension(&graph_path(graph, re.source_file))
            {
                reexported.entry(re.source_file).or_insert(barrel.file_id);
            }
        }
    }
    reexported
}

/// Shared read-only state threaded through the SFC emit (Pass 3) scan.
struct SfcScanContext<'a> {
    graph: &'a ModuleGraph,
    used: &'a FxHashSet<FileId>,
    reexported: &'a FxHashMap<FileId, FileId>,
    public_api: &'a FxHashSet<FileId>,
    public_api_entry_points: &'a FxHashSet<FileId>,
    suppressions: &'a SuppressionContext<'a>,
}

/// Evaluate one SFC module against the eligibility gate and abstain ladder
/// (reachable, non-entry-point, not used, barrel-kept-alive, not public-API, not
/// suppressed), returning a finding only when every abstain is cleared.
fn evaluate_unrendered_sfc(
    scan: &SfcScanContext<'_>,
    module: &crate::graph::ModuleNode,
    framework: SfcFramework,
) -> Option<UnrenderedComponent> {
    if !module.is_reachable() || module.is_entry_point() {
        return None;
    }
    if scan.used.contains(&module.file_id) {
        return None;
    }
    // Not kept alive by a barrel: `unused-file` / `unused-import` owns it.
    let &barrel_id = scan.reexported.get(&module.file_id)?;
    if scan.public_api.contains(&module.file_id)
        || scan.public_api_entry_points.contains(&module.file_id)
    {
        return None;
    }
    // A component file has no explicit default-export statement; the finding
    // anchors at the file head (line 1), so honor both a line-1 inline
    // suppression and a file-level suppression.
    if scan.suppressions.is_suppressed(
        module.file_id,
        COMPONENT_LINE,
        IssueKind::UnrenderedComponent,
    ) || scan
        .suppressions
        .is_file_suppressed(module.file_id, IssueKind::UnrenderedComponent)
    {
        return None;
    }

    let component_name = component_name(&module.path);
    // Absolute barrel path; serialized workspace-relative by serde_path (like
    // `path`), so JSON consumers never see a machine-specific absolute path.
    let reachable_via = scan
        .graph
        .modules
        .get(barrel_id.0 as usize)
        .map(|b| b.path.clone());
    Some(UnrenderedComponent {
        path: module.path.clone(),
        component_name,
        framework: framework.as_str().to_string(),
        reachable_via,
        line: COMPONENT_LINE,
        col: 0,
    })
}

/// Credit the SFC target(s) of one static import, if the binding is actually
/// referenced (or is a synthetic auto-import edge), following barrel chains.
fn credit_static_import(
    graph: &ModuleGraph,
    import: &ResolvedImport,
    referenced: &[String],
    used: &mut FxHashSet<FileId>,
) {
    let Some(target) = import.target.internal_file_id() else {
        return;
    };
    let is_auto_import = import.info.source.starts_with("<auto-import:");
    let is_referenced = referenced
        .iter()
        .any(|name| name == &import.info.local_name);
    if !is_auto_import && !is_referenced {
        return;
    }
    match &import.info.imported_name {
        ImportedName::Named(name) => credit_rendered_sfc_chain(graph, target, name, used),
        ImportedName::Default => credit_rendered_sfc_chain(graph, target, "default", used),
        ImportedName::SideEffect => {
            // A side-effect import of an SFC keeps it deliberately alive.
            if is_sfc_extension(&graph_path(graph, target)) {
                used.insert(target);
            }
        }
        ImportedName::Namespace => {
            // `import * as ns from barrel` then `<ns.Foo />`: credit every SFC
            // the barrel re-exports (liberal, zero-drift).
            if is_sfc_extension(&graph_path(graph, target)) {
                used.insert(target);
            }
            if let Some(module) = graph.modules.get(target.0 as usize) {
                let names: Vec<(FileId, String)> = module
                    .re_exports
                    .iter()
                    .map(|re| (re.source_file, re.imported_name.clone()))
                    .collect();
                for (source, name) in names {
                    credit_rendered_sfc_chain(graph, source, &name, used);
                }
            }
        }
    }
}

/// Walk re-export edges from `(start_file, name)` and credit EVERY SFC file
/// encountered in the chain. SFCs have no default `ExportSymbol`, so the generic
/// `walk_re_export_origins` (which terminates at a locally-defined export) does
/// not recognize them as origins; this variant credits the SFC file directly.
fn credit_rendered_sfc_chain(
    graph: &ModuleGraph,
    start_file: FileId,
    start_name: &str,
    used: &mut FxHashSet<FileId>,
) {
    let mut visited: FxHashSet<(FileId, String)> = FxHashSet::default();
    let mut stack: Vec<(FileId, String)> = vec![(start_file, start_name.to_string())];
    while let Some((file_id, name)) = stack.pop() {
        if !visited.insert((file_id, name.clone())) {
            continue;
        }
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };
        if is_sfc_extension(&module.path) {
            used.insert(file_id);
        }
        let mut matched_named = false;
        for re in &module.re_exports {
            if re.exported_name != "*" && re.imported_name != "*" && re.exported_name == name {
                stack.push((re.source_file, re.imported_name.clone()));
                matched_named = true;
            }
        }
        if matched_named {
            continue;
        }
        for re in &module.re_exports {
            if re.exported_name == "*" {
                stack.push((re.source_file, name.clone()));
            }
        }
    }
}

fn graph_path(graph: &ModuleGraph, file_id: FileId) -> std::path::PathBuf {
    graph
        .modules
        .get(file_id.0 as usize)
        .map(|m| m.path.clone())
        .unwrap_or_default()
}

/// Every SFC reachable through ANY re-export chain (any imported name, including
/// `*`) from a non-private package entry point. Such an SFC is exposed for a
/// downstream consumer to render, so it is never a project-internal unrendered
/// component. Walks the full chain (entry -> sub-barrel -> ... -> `.vue` leaf),
/// not just one hop, and is cycle-safe via the visited set.
fn public_api_reexported_sfcs(
    graph: &ModuleGraph,
    public_api_entry_points: &FxHashSet<FileId>,
) -> FxHashSet<FileId> {
    let mut result: FxHashSet<FileId> = FxHashSet::default();
    let mut visited: FxHashSet<FileId> = FxHashSet::default();
    let mut stack: Vec<FileId> = public_api_entry_points.iter().copied().collect();
    while let Some(file_id) = stack.pop() {
        if !visited.insert(file_id) {
            continue;
        }
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };
        for re in &module.re_exports {
            let source = re.source_file;
            if is_sfc_extension(&graph_path(graph, source)) {
                result.insert(source);
            }
            stack.push(source);
        }
    }
    result
}

/// The component name: the file stem in PascalCase-as-written (the stem is used
/// only in the human message, so the raw stem is sufficient).
fn component_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("component")
        .to_string()
}

/// Whether a single Angular selector is an ELEMENT (type) selector.
///
/// First-cut scope is element selectors only: a component ALL of whose selectors
/// are element selectors and none used is the only flaggable shape. Attribute
/// (`[appFoo]`), class (`.foo`), `:not(...)`, and any compound / combinator
/// selector are NOT element selectors, so a component carrying one abstains
/// entirely. An element selector is a plain custom-element tag name: it must
/// contain a hyphen (Angular / custom-element convention, matching the used-tag
/// harvest) and consist only of tag-name characters.
fn is_element_selector(selector: &str) -> bool {
    let s = selector.trim();
    !s.is_empty()
        && s.contains('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Find Angular `@Component`s whose element selector is rendered in NO template
/// project-wide and that are not routed / bootstrapped / dynamically rendered /
/// public-API. The Angular arm of `unrendered-component` (framework
/// `"angular"`), gated on the project declaring `@angular/core`.
///
/// First-cut scope: ELEMENT selectors only. A component is eligible only when
/// ALL of its selectors are element selectors (`is_element_selector`); any
/// attribute (`[appFoo]`) or class (`.foo`) selector, or `@Directive`, abstains
/// (directives are never harvested into `angular_component_selectors`). The
/// detector flags a reachable component when NONE of its element selectors is in
/// the project-wide used-selector set AND its class name is referenced by NO
/// other module (routed `component:` / `loadComponent().then(m => m.X)`,
/// `bootstrapApplication` / `bootstrap: [...]`, `createComponent(Class)` all
/// surface the class identifier as a referenced import binding) AND it is not
/// lazily routed through the bare `loadComponent: () => import('./x')` /
/// `loadChildren: () => import('./x.routes')` form (which carries no class name
/// and instead credits the target's DEFAULT export via arrow-wrapped
/// dynamic-import resolution, so a referenced default export abstains) AND no
/// reachable module dynamically renders a component
/// (`ViewContainerRef.createComponent` / `*ngComponentOutlet` /
/// `createComponent(<ident>)`) AND the component is not public-API-exported.
/// Over-crediting in any of the used / referenced / dynamic channels only
/// suppresses a finding, never creates one.
#[must_use]
pub fn find_unrendered_angular_components(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    public_api_entry_points: &FxHashSet<FileId>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    suppressions: &SuppressionContext<'_>,
) -> Vec<UnrenderedComponent> {
    if !declared_deps.contains("@angular/core") {
        return Vec::new();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    // Pass 1: project-wide signals, built LIBERALLY (every signal credits toward
    // "used", so only false negatives can result, never false positives).
    let Some(signals) = build_angular_render_signals(modules) else {
        // A component dynamically renderable from a non-literal class reference
        // could be rendered anywhere: abstain on the WHOLE project (mirrors
        // `unprovided-inject`'s `has_dynamic_provide`).
        return Vec::new();
    };

    // Public-API abstain: a component re-exported from a non-private package entry
    // point (an Angular library surface) is rendered by a downstream consumer.
    let public_api = public_api_reexported_files(graph, public_api_entry_points);

    // Pass 2: emit.
    //
    // Unlike the Vue/Svelte arm, an entry-point component is NOT skipped here: the
    // Angular plugin blanket-marks every `src/app/**/*.component.ts` as an entry
    // point (Angular's DI/module graph is not import-traceable), so skipping entry
    // points would make the rule never fire. Render-equivalence is established by
    // the selector-used / route / bootstrap / dynamic / public-API abstains
    // instead. A component not reachable at all is left to `unused-file`.
    let mut findings = Vec::new();
    for node in &graph.modules {
        if !node.is_reachable() {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        if module.angular_component_selectors.is_empty() {
            continue;
        }
        if public_api.contains(&node.file_id) || public_api_entry_points.contains(&node.file_id) {
            continue;
        }
        emit_angular_component_findings(
            node,
            module,
            &signals,
            line_offsets_by_file,
            suppressions,
            &mut findings,
        );
    }

    findings
}

/// Project-wide Angular render signals, all built LIBERALLY: a selector or class
/// in either set credits a component toward "rendered".
struct AngularRenderSignals<'a> {
    used_selectors: FxHashSet<String>,
    entry_classes: FxHashSet<&'a str>,
}

/// Pass 1: union the project-wide used-selector and entry-class signals. Returns
/// `None` when ANY module dynamically renders a component (whole-project abstain).
fn build_angular_render_signals(modules: &[ModuleInfo]) -> Option<AngularRenderSignals<'_>> {
    let mut used_selectors: FxHashSet<String> = FxHashSet::default();
    let mut entry_classes: FxHashSet<&str> = FxHashSet::default();
    for module in modules {
        for selector in &module.angular_used_selectors {
            used_selectors.insert(selector.clone());
        }
        for class_name in &module.angular_entry_component_refs {
            entry_classes.insert(class_name.as_str());
        }
        if module.has_dynamic_component_render {
            return None;
        }
    }
    Some(AngularRenderSignals {
        used_selectors,
        entry_classes,
    })
}

/// Emit one finding per genuinely-unrendered `@Component` on a node, applying the
/// per-component abstain ladder (element-selector scope, used-selector, route /
/// bootstrap entry, lazy-route default-export credit, suppression).
fn emit_angular_component_findings(
    node: &crate::graph::ModuleNode,
    module: &ModuleInfo,
    signals: &AngularRenderSignals<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    suppressions: &SuppressionContext<'_>,
    findings: &mut Vec<UnrenderedComponent>,
) {
    // A lazily-routed component declared with the bare loadComponent /
    // loadChildren form (`loadComponent: () => import('./x')`, no
    // `.then(m => m.X)`) is loaded through its module's DEFAULT export, which
    // fallow's arrow-wrapped dynamic-import resolution credits as a `default`
    // reference. Such a form has NO class name in the route config for
    // `entry_classes` to capture, so the default-export reference is the only
    // render-equivalence signal. Abstain when this file's default export carries
    // any reference (or is side-effect registered): it is reached via a dynamic
    // import, a default import, or a default-import render site. A genuinely-orphan
    // component is a NAMED export (the `imports: [...]` registration is a named
    // import, the dead case this rule catches), so a referenced NAMED export does
    // NOT suppress it; only the default-export signal does.
    let default_export_referenced = node.exports.iter().any(|export| {
        matches!(export.name, fallow_types::extract::ExportName::Default)
            && (!export.references.is_empty() || export.is_side_effect_used)
    });
    for component in &module.angular_component_selectors {
        // First-cut scope: every selector must be an element selector.
        if !component.selectors.iter().all(|s| is_element_selector(s)) {
            continue;
        }
        // Used if ANY selector is in the project-wide used set.
        if component
            .selectors
            .iter()
            .any(|s| signals.used_selectors.contains(&s.to_ascii_lowercase()))
        {
            continue;
        }
        // Referenced as a route / bootstrap entry point (render-equivalent:
        // Angular instantiates these without a template `<tag>`).
        if signals
            .entry_classes
            .contains(component.class_name.as_str())
        {
            continue;
        }
        // Lazily routed via the bare `loadComponent` / `loadChildren` form
        // (default-export dynamic-import credit).
        if default_export_referenced {
            continue;
        }
        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, node.file_id, component.span_start);
        if suppressions.is_suppressed(node.file_id, line, IssueKind::UnrenderedComponent)
            || suppressions.is_file_suppressed(node.file_id, IssueKind::UnrenderedComponent)
        {
            continue;
        }
        findings.push(UnrenderedComponent {
            path: node.path.clone(),
            component_name: component.class_name.clone(),
            framework: "angular".to_string(),
            reachable_via: None,
            line,
            col,
        });
    }
}

/// Every source file reachable through ANY re-export chain (any imported name,
/// including `*`) from a non-private package entry point. The extension-agnostic
/// analogue of `public_api_reexported_sfcs`: an Angular component re-exported
/// from a library `public-api.ts` is exposed for a downstream consumer to render,
/// so it is never a project-internal unrendered component. Walks the full chain
/// (entry -> sub-barrel -> ... -> leaf), cycle-safe via the visited set.
fn public_api_reexported_files(
    graph: &ModuleGraph,
    public_api_entry_points: &FxHashSet<FileId>,
) -> FxHashSet<FileId> {
    let mut result: FxHashSet<FileId> = FxHashSet::default();
    let mut visited: FxHashSet<FileId> = FxHashSet::default();
    let mut stack: Vec<FileId> = public_api_entry_points.iter().copied().collect();
    while let Some(file_id) = stack.pop() {
        if !visited.insert(file_id) {
            continue;
        }
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };
        for re in &module.re_exports {
            let source = re.source_file;
            result.insert(source);
            stack.push(source);
        }
    }
    result
}
