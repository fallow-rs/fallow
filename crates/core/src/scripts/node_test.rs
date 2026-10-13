//! The built-in test runners of JavaScript runtimes: `node --test`,
//! `bun test`, and `deno test`.
//!
//! Without file arguments, each runner searches the working directory for
//! files that match its default patterns and runs each one. With a directory
//! argument, the runner searches only that directory. With file arguments,
//! the runner runs only the files that the arguments name. The files that a
//! test run finds are entry points of the package that runs the script.
//!
//! The files that a test run names are test entry points, so the code that
//! they import counts as reached by tests.
//!
//! A directory argument is told apart from a file or a glob by its syntax,
//! not by a filesystem check: script arguments resolve against the package
//! directory and `cd` targets only later. An argument that ends with `/`, or
//! whose last segment has no extension and no glob character, is a
//! directory. A pattern for a directory that does not exist matches nothing.
//! Thus an extensionless file argument, such as `node --test scripts/run`,
//! counts as a directory and gets no entry-point credit.
//!
//! The `node --test` directory search follows Node 20. Node 21 and later
//! read each positional argument as a glob and do not search a directory, so
//! `node --test test/` reports an error there.

/// The source extensions of the `node --test` defaults. The TypeScript
/// extensions apply when Node strips types.
const NODE_EXTENSIONS: &str = "{js,mjs,cjs,ts,mts,cts}";

/// The file name patterns that `node --test` runs in every directory:
/// <https://nodejs.org/api/test.html#running-tests-from-the-command-line>.
const NODE_TEST_FILE_NAMES: &[&str] = &["*.test", "*-test", "*_test", "test-*", "test"];

/// The directory in which `node --test` runs every source file.
const NODE_TEST_DIRECTORY: &str = "test";

/// The file name patterns that `bun test` runs:
/// <https://bun.com/docs/cli/test>.
const BUN_TEST_FILE_NAMES: &[&str] = &["*.test", "*_test", "*.spec", "*_spec"];

/// The source extensions of the `bun test` defaults.
const BUN_EXTENSIONS: &str = "{js,jsx,ts,tsx,mjs,cjs,mts,cts}";

/// The file name patterns that `deno test` runs, the glob
/// `{*_,*.,}test.{ts,tsx,mts,js,mjs,jsx}`:
/// <https://docs.deno.com/runtime/fundamentals/testing/>.
const DENO_TEST_FILE_NAMES: &[&str] = &["*_test", "*.test", "test"];

/// The source extensions of the `deno test` defaults.
const DENO_EXTENSIONS: &str = "{ts,tsx,mts,js,mjs,jsx}";

/// Node flags that take their value from the next token when the command
/// does not write `--flag=value`. The value is not a test file argument.
const NODE_VALUE_FLAGS: &[&str] = &[
    "-C",
    "-r",
    "--conditions",
    "--disable-warning",
    "--env-file",
    "--env-file-if-exists",
    "--experimental-config-file",
    "--experimental-loader",
    "--import",
    "--input-type",
    "--loader",
    "--require",
    "--test-concurrency",
    "--test-coverage-branches",
    "--test-coverage-exclude",
    "--test-coverage-functions",
    "--test-coverage-include",
    "--test-coverage-lines",
    "--test-global-setup",
    "--test-isolation",
    "--test-name-pattern",
    "--test-reporter",
    "--test-reporter-destination",
    "--test-shard",
    "--test-skip-pattern",
    "--test-timeout",
    "--watch-path",
];

/// Node flags whose value names a module that Node loads. The value is an
/// entry file of the command.
const NODE_MODULE_VALUE_FLAGS: &[&str] = &[
    "-r",
    "--experimental-loader",
    "--import",
    "--loader",
    "--require",
    "--test-global-setup",
    "--test-reporter",
];

