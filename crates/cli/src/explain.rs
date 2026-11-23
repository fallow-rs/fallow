//! CLI rendering for explainable rule output.
//!
//! The rule registry and JSON contract live in `fallow-api` so embedders and
//! MCP do not depend on the CLI crate. This module keeps terminal rendering and
//! compatibility re-exports for existing CLI call sites.

use std::process::ExitCode;

use colored::Colorize;
use fallow_config::OutputFormat;
use fallow_output::{ExplainExampleFile, ExplainOutput};

use crate::report::sink::outln;

pub use fallow_api::{
    AUDIT_RULES, CHECK_RULES, DUPES_RULES, ExampleFile, FLAGS_RULES, HEALTH_RULES, MUTATION_RULES,
    RuleDef, RuleExample, RuleGuide, SECURITY_RULES, coverage_analyze_meta, coverage_setup_meta,
    rule_by_id, rule_by_token, rule_docs_url, rule_example, rule_guide, rule_has_guide,
    rule_rationale, rule_severity_key, security_meta, serialize_explain_programmatic_json,
};

/// Run the standalone explain subcommand.
pub(crate) fn run_explain(
    issue_type: &str,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let resolved = if matches!(output, OutputFormat::Json) {
        fallow_api::explain_issue_type(issue_type)
    } else {
        fallow_api::explain::explain_issue_type_for_display(issue_type)
    };
    let explanation = match resolved {
        Ok(explanation) => explanation,
        Err(error) => return crate::error::emit_programmatic_error(&error, output, json_style),
    };
    match output {
        OutputFormat::Json => match render_explain_json(explanation, json_style) {
            Ok(json) => {
                outln!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => crate::error::emit_programmatic_error(&error, output, json_style),
        },
        OutputFormat::Human => print_explain_human(&explanation),
        OutputFormat::Compact => print_explain_compact(&explanation),
        OutputFormat::Markdown => print_explain_markdown(&explanation),
        OutputFormat::Sarif
        | OutputFormat::CodeClimate
        | OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab
        | OutputFormat::Badge
        | OutputFormat::GithubAnnotations
        | OutputFormat::GithubSummary => crate::error::emit_input_error(
            "explain supports human, compact, markdown, and json output",
            output,
        ),
    }
}

fn render_explain_json(
    explanation: ExplainOutput,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, fallow_api::ProgrammaticError> {
    fallow_output::serialize_explain_json_output(
        explanation,
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .and_then(|value| json_style.serialize(&value))
    .map_err(|error| {
        fallow_api::ProgrammaticError::new(format!("JSON serialization error: {error}"), 2)
            .with_code("json_serialization")
    })
}

fn print_explain_human(explanation: &ExplainOutput) -> ExitCode {
    println!("{}", explanation.name.bold());
    println!("{}", explanation.id.dimmed());
    println!();
    println!("{}", explanation.summary);
    println!();
    println!("{}", "Why it matters".bold());
    println!("{}", explanation.rationale);
    println!();
    println!("{}", "Example".bold());
    println!("{}", explanation.example);
    println!();
    print_example_human(explanation);
    println!("{}", "How to fix".bold());
    println!("{}", explanation.how_to_fix);
    println!();
    println!("{} {}", "Docs:".dimmed(), explanation.docs.dimmed());
    ExitCode::SUCCESS
}

const BEFORE_CHANGE: &str = "Before the change";
const AFTER_CHANGE: &str = "After the change";

fn example_groups(
    explanation: &ExplainOutput,
) -> impl Iterator<Item = (Option<&'static str>, &[ExplainExampleFile])> {
    let has_base = !explanation.example_base_files.is_empty();
    [
        (
            Some(BEFORE_CHANGE),
            explanation.example_base_files.as_slice(),
        ),
        (
            has_base.then_some(AFTER_CHANGE),
            explanation.example_files.as_slice(),
        ),
    ]
    .into_iter()
    .filter(|(_, files)| !files.is_empty())
}

fn print_example_human(explanation: &ExplainOutput) {
    for (title, files) in example_groups(explanation) {
        if let Some(title) = title {
            println!("{title}:");
            println!();
        }
        for file in files {
            println!("  {}", file.path.dimmed());
            for line in file.code.lines() {
                println!("    {line}");
            }
            println!();
        }
    }
}

/// The language of a Markdown code fence for `path`: its file extension.
fn fence_language(path: &str) -> &str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    name.rsplit_once('.').map_or("", |(_, extension)| extension)
}

fn print_example_markdown(explanation: &ExplainOutput) {
    for (title, files) in example_groups(explanation) {
        if let Some(title) = title {
            println!("**{title}**");
            println!();
        }
        for file in files {
            println!("`{}`", file.path);
            println!();
            println!("```{}", fence_language(&file.path));
            println!("{}", file.code.trim_end());
            println!("```");
            println!();
        }
    }
}

fn print_explain_compact(explanation: &ExplainOutput) -> ExitCode {
    println!(
        "explain:{}:{}:{}",
        explanation.id, explanation.summary, explanation.docs
    );
    ExitCode::SUCCESS
}

fn print_explain_markdown(explanation: &ExplainOutput) -> ExitCode {
    println!("# {}", explanation.name);
    println!();
    println!("`{}`", explanation.id);
    println!();
    println!("{}", explanation.summary);
    println!();
    println!("## Why it matters");
    println!();
    println!("{}", explanation.rationale);
    println!();
    println!("## Example");
    println!();
    println!("{}", explanation.example);
    println!();
    print_example_markdown(explanation);
    println!("## How to fix");
    println!();
    println!("{}", explanation.how_to_fix);
    println!();
    println!("[Docs]({})", explanation.docs);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_json_respects_explicit_style() {
        let explanation = fallow_api::explain_issue_type("unused-export").unwrap();
        let compact =
            render_explain_json(explanation.clone(), crate::json_style::JsonStyle::Compact)
                .expect("compact explain JSON should serialize");
        let pretty = render_explain_json(explanation, crate::json_style::JsonStyle::Pretty)
            .expect("pretty explain JSON should serialize");

        assert!(
            !compact.contains('\n'),
            "compact JSON must stay on one line"
        );
        assert!(pretty.contains("\n  \""), "pretty JSON must be indented");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&compact).unwrap(),
            serde_json::from_str::<serde_json::Value>(&pretty).unwrap(),
        );
    }
}
