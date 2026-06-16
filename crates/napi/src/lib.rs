#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use fallow_cli::programmatic;
use napi::bindgen_prelude::{AsyncTask, JsObjectValue, ToNapiValue, Unknown};
use napi::{Env, ScopedTask, Status};
use napi_derive::napi;

#[napi(object)]
#[derive(Default)]
pub struct DeadCodeOptions {
    pub root: Option<String>,
    pub config_path: Option<String>,
    pub no_cache: Option<bool>,
    pub threads: Option<u32>,
    pub diff_file: Option<String>,
    pub production: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: Option<bool>,
    pub legacy_envelope: Option<bool>,
    pub unused_files: Option<bool>,
    pub unused_exports: Option<bool>,
    pub unused_deps: Option<bool>,
    pub unused_types: Option<bool>,
    pub private_type_leaks: Option<bool>,
    pub unused_enum_members: Option<bool>,
    pub unused_class_members: Option<bool>,
    pub unused_store_members: Option<bool>,
    pub unprovided_injects: Option<bool>,
    pub unrendered_components: Option<bool>,
    pub unused_component_props: Option<bool>,
    pub unused_component_emits: Option<bool>,
    pub unused_component_inputs: Option<bool>,
    pub unused_component_outputs: Option<bool>,
    pub unused_server_actions: Option<bool>,
    pub unused_load_data_keys: Option<bool>,
    pub unresolved_imports: Option<bool>,
    pub unlisted_deps: Option<bool>,
    pub duplicate_exports: Option<bool>,
    pub circular_deps: Option<bool>,
    pub re_export_cycles: Option<bool>,
    pub boundary_violations: Option<bool>,
    pub policy_violations: Option<bool>,
    pub stale_suppressions: Option<bool>,
    pub unused_catalog_entries: Option<bool>,
    pub empty_catalog_groups: Option<bool>,
    pub unresolved_catalog_references: Option<bool>,
    pub unused_dependency_overrides: Option<bool>,
    pub misconfigured_dependency_overrides: Option<bool>,
    pub files: Option<Vec<String>>,
    pub include_entry_exports: Option<bool>,
}

#[napi(object)]
#[derive(Default)]
pub struct DuplicationOptions {
    pub root: Option<String>,
    pub config_path: Option<String>,
    pub no_cache: Option<bool>,
    pub threads: Option<u32>,
    pub diff_file: Option<String>,
    pub production: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: Option<bool>,
    pub legacy_envelope: Option<bool>,
    pub mode: Option<String>,
    pub min_tokens: Option<u32>,
    pub min_lines: Option<u32>,
    /// Minimum occurrences before a clone group is reported. Must be >= 2.
    /// Defaults to 2 (current behavior).
    pub min_occurrences: Option<u32>,
    pub threshold: Option<f64>,
    pub skip_local: Option<bool>,
    pub cross_language: Option<bool>,
    pub ignore_imports: Option<bool>,
    pub top: Option<u32>,
}

#[napi(object)]
#[derive(Default)]
pub struct ComplexityOptions {
    pub root: Option<String>,
    pub config_path: Option<String>,
    pub no_cache: Option<bool>,
    pub threads: Option<u32>,
    pub diff_file: Option<String>,
    pub production: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: Option<bool>,
    pub legacy_envelope: Option<bool>,
    pub max_cyclomatic: Option<u32>,
    pub max_cognitive: Option<u32>,
    pub max_crap: Option<f64>,
    pub top: Option<u32>,
    pub sort: Option<String>,
    pub complexity: Option<bool>,
    pub file_scores: Option<bool>,
    pub coverage_gaps: Option<bool>,
    pub hotspots: Option<bool>,
    pub ownership: Option<bool>,
    pub ownership_emails: Option<String>,
    pub targets: Option<bool>,
    pub effort: Option<String>,
    pub score: Option<bool>,
    pub since: Option<String>,
    pub min_commits: Option<u32>,
    pub coverage: Option<String>,
    pub coverage_root: Option<String>,
}

struct CommonOptionsInput {
    root: Option<String>,
    config_path: Option<String>,
    no_cache: Option<bool>,
    threads: Option<u32>,
    diff_file: Option<String>,
    production: Option<bool>,
    changed_since: Option<String>,
    workspace: Option<Vec<String>>,
    changed_workspaces: Option<String>,
    explain: Option<bool>,
    legacy_envelope: Option<bool>,
}

