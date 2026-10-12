//! Effect dead-code rules: `unused-effect-layer`, `unused-tagged-error` and
//! `unproduced-catch-tag`.
//!
//! The rules read the [`EffectFacts`] that extraction records for modules that
//! import Effect, plus the import usage of every module. Names join across
//! modules, and every uncertain signal credits a name. A credit can only
//! remove a finding, so a missed reference gives a false negative, never a
//! false positive.
//!
//! - A layer is unused when no root reaches it through the layer graph and at
//!   least one other layer composes it. A layer with zero references is left
//!   to `unused-export` and `unused-class-member`.
//! - A tagged error is unused when some type position or `catchTag` handler
//!   references it, and no code reads the class as a value. A schema union, an
//!   API error declaration and `new X()` are all value reads.
//! - A `catchTag` handler is unproduced when every project class with its tag
//!   is unused and no `unused-tagged-error` finding covers those classes.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::{EffectFacts, ExportName, ImportedName, ModuleInfo};

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::resolve::ResolvedModule;
use crate::results::{
    EffectCatchSite, EffectLayerKind, UnproducedCatchTag, UnusedEffectLayer, UnusedTaggedError,
};

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Error tags that Effect and the `@effect/*` packages produce. A handler for
/// one of these tags is never reported, even when a project class reuses it.
const BUILTIN_TAGS: &[&str] = &[
    "And",
    "BadArgument",
    "Cause",
    "ConfigError",
    "Die",
    "Empty",
    "ExceededCapacityException",
    "Fail",
    "HttpApiDecodeError",
    "HttpBodyError",
    "IllegalArgumentException",
    "Interrupt",
    "InterruptedException",
    "InvalidData",
    "InvalidPubSubCapacityException",
    "MalformedMessage",
    "MissingData",
    "NoSuchElementException",
    "Or",
    "Parallel",
    "ParseError",
    "PlatformError",
    "RequestError",
    "ResponseError",
    "ResultLengthMismatch",
    "RpcClientError",
    "RuntimeException",
    "SchemaError",
    "Sequential",
    "SocketError",
    "SourceUnavailable",
    "SqlError",
    "SystemError",
    "TimeoutException",
    "UnknownException",
    "Unsupported",
];

/// Inputs of [`find_effect_findings`].
pub(super) struct EffectInput<'a> {
    pub(super) graph: &'a ModuleGraph,
    pub(super) modules: &'a [ModuleInfo],
    pub(super) resolved_modules: &'a [ResolvedModule],
    pub(super) declared_deps: &'a FxHashSet<String>,
    pub(super) public_api_entry_points: &'a FxHashSet<FileId>,
    pub(super) line_offsets_by_file: &'a LineOffsetsMap<'a>,
    pub(super) layers_enabled: bool,
    pub(super) errors_enabled: bool,
    pub(super) catch_tags_enabled: bool,
}

/// The raw findings of the three Effect rules.
#[derive(Default)]
pub(super) struct EffectFindings {
    pub(super) layers: Vec<UnusedEffectLayer>,
    pub(super) errors: Vec<UnusedTaggedError>,
    pub(super) catch_tags: Vec<UnproducedCatchTag>,
}

/// Run the Effect rules. Returns nothing unless the project declares `effect`.
///
/// `is_suppressed(file, line)` reports an `unused-tagged-error` suppression.
/// A suppressed error finding does not cover its `catchTag` handlers, so
/// `unproduced-catch-tag` can still report them.
pub(super) fn find_effect_findings(
    input: &EffectInput<'_>,
    is_suppressed: impl Fn(FileId, u32) -> bool,
) -> EffectFindings {
    let mut findings = EffectFindings::default();
    if !input.declared_deps.contains("effect") {
        return findings;
    }
    let facts: Vec<(FileId, &EffectFacts)> = input
        .modules
        .iter()
        .filter_map(|module| module.effect_facts.as_deref().map(|f| (module.file_id, f)))
        .collect();
    if facts.is_empty() {
        return findings;
    }
    let context = Context::build(input, &facts);
    if input.layers_enabled {
        findings.layers = find_unused_layers(&context);
    }
    let errors = find_unused_errors(&context, &is_suppressed);
    if input.errors_enabled {
        findings.errors = errors.findings;
    }
    if input.catch_tags_enabled {
        findings.catch_tags = find_unproduced_catch_tags(&context, &errors.covered_tags);
    }
    findings
}

