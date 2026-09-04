#[allow(
    clippy::wildcard_imports,
    reason = "object binding helpers use AST node shapes"
)]
use oxc_ast::ast::*;
use rustc_hash::FxHashSet;

use super::super::{
    BindingTarget, ExportName, ExportedObjectInstancePropertyFact, ModuleInfoExtractor,
    ObjectBindingCandidate, SemanticFact,
};

/// Per-module breadth cap on recorded object-binding targets (issue #1843
/// follow-up): the companion to `MAX_TAINTED_BINDINGS_PER_MODULE` for the
/// `const obj = { key: ident }` and `const obj = { key: new Class() }` channels.
/// Identifier candidates are resolved by a fixpoint pass with an O(n^2) worst
/// case. Direct targets are preseeded into scope maps, so both channels need a
/// bounded working set on dense machine-generated bundles. Past the cap no new
/// target is recorded, matching the false-negative-preferring direction of the
/// taint caps. Deliberately a constant, not a config knob: real hand-written
/// modules stay far below it.
const MAX_OBJECT_BINDING_TARGETS: usize = 4096;
const MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS: usize = 8192;

#[derive(Clone, Copy)]
enum DirectObjectBindingScope {
    Current,
    BindingOwner,
}

impl ModuleInfoExtractor {
    pub(in crate::visitor) fn record_exported_direct_object_binding_facts(&mut self) {
        let exported_roots = self
            .exports
            .iter()
            .filter(|export| !export.is_type_only)
            .filter_map(|export| {
                let local_name = match (export.local_name.as_deref(), &export.name) {
                    (Some(local_name), _) => local_name,
                    (None, ExportName::Named(name)) => name.as_str(),
                    (None, ExportName::Default) => return None,
                };
                Some((local_name.to_string(), export.name.to_string()))
            })
            .collect::<Vec<_>>();
        let mut facts = Vec::new();
        let mut seen = FxHashSet::default();
        let mut overflowed = false;
        'exports: for (root_name, export_name) in exported_roots {
            let Some(paths) = self
                .module_direct_object_binding_paths_by_root
                .get(root_name.as_str())
            else {
                continue;
            };
            let prefix = format!("{root_name}.");
            let mut paths = paths.iter().collect::<Vec<_>>();
            paths.sort_unstable();
            for path in paths {
                let Some(property_path) = path.strip_prefix(&prefix) else {
                    continue;
                };
                let Some(class_names) = self.module_direct_object_binding_targets.get(path) else {
                    continue;
                };
                let mut class_names = class_names.iter().collect::<Vec<_>>();
                class_names.sort_unstable();
                for class_local_name in class_names {
                    let fact = (
                        export_name.clone(),
                        property_path.to_string(),
                        class_local_name.clone(),
                    );
                    if !seen.insert(fact.clone()) {
                        continue;
                    }
                    if facts.len() >= MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS {
                        overflowed = true;
                        break 'exports;
                    }
                    facts.push(fact);
                }
            }
        }
        if overflowed {
            let class_names = self
                .module_direct_object_binding_targets
                .values()
                .flatten()
                .cloned()
                .collect::<FxHashSet<_>>();
            for class_name in class_names {
                if self
                    .direct_object_binding_whole_object_uses
                    .insert(class_name.clone())
                {
                    self.whole_object_uses.push(class_name);
                }
            }
        }
        self.semantic_facts.extend(facts.into_iter().map(
            |(export_name, property_path, class_local_name)| {
                SemanticFact::ExportedObjectInstanceProperty(ExportedObjectInstancePropertyFact {
                    export_name,
                    property_path,
                    class_local_name,
                })
            },
        ));
    }

    pub(super) fn extract_angular_inject_target(
        &self,
        call: &CallExpression<'_>,
    ) -> Option<String> {
        super::super::helpers::extract_angular_inject_target(
            call,
            &|local_name, source, imported_name| {
                self.is_named_import_from(local_name, source, imported_name)
            },
        )
    }

    pub(super) fn copy_nested_binding_targets(
        &mut self,
        source_binding: &str,
        target_binding: &str,
    ) -> bool {
        // Nothing to copy from an empty map: skip the two `format!` allocations
        // and the no-op scan/collect below.
        if self.binding_target_names.is_empty() {
            return false;
        }
        let source_prefix = format!("{source_binding}.");
        let target_prefix = format!("{target_binding}.");
        // Prefix-index fast-path (issue #1843 follow-up): during the object-binding
        // fixed-point, enumerate the keys under `source_binding.` in O(matches) via
        // the index instead of scanning all of `binding_target_names`, which is
        // what made a real minified bundle full of nested object maps take tens of
        // seconds. Outside the pass (`None`) the map is small and the full scan is
        // used. Both branches produce the same `(binding, target)` set.
        let copied: Vec<(String, BindingTarget)> =
            if let Some(index) = &self.binding_target_prefix_index {
                index
                    .get(source_binding)
                    .into_iter()
                    .flatten()
                    .filter_map(|key| {
                        self.binding_target_names.get(key).map(|target| {
                            let suffix = &key[source_prefix.len()..];
                            (format!("{target_prefix}{suffix}"), target.clone())
                        })
                    })
                    .collect()
            } else {
                self.binding_target_names
                    .iter()
                    .filter_map(|(binding, target)| {
                        binding
                            .strip_prefix(&source_prefix)
                            .map(|suffix| (format!("{target_prefix}{suffix}"), target.clone()))
                    })
                    .collect()
            };

        let mut changed = false;
        for (binding, target) in copied {
            changed |= self.insert_binding_target(binding, target);
        }
        changed
    }

    fn insert_binding_target(&mut self, binding: String, target: BindingTarget) -> bool {
        if self.binding_target_names.get(&binding) == Some(&target) {
            return false;
        }
        // Hard size cap on the object-binding fixed-point's growth (issue #1843
        // follow-up). A pathological minified bundle (huge object maps copied
        // across many bindings) makes the fixed-point multiply
        // `binding_target_names` without bound, taking tens of seconds. Once the
        // map reaches the cap, stop recording NEW keys (an over-cap chain degrades
        // to a false negative, matching the FN-preferring doctrine); updates to an
        // already-present key still apply. Only reached via the fixed-point (the
        // index is `Some`), so the walk-time member crediting is unaffected.
        const MAX_BINDING_TARGET_NAMES: usize = 8192;
        if self.binding_target_prefix_index.is_some()
            && self.binding_target_names.len() >= MAX_BINDING_TARGET_NAMES
            && !self.binding_target_names.contains_key(&binding)
        {
            return false;
        }
        // Keep the ancestor-prefix index current for inserts made during the
        // fixed-point (issue #1843 follow-up), so a key added this pass is visible
        // to a later `copy_nested_binding_targets` call under every prefix.
        if let Some(index) = self.binding_target_prefix_index.as_mut() {
            for (dot, _) in binding.match_indices('.') {
                index
                    .entry(binding[..dot].to_string())
                    .or_default()
                    .push(binding.clone());
            }
        }
        self.binding_target_names.insert(binding, target);
        true
    }

    pub(in crate::visitor) fn resolve_object_binding_candidate(
        &mut self,
        candidate: &ObjectBindingCandidate,
    ) -> bool {
        let mut changed = false;
        if self
            .namespace_binding_names
            .iter()
            .any(|name| name == candidate.source_name.as_str())
        {
            changed |= self.insert_binding_target(
                candidate.binding_path.clone(),
                BindingTarget::Class(candidate.source_name.clone()),
            );
        } else if let Some(target_name) = self
            .binding_target_names
            .get(candidate.source_name.as_str())
            .cloned()
        {
            changed |= self.insert_binding_target(candidate.binding_path.clone(), target_name);
        }
        changed | self.copy_nested_binding_targets(&candidate.source_name, &candidate.binding_path)
    }

    pub(super) fn record_object_binding_targets(
        &mut self,
        binding_name: &str,
        obj: &ObjectExpression<'_>,
    ) {
        self.record_object_binding_targets_at_path(binding_name, binding_name, obj);
    }

    pub(super) fn preseed_direct_object_binding_targets(&mut self, statements: &[Statement<'_>]) {
        for statement in statements {
            let declaration = match statement {
                Statement::ExportNamedDeclaration(export) => export.declaration.as_ref(),
                _ => statement.as_declaration(),
            };
            let Some(Declaration::VariableDeclaration(declaration)) = declaration else {
                continue;
            };
            self.preseed_direct_object_binding_declaration(declaration);
        }
    }

    pub(super) fn preseed_direct_object_binding_declaration(
        &mut self,
        declaration: &VariableDeclaration<'_>,
    ) {
        if !declaration.kind.is_lexical() {
            return;
        }
        for declarator in &declaration.declarations {
            let Some(init) = declarator.init.as_ref() else {
                continue;
            };
            match (&declarator.id, init) {
                (BindingPattern::BindingIdentifier(id), Expression::ObjectExpression(object)) => {
                    self.record_direct_object_binding_targets_at_path(
                        id.name.as_str(),
                        id.name.as_str(),
                        object,
                        DirectObjectBindingScope::Current,
                    );
                }
                (BindingPattern::BindingIdentifier(id), _) => {
                    if let Some(source_path) = direct_object_binding_expression_path(init) {
                        self.copy_direct_object_binding_targets(&source_path, id.name.as_str());
                    }
                }
                (BindingPattern::ObjectPattern(pattern), _) => {
                    self.record_direct_object_binding_destructure(pattern, init);
                }
                _ => {}
            }
        }
    }

    fn record_direct_object_binding_destructure(
        &mut self,
        pattern: &ObjectPattern<'_>,
        init: &Expression<'_>,
    ) {
        let Some(source_path) = direct_object_binding_expression_path(init) else {
            return;
        };
        self.record_direct_object_binding_object_pattern(pattern, &source_path);
    }

    fn record_direct_object_binding_object_pattern(
        &mut self,
        pattern: &ObjectPattern<'_>,
        source_path: &str,
    ) {
        for property in &pattern.properties {
            let Some(property_name) = property.key.static_name() else {
                continue;
            };
            self.record_direct_object_binding_pattern(
                &property.value,
                &format!("{source_path}.{property_name}"),
            );
        }
    }

    fn record_direct_object_binding_pattern(
        &mut self,
        pattern: &BindingPattern<'_>,
        source_path: &str,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                self.copy_direct_object_binding_targets(source_path, binding.name.as_str());
            }
            BindingPattern::ObjectPattern(pattern) => {
                self.record_direct_object_binding_object_pattern(pattern, source_path);
            }
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_pattern(&assignment.left, source_path);
                self.record_direct_object_binding_fallback(&assignment.left, &assignment.right);
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }

    fn record_direct_object_binding_fallback(
        &mut self,
        pattern: &BindingPattern<'_>,
        fallback: &Expression<'_>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => match fallback {
                Expression::NewExpression(new_expression) => {
                    if let Some(class_name) = self.direct_new_expression_class_name(new_expression)
                    {
                        self.record_direct_object_binding_target(
                            binding.name.as_str(),
                            binding.name.to_string(),
                            class_name,
                            DirectObjectBindingScope::BindingOwner,
                        );
                    }
                }
                Expression::ObjectExpression(object) => {
                    self.record_direct_object_binding_targets_at_path(
                        binding.name.as_str(),
                        binding.name.as_str(),
                        object,
                        DirectObjectBindingScope::BindingOwner,
                    );
                }
                fallback => {
                    if let Some(fallback_path) = direct_object_binding_expression_path(fallback) {
                        self.copy_direct_object_binding_targets(
                            &fallback_path,
                            binding.name.as_str(),
                        );
                    }
                }
            },
            BindingPattern::ObjectPattern(pattern) => match fallback {
                Expression::ObjectExpression(object) => {
                    for property in &pattern.properties {
                        let Some(property_name) = property.key.static_name() else {
                            continue;
                        };
                        let Some(value) = object.properties.iter().rev().find_map(|candidate| {
                            let ObjectPropertyKind::ObjectProperty(candidate) = candidate else {
                                return None;
                            };
                            (candidate.key.static_name().as_deref() == Some(property_name.as_ref()))
                                .then_some(&candidate.value)
                        }) else {
                            continue;
                        };
                        self.record_direct_object_binding_fallback(&property.value, value);
                    }
                }
                fallback => {
                    if let Some(fallback_path) = direct_object_binding_expression_path(fallback) {
                        self.record_direct_object_binding_object_pattern(pattern, &fallback_path);
                    }
                }
            },
            BindingPattern::AssignmentPattern(assignment) => {
                self.record_direct_object_binding_fallback(&assignment.left, fallback);
                self.record_direct_object_binding_fallback(&assignment.left, &assignment.right);
            }
            BindingPattern::ArrayPattern(_) => {}
        }
    }

    pub(super) fn record_direct_object_binding_assignment(
        &mut self,
        binding_path: &str,
        value: &Expression<'_>,
        is_plain_assignment: bool,
    ) {
        let root_name = binding_path
            .split_once('.')
            .map_or(binding_path, |(root, _)| root);
        if !is_plain_assignment {
            return;
        }

        match value {
            Expression::NewExpression(new_expression) => {
                if let Some(class_name) = self.direct_new_expression_class_name(new_expression) {
                    self.record_direct_object_binding_target(
                        root_name,
                        binding_path.to_string(),
                        class_name,
                        DirectObjectBindingScope::BindingOwner,
                    );
                }
            }
            Expression::ObjectExpression(object) => {
                self.record_direct_object_binding_targets_at_path(
                    root_name,
                    binding_path,
                    object,
                    DirectObjectBindingScope::BindingOwner,
                );
            }
            _ => {
                if let Some(source_path) = direct_object_binding_expression_path(value) {
                    self.copy_direct_object_binding_targets(&source_path, binding_path);
                }
            }
        }
    }

    fn copy_direct_object_binding_targets(&mut self, source_path: &str, target_path: &str) {
        let source_root = source_path
            .split_once('.')
            .map_or(source_path, |(root, _)| root);
        let nested_prefix = format!("{source_path}.");
        let mut copied = Vec::new();
        let mut shadowed = false;

        for index in (0..self.scoped_direct_object_binding_targets.len()).rev() {
            if let Some(paths) =
                self.scoped_direct_object_binding_paths_by_root[index].get(source_root)
            {
                copied.extend(paths.iter().filter_map(|path| {
                    if path != source_path && !path.starts_with(&nested_prefix) {
                        return None;
                    }
                    let suffix = &path[source_path.len()..];
                    self.scoped_direct_object_binding_targets[index]
                        .get(path)
                        .map(|targets| (format!("{target_path}{suffix}"), targets.clone()))
                }));
                shadowed = true;
                break;
            }
            if self.scoped_direct_object_binding_roots[index].contains(source_root) {
                shadowed = true;
                break;
            }
        }

        if !shadowed
            && let Some(paths) = self
                .module_direct_object_binding_paths_by_root
                .get(source_root)
        {
            copied.extend(paths.iter().filter_map(|path| {
                if path != source_path && !path.starts_with(&nested_prefix) {
                    return None;
                }
                let suffix = &path[source_path.len()..];
                self.module_direct_object_binding_targets
                    .get(path)
                    .map(|targets| (format!("{target_path}{suffix}"), targets.clone()))
            }));
        }

        let target_root = target_path
            .split_once('.')
            .map_or(target_path, |(root, _)| root);
        for (path, targets) in copied {
            for class_name in targets {
                self.record_direct_object_binding_target(
                    target_root,
                    path.clone(),
                    class_name,
                    DirectObjectBindingScope::BindingOwner,
                );
            }
        }
    }

    fn object_binding_target_count(&self) -> usize {
        self.object_binding_candidates
            .len()
            .saturating_add(self.direct_object_binding_target_count)
    }

    fn record_direct_object_binding_target(
        &mut self,
        root_name: &str,
        binding_path: String,
        class_name: String,
        scope: DirectObjectBindingScope,
    ) {
        let scope_index = self.direct_object_binding_scope_index(scope, root_name);
        if let Some(index) = scope_index {
            self.scoped_direct_object_binding_roots[index].insert(root_name.to_string());
        }
        let is_new_target = if let Some(index) = scope_index {
            self.scoped_direct_object_binding_targets[index]
                .get(&binding_path)
                .is_none_or(|targets| !targets.contains(&class_name))
        } else {
            self.module_direct_object_binding_targets
                .get(&binding_path)
                .is_none_or(|targets| !targets.contains(&class_name))
        };
        if self
            .abstained_direct_object_binding_paths
            .contains(&binding_path)
            && self
                .direct_object_binding_whole_object_uses
                .insert(class_name.clone())
        {
            self.whole_object_uses.push(class_name.clone());
        }
        if is_new_target && self.object_binding_target_count() >= MAX_OBJECT_BINDING_TARGETS {
            self.abstain_direct_object_binding_path(&binding_path);
            if self
                .direct_object_binding_whole_object_uses
                .insert(class_name.clone())
            {
                self.whole_object_uses.push(class_name);
            }
            return;
        }
        if is_new_target {
            self.direct_object_binding_target_count += 1;
            self.direct_object_binding_generation =
                self.direct_object_binding_generation.saturating_add(1);
            if let Some(index) = scope_index {
                self.scoped_direct_object_binding_generations[index]
                    .insert(binding_path.clone(), self.direct_object_binding_generation);
            } else {
                self.module_direct_object_binding_generations
                    .insert(binding_path.clone(), self.direct_object_binding_generation);
            }
        }

        if let Some(index) = scope_index {
            self.scoped_direct_object_binding_paths_by_root[index]
                .entry(root_name.to_string())
                .or_default()
                .insert(binding_path.clone());
            self.scoped_direct_object_binding_targets[index]
                .entry(binding_path)
                .or_default()
                .insert(class_name);
        } else {
            self.module_direct_object_binding_paths_by_root
                .entry(root_name.to_string())
                .or_default()
                .insert(binding_path.clone());
            self.module_direct_object_binding_targets
                .entry(binding_path)
                .or_default()
                .insert(class_name);
        }
    }

    fn direct_object_binding_scope_index(
        &self,
        scope: DirectObjectBindingScope,
        root_name: &str,
    ) -> Option<usize> {
        match scope {
            DirectObjectBindingScope::Current => self
                .scoped_direct_object_binding_targets
                .len()
                .checked_sub(1),
            DirectObjectBindingScope::BindingOwner => self
                .scoped_direct_object_binding_roots
                .iter()
                .rposition(|roots| roots.contains(root_name)),
        }
    }

    fn record_direct_object_binding_targets_at_path(
        &mut self,
        root_name: &str,
        object_path: &str,
        obj: &ObjectExpression<'_>,
        scope: DirectObjectBindingScope,
    ) {
        let mut seen_keys = FxHashSet::default();
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                self.remove_direct_object_binding_targets_at_or_below(object_path, scope);
                seen_keys.clear();
                continue;
            };
            let Some(key_name) = prop.key.static_name() else {
                self.remove_direct_object_binding_targets_at_or_below(object_path, scope);
                seen_keys.clear();
                continue;
            };
            let binding_path = format!("{object_path}.{key_name}");
            if !seen_keys.insert(key_name.to_string()) {
                self.remove_direct_object_binding_targets_at_or_below(&binding_path, scope);
            }
            match &prop.value {
                Expression::NewExpression(new_expr) => {
                    if let Some(class_name) = self.direct_new_expression_class_name(new_expr) {
                        self.record_direct_object_binding_target(
                            root_name,
                            binding_path,
                            class_name,
                            scope,
                        );
                    }
                }
                Expression::ObjectExpression(child) => {
                    self.record_direct_object_binding_targets_at_path(
                        root_name,
                        &binding_path,
                        child,
                        scope,
                    );
                }
                _ => {}
            }
        }
    }

    fn remove_direct_object_binding_targets_at_or_below(
        &mut self,
        prefix: &str,
        scope: DirectObjectBindingScope,
    ) {
        let nested_prefix = format!("{prefix}.");
        let root_name = prefix.split_once('.').map_or(prefix, |(root, _)| root);
        if let Some(index) = self.direct_object_binding_scope_index(scope, root_name) {
            let removed = self.scoped_direct_object_binding_paths_by_root[index]
                .get(root_name)
                .map(|paths| {
                    paths
                        .iter()
                        .filter(|path| *path == prefix || path.starts_with(&nested_prefix))
                        .cloned()
                        .collect::<FxHashSet<_>>()
                })
                .unwrap_or_default();
            for path in &removed {
                self.scoped_direct_object_binding_targets[index].remove(path);
                self.scoped_direct_object_binding_generations[index].remove(path);
            }
            if !removed.is_empty() {
                self.direct_object_binding_generation =
                    self.direct_object_binding_generation.saturating_add(1);
            }
            if let Some(paths) =
                self.scoped_direct_object_binding_paths_by_root[index].get_mut(root_name)
            {
                paths.retain(|path| !removed.contains(path));
                if paths.is_empty() {
                    self.scoped_direct_object_binding_paths_by_root[index].remove(root_name);
                }
            }
        } else {
            let removed = self
                .module_direct_object_binding_paths_by_root
                .get(root_name)
                .map(|paths| {
                    paths
                        .iter()
                        .filter(|path| *path == prefix || path.starts_with(&nested_prefix))
                        .cloned()
                        .collect::<FxHashSet<_>>()
                })
                .unwrap_or_default();
            for path in &removed {
                self.module_direct_object_binding_targets.remove(path);
                self.module_direct_object_binding_generations.remove(path);
            }
            if !removed.is_empty() {
                self.direct_object_binding_generation =
                    self.direct_object_binding_generation.saturating_add(1);
            }
            if let Some(paths) = self
                .module_direct_object_binding_paths_by_root
                .get_mut(root_name)
            {
                paths.retain(|path| !removed.contains(path));
                if paths.is_empty() {
                    self.module_direct_object_binding_paths_by_root
                        .remove(root_name);
                }
            }
        }
    }

    fn record_object_binding_targets_at_path(
        &mut self,
        root_name: &str,
        object_path: &str,
        obj: &ObjectExpression<'_>,
    ) {
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            let Some(key_name) = prop.key.static_name() else {
                continue;
            };

            let binding_path = format!("{object_path}.{key_name}");
            match &prop.value {
                // Per-module breadth cap (issue #1843 follow-up): the guard stops
                // recording once at capacity so a pathological object literal
                // cannot grow the candidate set (and its O(n^2) fixpoint resolver)
                // without bound. At capacity the arm falls through to the no-op
                // `_ =>` arm, identical to skipping the push.
                Expression::Identifier(ident)
                    if self.object_binding_target_count() < MAX_OBJECT_BINDING_TARGETS =>
                {
                    // The candidate gives this placement a precise path, which
                    // `resolve_object_binding_candidates` turns back into
                    // `<source>.<member>` accesses, so a namespace placed here
                    // is resolved rather than handed over bare. What hands it
                    // to, which `record_object_literal_namespace_pass` covers.
                    // See issue #2377.
                    self.record_object_literal_namespace_placement(root_name, ident);
                    self.object_binding_candidates.push(ObjectBindingCandidate {
                        binding_path,
                        source_name: ident.name.to_string(),
                    });
                }
                Expression::ObjectExpression(child) => {
                    self.record_object_binding_targets_at_path(root_name, &binding_path, child);
                }
                _ => {}
            }
        }
    }

    fn direct_new_expression_class_name(&self, expression: &NewExpression<'_>) -> Option<String> {
        match &expression.callee {
            Expression::Identifier(callee) => {
                if super::super::helpers::is_builtin_constructor(callee.name.as_str()) {
                    return None;
                }
                Some(callee.name.to_string())
            }
            callee => {
                let path = direct_object_binding_expression_path(callee)?;
                let (namespace_local, export_name) = path.split_once('.')?;
                (self.namespace_import_locals.contains(namespace_local)
                    && !export_name.contains('.')
                    && !self
                        .scoped_direct_object_binding_roots
                        .iter()
                        .any(|roots| roots.contains(namespace_local))
                    && !self.namespace_like_binding_is_shadowed(namespace_local))
                .then_some(path)
            }
        }
    }
}

