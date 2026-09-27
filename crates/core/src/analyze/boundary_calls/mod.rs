use rustc_hash::FxHashMap;

use fallow_config::ResolvedConfig;
use fallow_types::extract::{ImportedName, ModuleInfo};
use fallow_types::results::BoundaryCallViolation;

use crate::discover::FileId;
use crate::graph::{ModuleGraph, ModuleNode};
use crate::suppress::{IssueKind, SuppressionContext};

use super::security::CalleePattern;
use super::{LineOffsetsMap, byte_offset_to_line_col};

/// Detect calls from zoned files to callees forbidden for that zone via
/// `boundaries.calls.forbidden`.
///
/// Each callee use is matched against the zone's patterns twice: on the
/// written path (`cp.exec`, `console.log`) and on an import-resolved canonical
/// path (`child_process.exec` for `import { exec } from "node:child_process"`
/// or a namespace/default import of the package). Canonicalization only uses
/// real import provenance from bare specifiers; relative sources are skipped
/// because zone-to-zone calls are the import rules' job. Unzoned files are
/// unrestricted, consistent with the import rules; `coverage.requireAllFiles`
/// is the sanctioned way to force zoning first.
pub fn find_boundary_call_violations(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    config: &ResolvedConfig,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<BoundaryCallViolation> {
    let forbidden = &config.boundaries.calls_forbidden_by_zone;
    if forbidden.is_empty() {
        return Vec::new();
    }

    let patterns_by_zone = boundary_call_patterns_by_zone(config);
    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();
    let mut zone_file_counts = boundary_call_zone_file_counts(&patterns_by_zone);

    let mut violations = Vec::new();
    for node in &graph.modules {
        let Some((zone, patterns, module)) = boundary_call_scan_target(
            node,
            config,
            &patterns_by_zone,
            &modules_by_id,
            &mut zone_file_counts,
        ) else {
            continue;
        };
        violations.extend(boundary_call_violations_for_module(
            node,
            module,
            zone,
            patterns,
            suppressions,
            line_offsets_by_file,
        ));
    }

    warn_empty_boundary_call_zones(&zone_file_counts);
    violations
}

fn boundary_call_patterns_by_zone(config: &ResolvedConfig) -> FxHashMap<&str, Vec<CalleePattern>> {
    // Parse each zone's pattern strings once. Inert patterns are rejected at
    // config load, so a parse failure here only drops that single pattern.
    config
        .boundaries
        .calls_forbidden_by_zone
        .iter()
        .map(|(zone, patterns)| {
            (
                zone.as_str(),
                patterns
                    .iter()
                    .filter_map(|raw| CalleePattern::parse(raw))
                    .collect(),
            )
        })
        .collect()
}

fn boundary_call_zone_file_counts<'a>(
    patterns_by_zone: &FxHashMap<&'a str, Vec<CalleePattern>>,
) -> FxHashMap<&'a str, usize> {
    // Track how many analyzed files classified into each referenced zone so a
    // rule pointing at a zone that matches nothing warns instead of silently
    // reporting zero findings forever.
    patterns_by_zone.keys().map(|zone| (*zone, 0)).collect()
}

fn boundary_call_scan_target<'a>(
    node: &ModuleNode,
    config: &'a ResolvedConfig,
    patterns_by_zone: &'a FxHashMap<&str, Vec<CalleePattern>>,
    modules_by_id: &'a FxHashMap<FileId, &ModuleInfo>,
    zone_file_counts: &mut FxHashMap<&'a str, usize>,
) -> Option<(&'a str, &'a [CalleePattern], &'a ModuleInfo)> {
    let relative = node.path.strip_prefix(&config.root).ok()?;
    let relative = relative.to_string_lossy().replace('\\', "/");
    let zone = config.boundaries.classify_zone(&relative)?;
    if let Some(count) = zone_file_counts.get_mut(zone) {
        *count += 1;
    }
    let patterns = patterns_by_zone.get(zone)?;
    let module = modules_by_id.get(&node.file_id).copied()?;
    Some((zone, patterns, module))
}