struct Context<'a> {
    input: &'a EffectInput<'a>,
    facts: &'a [(FileId, &'a EffectFacts)],
    paths: FxHashMap<FileId, &'a Path>,
    reachable: FxHashSet<FileId>,
    /// Names that some module reads as a value through an import or a
    /// namespace member, after alias resolution.
    value_names: FxHashSet<String>,
    /// Names that some module reads in a type position through an import.
    type_names: FxHashSet<String>,
    /// Files whose every declaration counts as read: a namespace import used
    /// as a whole value, or a dynamic import target.
    whole_files: FxHashSet<FileId>,
    /// Exports that an entry module can expose, directly or through re-export
    /// chains. Code outside the project can use them, so they count as read.
    entry_exports: EntryExports,
    /// Layer keys that a reference outside every layer definition reads.
    layer_roots: FxHashSet<String>,
    /// Class names whose every static member counts as read: a module
    /// without Effect facts reads the class, so its member reads are not
    /// recorded precisely.
    static_wildcards: FxHashSet<String>,
    /// Re-export and local export aliases: exported name to original name.
    aliases: FxHashMap<String, Vec<String>>,
    /// Every caught tag, project-wide.
    caught_tags: FxHashSet<&'a str>,
}

impl<'a> Context<'a> {
    fn build(input: &'a EffectInput<'a>, facts: &'a [(FileId, &'a EffectFacts)]) -> Self {
        let paths = input
            .graph
            .modules
            .iter()
            .map(|module| (module.file_id, module.path.as_path()))
            .collect();
        let reachable = input
            .graph
            .modules
            .iter()
            .filter(|module| module.is_reachable())
            .map(|module| module.file_id)
            .collect();
        let with_facts: FxHashSet<FileId> = facts.iter().map(|(file_id, _)| *file_id).collect();
        let entry_exports = EntryExports::build(input.graph, input.public_api_entry_points);
        let mut context = Self {
            input,
            facts,
            paths,
            reachable,
            value_names: FxHashSet::default(),
            type_names: FxHashSet::default(),
            whole_files: FxHashSet::default(),
            entry_exports,
            layer_roots: FxHashSet::default(),
            static_wildcards: FxHashSet::default(),
            aliases: FxHashMap::default(),
            caught_tags: FxHashSet::default(),
        };
        for resolved in input.resolved_modules {
            context.credit_imports(resolved, with_facts.contains(&resolved.file_id));
        }
        context.collect_aliases();
        for (_, module_facts) in facts {
            context
                .layer_roots
                .extend(module_facts.layer_root_keys.iter().cloned());
            context.caught_tags.extend(
                module_facts
                    .caught_tags
                    .iter()
                    .map(|caught| caught.tag.as_str()),
            );
        }
        context.close_value_names_over_aliases();
        context
    }

    fn credit_imports(&mut self, resolved: &ResolvedModule, has_facts: bool) {
        let value_locals: FxHashSet<&str> = resolved
            .value_referenced_import_bindings
            .iter()
            .map(String::as_str)
            .collect();
        let type_locals: FxHashSet<&str> = resolved
            .type_referenced_import_bindings
            .iter()
            .map(String::as_str)
            .collect();
        for import in &resolved.resolved_imports {
            // A package import cannot name a project layer or error class.
            let Some(target) = import.target.internal_file_id() else {
                continue;
            };
            let local = import.info.local_name.as_str();
            match &import.info.imported_name {
                ImportedName::Named(name) => {
                    if type_locals.contains(local) {
                        self.type_names.insert(name.clone());
                    }
                    if !value_locals.contains(local) {
                        continue;
                    }
                    self.value_names.insert(name.clone());
                    if !has_facts {
                        self.layer_roots.insert(name.clone());
                        self.static_wildcards.insert(name.clone());
                    }
                    // A namespace re-export (`export * as errors`) imported by
                    // name reads its members through this binding.
                    self.credit_members(resolved, local, has_facts);
                }
                ImportedName::Namespace => {
                    if resolved.whole_object_uses.iter().any(|use_| use_ == local) {
                        self.whole_files.insert(target);
                    }
                    self.credit_members(resolved, local, has_facts);
                }
                ImportedName::Default | ImportedName::SideEffect => {}
            }
        }
        for dynamic in &resolved.resolved_dynamic_imports {
            if let Some(file_id) = dynamic.target.internal_file_id() {
                self.whole_files.insert(file_id);
            }
        }
    }

    fn credit_members(&mut self, resolved: &ResolvedModule, local: &str, has_facts: bool) {
        for access in resolved.member_accesses.iter() {
            if access.object != local {
                continue;
            }
            self.value_names.insert(access.member.clone());
            if !has_facts {
                self.layer_roots.insert(access.member.clone());
                self.static_wildcards.insert(access.member.clone());
            }
        }
    }

    fn collect_aliases(&mut self) {
        for resolved in self.input.resolved_modules {
            for re_export in &resolved.re_exports {
                let info = &re_export.info;
                if info.imported_name != info.exported_name && info.imported_name != "*" {
                    self.aliases
                        .entry(info.exported_name.clone())
                        .or_default()
                        .push(info.imported_name.clone());
                }
            }
        }
        for module in self.input.modules {
            for export in module.exports.iter() {
                let (ExportName::Named(name), Some(local)) = (&export.name, &export.local_name)
                else {
                    continue;
                };
                if name != local {
                    self.aliases
                        .entry(name.clone())
                        .or_default()
                        .push(local.clone());
                }
            }
        }
    }

    fn close_value_names_over_aliases(&mut self) {
        let value_names = std::mem::take(&mut self.value_names);
        self.value_names = self.resolve_aliases(value_names);
        let type_names = std::mem::take(&mut self.type_names);
        self.type_names = self.resolve_aliases(type_names);
        let layer_roots = std::mem::take(&mut self.layer_roots);
        self.layer_roots = self.resolve_aliases(layer_roots);
        let static_wildcards = std::mem::take(&mut self.static_wildcards);
        self.static_wildcards = self.resolve_aliases(static_wildcards);
    }

    /// Add the original name of every alias in `names`, also for a static
    /// key (`Alias.layer` adds `Original.layer`).
    fn resolve_aliases(&self, names: FxHashSet<String>) -> FxHashSet<String> {
        let mut out = names.clone();
        let mut queue: Vec<String> = names.into_iter().collect();
        while let Some(name) = queue.pop() {
            let (head, tail) = match name.split_once('.') {
                Some((head, tail)) => (head, Some(tail)),
                None => (name.as_str(), None),
            };
            let Some(originals) = self.aliases.get(head) else {
                continue;
            };
            for original in originals {
                let resolved = match tail {
                    Some(tail) => format!("{original}.{tail}"),
                    None => original.clone(),
                };
                if out.insert(resolved.clone()) {
                    queue.push(resolved);
                }
            }
        }
        out
    }

    /// Whether an entry module can expose `name` of `file_id`, under its own
    /// name or under an export alias (`export { Foo as Bar }`).
    fn is_entry_export(&self, file_id: FileId, name: &str) -> bool {
        self.entry_exports.contains(file_id, name)
            || self.aliases.iter().any(|(exported, originals)| {
                originals.iter().any(|original| original == name)
                    && self.entry_exports.contains(file_id, exported)
            })
    }

    fn path(&self, file_id: FileId) -> Option<&'a Path> {
        self.paths.get(&file_id).copied()
    }

    fn line_col(&self, file_id: FileId, span_start: u32) -> (u32, u32) {
        byte_offset_to_line_col(self.input.line_offsets_by_file, file_id, span_start)
    }
}

