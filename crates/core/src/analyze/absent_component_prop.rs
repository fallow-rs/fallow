//! Conservative optional-input review over cached component facts and canonical exports.

use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::discover::FileId;
use crate::graph::{
    EffectiveExportBinding, EffectiveExportResolution, ExportNamespace, ModuleGraph,
};
use crate::resolve::{ResolvedImport, ResolvedModule};
use crate::results::{AbsentComponentProp, ComponentPropCallSite};
use fallow_types::extract::{
    ComponentFramework, ComponentPropDeclaration, ComponentReference, ExportName, ImportedName,
    ModuleInfo, angular_template_member_names,
};
use rustc_hash::{FxHashMap, FxHashSet};

const EXPLANATION: &str = "Known reachable callers do not supply this optional prop. Review defaults and API intent before changing the component; static analysis does not prove runtime unreachability.";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ComponentId {
    file: FileId,
    name: String,
    span: u32,
}

struct Consumer<'a> {
    file: FileId,
    span: u32,
    supplied: &'a [String],
    properties: &'a [String],
    unknown: bool,
}

/// Inputs to shared optional-input analysis.
pub(super) struct AbsentPropInput<'a> {
    pub graph: &'a ModuleGraph,
    pub modules: &'a [ModuleInfo],
    pub resolved_modules: &'a [ResolvedModule],
    pub declared_deps: &'a FxHashSet<String>,
    pub public_api_entry_points: &'a FxHashSet<FileId>,
    pub line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

struct ContractIndex<'a> {
    input: &'a AbsentPropInput<'a>,
    modules: FxHashMap<FileId, &'a ModuleInfo>,
    resolved: FxHashMap<FileId, &'a ResolvedModule>,
    declarations: FxHashMap<ComponentId, Vec<&'a ComponentPropDeclaration>>,
    selectors: FxHashMap<String, Vec<ComponentId>>,
}

impl<'a> ContractIndex<'a> {
    fn new(input: &'a AbsentPropInput<'a>) -> Self {
        let modules = input.modules.iter().map(|m| (m.file_id, m)).collect();
        let resolved = input
            .resolved_modules
            .iter()
            .map(|m| (m.file_id, m))
            .collect();
        let mut declarations: FxHashMap<ComponentId, Vec<&ComponentPropDeclaration>> =
            FxHashMap::default();
        for module in input.modules {
            let Some(facts) = &module.component_contracts else {
                continue;
            };
            for prop in &facts.declarations {
                declarations
                    .entry(ComponentId {
                        file: module.file_id,
                        name: prop.component.clone(),
                        span: prop.component_span,
                    })
                    .or_default()
                    .push(prop);
            }
        }
        let mut identities: FxHashMap<(FileId, &str), Vec<ComponentId>> = FxHashMap::default();
        for id in declarations.keys() {
            identities
                .entry((id.file, id.name.as_str()))
                .or_default()
                .push(id.clone());
        }
        let mut selectors: FxHashMap<String, Vec<ComponentId>> = FxHashMap::default();
        for module in input.modules {
            for selector in &module.angular_component_selectors {
                let candidates = identities
                    .get(&(module.file_id, selector.class_name.as_str()))
                    .cloned()
                    .unwrap_or_else(|| {
                        vec![ComponentId {
                            file: module.file_id,
                            name: selector.class_name.clone(),
                            span: selector.span_start,
                        }]
                    });
                for tag in &selector.selectors {
                    selectors
                        .entry(tag.clone())
                        .or_default()
                        .extend(candidates.iter().cloned());
                }
            }
            for registration in &module.registered_custom_elements {
                let candidates = identities
                    .get(&(module.file_id, registration.class_local_name.as_str()))
                    .cloned()
                    .unwrap_or_else(|| {
                        vec![ComponentId {
                            file: module.file_id,
                            name: registration.class_local_name.clone(),
                            span: registration.span_start,
                        }]
                    });
                selectors
                    .entry(registration.tag.clone())
                    .or_default()
                    .extend(candidates);
            }
        }
        for ids in selectors.values_mut() {
            ids.sort_by_key(|id| (id.file.0, id.span));
            ids.dedup();
        }
        Self {
            input,
            modules,
            resolved,
            declarations,
            selectors,
        }
    }

