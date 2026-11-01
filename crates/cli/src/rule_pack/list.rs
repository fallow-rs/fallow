use std::path::Path;
use std::process::ExitCode;

use fallow_config::{OutputFormat, ResolvedConfig, RulePackRule, RulePackRuleKind, Severity};
use serde_json::json;

use super::RulePackContext;
use crate::runtime_support::{LoadConfigArgs, load_config};

pub fn run(ctx: &RulePackContext<'_>) -> ExitCode {
    let config = match load_config(
        ctx.root,
        ctx.config_path,
        LoadConfigArgs {
            output: ctx.output,
            no_cache: ctx.no_cache,
            threads: ctx.threads.unwrap_or_else(default_threads),
            production: false,
            quiet: ctx.quiet,
            allow_remote_extends: ctx.allow_remote_extends,
        },
    ) {
        Ok(config) => config,
        Err(code) => return code,
    };

    if matches!(ctx.output, OutputFormat::Json) {
        return emit_json(&config, ctx.json_style);
    }

    emit_human(&config)
}

fn emit_json(config: &ResolvedConfig, json_style: crate::json_style::JsonStyle) -> ExitCode {
    super::emit_json(
        &json!({
            "kind": "rule-pack-list",
            "packs": config.rule_packs.iter().enumerate().map(|(index, pack)| {
                json!({
                    "name": pack.name,
                    "source": config.rule_pack_sources.get(index).map(|path| path_to_string(path)),
                    "description": pack.description,
                    "rules": pack.rules.iter().map(|rule| {
                        let mut output = json!({
                            "id": rule.id,
                            "kind": rule_kind(rule.kind),
                            "severity": effective_severity(rule, config.rules.policy_violation).to_string(),
                            "patterns": rule_patterns(rule),
                            "files": rule.files,
                            "exclude": rule.exclude,
                            "message": rule.message,
                        });
                        if !rule.allow_names.is_empty() {
                            output["allowNames"] = json!(rule.allow_names);
                        }
                        if rule.kind == RulePackRuleKind::GdpProofProducer {
                            output["allowedFiles"] = json!(rule.allowed_files);
                            output["proofKinds"] = json!(rule.proof_kinds);
                        }
                        output
                    }).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
        }),
        "rule-pack-list",
        json_style,
    )
}

fn emit_human(config: &ResolvedConfig) -> ExitCode {
    if config.rule_packs.is_empty() {
        println!("No rule packs configured.");
        println!("Next: fallow rule-pack init");
        return ExitCode::SUCCESS;
    }

    for (index, pack) in config.rule_packs.iter().enumerate() {
        let source = config
            .rule_pack_sources
            .get(index)
            .map_or_else(|| "<unknown>".to_string(), |path| path_to_string(path));
        println!(
            "{} ({source}): {} {}",
            pack.name,
            pack.rules.len(),
            crate::report::plural(pack.rules.len())
        );
        if let Some(description) = &pack.description {
            println!("  {description}");
        }
        for rule in &pack.rules {
            let severity = effective_severity(rule, config.rules.policy_violation);
            let patterns = rule_patterns(rule);
            let pattern_text = if patterns.is_empty() {
                String::from("<none>")
            } else {
                patterns.join(", ")
            };
            let allowed_names = if rule.allow_names.is_empty() {
                String::new()
            } else {
                format!(" (allowed names: {})", rule.allow_names.join(", "))
            };
            println!(
                "  - {} [{}] {}: {}{}",
                rule.id,
                severity,
                rule_kind(rule.kind),
                pattern_text,
                allowed_names
            );
            if let Some(message) = &rule.message {
                println!("    {message}");
            }
            if rule.kind == RulePackRuleKind::GdpProofProducer {
                println!("    Allowed producers: {}", rule.allowed_files.join(", "));
                println!(
                    "    Proof kinds: {}",
                    if rule.proof_kinds.is_empty() {
                        "all, including dynamic labels".to_owned()
                    } else {
                        rule.proof_kinds.join(", ")
                    }
                );
            }
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
        RulePackRuleKind::GdpProofProducer => "gdp-proof-producer",
    }
}

fn rule_patterns(rule: &RulePackRule) -> Vec<String> {
    match rule.kind {
        RulePackRuleKind::BannedCall => rule.callees.clone(),
        RulePackRuleKind::BannedImport => rule.specifiers.clone(),
        RulePackRuleKind::BannedEffect => rule
            .effects
            .iter()
            .map(|effect| effect.as_str().to_string())
            .collect(),
        RulePackRuleKind::BannedExport => rule.exports.clone(),
        RulePackRuleKind::GdpProofProducer => vec!["@gdp-ts/core.defineProof".to_string()],
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}
