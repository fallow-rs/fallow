use std::path::{Path, PathBuf};

use fallow_api::{
    EditorAnalysisOutput, EditorAnalysisResults as AnalysisResults,
    EditorAnalysisSession as AnalysisSession, EditorDuplicationReport as DuplicationReport,
    EditorInlineComplexityFinding as InlineComplexityFinding,
};
use fallow_config::DuplicatesConfig;
use ls_types::MessageType;
use rustc_hash::FxHashSet;

use crate::initialization::LspDuplicationOptions;
use crate::protocol::config_load_error_detail;

/// Run dead-code + duplicates analysis for a single project root, appending
/// findings to the merged accumulators and a status message to
/// `config_messages`. Extracted out of `run_analysis` to keep that method
/// under the 150-line clippy ceiling.
pub struct ProjectRootAnalysisInput<'a> {
    pub project_root: &'a Path,
    pub config_path: Option<&'a Path>,
    pub allow_remote_extends: bool,
    pub duplication_options: Option<&'a LspDuplicationOptions>,
    pub production_override: Option<bool>,
    pub inline_complexity_enabled: bool,
    pub changed_files: Option<&'a FxHashSet<PathBuf>>,
    pub merged_analysis: &'a mut EditorAnalysisOutput,
    pub merged_inline_complexity: &'a mut Vec<InlineComplexityFinding>,
    pub config_messages: &'a mut Vec<(MessageType, String)>,
}

pub struct BlockingAnalysisInput {
    pub project_roots: Vec<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub allow_remote_extends: bool,
    pub duplication_options: Option<LspDuplicationOptions>,
    pub production_override: Option<bool>,
    pub inline_complexity_enabled: bool,
    pub root: PathBuf,
    pub toplevel: Option<PathBuf>,
    pub changed_since: Option<String>,
}

pub struct BlockingAnalysisOutput {
    pub analysis: EditorAnalysisOutput,
    pub inline_complexity: Vec<InlineComplexityFinding>,
    pub config_messages: Vec<(MessageType, String)>,
    pub changed_message: Option<(MessageType, String)>,
    pub applied_changed_since: Option<String>,
}

#[derive(Debug)]
pub struct ProjectAnalysisError {
    project_root: PathBuf,
    message: String,
}

impl std::fmt::Display for ProjectAnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "project analysis failed for {}: {}",
            self.project_root.display(),
            self.message
        )
    }
}

impl std::error::Error for ProjectAnalysisError {}

pub struct LspAnalysisSnapshot {
    pub results: AnalysisResults,
    pub duplication: DuplicationReport,
    pub inline_complexity: Vec<InlineComplexityFinding>,
}

impl LspAnalysisSnapshot {
    pub fn new(
        results: AnalysisResults,
        duplication: DuplicationReport,
        inline_complexity: Vec<InlineComplexityFinding>,
    ) -> Self {
        Self {
            results,
            duplication,
            inline_complexity,
        }
    }
}

pub fn analyze_project_root(
    input: &mut ProjectRootAnalysisInput<'_>,
) -> Result<(), ProjectAnalysisError> {
    let session = match AnalysisSession::load_with_config_options(
        input.project_root,
        input.config_path,
        fallow_config::ConfigLoadOptions {
            allow_remote_extends: input.allow_remote_extends,
        },
        |config| {
            // Override the project config's production resolution when the
            // editor forwarded an explicit `fallow.production` (on/off).
            // Mirrors the CLI-driven sidebar receiving
            // `--production`/`--no-production`, so the two surfaces agree;
            // `None` leaves the project config in force (issue #1055).
            if let Some(production) = input.production_override {
                config.production = production;
            }
        },
    ) {
        Ok(session) => session,
        Err(e) => {
            return analyze_project_root_config_fallback(input, &e);
        }
    };

    let message = (
        MessageType::INFO,
        session.config_path().map_or_else(
            || {
                format!(
                    "no config file found for {}, using defaults",
                    input.project_root.display()
                )
            },
            |path| format!("loaded config: {}", path.display()),
        ),
    );

    input.config_messages.push(message);

    let duplicates_config = input.duplication_options.map_or_else(
        || session.config().duplicates.clone(),
        |options| options.merge_with(&session.config().duplicates),
    );
    run_typed_project_analysis(input, &session, &duplicates_config)
}

/// Config-load failure path: record the warning, and when no explicit config
/// path was given, fall back to the path-based analysis + default duplication
/// scan so the editor still surfaces findings.
fn analyze_project_root_config_fallback(
    input: &mut ProjectRootAnalysisInput<'_>,
    err: &impl std::fmt::Display,
) -> Result<(), ProjectAnalysisError> {
    let detail = config_load_error_detail(input.project_root, input.config_path, err);
    if input.config_path.is_some() {
        return Err(ProjectAnalysisError {
            project_root: input.project_root.to_path_buf(),
            message: detail,
        });
    }
    input.config_messages.push((MessageType::WARNING, detail));
    let session = AnalysisSession::load_default(input.project_root);
    run_typed_project_analysis(input, &session, &DuplicatesConfig::default())
}