    fn reference(
        &self,
        file: FileId,
        reference: &ComponentReference,
        visited: &mut FxHashSet<EffectiveExportBinding>,
    ) -> Option<ComponentId> {
        match reference {
            ComponentReference::Local { name, span_start } => {
                let id = ComponentId {
                    file,
                    name: name.clone(),
                    span: *span_start,
                };
                self.declarations.contains_key(&id).then_some(id)
            }
            ComponentReference::Import { local, span_start } => {
                let Some(import) = self.import(file, local, *span_start) else {
                    let ids = self
                        .selectors
                        .get(&local.to_ascii_lowercase())
                        .filter(|ids| ids.len() == 1)?;
                    let id = ids.first()?;
                    return self
                        .declarations
                        .get(id)
                        .is_some_and(|props| {
                            props
                                .iter()
                                .any(|prop| prop.framework == ComponentFramework::Lit)
                        })
                        .then(|| id.clone());
                };
                let name = match &import.info.imported_name {
                    ImportedName::Default => "default",
                    ImportedName::Named(name) => name,
                    ImportedName::Namespace | ImportedName::SideEffect => return None,
                };
                self.import_binding(import, name, visited)
            }
            ComponentReference::NamespaceMember {
                local,
                span_start,
                member,
            } => {
                let import = self.import(file, local, *span_start)?;
                if import.info.imported_name != ImportedName::Namespace {
                    return None;
                }
                self.import_binding(import, member, visited)
            }
            ComponentReference::Selector(tag) => self
                .selectors
                .get(tag)
                .filter(|ids| ids.len() == 1)
                .and_then(|ids| ids.first())
                .cloned(),
            ComponentReference::Unknown => None,
        }
    }

