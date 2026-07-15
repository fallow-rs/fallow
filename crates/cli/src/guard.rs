//! `fallow guard` subcommand: report architecture rules before editing files.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_engine::guard::{GuardError, build_guard_report};
use fallow_types::guard::{GuardFileReport, GuardPolicyRule, GuardReport};
use serde_json::{Value, json};

use crate::error::emit_error;
use crate::report::sink::outln;
use crate::runtime_support::{LoadConfigArgs, load_config};

/// Options for the `fallow guard` subcommand.
pub struct GuardOptions<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub quiet: bool,
    pub allow_remote_extends: bool,
    pub files: &'a [String],
}

/// Run the `fallow guard` subcommand.
pub fn run_guard(opts: &GuardOptions<'_>) -> ExitCode {
    let config = match load_config(
        opts.root,
        opts.config_path,
        LoadConfigArgs {
            output: opts.output,
            no_cache: false,
            threads: 1,
            production: false,
            quiet: opts.quiet,
            allow_remote_extends: opts.allow_remote_extends,
        },
    ) {
        Ok(config) => config,
        Err(code) => return code,
    };

    let report = match build_guard_report(&config, opts.files) {
        Ok(report) => report,
        Err(GuardError::OutsideRoot(path)) => {
            return emit_error(
                &format!("guard target is outside project root: {path}"),
                2,
                opts.output,
            );
        }
    };

    emit_guard_report(&report, opts.output, opts.json_style)
}

fn emit_guard_report(
    report: &GuardReport,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match output {
        OutputFormat::Json => emit_json(report, json_style),
        OutputFormat::Human => {
            emit_human(report);
            ExitCode::SUCCESS
        }
        _ => emit_error("guard supports --format json or human", 2, output),
    }
}

fn emit_json(report: &GuardReport, json_style: crate::json_style::JsonStyle) -> ExitCode {
    match render_guard_json(report, json_style) {
        Ok(json) => {
            outln!("{json}");
            ExitCode::SUCCESS
        }
        Err(err) => crate::error::emit_error_with_style(
            &format!("failed to serialize guard output: {err}"),
            2,
            OutputFormat::Json,
            json_style,
        ),
    }
}

fn render_guard_json(
    report: &GuardReport,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, serde_json::Error> {
    let mut value = serde_json::to_value(report)?;
    if let Value::Object(map) = &mut value {
        map.insert("kind".to_string(), json!("guard"));
    }
    json_style.serialize(&value)
}

fn emit_human(report: &GuardReport) {
    for (index, file) in report.files.iter().enumerate() {
        if index > 0 {
            outln!();
        }
        emit_human_file(file);
    }
}

fn emit_human_file(file: &GuardFileReport) {
    let zone = file.zone.as_ref().map_or("none", |zone| zone.name.as_str());
    outln!("{} (zone: {zone})", file.path);
    if !file.exists {
        outln!("  exists: false");
    }

    for note in &file.notes {
        outln!("  note: {note}");
    }

    outln!(
        "  may import zones: {}   type-only: {}",
        zone_list(file),
        list_or_none(&file.boundary.allowed_type_only_zones)
    );
    if file.boundary.forbidden_calls.is_empty() {
        outln!("  forbidden calls in zone: none");
    } else {
        outln!(
            "  forbidden calls in zone: {}",
            file.boundary.forbidden_calls.join(", ")
        );
    }

    if file.policy_rules.is_empty() {
        outln!("  policy rules: none");
    } else {
        outln!("  policy rules:");
        for rule in &file.policy_rules {
            emit_policy_rule(rule);
        }
    }

    outln!(
        "  severities: boundary-violation={}  policy-violation={}",
        file.severities.boundary_violation,
        file.severities.policy_violation
    );
}

fn zone_list(file: &GuardFileReport) -> String {
    if file.boundary.allowed_zones.is_empty() {
        return "none".to_string();
    }
    let current_zone = file.zone.as_ref().map(|zone| zone.name.as_str());
    file.boundary
        .allowed_zones
        .iter()
        .map(|zone| {
            if Some(zone.as_str()) == current_zone {
                format!("{zone} (same zone)")
            } else {
                zone.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

fn emit_policy_rule(rule: &GuardPolicyRule) {
    let patterns = list_or_none(&rule.patterns);
    outln!(
        "    {}  {}/{}  {}: {}",
        rule.severity,
        rule.pack,
        rule.rule_id,
        rule.kind,
        patterns
    );
    if let Some(message) = &rule.message {
        outln!("           {message}");
    }
    outln!(
        "           suppress: // fallow-ignore-next-line {} -- <reason>",
        rule.suppress_token
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_json_respects_explicit_style() {
        let report = GuardReport { files: vec![] };
        let compact = render_guard_json(&report, crate::json_style::JsonStyle::Compact)
            .expect("compact guard JSON should serialize");
        let pretty = render_guard_json(&report, crate::json_style::JsonStyle::Pretty)
            .expect("pretty guard JSON should serialize");

        assert!(
            !compact.contains('\n'),
            "compact JSON must stay on one line"
        );
        assert!(pretty.contains("\n  \""), "pretty JSON must be indented");
        assert_eq!(
            serde_json::from_str::<Value>(&compact).unwrap(),
            serde_json::from_str::<Value>(&pretty).unwrap(),
        );
    }
}
