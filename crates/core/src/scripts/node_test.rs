//! The Node.js test runner (`node --test`).
//!
//! Without file arguments, `node --test` searches the working directory for
//! files that match its default patterns and runs each one. Those files are
//! entry points of the package that runs the script. With file arguments,
//! Node runs only the files that the arguments name.

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
    fn other_binaries_never_use_default_patterns() {
        assert!(default_test_patterns("tsx", &["--test"]).is_empty());
        assert!(default_test_patterns("bun", &["--test"]).is_empty());
    }
}