fn map_common_options(input: CommonOptionsInput) -> napi::Result<programmatic::AnalysisOptions> {
    let threads = input
        .threads
        .map(usize::try_from)
        .transpose()
        .map_err(|_| {
            napi::Error::new(
                Status::InvalidArg,
                "`threads` does not fit into usize".to_string(),
            )
        })?;

    Ok(programmatic::AnalysisOptions {
        root: input.root.map(std::path::PathBuf::from),
        config_path: input.config_path.map(std::path::PathBuf::from),
        no_cache: input.no_cache.unwrap_or(false),
        threads,
        diff_file: input.diff_file.map(std::path::PathBuf::from),
        production: input.production.unwrap_or(false),
        production_override: input.production,
        changed_since: input.changed_since,
        workspace: input.workspace,
        changed_workspaces: input.changed_workspaces,
        explain: input.explain.unwrap_or(false),
        legacy_envelope: input.legacy_envelope.unwrap_or(false),
    })
}

fn invalid_enum_value(field: &str, value: &str, allowed: &[&str]) -> napi::Error {
    napi::Error::new(
        Status::InvalidArg,
        format!(
            "invalid `{field}` value `{value}`; expected one of: {}",
            allowed.join(", ")
        ),
    )
}

fn normalize_enum_literal(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn parse_duplication_mode(value: Option<String>) -> napi::Result<programmatic::DuplicationMode> {
    let Some(value) = value else {
        return Ok(programmatic::DuplicationMode::Mild);
    };
    match normalize_enum_literal(&value).as_str() {
        "strict" => Ok(programmatic::DuplicationMode::Strict),
        "mild" => Ok(programmatic::DuplicationMode::Mild),
        "weak" => Ok(programmatic::DuplicationMode::Weak),
        "semantic" => Ok(programmatic::DuplicationMode::Semantic),
        _ => Err(invalid_enum_value(
            "mode",
            &value,
            &["strict", "mild", "weak", "semantic"],
        )),
    }
}

fn parse_complexity_sort(value: Option<String>) -> napi::Result<programmatic::ComplexitySort> {
    let Some(value) = value else {
        return Ok(programmatic::ComplexitySort::Cyclomatic);
    };
    match normalize_enum_literal(&value).as_str() {
        "cyclomatic" => Ok(programmatic::ComplexitySort::Cyclomatic),
        "cognitive" => Ok(programmatic::ComplexitySort::Cognitive),
        "lines" => Ok(programmatic::ComplexitySort::Lines),
        "severity" => Ok(programmatic::ComplexitySort::Severity),
        _ => Err(invalid_enum_value(
            "sort",
            &value,
            &["cyclomatic", "cognitive", "lines", "severity"],
        )),
    }
}

fn parse_ownership_email_mode(
    value: Option<String>,
) -> napi::Result<Option<programmatic::OwnershipEmailMode>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match normalize_enum_literal(&value).as_str() {
        "raw" => Ok(Some(programmatic::OwnershipEmailMode::Raw)),
        "handle" => Ok(Some(programmatic::OwnershipEmailMode::Handle)),
        "anonymized" => Ok(Some(programmatic::OwnershipEmailMode::Anonymized)),
        "hash" => Ok(Some(programmatic::OwnershipEmailMode::Hash)),
        _ => Err(invalid_enum_value(
            "ownershipEmails",
            &value,
            &["raw", "handle", "anonymized", "hash"],
        )),
    }
}

fn narrow_to_u16(field: &str, value: u32) -> napi::Result<u16> {
    u16::try_from(value).map_err(|_| {
        napi::Error::new(
            Status::InvalidArg,
            format!("`{field}` must be between 0 and {}", u16::MAX),
        )
    })
}

fn parse_target_effort(value: Option<String>) -> napi::Result<Option<programmatic::TargetEffort>> {
    let Some(value) = value else {
        return Ok(None);
    };
    match normalize_enum_literal(&value).as_str() {
        "low" => Ok(Some(programmatic::TargetEffort::Low)),
        "medium" => Ok(Some(programmatic::TargetEffort::Medium)),
        "high" => Ok(Some(programmatic::TargetEffort::High)),
        _ => Err(invalid_enum_value(
            "effort",
            &value,
            &["low", "medium", "high"],
        )),
    }
}

