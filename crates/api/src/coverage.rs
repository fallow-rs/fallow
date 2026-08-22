//! Istanbul coverage input precedence shared by every surface that feeds CRAP
//! scoring: the explicit flag or tool parameter, then `FALLOW_COVERAGE` /
//! `FALLOW_COVERAGE_ROOT`, then `health.coverage` / `health.coverageRoot` from
//! the project config. Engine auto-detection of `coverage/coverage-final.json`
//! applies only when every layer leaves the coverage path unset.
//!
//! [`resolve_coverage_inputs`] is pure: adapters read the environment and load
//! config at their own boundary and pass the layers in, so the CLI and the MCP
//! typed route share one order (#2359, #2368) without the API reading process
//! state. NAPI keeps explicit options and does not call it.

use std::path::PathBuf;

use fallow_config::HealthConfig;

use crate::ProgrammaticError;

/// Istanbul coverage inputs from one resolution layer, or the resolved result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoverageInputs {
    /// Path to an Istanbul `coverage-final.json`, or a directory holding one.
    /// Relative paths resolve against the analysis root.
    pub coverage: Option<PathBuf>,
    /// Absolute path prefix the coverage map recorded its files under.
    pub coverage_root: Option<PathBuf>,
}

impl CoverageInputs {
    /// Whether the config layer still has to be consulted: at least one input
    /// stays unset after layering `explicit` over `env`. Callers that load
    /// config lazily skip the load when this is `false`.
    #[must_use]
    pub const fn needs_config_layer(explicit: &Self, env: &Self) -> bool {
        (explicit.coverage.is_none() && env.coverage.is_none())
            || (explicit.coverage_root.is_none() && env.coverage_root.is_none())
    }
}

/// The layer that supplied a resolved coverage input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageInputSource {
    /// A CLI flag or a tool parameter.
    Explicit,
    /// `FALLOW_COVERAGE` / `FALLOW_COVERAGE_ROOT`.
    Environment,
    /// `health.coverage` / `health.coverageRoot` in the project config.
    Config,
}

/// Shape errors in resolved coverage inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoverageInputError {
    /// The winning `coverage_root` is not an absolute prefix from the
    /// coverage data.
    RelativeCoverageRoot {
        /// The layer that supplied the rejected value.
        source: CoverageInputSource,
        /// The engine's rejection message, naming the value.
        message: String,
    },
}

impl CoverageInputError {
    /// Convert into the structured `FALLOW_INVALID_COVERAGE_ROOT` error
    /// (exit 2). `explicit_context` names the adapter's own input for the
    /// explicit layer; the environment and config layers name
    /// `FALLOW_COVERAGE_ROOT` and `health.coverageRoot`.
    #[must_use]
    pub fn into_programmatic_error(self, explicit_context: &str) -> ProgrammaticError {
        let Self::RelativeCoverageRoot { source, message } = self;
        let context = match source {
            CoverageInputSource::Explicit => explicit_context,
            CoverageInputSource::Environment => "FALLOW_COVERAGE_ROOT",
            CoverageInputSource::Config => "health.coverageRoot",
        };
        ProgrammaticError::new(message, 2)
            .with_code("FALLOW_INVALID_COVERAGE_ROOT")
            .with_context(context)
    }
}

impl std::fmt::Display for CoverageInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self::RelativeCoverageRoot { message, .. } = self;
        f.write_str(message)
    }
}

impl std::error::Error for CoverageInputError {}

/// Resolve coverage inputs with the shared precedence: `explicit`, then
/// `env`, then `config_health`. Each input resolves independently, so an
/// explicit coverage path pairs with an environment or config root and vice
/// versa. `config_health` is the `health` section of the project config when
/// the caller loaded one; see [`CoverageInputs::needs_config_layer`].
///
/// # Errors
///
/// Returns [`CoverageInputError::RelativeCoverageRoot`] when the winning
/// `coverage_root` is not absolute under Unix or Windows conventions.
pub fn resolve_coverage_inputs(
    explicit: CoverageInputs,
    env: CoverageInputs,
    config_health: Option<&HealthConfig>,
) -> Result<CoverageInputs, CoverageInputError> {
    let coverage = layer(
        explicit.coverage,
        env.coverage,
        config_health.and_then(|health| health.coverage.clone()),
    );
    let coverage_root = layer(
        explicit.coverage_root,
        env.coverage_root,
        config_health.and_then(|health| health.coverage_root.clone()),
    );
    if let Some((path, source)) = &coverage_root
        && let Err(message) = fallow_engine::health::validate_coverage_root_absolute(Some(path))
    {
        return Err(CoverageInputError::RelativeCoverageRoot {
            source: *source,
            message,
        });
    }
    Ok(CoverageInputs {
        coverage: coverage.map(|(path, _)| path),
        coverage_root: coverage_root.map(|(path, _)| path),
    })
}

