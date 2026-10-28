//! The file arguments of a script command that become entry points
//! (issue #3967).
//!
//! The program of a command decides which of its file arguments are entry
//! points:
//!
//! - A script runtime (`node`, `tsx`, `bun`, `deno run`, a shell) runs one
//!   script: its first positional argument that looks like a script file.
//!   The arguments after the script are data for the script. A relative
//!   module path of a runtime flag that loads code (`-r ./setup.js`,
//!   `--import=./register.js`) stays an entry point. When the script is the
//!   binary of a known file runner (`node node_modules/jest/bin/jest.js`),
//!   the rules of that runner apply.
//! - A known file runner (a test runner, a bundler, a compiler) runs or
//!   loads every file that it gets. All its file arguments stay entry points.
//! - Another program can run a known program (`time node x.js`,
//!   `infisical run -- node x.js`). The rules of the first known program in
//!   its arguments then apply. Without a known program, the literal file
//!   arguments and option values stay entry points, but a glob never
//!   expands: `watchee -f './src/**/*'` gives no entry point.
//!
//! The built-in test runners of the runtimes (`node --test`, `bun test`,
//! `deno test`) and `borp` keep all their file arguments. The formatters and
//! linters of `file_target_tools` and the task runners have their own rules.

use super::{
    borp, file_target_tool_loaded_files, is_file_target_tool, looks_like_file_path, node_test,
    tool_name,
};

/// A runtime that runs one script file and gives the remaining arguments to
/// that script.
struct ScriptRuntime {
    /// Binary names of the runtime.
    names: &'static [&'static str],
    /// Bare words before the script that select a mode of the runtime
    /// (`tsx watch`, `bun run`, `deno run`).
    subcommands: &'static [&'static str],
    /// Flags whose value is the next token and names no module.
    value_flags: &'static [&'static str],
    /// Flags whose value names a module that the runtime loads, as the next
    /// token or after `=`. A relative path value (`./setup.js`) is an entry
    /// point. A bare value (`tsx`, `ts-node/esm`) names a package.
    loading_flags: &'static [&'static str],
}

/// The flags of Node, of the runtimes that give their flags to Node, and of
/// the TypeScript runtimes, whose value names no module.
const NODE_VALUE_FLAGS: &[&str] = &[
    "-C",
    "-O",
    "-P",
    "--compiler-options",
    "--conditions",
    "--disable-warning",
    "--env-file",
    "--env-file-if-exists",
    "--exclude",
    "--experimental-config-file",
    "--ignore",
    "--include",
    "--input-type",
    "--inspect-port",
    "--project",
    "--title",
    "--tsconfig",
    "--unhandled-rejections",
    "--watch-path",
];

/// Node flags whose value names a module that Node loads before the script.
const NODE_LOADING_FLAGS: &[&str] = &[
    "-r",
    "--require",
    "--import",
    "--loader",
    "--experimental-loader",
];

/// The prefixes of a module value that names a file, not a package.
const RELATIVE_PATH_PREFIXES: &[&str] = &["./", "../", "/"];

/// Node flags that run code from the command line. The flag takes the code
/// as its value, and the next positional argument is read as the script.
const NODE_INLINE_CODE_FLAGS: &[&str] = &["-e", "--eval", "-p", "--print"];

/// The extensions of a file that a script runtime runs.
const SCRIPT_EXTENSIONS: &[&str] = &[
    ".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx", ".sh", ".bash",
];

/// The characters that make an argument a glob.
const GLOB_CHARS: &[char] = &['*', '?', '{'];