impl TryFrom<DeadCodeOptions> for programmatic::DeadCodeOptions {
    type Error = napi::Error;

    fn try_from(value: DeadCodeOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            analysis: map_common_options(CommonOptionsInput {
                root: value.root,
                config_path: value.config_path,
                no_cache: value.no_cache,
                threads: value.threads,
                diff_file: value.diff_file,
                production: value.production,
                changed_since: value.changed_since,
                workspace: value.workspace,
                changed_workspaces: value.changed_workspaces,
                explain: value.explain,
                legacy_envelope: value.legacy_envelope,
            })?,
            filters: programmatic::DeadCodeFilters {
                unused_files: value.unused_files.unwrap_or(false),
                unused_exports: value.unused_exports.unwrap_or(false),
                unused_deps: value.unused_deps.unwrap_or(false),
                unused_types: value.unused_types.unwrap_or(false),
                private_type_leaks: value.private_type_leaks.unwrap_or(false),
                unused_enum_members: value.unused_enum_members.unwrap_or(false),
                unused_class_members: value.unused_class_members.unwrap_or(false),
                unused_store_members: value.unused_store_members.unwrap_or(false),
                unprovided_injects: value.unprovided_injects.unwrap_or(false),
                unrendered_components: value.unrendered_components.unwrap_or(false),
                unused_component_props: value.unused_component_props.unwrap_or(false),
                unused_component_emits: value.unused_component_emits.unwrap_or(false),
                unused_component_inputs: value.unused_component_inputs.unwrap_or(false),
                unused_component_outputs: value.unused_component_outputs.unwrap_or(false),
                unused_server_actions: value.unused_server_actions.unwrap_or(false),
                unused_load_data_keys: value.unused_load_data_keys.unwrap_or(false),
                unresolved_imports: value.unresolved_imports.unwrap_or(false),
                unlisted_deps: value.unlisted_deps.unwrap_or(false),
                duplicate_exports: value.duplicate_exports.unwrap_or(false),
                circular_deps: value.circular_deps.unwrap_or(false),
                re_export_cycles: value.re_export_cycles.unwrap_or(false),
                boundary_violations: value.boundary_violations.unwrap_or(false),
                policy_violations: value.policy_violations.unwrap_or(false),
                stale_suppressions: value.stale_suppressions.unwrap_or(false),
                unused_catalog_entries: value.unused_catalog_entries.unwrap_or(false),
                empty_catalog_groups: value.empty_catalog_groups.unwrap_or(false),
                unresolved_catalog_references: value.unresolved_catalog_references.unwrap_or(false),
                unused_dependency_overrides: value.unused_dependency_overrides.unwrap_or(false),
                misconfigured_dependency_overrides: value
                    .misconfigured_dependency_overrides
                    .unwrap_or(false),
            },
            files: value
                .files
                .unwrap_or_default()
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect(),
            include_entry_exports: value.include_entry_exports.unwrap_or(false),
        })
    }
}

impl TryFrom<DuplicationOptions> for programmatic::DuplicationOptions {
    type Error = napi::Error;

    fn try_from(value: DuplicationOptions) -> Result<Self, Self::Error> {
        let defaults = programmatic::DuplicationOptions::default();
        Ok(Self {
            analysis: map_common_options(CommonOptionsInput {
                root: value.root,
                config_path: value.config_path,
                no_cache: value.no_cache,
                threads: value.threads,
                diff_file: value.diff_file,
                production: value.production,
                changed_since: value.changed_since,
                workspace: value.workspace,
                changed_workspaces: value.changed_workspaces,
                explain: value.explain,
                legacy_envelope: value.legacy_envelope,
            })?,
            mode: parse_duplication_mode(value.mode)?,
            min_tokens: value.min_tokens.map_or(defaults.min_tokens, |n| n as usize),
            min_lines: value.min_lines.map_or(defaults.min_lines, |n| n as usize),
            min_occurrences: match value.min_occurrences {
                Some(n) if n < 2 => {
                    return Err(napi::Error::from_reason(format!(
                        "min_occurrences must be at least 2 (got {n})"
                    )));
                }
                Some(n) => n as usize,
                None => defaults.min_occurrences,
            },
            threshold: value.threshold.unwrap_or(defaults.threshold),
            skip_local: value.skip_local.unwrap_or(defaults.skip_local),
            cross_language: value.cross_language.unwrap_or(defaults.cross_language),
            // `None` defers to the project config (default `true`); `Some(false)`
            // forces import blocks to be counted. No `unwrap_or` so the
            // defer-to-config semantics survive (#1224).
            ignore_imports: value.ignore_imports,
            top: value.top.map(|n| n as usize),
        })
    }
}