    fn import(&self, file: FileId, local: &str, span: u32) -> Option<&'a ResolvedImport> {
        let mut matches = self
            .resolved
            .get(&file)?
            .resolved_imports
            .iter()
            .filter(|import| {
                import.info.local_name == local
                    && (import.target.is_synthetic_auto_import()
                        || (import.info.span.start <= span && span < import.info.span.end))
            });
        let import = matches.next()?;
        matches.next().is_none().then_some(import)
    }

    fn import_binding(
        &self,
        import: &ResolvedImport,
        name: &str,
        visited: &mut FxHashSet<EffectiveExportBinding>,
    ) -> Option<ComponentId> {
        let target = import.target.internal_file_id()?;
        let EffectiveExportResolution::Unique(binding) =
            self.input
                .graph
                .resolve_export(target, name, ExportNamespace::Value)
        else {
            return None;
        };
        self.binding(binding, visited)
    }

    fn binding(
        &self,
        binding: EffectiveExportBinding,
        visited: &mut FxHashSet<EffectiveExportBinding>,
    ) -> Option<ComponentId> {
        if !visited.insert(binding) {
            return None;
        }
        let file = binding.origin_file();
        let facts = self.modules.get(&file)?.component_contracts.as_ref()?;
        let export = if binding.is_implicit_default() {
            facts
                .exports
                .iter()
                .find(|export| export.export_name == "default")?
        } else {
            let origin = self.input.graph.export_binding_origin(binding)?;
            let name = match &origin.export().name {
                ExportName::Default => "default",
                ExportName::Named(name) => name,
            };
            facts.exports.iter().find(|export| {
                export.export_name == name && export.export_span == origin.export().span.start
            })?
        };
        self.reference(file, &export.target, visited)
    }

    fn surface(&self, file: FileId, blocked: &mut FxHashSet<ComponentId>) {
        let mut stack: Vec<_> = self
            .input
            .graph
            .unique_export_bindings(file, ExportNamespace::Value)
            .into_iter()
            .collect();
        let mut visited = FxHashSet::default();
        while let Some(binding) = stack.pop() {
            if !visited.insert(binding) {
                continue;
            }
            if let Some(source) = binding.namespace_source() {
                stack.extend(
                    self.input
                        .graph
                        .unique_export_bindings(source, ExportNamespace::Value),
                );
            } else if let Some(id) = self.binding(binding, &mut FxHashSet::default()) {
                blocked.insert(id);
            }
        }
    }

    fn framework(&self, file: FileId, framework: ComponentFramework) -> Option<ComponentFramework> {
        let deps = self.input.declared_deps;
        let has = |names: &[&str]| names.iter().any(|name| deps.contains(*name));
        match framework {
            ComponentFramework::React
                if has(&[
                    "react",
                    "react-dom",
                    "react-native",
                    "next",
                    "@remix-run/react",
                ]) =>
            {
                let mixed = has(&[
                    "preact",
                    "solid-js",
                    "@solidjs/start",
                    "@builder.io/qwik",
                    "@qwik.dev/core",
                ]);
                let proven = self.resolved.get(&file).is_some_and(|module| {
                    module.resolved_imports.iter().any(|import| {
                        matches!(
                            import.info.source.as_str(),
                            "react"
                                | "react-dom"
                                | "react-native"
                                | "react/jsx-runtime"
                                | "react/jsx-dev-runtime"
                        )
                    })
                });
                (!mixed || proven).then_some(framework)
            }
            ComponentFramework::React => {
                let alternatives = [
                    (ComponentFramework::Preact, has(&["preact"])),
                    (
                        ComponentFramework::Solid,
                        has(&["solid-js", "@solidjs/start"]),
                    ),
                    (
                        ComponentFramework::Qwik,
                        has(&["@builder.io/qwik", "@qwik.dev/core"]),
                    ),
                ];
                let mut enabled = alternatives
                    .into_iter()
                    .filter(|(_, enabled)| *enabled)
                    .map(|(framework, _)| framework);
                let candidate = enabled.next()?;
                enabled.next().is_none().then_some(candidate)
            }
            ComponentFramework::Preact if has(&["preact"]) => Some(framework),
            ComponentFramework::Solid if has(&["solid-js", "@solidjs/start"]) => Some(framework),
            ComponentFramework::Qwik if has(&["@builder.io/qwik", "@qwik.dev/core"]) => {
                Some(framework)
            }
            ComponentFramework::Vue if has(&["vue", "@vue/runtime-core", "nuxt"]) => {
                Some(framework)
            }
            ComponentFramework::Svelte if has(&["svelte", "@sveltejs/kit"]) => Some(framework),
            ComponentFramework::Astro if has(&["astro"]) => Some(framework),
            ComponentFramework::Angular if has(&["@angular/core"]) => Some(framework),
            ComponentFramework::Lit if has(&["lit", "lit-element", "@lit/reactive-element"]) => {
                Some(framework)
            }
            ComponentFramework::Ember
                if has(&["ember-source", "@glimmer/component", "@glimmer/core"]) =>
            {
                Some(framework)
            }
            _ => None,
        }
    }

    fn block_framework(&self, framework: ComponentFramework, blocked: &mut FxHashSet<ComponentId>) {
        for (id, props) in &self.declarations {
            if props.iter().any(|prop| {
                self.framework(id.file, prop.framework) == self.framework(id.file, framework)
                    || (framework == ComponentFramework::React
                        && matches!(
                            prop.framework,
                            ComponentFramework::React
                                | ComponentFramework::Preact
                                | ComponentFramework::Solid
                                | ComponentFramework::Qwik
                        ))
            }) {
                blocked.insert(id.clone());
            }
        }
    }

    fn block_imported_surfaces(&self, file: FileId, blocked: &mut FxHashSet<ComponentId>) {
        let Some(module) = self.resolved.get(&file) else {
            return;
        };
        for import in &module.resolved_imports {
            if let Some(target) = import.target.internal_file_id() {
                self.surface(target, blocked);
            }
        }
    }

    fn block_reference(
        &self,
        file: FileId,
        reference: &ComponentReference,
        framework: ComponentFramework,
        blocked: &mut FxHashSet<ComponentId>,
    ) {
        if let Some(id) = self.reference(file, reference, &mut FxHashSet::default()) {
            blocked.insert(id);
            return;
        }
        match reference {
            ComponentReference::Import { local, span_start }
            | ComponentReference::NamespaceMember {
                local, span_start, ..
            } => {
                if let Some(import) = self.import(file, local, *span_start) {
                    if let Some(target) = import.target.internal_file_id() {
                        let name = match reference {
                            ComponentReference::NamespaceMember { member, .. } => {
                                Some(member.as_str())
                            }
                            _ => match &import.info.imported_name {
                                ImportedName::Default => Some("default"),
                                ImportedName::Named(name) => Some(name.as_str()),
                                _ => None,
                            },
                        };
                        if name.is_some_and(|name| {
                            self.input
                                .graph
                                .resolve_export(target, name, ExportNamespace::Value)
                                == EffectiveExportResolution::Ambiguous
                        }) {
                            self.block_framework(framework, blocked);
                        }
                        self.surface(target, blocked);
                    }
                } else {
                    self.block_framework(framework, blocked);
                    if let Some(module) = self.resolved.get(&file) {
                        for import in &module.resolved_imports {
                            if let Some(target) = import.target.internal_file_id() {
                                self.surface(target, blocked);
                            }
                        }
                    }
                }
            }
            ComponentReference::Selector(tag) => {
                if let Some(ids) = self.selectors.get(tag) {
                    blocked.extend(ids.iter().cloned());
                }
            }
            ComponentReference::Unknown => {
                blocked.extend(self.declarations.keys().cloned());
            }
            ComponentReference::Local { .. } => {}
        }
    }

    fn angular_used(&self, id: &ComponentId, prop: &ComponentPropDeclaration) -> bool {
        if prop.is_used {
            return true;
        }
        let Some(facts) = self
            .modules
            .get(&id.file)
            .and_then(|module| module.component_contracts.as_deref())
        else {
            return false;
        };
        let Some(resolved) = self.resolved.get(&id.file) else {
            return false;
        };
        facts
            .external_templates
            .iter()
            .filter(|template| template.component_span == id.span)
            .any(|template| {
                let mut edges = resolved.resolved_imports.iter().filter(|import| {
                    import.info.source == template.source
                        && import.info.imported_name == ImportedName::SideEffect
                });
                let Some(edge) = edges.next() else {
                    return false;
                };
                if edges.next().is_some() {
                    return false;
                }
                edge.target
                    .internal_file_id()
                    .and_then(|target| self.modules.get(&target))
                    .is_some_and(|template| {
                        angular_template_member_names(template).any(|name| name == prop.local)
                    })
            })
    }
}

