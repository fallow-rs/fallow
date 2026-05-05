//! Helper functions for plugin registry orchestration.
//!
//! Contains pattern aggregation, external plugin processing, config file discovery,
//! config result merging, and plugin detection logic.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use fallow_config::{ExternalPluginDef, PluginDetection, UsedClassMemberRule};

use super::super::{PathRule, Plugin, PluginResult, PluginUsedExportRule, UsedExportRule};
use super::AggregatedPluginResult;

/// Collect static patterns from a single plugin into the aggregated result.
pub fn process_static_patterns(
    plugin: &dyn Plugin,
    root: &Path,
    result: &mut AggregatedPluginResult,
) {
    result.active_plugins.push(plugin.name().to_string());

    let pname = plugin.name().to_string();
    result
        .entry_point_roles
        .insert(pname.clone(), plugin.entry_point_role());
    for rule in plugin.entry_pattern_rules() {
        result.entry_patterns.push((rule, pname.clone()));
    }
    for pat in plugin.config_patterns() {
        result.config_patterns.push((*pat).to_string());
    }
    for pat in plugin.always_used() {
        result.always_used.push(((*pat).to_string(), pname.clone()));
    }
    for rule in plugin.used_export_rules() {
        result
            .used_exports
            .push(PluginUsedExportRule::new(pname.clone(), rule));
    }
    for member in plugin.used_class_members() {
        result
            .used_class_members
            .push(UsedClassMemberRule::from(*member));
    }
    for rule in plugin.used_class_member_rules() {
        result.used_class_members.push(rule);
    }
    for dep in plugin.tooling_dependencies() {
        result.tooling_dependencies.push((*dep).to_string());
    }
    for prefix in plugin.virtual_module_prefixes() {
        result.virtual_module_prefixes.push((*prefix).to_string());
    }
    for suffix in plugin.virtual_package_suffixes() {
        result.virtual_package_suffixes.push((*suffix).to_string());
    }
    for pattern in plugin.generated_import_patterns() {
        result
            .generated_import_patterns
            .push((*pattern).to_string());
    }
    for (prefix, replacement) in plugin.path_aliases(root) {
        result.path_aliases.push((prefix.to_string(), replacement));
    }
    for pat in plugin.fixture_glob_patterns() {
        result
            .fixture_patterns
            .push(((*pat).to_string(), pname.clone()));
    }
}

/// Process external plugin definitions, checking activation and aggregating patterns.
pub fn process_external_plugins(
    external_plugins: &[ExternalPluginDef],
    all_deps: &[String],
    root: &Path,
    discovered_files: &[PathBuf],
    result: &mut AggregatedPluginResult,
) {
    let all_dep_refs: Vec<&str> = all_deps.iter().map(String::as_str).collect();
    for ext in external_plugins {
        let is_active = if let Some(detection) = &ext.detection {
            check_plugin_detection(detection, &all_dep_refs, root, discovered_files)
        } else if !ext.enablers.is_empty() {
            ext.enablers.iter().any(|enabler| {
                if enabler.ends_with('/') {
                    all_deps.iter().any(|d| d.starts_with(enabler))
                } else {
                    all_deps.iter().any(|d| d == enabler)
                }
            })
        } else {
            false
        };
        if is_active {
            result.active_plugins.push(ext.name.clone());
            result
                .entry_point_roles
                .insert(ext.name.clone(), ext.entry_point_role);
            result.entry_patterns.extend(
                ext.entry_points
                    .iter()
                    .map(|p| (PathRule::new(p.clone()), ext.name.clone())),
            );
            // Track config patterns for introspection (not used for AST parsing —
            // external plugins cannot do resolve_config())
            result.config_patterns.extend(ext.config_patterns.clone());
            result.always_used.extend(
                ext.config_patterns
                    .iter()
                    .chain(ext.always_used.iter())
                    .map(|p| (p.clone(), ext.name.clone())),
            );
            result
                .tooling_dependencies
                .extend(ext.tooling_dependencies.clone());
            for ue in &ext.used_exports {
                result.used_exports.push(PluginUsedExportRule::new(
                    ext.name.clone(),
                    UsedExportRule::new(ue.pattern.clone(), ue.exports.clone()),
                ));
            }
            result
                .used_class_members
                .extend(ext.used_class_members.iter().cloned());
        }
    }
}

/// Discover config files on disk for plugins that were not matched against the
/// discovered source set.
///
/// This intentionally probes only known search roots instead of recursively
/// globbing the whole repository tree. Large monorepos often contain enormous
/// `node_modules` directories, and a full `**/project.json` walk becomes
/// pathological there. Callers should therefore pass a focused root list such
/// as the repo root, workspace roots, and ancestors of discovered source files.
pub fn discover_config_files<'a>(
    config_matchers: &[(&'a dyn Plugin, Vec<globset::GlobMatcher>)],
    resolved_plugins: &FxHashSet<&str>,
    roots: &[&Path],
) -> Vec<(PathBuf, &'a dyn Plugin)> {
    let mut config_files: Vec<(PathBuf, &'a dyn Plugin)> = Vec::new();
    let mut seen: FxHashSet<(PathBuf, &'a str)> = FxHashSet::default();

    for (plugin, _) in config_matchers {
        if resolved_plugins.contains(plugin.name()) {
            continue;
        }

        for root in roots {
            for pat in plugin.config_patterns() {
                for expanded in expand_brace_pattern(pat) {
                    for path in discover_pattern_matches(root, &expanded) {
                        if seen.insert((path.clone(), plugin.name())) {
                            config_files.push((path, *plugin));
                        }
                    }
                }
            }
        }
    }

    config_files
}

fn pattern_has_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

fn discover_pattern_matches(root: &Path, pattern: &str) -> Vec<PathBuf> {
    if !pattern_has_glob(pattern) {
        let path = root.join(pattern);
        return if path.is_file() {
            vec![path]
        } else {
            Vec::new()
        };
    }

    if let Some(stripped) = pattern.strip_prefix("**/") {
        return discover_pattern_matches(root, stripped);
    }

    let (dir, file_pattern) = match pattern.rsplit_once('/') {
        Some((parent, file_pattern)) if !pattern_has_glob(parent) => {
            (root.join(parent), file_pattern)
        }
        Some(_) => return Vec::new(),
        None => (root.to_path_buf(), pattern),
    };

    scan_dir_for_pattern(&dir, file_pattern)
}

fn scan_dir_for_pattern(dir: &Path, file_pattern: &str) -> Vec<PathBuf> {
    let Ok(matcher) = globset::Glob::new(file_pattern).map(|g| g.compile_matcher()) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| matcher.is_match(std::path::Path::new(name)))
        })
        .collect()
}