impl TryFrom<ComplexityOptions> for programmatic::ComplexityOptions {
    type Error = napi::Error;

    fn try_from(value: ComplexityOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            analysis: map_common_options(CommonOptionsInput {
                root: value.root,
                config_path: value.config_path,
                no_cache: value.no_cache,
                threads: value.threads,
                diff_file: value.diff_file,
                production: value.production,
                changed_since: value.changed_since,
                workspace: value.workspace,
                changed_workspaces: value.changed_workspaces,
                explain: value.explain,
                legacy_envelope: value.legacy_envelope,
            })?,
            max_cyclomatic: value
                .max_cyclomatic
                .map(|n| narrow_to_u16("maxCyclomatic", n))
                .transpose()?,
            max_cognitive: value
                .max_cognitive
                .map(|n| narrow_to_u16("maxCognitive", n))
                .transpose()?,
            max_crap: value.max_crap,
            top: value.top.map(|n| n as usize),
            sort: parse_complexity_sort(value.sort)?,
            complexity: value.complexity.unwrap_or(false),
            file_scores: value.file_scores.unwrap_or(false),
            coverage_gaps: value.coverage_gaps.unwrap_or(false),
            hotspots: value.hotspots.unwrap_or(false),
            ownership: value.ownership.unwrap_or(false),
            ownership_emails: parse_ownership_email_mode(value.ownership_emails)?,
            targets: value.targets.unwrap_or(false),
            effort: parse_target_effort(value.effort)?,
            score: value.score.unwrap_or(false),
            since: value.since,
            min_commits: value.min_commits,
            coverage: value.coverage.map(std::path::PathBuf::from),
            coverage_root: value.coverage_root.map(std::path::PathBuf::from),
        })
    }
}

fn to_napi_error(env: Env, error: programmatic::ProgrammaticError) -> napi::Error {
    let programmatic::ProgrammaticError {
        message,
        exit_code,
        code,
        help,
        context,
    } = error;

    let Ok(mut js_error) = env.create_error(napi::Error::new(Status::GenericFailure, &message))
    else {
        return napi::Error::new(Status::GenericFailure, message);
    };

    let _ = js_error.set_named_property("name", "FallowNodeError");
    let _ = js_error.set_named_property("exitCode", u32::from(exit_code));
    if let Some(code) = code {
        let _ = js_error.set_named_property("code", code);
    }
    if let Some(help) = help {
        let _ = js_error.set_named_property("help", help);
    }
    if let Some(context) = context {
        let _ = js_error.set_named_property("context", context);
    }

    match js_error.into_unknown(&env) {
        Ok(js_error) => napi::Error::from(js_error),
        Err(_) => napi::Error::new(Status::GenericFailure, message),
    }
}

type ProgrammaticWork = Box<
    dyn FnOnce() -> Result<serde_json::Value, programmatic::ProgrammaticError> + Send + 'static,
>;

#[doc(hidden)]
pub struct ProgrammaticTask {
    task: Option<ProgrammaticWork>,
    error: Option<programmatic::ProgrammaticError>,
}

impl ProgrammaticTask {
    fn new<F>(task: F) -> Self
    where
        F: FnOnce() -> Result<serde_json::Value, programmatic::ProgrammaticError> + Send + 'static,
    {
        Self {
            task: Some(Box::new(task)),
            error: None,
        }
    }
}

impl<'task> ScopedTask<'task> for ProgrammaticTask {
    type Output = serde_json::Value;
    type JsValue = Unknown<'task>;

    fn compute(&mut self) -> napi::Result<Self::Output> {
        let Some(task) = self.task.take() else {
            return Err(napi::Error::new(
                Status::GenericFailure,
                "programmatic task was already consumed",
            ));
        };

        match task() {
            Ok(output) => Ok(output),
            Err(error) => {
                let message = error.message.clone();
                self.error = Some(error);
                Err(napi::Error::new(Status::GenericFailure, message))
            }
        }
    }