fn direct_object_binding_expression_path(expression: &Expression<'_>) -> Option<String> {
    match expression {
        Expression::Identifier(identifier) => Some(identifier.name.to_string()),
        Expression::StaticMemberExpression(member) => Some(format!(
            "{}.{}",
            direct_object_binding_expression_path(&member.object)?,
            member.property.name
        )),
        Expression::ComputedMemberExpression(member) => Some(format!(
            "{}.{}",
            direct_object_binding_expression_path(&member.object)?,
            member.static_property_name()?
        )),
        Expression::ParenthesizedExpression(parenthesized) => {
            direct_object_binding_expression_path(&parenthesized.expression)
        }
        Expression::TSAsExpression(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        Expression::TSSatisfiesExpression(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        Expression::TSNonNullExpression(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        Expression::TSTypeAssertion(assertion) => {
            direct_object_binding_expression_path(&assertion.expression)
        }
        _ => None,
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::{MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS, MAX_OBJECT_BINDING_TARGETS};
    use crate::SemanticFact;
    use crate::visitor::{MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES, ModuleInfoExtractor};
    use oxc_allocator::Allocator;
    use oxc_ast_visit::Visit;
    use oxc_parser::Parser;
    use oxc_span::SourceType;
    use rustc_hash::FxHashSet;

    /// A single object literal with far more identifier-valued properties than
    /// the per-module cap must not grow `object_binding_candidates` past the cap.
    /// Mirrors `tainted_binding_recording_is_bounded_on_dense_source`: the
    /// object-binding channel has the same super-linear failure mode (an O(n^2)
    /// fixpoint resolver over an unbounded candidate set) on dense machine-
    /// generated source, and the cap degrades over-cap files to module-level
    /// reachability rather than OOMing. See issue #1843 follow-up.
    #[test]
    fn object_binding_candidate_recording_is_bounded_on_dense_source() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut props = String::new();
        for k in 0..over_cap {
            // Each identifier-valued property seeds one object-binding candidate.
            let _ = write!(props, "k{k}: v{k}, ");
        }
        let source = format!("const big = {{ {props} }};");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        // The cap must engage (input deterministically exceeds it) but never
        // zero out recording.
        assert!(
            !extractor.object_binding_candidates.is_empty(),
            "the cap must not zero out object-binding recording"
        );
        assert!(
            extractor.object_binding_candidates.len() <= MAX_OBJECT_BINDING_TARGETS,
            "object-binding candidate recording must stay bounded at the \
             per-module cap on dense source (got {})",
            extractor.object_binding_candidates.len()
        );
    }

    #[test]
    fn exported_object_instance_property_facts_are_bounded() {
        use std::fmt::Write as _;

        let mut exports = String::new();
        for index in 0..=MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS {
            let _ = writeln!(exports, "export {{ holder as alias{index} }};");
        }
        let source = format!(
            "import * as NS from './services';\nconst holder = {{ service: new NS.Service() }};\n{exports}"
        );

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);
        extractor.resolve_pending_local_export_specifiers();
        extractor.record_exported_direct_object_binding_facts();

        let fact_count = extractor
            .semantic_facts
            .iter()
            .filter(|fact| matches!(fact, SemanticFact::ExportedObjectInstanceProperty(_)))
            .count();
        assert_eq!(fact_count, MAX_EXPORTED_OBJECT_INSTANCE_PROPERTY_FACTS);
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("NS.Service"),
            "cap exhaustion must conservatively credit every possible qualified class"
        );
    }

    #[test]
    fn direct_object_binding_target_recording_is_bounded_on_dense_source() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut props = String::new();
        for k in 0..over_cap {
            let _ = write!(props, "k{k}: new Service(), ");
        }
        let source = format!("const big = {{ {props} }};");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        assert_eq!(
            extractor.direct_object_binding_target_count, MAX_OBJECT_BINDING_TARGETS,
            "the direct target cap should engage on dense source"
        );
        assert!(
            extractor.module_direct_object_binding_targets.len() <= MAX_OBJECT_BINDING_TARGETS,
            "direct object binding targets must keep the scope map bounded"
        );
    }

    #[test]
    fn direct_object_binding_associations_are_bounded_on_repeated_path() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut assignments = String::new();
        for k in 0..over_cap {
            let _ = writeln!(assignments, "holder.property = new Service{k}();");
        }
        let source = format!("const holder = {{ property: new InitialService() }};\n{assignments}");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        assert_eq!(
            extractor.direct_object_binding_target_count, MAX_OBJECT_BINDING_TARGETS,
            "the direct target cap should include repeated-path associations"
        );
        assert!(
            extractor
                .module_direct_object_binding_targets
                .get("holder.property")
                .is_some_and(|targets| targets.len() <= MAX_OBJECT_BINDING_TARGETS),
            "a repeated path must not accumulate unbounded class targets"
        );

        extractor.record_direct_object_binding_target(
            "holder",
            "holder.property".to_string(),
            "ServiceAfterCap".to_string(),
            super::DirectObjectBindingScope::BindingOwner,
        );
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("ServiceAfterCap"),
            "an over-cap target must receive conservative whole-object credit"
        );
    }

    #[test]
    fn direct_object_binding_member_access_emission_is_bounded_and_deduplicated() {
        let mut extractor = ModuleInfoExtractor::new();
        let classes = (0..128).map(|index| format!("Service{index}")).collect();
        extractor
            .module_direct_object_binding_targets
            .insert("holder.property".to_string(), classes);

        for index in 0..128 {
            assert!(
                extractor
                    .record_walk_order_member_access("holder.property", &format!("member{index}"))
            );
        }
        assert_eq!(
            extractor.member_accesses.len(),
            MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES,
            "direct member access emission should stop at its module cap"
        );

        assert!(
            extractor.record_walk_order_member_access("holder.property", "member128"),
            "the direct receiver should remain resolved after reaching the cap"
        );
        assert_eq!(
            extractor.member_accesses.len(),
            MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES,
            "an over-cap access must not grow the emitted vector"
        );
        assert_eq!(
            extractor.direct_object_binding_whole_object_uses.len(),
            128,
            "cap exhaustion should conservatively abstain every possible class"
        );
    }

    #[test]
    fn repeated_multi_target_member_access_uses_generation_cache() {
        let mut extractor = ModuleInfoExtractor::new();
        let classes = (0..2048).map(|index| format!("Service{index}")).collect();
        extractor
            .module_direct_object_binding_targets
            .insert("holder.property".to_string(), classes);

        for _ in 0..2048 {
            assert!(extractor.record_walk_order_member_access("holder.property", "run"));
        }

        assert_eq!(
            extractor.member_accesses.len(),
            2048,
            "an unchanged repeated access should emit each class association once"
        );
        assert_eq!(
            extractor
                .direct_object_binding_access_generations
                .get("holder.property")
                .and_then(|members| members.get("run")),
            Some(&0),
            "the processed generation should be cached for constant-time repeats"
        );
    }

    #[test]
    fn repeated_dynamic_member_access_uses_generation_cache() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_paths_by_root.insert(
            "holder".to_string(),
            FxHashSet::from_iter(["holder.primary".to_string(), "holder.secondary".to_string()]),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.primary".to_string(),
            FxHashSet::from_iter(["Primary".to_string()]),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.secondary".to_string(),
            FxHashSet::from_iter(["Secondary".to_string()]),
        );

        for _ in 0..2048 {
            assert!(extractor.record_dynamic_direct_object_binding_member_access("holder", "run"));
        }

        assert_eq!(extractor.member_accesses.len(), 2);
        assert_eq!(
            extractor
                .dynamic_direct_object_binding_access_generations
                .get(&("holder".to_string(), "run".to_string())),
            Some(&0),
            "the effective dynamic receiver generation should be cached"
        );
    }

    #[test]
    fn dynamic_member_cap_abstains_receiver_once() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_paths_by_root.insert(
            "holder".to_string(),
            FxHashSet::from_iter(["holder.primary".to_string(), "holder.secondary".to_string()]),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.primary".to_string(),
            (0..MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES)
                .map(|index| format!("Primary{index}"))
                .collect(),
        );
        extractor.module_direct_object_binding_targets.insert(
            "holder.secondary".to_string(),
            FxHashSet::from_iter(["Secondary".to_string()]),
        );

        assert!(extractor.record_dynamic_direct_object_binding_member_access("holder", "run"));
        assert_eq!(
            extractor
                .dynamic_direct_object_binding_abstention_generations
                .get("holder"),
            Some(&0),
            "cap exhaustion should cache whole-receiver abstention"
        );
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("Secondary"),
            "every matching path must receive conservative whole-object credit"
        );

        let emitted = extractor.member_accesses.len();
        assert!(extractor.record_dynamic_direct_object_binding_member_access("holder", "other"));
        assert_eq!(
            extractor.member_accesses.len(),
            emitted,
            "an unchanged abstained receiver should not be enumerated again"
        );
    }

    #[test]
    fn abstained_path_does_not_suppress_shadowed_raw_access() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_targets.insert(
            "holder.property".to_string(),
            FxHashSet::from_iter(["Alpha".to_string()]),
        );
        extractor
            .abstained_direct_object_binding_paths
            .insert("holder.property".to_string());
        extractor.push_direct_object_binding_scope(FxHashSet::from_iter(["holder".to_string()]));

        assert!(
            !extractor.record_walk_order_member_access("holder.property", "used"),
            "a nearer shadow without a direct target must preserve the legacy raw access"
        );
        extractor.pop_direct_object_binding_scope();
    }

    #[test]
    fn scoped_abstention_credits_same_path_targets_in_outer_scopes() {
        let mut extractor = ModuleInfoExtractor::new();
        extractor.module_direct_object_binding_targets.insert(
            "holder.property".to_string(),
            FxHashSet::from_iter(["Alpha".to_string()]),
        );
        extractor.push_direct_object_binding_scope(FxHashSet::from_iter(["holder".to_string()]));
        extractor.scoped_direct_object_binding_targets[0].insert(
            "holder.property".to_string(),
            FxHashSet::from_iter(["Beta".to_string()]),
        );
        for index in 0..MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES {
            extractor
                .direct_object_binding_member_accesses
                .insert((format!("Used{index}"), "member".to_string()));
        }

        assert!(extractor.record_walk_order_member_access("holder.property", "used"));
        assert!(
            extractor
                .direct_object_binding_whole_object_uses
                .contains("Alpha")
                && extractor
                    .direct_object_binding_whole_object_uses
                    .contains("Beta"),
            "scope-global abstention must credit both visible and shadowed targets"
        );
        extractor.pop_direct_object_binding_scope();
        assert!(extractor.record_walk_order_member_access("holder.property", "used"));
    }

    #[test]
    fn mixed_object_binding_targets_share_the_breadth_cap() {
        use std::fmt::Write as _;

        let over_cap = MAX_OBJECT_BINDING_TARGETS + 1000;
        let mut props = String::new();
        for k in 0..over_cap {
            if k % 2 == 0 {
                let _ = write!(props, "k{k}: new Service(), ");
            } else {
                let _ = write!(props, "k{k}: v{k}, ");
            }
        }
        let source = format!("const big = {{ {props} }};");

        let allocator = Allocator::default();
        let parser_return = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let mut extractor = ModuleInfoExtractor::new();
        extractor.visit_program(&parser_return.program);

        assert_eq!(
            extractor.object_binding_target_count(),
            MAX_OBJECT_BINDING_TARGETS,
            "identifier candidates and direct targets must share one cap"
        );
    }
}
