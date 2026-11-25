//! Diagnostics of the silent-fail plugin checks: config pattern collisions and enabler typos.

use rustc_hash::FxHashSet;

use fallow_config::ExternalPluginDef;

use super::super::Plugin;
use super::should_warn;

/// Structured diagnostic surfaced by the silent-fail plugin checks (#479).
///
/// Returned by [`detect_pattern_collisions`] and [`detect_enabler_typos`] so
/// unit tests can assert on the findings without standing up a tracing
/// subscriber. The runtime path calls [`emit_plugin_diagnostics`] to convert
/// each variant into one `tracing::warn!` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PluginDiagnostic {
    /// Two or more plugins declared an identical `config_patterns` entry.
    PatternCollision {
        pattern: String,
        owners: Vec<String>,
    },
    /// An external plugin enabler does not match any project dependency, but
    /// at least one Levenshtein-close dep name exists.
    EnablerTypo {
        plugin: String,
        enabler: String,
        suggestion: String,
    },
}

/// Detect plugins whose `config_patterns` collide byte-for-byte.
///
/// Detection is byte-equal on the pattern string. Overlapping but non-identical
/// globs (e.g. `vite.config.{ts,js}` vs `vite.config.ts`) require pattern
/// intersection logic and are intentionally out of scope. The warning's purpose
/// is to surface USER-AUTHORED collisions between external plugins or between an
/// external plugin and a built-in, so the user can disambiguate by editing one
/// side.
///
/// Built-in-vs-built-in collisions are intentionally NOT reported: they are
/// curated and benign (Phase 3a config matching runs every matching plugin's
/// `resolve_config` independently, so there is no data loss), and the warning's
/// remediation advice ("rename one of the patterns or remove the duplicate
/// plugin") is impossible to follow for a built-in. Such a collision exists by
/// design, e.g. both `vite` and `tanstack-router` claim
/// `vite.config.{ts,js,mts,mjs}` because tanstack-router parses the
/// `tanstackRouter({...})` call inside the vite config to find a custom
/// `generatedRouteTree` path (#808). A finding is therefore emitted only when
/// at least one owner is an external (user-authored) plugin.
///
/// Precedence rule when two plugins claim the same pattern: the one registered
/// first wins. For built-in plugins, registration order is defined in
/// [`super::builtin::create_builtin_plugins`]. External plugins (file-loaded plus
/// inline `framework[]`) run AFTER built-ins, so they cannot displace a
/// built-in's `resolve_config` result for the same file.
pub(super) fn detect_pattern_collisions(
    builtin_active: &[&dyn Plugin],
    external_active: &[&ExternalPluginDef],
) -> Vec<PluginDiagnostic> {
    use rustc_hash::FxHashMap;

    let mut pattern_owners: FxHashMap<String, (Vec<String>, FxHashSet<String>)> =
        FxHashMap::default();

    let record = |pattern_owners: &mut FxHashMap<_, (Vec<String>, FxHashSet<String>)>,
                  pattern: String,
                  name: String| {
        let (list, seen) = pattern_owners.entry(pattern).or_default();
        if seen.insert(name.clone()) {
            list.push(name);
        }
    };

    for plugin in builtin_active {
        for pat in plugin.config_patterns() {
            record(
                &mut pattern_owners,
                (*pat).to_string(),
                plugin.name().to_string(),
            );
        }
    }
    for ext in external_active {
        for pat in &ext.config_patterns {
            record(&mut pattern_owners, pat.clone(), ext.name.clone());
        }
    }

    // Names of built-in plugins. Built-in-only collisions are curated + benign
    // (every matching plugin runs `resolve_config` independently), so they must
    // not surface an un-actionable warning (#808). Keying on the built-in set
    // and emitting only when an owner is NOT built-in is robust even if a
    // user-authored external plugin happens to share a built-in's name: the
    // built-in owner alone never re-enables the warning.
    let builtin_names: FxHashSet<&str> = builtin_active.iter().map(|p| p.name()).collect();

    let mut findings: Vec<PluginDiagnostic> = pattern_owners
        .into_iter()
        .filter_map(|(pattern, (owners, _seen))| {
            if owners.len() < 2 || owners.iter().all(|o| builtin_names.contains(o.as_str())) {
                None
            } else {
                Some(PluginDiagnostic::PatternCollision { pattern, owners })
            }
        })
        .collect();
    findings.sort_unstable_by(|a, b| match (a, b) {
        (
            PluginDiagnostic::PatternCollision { pattern: ap, .. },
            PluginDiagnostic::PatternCollision { pattern: bp, .. },
        ) => ap.cmp(bp),
        _ => std::cmp::Ordering::Equal,
    });
    findings
}

