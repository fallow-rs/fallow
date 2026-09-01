//! Shared policy for a type-aware pass that could not run.
//!
//! Syntactic analysis reports a superset of findings and the type-aware pass
//! only removes candidates it confirms are used, so a run that loses the
//! semantic pass is stricter than a refined one, never laxer. Every CLI surface
//! therefore continues with the syntactic result unless `typeAware.require` is
//! `complete`, which keeps the historical hard failure. This mirrors the LSP
//! behavior in `crates/lsp/src/analysis.rs`.

use std::path::Path;
use std::process::ExitCode;

use fallow_config::{OutputFormat, TypeAwareRequire};
use fallow_types::envelope::TypeAwareMeta;

/// Warning text for a run that fell back to syntactic findings.
pub fn degraded_message(root: &Path, error: &str) -> String {
    format!(
        "type-aware refinement unavailable for {}: {error}; showing conservative syntactic findings",
        root.display()
    )
}

/// Metadata for a degraded pass, so `_meta.type_aware.warnings` and
/// `warning_count` carry the reason for machine consumers.
pub fn degraded_meta(message: String, require: TypeAwareRequire) -> TypeAwareMeta {
    TypeAwareMeta {
        required_completeness: Some(require.into()),
        executed: false,
        warning_count: 1,
        warnings: vec![message],
        ..TypeAwareMeta::default()
    }
}

/// Inputs for [`degrade_or_fail`].
pub struct DegradeContext<'a> {
    /// Project root named in the warning.
    pub root: &'a Path,
    /// Failure description from the type-aware pass.
    pub error: &'a str,
    /// Surface-specific prefix used for the `require = complete` hard error.
    pub failure_label: &'a str,
    /// Effective `typeAware.require` policy.
    pub require: TypeAwareRequire,
    /// Suppress the stderr warning.
    pub quiet: bool,
    /// Active output format, used to shape the hard error.
    pub output: OutputFormat,
}

/// Resolve a failed type-aware pass into degraded metadata, or into the exit
/// code 2 that `require = complete` still demands.
pub fn degrade_or_fail(ctx: &DegradeContext<'_>) -> Result<TypeAwareMeta, ExitCode> {
    if ctx.require == TypeAwareRequire::Complete {
        return Err(crate::error::emit_error(
            &format!("{}: {}", ctx.failure_label, ctx.error),
            2,
            ctx.output,
        ));
    }
    let message = degraded_message(ctx.root, ctx.error);
    emit_warning(&message, ctx.quiet);
    Ok(degraded_meta(message, ctx.require))
}

#[expect(
    clippy::print_stderr,
    reason = "degradation notice belongs on stderr next to other CLI warnings"
)]
fn emit_warning(message: &str, quiet: bool) {
    if !quiet {
        eprintln!("Warning: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(error: &str, require: TypeAwareRequire) -> DegradeContext<'_> {
        DegradeContext {
            root: Path::new("/repo"),
            error,
            failure_label: "Type-aware analysis failed",
            require,
            quiet: true,
            output: OutputFormat::Json,
        }
    }

    #[test]
    fn best_effort_degrades_with_a_recorded_warning() {
        let meta = degrade_or_fail(&context("sidecar timed out", TypeAwareRequire::BestEffort))
            .expect("best-effort should continue with syntactic findings");
        assert!(!meta.executed);
        assert_eq!(meta.warning_count, 1);
        assert!(meta.warnings[0].contains("sidecar timed out"));
        assert!(
            meta.warnings[0].contains("showing conservative syntactic findings"),
            "warning should explain the fallback: {}",
            meta.warnings[0]
        );
    }

    #[test]
    fn complete_still_fails_hard() {
        let outcome = degrade_or_fail(&context("sidecar timed out", TypeAwareRequire::Complete));
        assert!(outcome.is_err(), "require=complete must not degrade");
    }
}
