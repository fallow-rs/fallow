//! CLI rendering for explainable rule output.
//!
//! The rule registry and JSON contract live in `fallow-api` so embedders and
//! MCP do not depend on the CLI crate. This module keeps terminal rendering and
//! compatibility re-exports for existing CLI call sites.

use std::process::ExitCode;

use colored::Colorize;
use fallow_config::OutputFormat;

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
    let Some(rule) = rule_by_token(issue_type) else {
        return crate::error::emit_programmatic_error(
            &fallow_api::unknown_explain_error(issue_type),
            output,
            json_style,
        );
    };
    let guide = rule_guide(rule);
    match output {
        OutputFormat::Json => match render_explain_json(issue_type, json_style) {
            Ok(json) => {
                outln!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => crate::error::emit_programmatic_error(&error, output, json_style),
        },
        OutputFormat::Human => print_explain_human(rule, &guide),
        OutputFormat::Compact => print_explain_compact(rule),
        OutputFormat::Markdown => print_explain_markdown(rule, &guide),
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
    issue_type: &str,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, fallow_api::ProgrammaticError> {
    let value = serialize_explain_programmatic_json(
        issue_type,
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )?;
    json_style.serialize(&value).map_err(|error| {
        fallow_api::ProgrammaticError::new(format!("JSON serialization error: {error}"), 2)
            .with_code("json_serialization")
    })
}

fn print_explain_human(rule: &RuleDef, guide: &RuleGuide) -> ExitCode {
    println!("{}", rule.name.bold());
    println!("{}", rule.id.dimmed());
    println!();
    println!("{}", rule.short);
    println!();
    println!("{}", "Why it matters".bold());
    println!("{}", rule_rationale(rule));
    println!();
    println!("{}", "Example".bold());
    println!("{}", guide.example);
    println!();
    if let Some(example) = rule_example(rule) {
        print_example_human(example);
    }
    println!("{}", "How to fix".bold());
    println!("{}", guide.how_to_fix);
    println!();
    println!("{} {}", "Docs:".dimmed(), rule_docs_url(rule).dimmed());
    ExitCode::SUCCESS
}

const BEFORE_CHANGE: &str = "Before the change";
const AFTER_CHANGE: &str = "After the change";

/// The visible files of `example` in groups: one group with no title, or
/// the files before the change and the files after it.
fn example_groups(example: &RuleExample) -> Vec<(Option<&'static str>, Vec<&ExampleFile>)> {
    let visible =
        |files: &'static [ExampleFile]| files.iter().filter(|file| !file.hidden).collect();
    if example.base.is_empty() {
        return vec![(None, visible(example.files))];
    }
    vec![
        (Some(BEFORE_CHANGE), visible(example.base)),
        (Some(AFTER_CHANGE), visible(example.files)),
    ]
}

fn print_example_human(example: &RuleExample) {
    for (title, files) in example_groups(example) {
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

fn print_example_markdown(example: &RuleExample) {
    for (title, files) in example_groups(example) {
        if let Some(title) = title {
            println!("**{title}**");
            println!();
        }
        for file in files {
            println!("`{}`", file.path);
            println!();
            println!("```{}", fence_language(file.path));
            println!("{}", file.code.trim_end());
            println!("```");
            println!();
        }
    }
}

fn print_explain_compact(rule: &RuleDef) -> ExitCode {
    println!("explain:{}:{}:{}", rule.id, rule.short, rule_docs_url(rule));
    ExitCode::SUCCESS
}

fn print_explain_markdown(rule: &RuleDef, guide: &RuleGuide) -> ExitCode {
    println!("# {}", rule.name);
    println!();
    println!("`{}`", rule.id);
    println!();
    println!("{}", rule.short);
    println!();
    println!("## Why it matters");
    println!();
    println!("{}", rule_rationale(rule));
    println!();
    println!("## Example");
    println!();
    println!("{}", guide.example);
    println!();
    if let Some(example) = rule_example(rule) {
        print_example_markdown(example);
    }
    println!("## How to fix");
    println!();
    println!("{}", guide.how_to_fix);
    println!();
    println!("[Docs]({})", rule_docs_url(rule));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_json_respects_explicit_style() {
        let compact = render_explain_json("unused-export", crate::json_style::JsonStyle::Compact)
            .expect("compact explain JSON should serialize");
        let pretty = render_explain_json("unused-export", crate::json_style::JsonStyle::Pretty)
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
