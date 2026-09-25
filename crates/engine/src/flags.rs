//! Feature flag analysis owned by the engine boundary.

use std::{path::Path, sync::Arc};

use fallow_types::discover::DiscoveredFile;
use fallow_types::extract::{FlagUse, FlagUseKind, ModuleInfo};
use fallow_types::results::{AnalysisResults, FeatureFlag, FlagConfidence, FlagKind, UnusedExport};
use rustc_hash::FxHashMap;

use crate::flag_registry::RegistryIndex;
use crate::session::AnalysisSession;
use crate::suppress::{IssueKind, is_file_suppressed, is_suppressed};

/// Typed result from running feature flag analysis.
#[derive(Debug, Clone)]
pub struct FeatureFlagsAnalysis {
    /// Detected feature flags with their usage sites and confidence.
    pub flags: Vec<FeatureFlag>,
    /// Number of files the flag scan covered.
    pub files_scanned: usize,
}

/// Run feature flag analysis with a reusable analysis session.
///
/// # Errors
///
/// Returns [`crate::EngineError::cancelled`] when the session's caller
/// cancelled the run. The scan spends its time in the parse loop and in the
/// dead-code correlation behind it, and both observe the token. A session
/// without a cancellation token can never return this error.
pub fn analyze_feature_flags_with_session(
    session: &AnalysisSession,
) -> crate::EngineResult<FeatureFlagsAnalysis> {
    let modules = session.shared_parsed_modules_cancellable(false, "the feature-flag scan")?;
    let flags = collect_flags_for_modules(session, session.files(), &modules)?;
    Ok(FeatureFlagsAnalysis {
        flags,
        files_scanned: session.files().len(),
    })
}

/// Run feature flag analysis while reusing dead-code results from the same
/// session.
///
/// Compound surfaces such as `fallow viz` use this path to avoid rebuilding
/// the module graph solely to correlate guarded dead exports.
#[must_use]
pub fn analyze_feature_flags_with_session_and_results(
    session: &AnalysisSession,
    results: &AnalysisResults,
) -> FeatureFlagsAnalysis {
    let modules = session.shared_parsed_modules(false);
    let mut flags = collect_flags_from_modules(session.files(), &modules);
    correlate_with_dead_code(&mut flags, results);
    FeatureFlagsAnalysis {
        flags,
        files_scanned: session.files().len(),
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

fn collect_flags_for_modules(
    session: &AnalysisSession,
    files: &[DiscoveredFile],
    modules: &Arc<[ModuleInfo]>,
) -> crate::EngineResult<Vec<FeatureFlag>> {
    let mut flags = collect_flags_from_modules(files, modules);
    correlate_flags_with_dead_code(&mut flags, session, modules)?;
    Ok(flags)
}

fn correlate_flags_with_dead_code(
    flags: &mut [FeatureFlag],
    session: &AnalysisSession,
    modules: &Arc<[ModuleInfo]>,
) -> crate::EngineResult<()> {
    match session.analyze_dead_code_with_shared_modules(Arc::clone(modules)) {
        Ok(analysis_output) => correlate_with_dead_code(flags, &analysis_output.results),
        // Correlation only enriches the flags, so a broken dead-code pass
        // leaves them uncorrelated rather than failing the scan. A cancelled
        // one is not a failure to enrich, it is the caller asking to stop.
        Err(err) if err.is_cancelled() => return Err(err),
        Err(_) => {}
    }
    Ok(())
}

fn correlate_with_dead_code(flags: &mut [FeatureFlag], results: &AnalysisResults) {
    if results.unused_exports.is_empty() && results.unused_types.is_empty() {
        return;
    }

    let exports =
        ExportLineIndex::new(results.unused_exports.iter().map(|finding| &finding.export));
    let types = ExportLineIndex::new(results.unused_types.iter().map(|finding| &finding.export));
    for flag in flags.iter_mut() {
        let (Some(guard_start), Some(guard_end)) = (flag.guard_line_start, flag.guard_line_end)
        else {
            continue;
        };
        for index in [&exports, &types] {
            flag.guarded_dead_exports
                .extend(index.names_in(&flag.path, guard_start, guard_end));
        }
    }
}

/// Unused exports grouped by file and sorted by line, so the guard lookup
/// of each flag is a binary search and not a scan of every finding.
struct ExportLineIndex<'r> {
    by_path: FxHashMap<&'r Path, Vec<(u32, usize, &'r str)>>,
}

