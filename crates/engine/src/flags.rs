//! Feature flag analysis owned by the engine boundary.

use std::path::Path;

use fallow_config::ResolvedConfig;
use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::{FlagUse, FlagUseKind, ModuleInfo, ParseResult};
use fallow_types::results::{AnalysisResults, FeatureFlag, FlagConfidence, FlagKind};
use rustc_hash::FxHashMap;

use crate::dead_code::analyze_with_parse_result;
use crate::discover::discover_files_with_plugin_scopes;
use crate::session::parse_files_for_config;
use crate::suppress::{IssueKind, is_file_suppressed, is_suppressed};

/// Typed result from running feature flag analysis.
#[derive(Debug, Clone)]
pub struct FeatureFlagsAnalysis {
    pub flags: Vec<FeatureFlag>,
    pub files_scanned: usize,
}

/// Run feature flag analysis for a resolved project config.
#[must_use]
pub fn analyze_feature_flags(config: &ResolvedConfig) -> FeatureFlagsAnalysis {
    let files = discover_files_with_plugin_scopes(config);
    let flags = collect_flags_for_files(config, &files);
    FeatureFlagsAnalysis {
        flags,
        files_scanned: files.len(),
    }
}

/// Built-in environment variable prefixes treated as feature flags.
#[must_use]
pub fn builtin_env_prefixes() -> &'static [&'static str] {
    crate::feature_flags::builtin_env_prefixes()
}

/// Distinct built-in SDK provider labels, in declaration order.
#[must_use]
pub fn builtin_sdk_providers() -> Vec<&'static str> {
    crate::feature_flags::builtin_sdk_providers()
}

fn collect_flags_for_files(config: &ResolvedConfig, files: &[DiscoveredFile]) -> Vec<FeatureFlag> {
    let parse_result = parse_files_for_config(config, files, false);

    let mut flags = collect_flags_from_parse_result(config, files, &parse_result);
    correlate_flags_with_dead_code(&mut flags, config, &parse_result);
    flags
}

fn correlate_flags_with_dead_code(
    flags: &mut [FeatureFlag],
    config: &ResolvedConfig,
    parse_result: &ParseResult,
) {
    if let Ok(analysis_output) = analyze_with_parse_result(config, &parse_result.modules) {
        correlate_with_dead_code(flags, &analysis_output.results);
    }
}

fn correlate_with_dead_code(flags: &mut [FeatureFlag], results: &AnalysisResults) {
    if results.unused_exports.is_empty() && results.unused_types.is_empty() {
        return;
    }

    for flag in flags.iter_mut() {
        let (Some(guard_start), Some(guard_end)) = (flag.guard_line_start, flag.guard_line_end)
        else {
            continue;
        };

        for export in &results.unused_exports {
            if export.export.path == flag.path
                && export.export.line >= guard_start
                && export.export.line <= guard_end
            {
                flag.guarded_dead_exports
                    .push(export.export.export_name.clone());
            }
        }

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

fn collect_flags_from_parse_result(
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
    parse_result: &ParseResult,
) -> Vec<FeatureFlag> {
    let file_paths: FxHashMap<_, _> = files.iter().map(|file| (file.id, &file.path)).collect();

    let extra_sdk: Vec<(String, usize, String)> = config
        .flags
        .sdk_patterns
        .iter()
        .map(|pattern| {
            (
                pattern.function.clone(),
                pattern.name_arg,
                pattern.provider.clone().unwrap_or_default(),
            )
        })
        .collect();
    let has_custom_config = !extra_sdk.is_empty()
        || !config.flags.env_prefixes.is_empty()
        || config.flags.config_object_heuristics;

    let mut flags = Vec::new();
    for module in &parse_result.modules {
        let Some(path) = file_paths.get(&module.file_id) else {
            continue;
        };

        collect_builtin_flags(&mut flags, module, path);
        if has_custom_config {
            collect_custom_flags(&mut flags, config, module, path, &extra_sdk);
        }
    }
    flags
}

fn collect_builtin_flags(flags: &mut Vec<FeatureFlag>, module: &ModuleInfo, path: &Path) {
    let file_suppressed = is_file_suppressed(&module.suppressions, IssueKind::FeatureFlag);
    for flag_use in &module.flag_uses {
        if file_suppressed
            || is_suppressed(&module.suppressions, flag_use.line, IssueKind::FeatureFlag)
        {
            continue;
        }
        flags.push(flag_use_to_feature_flag(flag_use, module, path));
    }
}

fn collect_custom_flags(
    flags: &mut Vec<FeatureFlag>,
    config: &ResolvedConfig,
    module: &ModuleInfo,
    path: &Path,
    extra_sdk: &[(String, usize, String)],
) {
    let Ok(source) = std::fs::read_to_string(path) else {
        return;
    };

    let custom_flags = crate::feature_flags::extract_flags_from_source(
        &source,
        path,
        extra_sdk,
        &config.flags.env_prefixes,
        config.flags.config_object_heuristics,
    );
    for flag_use in &custom_flags {
        let already_found = module.flag_uses.iter().any(|existing| {
            existing.line == flag_use.line && existing.flag_name == flag_use.flag_name
        });
        if !already_found
            && !is_suppressed(&module.suppressions, flag_use.line, IssueKind::FeatureFlag)
        {
            flags.push(flag_use_to_feature_flag(flag_use, module, path));
        }
    }
}

fn flag_use_to_feature_flag(flag_use: &FlagUse, module: &ModuleInfo, path: &Path) -> FeatureFlag {
    let (kind, confidence) = match flag_use.kind {
        FlagUseKind::EnvVar => (FlagKind::EnvironmentVariable, FlagConfidence::High),
        FlagUseKind::SdkCall => (FlagKind::SdkCall, FlagConfidence::High),
        FlagUseKind::ConfigObject => (FlagKind::ConfigObject, FlagConfidence::Low),
    };

    let (guard_line_start, guard_line_end) = if let (Some(start), Some(end)) =
        (flag_use.guard_span_start, flag_use.guard_span_end)
        && !module.line_offsets.is_empty()
    {
        let (start_line, _) =
            fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, start);
        let (end_line, _) =
            fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, end);
        (Some(start_line), Some(end_line))
    } else {
        (None, None)
    };

    FeatureFlag {
        path: path.to_path_buf(),
        flag_name: flag_use.flag_name.clone(),
        kind,
        confidence,
        line: flag_use.line,
        col: flag_use.col,
        guard_span_start: flag_use.guard_span_start,
        guard_span_end: flag_use.guard_span_end,
        sdk_name: flag_use.sdk_name.clone(),
        guard_line_start,
        guard_line_end,
        guarded_dead_exports: Vec::new(),
    }
}