/// Node flags whose module runs in the same process as the test files. Only
/// the values of these flags are test entry points. A reporter and a global
/// setup module run in the parent process, and a loader runs in its own
/// thread.
const NODE_TEST_PROCESS_FLAGS: &[&str] = &["-r", "--import", "--require"];

/// Return `true` when the Node flag `token` takes the next token as a value
/// that names no module, such as a glob of `--test-coverage-include`. That
/// value is not a file argument.
pub(super) fn takes_non_module_value(token: &str) -> bool {
    NODE_VALUE_FLAGS.contains(&token) && !NODE_MODULE_VALUE_FLAGS.contains(&token)
}

/// Return the files of `file_args` that the command runs as tests.
///
/// The result is empty when `binary` with `args` does not run the built-in
/// test runner. The value of a flag is not a test file, unless the module
/// runs in the test process (`--import`, `--require`).
pub(super) fn test_file_args(binary: &str, args: &[&str], file_args: &[String]) -> Vec<String> {
    if !runs_tests(binary, args) {
        return Vec::new();
    }
    let flag_values: Vec<&str> = args
        .windows(2)
        .filter(|pair| NODE_VALUE_FLAGS.contains(&pair[0]))
        .filter(|pair| !NODE_TEST_PROCESS_FLAGS.contains(&pair[0]))
        .map(|pair| pair[1])
        .collect();
    file_args
        .iter()
        .filter(|file| !flag_values.contains(&file.as_str()))
        .cloned()
        .collect()
}

/// Return `file_args` with the test files that the test run finds by the
/// default patterns of its runner.
///
/// A directory argument becomes the default patterns inside that directory.
/// A run with no positional argument gets the default patterns of the
/// working directory. Bun reads a bare word such as `bun test parser` as a
/// filter on the path of a test file. A glob cannot express that filter, so
/// a filter adds no pattern and turns off the defaults. A directory argument
/// of Bun is also a filter, and the directory patterns match a subset of the
/// files that it selects.
///
/// `args` are the tokens after the binary. The result is `file_args`
/// unchanged when the command does not run a test runner.
pub(super) fn with_default_test_patterns(
    binary: &str,
    args: &[&str],
    mut file_args: Vec<String>,
) -> Vec<String> {
    let Some(runner) = TestRunner::of(binary, args) else {
        return file_args;
    };
    let positionals: Vec<&str> = runner
        .positionals(args)
        .into_iter()
        .filter(|token| !is_number(token))
        .collect();
    if positionals.is_empty() {
        // tsx runs the Node test runner, but a bare `tsx --test` does not
        // show which files it loads.
        if binary != "tsx" {
            file_args.extend(runner.patterns_in(""));
        }
        return file_args;
    }
    let directories: Vec<&str> = positionals
        .into_iter()
        .filter(|token| runner != TestRunner::Bun || !is_bun_name_filter(token))
        .filter(|token| is_directory_arg(token))
        .collect();
    file_args.retain(|file| !directories.contains(&file.as_str()));
    for directory in directories {
        file_args.extend(runner.patterns_in(directory_prefix(directory)));
    }
    file_args
}

/// A built-in test runner with default test file patterns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum TestRunner {
    Node,
    Bun,
    Deno,
}

impl TestRunner {
    /// Return the runner when `binary` with `args` runs tests.
    fn of(binary: &str, args: &[&str]) -> Option<Self> {
        if !runs_tests(binary, args) {
            return None;
        }
        match binary {
            "node" | "tsx" => Some(Self::Node),
            "bun" => Some(Self::Bun),
            "deno" => Some(Self::Deno),
            _ => None,
        }
    }