/// Detect external plugins whose enablers do not match any project dependency
/// AND at least one enabler is a plausible typo of a real dep.
///
/// Scope:
/// - Only external plugins (file-loaded plus inline `framework[]`). Built-in
///   plugins' enablers are hard-coded so cannot be misspelled.
/// - Skip plugins with a `detection` block: detection is the rich-logic path
///   and false negatives there are not enabler typos.
/// - Skip plugins with empty `enablers` (no signal to validate against).
/// - Stay silent when no Levenshtein-close dep exists: the plugin may
///   legitimately not apply to this project.
///
/// Matches the established #467 / #510 pattern: tracing-warn with a `did you
/// mean` suggestion at the call site. No exit non-zero, no new CLI flag.
pub(super) fn detect_enabler_typos(
    external_plugins: &[ExternalPluginDef],
    all_deps: &[String],
) -> Vec<PluginDiagnostic> {
    let mut findings = Vec::new();

    for ext in external_plugins {
        if ext.detection.is_some() || ext.enablers.is_empty() {
            continue;
        }

        let any_match = ext.enablers.iter().any(|enabler| {
            if enabler.ends_with('/') {
                all_deps.iter().any(|d| d.starts_with(enabler))
            } else {
                all_deps.iter().any(|d| d == enabler)
            }
        });
        if any_match {
            continue;
        }

        for enabler in &ext.enablers {
            let candidates = all_deps.iter().map(String::as_str);
            let Some(suggestion) = fallow_config::levenshtein::closest_match(enabler, candidates)
            else {
                continue;
            };

            findings.push(PluginDiagnostic::EnablerTypo {
                plugin: ext.name.clone(),
                enabler: enabler.clone(),
                suggestion: suggestion.to_string(),
            });
        }
    }

    findings
}

/// Emit one `tracing::warn!` per finding, dedup'd against the process-wide
/// `plugin_warn_dedupe` set so combined-mode does not triple-warn.
pub(super) fn emit_plugin_diagnostics(findings: &[PluginDiagnostic]) {
    for finding in findings {
        match finding {
            PluginDiagnostic::PatternCollision { pattern, owners } => {
                let key = format!("collision::{pattern}::{owners:?}");
                if !should_warn(key) {
                    continue;
                }
                let winner = &owners[0];
                let others = owners[1..].join(", ");
                tracing::warn!(
                    "plugin config_patterns collision: identical pattern \
                     '{pattern}' is claimed by plugins [{joined}]; '{winner}' \
                     runs first (registration order), others ({others}) \
                     follow. Rename one of the patterns or remove the \
                     duplicate plugin to make resolution explicit. A future \
                     release may reject identical-pattern collisions.",
                    joined = owners.join(", "),
                );
            }
            PluginDiagnostic::EnablerTypo {
                plugin,
                enabler,
                suggestion,
            } => {
                let key = format!("enabler::{plugin}::{enabler}");
                if !should_warn(key) {
                    continue;
                }
                tracing::warn!(
                    "plugin '{plugin}' enabler '{enabler}' does not match any \
                     dependency in package.json; did you mean '{suggestion}'? \
                     The plugin will not activate. A future release may reject \
                     unmatched enablers.",
                );
            }
        }
    }
}
