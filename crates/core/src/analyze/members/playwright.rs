use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum PlaywrightTestKey {
    Export(ExportKey),
    Local { file_id: FileId, local_name: String },
}

fn push_playwright_test_key(keys: &mut Vec<PlaywrightTestKey>, key: PlaywrightTestKey) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

fn collect_playwright_local_test_names(resolved: &ResolvedModule) -> FxHashSet<String> {
    let mut names = FxHashSet::default();
    let definition_facts = playwright_fixture_definitions(resolved);
    for access in &definition_facts {
        names.insert(access.test_name.clone());
    }
    let alias_facts = playwright_fixture_aliases(resolved);
    for access in &alias_facts {
        names.insert(access.test_name.clone());
    }
    names
}

fn playwright_test_keys_for_local(
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    local_playwright_test_names: &FxHashSet<String>,
    file_id: FileId,
    local_name: &str,
) -> Vec<PlaywrightTestKey> {
    if let Some(export_keys) = local_to_export_keys.get(local_name) {
        return export_keys
            .iter()
            .cloned()
            .map(PlaywrightTestKey::Export)
            .collect();
    }
    if local_playwright_test_names.contains(local_name) {
        return vec![PlaywrightTestKey::Local {
            file_id,
            local_name: local_name.to_string(),
        }];
    }
    Vec::new()
}

/// Split an extraction-encoded indexed-access fixture type
/// (`TaskAsserterFactory["taskAsserter"]`, issue #2070) into its object type
/// name and literal index. `None` for every ordinary type name, so callers can
/// use it as a fallback after the direct local-name lookup misses.
fn parse_indexed_access_type_name(type_name: &str) -> Option<(&str, &str)> {
    let (class_name, rest) = type_name.split_once("[\"")?;
    let index = rest.strip_suffix("\"]")?;
    if class_name.is_empty() || index.is_empty() || index.contains('"') {
        return None;
    }
    Some((class_name, index))
}

/// Index every exported class's `(member, declared type)` instance bindings
/// (constructor params, typed properties, getter return types) by export key,
/// so an indexed-access fixture type can hop from `Factory["taskAsserter"]` to
/// the getter's declared return type.
fn build_class_instance_binding_index<'a>(
    modules: &'a [ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<ExportKey, &'a [(String, String)]> {
    let mut bindings_by_class: FxHashMap<ExportKey, &'a [(String, String)]> = FxHashMap::default();
    for module in modules {
        if !indexes.module_by_id.contains_key(&module.file_id) {
            continue;
        }
        for heritage in &module.class_heritage {
            if heritage.instance_bindings.is_empty() {
                continue;
            }
            bindings_by_class.insert(
                ExportKey::new(module.file_id, heritage.export_name.clone()),
                heritage.instance_bindings.as_slice(),
            );
        }
    }
    bindings_by_class
}

/// Resolve an indexed-access fixture type (`Factory["getter"]`, issue #2070) to
/// the class export keys backing the indexed member's declared type. Mirrors
/// the factory chain-of-gates shape; a miss at any link resolves nothing
/// (false-negative-preferring):
///
///   1. the object type name resolves through the consumer's imports/exports
///      (re-export aware) to an exported class carrying instance bindings;
///   2. the literal index matches one of that class's instance bindings (a
///      public getter's return type, a typed property, or a constructor
///      param), yielding the bound type's local name in the DECLARING module;
///   3. the bound type name resolves through the declaring module's own
///      imports/exports to an export that is a class with members
///      (`export_is_class_with_members`), the final over-credit gate.
fn playwright_indexed_access_targets(
    graph: &ModuleGraph,
    indexes: &MemberPassIndexes<'_>,
    instance_bindings_by_class: &FxHashMap<ExportKey, &[(String, String)]>,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    type_name: &str,
) -> Vec<ExportKey> {
    let Some((class_name, index)) = parse_indexed_access_type_name(type_name) else {
        return Vec::new();
    };
    let Some(seed_keys) = local_to_export_keys.get(class_name) else {
        return Vec::new();
    };
    let mut targets = Vec::new();
    for seed_key in seed_keys {
        for origin in export_key_with_origins(graph, seed_key) {
            let Some(bindings) = instance_bindings_by_class.get(&origin) else {
                continue;
            };
            let Some((_, bound_type)) = bindings.iter().find(|(member, _)| member == index) else {
                continue;
            };
            let Some(target_seeds) = indexes.local_keys(origin.file_id).get(bound_type.as_str())
            else {
                continue;
            };
            for target_seed in target_seeds {
                for target in export_key_with_origins(graph, target_seed) {
                    let is_class_target =
                        indexes
                            .module_by_id
                            .get(&target.file_id)
                            .is_some_and(|module| {
                                export_is_class_with_members(module, target.export_name.as_str())
                            });
                    if is_class_target {
                        push_export_key(&mut targets, target);
                    }
                }
            }
        }
    }
    targets
}

