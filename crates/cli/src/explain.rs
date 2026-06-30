//! CLI rendering for explainable rule output.
//!
//! The rule registry and JSON contract live in `fallow-api` so embedders and
//! MCP do not depend on the CLI crate. This module keeps terminal rendering and
//! compatibility re-exports for existing CLI call sites.

use std::process::ExitCode;

use colored::Colorize;
use fallow_config::OutputFormat;

pub use fallow_api::{
    CHECK_RULES, DUPES_RULES, FLAGS_RULES, HEALTH_RULES, RuleDef, RuleGuide, SECURITY_RULES,
    coverage_analyze_meta, coverage_setup_meta, rule_by_id, rule_by_token, rule_docs_url,
    rule_guide, security_meta, serialize_explain_programmatic_json,
};

/// Run the standalone explain subcommand.
#[must_use]
pub fn run_explain(issue_type: &str, output: OutputFormat) -> ExitCode {
    let Some(rule) = rule_by_token(issue_type) else {
        return crate::error::emit_error(
            &fallow_api::unknown_explain_error(issue_type).message,
            2,
            output,
        );
    };
    let guide = rule_guide(rule);
    match output {
        OutputFormat::Json => match serialize_explain_programmatic_json(
            issue_type,
            crate::output_runtime::current_root_envelope_mode(),
            crate::output_runtime::telemetry_analysis_run_id().as_deref(),
        ) {
            Ok(value) => crate::report::emit_json(&value, "explain"),
            Err(error) => crate::error::emit_error(&error.message, error.exit_code, output),
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
        | OutputFormat::Badge => crate::error::emit_error(
            "explain supports human, compact, markdown, and json output",
            2,
            output,
        ),
    }
}

fn print_explain_human(rule: &RuleDef, guide: &RuleGuide) -> ExitCode {
    println!("{}", rule.name.bold());
    println!("{}", rule.id.dimmed());
    println!();
    println!("{}", rule.short);
    println!();
    println!("{}", "Why it matters".bold());
    println!("{}", rule.full);
    println!();
    println!("{}", "Example".bold());
    println!("{}", guide.example);
    println!();
    println!("{}", "How to fix".bold());
    println!("{}", guide.how_to_fix);
    println!();
    println!("{} {}", "Docs:".dimmed(), rule_docs_url(rule).dimmed());
    ExitCode::SUCCESS
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
    println!("{}", rule.full);
    println!();
    println!("## Example");
    println!();
    println!("{}", guide.example);
    println!();
    println!("## How to fix");
    println!();
    println!("{}", guide.how_to_fix);
    println!();
    println!("[Docs]({})", rule_docs_url(rule));
    ExitCode::SUCCESS
}