    const fn file_names(self) -> &'static [&'static str] {
        match self {
            Self::Node => NODE_TEST_FILE_NAMES,
            Self::Bun => BUN_TEST_FILE_NAMES,
            Self::Deno => DENO_TEST_FILE_NAMES,
        }
    }

    const fn extensions(self) -> &'static str {
        match self {
            Self::Node => NODE_EXTENSIONS,
            Self::Bun => BUN_EXTENSIONS,
            Self::Deno => DENO_EXTENSIONS,
        }
    }

    /// Return the default test file patterns inside `dir`. An empty `dir`
    /// is the working directory.
    fn patterns_in(self, dir: &str) -> Vec<String> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{dir}/")
        };
        let extensions = self.extensions();
        let mut patterns: Vec<String> = self
            .file_names()
            .iter()
            .map(|name| format!("{prefix}**/{name}.{extensions}"))
            .collect();
        if self == Self::Node {
            patterns.push(format!(
                "{prefix}**/{NODE_TEST_DIRECTORY}/**/*.{extensions}"
            ));
            // Node runs every source file below a directory named `test`.
            // Only the argument itself counts here: an ancestor named `test`
            // has no effect, and the pattern above covers `test`
            // directories below the argument.
            if dir.rsplit('/').next() == Some(NODE_TEST_DIRECTORY) {
                patterns.push(format!("{prefix}**/*.{extensions}"));
            }
        }
        patterns
    }

    /// Return the positional arguments of the test run: the tokens after
    /// the flags, the flag values and, for Bun and Deno, the `test`
    /// subcommand. For Bun and Deno, the tokens after `--` go to the test
    /// process, so the scan stops there. Node can read them as files.
    fn positionals<'a>(self, args: &[&'a str]) -> Vec<&'a str> {
        let (start, value_flags) = match self {
            Self::Node => (0, NODE_VALUE_FLAGS),
            Self::Bun => (
                subcommand_index(args).map_or(args.len(), |idx| idx + 1),
                BUN_TEST_VALUE_FLAGS,
            ),
            Self::Deno => (
                subcommand_index(args).map_or(args.len(), |idx| idx + 1),
                DENO_TEST_VALUE_FLAGS,
            ),
        };
        let mut positionals = Vec::new();
        let mut idx = start;
        while let Some(&token) = args.get(idx) {
            if token == "--" && self != Self::Node {
                break;
            }
            if value_flags.contains(&token) {
                idx += 1;
            } else if !token.starts_with('-') {
                positionals.push(token);
            }
            idx += 1;
        }
        positionals
    }
}

/// Bun test flags that take their value from the next token:
/// <https://bun.com/docs/cli/test>.
const BUN_TEST_VALUE_FLAGS: &[&str] = &[
    "-c",
    "-d",
    "-l",
    "-r",
    "-t",
    "--conditions",
    "--config",
    "--coverage-dir",
    "--coverage-reporter",
    "--cwd",
    "--define",
    "--env-file",
    "--loader",
    "--max-concurrency",
    "--preload",
    "--reporter",
    "--reporter-outfile",
    "--rerun-each",
    "--retry",
    "--seed",
    "--test-name-pattern",
    "--timeout",
    "--tsconfig-override",
];

/// Deno test flags that take their value from the next token.
const DENO_TEST_VALUE_FLAGS: &[&str] = &[
    "-L",
    "-c",
    "--cert",
    "--config",
    "--ext",
    "--filter",
    "--import-map",
    "--junit-path",
    "--location",
    "--log-level",
    "--preload",
    "--reporter",
    "--seed",
];

/// Return `true` when `token` names a directory by its syntax: it ends with
/// `/`, or its last segment has no extension and no glob character.
fn is_directory_arg(token: &str) -> bool {
    if token.contains(['*', '?', '[', '{']) {
        return false;
    }
    if token.ends_with('/') {
        return true;
    }
    let last = token.rsplit('/').next().unwrap_or(token);
    matches!(last, "." | "..") || !last.trim_start_matches('.').contains('.')
}

/// Return `true` when Bun reads `token` as a filter, not as a path: it has
/// no path separator, no leading `.`, and no extension.
fn is_bun_name_filter(token: &str) -> bool {
    !token.contains('/') && !token.starts_with('.') && !token.contains('.')
}