/// The exports that entry modules can expose. The walk follows re-export
/// edges from every entry module and public API entry. It over-approximates:
/// a star re-export exposes every name of its source, and a named re-export of
/// a file that also has star re-exports exposes the name in each star source.
/// A larger set only removes findings.
#[derive(Default)]
struct EntryExports {
    /// Files whose every export is exposed.
    all: FxHashSet<FileId>,
    /// Single exposed names in files that are not in `all`.
    named: FxHashSet<(FileId, String)>,
}

impl EntryExports {
    fn build(graph: &ModuleGraph, public_api_entry_points: &FxHashSet<FileId>) -> Self {
        let mut exposed = Self::default();
        let mut queue: Vec<(FileId, Option<String>)> = graph
            .modules
            .iter()
            .filter(|module| module.is_entry_point())
            .map(|module| (module.file_id, None))
            .chain(
                public_api_entry_points
                    .iter()
                    .map(|file_id| (*file_id, None)),
            )
            .collect();
        let modules_by_id: FxHashMap<FileId, usize> = graph
            .modules
            .iter()
            .enumerate()
            .map(|(index, module)| (module.file_id, index))
            .collect();
        while let Some((file_id, name)) = queue.pop() {
            if exposed.all.contains(&file_id) {
                continue;
            }
            match &name {
                None => {
                    exposed.all.insert(file_id);
                }
                Some(name) => {
                    if !exposed.named.insert((file_id, name.clone())) {
                        continue;
                    }
                }
            }
            let Some(&index) = modules_by_id.get(&file_id) else {
                continue;
            };
            for edge in &graph.modules[index].re_exports {
                let star = edge.imported_name == "*";
                match (&name, star) {
                    (None, true) => queue.push((edge.source_file, None)),
                    (None, false) => {
                        queue.push((edge.source_file, Some(edge.imported_name.clone())));
                    }
                    (Some(name), true) if edge.exported_name == *name => {
                        queue.push((edge.source_file, None));
                    }
                    (Some(name), true) => queue.push((edge.source_file, Some(name.clone()))),
                    (Some(name), false) if edge.exported_name == *name => {
                        queue.push((edge.source_file, Some(edge.imported_name.clone())));
                    }
                    (Some(_), false) => {}
                }
            }
        }
        exposed
    }

