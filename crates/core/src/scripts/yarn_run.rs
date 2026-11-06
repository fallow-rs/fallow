//! Flags and shell forms of `yarn run`.
//!
//! Yarn 2 and later puts a `run` command on the `PATH` of each package script,
//! so a script body `run -T jest` is the same call as `yarn run -T jest`.

/// `yarn run` flags without a value. `-s` and `--silent` are Yarn 1 flags.
/// `-T` (`--top-level`) looks up the binary in the root workspace, and `-B`
/// (`--binaries-only`) skips the scripts. Both keep the working directory of
/// the calling package.
const BOOLEAN_FLAGS: &[&str] = &[
    "-s",
    "--silent",
    "-T",
    "--top-level",
    "-B",
    "--binaries-only",
    "--inspect",
    "--inspect-brk",
];

/// `yarn run` flags that take a value, as the next token or after `=`.
const VALUE_FLAGS: &[&str] = &["--require"];

/// Flags that take an optional value, which must then follow `=`.
const OPTIONAL_VALUE_FLAGS: &[&str] = &["--inspect", "--inspect-brk"];

/// Whether the token at `idx` is the `run` command that Yarn 2 and later
/// puts on the `PATH` of a package script.
pub(super) fn is_shell_run(tokens: &[&str], idx: usize) -> bool {
    tokens.get(idx) == Some(&"run")
}

/// Flags of `yarn run` that skip the scripts of the calling package: `-T`
/// looks in the root workspace, and `-B` runs only binaries.
const LOCAL_SCRIPT_SKIP_FLAGS: &[&str] = &["-T", "--top-level", "-B", "--binaries-only"];

/// Whether the tokens between `run` and the name hold a flag that skips the
/// scripts of the calling package.
pub(super) fn skips_local_scripts(flags: &[&str]) -> bool {
    flags
        .iter()
        .any(|flag| LOCAL_SCRIPT_SKIP_FLAGS.contains(flag))
}

impl super::PackageManagerRun<'_> {
    /// Whether the call runs a script of `catalog` instead of a binary.
    pub(super) fn declares_script(&self, catalog: &super::ScriptCatalog) -> bool {
        !self.skips_local_scripts && catalog.declares_script_at(self.name, &self.location)
    }
}

/// Return the index of the first token from `idx` that is not a `yarn run`
/// flag, or the value of such a flag.
pub(super) fn skip_run_flags(tokens: &[&str], mut idx: usize) -> usize {
    while let Some(&token) = tokens.get(idx) {
        if BOOLEAN_FLAGS.contains(&token) {
            idx += 1;
        } else if VALUE_FLAGS.contains(&token) {
            idx += 2;
        } else if token.split_once('=').is_some_and(|(flag, _)| {
            VALUE_FLAGS.contains(&flag) || OPTIONAL_VALUE_FLAGS.contains(&flag)
        }) {
            idx += 1;
        } else {
            break;
        }
    }
    idx.min(tokens.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_each_run_flag_and_its_value() {
        let tokens = [
            "-T",
            "--binaries-only",
            "--inspect=9229",
            "--require",
            "./hook.js",
            "--require=./other.js",
            "-s",
            "jest",
            "-T",
        ];
        assert_eq!(skip_run_flags(&tokens, 0), 7);
    }

    #[test]
    fn a_value_flag_without_a_value_stops_at_the_end() {
        assert_eq!(skip_run_flags(&["--require"], 0), 1);
    }

    #[test]
    fn only_a_bare_run_token_is_the_shell_run() {
        assert!(is_shell_run(&["run", "-T", "jest"], 0));
        assert!(!is_shell_run(&["yarn", "run", "jest"], 0));
        assert!(!is_shell_run(&[], 0));
    }
}