fn boundary_call_violations_for_module(
    node: &ModuleNode,
    module: &ModuleInfo,
    zone: &str,
    patterns: &[CalleePattern],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Vec<BoundaryCallViolation> {
    if suppressions.is_file_suppressed(node.file_id, IssueKind::BoundaryViolation) {
        return Vec::new();
    }

    let mut violations = Vec::new();
    for callee_use in &module.callee_uses {
        let Some(pattern) = first_matching_pattern(patterns, &callee_use.callee_path, module)
        else {
            continue;
        };
        let (line, col) =
            byte_offset_to_line_col(line_offsets_by_file, node.file_id, callee_use.span_start);
        if suppressions.is_suppressed(node.file_id, line, IssueKind::BoundaryViolation) {
            continue;
        }
        violations.push(BoundaryCallViolation {
            path: node.path.clone(),
            line,
            col,
            zone: zone.to_owned(),
            callee: callee_use.callee_path.clone(),
            pattern: pattern.raw().to_owned(),
        });
    }
    violations
}

fn warn_empty_boundary_call_zones(zone_file_counts: &FxHashMap<&str, usize>) {
    for (zone, count) in zone_file_counts {
        if *count == 0 {
            tracing::warn!(
                "boundaries.calls.forbidden references zone '{zone}', but no analyzed file \
                 classified into that zone; forbidden-call rules only apply to files matched \
                 by a zone's patterns"
            );
        }
    }
}

/// First pattern (in config order) matching the written callee path or its
/// import-resolved canonical form.
fn first_matching_pattern<'p>(
    patterns: &'p [CalleePattern],
    callee_path: &str,
    module: &ModuleInfo,
) -> Option<&'p CalleePattern> {
    if let Some(pattern) = patterns.iter().find(|p| p.matches(callee_path)) {
        return Some(pattern);
    }
    let canonical = canonical_callee_path(module, callee_path)?;
    patterns.iter().find(|p| p.matches(&canonical))
}

/// Resolve a written callee path through the module's import table to a
/// package-canonical form, so one pattern covers every import style:
///
/// - named import (`import { exec as run } from "node:child_process"`,
///   `run(...)`) canonicalizes to `child_process.exec`;
/// - namespace or default import (`import * as cp from "child_process"`,
///   `cp.exec(...)`) canonicalizes to `child_process.exec`.
///
/// The `node:` prefix is stripped so one pattern covers both specifier forms.
/// Relative, root-relative, and `#`-alias sources return `None` (project
/// internals are governed by the import rules), as do type-only imports
/// (their binding cannot be a runtime callee) and unmatched leading segments
/// (globals like `console` match on the written path instead).
pub(super) fn canonical_callee_path(module: &ModuleInfo, callee_path: &str) -> Option<String> {
    let (head, rest) = match callee_path.split_once('.') {
        Some((head, rest)) => (head, Some(rest)),
        None => (callee_path, None),
    };

    let import = module
        .imports
        .iter()
        .find(|import| import.local_name == head)?;
    if import.is_type_only {
        return None;
    }
    let source = import.source.as_str();
    if source.starts_with('.') || source.starts_with('/') || source.starts_with('#') {
        return None;
    }
    let source = source.strip_prefix("node:").unwrap_or(source);

    match &import.imported_name {
        ImportedName::Named(imported) => Some(match rest {
            Some(rest) => format!("{source}.{imported}.{rest}"),
            None => format!("{source}.{imported}"),
        }),
        ImportedName::Namespace | ImportedName::Default => Some(match rest {
            Some(rest) => format!("{source}.{rest}"),
            None => source.to_owned(),
        }),
        ImportedName::SideEffect => None,
    }
}

#[cfg(test)]
mod tests;
