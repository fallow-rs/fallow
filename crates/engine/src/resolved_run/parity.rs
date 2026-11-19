//! The harness of the run parity matrix.
//!
//! Each case in `tests/fixtures/run-parity/cases/<name>.json` names project
//! files, environment variables, one input per surface (a row) and the
//! expected decisions. A row test gives the translator of its surface to
//! [`assert_rows`]. The translator takes the raw native input of the case and
//! an [`EnvSnapshot`], and returns the resolved run of that input. The harness
//! compares the decisions of the [`ResolvedRun::view`] with the expected
//! values. No case changes the process environment and no case runs an
//! analysis.
//!
//! The snapshot of each row has a current directory that is not the case
//! root, so a translator that forgets the cwd rule of its surface fails a
//! case with a relative path.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fallow_config::EnvSnapshot;
use serde::Deserialize;
use serde_json::Value;

use super::ResolvedRun;

/// The expected value of a case key that names the row of the test.
const ROW_PLACEHOLDER: &str = "<row>";

/// The folder of the case files.
fn cases_dir() -> PathBuf {
    let Some(manifest_dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        panic!("cargo sets CARGO_MANIFEST_DIR");
    };
    Path::new(&manifest_dir).join("../../tests/fixtures/run-parity/cases")
}

/// One case of the parity matrix.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// The name of the case. It equals the file name without `.json`.
    pub name: String,
    /// The project files, by path relative to the case root.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// The environment variables of the run.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The raw native input of each row.
    pub inputs: BTreeMap<String, Value>,
    /// The expected decisions, by the dotted path of a view field, for
    /// example `analyses.dead_code.production`.
    pub expected: BTreeMap<String, Expected>,
    /// The rows that differ from `expected`, each with its reason.
    #[serde(default)]
    pub deviations: BTreeMap<String, Deviation>,
}

/// The expected value of one view field.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Expected {
    /// A decision: the value and the source, as `<kind>:<name>`, for example
    /// `config-file:production.deadCode`.
    Decision {
        /// The value of the decision.
        value: Value,
        /// The source of the decision.
        source: String,
    },
    /// A plain value. `"<row>"` means the name of the row.
    Plain(Value),
}

/// The expected decisions of one row that differ from the case.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deviation {
    /// Why the row differs. A deviation without a reason fails.
    pub reason: String,
    /// The expected values of the row, over the values of the case.
    pub expected: BTreeMap<String, Expected>,
}

/// The result of a translator for one case.
pub enum RowResult {
    /// The translator resolved the run.
    Run(Box<ResolvedRun>),
    /// The surface cannot express the input of the case. The case input of
    /// the row must give the reason as `not_expressible`.
    NotExpressible,
    /// The translator or the resolver failed.
    Failed(String),
}

/// A case that a row does not pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowFailure {
    /// The name of the case.
    pub case: String,
    /// The view field, or `input` for a problem of the case input.
    pub key: String,
    /// What is wrong.
    pub message: String,
}

/// Every case of the matrix, sorted by name.
///
/// # Panics
///
/// Panics when a case file cannot be read or parsed, or when its name does
/// not equal its file name.
#[must_use]
pub fn load_cases() -> Vec<Case> {
    let dir = cases_dir();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            let case: Case = serde_json::from_str(&text)
                .unwrap_or_else(|err| panic!("cannot parse {}: {err}", path.display()));
            let stem = path.file_stem().and_then(|stem| stem.to_str());
            assert_eq!(stem, Some(case.name.as_str()), "{}", path.display());
            case
        })
        .collect()
}

/// Write the files of `case` into `dir`, with an empty `.git` folder that
/// stops the config discovery at `dir`.
///
/// # Panics
///
/// Panics when a file cannot be written.
fn write_case(dir: &Path, case: &Case) {
    std::fs::create_dir_all(dir.join(".git")).unwrap_or_else(|err| panic!("git marker: {err}"));
    for (name, content) in &case.files {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap_or_else(|err| panic!("case file folder: {err}"));
        }
        std::fs::write(&path, content).unwrap_or_else(|err| panic!("case file: {err}"));
    }
}

