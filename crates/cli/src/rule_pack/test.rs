use std::process::ExitCode;

use fallow_config::{OutputFormat, ResolvedConfig, RulePackRule, RulePackRuleKind, Severity};
use fallow_types::output_dead_code::PolicyViolationFinding;
use serde_json::json;

use super::{RulePackContext, TestArgs};
use crate::runtime_support::{LoadConfigArgs, load_config};

pub fn run(args: &TestArgs, ctx: &RulePackContext<'_>) -> ExitCode {
    let mut config = match load_test_config(args, ctx) {
        Ok(config) => config,
        Err(code) => return code,
    };

    let forced_severity = if config.rules.policy_violation == Severity::Off {
        config.rules.policy_violation = Severity::Warn;
        eprintln!("note: rules.policy-violation is off; forcing warn for this test run");
        true
    } else {
        false
    };

    let analysis =
        match fallow_engine::session::AnalysisSession::from_resolved_config(config.clone())
            .analyze_dead_code()
        {
            Ok(analysis) => analysis,
            Err(error) => return crate::error::emit_error(error.message(), 2, ctx.output),
        };
    let findings = analysis.results.policy_violations;
    let summaries = build_rule_summaries(&config, &findings);

    if matches!(ctx.output, OutputFormat::Json) {
        return emit_json(&config, forced_severity, &summaries, &findings);
    }

    emit_human(&summaries, &findings, ctx.root, forced_severity)
}

fn load_test_config(
    args: &TestArgs,
    ctx: &RulePackContext<'_>,
) -> Result<ResolvedConfig, ExitCode> {
    let mut config = load_config(
        ctx.root,
        ctx.config_path,
        LoadConfigArgs {
            output: ctx.output,
            no_cache: ctx.no_cache,
            threads: ctx.threads.unwrap_or_else(default_threads),
            production: false,
            quiet: ctx.quiet,
        },
    )?;

    if let Some(pack) = &args.pack {
        let pack_path = pack_arg_to_config_path(ctx.root, pack);
        let loaded =
            match fallow_config::load_rule_packs(ctx.root, std::slice::from_ref(&pack_path)) {
                Ok(packs) => packs,
                Err(errors) => {
                    let message = errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n  - ");
                    return Err(crate::error::emit_error(
                        &format!("invalid rule pack:\n  - {message}"),
                        2,
                        ctx.output,
                    ));
                }
            };
        config.rule_packs = loaded;
        config.rule_pack_sources = vec![std::path::PathBuf::from(pack_path)];
    } else if config.rule_packs.is_empty() {
        return Err(crate::error::emit_error(
            "no rule packs configured; pass a pack path or run: fallow rule-pack init",
            2,
            ctx.output,
        ));
    }

    Ok(config)
}

#[derive(Debug)]
struct RuleSummary {
    pack: String,
    rule_id: String,
    kind: &'static str,
    severity: Severity,
    findings: usize,
}

fn build_rule_summaries(
    config: &ResolvedConfig,
    findings: &[PolicyViolationFinding],
) -> Vec<RuleSummary> {
    config
        .rule_packs
        .iter()
        .flat_map(|pack| {
            pack.rules.iter().map(|rule| RuleSummary {
                pack: pack.name.clone(),
                rule_id: rule.id.clone(),
                kind: rule_kind(rule.kind),
                severity: effective_severity(rule, config.rules.policy_violation),
                findings: findings
                    .iter()
                    .filter(|finding| {
                        finding.violation.pack == pack.name && finding.violation.rule_id == rule.id
                    })
                    .count(),
            })
        })
        .collect()
}

fn emit_json(
    config: &ResolvedConfig,
    forced_severity: bool,
    summaries: &[RuleSummary],
    findings: &[PolicyViolationFinding],
) -> ExitCode {
    crate::report::emit_json(
        &json!({
            "kind": "rule-pack-test",
            "packs": config.rule_packs.iter().map(|pack| pack.name.as_str()).collect::<Vec<_>>(),
            "forced_severity": forced_severity,
            "rules": summaries.iter().map(|summary| {
                json!({
                    "pack": summary.pack,
                    "rule_id": summary.rule_id,
                    "kind": summary.kind,
                    "severity": summary.severity.to_string(),
                    "findings": summary.findings,
                })
            }).collect::<Vec<_>>(),
            "findings": findings,
        }),
        "rule-pack-test",
    )
}

fn emit_human(
    summaries: &[RuleSummary],
    findings: &[PolicyViolationFinding],
    root: &std::path::Path,
    forced_severity: bool,
) -> ExitCode {
    if forced_severity {
        println!("rules.policy-violation is off; testing with warn severity");
    }

    for summary in summaries.iter().filter(|summary| summary.findings > 0) {
        println!(
            "{}  {}/{}  {} {}",
            summary.severity,
            summary.pack,
            summary.rule_id,
            summary.findings,
            crate::report::plural(summary.findings)
        );
        for finding in findings.iter().filter(|finding| {
            finding.violation.pack == summary.pack && finding.violation.rule_id == summary.rule_id
        }) {
            println!(
                "  {}:{}:{}  matched \"{}\"",
                display_path(root, &finding.violation.path),
                finding.violation.line,
                finding.violation.col,
                finding.violation.matched
            );
        }
    }

    let empty = summaries
        .iter()
        .filter(|summary| summary.findings == 0)
        .collect::<Vec<_>>();
    if !empty.is_empty() {
        println!("no findings:");
        for summary in empty {
            println!(
                "  {}  {}/{}  {}",
                summary.severity, summary.pack, summary.rule_id, summary.kind
            );
        }
    }

    ExitCode::SUCCESS
}

fn effective_severity(rule: &RulePackRule, default_severity: Severity) -> Severity {
    rule.severity.unwrap_or(default_severity)
}

fn rule_kind(kind: RulePackRuleKind) -> &'static str {
    match kind {
        RulePackRuleKind::BannedCall => "banned-call",
        RulePackRuleKind::BannedImport => "banned-import",
        RulePackRuleKind::BannedEffect => "banned-effect",
        RulePackRuleKind::BannedExport => "banned-export",
    }
}

fn pack_arg_to_config_path(root: &std::path::Path, pack: &std::path::Path) -> String {
    let path = if pack.is_absolute() {
        pack.strip_prefix(root).unwrap_or(pack)
    } else {
        pack
    };
    path.to_string_lossy().replace('\\', "/")
}

fn display_path(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}
