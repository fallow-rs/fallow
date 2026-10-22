//! Command wrappers that run a child command after a separator, such as
//! `varlock run -- <command>` and `dotenvx run -- <command>`.

use super::{shell, vite_plus};

struct CommandWrapperSpec {
    binary: &'static str,
    prefix: &'static [&'static str],
    separator: &'static str,
}

/// Known wrappers; `--` alone does not imply that a CLI executes another command.
const COMMAND_WRAPPERS: &[CommandWrapperSpec] = &[
    CommandWrapperSpec {
        // `varlock run [options] -- <command>`: https://varlock.dev/reference/cli/load-and-run/
        binary: "varlock",
        prefix: &["run"],
        separator: "--",
    },
    CommandWrapperSpec {
        // `dotenvx run [options] -- <command>`: https://dotenvx.com/docs/quickstart
        binary: "dotenvx",
        prefix: &["run"],
        separator: "--",
    },
];

/// Return the first token of the child command for a known command wrapper.
pub(super) fn command_wrapper_child_index(tokens: &[&str], idx: usize) -> Option<usize> {
    let Some(wrapper) = COMMAND_WRAPPERS.iter().find(|wrapper| {
        tokens.get(idx) == Some(&wrapper.binary)
            && tokens.get(idx + 1..idx + 1 + wrapper.prefix.len()) == Some(wrapper.prefix)
    }) else {
        return vite_plus::exec_child_index(tokens, idx);
    };
    let args_start = idx + 1 + wrapper.prefix.len();
    let separator = tokens[args_start..]
        .iter()
        .position(|token| *token == wrapper.separator)?;
    let child_idx = args_start + separator + 1;

    (child_idx < tokens.len()).then_some(child_idx)
}

pub(super) fn skip_command_wrappers(tokens: &[&str]) -> Option<usize> {
    let mut idx = shell::skip_initial_wrappers(tokens, 0)?;
    while let Some(child_idx) = command_wrapper_child_index(tokens, idx) {
        idx = shell::skip_initial_wrappers(tokens, child_idx)?;
    }
    Some(idx)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::disallowed_types,
        reason = "ScriptCatalog takes the serde-deserialized std HashMap"
    )]

    use std::collections::HashMap;
    use std::path::Path;

    use rustc_hash::{FxHashMap, FxHashSet};

    use super::super::{
        IgnoredCommandEntries, ScriptCatalog, analyze_scripts_with_dependency_context,
    };

    fn package_set(packages: &[&str]) -> FxHashSet<String> {
        packages
            .iter()
            .map(|package| (*package).to_string())
            .collect()
    }

    #[test]
    fn analyze_scripts_with_dependencies_credits_dotenvx_run_binary() {
        for command in [
            "dotenvx run -- is-ci",
            "dotenvx run -f .env.production -- is-ci",
            "dotenvx run --env-file=.env.ci -- pnpm is-ci",
            "pnpm exec dotenvx run -- is-ci",
        ] {
            let scripts = HashMap::from([("ci".to_string(), command.to_string())]);
            let result = analyze_scripts_with_dependency_context(
                &scripts,
                Path::new("/nonexistent"),
                &FxHashMap::default(),
                &package_set(&["@dotenvx/dotenvx", "is-ci"]),
                &ScriptCatalog::from_scripts(&scripts),
                IgnoredCommandEntries::NONE,
            );
            assert_eq!(
                result.used_packages,
                package_set(&["@dotenvx/dotenvx", "is-ci"]),
                "{command}"
            );
        }
    }
}