/// The value of a flag with an optional count, such as `--bail 1`.
fn is_number(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_digit())
}

/// Return the pattern prefix of a directory argument: `src/` gives `src`,
/// and `.` gives the working directory.
fn directory_prefix(directory: &str) -> &str {
    match directory.trim_end_matches('/') {
        "." | "./" => "",
        dir => dir,
    }
}

/// Return `true` when `binary` with `args` runs tests with the built-in
/// test runner of the runtime: `node --test`, `bun test`, or `deno test`.
///
/// `args` are the tokens after the binary. Node reads `--test` only before
/// the script path, because later tokens go to the script.
pub(super) fn runs_tests(binary: &str, args: &[&str]) -> bool {
    match binary {
        // tsx gives the Node flags before the script path to Node.
        "node" | "tsx" => has_leading_test_flag(args),
        "bun" | "deno" => {
            subcommand_index(args).and_then(|idx| args.get(idx).copied()) == Some("test")
        }
        _ => false,
    }
}

/// Return `true` when the Node flags before the script path contain
/// `--test`.
fn has_leading_test_flag(args: &[&str]) -> bool {
    let mut idx = 0;
    while let Some(&token) = args.get(idx) {
        if token == "--test" {
            return true;
        }
        if NODE_VALUE_FLAGS.contains(&token) {
            idx += 1;
        } else if !token.starts_with('-') {
            return false;
        }
        idx += 1;
    }
    false
}

/// Bun and Deno flags that take their value from the next token before the
/// subcommand.
const BUN_DENO_VALUE_FLAGS: &[&str] = &["-c", "--config", "--cwd"];