/// Run the translator of `row` on each case and compare its decisions with
/// the expected values. Returns each failure; an empty list means that the
/// row passes.
///
/// # Panics
///
/// Panics when a temporary folder cannot be made or a case cannot be
/// written.
pub fn assert_rows(
    row: &str,
    cases: &[Case],
    translate: impl Fn(&Case, &Path, EnvSnapshot) -> RowResult,
) -> Vec<RowFailure> {
    let mut failures = Vec::new();
    for case in cases {
        let failure = |key: &str, message: String| RowFailure {
            case: case.name.clone(),
            key: key.to_owned(),
            message,
        };
        let Some(input) = case.inputs.get(row) else {
            failures.push(failure(
                "input",
                format!("the case has no input for `{row}`"),
            ));
            continue;
        };
        let project = tempfile::tempdir().unwrap_or_else(|err| panic!("case root: {err}"));
        let cwd = tempfile::tempdir().unwrap_or_else(|err| panic!("case cwd: {err}"));
        let root = dunce::canonicalize(project.path())
            .unwrap_or_else(|err| panic!("canonical case root: {err}"));
        write_case(&root, case);
        let pairs: Vec<(&str, &str)> = case
            .env
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect();
        let env = EnvSnapshot::from_pairs(&pairs, Some(cwd.path()));
        match translate(case, &root, env) {
            RowResult::NotExpressible => {
                let reason = input.get("not_expressible").and_then(Value::as_str);
                if reason.is_none_or(str::is_empty) {
                    failures.push(failure(
                        "input",
                        "the row is not expressible, and the case gives no reason".to_owned(),
                    ));
                }
            }
            RowResult::Failed(message) => failures.push(failure("run", message)),
            RowResult::Run(run) => {
                failures.extend(
                    compare(row, case, &run)
                        .into_iter()
                        .map(|(key, message)| failure(&key, message)),
                );
            }
        }
    }
    failures
}

/// The expected values of `row`: the case values with the deviation of the
/// row over them.
fn expected_for(row: &str, case: &Case) -> Result<BTreeMap<String, Expected>, String> {
    let mut expected = case.expected.clone();
    if let Some(deviation) = case.deviations.get(row) {
        if deviation.reason.trim().is_empty() {
            return Err(format!("the deviation of `{row}` has no reason"));
        }
        expected.extend(deviation.expected.clone());
    }
    Ok(expected)
}

fn compare(row: &str, case: &Case, run: &ResolvedRun) -> Vec<(String, String)> {
    let expected = match expected_for(row, case) {
        Ok(expected) => expected,
        Err(message) => return vec![("deviations".to_owned(), message)],
    };
    let view = match serde_json::to_value(run.view()) {
        Ok(view) => view,
        Err(err) => return vec![("view".to_owned(), err.to_string())],
    };
    expected
        .iter()
        .filter_map(|(key, expected)| {
            let actual = key
                .split('.')
                .try_fold(&view, |value, part| value.get(part));
            let Some(actual) = actual else {
                return Some((key.clone(), "the view has no such field".to_owned()));
            };
            compare_value(row, expected, actual).map(|message| (key.clone(), message))
        })
        .collect()
}

fn compare_value(row: &str, expected: &Expected, actual: &Value) -> Option<String> {
    match expected {
        Expected::Plain(Value::String(text)) if text == ROW_PLACEHOLDER => {
            (actual.as_str() != Some(row)).then(|| format!("expected `{row}`, got {actual}"))
        }
        Expected::Plain(value) => {
            (value != actual).then(|| format!("expected {value}, got {actual}"))
        }
        Expected::Decision { value, source } => {
            let actual_value = actual.get("value").unwrap_or(&Value::Null);
            let actual_source = actual.get("source").map(source_label);
            if actual_value == value && actual_source.as_deref() == Some(source.as_str()) {
                None
            } else {
                Some(format!(
                    "expected {value} from {source}, got {actual_value} from {}",
                    actual_source.as_deref().unwrap_or("no source")
                ))
            }
        }
    }
}

/// The `<kind>:<name>` label of a serialized source.
fn source_label(source: &Value) -> String {
    let kind = source.get("kind").and_then(Value::as_str).unwrap_or("");
    let field = |name| source.get(name).and_then(Value::as_str).unwrap_or("");
    let name = match kind {
        "default" => field("rule"),
        "derived" => field("from"),
        "config-file" => field("key"),
        "environment" => field("name"),
        "given" => source
            .get("origin")
            .and_then(|origin| origin.get("name"))
            .and_then(Value::as_str)
            .unwrap_or(""),
        _ => "",
    };
    format!("{kind}:{name}")
}

/// The argv of an MCP CLI route without the arguments that the MCP server
/// adds to each command: `--format json`, `--quiet` and `--explain`.
#[must_use]
pub fn normalize_mcp_cli_argv(argv: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(argv.len());
    let mut args = argv.iter().peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--quiet" | "--explain" | "--format=json" => {}
            "--format" if args.peek().is_some_and(|next| *next == "json") => {
                args.next();
            }
            _ => normalized.push(arg.clone()),
        }
    }
    normalized
}

#[cfg(test)]
#[path = "parity_tests.rs"]
mod tests;
