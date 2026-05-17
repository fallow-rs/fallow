//! Feature flag collection and cross-reference with dead code findings.
//!
//! Collects per-file flag uses from parsed modules and builds
//! project-level `FeatureFlag` results. Optionally correlates with
//! dead code findings to identify flags guarding unused code.

use std::path::PathBuf;

use fallow_types::extract::{FlagUse, FlagUseKind, ModuleInfo, byte_offset_to_line_col};
use fallow_types::results::{AnalysisResults, FeatureFlag, FlagConfidence, FlagKind};

use crate::graph::ModuleGraph;

/// Collect feature flag uses from all parsed modules into `FeatureFlag` results.
///
/// Maps extraction-level `FlagUse` (per-file, no path) to result-level
/// `FeatureFlag` (with full path, confidence). Resolves guard span byte
/// offsets to line numbers using per-file line offset tables.
pub fn collect_feature_flags(modules: &[ModuleInfo], graph: &ModuleGraph) -> Vec<FeatureFlag> {
    let mut flags = Vec::new();

    for module in modules {
        if module.flag_uses.is_empty() {
            continue;
        }

        let idx = module.file_id.0 as usize;
        let Some(node) = graph.modules.get(idx) else {
            continue;
        };

        for flag_use in &module.flag_uses {
            let mut flag = flag_use_to_feature_flag(flag_use, node.path.clone());

            // Resolve guard span byte offsets to line numbers
            if let (Some(start), Some(end)) = (flag_use.guard_span_start, flag_use.guard_span_end)
                && !module.line_offsets.is_empty()
            {
                let (start_line, _) = byte_offset_to_line_col(&module.line_offsets, start);
                let (end_line, _) = byte_offset_to_line_col(&module.line_offsets, end);
                flag.guard_line_start = Some(start_line);
                flag.guard_line_end = Some(end_line);
            }

            flags.push(flag);
        }
    }

    flags
}

/// Correlate feature flags with dead code findings.
///
/// For each flag that guards a code span, check if any dead code findings
/// (unused exports) fall within that span. Populates `guarded_dead_exports`
/// on each flag.
pub fn correlate_with_dead_code(flags: &mut [FeatureFlag], results: &AnalysisResults) {
    if results.unused_exports.is_empty() && results.unused_types.is_empty() {
        return;
    }

    for flag in flags.iter_mut() {
        let (Some(guard_start), Some(guard_end)) = (flag.guard_line_start, flag.guard_line_end)
        else {
            continue;
        };

        // Find unused exports in the same file within the guard span
        for export in &results.unused_exports {
            if export.export.path == flag.path
                && export.export.line >= guard_start
                && export.export.line <= guard_end
            {
                flag.guarded_dead_exports
                    .push(export.export.export_name.clone());
            }
        }

        // Also check unused type exports
        for export in &results.unused_types {
            if export.export.path == flag.path
                && export.export.line >= guard_start
                && export.export.line <= guard_end
            {
                flag.guarded_dead_exports
                    .push(export.export.export_name.clone());
            }
        }
    }
}

/// Convert an extraction-level `FlagUse` to a result-level `FeatureFlag`.
fn flag_use_to_feature_flag(flag_use: &FlagUse, path: PathBuf) -> FeatureFlag {
    let (kind, confidence) = match flag_use.kind {
        FlagUseKind::EnvVar => (FlagKind::EnvironmentVariable, FlagConfidence::High),
        FlagUseKind::SdkCall => (FlagKind::SdkCall, FlagConfidence::High),
        FlagUseKind::ConfigObject => (FlagKind::ConfigObject, FlagConfidence::Low),
    };

    FeatureFlag {
        path,
        flag_name: flag_use.flag_name.clone(),
        kind,
        confidence,
        line: flag_use.line,
        col: flag_use.col,
        guard_span_start: flag_use.guard_span_start,
        guard_span_end: flag_use.guard_span_end,
        sdk_name: flag_use.sdk_name.clone(),
        guard_line_start: None,
        guard_line_end: None,
        guarded_dead_exports: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_use_to_feature_flag_env_var() {
        let flag_use = FlagUse {
            flag_name: "FEATURE_X".to_string(),
            kind: FlagUseKind::EnvVar,
            line: 10,
            col: 4,
            guard_span_start: Some(100),
            guard_span_end: Some(200),
            sdk_name: None,
        };

        let result = flag_use_to_feature_flag(&flag_use, PathBuf::from("src/config.ts"));
        assert_eq!(result.flag_name, "FEATURE_X");
        assert_eq!(result.kind, FlagKind::EnvironmentVariable);
        assert_eq!(result.confidence, FlagConfidence::High);
        assert_eq!(result.line, 10);
        assert!(result.guard_span_start.is_some());
    }

    #[test]
    fn flag_use_to_feature_flag_sdk_call() {
        let flag_use = FlagUse {
            flag_name: "new-checkout".to_string(),
            kind: FlagUseKind::SdkCall,
            line: 5,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: Some("LaunchDarkly".to_string()),
        };

        let result = flag_use_to_feature_flag(&flag_use, PathBuf::from("src/hooks.ts"));
        assert_eq!(result.kind, FlagKind::SdkCall);
        assert_eq!(result.confidence, FlagConfidence::High);
        assert_eq!(result.sdk_name.as_deref(), Some("LaunchDarkly"));
    }

    #[test]
    fn flag_use_to_feature_flag_config_object() {
        let flag_use = FlagUse {
            flag_name: "features.newCheckout".to_string(),
            kind: FlagUseKind::ConfigObject,
            line: 42,
            col: 8,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
        };

        let result = flag_use_to_feature_flag(&flag_use, PathBuf::from("src/app.ts"));
        assert_eq!(result.kind, FlagKind::ConfigObject);
        assert_eq!(result.confidence, FlagConfidence::Low);
    }
}