/// Every [`ScriptRuntime`].
const SCRIPT_RUNTIMES: &[ScriptRuntime] = &[
    ScriptRuntime {
        names: &[
            "node",
            "ts-node",
            "ts-node-esm",
            "babel-node",
            "esno",
            "esr",
            "jiti",
            "node-ts",
            "sucrase-node",
            "swc-node",
            "tsimp",
            "electron",
            "zx",
        ],
        subcommands: &[],
        value_flags: NODE_VALUE_FLAGS,
        loading_flags: NODE_LOADING_FLAGS,
    },
    ScriptRuntime {
        names: &["tsx"],
        subcommands: &["watch"],
        value_flags: NODE_VALUE_FLAGS,
        loading_flags: NODE_LOADING_FLAGS,
    },
    ScriptRuntime {
        names: &["vite-node"],
        subcommands: &["watch"],
        value_flags: &["-r", "--root", "-c", "--config", "-m", "--mode"],
        loading_flags: &[],
    },
    ScriptRuntime {
        names: &["bun"],
        subcommands: &["run"],
        value_flags: &[
            "--cwd",
            "-c",
            "--config",
            "--env-file",
            "--tsconfig-override",
        ],
        loading_flags: &["-r", "--preload"],
    },
    ScriptRuntime {
        names: &["deno"],
        subcommands: &["run"],
        value_flags: &[
            "-c",
            "--config",
            "--import-map",
            "--lock",
            "--cert",
            "--location",
            "--seed",
            "--env-file",
            "-L",
            "--log-level",
        ],
        loading_flags: &[],
    },
    ScriptRuntime {
        names: &["nodemon", "node-dev", "ts-node-dev", "tsnd"],
        subcommands: &[],
        value_flags: &[
            "-w",
            "--watch",
            "-e",
            "--ext",
            "-i",
            "--ignore",
            "--ignore-watch",
            "-x",
            "--exec",
            "-d",
            "--delay",
            "--signal",
            "-r",
            "--require",
            "--config",
            "--project",
        ],
        loading_flags: &[],
    },
    ScriptRuntime {
        names: &["sh", "bash", "zsh", "dash"],
        subcommands: &[],
        value_flags: &["-c", "-o", "+o"],
        loading_flags: &[],
    },
];

/// Programs that run, load, bundle, or compile every file argument that they
/// get: test runners, bundlers, compilers, and process managers. All their
/// file arguments stay entry points.
const FILE_RUNNERS: &[&str] = &[
    // Test runners.
    "ava",
    "brittle",
    "codecept",
    "cucumber-js",
    "cypress",
    "detox",
    "electron-mocha",
    "jasmine",
    "jest",
    "jest-electron",
    "karma",
    "lab",
    "mocha",
    "nightwatch",
    "playwright",
    "poku",
    "qunit",
    "tap",
    "tape",
    "testcafe",
    "ts-mocha",
    "uvu",
    "vitest",
    "wdio",
    "web-test-runner",
    "wtr",
    "zora",
    // Load test runners.
    "artillery",
    "k6",
    // Coverage tools that run a command.
    "c8",
    "nyc",
    // Bundlers and compilers.
    "babel",
    "browserify",
    "bunchee",
    "esbuild",
    "microbundle",
    "ncc",
    "nexe",
    "parcel",
    "pkg",
    "pkgroll",
    "rolldown",
    "rollup",
    "rspack",
    "swc",
    "tsc",
    "tsdown",
    "tsup",
    "unbuild",
    "vite",
    "watchify",
    "webpack",
    "webpack-cli",
    "webpack-dev-server",
    "wrangler",
    // Process managers that run a script.
    "forever",
    "pm2",
    // Infrastructure tools that run a stack file.
    "alchemy",
];

/// Subcommands of a script runtime that bundle, compile, serve, or measure
/// every file argument: `bun build`, `deno compile`, `deno bundle`,
/// `deno serve`, `deno bench`. Their file arguments stay entry points.
const RUNTIME_RUNNER_SUBCOMMANDS: &[(&str, &str)] = &[
    ("bun", "build"),
    ("deno", "bench"),
    ("deno", "bundle"),
    ("deno", "compile"),
    ("deno", "serve"),
];

/// Which arguments of a command name entry points.
#[derive(Debug, PartialEq, Eq)]
pub enum ArgumentEntries<'a> {
    /// Every file argument: the program runs or loads each one.
    All,
    /// Every literal file argument and option value, but no glob: the
    /// program is not known.
    Literal,
    /// Only these arguments: the script of a runtime and the modules that
    /// its flags load.
    Listed(Vec<&'a str>),
    /// Only these files: the modules that the flags of a formatter or linter
    /// load.
    Loaded(Vec<String>),
}

