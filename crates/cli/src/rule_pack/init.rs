use std::process::ExitCode;

use super::{InitArgs, RulePackContext};

pub fn run(_args: &InitArgs, ctx: &RulePackContext<'_>) -> ExitCode {
    crate::error::emit_error(
        "fallow rule-pack init is not implemented yet",
        2,
        ctx.output,
    )
}
