//! Commands that a tool config runs as named tasks: the Nx `run-commands`
//! targets of `project.json` and the `run.tasks` of a Vite+ config.

#![expect(
    clippy::disallowed_types,
    reason = "joins the std HashMap of package.json scripts"
)]

use std::collections::HashMap;
use std::path::Path;

/// The task commands of the package in `dir`, keyed `nx:<target>` and
/// `vp:<task>` so they never collide with a `package.json` script.
/// `project_root` is the package directory relative to the project root.
/// Production mode keeps only the tasks with a production script name.
#[must_use]
pub fn config_task_scripts(
    dir: &Path,
    project_root: &str,
    production: bool,
) -> HashMap<String, String> {
    let nx_targets: HashMap<String, String> = std::fs::read_to_string(dir.join("project.json"))
        .map(|source| {
            super::nx_targets::run_commands_scripts(&source, project_root)
                .into_iter()
                .collect()
        })
        .unwrap_or_default();
    let vp_tasks: HashMap<String, String> =
        super::vite_plus::task_commands(dir).into_iter().collect();
    let keep = |tasks: HashMap<String, String>| {
        if production {
            super::filter_production_scripts(&tasks)
        } else {
            tasks
        }
    };
    let prefixed = |prefix: &'static str| {
        move |(name, command): (String, String)| (format!("{prefix}:{name}"), command)
    };
    keep(nx_targets)
        .into_iter()
        .map(prefixed("nx"))
        .chain(keep(vp_tasks).into_iter().map(prefixed("vp")))
        .collect()
}
