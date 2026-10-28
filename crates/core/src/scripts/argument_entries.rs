//! The file arguments of a script command that become entry points
//! (issue #3967).
//!
//! A file argument is an entry point only when the program runs or loads the
//! file:
//!
//! - A script runtime (`node`, `tsx`, `bun`, `deno run`, a shell) runs one
//!   script: its first positional argument. The arguments after the script
//!   are data for the script. The values of the runtime flags that load a
//!   module (`--import`, `--loader`) stay entry points.
//! - A known file runner (a test runner, a bundler, a compiler) runs or
//!   loads every file that it gets. All its file arguments stay entry points.
//! - Every other program gets data from its arguments, for example a list of
//!   files to read or to watch. Its arguments name no entry point.
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
    /// Flags whose value is the next token and names a module that the
    /// runtime loads. A relative path value (`./setup.js`) is an entry
    /// point. A bare value (`tsx`, `ts-node/esm`) names a package.
    loading_flags: &'static [&'static str],
}

/// The flags of Node and of the runtimes that give their flags to Node,
/// whose value names no module.
const NODE_VALUE_FLAGS: &[&str] = &[
    "-C",
    "--conditions",
    "--disable-warning",
    "--env-file",
    "--env-file-if-exists",
    "--experimental-config-file",
    "--input-type",
    "--inspect-port",
    "--title",
    "--tsconfig",
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
            "sucrase-node",
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
        value_flags: &["--cwd", "-c", "--config", "--env-file"],
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
    "cucumber-js",
    "cypress",
    "jasmine",
    "jest",
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
    "uvu",
    "vitest",
    "wdio",
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

/// Subcommands of a script runtime that bundle or compile every file
/// argument: `deno compile`, `deno bundle`. Their file arguments stay entry
/// points.
const RUNTIME_BUILD_SUBCOMMANDS: &[(&str, &str)] = &[("deno", "compile"), ("deno", "bundle")];

/// Which arguments of a command name entry points.
#[derive(Debug, PartialEq, Eq)]
pub enum ArgumentEntries<'a> {
    /// Every file argument: the program runs or loads each one.
    All,
    /// Only these arguments: the script of a runtime and the modules that
    /// its flags load.
    Listed(Vec<&'a str>),
    /// No argument: the arguments are data for the program.
    None,
}

/// Return which arguments of `binary` name entry points. `args` are the
/// tokens after the binary.
#[must_use]
pub fn argument_entries<'a>(binary: &str, args: &[&'a str]) -> ArgumentEntries<'a> {
    let name = tool_name(binary);
    if takes_file_arguments(name) || node_test::runs_tests(name, args) {
        return ArgumentEntries::All;
    }
    match SCRIPT_RUNTIMES
        .iter()
        .find(|runtime| runtime.names.contains(&name))
    {
        Some(runtime) => runtime_entry_args(name, runtime, args),
        None => ArgumentEntries::None,
    }
}

/// Return `true` when `binary` runs or loads every file argument that it
/// gets: a test runner, a bundler, or a compiler.
#[must_use]
pub fn takes_file_arguments(binary: &str) -> bool {
    let name = tool_name(binary);
    borp::is_borp(name) || FILE_RUNNERS.contains(&name)
}

/// Return the file arguments of `binary` that become entry points.
///
/// `args` are the tokens after the binary. `file_args` are the file-like
/// arguments that the caller found. A formatter or linter gives only the
/// modules that its flags load. When `binary` is itself a file that a
/// command wrapper runs (`varlock run -- ./worker.js`), that file stays an
/// entry point.
pub(super) fn entry_file_args(binary: &str, args: &[&str], file_args: Vec<String>) -> Vec<String> {
    if is_file_target_tool(binary) {
        return file_target_tool_loaded_files(binary, args);
    }
    let executable = file_args.first().filter(|file| file.as_str() == binary);
    let mut entries: Vec<String> = executable.cloned().into_iter().collect();
    match argument_entries(binary, args) {
        ArgumentEntries::All => return file_args,
        ArgumentEntries::Listed(listed) => entries.extend(
            listed
                .into_iter()
                .filter(|token| looks_like_file_path(token))
                .map(str::to_string),
        ),
        ArgumentEntries::None => {}
    }
    entries
}

/// Return the script that `runtime` runs and the modules that its flags
/// load. The script is the first positional argument after an optional
/// subcommand. Each argument after the script goes to the script.
fn runtime_entry_args<'a>(
    name: &str,
    runtime: &ScriptRuntime,
    args: &[&'a str],
) -> ArgumentEntries<'a> {
    let mut entries = Vec::new();
    let mut idx = 0;
    let mut seen_subcommand = false;
    while let Some(&token) = args.get(idx) {
        if runtime.loading_flags.contains(&token) {
            entries.extend(
                args.get(idx + 1)
                    .copied()
                    .filter(|value| RELATIVE_PATH_PREFIXES.iter().any(|p| value.starts_with(p))),
            );
            idx += 2;
            continue;
        }
        if let Some(width) = flag_width(runtime, token) {
            idx += width;
            continue;
        }
        if !seen_subcommand {
            if RUNTIME_BUILD_SUBCOMMANDS.contains(&(name, token)) {
                return ArgumentEntries::All;
            }
            if runtime.subcommands.contains(&token) {
                seen_subcommand = true;
                idx += 1;
                continue;
            }
        }
        entries.push(token);
        break;
    }
    ArgumentEntries::Listed(entries)
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
    fn unknown_program_makes_no_entry_from_its_arguments() {
        for script in [
            "watchee -f './src/**/*' -c 'echo hi'",
            "some-tool src/a.js src/lib/b.js",
            "some-tool --input src/lib/b.js",
            "pnpm watchee -f './src/**/*' -c 'echo hi'",
        ] {
            assert_eq!(entries(script), Vec::<String>::new(), "{script}");
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
        assert!(forwarded("npm run codegen -- src/input.ts").is_empty());
        assert_eq!(
            forwarded("npm run gen src/input.ts"),
            vec!["scripts/gen.ts"]
        );
        assert_eq!(forwarded("npm run spec src/input.ts"), vec!["src/input.ts"]);
    }

    #[test]
    fn wrapped_executable_file_stays_an_entry() {
        assert_eq!(
            entries("varlock run -- ./scripts/worker.js src/a.js"),
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