fn build_playwright_fixture_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> {
    let instance_bindings_by_class = build_class_instance_binding_index(modules, indexes);
    let type_targets = build_playwright_fixture_type_targets(
        graph,
        resolved_modules,
        indexes,
        &instance_bindings_by_class,
    );
    let mut targets_by_test: FxHashMap<PlaywrightTestKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();
    let mut aliases_by_test: FxHashMap<PlaywrightTestKey, Vec<PlaywrightTestKey>> =
        FxHashMap::default();
    let def_context = PlaywrightFixtureDefContext {
        graph,
        indexes,
        type_targets: &type_targets,
        instance_bindings_by_class: &instance_bindings_by_class,
    };

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        let local_playwright_test_names = collect_playwright_local_test_names(resolved);
        collect_playwright_fixture_def_targets(
            &def_context,
            resolved,
            &local_playwright_test_names,
            &mut targets_by_test,
        );
        collect_playwright_fixture_aliases(
            graph,
            resolved,
            local_to_export_keys,
            &local_playwright_test_names,
            &mut aliases_by_test,
        );
    }

    expand_playwright_fixture_aliases(&mut targets_by_test, &aliases_by_test);
    targets_by_test
        .into_iter()
        .filter_map(|(key, targets)| match key {
            PlaywrightTestKey::Export(export_key) => Some((export_key, targets)),
            PlaywrightTestKey::Local { .. } => None,
        })
        .collect()
}

/// Shared read-only inputs for fixture-definition target resolution.
struct PlaywrightFixtureDefContext<'a, 'b> {
    graph: &'a ModuleGraph,
    indexes: &'a MemberPassIndexes<'b>,
    type_targets: &'a FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    instance_bindings_by_class: &'a FxHashMap<ExportKey, &'a [(String, String)]>,
}

/// Collect fixture-definition facts for one module, recording each fixture's
/// POM type export keys under its owning test key.
fn collect_playwright_fixture_def_targets(
    context: &PlaywrightFixtureDefContext<'_, '_>,
    resolved: &ResolvedModule,
    local_playwright_test_names: &FxHashSet<String>,
    targets_by_test: &mut FxHashMap<PlaywrightTestKey, FxHashMap<String, Vec<ExportKey>>>,
) {
    let local_to_export_keys = context.indexes.local_keys(resolved.file_id);
    let definition_facts = playwright_fixture_definitions(resolved);
    for access in definition_facts {
        let test_keys = playwright_test_keys_for_local(
            local_to_export_keys,
            local_playwright_test_names,
            resolved.file_id,
            access.test_name.as_str(),
        );
        let target_keys = match local_to_export_keys.get(access.type_name.as_str()) {
            Some(keys) => keys.clone(),
            None => playwright_indexed_access_targets(
                context.graph,
                context.indexes,
                context.instance_bindings_by_class,
                local_to_export_keys,
                access.type_name.as_str(),
            ),
        };
        if target_keys.is_empty() {
            continue;
        }

        for test_key in test_keys {
            let fixture_targets = targets_by_test.entry(test_key).or_default();
            for target_key in &target_keys {
                push_playwright_fixture_target(
                    context.graph,
                    context.type_targets,
                    fixture_targets,
                    access.fixture_name.as_str(),
                    target_key,
                );
            }
        }
    }
}

/// Collect wrapper-alias facts for one module, recording each alias's base test
/// keys (origins expanded) under its owning test key.
fn collect_playwright_fixture_aliases(
    graph: &ModuleGraph,
    resolved: &ResolvedModule,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    local_playwright_test_names: &FxHashSet<String>,
    aliases_by_test: &mut FxHashMap<PlaywrightTestKey, Vec<PlaywrightTestKey>>,
) {
    let alias_facts = playwright_fixture_aliases(resolved);
    for access in alias_facts {
        let test_keys = playwright_test_keys_for_local(
            local_to_export_keys,
            local_playwright_test_names,
            resolved.file_id,
            access.test_name.as_str(),
        );
        let base_keys = playwright_test_keys_for_local(
            local_to_export_keys,
            local_playwright_test_names,
            resolved.file_id,
            access.base_name.as_str(),
        );

        for test_key in test_keys {
            let aliases = aliases_by_test.entry(test_key).or_default();
            for base_key in &base_keys {
                match base_key {
                    PlaywrightTestKey::Export(export_key) => {
                        for key in export_key_with_origins(graph, export_key) {
                            push_playwright_test_key(aliases, PlaywrightTestKey::Export(key));
                        }
                    }
                    PlaywrightTestKey::Local { .. } => {
                        push_playwright_test_key(aliases, base_key.clone());
                    }
                }
            }
        }
    }
}

