//! `fallow mutation tasks`: the open tasks of a saved report as one batch.
//! A task is a `survived` or `no_coverage` mutant with the source lines
//! before and after the mutation. It reads files and instruments source text
//! in memory. It never executes code.
//!
//! The source lines come from the current file, with the freshness rule of
//! `inspect`: a file without the digest of the report gives a stale task
//! without code.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use fallow_output::{
    MutantStatus, MutationTask, MutationTaskWitness, MutationTasksOutput,
    MutationTasksSchemaVersion, MutationTasksSummary,
};
use fallow_types::envelope::ToolVersion;

use crate::mutation_human::EQUIVALENCE_HINT_TEXT;
use crate::mutation_inspect::{
    SavedMutant, SavedReport, SavedWitness, mutant_code, report_input_error, report_relative_path,
    sha256_hex,
};

/// The current bytes of one file and their digest. `None` for a file that
/// cannot be read.
type CurrentFile = Option<(Vec<u8>, String)>;

/// Builds the open tasks of `report` against the files below `root`.
///
/// # Errors
///
/// Returns an error for a plan document, which has no test results, and for
/// a mutant path that leaves the project root.
pub fn tasks(
    report: &SavedReport,
    root: &Path,
    witness_values: &dyn Fn(&str, &str) -> Option<WitnessValues>,
) -> Result<MutationTasksOutput, fallow_api::ProgrammaticError> {
    if report.kind.as_deref() == Some("mutation-plan") {
        return Err(report_input_error(
            "the file is a mutation plan, which has no test results; pass a saved `fallow mutation --format json` report",
        ));
    }
    let mut files: BTreeMap<&str, CurrentFile> = BTreeMap::new();
    let mut tasks = Vec::new();
    for mutant in report.mutants.iter().filter(|mutant| is_open(mutant)) {
        let relative = report_relative_path(&mutant.file)?;
        let current = files.entry(mutant.file.as_str()).or_insert_with(|| {
            std::fs::read(root.join(relative)).ok().map(|bytes| {
                let digest = sha256_hex(&bytes);
                (bytes, digest)
            })
        });
        let mut task = task(report, mutant, current.as_ref());
        // A stale task describes older code, so it gets no cached values.
        let fresh = task.stale_reason.is_none();
        task.witness = mutant.witness.as_ref().map(|witness| {
            task_witness(&mutant.mutant_id, witness, &|mutant_id, input_id| {
                fresh.then(|| witness_values(mutant_id, input_id)).flatten()
            })
        });
        tasks.push(task);
    }
    tasks.sort_by(|left, right| {
        (&left.file, left.line, left.column, &left.mutant_id).cmp(&(
            &right.file,
            right.line,
            right.column,
            &right.mutant_id,
        ))
    });
    Ok(MutationTasksOutput {
        schema_version: MutationTasksSchemaVersion::V1,
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_owned()),
        report_completion: report
            .completion
            .as_ref()
            .map(|completion| completion.status),
        summary: summary(&tasks),
        tasks,
    })
}

/// The cached values of a witness: the arguments and the result of each side.
pub type WitnessValues = (
    Vec<serde_json::Value>,
    fallow_output::MutationWitnessSide,
    fallow_output::MutationWitnessSide,
);

/// The witness of a task: the reference of the report, the replay command,
/// and the values of a witness when the local cache still has them.
fn task_witness(
    mutant_id: &str,
    witness: &SavedWitness,
    witness_values: &dyn Fn(&str, &str) -> Option<WitnessValues>,
) -> MutationTaskWitness {
    let values = (witness.verdict == fallow_output::MutationReplayVerdict::Witness)
        .then_some(witness.input_id.as_deref())
        .flatten()
        .and_then(|input_id| witness_values(mutant_id, input_id));
    let (args, original, mutant) = match values {
        Some((args, original, mutant)) => (Some(args), Some(original), Some(mutant)),
        None => (None, None, None),
    };
    MutationTaskWitness {
        verdict: witness.verdict,
        input_id: witness.input_id.clone(),
        inputs_checked: witness.inputs_checked,
        reason: witness.reason.clone(),
        replay_command: format!("fallow mutation inspect {mutant_id} --replay"),
        args,
        original,
        mutant,
    }
}

fn is_open(mutant: &SavedMutant) -> bool {
    matches!(
        mutant.status,
        Some(MutantStatus::Survived | MutantStatus::NoCoverage)
    )
}