impl<'r> ExportLineIndex<'r> {
    fn new(exports: impl Iterator<Item = &'r UnusedExport>) -> Self {
        let mut by_path: FxHashMap<&Path, Vec<(u32, usize, &str)>> = FxHashMap::default();
        for (position, export) in exports.enumerate() {
            by_path.entry(export.path.as_path()).or_default().push((
                export.line,
                position,
                export.export_name.as_str(),
            ));
        }
        for entries in by_path.values_mut() {
            entries.sort_unstable_by_key(|&(line, position, _)| (line, position));
        }
        Self { by_path }
    }

    /// Names of the exports in `path` on lines `start..=end`, in the order of
    /// the findings.
    fn names_in(&self, path: &Path, start: u32, end: u32) -> Vec<String> {
        let mut matches: Vec<(usize, &str)> = self
            .by_path
            .get(path)
            .map(|entries| {
                let first = entries.partition_point(|&(line, _, _)| line < start);
                entries[first..]
                    .iter()
                    .take_while(|&&(line, _, _)| line <= end)
                    .map(|&(_, position, name)| (position, name))
                    .collect()
            })
            .unwrap_or_default();
        matches.sort_unstable_by_key(|&(position, _)| position);
        matches
            .into_iter()
            .map(|(_, name)| name.to_string())
            .collect()
    }
}