    fn resolve(&mut self, env: &'task Env, output: Self::Output) -> napi::Result<Self::JsValue> {
        env.to_js_value(&output)
    }

    fn reject(&mut self, env: &'task Env, err: napi::Error) -> napi::Result<Self::JsValue> {
        let error = self.error.take().unwrap_or_else(|| {
            programmatic::ProgrammaticError::new(err.reason.clone(), 2)
                .with_code("FALLOW_NODE_ERROR")
        });
        Err(to_napi_error(*env, error))
    }
}

#[napi(js_name = "detectDeadCode")]
pub fn detect_dead_code(
    options: Option<DeadCodeOptions>,
) -> napi::Result<AsyncTask<ProgrammaticTask>> {
    let options = programmatic::DeadCodeOptions::try_from(options.unwrap_or_default())?;
    Ok(AsyncTask::new(ProgrammaticTask::new(move || {
        programmatic::detect_dead_code(&options)
    })))
}

#[napi(js_name = "detectCircularDependencies")]
pub fn detect_circular_dependencies(
    options: Option<DeadCodeOptions>,
) -> napi::Result<AsyncTask<ProgrammaticTask>> {
    let options = programmatic::DeadCodeOptions::try_from(options.unwrap_or_default())?;
    Ok(AsyncTask::new(ProgrammaticTask::new(move || {
        programmatic::detect_circular_dependencies(&options)
    })))
}

#[napi(js_name = "detectBoundaryViolations")]
pub fn detect_boundary_violations(
    options: Option<DeadCodeOptions>,
) -> napi::Result<AsyncTask<ProgrammaticTask>> {
    let options = programmatic::DeadCodeOptions::try_from(options.unwrap_or_default())?;
    Ok(AsyncTask::new(ProgrammaticTask::new(move || {
        programmatic::detect_boundary_violations(&options)
    })))
}

#[napi(js_name = "detectDuplication")]
pub fn detect_duplication(
    options: Option<DuplicationOptions>,
) -> napi::Result<AsyncTask<ProgrammaticTask>> {
    let options = programmatic::DuplicationOptions::try_from(options.unwrap_or_default())?;
    Ok(AsyncTask::new(ProgrammaticTask::new(move || {
        programmatic::detect_duplication(&options)
    })))
}

#[napi(js_name = "computeComplexity")]
pub fn compute_complexity(
    options: Option<ComplexityOptions>,
) -> napi::Result<AsyncTask<ProgrammaticTask>> {
    let options = programmatic::ComplexityOptions::try_from(options.unwrap_or_default())?;
    Ok(AsyncTask::new(ProgrammaticTask::new(move || {
        programmatic::compute_complexity(&options)
    })))
}