fn task(
    report: &SavedReport,
    mutant: &SavedMutant,
    current: Option<&(Vec<u8>, String)>,
) -> MutationTask {
    let saved_file = report.saved_file(&mutant.file);
    let code = mutant_code(
        current.map(|(bytes, _)| bytes.as_slice()),
        current.map(|(_, digest)| digest.as_str()),
        saved_file.map(|file| file.source_sha256.as_str()),
        mutant,
    );
    let static_test_files = saved_file.map_or(&[][..], |file| file.static_test_files.as_slice());
    let covering_test_files = covering_test_files(&mutant.covering_tests, static_test_files);
    let (original_lines, mutated_lines, stale_reason) = match code {
        Ok(code) => (Some(code.original_lines), Some(code.mutated_lines), None),
        Err(reason) => (None, None, Some(reason)),
    };
    MutationTask {
        mutant_id: mutant.mutant_id.clone(),
        file: mutant.file.clone(),
        line: mutant.line,
        column: mutant.column,
        operator: mutant.operator.clone(),
        function: mutant.function.clone(),
        status: mutant.status.unwrap_or(MutantStatus::Survived),
        original_lines,
        mutated_lines,
        equivalence_hint: mutant.equivalence_hint,
        stale_reason,
        covering_test_files,
        witness: None,
    }
}

/// The test files of the covering test ids, sorted and unique. A test id is
/// `<file>::<name>`, or `<project>::<file>::<name>` in a Vitest workspace
/// with more than one project. A static test file that the id names wins;
/// otherwise the file is the first `::` part with a JavaScript or
/// TypeScript extension. An id without such a part gives no file.
fn covering_test_files(test_ids: &[String], static_test_files: &[String]) -> Vec<String> {
    let mut files: Vec<String> = test_ids
        .iter()
        .filter_map(|id| test_file_of(id, static_test_files))
        .collect();
    files.sort();
    files.dedup();
    files
}

/// The test file of one test id: a static test file that the id names, or
/// else the first `::` part with a JavaScript or TypeScript extension.
pub fn test_file_of(id: &str, static_test_files: &[String]) -> Option<String> {
    static_test_files
        .iter()
        .find(|file| id.starts_with(&format!("{file}::")) || id.contains(&format!("::{file}::")))
        .cloned()
        .or_else(|| {
            id.split("::")
                .find(|part| is_script_path(part))
                .map(str::to_owned)
        })
}

/// The extensions of a test file that a supported runner loads.
const SCRIPT_EXTENSIONS: &[&str] = &[
    "js", "jsx", "ts", "tsx", "mjs", "cjs", "mts", "cts", "vue", "svelte",
];

fn is_script_path(part: &str) -> bool {
    Path::new(part)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension))
}

fn summary(tasks: &[MutationTask]) -> MutationTasksSummary {
    let count = |status: MutantStatus| tasks.iter().filter(|task| task.status == status).count();
    MutationTasksSummary {
        tasks: tasks.len(),
        survived: count(MutantStatus::Survived),
        no_coverage: count(MutantStatus::NoCoverage),
        stale: tasks
            .iter()
            .filter(|task| task.stale_reason.is_some())
            .count(),
    }
}

const HEADER: &str = "Each item is a small change to the code that made no test fail. Write tests that pass on the current code and fail on the changed code. One test can cover several items. Skip an item when no input can detect the change; a verdict document (`--verdicts`) can mark it as equivalent.";

/// The plain-text batch: a short header, then one numbered item per task.
/// An agent can paste it into its context as it is.
pub fn render_text(output: &MutationTasksOutput) -> String {
    let mut text = String::new();
    if output.tasks.is_empty() {
        text.push_str("No open mutation tasks: no mutant survived and each mutant has coverage.\n");
        push_partial_note(&mut text, output);
        return text;
    }
    let _ = writeln!(text, "Mutation tasks: {}\n", counts_line(&output.summary));
    let _ = writeln!(text, "{HEADER}");
    push_partial_note(&mut text, output);
    for (index, task) in output.tasks.iter().enumerate() {
        let _ = writeln!(text, "\n{}. {}", index + 1, item_title(task, false));
        let body = item_body(task);
        for line in body.code.iter().chain(&body.notes) {
            let _ = writeln!(text, "   {line}");
        }
    }
    text
}

/// The Markdown batch: the same items, with the source lines in a fenced
/// block.
pub fn render_markdown(output: &MutationTasksOutput) -> String {
    let mut text = String::from("## Mutation tasks\n\n");
    if output.tasks.is_empty() {
        text.push_str("No open mutation tasks: no mutant survived and each mutant has coverage.\n");
        push_partial_note(&mut text, output);
        return text;
    }
    let _ = writeln!(text, "{}\n", counts_line(&output.summary));
    let _ = writeln!(text, "{HEADER}");
    push_partial_note(&mut text, output);
    for (index, task) in output.tasks.iter().enumerate() {
        let _ = writeln!(text, "\n{}. {}", index + 1, item_title(task, true));
        let body = item_body(task);
        if !body.code.is_empty() {
            let fence = fence_for(&body.code);
            let _ = writeln!(text, "\n   {fence}text");
            for line in &body.code {
                let _ = writeln!(text, "   {line}");
            }
            let _ = writeln!(text, "   {fence}");
        }
        if !body.notes.is_empty() {
            text.push('\n');
            for line in &body.notes {
                let _ = writeln!(text, "   {line}");
            }
        }
    }
    text
}