fn expand_playwright_fixture_aliases(
    targets_by_test: &mut FxHashMap<PlaywrightTestKey, FxHashMap<String, Vec<ExportKey>>>,
    aliases_by_test: &FxHashMap<PlaywrightTestKey, Vec<PlaywrightTestKey>>,
) {
    if aliases_by_test.is_empty() {
        return;
    }

    let max_iters = aliases_by_test.len() + 1;
    for _ in 0..max_iters {
        let snapshot = targets_by_test.clone();
        let mut changed = false;
        for (alias_key, base_keys) in aliases_by_test {
            for base_key in base_keys {
                let Some(base_targets) = snapshot.get(base_key) else {
                    continue;
                };
                let alias_targets = targets_by_test.entry(alias_key.clone()).or_default();
                for (fixture_name, target_keys) in base_targets {
                    let fixture_targets = alias_targets.entry(fixture_name.clone()).or_default();
                    for target_key in target_keys {
                        let before = fixture_targets.len();
                        push_export_key(fixture_targets, target_key.clone());
                        changed |= fixture_targets.len() != before;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn push_playwright_fixture_target(
    graph: &ModuleGraph,
    type_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    fixture_targets: &mut FxHashMap<String, Vec<ExportKey>>,
    fixture_name: &str,
    target_key: &ExportKey,
) {
    let origin_keys = export_key_with_origins(graph, target_key);
    for key in &origin_keys {
        push_export_key(
            fixture_targets.entry(fixture_name.to_string()).or_default(),
            key.clone(),
        );
    }
    for alias_key in origin_keys {
        push_playwright_fixture_type_target(
            type_targets,
            fixture_targets,
            fixture_name,
            &alias_key,
        );
    }
}

fn push_playwright_fixture_type_target(
    type_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    fixture_targets: &mut FxHashMap<String, Vec<ExportKey>>,
    fixture_name: &str,
    alias_key: &ExportKey,
) {
    let Some(alias_targets) = type_targets.get(alias_key) else {
        return;
    };
    for (suffix, nested_targets) in alias_targets {
        let nested_fixture_name = format!("{fixture_name}.{suffix}");
        let fixture_targets = fixture_targets.entry(nested_fixture_name).or_default();
        for nested_target in nested_targets {
            push_export_key(fixture_targets, nested_target.clone());
        }
    }
}

fn build_playwright_fixture_type_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    instance_bindings_by_class: &FxHashMap<ExportKey, &[(String, String)]>,
) -> FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> {
    let mut targets_by_alias: FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        let type_facts = playwright_fixture_types(resolved);
        for access in type_facts {
            let Some(alias_keys) = local_to_export_keys.get(access.alias_name.as_str()) else {
                continue;
            };
            let target_keys = match local_to_export_keys.get(access.type_name.as_str()) {
                Some(keys) => keys.clone(),
                None => playwright_indexed_access_targets(
                    graph,
                    indexes,
                    instance_bindings_by_class,
                    local_to_export_keys,
                    access.type_name.as_str(),
                ),
            };
            if target_keys.is_empty() {
                continue;
            }

            for alias_key in alias_keys {
                let alias_targets = targets_by_alias.entry(alias_key.clone()).or_default();
                let fixture_targets = alias_targets
                    .entry(access.fixture_name.clone())
                    .or_default();
                for target_key in &target_keys {
                    for key in export_key_with_origins(graph, target_key) {
                        push_export_key(fixture_targets, key);
                    }
                }
            }
        }
    }

    targets_by_alias
}

pub(super) fn propagate_playwright_fixture_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let targets_by_test =
        build_playwright_fixture_targets(graph, resolved_modules, modules, indexes);
    if targets_by_test.is_empty() {
        return;
    }

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        let use_facts = playwright_fixture_uses(resolved);
        for access in use_facts {
            let Some(test_keys) = local_to_export_keys.get(access.test_name.as_str()) else {
                continue;
            };

            for test_key in test_keys {
                let Some(fixture_targets) = targets_by_test.get(test_key) else {
                    continue;
                };
                let Some(target_keys) = fixture_targets.get(access.fixture_name.as_str()) else {
                    continue;
                };
                for target_key in target_keys {
                    accessed_members
                        .entry(target_key.clone())
                        .or_default()
                        .insert(access.member.clone());
                }
            }
        }
    }
}
