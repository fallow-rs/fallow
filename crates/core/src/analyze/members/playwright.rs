use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum PlaywrightTestKey {
    Export(ExportKey),
    Local { file_id: FileId, local_name: String },
}

pub(super) fn push_playwright_test_key(keys: &mut Vec<PlaywrightTestKey>, key: PlaywrightTestKey) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

pub(super) fn collect_playwright_local_test_names(resolved: &ResolvedModule) -> FxHashSet<String> {
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

pub(super) fn playwright_test_keys_for_local(
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

pub(super) fn build_playwright_fixture_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
) -> FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> {
    let type_targets = build_playwright_fixture_type_targets(graph, resolved_modules, indexes);
    let mut targets_by_test: FxHashMap<PlaywrightTestKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();
    let mut aliases_by_test: FxHashMap<PlaywrightTestKey, Vec<PlaywrightTestKey>> =
        FxHashMap::default();

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        let local_playwright_test_names = collect_playwright_local_test_names(resolved);
        collect_playwright_fixture_def_targets(
            graph,
            resolved,
            local_to_export_keys,
            &local_playwright_test_names,
            &type_targets,
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

/// Collect fixture-definition facts for one module, recording each fixture's
/// POM type export keys under its owning test key.
pub(super) fn collect_playwright_fixture_def_targets(
    graph: &ModuleGraph,
    resolved: &ResolvedModule,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    local_playwright_test_names: &FxHashSet<String>,
    type_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    targets_by_test: &mut FxHashMap<PlaywrightTestKey, FxHashMap<String, Vec<ExportKey>>>,
) {
    let definition_facts = playwright_fixture_definitions(resolved);
    for access in definition_facts {
        let test_keys = playwright_test_keys_for_local(
            local_to_export_keys,
            local_playwright_test_names,
            resolved.file_id,
            access.test_name.as_str(),
        );
        let Some(target_keys) = local_to_export_keys.get(access.type_name.as_str()) else {
            continue;
        };

        for test_key in test_keys {
            let fixture_targets = targets_by_test.entry(test_key).or_default();
            for target_key in target_keys {
                push_playwright_fixture_target(
                    graph,
                    type_targets,
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
pub(super) fn collect_playwright_fixture_aliases(
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

pub(super) fn expand_playwright_fixture_aliases(
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

pub(super) fn push_playwright_fixture_target(
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

pub(super) fn push_playwright_fixture_type_target(
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

pub(super) fn build_playwright_fixture_type_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
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
            let Some(target_keys) = local_to_export_keys.get(access.type_name.as_str()) else {
                continue;
            };

            for alias_key in alias_keys {
                let alias_targets = targets_by_alias.entry(alias_key.clone()).or_default();
                let fixture_targets = alias_targets
                    .entry(access.fixture_name.clone())
                    .or_default();
                for target_key in target_keys {
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
    indexes: &MemberPassIndexes<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let targets_by_test = build_playwright_fixture_targets(graph, resolved_modules, indexes);
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