    fn contains(&self, file_id: FileId, name: &str) -> bool {
        self.all.contains(&file_id) || self.named.contains(&(file_id, name.to_owned()))
    }
}

// ── unused-effect-layer ─────────────────────────────────────────────

fn find_unused_layers(context: &Context<'_>) -> Vec<UnusedEffectLayer> {
    let mut nodes_by_key: FxHashMap<&str, Vec<(FileId, usize)>> = FxHashMap::default();
    for (file_id, facts) in context.facts {
        for (index, layer) in facts.layers.iter().enumerate() {
            nodes_by_key
                .entry(layer.key.as_str())
                .or_default()
                .push((*file_id, index));
        }
    }
    if nodes_by_key.is_empty() {
        return Vec::new();
    }

    let mut queue: Vec<String> = context.layer_roots.iter().cloned().collect();
    for (file_id, facts) in context.facts {
        for layer in &facts.layers {
            let owner = layer
                .key
                .split_once('.')
                .map_or(layer.key.as_str(), |(c, _)| c);
            let is_static = layer.key.contains('.');
            let public = layer.exported && context.is_entry_export(*file_id, owner);
            if public
                || context.whole_files.contains(file_id)
                || (is_static && context.static_wildcards.contains(owner))
            {
                queue.push(layer.key.clone());
            }
        }
    }

    let reached = reach(context, &nodes_by_key, queue);
    let mut consumers: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for (_, facts) in context.facts {
        for layer in &facts.layers {
            for dependency in &layer.dependencies {
                consumers
                    .entry(layer_key_of(dependency, &nodes_by_key))
                    .or_default()
                    .push(layer.key.as_str());
            }
        }
    }

    let mut findings = Vec::new();
    for (file_id, facts) in context.facts {
        if !context.reachable.contains(file_id) {
            continue;
        }
        let Some(path) = context.path(*file_id) else {
            continue;
        };
        for layer in &facts.layers {
            if reached.contains(layer.key.as_str()) {
                continue;
            }
            let Some(dead_consumers) = consumers.get(layer.key.as_str()) else {
                continue;
            };
            let mut dead_consumers: Vec<String> = dead_consumers
                .iter()
                .map(|key| (*key).to_string())
                .collect();
            dead_consumers.sort_unstable();
            dead_consumers.dedup();
            let (line, col) = context.line_col(*file_id, layer.span_start);
            findings.push(UnusedEffectLayer {
                path: path.to_path_buf(),
                name: layer.key.clone(),
                kind: if layer.key.contains('.') {
                    EffectLayerKind::StaticMember
                } else {
                    EffectLayerKind::TopLevel
                },
                line,
                col,
                dead_consumers,
            });
        }
    }
    findings
}

/// The layer key a reference key reads. `FooLive.pipe` reads the const
/// layer `FooLive`; `Foo.layer` reads the static layer `Foo.layer`.
fn layer_key_of<'k>(key: &'k str, nodes_by_key: &FxHashMap<&str, Vec<(FileId, usize)>>) -> &'k str {
    if nodes_by_key.contains_key(key) {
        return key;
    }
    key.split_once('.').map_or(key, |(head, _)| head)
}

fn reach<'k>(
    context: &'k Context<'_>,
    nodes_by_key: &FxHashMap<&'k str, Vec<(FileId, usize)>>,
    mut queue: Vec<String>,
) -> FxHashSet<&'k str> {
    let facts_by_file: FxHashMap<FileId, &EffectFacts> = context.facts.iter().copied().collect();
    let mut reached: FxHashSet<&'k str> = FxHashSet::default();
    while let Some(key) = queue.pop() {
        let key = layer_key_of(&key, nodes_by_key);
        let Some((&node_key, nodes)) = nodes_by_key.get_key_value(key) else {
            continue;
        };
        if !reached.insert(node_key) {
            continue;
        }
        for (file_id, index) in nodes {
            if let Some(facts) = facts_by_file.get(file_id) {
                queue.extend(facts.layers[*index].dependencies.iter().cloned());
            }
        }
    }
    reached
}

