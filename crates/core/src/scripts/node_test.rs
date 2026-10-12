//! The built-in test runners of JavaScript runtimes: `node --test`,
//! `bun test`, and `deno test`.
//!
//! Without file arguments, `node --test` searches the working directory for
//! files that match its default patterns and runs each one. Those files are
//! entry points of the package that runs the script. With file arguments,
//! Node runs only the files that the arguments name.
//!
//! The files that a test run names are test entry points, so the code that
//! they import counts as reached by tests.

/// The files that `node --test` runs when the command names no file:
/// <https://nodejs.org/api/test.html#running-tests-from-the-command-line>.
/// The TypeScript extensions apply when Node strips types.
const NODE_TEST_DEFAULT_PATTERNS: &[&str] = &[
    "**/*.test.{js,mjs,cjs,ts,mts,cts}",
    "**/*-test.{js,mjs,cjs,ts,mts,cts}",
    "**/*_test.{js,mjs,cjs,ts,mts,cts}",
    "**/test-*.{js,mjs,cjs,ts,mts,cts}",
    "**/test.{js,mjs,cjs,ts,mts,cts}",
    "**/test/**/*.{js,mjs,cjs,ts,mts,cts}",
];

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

/// Return the default test file patterns when `binary` is `node`, `args`
/// contain the `--test` flag, and no argument names a test file.
///
/// `args` are the tokens after the binary.
pub(super) fn default_test_patterns(binary: &str, args: &[&str]) -> Vec<String> {
    if binary != "node" || !runs_default_test_files(args) {
        return Vec::new();
    }
    NODE_TEST_DEFAULT_PATTERNS
        .iter()
        .map(|pattern| (*pattern).to_string())
        .collect()
}

/// Return `true` when `binary` with `args` runs tests with the built-in
/// test runner of the runtime: `node --test`, `bun test`, or `deno test`.
///
/// `args` are the tokens after the binary. Node reads `--test` only before
/// the script path, because later tokens go to the script.
pub(super) fn runs_tests(binary: &str, args: &[&str]) -> bool {
    match binary {
        "node" => node_test_flag_precedes_script(args),
        "bun" | "deno" => first_positional(args) == Some("test"),
        _ => false,
    }
}

fn node_test_flag_precedes_script(args: &[&str]) -> bool {
    let mut idx = 0;
    while idx < args.len() {
        let token = args[idx];
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

/// The first token that is not a flag. Bun and Deno runtime flags before
/// the subcommand take no separate value.
fn first_positional<'a>(args: &[&'a str]) -> Option<&'a str> {
    args.iter().copied().find(|token| !token.starts_with('-'))
}

fn runs_default_test_files(args: &[&str]) -> bool {
    let mut test_mode = false;
    let mut idx = 0;
    while idx < args.len() {
        let token = args[idx];
        if token == "--test" {
            test_mode = true;
        } else if NODE_VALUE_FLAGS.contains(&token) {
            idx += 1;
        } else if !token.starts_with('-') {
            return false;
        }
        idx += 1;
    }
    test_mode
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(script_args: &str) -> Vec<String> {
        let args: Vec<&str> = script_args.split_whitespace().collect();
        default_test_patterns("node", &args)
    }

    #[test]
    fn bare_test_flag_uses_default_patterns() {
        let found = patterns("--test");
        assert_eq!(found.len(), NODE_TEST_DEFAULT_PATTERNS.len());
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
    fn other_binaries_never_use_default_patterns() {
        assert!(default_test_patterns("tsx", &["--test"]).is_empty());
        assert!(default_test_patterns("bun", &["--test"]).is_empty());
    }
}
