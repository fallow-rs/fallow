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
    pub production: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: Option<bool>,
    pub unused_files: Option<bool>,
    pub unused_exports: Option<bool>,
    pub unused_deps: Option<bool>,
    pub unused_types: Option<bool>,
    pub private_type_leaks: Option<bool>,
    pub unused_enum_members: Option<bool>,
    pub unused_class_members: Option<bool>,
    pub unresolved_imports: Option<bool>,
    pub unlisted_deps: Option<bool>,
    pub duplicate_exports: Option<bool>,
    pub circular_deps: Option<bool>,
    pub boundary_violations: Option<bool>,
    pub stale_suppressions: Option<bool>,
    pub unused_catalog_entries: Option<bool>,
    pub unresolved_catalog_references: Option<bool>,
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
    pub production: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: Option<bool>,
    pub mode: Option<String>,
    pub min_tokens: Option<u32>,
    pub min_lines: Option<u32>,
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
    pub production: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: Option<bool>,
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

#[expect(
    clippy::too_many_arguments,
    reason = "maps the shared analysis fields across multiple NAPI option objects"
)]
fn map_common_options(
    root: Option<String>,
    config_path: Option<String>,
    no_cache: Option<bool>,
    threads: Option<u32>,
    production: Option<bool>,
    changed_since: Option<String>,
    workspace: Option<Vec<String>>,
    changed_workspaces: Option<String>,
    explain: Option<bool>,
) -> napi::Result<programmatic::AnalysisOptions> {
    let threads = threads.map(usize::try_from).transpose().map_err(|_| {
        napi::Error::new(
            Status::InvalidArg,
            "`threads` does not fit into usize".to_string(),
        )
    })?;

    Ok(programmatic::AnalysisOptions {
        root: root.map(std::path::PathBuf::from),
        config_path: config_path.map(std::path::PathBuf::from),
        no_cache: no_cache.unwrap_or(false),
        threads,
        production: production.unwrap_or(false),
        production_override: production,
        changed_since,
        workspace,
        changed_workspaces,
        explain: explain.unwrap_or(false),
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
        "hash" => Ok(Some(programmatic::OwnershipEmailMode::Hash)),
        _ => Err(invalid_enum_value(
            "ownershipEmails",
            &value,
            &["raw", "handle", "hash"],
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
            analysis: map_common_options(
                value.root,
                value.config_path,
                value.no_cache,
                value.threads,
                value.production,
                value.changed_since,
                value.workspace,
                value.changed_workspaces,
                value.explain,
            )?,
            filters: programmatic::DeadCodeFilters {
                unused_files: value.unused_files.unwrap_or(false),
                unused_exports: value.unused_exports.unwrap_or(false),
                unused_deps: value.unused_deps.unwrap_or(false),
                unused_types: value.unused_types.unwrap_or(false),
                private_type_leaks: value.private_type_leaks.unwrap_or(false),
                unused_enum_members: value.unused_enum_members.unwrap_or(false),
                unused_class_members: value.unused_class_members.unwrap_or(false),
                unresolved_imports: value.unresolved_imports.unwrap_or(false),
                unlisted_deps: value.unlisted_deps.unwrap_or(false),
                duplicate_exports: value.duplicate_exports.unwrap_or(false),
                circular_deps: value.circular_deps.unwrap_or(false),
                boundary_violations: value.boundary_violations.unwrap_or(false),
                stale_suppressions: value.stale_suppressions.unwrap_or(false),
                unused_catalog_entries: value.unused_catalog_entries.unwrap_or(false),
                unresolved_catalog_references: value.unresolved_catalog_references.unwrap_or(false),
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
            analysis: map_common_options(
                value.root,
                value.config_path,
                value.no_cache,
                value.threads,
                value.production,
                value.changed_since,
                value.workspace,
                value.changed_workspaces,
                value.explain,
            )?,
            mode: parse_duplication_mode(value.mode)?,
            min_tokens: value.min_tokens.map_or(defaults.min_tokens, |n| n as usize),
            min_lines: value.min_lines.map_or(defaults.min_lines, |n| n as usize),
            threshold: value.threshold.unwrap_or(defaults.threshold),
            skip_local: value.skip_local.unwrap_or(defaults.skip_local),
            cross_language: value.cross_language.unwrap_or(defaults.cross_language),
            ignore_imports: value.ignore_imports.unwrap_or(defaults.ignore_imports),
            top: value.top.map(|n| n as usize),
        })
    }
}

impl TryFrom<ComplexityOptions> for programmatic::ComplexityOptions {
    type Error = napi::Error;

    fn try_from(value: ComplexityOptions) -> Result<Self, Self::Error> {
        Ok(Self {
            analysis: map_common_options(
                value.root,
                value.config_path,
                value.no_cache,
                value.threads,
                value.production,
                value.changed_since,
                value.workspace,
                value.changed_workspaces,
                value.explain,
            )?,
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
    use super::*;

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
}