/// Return which arguments of `binary` name entry points. `args` are the
/// tokens after the binary.
#[must_use]
pub fn argument_entries<'a>(binary: &str, args: &[&'a str]) -> ArgumentEntries<'a> {
    let name = tool_name(binary);
    if takes_file_arguments(name) || node_test::runs_tests(name, args) {
        return ArgumentEntries::All;
    }
    if is_file_target_tool(name) {
        return ArgumentEntries::Loaded(file_target_tool_loaded_files(name, args));
    }
    if let Some(runtime) = script_runtime(name) {
        return runtime_entry_args(name, runtime, args);
    }
    // A program that fallow does not know can run a known program.
    match args.iter().position(|token| is_known_program(token)) {
        Some(idx) => argument_entries(args[idx], &args[idx + 1..]),
        None => ArgumentEntries::Literal,
    }
}

/// Return `true` when `binary` runs or loads every file argument that it
/// gets: a test runner, a bundler, or a compiler.
#[must_use]
pub(super) fn takes_file_arguments(binary: &str) -> bool {
    let name = tool_name(binary);
    borp::is_borp(name) || FILE_RUNNERS.contains(&name)
}

/// Return `true` when `binary` is a script runtime such as `node` or `tsx`.
#[must_use]
pub fn is_script_runtime(binary: &str) -> bool {
    script_runtime(tool_name(binary)).is_some()
}

/// Return `true` when `token` is a glob, which fallow never expands from the
/// arguments of a program that it does not know.
#[must_use]
pub fn is_glob(token: &str) -> bool {
    token.contains(GLOB_CHARS)
}

fn script_runtime(name: &str) -> Option<&'static ScriptRuntime> {
    SCRIPT_RUNTIMES
        .iter()
        .find(|runtime| runtime.names.contains(&name))
}

/// Whether `token` names a program that has its own rules.
fn is_known_program(token: &str) -> bool {
    let name = tool_name(token);
    takes_file_arguments(name) || is_file_target_tool(name) || script_runtime(name).is_some()
}

/// Return the file arguments of `binary` that become entry points.
///
/// `args` are the tokens after the binary. `file_args` are the file-like
/// arguments that the caller found. When `binary` is itself a file that a
/// command wrapper runs (`varlock run -- ./worker.js`), that file stays an
/// entry point.
pub(super) fn entry_file_args(binary: &str, args: &[&str], file_args: Vec<String>) -> Vec<String> {
    let executable = file_args.first().filter(|file| file.as_str() == binary);
    let mut entries: Vec<String> = executable.cloned().into_iter().collect();
    match argument_entries(binary, args) {
        ArgumentEntries::All => return file_args,
        ArgumentEntries::Literal => {
            return file_args
                .into_iter()
                .filter(|file| !is_glob(file))
                .collect();
        }
        ArgumentEntries::Listed(listed) => entries.extend(
            listed
                .into_iter()
                .filter(|token| looks_like_file_path(token))
                .map(str::to_string),
        ),
        ArgumentEntries::Loaded(loaded) => entries.extend(loaded),
    }
    entries
}

/// Return the script that `runtime` runs and the modules that its flags
/// load. The script is the first positional argument after an optional
/// subcommand that looks like a script file. Each argument after the script
/// goes to the script.
fn runtime_entry_args<'a>(
    name: &str,
    runtime: &ScriptRuntime,
    args: &[&'a str],
) -> ArgumentEntries<'a> {
    let mut entries = Vec::new();
    let mut idx = 0;
    let mut seen_positional = false;
    while let Some(&token) = args.get(idx) {
        if let Some((width, value)) = loading_flag(runtime, token, args.get(idx + 1).copied()) {
            entries.extend(value);
            idx += width;
            continue;
        }
        if let Some(width) = flag_width(runtime, token) {
            idx += width;
            continue;
        }
        if !seen_positional {
            seen_positional = true;
            if RUNTIME_RUNNER_SUBCOMMANDS.contains(&(name, token)) {
                return ArgumentEntries::All;
            }
            if runtime.subcommands.contains(&token) {
                idx += 1;
                continue;
            }
        }
        if is_script_candidate(token) {
            // `node node_modules/jest/bin/jest.js a.test.js` runs jest.
            if runs_file_runner(token) {
                return ArgumentEntries::All;
            }
            entries.push(token);
            break;
        }
        idx += 1;
    }
    ArgumentEntries::Listed(entries)
}

