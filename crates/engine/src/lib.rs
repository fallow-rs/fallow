//! Typed analysis engine boundary for fallow consumers.
//!
//! `fallow-core` remains the internal orchestration backend. This crate owns
//! the typed boundary that editor, API, and embedding surfaces can depend on
//! without calling deprecated core entry points directly. Public modules should
//! expose owned engine runners, typed result structs, or narrowly scoped aliases
//! instead of broad core re-exports.

#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use std::fmt;
#[cfg(test)]
use std::path::Path;

pub mod baseline;
pub mod changed_files;
pub mod churn;
pub mod codeowners;
mod core_backend;
pub mod cross_reference;
mod css;
pub mod dead_code;
pub mod discover;
pub mod duplicates;
mod error;
mod feature_flags;
pub mod flags;
#[path = "git_env.rs"]
mod git_env;
pub mod guard;
pub mod health;
pub mod module_graph;
pub mod plugins;
pub mod project_analysis;
pub mod project_config;
mod public_api;
mod results;
mod security;
pub mod session;
pub mod source;
mod suppress;
pub mod trace;
pub mod trace_chain;
pub mod validate;
pub mod vital_signs;

/// Result alias for typed engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

/// Error type exposed by the typed engine boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError {
    message: String,
}

impl EngineError {
    /// Create an engine error from a user-facing message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// User-facing error message from the backend.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EngineError {}

pub(crate) fn engine_error(err: impl fmt::Display) -> EngineError {
    EngineError::new(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        project_analysis::ProjectAnalysisArtifactOptions,
        project_config::{
            ProjectConfigOptions, config_for_project, config_for_project_analysis,
            resolve_cache_max_size_bytes,
        },
        session::AnalysisSession,
    };
    use fallow_config::ProductionAnalysis;
    use fallow_types::output_format::OutputFormat;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn engine_error_displays_message() {
        let err = EngineError::new("config failed");

        assert_eq!(err.message(), "config failed");
        assert_eq!(err.to_string(), "config failed");
    }

    #[test]
    fn engine_resolves_parse_cache_size_policy() {
        let mut config = fallow_config::FallowConfig::default().resolve(
            PathBuf::from("/repo"),
            OutputFormat::Json,
            1,
            false,
            true,
            None,
        );
        assert_eq!(
            resolve_cache_max_size_bytes(&config),
            fallow_extract::cache::DEFAULT_CACHE_MAX_SIZE
        );

        config.cache_max_size_mb = Some(3);
        assert_eq!(resolve_cache_max_size_bytes(&config), 3 * 1024 * 1024);

        config.cache_max_size_mb = Some(u32::MAX);
        assert_eq!(
            resolve_cache_max_size_bytes(&config),
            (u32::MAX as usize).saturating_mul(1024 * 1024)
        );
    }

    #[test]
    fn engine_root_does_not_reexport_broad_surface_modules() {
        let source = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
            .expect("read engine lib");
        let public_surface = source
            .split("#[cfg(test)]")
            .next()
            .expect("engine lib has public surface before tests");
        let forbidden_exports = [
            "pub use error::",
            "pub use flags::",
            "pub use git_env::",
            "pub use public_api::",
            "pub use results::",
            "pub use security::",
            "pub use suppress::",
            "health_shared_parse_data_from_artifacts",
        ];

        for forbidden in forbidden_exports {
            assert!(
                !public_surface.contains(forbidden),
                "engine root must expose typed modules, not `{forbidden}`"
            );
        }
    }

    #[test]
    fn engine_session_owns_dead_code_pipeline_sequence() {
        let session_source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/session.rs"))
                .expect("read engine session");
        assert!(
            !session_source.contains("analyze_with_owned_parse_result_from_discovery"),
            "engine session must not delegate dead-code orchestration to the old core monolith"
        );
        for required_phase in [
            "prepare_dead_code_backend_prelude",
            "discover_dead_code_entry_points",
            "try_load_dead_code_graph_cache",
            "resolve_dead_code_imports",
            "build_dead_code_graph",
            "run_dead_code_detectors",
        ] {
            assert!(
                session_source.contains(required_phase),
                "engine session must explicitly sequence `{required_phase}`"
            );
        }
    }

    #[test]
    fn engine_session_owns_analysis_discovery() {
        let session_source =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/session.rs"))
                .expect("read engine session");
        assert!(
            session_source.contains("crate::discover::prepare_analysis_discovery"),
            "engine session must build discovery through the engine discovery boundary"
        );
        assert!(
            session_source.contains("prepare_analysis_discovery_with_workspaces"),
            "engine session must reuse workspace metadata captured during config load"
        );
        assert!(
            session_source.contains("workspace_discovery_ms.is_some()"),
            "AnalysisSession::from_config must only reuse workspace metadata when ProjectConfig preloaded it"
        );
        assert!(
            !session_source.contains("core_backend::prepare_analysis_discovery"),
            "engine session must not delegate discovery orchestration to core_backend"
        );
    }

    #[test]
    fn analysis_session_loads_config_and_discovered_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");

        assert_eq!(session.root(), temp.path());
        assert!(session.config_path().is_none());
        assert!(session.files().iter().any(|file| {
            file.path
                .strip_prefix(temp.path())
                .is_ok_and(|path| path == Path::new("src/index.ts"))
        }));
    }

