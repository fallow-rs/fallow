use std::process::ExitCode;

use super::{RulePackContext, TestArgs};

pub fn run(_args: &TestArgs, ctx: &RulePackContext<'_>) -> ExitCode {
    crate::error::emit_error(
        "fallow rule-pack test is not implemented yet",
        2,
        ctx.output,
    )
}