#[napi(js_name = "computeHealth")]
pub fn compute_health(
    options: Option<ComplexityOptions>,
) -> napi::Result<AsyncTask<ProgrammaticTask>> {
    let options = programmatic::ComplexityOptions::try_from(options.unwrap_or_default())?;
    Ok(AsyncTask::new(ProgrammaticTask::new(move || {
        programmatic::compute_health(&options)
    })))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn error_reason<T>(result: napi::Result<T>) -> String {
        match result {
            Ok(_) => panic!("option validation should fail"),
            Err(error) => error.reason.clone(),
        }
    }

    #[test]
    fn dead_code_options_map_common_fields_filters_and_files() {
        let options = programmatic::DeadCodeOptions::try_from(DeadCodeOptions {
            root: Some("/repo".to_string()),
            config_path: Some("/repo/fallow.toml".to_string()),
            no_cache: Some(true),
            threads: Some(4),
            diff_file: Some("/tmp/diff.patch".to_string()),
            production: Some(true),
            changed_since: Some("origin/main".to_string()),
            workspace: Some(vec!["apps/web".to_string()]),
            changed_workspaces: None,
            explain: Some(true),
            legacy_envelope: Some(true),
            unused_files: Some(true),
            unused_exports: Some(true),
            unused_deps: Some(true),
            unused_types: Some(true),
            private_type_leaks: Some(true),
            unused_enum_members: Some(true),
            unused_class_members: Some(true),
            unused_store_members: Some(true),
            unprovided_injects: Some(true),
            unrendered_components: Some(true),
            unused_component_props: Some(true),
            unused_component_emits: Some(true),
            unused_component_inputs: Some(true),
            unused_component_outputs: Some(true),
            unused_server_actions: Some(true),
            unused_load_data_keys: Some(true),
            unresolved_imports: Some(true),
            unlisted_deps: Some(true),
            duplicate_exports: Some(true),
            circular_deps: Some(true),
            re_export_cycles: Some(true),
            boundary_violations: Some(true),
            policy_violations: Some(true),
            stale_suppressions: Some(true),
            unused_catalog_entries: Some(true),
            empty_catalog_groups: Some(true),
            unresolved_catalog_references: Some(true),
            unused_dependency_overrides: Some(true),
            misconfigured_dependency_overrides: Some(true),
            files: Some(vec!["src/app.ts".to_string(), "src/lib.ts".to_string()]),
            include_entry_exports: Some(true),
        })
        .expect("options should map");

        assert_eq!(options.analysis.root.as_deref(), Some(Path::new("/repo")));
        assert_eq!(
            options.analysis.config_path.as_deref(),
            Some(Path::new("/repo/fallow.toml"))
        );
        assert!(options.analysis.no_cache);
        assert_eq!(options.analysis.threads, Some(4));
        assert_eq!(
            options.analysis.diff_file.as_deref(),
            Some(Path::new("/tmp/diff.patch"))
        );
        assert!(options.analysis.production);
        assert_eq!(options.analysis.production_override, Some(true));
        assert_eq!(
            options.analysis.changed_since.as_deref(),
            Some("origin/main")
        );
        assert_eq!(
            options.analysis.workspace,
            Some(vec!["apps/web".to_string()])
        );
        assert!(options.analysis.explain);
        assert!(options.analysis.legacy_envelope);
        assert!(options.filters.unused_files);
        assert!(options.filters.unused_exports);
        assert!(options.filters.unused_deps);
        assert!(options.filters.unused_types);
        assert!(options.filters.private_type_leaks);
        assert!(options.filters.unused_enum_members);
        assert!(options.filters.unused_class_members);
        assert!(options.filters.unused_store_members);
        assert!(options.filters.unresolved_imports);
        assert!(options.filters.unlisted_deps);
        assert!(options.filters.duplicate_exports);
        assert!(options.filters.circular_deps);
        assert!(options.filters.re_export_cycles);
        assert!(options.filters.boundary_violations);
        assert!(options.filters.stale_suppressions);
        assert!(options.filters.unused_catalog_entries);
        assert!(options.filters.empty_catalog_groups);
        assert!(options.filters.unresolved_catalog_references);
        assert!(options.filters.unused_dependency_overrides);
        assert!(options.filters.misconfigured_dependency_overrides);
        assert_eq!(
            options.files,
            vec![Path::new("src/app.ts"), Path::new("src/lib.ts")]
        );
        assert!(options.include_entry_exports);
    }

    #[test]
    fn omitted_production_option_defers_to_config() {
        let options = programmatic::DeadCodeOptions::try_from(DeadCodeOptions::default())
            .expect("options should map");

        assert_eq!(options.analysis.production_override, None);
    }

    #[test]
    fn explicit_production_false_is_forwarded_as_override() {
        let options = programmatic::DeadCodeOptions::try_from(DeadCodeOptions {
            production: Some(false),
            ..DeadCodeOptions::default()
        })
        .expect("options should map");

        assert_eq!(options.analysis.production_override, Some(false));
    }
    #[test]
    fn detect_duplication_accepts_normalized_mode() {
        let task = detect_duplication(Some(DuplicationOptions {
            mode: Some(" STRICT ".to_string()),
            ..DuplicationOptions::default()
        }));

        assert!(task.is_ok());
    }

    #[test]
    fn detect_duplication_rejects_unknown_mode() {
        let reason = error_reason(detect_duplication(Some(DuplicationOptions {
            mode: Some("strictest".to_string()),
            ..DuplicationOptions::default()
        })));

        assert_eq!(
            reason,
            "invalid `mode` value `strictest`; expected one of: strict, mild, weak, semantic"
        );
    }

    #[test]
    fn detect_duplication_rejects_single_min_occurrence() {
        let reason = error_reason(detect_duplication(Some(DuplicationOptions {
            min_occurrences: Some(1),
            ..DuplicationOptions::default()
        })));

        assert_eq!(reason, "min_occurrences must be at least 2 (got 1)");
    }

    #[test]
    fn compute_complexity_accepts_normalized_enum_options() {
        let task = compute_complexity(Some(ComplexityOptions {
            sort: Some(" LINES ".to_string()),
            ownership_emails: Some("HASH".to_string()),
            effort: Some("Medium".to_string()),
            ..ComplexityOptions::default()
        }));

        assert!(task.is_ok());
    }

    #[test]
    fn compute_complexity_rejects_unknown_sort() {
        let reason = error_reason(compute_complexity(Some(ComplexityOptions {
            sort: Some("risk".to_string()),
            ..ComplexityOptions::default()
        })));

        assert_eq!(
            reason,
            "invalid `sort` value `risk`; expected one of: cyclomatic, cognitive, lines, severity"
        );
    }

    #[test]
    fn compute_complexity_rejects_unknown_ownership_email_mode() {
        let reason = error_reason(compute_complexity(Some(ComplexityOptions {
            ownership_emails: Some("masked".to_string()),
            ..ComplexityOptions::default()
        })));

        assert_eq!(
            reason,
            "invalid `ownershipEmails` value `masked`; expected one of: raw, handle, anonymized, hash"
        );
    }

    #[test]
    fn compute_complexity_rejects_unknown_target_effort() {
        let reason = error_reason(compute_complexity(Some(ComplexityOptions {
            effort: Some("tiny".to_string()),
            ..ComplexityOptions::default()
        })));

        assert_eq!(
            reason,
            "invalid `effort` value `tiny`; expected one of: low, medium, high"
        );
    }

    #[test]
    fn compute_complexity_rejects_out_of_range_u16_options() {
        let reason = error_reason(compute_complexity(Some(ComplexityOptions {
            max_cyclomatic: Some(u32::from(u16::MAX) + 1),
            ..ComplexityOptions::default()
        })));

        assert_eq!(reason, "`maxCyclomatic` must be between 0 and 65535");
    }

    #[test]
    fn duplication_options_map_modes_thresholds_and_flags() {
        let options = programmatic::DuplicationOptions::try_from(DuplicationOptions {
            mode: Some(" SEMANTIC ".to_string()),
            min_tokens: Some(30),
            min_lines: Some(4),
            min_occurrences: Some(3),
            threshold: Some(2.5),
            skip_local: Some(true),
            cross_language: Some(true),
            ignore_imports: Some(true),
            top: Some(7),
            ..DuplicationOptions::default()
        })
        .expect("options should map");

        assert!(matches!(
            options.mode,
            programmatic::DuplicationMode::Semantic
        ));
        assert_eq!(options.min_tokens, 30);
        assert_eq!(options.min_lines, 4);
        assert_eq!(options.min_occurrences, 3);
        assert!((options.threshold - 2.5).abs() < f64::EPSILON);
        assert!(options.skip_local);
        assert!(options.cross_language);
        assert_eq!(options.ignore_imports, Some(true));
        assert_eq!(options.top, Some(7));
    }

    #[test]
    fn duplication_options_reject_invalid_mode_and_min_occurrences() {
        let invalid_mode = programmatic::DuplicationOptions::try_from(DuplicationOptions {
            mode: Some("exact".to_string()),
            ..DuplicationOptions::default()
        })
        .expect_err("invalid mode should fail");

        assert_eq!(invalid_mode.status, Status::InvalidArg);
        assert!(invalid_mode.reason.contains("invalid `mode` value `exact`"));

        let too_few_occurrences = programmatic::DuplicationOptions::try_from(DuplicationOptions {
            min_occurrences: Some(1),
            ..DuplicationOptions::default()
        })
        .expect_err("single occurrence should fail");

        assert!(
            too_few_occurrences
                .reason
                .contains("min_occurrences must be at least 2")
        );
    }

    #[test]
    fn complexity_options_map_sections_sort_ownership_effort_and_coverage() {
        let options = programmatic::ComplexityOptions::try_from(ComplexityOptions {
            max_cyclomatic: Some(42),
            max_cognitive: Some(21),
            max_crap: Some(18.5),
            top: Some(5),
            sort: Some(" Severity ".to_string()),
            complexity: Some(true),
            file_scores: Some(true),
            coverage_gaps: Some(true),
            hotspots: Some(true),
            ownership: Some(true),
            ownership_emails: Some("hash".to_string()),
            targets: Some(true),
            effort: Some("HIGH".to_string()),
            score: Some(true),
            since: Some("90d".to_string()),
            min_commits: Some(3),
            coverage: Some("coverage/coverage-final.json".to_string()),
            coverage_root: Some("/ci/workspace".to_string()),
            ..ComplexityOptions::default()
        })
        .expect("options should map");

        assert_eq!(options.max_cyclomatic, Some(42));
        assert_eq!(options.max_cognitive, Some(21));
        assert_eq!(options.max_crap, Some(18.5));
        assert_eq!(options.top, Some(5));
        assert!(matches!(
            options.sort,
            programmatic::ComplexitySort::Severity
        ));
        assert!(options.complexity);
        assert!(options.file_scores);
        assert!(options.coverage_gaps);
        assert!(options.hotspots);
        assert!(options.ownership);
        assert!(matches!(
            options.ownership_emails,
            Some(programmatic::OwnershipEmailMode::Hash)
        ));
        assert!(options.targets);
        assert!(matches!(
            options.effort,
            Some(programmatic::TargetEffort::High)
        ));
        assert!(options.score);
        assert_eq!(options.since.as_deref(), Some("90d"));
        assert_eq!(options.min_commits, Some(3));
        assert_eq!(
            options.coverage.as_deref(),
            Some(Path::new("coverage/coverage-final.json"))
        );
        assert_eq!(
            options.coverage_root.as_deref(),
            Some(Path::new("/ci/workspace"))
        );
    }

    #[test]
    fn complexity_options_reject_invalid_values_and_out_of_range_thresholds() {
        let invalid_sort = programmatic::ComplexityOptions::try_from(ComplexityOptions {
            sort: Some("weighted".to_string()),
            ..ComplexityOptions::default()
        })
        .expect_err("invalid sort should fail");

        assert_eq!(invalid_sort.status, Status::InvalidArg);
        assert!(
            invalid_sort
                .reason
                .contains("invalid `sort` value `weighted`")
        );

        let invalid_ownership = programmatic::ComplexityOptions::try_from(ComplexityOptions {
            ownership_emails: Some("cleartext".to_string()),
            ..ComplexityOptions::default()
        })
        .expect_err("invalid ownership email mode should fail");

        assert!(
            invalid_ownership
                .reason
                .contains("invalid `ownershipEmails` value `cleartext`")
        );

        let invalid_effort = programmatic::ComplexityOptions::try_from(ComplexityOptions {
            effort: Some("tiny".to_string()),
            ..ComplexityOptions::default()
        })
        .expect_err("invalid effort should fail");

        assert!(
            invalid_effort
                .reason
                .contains("invalid `effort` value `tiny`")
        );

        let invalid_threshold = programmatic::ComplexityOptions::try_from(ComplexityOptions {
            max_cyclomatic: Some(u32::from(u16::MAX) + 1),
            ..ComplexityOptions::default()
        })
        .expect_err("threshold above u16 should fail");

        assert!(
            invalid_threshold
                .reason
                .contains("`maxCyclomatic` must be between 0")
        );
    }

    #[test]
    fn programmatic_task_runs_once_and_preserves_compute_errors() {
        let mut task = ProgrammaticTask::new(|| Ok(serde_json::json!({ "ok": true })));

        let output = task.compute().expect("task should succeed");
        assert_eq!(output["ok"], true);

        let consumed = task.compute().expect_err("task should only run once");
        assert!(consumed.reason.contains("already consumed"));

        let mut failing_task = ProgrammaticTask::new(|| {
            Err(programmatic::ProgrammaticError::new("analysis failed", 2)
                .with_code("FALLOW_TEST_FAILURE"))
        });

        let error = failing_task.compute().expect_err("task should fail");
        assert_eq!(error.reason, "analysis failed");
        let stored = failing_task
            .error
            .as_ref()
            .expect("programmatic error should be retained for reject");
        assert_eq!(stored.code.as_deref(), Some("FALLOW_TEST_FAILURE"));
    }
}
