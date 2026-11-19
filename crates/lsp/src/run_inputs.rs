//! The run inputs of an editor session: the one place that maps the
//! initialization options of the client to [`RunInputs`].

use std::path::{Path, PathBuf};

use fallow_config::{CommandKind, ConfigFallback, EnvSnapshot, RunInputs, SelectionInput, Surface};
use fallow_types::sourced::{Given, Origin};

use crate::initialization::LspInitializationOptions;

/// The origin of the selection of an editor session. The editor always asks
/// for dead code and duplication, and the config decides on duplication.
const EDITOR_SELECTION: Origin = Origin::EditorSetting("editor-session");

const fn setting<T>(value: T, name: &'static str) -> Given<T> {
    Given {
        value,
        origin: Origin::EditorSetting(name),
    }
}

/// The run inputs of the editor session of `workspace_root`.
///
/// The run of an editor session plans dead code and duplication, never
/// health. A discovered config that fails gives the default config; an
/// explicit `configPath` that fails is an error. A relative `configPath`
/// resolves from `workspace_root`, because the snapshot has it as its
/// current directory. `allowRemoteExtends: false` and
/// `packageBaselines: true` are the defaults, so only the other value is a
/// given input. The duplication, `changedSince` and type-aware options apply
/// per run and are not inputs of the resolved run.
pub fn run_inputs(
    options: &LspInitializationOptions,
    workspace_root: &Path,
    env: EnvSnapshot,
) -> RunInputs {
    let mut inputs = RunInputs::new(
        Surface::Lsp,
        env.with_cwd(workspace_root.to_path_buf()),
        CommandKind::Editor,
        SelectionInput::Booleans {
            dead_code: true,
            duplication: true,
            health: false,
            origin: EDITOR_SELECTION,
        },
    );
    inputs.config_fallback = ConfigFallback::DefaultsOnDiscoveredError;
    inputs.config_path = options
        .config_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| setting(PathBuf::from(path), "fallow.configPath"));
    inputs.allow_remote_extends = options
        .allow_remote_extends
        .then(|| setting(true, "fallow.allowRemoteExtends"));
    inputs.production = options
        .production
        .map(|on| setting(on, "fallow.production"));
    inputs.package_baselines = (options.package_baselines == Some(false))
        .then(|| setting(false, "fallow.packageBaselines"));
    inputs
}

#[cfg(test)]
#[path = "run_inputs_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "run_parity_tests.rs"]
mod parity_tests;
