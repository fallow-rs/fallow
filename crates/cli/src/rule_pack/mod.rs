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
    pub quiet: bool,
    pub no_cache: bool,
    pub threads: Option<usize>,
    pub allow_remote_extends: bool,
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
        RulePackSubcommand::Schema => crate::init::run_rule_pack_schema(),
        RulePackSubcommand::Init(args) => init::run(args, ctx),
        RulePackSubcommand::List => list::run(ctx),
        RulePackSubcommand::Test(args) => test::run(args, ctx),
    }
}