fn collect_flags_from_modules(
    files: &[DiscoveredFile],
    modules: &[ModuleInfo],
) -> Vec<FeatureFlag> {
    let file_paths: FxHashMap<_, _> = files.iter().map(|file| (file.id, &file.path)).collect();

    let registry_index = RegistryIndex::build(files, modules);
    let mut flags = Vec::new();
    for module in modules {
        let Some(path) = file_paths.get(&module.file_id) else {
            continue;
        };

        collect_builtin_flags(&mut flags, module, path);
        if let Some(index) = &registry_index {
            collect_registry_flags(&mut flags, module, path, index);
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

/// Resolve reads such as `useFlag(FLAGS.X)`, where `FLAGS` is imported.
fn collect_registry_flags(
    flags: &mut Vec<FeatureFlag>,
    module: &ModuleInfo,
    path: &Path,
    index: &RegistryIndex<'_>,
) {
    let Some(facts) = module.flag_registry_facts.as_ref() else {
        return;
    };
    if facts.reads.is_empty() || is_file_suppressed(&module.suppressions, IssueKind::FeatureFlag) {
        return;
    }
    for read in &facts.reads {
        if is_suppressed(
            &module.suppressions,
            read.flag_use.line,
            IssueKind::FeatureFlag,
        ) {
            continue;
        }
        let Some(key) = index.resolve(module, path, read) else {
            continue;
        };
        let mut flag = flag_use_to_feature_flag(&read.flag_use, module, path);
        flag.flag_name = key.to_string();
        flags.push(flag);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_runner_uses_session_discovery_instead_of_rediscovering() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"flags-session","main":"src/index.ts"}"#,
        )
        .expect("package json");
        std::fs::write(
            root.join("src/index.ts"),
            "if (process.env.FEATURE_EXISTING) {}\n",
        )
        .expect("initial source");

        let session = AnalysisSession::load(root, None).expect("session loads");

        std::fs::write(
            root.join("src/late.ts"),
            "if (process.env.FEATURE_LATE) {}\n",
        )
        .expect("late source");

        let session_flags =
            analyze_feature_flags_with_session(&session).expect("session flag scan");
        let session_names: Vec<_> = session_flags
            .flags
            .iter()
            .map(|flag| flag.flag_name.as_str())
            .collect();
        assert_eq!(session_names, vec!["FEATURE_EXISTING"]);

        let second_session_flags =
            analyze_feature_flags_with_session(&session).expect("second session flag scan");
        let second_session_names: Vec<_> = second_session_flags
            .flags
            .iter()
            .map(|flag| flag.flag_name.as_str())
            .collect();
        assert_eq!(second_session_names, vec!["FEATURE_EXISTING"]);
    }

    fn scan(files: &[(&str, &str)]) -> Vec<FeatureFlag> {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"flag-registries","main":"src/index.ts"}"#,
        )
        .expect("package json");
        for (path, source) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, source).expect("source");
        }
        let session = AnalysisSession::load(root, None).expect("session loads");
        let mut flags = analyze_feature_flags_with_session(&session)
            .expect("flag scan")
            .flags;
        flags.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
        flags
    }

    fn names(flags: &[FeatureFlag]) -> Vec<&str> {
        flags.iter().map(|flag| flag.flag_name.as_str()).collect()
    }

    #[test]
    fn resolves_keys_through_relative_registry_imports() {
        let flags = scan(&[
            (
                "src/flags.ts",
                "export const FLAGS = { NewCheckout: 'new-checkout' } as const;\n\
                 export enum Gates { Beta = 'beta-gate' }\n",
            ),
            (
                "src/index.ts",
                "import { FLAGS, Gates as G } from './flags.js';\n\
                 if (useFlag(FLAGS.NewCheckout)) { run(); }\n\
                 useGate(G.Beta);\n\
                 useFlag(FLAGS.Missing);\n",
            ),
        ]);
        assert_eq!(names(&flags), ["new-checkout", "beta-gate"]);
        assert_eq!(flags[0].line, 2);
        assert_eq!(flags[0].sdk_name.as_deref(), Some("LaunchDarkly"));
        assert_eq!(flags[0].guard_line_start, Some(2));
    }

    #[test]
    fn resolves_alias_and_barrel_imports_by_the_unique_registry_name() {
        let flags = scan(&[
            (
                "src/config/flags.ts",
                "export const FLAGS = { Chat: 'chat' } as const;\n",
            ),
            ("src/config/index.ts", "export { FLAGS } from './flags';\n"),
            (
                "src/index.ts",
                "import { FLAGS } from '@/config';\n\
                 import { FLAGS as BarrelFlags } from './config';\n\
                 useFlag(FLAGS.Chat);\n\
                 useFlag(BarrelFlags.Chat);\n",
            ),
        ]);
        assert_eq!(names(&flags), ["chat", "chat"]);
    }

    #[test]
    fn leaves_ambiguous_and_non_imported_registries_unresolved() {
        let flags = scan(&[
            (
                "src/a.ts",
                "export const FLAGS = { Chat: 'chat-a' } as const;\n",
            ),
            (
                "src/b.ts",
                "export const FLAGS = { Chat: 'chat-b' } as const;\n",
            ),
            (
                "src/index.ts",
                "import { FLAGS } from '@/flags';\n\
                 useFlag(FLAGS.Chat);\n\
                 const local = { Chat: 'local' };\n\
                 useFlag(local.Chat);\n",
            ),
        ]);
        assert!(flags.is_empty(), "unexpected flags: {:?}", names(&flags));
    }

    #[test]
    fn registry_reads_honor_suppressions() {
        let flags = scan(&[
            (
                "src/flags.ts",
                "export const FLAGS = { Chat: 'chat' } as const;\n",
            ),
            (
                "src/index.ts",
                "import { FLAGS } from './flags';\n\
                 // fallow-ignore-next-line feature-flag\n\
                 useFlag(FLAGS.Chat);\n\
                 useFlag(FLAGS.Chat);\n",
            ),
        ]);
        assert_eq!(names(&flags), ["chat"]);
        assert_eq!(flags[0].line, 4);
    }

    fn unused(path: &str, name: &str, line: u32) -> UnusedExport {
        UnusedExport {
            path: std::path::PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: false,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }
    }

    fn guarded_flag(path: &str, lines: Option<(u32, u32)>) -> FeatureFlag {
        FeatureFlag {
            path: std::path::PathBuf::from(path),
            flag_name: "flag".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 1,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            guard_line_start: lines.map(|(start, _)| start),
            guard_line_end: lines.map(|(_, end)| end),
            guarded_dead_exports: Vec::new(),
        }
    }

    /// The loop the index replaced: every flag against every finding.
    fn correlate_by_scan(flags: &mut [FeatureFlag], results: &AnalysisResults) {
        for flag in flags.iter_mut() {
            let (Some(start), Some(end)) = (flag.guard_line_start, flag.guard_line_end) else {
                continue;
            };
            let exports = results.unused_exports.iter().map(|finding| &finding.export);
            let types = results.unused_types.iter().map(|finding| &finding.export);
            for export in exports.chain(types) {
                if export.path == flag.path && export.line >= start && export.line <= end {
                    flag.guarded_dead_exports.push(export.export_name.clone());
                }
            }
        }
    }

    #[test]
    fn indexed_correlation_matches_the_full_scan() {
        use fallow_types::output_dead_code::{UnusedExportFinding, UnusedTypeFinding};

        let mut results = AnalysisResults::default();
        for export in [
            unused("src/b.ts", "late", 40),
            unused("src/a.ts", "second", 12),
            unused("src/a.ts", "first", 10),
            unused("src/a.ts", "sameLineB", 12),
            unused("src/a.ts", "edgeEnd", 20),
            unused("src/a.ts", "outside", 21),
            unused("src/b.ts", "early", 2),
        ] {
            results
                .unused_exports
                .push(UnusedExportFinding::with_actions(export));
        }
        for export in [
            unused("src/a.ts", "Shape", 15),
            unused("src/a.ts", "Before", 9),
        ] {
            results
                .unused_types
                .push(UnusedTypeFinding::with_actions(export));
        }
        let flags = || {
            vec![
                guarded_flag("src/a.ts", Some((10, 20))),
                guarded_flag("src/a.ts", Some((12, 12))),
                guarded_flag("src/b.ts", Some((1, 50))),
                guarded_flag("src/c.ts", Some((1, 50))),
                guarded_flag("src/a.ts", None),
            ]
        };

        let mut indexed = flags();
        correlate_with_dead_code(&mut indexed, &results);
        let mut scanned = flags();
        correlate_by_scan(&mut scanned, &results);

        let names = |flags: &[FeatureFlag]| -> Vec<Vec<String>> {
            flags
                .iter()
                .map(|flag| flag.guarded_dead_exports.clone())
                .collect()
        };
        assert_eq!(names(&indexed), names(&scanned));
        assert_eq!(
            names(&indexed)[0],
            ["second", "first", "sameLineB", "edgeEnd", "Shape"]
        );
    }

    #[test]
    fn custom_patterns_apply_in_the_one_parse() {
        let flags = scan(&[
            (
                ".fallowrc.json",
                r#"{"flags":{"sdkPatterns":[{"function":"isFeatureActive","provider":"Internal"}],"envPrefixes":["MYAPP_"]}}"#,
            ),
            (
                "src/keys.ts",
                "export const KEYS = { Beta: 'beta' } as const;\n",
            ),
            (
                "src/index.ts",
                "import { KEYS } from './keys';\n\
                 export const a = isFeatureActive(KEYS.Beta);\n\
                 export const b = isFeatureActive('literal');\n",
            ),
            (
                "src/app.js",
                "// A .js file with JSX parses again as JSX, and the flags come from that parse.\n\
                 export const App = () => <div>{x}</div>;\n\
                 export const key = process.env.MYAPP_BETA;\n",
            ),
        ]);
        assert_eq!(names(&flags), ["MYAPP_BETA", "beta", "literal"]);
        assert!(
            flags[1..]
                .iter()
                .all(|flag| flag.sdk_name.as_deref() == Some("Internal"))
        );
    }
}
