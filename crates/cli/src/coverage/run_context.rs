//! The resolved runs of the `fallow coverage` subcommands.

use fallow_engine::resolved_run::ResolveError;

use super::RunContext;

impl<'a> RunContext<'a> {
    /// The context of the command line that `dispatch` holds.
    pub(crate) fn of(dispatch: &'a crate::DispatchContext<'a>) -> Self {
        Self {
            root: dispatch.root,
            config_path: &dispatch.cli.config,
            output: dispatch.output,
            json_style: dispatch.json_style,
            quiet: dispatch.quiet,
            no_cache: dispatch.cli.no_cache,
            threads: dispatch.threads,
            explain: dispatch.cli.explain,
            allow_remote_extends: dispatch.cli.allow_remote_extends,
            run_inputs: dispatch.run_inputs,
        }
    }

    /// Resolve the run of the command line with `change` on its inputs.
    pub(super) fn resolve(
        &self,
        change: impl FnOnce(&mut fallow_config::RunInputs),
    ) -> Result<fallow_engine::resolved_run::ResolvedRun, String> {
        self.resolve_inputs(change).map_err(|err| {
            crate::cli_production::resolve_error_message(&err, self.config_path.as_deref())
        })
    }

    fn resolve_inputs(
        &self,
        change: impl FnOnce(&mut fallow_config::RunInputs),
    ) -> Result<fallow_engine::resolved_run::ResolvedRun, ResolveError> {
        let mut inputs = self.run_inputs.cloned().unwrap_or_else(|| {
            crate::cli_production::benchmark_inputs(
                self.root,
                fallow_config::CommandKind::CoverageAnalyze,
                self.threads,
            )
        });
        change(&mut inputs);
        fallow_engine::resolved_run::resolve_run(&inputs)
    }

    /// The run of an upload: no parse cache and no progress output, as the
    /// upload reads the whole project once. A config file that does not load
    /// gives `config load failed:` and the error of the parser.
    pub(super) fn upload_run(&self) -> Result<fallow_engine::resolved_run::ResolvedRun, String> {
        self.resolve_inputs(|inputs| {
            inputs.no_cache = Some(fallow_types::sourced::Given {
                value: true,
                origin: fallow_types::sourced::Origin::Flag("--no-cache"),
            });
            inputs.quiet = true;
        })
        .map_err(|err| {
            let message = match &err {
                ResolveError::ConfigLoad(load) => load.message().to_owned(),
                _ => err.message(),
            };
            format!("config load failed: {message}")
        })
    }
}