/// The index of the first token that is not a flag or the value of a flag.
fn subcommand_index(args: &[&str]) -> Option<usize> {
    let mut idx = 0;
    while let Some(&token) = args.get(idx) {
        if !token.starts_with('-') {
            return Some(idx);
        }
        idx += if BUN_DENO_VALUE_FLAGS.contains(&token) {
            2
        } else {
            1
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(script_args: &str) -> Vec<String> {
        let args: Vec<&str> = script_args.split_whitespace().collect();
        with_default_test_patterns("node", &args, Vec::new())
    }

    fn run_patterns(binary: &str, script_args: &str) -> Vec<String> {
        let args: Vec<&str> = script_args.split_whitespace().collect();
        let file_args = args
            .iter()
            .filter(|token| token.contains('/') || token.contains(".t"))
            .map(|token| (*token).to_string())
            .collect();
        with_default_test_patterns(binary, &args, file_args)
    }

    #[test]
    fn bare_test_flag_uses_default_patterns() {
        let found = patterns("--test");
        assert_eq!(found.len(), NODE_TEST_FILE_NAMES.len() + 1);
        assert!(found.contains(&"**/test/**/*.{js,mjs,cjs,ts,mts,cts}".to_string()));
    }

    #[test]
    fn boolean_flags_keep_default_patterns() {
        for args in [
            "--experimental-strip-types --test",
            "--test --watch",
            "--test --experimental-test-coverage --test-only",
            "--test --test-reporter=spec",
        ] {
            assert!(!patterns(args).is_empty(), "`node {args}`");
        }
    }

    #[test]
    fn flag_values_are_not_test_file_arguments() {
        for args in [
            "--test --import tsx",
            "--import ./setup.ts --test",
            "--test --test-reporter spec --test-reporter-destination stdout",
            "--test --test-concurrency 1",
            "--test --test-name-pattern adds",
            "-r ./register.cjs --test",
        ] {
            assert!(!patterns(args).is_empty(), "`node {args}`");
        }
    }

    #[test]
    fn explicit_file_arguments_replace_default_patterns() {
        assert!(patterns("--test test/only.test.ts").is_empty());
        assert!(patterns("--test --import tsx src/**/*.test.ts").is_empty());
    }

    #[test]
    fn test_prefixed_flags_alone_do_not_enable_test_mode() {
        assert!(patterns("--test-reporter=spec").is_empty());
        assert!(patterns("--test-name-pattern adds").is_empty());
        assert!(patterns("server.js").is_empty());
    }

    #[test]
    fn node_test_flag_marks_a_test_run() {
        for args in [
            "--test",
            "--test test/a.test.mjs",
            "--test --test-reporter=spec test/a.test.mjs",
            "--import tsx --test src/**/*.test.ts",
            "--experimental-test-coverage --test a.test.mjs b.test.mjs",
        ] {
            let tokens: Vec<&str> = args.split_whitespace().collect();
            assert!(runs_tests("node", &tokens), "`node {args}`");
        }
    }

    #[test]
    fn test_flag_after_the_script_is_a_script_argument() {
        for args in [
            "scripts/run.mjs --test",
            "--test-only a.test.mjs",
            "--test-reporter=spec a.test.mjs",
            "--import tsx scripts/run.ts --test",
        ] {
            let tokens: Vec<&str> = args.split_whitespace().collect();
            assert!(!runs_tests("node", &tokens), "`node {args}`");
        }
    }

    #[test]
    fn bun_and_deno_test_subcommands_mark_a_test_run() {
        assert!(runs_tests("bun", &["test", "./test/a.test.ts"]));
        assert!(runs_tests("bun", &["--bun", "test"]));
        assert!(runs_tests("deno", &["test", "--allow-read", "a_test.ts"]));
        assert!(!runs_tests("bun", &["run", "test"]));
        assert!(!runs_tests("bun", &["scripts/test.ts"]));
        assert!(!runs_tests("deno", &["run", "test.ts"]));
        assert!(!runs_tests("tsx", &["test"]));
    }

    #[test]
    fn subcommand_after_a_flag_value_marks_a_test_run() {
        assert!(runs_tests("bun", &["--cwd", ".", "test", "./a.test.mjs"]));
        assert!(runs_tests("deno", &["--config", "deno.json", "test"]));
        assert!(!runs_tests("bun", &["--cwd", "test", "run", "a.ts"]));
    }

    #[test]
    fn tsx_test_flag_marks_a_test_run() {
        assert!(runs_tests("tsx", &["--test", "test/a.test.ts"]));
        assert!(!runs_tests("tsx", &["scripts/run.ts", "--test"]));
    }

    #[test]
    fn other_binaries_never_use_default_patterns() {
        assert!(with_default_test_patterns("tsx", &["--test"], Vec::new()).is_empty());
        assert!(with_default_test_patterns("bun", &["--test"], Vec::new()).is_empty());
        assert!(with_default_test_patterns("deno", &["run", "main.ts"], Vec::new()).is_empty());
    }

    #[test]
    fn bare_bun_and_deno_test_runs_use_their_default_patterns() {
        assert_eq!(
            run_patterns("bun", "test --coverage --timeout 5000 --bail 1"),
            vec![
                "**/*.test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
                "**/*_test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
                "**/*.spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
                "**/*_spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
            ]
        );
        assert_eq!(
            run_patterns("deno", "test -A --filter adds"),
            vec![
                "**/*_test.{ts,tsx,mts,js,mjs,jsx}",
                "**/*.test.{ts,tsx,mts,js,mjs,jsx}",
                "**/test.{ts,tsx,mts,js,mjs,jsx}",
            ]
        );
    }

    #[test]
    fn directory_arguments_scope_the_default_patterns() {
        assert_eq!(
            run_patterns("bun", "test src/"),
            vec![
                "src/**/*.test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
                "src/**/*_test.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
                "src/**/*.spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
                "src/**/*_spec.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
            ]
        );
        assert_eq!(
            run_patterns("deno", "test --allow-read tests"),
            vec![
                "tests/**/*_test.{ts,tsx,mts,js,mjs,jsx}",
                "tests/**/*.test.{ts,tsx,mts,js,mjs,jsx}",
                "tests/**/test.{ts,tsx,mts,js,mjs,jsx}",
            ]
        );
        assert_eq!(
            run_patterns("deno", "test ."),
            run_patterns("deno", "test"),
            "`.` is the working directory"
        );
    }

    #[test]
    fn node_runs_every_source_file_below_a_test_directory() {
        let found = run_patterns("node", "--test test/");
        assert!(found.contains(&"test/**/*.{js,mjs,cjs,ts,mts,cts}".to_string()));
        let found = run_patterns("node", "--test spec/unit");
        assert!(found.contains(&"spec/unit/**/*.test.{js,mjs,cjs,ts,mts,cts}".to_string()));
        assert!(!found.contains(&"spec/unit/**/*.{js,mjs,cjs,ts,mts,cts}".to_string()));
    }

    #[test]
    fn node_test_ancestor_directory_does_not_run_every_source_file() {
        let found = run_patterns("node", "--test test/unit");
        assert!(found.contains(&"test/unit/**/*.test.{js,mjs,cjs,ts,mts,cts}".to_string()));
        assert!(found.contains(&"test/unit/**/test/**/*.{js,mjs,cjs,ts,mts,cts}".to_string()));
        assert!(!found.contains(&"test/unit/**/*.{js,mjs,cjs,ts,mts,cts}".to_string()));
        let found = run_patterns("node", "--test packages/core/test");
        assert!(found.contains(&"packages/core/test/**/*.{js,mjs,cjs,ts,mts,cts}".to_string()));
    }

    fn keeps_defaults(binary: &str, script_args: &str) -> bool {
        let found = run_patterns(binary, script_args);
        run_patterns(binary, "test")
            .iter()
            .all(|pattern| found.contains(pattern))
    }

    #[test]
    fn bun_and_deno_value_flags_are_not_directories() {
        for (binary, args) in [
            ("bun", "test --env-file .env.test"),
            (
                "bun",
                "test --define process.env.CI:true --conditions development",
            ),
            (
                "bun",
                "test -l .svg:file --tsconfig-override tsconfig.test.json",
            ),
            ("deno", "test -A --log-level debug"),
            ("deno", "test -A -L info"),
        ] {
            assert!(keeps_defaults(binary, args), "`{binary} {args}`");
        }
    }

    #[test]
    fn bun_and_deno_arguments_after_a_terminator_go_to_the_tests() {
        assert_eq!(
            run_patterns("deno", "test -A -- update"),
            run_patterns("deno", "test")
        );
        assert_eq!(
            run_patterns("bun", "test -- fixtures"),
            run_patterns("bun", "test")
        );
        assert_eq!(
            run_patterns("deno", "test tests -- update"),
            run_patterns("deno", "test tests")
        );
    }

    #[test]
    fn file_and_glob_arguments_keep_their_form() {
        for (binary, args, expected) in [
            ("bun", "test ./test/a.test.ts", vec!["./test/a.test.ts"]),
            ("deno", "test tests/a_test.ts", vec!["tests/a_test.ts"]),
            (
                "node",
                "--test test/**/*.test.mjs",
                vec!["test/**/*.test.mjs"],
            ),
        ] {
            assert_eq!(run_patterns(binary, args), expected, "`{binary} {args}`");
        }
    }

    #[test]
    fn bun_name_filters_add_no_patterns() {
        assert!(run_patterns("bun", "test parser").is_empty());
        assert!(run_patterns("bun", "test parser -t adds").is_empty());
    }

    #[test]
    fn flag_values_are_not_directories() {
        assert_eq!(
            run_patterns("bun", "test --preload ./setup"),
            vec!["./setup".to_string()]
                .into_iter()
                .chain(run_patterns("bun", "test"))
                .collect::<Vec<_>>()
        );
        assert!(
            !run_patterns("node", "--test --import ./register test/a.test.mjs")
                .iter()
                .any(|pattern| pattern.starts_with("./register/"))
        );
    }
}