/// Find optional inputs with a closed, known set of reachable static callers.
pub(super) fn find_absent_component_props(input: &AbsentPropInput<'_>) -> Vec<AbsentComponentProp> {
    let index = ContractIndex::new(input);
    let mut blocked = FxHashSet::default();
    let mut consumers: FxHashMap<ComponentId, Vec<Consumer<'_>>> = FxHashMap::default();
    block_public_and_entries(&index, &mut blocked);
    for module in input.modules {
        let Some(node) = input.graph.modules.get(module.file_id.0 as usize) else {
            continue;
        };
        if !node.is_runtime_reachable() {
            continue;
        }
        if module.parse_error_count > 0 || module.parse_panicked {
            blocked.extend(index.declarations.keys().cloned());
        }
        let Some(facts) = &module.component_contracts else {
            continue;
        };
        for framework in &facts.incomplete_frameworks {
            index.block_framework(*framework, &mut blocked);
        }
        if !facts.incomplete_frameworks.is_empty() {
            index.block_imported_surfaces(module.file_id, &mut blocked);
            index.block_framework(ComponentFramework::Lit, &mut blocked);
        }
        for escape in &facts.escapes {
            index.block_reference(
                module.file_id,
                escape,
                ComponentFramework::React,
                &mut blocked,
            );
        }
        for export in &facts.exports {
            if let ComponentReference::Import { local, span_start } = &export.target
                && let Some(import) = index.import(module.file_id, local, *span_start)
                && import.info.imported_name == ImportedName::Namespace
                && let Some(target) = import.target.internal_file_id()
            {
                index.surface(target, &mut blocked);
            }
        }
        for invocation in &facts.invocations {
            if let Some(id) = index.reference(
                module.file_id,
                &invocation.target,
                &mut FxHashSet::default(),
            ) {
                consumers.entry(id.clone()).or_default().push(Consumer {
                    file: module.file_id,
                    span: invocation.span_start,
                    supplied: &invocation.supplied,
                    properties: &invocation.supplied_properties,
                    unknown: invocation.unknown_props
                        || (invocation.framework != ComponentFramework::Lit
                            && index.declarations.get(&id).is_some_and(|props| {
                                props
                                    .iter()
                                    .any(|prop| prop.framework == ComponentFramework::Lit)
                            })),
                });
            } else {
                index.block_reference(
                    module.file_id,
                    &invocation.target,
                    invocation.framework,
                    &mut blocked,
                );
            }
        }
    }
    for resolved in input.resolved_modules {
        if !input
            .graph
            .modules
            .get(resolved.file_id.0 as usize)
            .is_some_and(|node| node.is_runtime_reachable())
        {
            continue;
        }
        for import in &resolved.resolved_dynamic_imports {
            if let Some(target) = import.target.internal_file_id() {
                index.surface(target, &mut blocked);
            }
        }
        for (_, targets) in &resolved.resolved_dynamic_patterns {
            for target in targets {
                index.surface(*target, &mut blocked);
            }
        }
    }
    emit_findings(&index, &consumers, &blocked)
}

