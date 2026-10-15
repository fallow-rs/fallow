//! The `borp` test runner, which runs `node:test` on a set of test files.
//!
//! `borp` 1.0 selects the test files in this order (`lib/run.js`):
//!
//! 1. The positional arguments. A positional argument with `*` is a glob,
//!    and a glob that starts with `!` excludes files.
//! 2. The glob of `-p` / `--pattern`, when there is no positional argument.
//! 3. The default glob `**/*.test.{js,mjs,cjs,ts,mts,cts}`.
//!
//! The files that `borp` selects are test entry points. A `.borp.yaml` file
//! can add arguments to the command line. Fallow does not read that file.

/// The name of the `borp` binary.
const BINARY: &str = "borp";

/// The glob that `borp` 1.0 uses when the command gives no file and no
/// pattern. `borp` 0.x uses `**/*.test.{cjs,mjs,js}`, which this glob
/// contains.
const DEFAULT_PATTERN: &str = "**/*.test.{js,mjs,cjs,ts,mts,cts}";

/// The flags whose value is the test file glob.
const PATTERN_FLAGS: &[&str] = &["-p", "--pattern"];

/// The prefix of the `--pattern=<glob>` form.
const PATTERN_PREFIX: &str = "--pattern=";

/// The flags whose value names a module that `borp` loads. The value stays
/// an entry file, but it is not a test file.
const MODULE_VALUE_FLAGS: &[&str] = &["-r", "--reporter"];

/// The flags whose value names no module: a number or a glob that is not a
/// test file glob.
const OTHER_VALUE_FLAGS: &[&str] = &[
    "-c",
    "--concurrency",
    "-t",
    "--timeout",
    "-X",
    "--coverage-exclude",
    "-i",
    "--ignore",
    "--lines",
    "--branches",
    "--functions",
    "--statements",
];

/// Return `true` when `binary` is the `borp` binary, also as a path such as
/// `./node_modules/.bin/borp`.
pub(super) fn is_borp(binary: &str) -> bool {
    binary.rsplit('/').next() == Some(BINARY)
}

/// The arguments of a `borp` command, split by their role.
struct BorpArgs<'a> {
    positionals: Vec<&'a str>,
    pattern: Option<&'a str>,
    /// The values of flags that name no module.
    other_values: Vec<&'a str>,
}

impl<'a> BorpArgs<'a> {
    fn parse(args: &[&'a str]) -> Self {
        let mut parsed = Self {
            positionals: Vec::new(),
            pattern: None,
            other_values: Vec::new(),
        };
        let mut idx = 0;
        while let Some(&token) = args.get(idx) {
            idx += 1;
            if token == "--" {
                parsed
                    .positionals
                    .extend(args.get(idx..).unwrap_or_default().iter().copied());
                break;
            }
            if let Some(value) = token.strip_prefix(PATTERN_PREFIX) {
                parsed.pattern = Some(value);
            } else if PATTERN_FLAGS.contains(&token) {
                parsed.pattern = args.get(idx).copied();
                idx += 1;
            } else if OTHER_VALUE_FLAGS.contains(&token) {
                parsed.other_values.extend(args.get(idx).copied());
                idx += 1;
            } else if MODULE_VALUE_FLAGS.contains(&token) {
                idx += 1;
            } else if !token.starts_with('-') {
                parsed.positionals.push(token);
            }
        }
        parsed
    }

    /// Return the test files and globs that the command runs.
    fn test_files(&self) -> Vec<String> {
        if !self.positionals.is_empty() {
            return self
                .positionals
                .iter()
                .filter(|token| !token.starts_with('!'))
                .map(|token| (*token).to_string())
                .collect();
        }
        vec![self.pattern.unwrap_or(DEFAULT_PATTERN).to_string()]
    }

    /// Return `true` when `value` is a flag value that is not a test file:
    /// a value of a flag that names no module, or a pattern that the
    /// positional arguments override.
    fn is_unused_value(&self, value: &str) -> bool {
        self.other_values.contains(&value)
            || (!self.positionals.is_empty() && self.pattern == Some(value))
    }
}

/// Return the test files and globs that `borp` runs with `args`.
pub(super) fn test_files(args: &[&str]) -> Vec<String> {
    BorpArgs::parse(args).test_files()
}

/// Return `file_args` without the flag values that name no file, plus the
/// test files and globs that `borp` runs with `args`.
pub(super) fn with_test_files(args: &[&str], mut file_args: Vec<String>) -> Vec<String> {
    let parsed = BorpArgs::parse(args);
    file_args.retain(|file| !parsed.is_unused_value(file));
    for file in parsed.test_files() {
        if !file_args.contains(&file) {
            file_args.push(file);
        }
    }
    file_args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_binary_matches_by_file_name() {
        assert!(is_borp("borp"));
        assert!(is_borp("./node_modules/.bin/borp"));
        assert!(!is_borp("borpx"));
        assert!(!is_borp("node"));
    }

    #[test]
    fn positional_arguments_win_over_the_pattern() {
        assert_eq!(
            test_files(&["-p", "src/**/*.js", "test/a.test.js", "!test/b.test.js"]),
            vec!["test/a.test.js"]
        );
        assert_eq!(
            test_files(&["--", "test/a.test.js"]),
            vec!["test/a.test.js"]
        );
    }

    #[test]
    fn the_last_pattern_wins() {
        assert_eq!(
            test_files(&["-p", "a/*.js", "--pattern=b/*.js"]),
            vec!["b/*.js"]
        );
    }

    #[test]
    fn a_command_without_files_gets_the_default_pattern() {
        assert_eq!(
            test_files(&["--coverage", "-c", "1", "-r", "spec"]),
            vec![DEFAULT_PATTERN]
        );
    }
}