fn expand_brace_pattern(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    let Some(close_rel) = pattern[open + 1..].find('}') else {
        return vec![pattern.to_string()];
    };
    let close = open + 1 + close_rel;

    let prefix = &pattern[..open];
    let suffix = &pattern[close + 1..];
    let inner = &pattern[open + 1..close];
    let mut expanded = Vec::new();
    for option in inner.split(',') {
        for tail in expand_brace_pattern(suffix) {
            expanded.push(format!("{prefix}{option}{tail}"));
        }
    }
    expanded
}

/// Merge a `PluginResult` from config parsing into the aggregated result.
pub fn process_config_result(
    plugin_name: &str,
    plugin_result: PluginResult,
    result: &mut AggregatedPluginResult,
) {
    let pname = plugin_name.to_string();
    // When the config explicitly defines entry patterns or used-export rules,
    // treat it as a full override of that plugin's static defaults instead of
    // layering both sets together.
    if plugin_result.replace_entry_patterns && !plugin_result.entry_patterns.is_empty() {
        result.entry_patterns.retain(|(_, name)| name != &pname);
    }
    if plugin_result.replace_used_export_rules && !plugin_result.used_exports.is_empty() {
        result.used_exports.retain(|rule| rule.plugin_name != pname);
    }
    result.entry_patterns.extend(
        plugin_result
            .entry_patterns
            .into_iter()
            .map(|rule| (rule, pname.clone())),
    );
    result.used_exports.extend(
        plugin_result
            .used_exports
            .into_iter()
            .map(|rule| PluginUsedExportRule::new(pname.clone(), rule)),
    );
    result
        .used_class_members
        .extend(plugin_result.used_class_members);
    result
        .referenced_dependencies
        .extend(plugin_result.referenced_dependencies);
    result.discovered_always_used.extend(
        plugin_result
            .always_used_files
            .into_iter()
            .map(|p| (p, pname.clone())),
    );
    for (prefix, replacement) in plugin_result.path_aliases {
        result
            .path_aliases
            .retain(|(existing_prefix, _)| existing_prefix != &prefix);
        result.path_aliases.push((prefix, replacement));
    }
    result.setup_files.extend(
        plugin_result
            .setup_files
            .into_iter()
            .map(|p| (p, pname.clone())),
    );
    result.fixture_patterns.extend(
        plugin_result
            .fixture_patterns
            .into_iter()
            .map(|p| (p, pname.clone())),
    );
    result
        .scss_include_paths
        .extend(plugin_result.scss_include_paths);
}

/// Check if a plugin already has a config file matched against discovered files.
pub fn check_has_config_file(
    plugin: &dyn Plugin,
    config_matchers: &[(&dyn Plugin, Vec<globset::GlobMatcher>)],
    relative_files: &[(PathBuf, String)],
) -> bool {
    !plugin.config_patterns().is_empty()
        && config_matchers.iter().any(|(p, matchers)| {
            p.name() == plugin.name()
                && relative_files
                    .iter()
                    .any(|(_, rel)| matchers.iter().any(|m| m.is_match(rel.as_str())))
        })
}

/// Check if a `PluginDetection` condition is satisfied.
pub fn check_plugin_detection(
    detection: &PluginDetection,
    all_deps: &[&str],
    root: &Path,
    discovered_files: &[PathBuf],
) -> bool {
    match detection {
        PluginDetection::Dependency { package } => all_deps.iter().any(|d| *d == package),
        PluginDetection::FileExists { pattern } => {
            // Check against discovered files first (fast path)
            if let Ok(matcher) = globset::Glob::new(pattern).map(|g| g.compile_matcher()) {
                for file in discovered_files {
                    let relative = file.strip_prefix(root).unwrap_or(file);
                    if matcher.is_match(relative) {
                        return true;
                    }
                }
            }
            // Fall back to glob on disk for non-source files (e.g., config files)
            let full_pattern = root.join(pattern).to_string_lossy().to_string();
            glob::glob(&full_pattern)
                .ok()
                .is_some_and(|mut g| g.next().is_some())
        }
        PluginDetection::All { conditions } => conditions
            .iter()
            .all(|c| check_plugin_detection(c, all_deps, root, discovered_files)),
        PluginDetection::Any { conditions } => conditions
            .iter()
            .any(|c| check_plugin_detection(c, all_deps, root, discovered_files)),
    }
}
