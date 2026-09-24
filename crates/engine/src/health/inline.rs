//! Complexity signals for editors, from the same threshold rule as
//! `fallow health`.

use std::path::PathBuf;

use fallow_config::{ResolvedConfig, Severity};
use fallow_types::discover::DiscoveredFile;

use super::threshold_overrides::{GlobalHealthThresholds, ThresholdOverrideResolver};
use crate::source::ModuleInfo;

/// One function above a complexity threshold, for an editor code lens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineComplexity {
    /// Absolute path of the file that declares the function.
    pub path: PathBuf,
    /// Function name as extracted from the source.
    pub name: String,
    /// One-based line of the function declaration.
    pub line: u32,
    /// Zero-based column of the function declaration.
    pub col: u32,
    /// Measured cyclomatic complexity.
    pub cyclomatic: u16,
    /// Measured cognitive complexity.
    pub cognitive: u16,
    /// The function is above the effective cyclomatic threshold.
    pub exceeds_cyclomatic: bool,
    /// The function is above the effective cognitive threshold.
    pub exceeds_cognitive: bool,
}

/// The functions of the parsed modules that are above a complexity threshold.
///
/// This applies the rules of the `fallow health` findings: `health.ignore`,
/// the `complexity` suppression comments, the module-scope unit that never
/// becomes a finding, and the effective thresholds of `health.thresholdOverrides`
/// per file and function. It also applies the `complexity-cyclomatic` and
/// `complexity-cognitive` rules with `overrides[].rules` for the file: a
/// function whose contributing kinds are all `off` is dropped, as the health
/// report drops the finding. An editor code lens and the health report
/// therefore flag the same functions.
#[must_use]
pub fn inline_complexity(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
    files: &[DiscoveredFile],
) -> Vec<InlineComplexity> {
    let file_paths: rustc_hash::FxHashMap<_, _> =
        files.iter().map(|file| (file.id, &file.path)).collect();
    let ignore_set = super::ignore::build_ignore_set(&config.health.ignore);
    let resolver = ThresholdOverrideResolver::new(
        &config.health.threshold_overrides,
        GlobalHealthThresholds {
            cyclomatic: config.health.max_cyclomatic,
            cognitive: config.health.max_cognitive,
            crap: config.health.max_crap,
            unit_size: config.health.max_unit_size,
        },
    );
    let mut findings = Vec::new();

    for module in modules {
        let Some(path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(&config.root).unwrap_or(path);
        if ignore_set.is_match(relative) {
            continue;
        }
        // The rules of this file, resolved one time for all of its functions.
        let path_rules =
            (!config.overrides.is_empty()).then(|| config.resolve_rules_for_path(path));
        let rules = path_rules.as_ref().unwrap_or(&config.rules);
        for function in &module.complexity {
            if fallow_types::extract::is_synthetic_module_unit(&function.name)
                || crate::suppress::is_suppressed(
                    &module.suppressions,
                    function.line,
                    crate::suppress::IssueKind::Complexity,
                )
            {
                continue;
            }
            let (applied, _) = resolver.resolve(relative, &function.name);
            let Some((exceeds_cyclomatic, exceeds_cognitive)) =
                super::findings::complexity_exceeded(function, applied.effective)
            else {
                continue;
            };
            // A lens has no CRAP score, so only the two kinds it shows count.
            if rules.complexity_severity(exceeds_cyclomatic, exceeds_cognitive, false)
                == Severity::Off
            {
                continue;
            }
            findings.push(InlineComplexity {
                path: (*path).clone(),
                name: function.name.clone(),
                line: function.line,
                col: function.col,
                cyclomatic: function.cyclomatic,
                cognitive: function.cognitive,
                exceeds_cyclomatic,
                exceeds_cognitive,
            });
        }
    }

    findings
}