/// Run typed project analysis for a loaded config, with the optional
/// inline-complexity artifact retention when the client opted in, folding
/// results into the accumulators.
fn run_typed_project_analysis(
    input: &mut ProjectRootAnalysisInput<'_>,
    session: &AnalysisSession,
    duplicates_config: &DuplicatesConfig,
) -> Result<(), ProjectAnalysisError> {
    let output = session
        .analyze_project_with_changed_files(
            duplicates_config,
            input.inline_complexity_enabled,
            input.changed_files,
        )
        .map_err(|error| ProjectAnalysisError {
            project_root: input.project_root.to_path_buf(),
            message: error.to_string(),
        })?;
    if input.inline_complexity_enabled {
        input
            .merged_inline_complexity
            .extend(fallow_api::collect_inline_complexity(
                session.config(),
                &output.dead_code,
            ));
    }
    input.merged_analysis.merge_project_output(output);
    Ok(())
}

pub fn run_blocking_analysis(
    input: &BlockingAnalysisInput,
) -> Result<BlockingAnalysisOutput, ProjectAnalysisError> {
    let mut analysis = EditorAnalysisOutput::default();
    let mut inline_complexity = Vec::new();
    let mut config_messages: Vec<(MessageType, String)> =
        Vec::with_capacity(input.project_roots.len());
    let changed_scope = resolve_changed_since_scope(
        input.changed_since.as_deref(),
        input.toplevel.as_deref().unwrap_or(input.root.as_path()),
        &input.root,
    );
    for project_root in &input.project_roots {
        analyze_project_root(&mut ProjectRootAnalysisInput {
            project_root,
            config_path: input.config_path.as_deref(),
            allow_remote_extends: input.allow_remote_extends,
            duplication_options: input.duplication_options.as_ref(),
            production_override: input.production_override,
            inline_complexity_enabled: input.inline_complexity_enabled,
            changed_files: changed_scope.files.as_ref(),
            merged_analysis: &mut analysis,
            merged_inline_complexity: &mut inline_complexity,
            config_messages: &mut config_messages,
        })?;
    }

    if let Some(changed_files) = changed_scope.files.as_ref() {
        analysis.filter_by_changed_files(changed_files, &input.root);
        fallow_api::filter_inline_complexity_by_changed_files(
            &mut inline_complexity,
            changed_files,
        );
    }

    Ok(BlockingAnalysisOutput {
        analysis,
        inline_complexity,
        config_messages,
        changed_message: changed_scope.message,
        applied_changed_since: changed_scope.applied_ref,
    })
}

/// Test helper over the editor API accumulator.
#[cfg(test)]
pub fn merge_results(target: &mut AnalysisResults, source: AnalysisResults) {
    let mut output =
        EditorAnalysisOutput::new(std::mem::take(target), DuplicationReport::default());
    output.merge_results(source);
    *target = output.results;
}

/// Test helper over the editor API accumulator.
#[cfg(test)]
pub fn merge_duplication(target: &mut DuplicationReport, source: DuplicationReport) {
    let mut output = EditorAnalysisOutput::new(AnalysisResults::default(), std::mem::take(target));
    output.merge_duplication(source);
    *target = output.duplication;
}

struct ChangedSinceScope {
    files: Option<FxHashSet<PathBuf>>,
    message: Option<(MessageType, String)>,
    applied_ref: Option<String>,
}

fn resolve_changed_since_scope(
    changed_since: Option<&str>,
    toplevel: &Path,
    root: &Path,
) -> ChangedSinceScope {
    let Some(git_ref) = changed_since else {
        return ChangedSinceScope {
            files: None,
            message: None,
            applied_ref: None,
        };
    };

    match fallow_api::try_get_changed_files_with_toplevel(root, toplevel, git_ref) {
        Ok(changed) => {
            let count = changed.len();
            ChangedSinceScope {
                files: Some(changed),
                applied_ref: Some(git_ref.to_string()),
                message: Some((
                    MessageType::INFO,
                    format!("changedSince '{git_ref}': scoped to {count} changed file(s)"),
                )),
            }
        }
        Err(err) => ChangedSinceScope {
            files: None,
            applied_ref: None,
            message: Some((
                MessageType::WARNING,
                format!(
                    "changedSince '{git_ref}' ignored: {} (showing full-scope results)",
                    err.describe()
                ),
            )),
        },
    }
}