fn block_public_and_entries(index: &ContractIndex<'_>, blocked: &mut FxHashSet<ComponentId>) {
    for file in index.input.public_api_entry_points {
        index.surface(*file, blocked);
    }
    for module in index.input.modules {
        if let Some(facts) = &module.component_contracts
            && index
                .input
                .graph
                .modules
                .get(module.file_id.0 as usize)
                .is_some_and(|node| node.is_entry_point())
        {
            for export in &facts.exports {
                if export.export_name == "default"
                    && let Some(id) =
                        index.reference(module.file_id, &export.target, &mut FxHashSet::default())
                {
                    blocked.insert(id);
                }
            }
        }
        for name in &module.angular_entry_component_refs {
            blocked.extend(
                index
                    .declarations
                    .keys()
                    .filter(|id| id.name == *name)
                    .cloned(),
            );
        }
        if module.has_dynamic_component_render {
            index.block_framework(ComponentFramework::Angular, blocked);
        }
    }
}

fn emit_findings(
    index: &ContractIndex<'_>,
    consumers: &FxHashMap<ComponentId, Vec<Consumer<'_>>>,
    blocked: &FxHashSet<ComponentId>,
) -> Vec<AbsentComponentProp> {
    let mut findings = Vec::new();
    for (id, props) in &index.declarations {
        if blocked.contains(id) {
            continue;
        }
        let Some(node) = index.input.graph.modules.get(id.file.0 as usize) else {
            continue;
        };
        if !node.is_runtime_reachable() {
            continue;
        }
        let Some(callers) = consumers.get(id) else {
            continue;
        };
        if callers.is_empty() || callers.iter().any(|caller| caller.unknown) {
            continue;
        }
        for prop in props {
            let Some(framework) = index.framework(id.file, prop.framework) else {
                continue;
            };
            if !prop.optional
                || prop.incomplete
                || !(prop.is_used
                    || (framework == ComponentFramework::Angular && index.angular_used(id, prop)))
            {
                continue;
            }
            let supplied = callers.iter().any(|caller| {
                if framework == ComponentFramework::Lit {
                    caller.properties.contains(&prop.name)
                        || caller
                            .supplied
                            .iter()
                            .any(|name| prop.aliases.contains(name))
                } else {
                    caller
                        .supplied
                        .iter()
                        .any(|name| *name == prop.name || prop.aliases.contains(name))
                }
            });
            if supplied {
                continue;
            }
            let (line, col) =
                byte_offset_to_line_col(index.input.line_offsets_by_file, id.file, prop.span_start);
            let mut inspected_call_sites: Vec<_> = callers
                .iter()
                .filter_map(|caller| {
                    let node = index.input.graph.modules.get(caller.file.0 as usize)?;
                    let (line, col) = byte_offset_to_line_col(
                        index.input.line_offsets_by_file,
                        caller.file,
                        caller.span,
                    );
                    Some(ComponentPropCallSite {
                        path: node.path.clone(),
                        line,
                        col,
                    })
                })
                .collect();
            inspected_call_sites.sort_by(|a, b| {
                a.path
                    .cmp(&b.path)
                    .then(a.line.cmp(&b.line))
                    .then(a.col.cmp(&b.col))
            });
            inspected_call_sites.dedup();
            let component_name = if id.name.is_empty() {
                node.path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("")
                    .to_string()
            } else {
                id.name.clone()
            };
            findings.push(AbsentComponentProp {
                path: node.path.clone(),
                component_name,
                framework: framework_name(framework).to_string(),
                prop_name: prop.name.clone(),
                line,
                col,
                has_default: prop.has_default,
                inspected_call_sites,
                explanation: EXPLANATION.to_string(),
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

fn framework_name(framework: ComponentFramework) -> &'static str {
    match framework {
        ComponentFramework::React => "react",
        ComponentFramework::Preact => "preact",
        ComponentFramework::Solid => "solid",
        ComponentFramework::Qwik => "qwik",
        ComponentFramework::Vue => "vue",
        ComponentFramework::Svelte => "svelte",
        ComponentFramework::Astro => "astro",
        ComponentFramework::Angular => "angular",
        ComponentFramework::Lit => "lit",
        ComponentFramework::Ember => "ember",
    }
}