fn layer(
    explicit: Option<PathBuf>,
    env: Option<PathBuf>,
    config: Option<PathBuf>,
) -> Option<(PathBuf, CoverageInputSource)> {
    explicit
        .map(|path| (path, CoverageInputSource::Explicit))
        .or_else(|| env.map(|path| (path, CoverageInputSource::Environment)))
        .or_else(|| config.map(|path| (path, CoverageInputSource::Config)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(coverage: Option<&str>, coverage_root: Option<&str>) -> CoverageInputs {
        CoverageInputs {
            coverage: coverage.map(PathBuf::from),
            coverage_root: coverage_root.map(PathBuf::from),
        }
    }

    fn health(coverage: Option<&str>, coverage_root: Option<&str>) -> HealthConfig {
        HealthConfig {
            coverage: coverage.map(PathBuf::from),
            coverage_root: coverage_root.map(PathBuf::from),
            ..HealthConfig::default()
        }
    }

    #[test]
    fn explicit_beats_env_beats_config() {
        let config = health(Some("config.json"), Some("/config"));
        let resolved = resolve_coverage_inputs(
            inputs(Some("flag.json"), Some("/flag")),
            inputs(Some("env.json"), Some("/env")),
            Some(&config),
        )
        .expect("absolute roots");
        assert_eq!(resolved, inputs(Some("flag.json"), Some("/flag")));

        let resolved = resolve_coverage_inputs(
            CoverageInputs::default(),
            inputs(Some("env.json"), Some("/env")),
            Some(&config),
        )
        .expect("absolute roots");
        assert_eq!(resolved, inputs(Some("env.json"), Some("/env")));

        let resolved = resolve_coverage_inputs(
            CoverageInputs::default(),
            CoverageInputs::default(),
            Some(&config),
        )
        .expect("absolute roots");
        assert_eq!(resolved, inputs(Some("config.json"), Some("/config")));
    }

    #[test]
    fn each_input_resolves_independently() {
        let config = health(Some("config.json"), Some("/config"));
        let resolved = resolve_coverage_inputs(
            inputs(Some("flag.json"), None),
            inputs(None, Some("/env")),
            Some(&config),
        )
        .expect("absolute roots");
        assert_eq!(resolved, inputs(Some("flag.json"), Some("/env")));

        let resolved = resolve_coverage_inputs(
            inputs(None, Some("/flag")),
            inputs(Some("env.json"), None),
            Some(&config),
        )
        .expect("absolute roots");
        assert_eq!(resolved, inputs(Some("env.json"), Some("/flag")));
    }

    #[test]
    fn empty_layers_leave_auto_detection_to_the_engine() {
        let resolved =
            resolve_coverage_inputs(CoverageInputs::default(), CoverageInputs::default(), None)
                .expect("nothing to validate");
        assert_eq!(resolved, CoverageInputs::default());

        let config = HealthConfig::default();
        let resolved = resolve_coverage_inputs(
            CoverageInputs::default(),
            CoverageInputs::default(),
            Some(&config),
        )
        .expect("nothing to validate");
        assert_eq!(resolved, CoverageInputs::default());
    }

    #[test]
    fn needs_config_layer_only_while_an_input_is_unset() {
        assert!(CoverageInputs::needs_config_layer(
            &CoverageInputs::default(),
            &CoverageInputs::default()
        ));
        assert!(CoverageInputs::needs_config_layer(
            &inputs(Some("flag.json"), None),
            &inputs(None, None)
        ));
        assert!(CoverageInputs::needs_config_layer(
            &inputs(None, Some("/flag")),
            &inputs(None, None)
        ));
        assert!(!CoverageInputs::needs_config_layer(
            &inputs(Some("flag.json"), None),
            &inputs(None, Some("/env"))
        ));
        assert!(!CoverageInputs::needs_config_layer(
            &inputs(Some("flag.json"), Some("/flag")),
            &CoverageInputs::default()
        ));
    }

    #[test]
    fn relative_root_is_rejected_and_names_its_layer() {
        let config = health(None, Some("src"));
        let err = resolve_coverage_inputs(
            CoverageInputs::default(),
            CoverageInputs::default(),
            Some(&config),
        )
        .expect_err("relative config root");
        assert!(matches!(
            &err,
            CoverageInputError::RelativeCoverageRoot {
                source: CoverageInputSource::Config,
                ..
            }
        ));
        assert!(
            err.to_string()
                .contains("--coverage-root expects an absolute path prefix")
                && err.to_string().contains("got 'src'"),
            "{err}"
        );

        let err = resolve_coverage_inputs(
            CoverageInputs::default(),
            inputs(None, Some("./coverage")),
            Some(&config),
        )
        .expect_err("relative env root wins over the config root");
        assert!(matches!(
            err,
            CoverageInputError::RelativeCoverageRoot {
                source: CoverageInputSource::Environment,
                ..
            }
        ));

        let err = resolve_coverage_inputs(
            inputs(None, Some("a/b")),
            inputs(None, Some("/env")),
            Some(&config),
        )
        .expect_err("relative explicit root wins over the env root");
        assert!(matches!(
            err,
            CoverageInputError::RelativeCoverageRoot {
                source: CoverageInputSource::Explicit,
                ..
            }
        ));
    }

    #[test]
    fn absolute_roots_under_either_platform_convention_are_accepted() {
        for root in ["/ci/workspace", r"C:\ci\workspace"] {
            let config = health(None, Some(root));
            let resolved = resolve_coverage_inputs(
                CoverageInputs::default(),
                CoverageInputs::default(),
                Some(&config),
            )
            .expect("absolute root");
            assert_eq!(resolved.coverage_root, Some(PathBuf::from(root)));
        }
    }

    #[test]
    fn programmatic_error_keeps_the_root_code_and_names_the_layer() {
        let cases = [
            (CoverageInputSource::Explicit, "audit.coverageRoot"),
            (CoverageInputSource::Environment, "FALLOW_COVERAGE_ROOT"),
            (CoverageInputSource::Config, "health.coverageRoot"),
        ];
        for (source, context) in cases {
            let err = CoverageInputError::RelativeCoverageRoot {
                source,
                message: "relative".to_string(),
            }
            .into_programmatic_error("audit.coverageRoot");
            assert_eq!(err.exit_code, 2);
            assert_eq!(err.message, "relative");
            assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_COVERAGE_ROOT"));
            assert_eq!(err.context.as_deref(), Some(context));
        }
    }
}