/// Return `Some` when `token` is a loading flag of `runtime`: the number of
/// tokens that the flag takes, and the module value when it is a relative
/// path. `next` is the token after `token`.
fn loading_flag<'a>(
    runtime: &ScriptRuntime,
    token: &'a str,
    next: Option<&'a str>,
) -> Option<(usize, Option<&'a str>)> {
    let (width, value) = if runtime.loading_flags.contains(&token) {
        (2, next)
    } else {
        let (flag, value) = token.split_once('=')?;
        if !runtime.loading_flags.contains(&flag) {
            return None;
        }
        (1, Some(value))
    };
    let value = value.filter(|value| RELATIVE_PATH_PREFIXES.iter().any(|p| value.starts_with(p)));
    Some((width, value))
}

/// Whether a positional argument of a runtime can be the script that it
/// runs: a file with a script extension, or an extensionless path.
fn is_script_candidate(token: &str) -> bool {
    if is_glob(token) {
        return false;
    }
    if SCRIPT_EXTENSIONS.iter().any(|ext| token.ends_with(ext)) {
        return true;
    }
    token
        .rsplit_once('/')
        .is_some_and(|(_, file)| !file.is_empty() && !file.contains('.'))
}

/// Whether the script `path` is the binary of a known file runner inside
/// `node_modules`: `node_modules/.bin/mocha` or
/// `node_modules/jest/bin/jest.js`.
fn runs_file_runner(path: &str) -> bool {
    if !path.contains("node_modules/") {
        return false;
    }
    let file = tool_name(path);
    let stem = file.split('.').next().unwrap_or(file);
    takes_file_arguments(stem)
}