// ── unused-tagged-error ─────────────────────────────────────────────

struct ErrorOutcome {
    findings: Vec<UnusedTaggedError>,
    /// Tags whose unused classes an `unused-tagged-error` finding reports.
    covered_tags: FxHashSet<String>,
}

fn error_is_produced(
    context: &Context<'_>,
    file_id: FileId,
    error: &fallow_types::extract::EffectTaggedError,
) -> bool {
    error.local_value_reference
        || context.value_names.contains(&error.name)
        || context.whole_files.contains(&file_id)
        || (error.exported && context.is_entry_export(file_id, &error.name))
}

fn find_unused_errors(
    context: &Context<'_>,
    is_suppressed: &impl Fn(FileId, u32) -> bool,
) -> ErrorOutcome {
    let catch_sites = catch_sites_by_tag(context);
    let mut outcome = ErrorOutcome {
        findings: Vec::new(),
        covered_tags: FxHashSet::default(),
    };
    for (file_id, facts) in context.facts {
        if !context.reachable.contains(file_id) {
            continue;
        }
        let Some(path) = context.path(*file_id) else {
            continue;
        };
        for error in &facts.tagged_errors {
            if error_is_produced(context, *file_id, error) {
                continue;
            }
            let referenced = error.local_type_reference
                || context.type_names.contains(&error.name)
                || context.caught_tags.contains(error.tag.as_str());
            if !referenced {
                continue;
            }
            let (line, col) = context.line_col(*file_id, error.span_start);
            if !context.input.errors_enabled || is_suppressed(*file_id, line) {
                continue;
            }
            outcome.covered_tags.insert(error.tag.clone());
            outcome.findings.push(UnusedTaggedError {
                path: path.to_path_buf(),
                name: error.name.clone(),
                tag: error.tag.clone(),
                line,
                col,
                catch_sites: catch_sites
                    .get(error.tag.as_str())
                    .cloned()
                    .unwrap_or_default(),
            });
        }
    }
    outcome
}

fn catch_sites_by_tag<'a>(context: &Context<'a>) -> FxHashMap<&'a str, Vec<EffectCatchSite>> {
    let mut sites: FxHashMap<&str, Vec<EffectCatchSite>> = FxHashMap::default();
    for (file_id, facts) in context.facts {
        if !context.reachable.contains(file_id) {
            continue;
        }
        let Some(path) = context.path(*file_id) else {
            continue;
        };
        for caught in &facts.caught_tags {
            let (line, col) = context.line_col(*file_id, caught.span_start);
            sites
                .entry(caught.tag.as_str())
                .or_default()
                .push(EffectCatchSite {
                    path: path.to_path_buf(),
                    line,
                    col,
                });
        }
    }
    for list in sites.values_mut() {
        list.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.line.cmp(&b.line))
                .then(a.col.cmp(&b.col))
        });
    }
    sites
}

// ── unproduced-catch-tag ────────────────────────────────────────────

fn find_unproduced_catch_tags(
    context: &Context<'_>,
    covered_tags: &FxHashSet<String>,
) -> Vec<UnproducedCatchTag> {
    let mut produced: FxHashSet<&str> = FxHashSet::default();
    let mut classes_by_tag: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for (file_id, facts) in context.facts {
        produced.extend(facts.literal_tags.iter().map(String::as_str));
        for error in &facts.tagged_errors {
            classes_by_tag
                .entry(error.tag.as_str())
                .or_default()
                .push(error.name.as_str());
            if error_is_produced(context, *file_id, error) {
                produced.insert(error.tag.as_str());
            }
        }
    }

    let mut findings = Vec::new();
    for (file_id, facts) in context.facts {
        if !context.reachable.contains(file_id) {
            continue;
        }
        let Some(path) = context.path(*file_id) else {
            continue;
        };
        for caught in &facts.caught_tags {
            let tag = caught.tag.as_str();
            if BUILTIN_TAGS.contains(&tag) || produced.contains(tag) || covered_tags.contains(tag) {
                continue;
            }
            let Some(classes) = classes_by_tag.get(tag) else {
                continue;
            };
            let mut candidate_classes: Vec<String> =
                classes.iter().map(|name| (*name).to_string()).collect();
            candidate_classes.sort_unstable();
            candidate_classes.dedup();
            let (line, col) = context.line_col(*file_id, caught.span_start);
            findings.push(UnproducedCatchTag {
                path: path.to_path_buf(),
                tag: caught.tag.clone(),
                line,
                col,
                candidate_classes,
            });
        }
    }
    findings
}
