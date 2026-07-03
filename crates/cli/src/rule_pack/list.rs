use std::process::ExitCode;

use super::RulePackContext;

pub fn run(ctx: &RulePackContext<'_>) -> ExitCode {
    crate::error::emit_error(
        "fallow rule-pack list is not implemented yet",
        2,
        ctx.output,
    )
}