/// The number of tokens that the flag `token` takes, with its value, or
/// `None` when `token` is not a flag.
fn flag_width(runtime: &ScriptRuntime, token: &str) -> Option<usize> {
    if runtime.value_flags.contains(&token) || NODE_INLINE_CODE_FLAGS.contains(&token) {
        Some(2)
    } else if token.starts_with('-') || token.starts_with('+') {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse_script;

    fn entries(script: &str) -> Vec<String> {
        parse_script(script)
            .into_iter()
            .flat_map(|cmd| cmd.file_args)
            .collect()
    }

    #[test]
    fn runtime_makes_an_entry_of_the_script_only() {
        for script in [
            "node tool.js src/**/*.js",
            "node tool.js --input src/lib/b.js",
            "node --max-old-space-size=4096 tool.js src/a.js src/lib/b.js",
            "tsx tool.js src/a.js",
            "npx tsx watch tool.js src/a.js",
            "ts-node tool.js src/a.js",
            "babel-node tool.js src/a.js",
            "esno tool.js src/a.js",
            "jiti tool.js src/a.js",
            "deno run --allow-read tool.js src/a.js",
            "nodemon --watch src/lib tool.js src/a.js",
            "pnpm exec node tool.js src/a.js",
        ] {
            assert_eq!(entries(script), vec!["tool.js".to_string()], "{script}");
        }
        assert_eq!(
            entries("bun scripts/tool.js src/a.js"),
            vec!["scripts/tool.js".to_string()]
        );
    }

    #[test]
    fn runtime_keeps_the_modules_that_its_flags_load() {
        assert_eq!(
            entries("node --import ./register.js tool.js src/a.js"),
            vec!["./register.js".to_string(), "tool.js".to_string()]
        );
        assert_eq!(
            entries("node --loader ./loader.mjs --experimental-loader ./b.mjs tool.js data.json"),
            vec![
                "./loader.mjs".to_string(),
                "./b.mjs".to_string(),
                "tool.js".to_string()
            ]
        );
        assert_eq!(
            entries("node -r ./setup.js --require tsconfig-paths/register tool.js data.json"),
            vec!["./setup.js".to_string(), "tool.js".to_string()]
        );
        assert_eq!(
            entries("node --import tsx --loader ts-node/esm tool.js data.json"),
            vec!["tool.js".to_string()]
        );
    }

    #[test]
    fn bun_preload_module_stays_an_entry() {
        assert_eq!(
            super::argument_entries(
                "bun",
                &[
                    "--smol",
                    "--preload",
                    "./test/preload.js",
                    "scripts/tool.js",
                    "src/a.js"
                ]
            ),
            super::ArgumentEntries::Listed(vec!["./test/preload.js", "scripts/tool.js"])
        );
    }

    #[test]
    fn runtime_flag_values_are_not_the_script() {
        assert_eq!(
            entries("node --env-file config/.env --watch-path src/lib tool.js src/a.js"),
            vec!["tool.js".to_string()]
        );
        assert_eq!(
            entries("node -r tsconfig-paths/register ./src/server.ts data.json"),
            vec!["./src/server.ts".to_string()]
        );
    }

    #[test]
    fn unknown_program_expands_no_glob() {
        for script in [
            "watchee -f './src/**/*' -c 'echo hi'",
            "pnpm watchee -f './src/**/*' -c 'echo hi'",
            "some-tool 'src/*.js' src/lib/{a,b}.js",
        ] {
            assert_eq!(entries(script), Vec::<String>::new(), "{script}");
        }
    }

    #[test]
    fn unknown_program_keeps_literal_files_and_option_values() {
        for (script, expected) in [
            (
                "jscodeshift -t codemods/transform.js src/a.js",
                &["codemods/transform.js", "src/a.js"][..],
            ),
            (
                "knex migrate:latest --knexfile app/knexfile.js",
                &["app/knexfile.js"],
            ),
            (
                "typeorm migration:run -d app/config/typeorm.ts",
                &["app/config/typeorm.ts"],
            ),
            (
                "gulp build --gulpfile tools/gulpfile.js",
                &["tools/gulpfile.js"],
            ),
            ("truffle exec app/script.js", &["app/script.js"]),
            ("hardhat run tools/deploy.ts", &["tools/deploy.ts"]),
        ] {
            assert_eq!(entries(script), expected, "{script}");
        }
    }

    #[test]
    fn unknown_wrapper_applies_the_rules_of_the_program_that_it_runs() {
        for script in [
            "time node app/x.js data/a.js",
            "env-cmd -f .env node app/x.js data/a.js",
            "infisical run -- node app/x.js data/a.js",
            "op run --env-file .env -- node app/x.js data/a.js",
            "xvfb-run node app/x.js data/a.js",
            "nice -n 10 node app/x.js data/a.js",
            "percy exec -- node app/x.js data/a.js",
            "clinic doctor -- node app/x.js data/a.js",
        ] {
            assert_eq!(entries(script), vec!["app/x.js".to_string()], "{script}");
        }
        assert_eq!(
            entries("percy exec -- cypress run --spec app/a.cy.js"),
            vec!["app/a.cy.js".to_string()]
        );
    }

    #[test]
    fn runtime_flag_values_do_not_hide_the_script() {
        for script in [
            "ts-node -P tsconfig.app.json app/x.ts data/a.ts",
            "ts-node --project tsconfig.app.json app/x.ts data/a.ts",
            "ts-node -O '{\"module\":\"commonjs\"}' app/x.ts data/a.ts",
            "ts-node --compiler-options '{}' app/x.ts data/a.ts",
            "ts-node-dev --project tsconfig.app.json app/x.ts data/a.ts",
            "nodemon --config nodemon.app.json app/x.ts data/a.ts",
            "tsx watch --include src/lib --exclude src/gen --ignore src/tmp app/x.ts data/a.ts",
            "deno run -L info app/x.ts data/a.ts",
            "deno run --log-level debug app/x.ts data/a.ts",
            "node --unhandled-rejections strict app/x.ts data/a.ts",
            "ts-node --unknown-flag tsconfig.json app/x.ts data/a.ts",
        ] {
            assert_eq!(entries(script), vec!["app/x.ts".to_string()], "{script}");
        }
        assert_eq!(
            super::argument_entries(
                "bun",
                &["--tsconfig-override", "tsconfig.app.json", "app/x.ts"]
            ),
            super::ArgumentEntries::Listed(vec!["app/x.ts"])
        );
    }

    #[test]
    fn loading_flag_value_after_equals_stays_an_entry() {
        assert_eq!(
            entries("node --import=./app/instrument.mjs app/server.js data.js"),
            vec![
                "./app/instrument.mjs".to_string(),
                "app/server.js".to_string()
            ]
        );
        assert_eq!(
            entries("node --require=./app/setup.js --import=tsx app/worker.js"),
            vec!["./app/setup.js".to_string(), "app/worker.js".to_string()]
        );
    }

    #[test]
    fn runtime_runner_subcommands_keep_every_file() {
        for (args, name) in [
            (&["build", "./x.ts", "--outdir", "dist"][..], "bun"),
            (&["build", "--compile", "./x.ts"], "bun"),
            (&["serve", "x.ts"], "deno"),
            (&["bench", "x.ts"], "deno"),
        ] {
            assert_eq!(
                super::argument_entries(name, args),
                super::ArgumentEntries::All,
                "{name} {args:?}"
            );
        }
    }

    #[test]
    fn runtime_running_a_runner_binary_applies_the_runner_rules() {
        assert_eq!(
            entries("node --experimental-vm-modules node_modules/jest/bin/jest.js app/a.test.js"),
            vec![
                "node_modules/jest/bin/jest.js".to_string(),
                "app/a.test.js".to_string()
            ]
        );
        assert_eq!(
            entries("node ./node_modules/.bin/mocha app/one.spec.js"),
            vec![
                "./node_modules/.bin/mocha".to_string(),
                "app/one.spec.js".to_string()
            ]
        );
    }

    #[test]
    fn new_runners_keep_every_file() {
        for runner in [
            "ts-mocha",
            "web-test-runner",
            "wtr",
            "electron-mocha",
            "jest-electron",
            "codecept",
            "detox",
        ] {
            assert!(super::takes_file_arguments(runner), "{runner}");
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_types,
        reason = "ScriptCatalog takes the serde-deserialized std HashMap"
    )]
    fn forwarded_arguments_follow_the_program_of_the_script_body() {
        let scripts = std::collections::HashMap::from([
            ("codegen".to_string(), "my-codegen".to_string()),
            ("gen".to_string(), "node scripts/gen.ts".to_string()),
            ("spec".to_string(), "mocha".to_string()),
        ]);
        let catalog = super::super::ScriptCatalog::from_scripts(&scripts);
        let forwarded = |script: &str| -> Vec<String> {
            super::super::parse_script_with_catalog(script, &catalog)
                .into_iter()
                .flat_map(|cmd| cmd.file_args)
                .collect()
        };
        assert_eq!(
            forwarded("npm run codegen -- src/input.ts"),
            vec!["src/input.ts"]
        );
        assert!(forwarded("npm run codegen -- 'src/**/*.ts'").is_empty());
        assert_eq!(
            forwarded("npm run gen src/input.ts"),
            vec!["scripts/gen.ts"]
        );
        assert_eq!(forwarded("npm run spec src/input.ts"), vec!["src/input.ts"]);
    }

    #[test]
    fn wrapped_executable_file_stays_an_entry() {
        assert_eq!(
            entries("varlock run -- ./scripts/worker.js"),
            vec!["./scripts/worker.js".to_string()]
        );
    }

    #[test]
    fn file_runner_keeps_every_file_argument() {
        assert_eq!(
            entries("mocha test/a.spec.js test/b.spec.js"),
            vec!["test/a.spec.js".to_string(), "test/b.spec.js".to_string()]
        );
        assert_eq!(
            entries("esbuild src/a.ts src/b.ts --bundle"),
            vec!["src/a.ts".to_string(), "src/b.ts".to_string()]
        );
        assert_eq!(
            entries("deno compile --allow-read src/a.ts"),
            vec!["src/a.ts".to_string()]
        );
    }

    #[test]
    fn runtime_test_mode_keeps_every_test_file() {
        assert_eq!(
            entries("node --test test/a.test.js test/b.test.js"),
            vec!["test/a.test.js".to_string(), "test/b.test.js".to_string()]
        );
    }

    #[test]
    fn shell_makes_an_entry_of_the_helper_only() {
        assert_eq!(
            entries("sh api-docs/build-docs.sh src/a.js"),
            vec!["api-docs/build-docs.sh".to_string()]
        );
    }
}