    #[test]
    fn analysis_session_applies_config_adjustment_before_discovery() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");
        std::fs::write(src.join("index.test.ts"), "export const testValue = 1;\n")
            .expect("test source file");

        let session = AnalysisSession::load_with_config(temp.path(), None, |config| {
            config.production = true;
        })
        .expect("session loads");

        let relative_paths: Vec<_> = session
            .files()
            .iter()
            .filter_map(|file| file.path.strip_prefix(temp.path()).ok())
            .collect();
        assert!(relative_paths.contains(&Path::new("src/index.ts")));
        assert!(!relative_paths.contains(&Path::new("src/index.test.ts")));
    }

    #[test]
    fn analysis_session_config_adjustment_invalidates_preloaded_workspaces() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .expect("root package");
        std::fs::create_dir_all(temp.path().join("packages/a")).expect("workspace dir");
        std::fs::create_dir_all(temp.path().join("packages/ignored")).expect("ignored dir");
        std::fs::write(
            temp.path().join("packages/a/package.json"),
            r#"{"name":"a","main":"src/index.ts"}"#,
        )
        .expect("workspace package");

        let session = AnalysisSession::load_with_config(temp.path(), None, |config| {
            config.ignore_patterns = globset::GlobSetBuilder::new()
                .add(globset::Glob::new("packages/ignored").expect("ignore glob"))
                .build()
                .expect("ignore set");
        })
        .expect("session loads");

        assert!(
            session
                .workspaces()
                .iter()
                .all(|workspace| workspace.name != "ignored"),
            "config mutations that affect workspace discovery must not reuse preloaded workspaces"
        );
        assert!(
            !session
                .workspace_diagnostics()
                .iter()
                .any(|diagnostic| diagnostic.path.ends_with("packages/ignored")),
            "config mutations that affect workspace diagnostics must not reuse stale diagnostics"
        );
    }

    #[test]
    fn analysis_session_captures_workspace_diagnostics() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"name":"diagnostic-root","workspaces":["packages/*"]}"#,
        )
        .expect("package json");
        std::fs::create_dir_all(temp.path().join("packages/empty")).expect("workspace dir");
        std::fs::create_dir(temp.path().join("src")).expect("src dir");
        std::fs::write(
            temp.path().join("src/index.ts"),
            "export const value = 1;\n",
        )
        .expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");

        assert!(session.workspace_diagnostics().iter().any(|diagnostic| {
            diagnostic.kind.id() == "glob-matched-no-package-json"
                && diagnostic.path.ends_with("packages/empty")
        }));
    }

    #[test]
    fn analysis_session_from_resolved_config_discovers_workspaces() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            temp.path().join("package.json"),
            r#"{"name":"root","workspaces":["packages/*"]}"#,
        )
        .expect("root package");
        std::fs::create_dir_all(temp.path().join("packages/a/src")).expect("workspace dir");
        std::fs::write(
            temp.path().join("packages/a/package.json"),
            r#"{"name":"pkg-a","main":"src/index.ts"}"#,
        )
        .expect("workspace package");
        std::fs::write(
            temp.path().join("packages/a/src/index.ts"),
            "export const value = 1;\n",
        )
        .expect("workspace source");

        let config = fallow_config::FallowConfig::default().resolve(
            temp.path().to_path_buf(),
            OutputFormat::Json,
            1,
            false,
            true,
            None,
        );
        let session = AnalysisSession::from_resolved_config(config);

        assert!(
            session
                .workspaces()
                .iter()
                .any(|workspace| workspace.name == "pkg-a"),
            "resolved-config sessions must expose workspaces found during fallback discovery"
        );
    }

    #[test]
    fn analysis_session_can_be_consumed_into_pipeline_parts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let parts = session.into_parts();

        assert_eq!(parts.config.root, temp.path());
        assert!(parts.config_path.is_none());
        assert!(parts.files.iter().any(|file| {
            file.path
                .strip_prefix(temp.path())
                .is_ok_and(|path| path == Path::new("src/index.ts"))
        }));
    }

    #[test]
    fn analysis_session_can_be_consumed_into_parsed_pipeline_parts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        std::fs::write(src.join("late.ts"), "export const late = 1;\n").expect("late source file");
        let parts = session.into_parsed_parts(false);

        assert_eq!(parts.config.root, temp.path());
        assert!(parts.config_path.is_none());
        assert!(parts.modules.iter().any(|module| {
            parts.files[module.file_id.0 as usize]
                .path
                .strip_prefix(temp.path())
                .is_ok_and(|path| path == Path::new("src/index.ts"))
        }));
        assert!(parts.modules.iter().all(|module| {
            !parts.files[module.file_id.0 as usize]
                .path
                .ends_with("late.ts")
        }));
    }

    #[test]
    fn analysis_session_reuses_complexity_parse_for_plain_parse() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(
            src.join("index.ts"),
            "export function value() { return 1; }\n",
        )
        .expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let first = session.parsed_parts(true);
        assert!(!first.modules.is_empty());

        let second = session.parsed_parts(false);

        assert!(!second.modules.is_empty());
        assert!(second.parse_ms.abs() < f64::EPSILON);
        assert!(second.parse_cpu_ms.abs() < f64::EPSILON);
    }

    #[test]
    fn dead_code_reused_parse_path_uses_engine_pipeline() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "import './util';\n").expect("entry file");
        std::fs::write(src.join("util.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let parts = session.into_parsed_parts(false);
        let analysis = crate::dead_code::analyze_with_parse_result(&parts.config, &parts.modules)
            .expect("reused parse analysis succeeds");

        assert!(analysis.graph.is_some());
        assert!(analysis.modules.is_none());
        assert!(analysis.files.is_none());
        assert!(
            analysis
                .file_hashes
                .keys()
                .any(|path| path.ends_with("util.ts"))
        );
    }

    #[test]
    fn analysis_session_reparses_when_cached_source_changes() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(
            src.join("index.ts"),
            "import { value } from './util';\nconsole.log(value);\n",
        )
        .expect("entry file");
        let util_path = src.join("util.ts");
        std::fs::write(&util_path, "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let first = session
            .analyze_project_with(&fallow_config::DuplicatesConfig::default(), true)
            .expect("first analysis succeeds");
        assert!(first.dead_code.results.unused_exports.is_empty());

        std::fs::write(
            &util_path,
            "export const value = 1;\nexport const addedUnused = 2;\n",
        )
        .expect("updated source file");

        let second = session
            .analyze_project_with(&fallow_config::DuplicatesConfig::default(), true)
            .expect("second analysis succeeds");
        assert!(
            second
                .dead_code
                .results
                .unused_exports
                .iter()
                .any(|finding| finding.export.export_name == "addedUnused")
        );
    }

    #[test]
    fn analysis_session_returns_combined_project_analysis() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        let repeated =
            "export function repeated() {\n  return ['alpha', 'beta', 'gamma'].join(',');\n}\n";
        std::fs::write(src.join("a.ts"), repeated).expect("source file");
        std::fs::write(src.join("b.ts"), repeated).expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let mut config = session.config().duplicates.clone();
        config.min_tokens = 1;
        config.min_lines = 1;

        let analysis = session
            .analyze_project_with(&config, true)
            .expect("project analysis succeeds");

        assert!(analysis.dead_code.modules.is_some());
        assert!(analysis.dead_code.files.is_some());
        assert!(!analysis.duplication.clone_groups.is_empty());
    }

    #[test]
    fn analysis_session_reuses_discovery_for_dead_code() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(src.join("index.ts"), "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        std::fs::write(src.join("late.ts"), "export const late = 1;\n").expect("late source file");

        let analysis = session.analyze_dead_code().expect("analysis succeeds");

        assert!(
            analysis
                .results
                .unused_files
                .iter()
                .all(|finding| !finding.file.path.ends_with("late.ts")),
            "session analysis must not rediscover files added after session load"
        );
    }

    #[test]
    fn analysis_session_returns_retained_artifacts() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(
            src.join("index.ts"),
            "export function used() { return 1; }\nused();\n",
        )
        .expect("source file");

        let config = config_for_project(temp.path(), None)
            .expect("config")
            .config;
        let session = AnalysisSession::from_resolved_config(config);
        let artifacts = session
            .analyze_dead_code_with_artifacts(true, true)
            .expect("analysis succeeds");

        assert!(artifacts.graph.is_some());
        assert!(artifacts.modules.is_some_and(|modules| !modules.is_empty()));
        assert!(artifacts.files.is_some_and(|files| !files.is_empty()));
    }

    #[test]
    fn analysis_session_returns_reuse_artifacts_with_fingerprints_and_scope() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        let source = src.join("index.ts");
        std::fs::write(&source, "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let mut changed_files = rustc_hash::FxHashSet::default();
        changed_files.insert(source.clone());
        let artifacts = session
            .analyze_dead_code_with_session_artifacts(false, true, Some(changed_files))
            .expect("analysis succeeds");

        assert!(artifacts.analysis.graph.is_some());
        assert!(
            artifacts
                .changed_files
                .as_ref()
                .is_some_and(|changed| changed.contains(&source))
        );
        assert!(
            artifacts
                .source_fingerprints
                .get(&source)
                .is_some_and(|fingerprint| fingerprint.file_size > 0)
        );
    }

    #[test]
    fn analysis_session_returns_project_artifacts_with_reuse_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        let source = src.join("index.ts");
        std::fs::write(&source, "export const value = 1;\n").expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let mut changed_files = rustc_hash::FxHashSet::default();
        changed_files.insert(source.clone());
        let artifacts = session
            .analyze_project_with_artifacts(
                &session.config().duplicates,
                ProjectAnalysisArtifactOptions {
                    retain_complexity_artifacts: true,
                    retain_graph: true,
                    changed_files: Some(changed_files),
                    collect_source_fingerprints: true,
                },
            )
            .expect("project analysis succeeds");

        assert!(artifacts.dead_code.graph.is_some());
        assert!(
            artifacts
                .changed_files
                .as_ref()
                .is_some_and(|changed| changed.contains(&source))
        );
        assert!(
            artifacts
                .source_fingerprints
                .as_ref()
                .and_then(|fingerprints| fingerprints.get(&source))
                .is_some_and(|fingerprint| fingerprint.file_size > 0)
        );

        let lightweight = session
            .analyze_project_with_artifacts(
                &session.config().duplicates,
                ProjectAnalysisArtifactOptions::default(),
            )
            .expect("project analysis succeeds");
        assert!(
            lightweight.source_fingerprints.is_none(),
            "source fingerprints should be opt-in for lightweight editor analysis"
        );

        let output = artifacts.into_output();
        assert!(output.dead_code.modules.is_some());
        assert!(output.dead_code.files.is_some());
    }

    #[test]
    fn project_artifacts_focus_duplication_to_changed_files() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        let repeated =
            "export function repeated() {\n  return ['alpha', 'beta', 'gamma'].join(',');\n}\n";
        let a = src.join("a.ts");
        std::fs::write(&a, repeated).expect("source file");
        std::fs::write(src.join("b.ts"), repeated).expect("source file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let mut config = session.config().duplicates.clone();
        config.min_tokens = 1;
        config.min_lines = 1;

        let full = session
            .analyze_project_with_artifacts(&config, ProjectAnalysisArtifactOptions::default())
            .expect("project analysis succeeds");
        assert!(!full.duplication.clone_groups.is_empty());

        let mut unrelated = rustc_hash::FxHashSet::default();
        unrelated.insert(src.join("unrelated.ts"));
        let focused_empty = session
            .analyze_project_with_artifacts(
                &config,
                ProjectAnalysisArtifactOptions {
                    changed_files: Some(unrelated),
                    ..ProjectAnalysisArtifactOptions::default()
                },
            )
            .expect("project analysis succeeds");
        assert!(focused_empty.duplication.clone_groups.is_empty());

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(a);
        let focused = session
            .analyze_project_with_artifacts(
                &config,
                ProjectAnalysisArtifactOptions {
                    changed_files: Some(changed),
                    ..ProjectAnalysisArtifactOptions::default()
                },
            )
            .expect("project analysis succeeds");
        assert!(!focused.duplication.clone_groups.is_empty());
    }

    #[test]
    fn analysis_session_runs_duplication_with_default_skip_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        let generated = temp.path().join("storybook-static");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::create_dir(&generated).expect("generated dir");
        let repeated =
            "export function repeated() {\n  return ['alpha', 'beta', 'gamma'].join(',');\n}\n";
        std::fs::write(src.join("a.ts"), repeated).expect("source file");
        std::fs::write(src.join("b.ts"), repeated).expect("source file");
        std::fs::write(generated.join("generated.ts"), repeated).expect("generated file");

        let session = AnalysisSession::load(temp.path(), None).expect("session loads");
        let mut config = session.config().duplicates.clone();
        config.min_tokens = 1;
        config.min_lines = 1;

        let analysis = session.find_duplicates_with_defaults(&config, None);

        assert!(!analysis.report.clone_groups.is_empty());
        assert!(analysis.default_ignore_skips.total > 0);
    }

    #[test]
    fn trace_symbol_chain_uses_retained_engine_analysis() {
        let temp = tempfile::tempdir().expect("tempdir");
        let src = temp.path().join("src");
        std::fs::create_dir(&src).expect("src dir");
        std::fs::write(
            src.join("util.ts"),
            "export function helper() { return 1; }\n",
        )
        .expect("util source");
        std::fs::write(
            src.join("index.ts"),
            "import { helper } from './util';\nexport const value = helper();\n",
        )
        .expect("index source");

        let project_config = config_for_project_analysis(
            temp.path(),
            None,
            ProjectConfigOptions {
                output: OutputFormat::Json,
                no_cache: true,
                threads: 1,
                production_override: None,
                quiet: true,
                analysis: ProductionAnalysis::DeadCode,
            },
        )
        .expect("project config loads");
        let session = AnalysisSession::from_config(project_config);
        let trace = crate::trace_chain::trace_symbol_chain_with_session(
            &session,
            fallow_types::trace_chain::SymbolChainQuery {
                file: "src/util.ts",
                symbol: "helper",
                depth: 1,
                directions: fallow_types::trace_chain::TraceDirections {
                    callers: true,
                    callees: false,
                },
            },
        )
        .expect("trace succeeds")
        .expect("trace target exists");

        assert!(trace.symbol_found);
        assert_eq!(trace.file, Path::new("src/util.ts"));
        assert!(trace.callers.is_some_and(|callers| {
            callers
                .iter()
                .any(|caller| caller.file == Path::new("src/index.ts"))
        }));
    }
}