fn counts_line(summary: &MutationTasksSummary) -> String {
    let mut parts = Vec::new();
    if summary.survived > 0 {
        parts.push(format!("{} survived", summary.survived));
    }
    if summary.no_coverage > 0 {
        parts.push(format!("{} without coverage", summary.no_coverage));
    }
    let mut line = format!("{} open ({})", summary.tasks, parts.join(", "));
    if summary.stale > 0 {
        let _ = write!(line, "; {} stale, without code", summary.stale);
    }
    line
}

fn push_partial_note(text: &mut String, output: &MutationTasksOutput) {
    if output.report_completion == Some(fallow_output::MutationCompletionStatus::Partial) {
        text.push_str(
            "\nThe report is partial, so this list can miss a mutant that did not run.\n",
        );
    }
}

fn item_title(task: &MutationTask, markdown: bool) -> String {
    let location = format!("{}:{}", task.file, task.line);
    let mut title = if markdown {
        format!("`{location}`")
    } else {
        location
    };
    if let Some(function) = &task.function {
        if markdown {
            let _ = write!(title, ", function `{function}`");
        } else {
            let _ = write!(title, ", function {function}");
        }
    }
    let status = match task.status {
        MutantStatus::NoCoverage => "no test reaches it",
        _ => "survived",
    };
    let _ = write!(title, " ({status})");
    if markdown {
        let _ = write!(title, " `{}`", task.mutant_id);
    } else {
        let _ = write!(title, " [{}]", task.mutant_id);
    }
    title
}

/// The lines below the title of one item.
struct ItemBody {
    /// The current and the changed code. Empty for a stale task.
    code: Vec<String>,
    /// The stale note and the test files.
    notes: Vec<String>,
}

fn item_body(task: &MutationTask) -> ItemBody {
    let mut body = ItemBody {
        code: Vec::new(),
        notes: Vec::new(),
    };
    match (&task.original_lines, &task.mutated_lines) {
        (Some(original), Some(mutated)) => {
            let (original, mutated) = dedent_pair(original, mutated);
            push_labeled(&mut body.code, "current: ", &original);
            push_labeled(&mut body.code, "changed: ", &mutated);
        }
        _ => body.notes.push(format!(
            "stale: {} changed after the run, so the code is not shown. Run `fallow mutation` again.",
            task.file
        )),
    }
    if !task.covering_test_files.is_empty() {
        body.notes
            .push(format!("tests: {}", task.covering_test_files.join(", ")));
    }
    if task.equivalence_hint.is_some() {
        body.notes.push(EQUIVALENCE_HINT_TEXT.to_owned());
    }
    if let Some(witness) = &task.witness {
        body.notes.push(witness_note(witness));
    }
    body
}

/// The note of a witness. With the cached values, the note gives the
/// input and the result of each side; the markdown output has no values.
fn witness_note(witness: &MutationTaskWitness) -> String {
    let command = &witness.replay_command;
    match (
        witness.verdict,
        &witness.args,
        &witness.original,
        &witness.mutant,
    ) {
        (
            fallow_output::MutationReplayVerdict::Witness,
            Some(args),
            Some(original),
            Some(mutant),
        ) => {
            let args: Vec<String> = args.iter().map(ToString::to_string).collect();
            format!(
                "witness: input [{}] gives {} on the current code and {} on the changed code ({command})",
                args.join(", "),
                original.text,
                mutant.text
            )
        }
        (fallow_output::MutationReplayVerdict::Witness, ..) => {
            format!("witness: a recorded input shows a difference ({command})")
        }
        _ => format!(
            "replay: {} ({command})",
            witness.reason.as_deref().unwrap_or("no witness")
        ),
    }
}

fn push_labeled(lines: &mut Vec<String>, label: &str, text: &str) {
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            lines.push(format!("{label}{line}"));
        } else {
            lines.push(format!("{}{line}", " ".repeat(label.len())));
        }
    }
}

/// Removes the indentation that each line of both texts shares.
fn dedent_pair(original: &str, mutated: &str) -> (String, String) {
    let indent = original
        .lines()
        .chain(mutated.lines())
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    let dedent = |text: &str| {
        text.lines()
            .map(|line| line.get(indent..).unwrap_or_else(|| line.trim_start()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    (dedent(original), dedent(mutated))
}

/// A code fence longer than each backtick run in `lines`.
fn fence_for(lines: &[String]) -> String {
    let longest = lines
        .iter()
        .flat_map(|line| line.split(|ch| ch != '`'))
        .map(str::len)
        .max()
        .unwrap_or(0);
    "`".repeat(longest.max(2) + 1)
}

#[cfg(test)]
#[path = "mutation_tasks_tests.rs"]
mod tests;
