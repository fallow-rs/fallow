//! `fallow rule-pack` subcommands: init, list, test, schema.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::OutputFormat;

mod init;
mod list;
mod templates;
mod test;

#[allow(
    dead_code,
    reason = "the command family is wired before every subcommand consumes all context fields"
)]
pub struct RulePackContext<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub json_style: crate::json_style::JsonStyle,
    pub quiet: bool,
    pub no_cache: bool,
    pub threads: Option<usize>,
    pub allow_remote_extends: bool,
}

fn render_json(
    value: &serde_json::Value,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, serde_json::Error> {
    json_style.serialize(value)
}

fn emit_json(
    value: &serde_json::Value,
    kind: &str,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    match render_json(value, json_style) {
        Ok(json) => {
            crate::report::sink::outln!("{json}");
            ExitCode::SUCCESS
        }
        Err(error) => crate::error::emit_error_with_style(
            &format!("failed to serialize {kind} output: {error}"),
            2,
            OutputFormat::Json,
            json_style,
        ),
    }
}

#[allow(
    dead_code,
    reason = "the init implementation consumes these parsed fields"
)]
pub struct InitArgs {
    pub name: Option<String>,
    pub template: String,
    pub dir: String,
    pub no_config: bool,
}

#[allow(
    dead_code,
    reason = "the test implementation consumes the optional pack path"
)]
pub struct TestArgs {
    pub pack: Option<PathBuf>,
}

pub enum RulePackSubcommand {
    Init(InitArgs),
    List,
    Test(TestArgs),
    Schema,
}

pub fn run(subcommand: &RulePackSubcommand, ctx: &RulePackContext<'_>) -> ExitCode {
    match subcommand {
        RulePackSubcommand::Schema => crate::init::run_rule_pack_schema(ctx.json_style),
        RulePackSubcommand::Init(args) => init::run(args, ctx),
        RulePackSubcommand::List => list::run(ctx),
        RulePackSubcommand::Test(args) => test::run(args, ctx),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rule_pack_json_respects_explicit_style() {
        let value = serde_json::json!({"kind": "rule-pack-test", "rules": []});
        let compact = super::render_json(&value, crate::json_style::JsonStyle::Compact)
            .expect("compact rule-pack JSON should serialize");
        let pretty = super::render_json(&value, crate::json_style::JsonStyle::Pretty)
            .expect("pretty rule-pack JSON should serialize");

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
