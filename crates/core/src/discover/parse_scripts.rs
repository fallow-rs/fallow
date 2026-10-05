use crate::scripts::{
    DeclaredScriptCall, IgnoredCommandEntries, MAX_SCRIPT_EXPANSIONS, MAX_SCRIPT_INDIRECTION_DEPTH,
    MAX_SUBSTITUTION_DEPTH, ScriptCatalog,
};

/// Extract file path references from a package.json script value.
///
/// Recognises patterns like:
/// - `node path/to/script.js`
/// - `ts-node path/to/script.ts`
/// - `tsx path/to/script.ts`
/// - `npx ts-node path/to/script.ts`
/// - Bare file paths ending in `.js`, `.ts`, `.mjs`, `.cjs`, `.mts`, `.cts`
///
/// Script values are split by `&&`, `||`, and `;` to handle chained commands.
/// A segment that invokes a formatter or linter (`eslint src/a.ts`) yields no
/// references for its targets, because the tool reads them but does not
/// execute them. A module that such a tool loads through a flag, such as
/// `eslint -f ./tools/fmt.js`, is still a reference. A segment whose command
/// `ignored` lists yields no references.
///
/// A segment that calls a script that `context.scripts` declares
/// (`npm run lint -- src/a.ts`) resolves to the script body with the
/// forwarded arguments appended, as the package manager runs it. The rules
/// above then apply to that command.
///
/// The body of a `$(...)` or backtick command substitution is scanned as a
/// script of its own, because the shell runs it.
pub fn extract_script_file_refs(script: &str, context: CommandRefContext<'_>) -> Vec<String> {
    let mut refs = Vec::new();
    let mut expansions = 0;
    collect_script_file_refs(
        script,
        context,
        Depth::default(),
        &mut expansions,
        &mut refs,
    );
    refs
}

/// How deep the scan is in script calls and in command substitutions.
#[derive(Debug, Clone, Copy, Default)]
struct Depth {
    scripts: usize,
    substitutions: usize,
}

/// The inputs that decide which file arguments of a command become file
/// references.
#[derive(Debug, Clone, Copy)]
pub struct CommandRefContext<'a> {
    /// Commands whose file arguments are never references
    /// (`ignoreCommandEntries`).
    pub ignored: IgnoredCommandEntries<'a>,
    /// Declared package.json scripts, so a call such as
    /// `npm run lint -- src/a.ts` resolves to the command it runs.
    pub scripts: &'a ScriptCatalog,
}

#[cfg(test)]
impl CommandRefContext<'static> {
    /// No ignored command and no declared script.
    pub const NONE: Self = Self {
        ignored: IgnoredCommandEntries::NONE,
        scripts: ScriptCatalog::EMPTY,
    };
}

fn collect_script_file_refs(
    script: &str,
    context: CommandRefContext<'_>,
    depth: Depth,
    expansions: &mut usize,
    refs: &mut Vec<String>,
) {
    const RUNNERS: &[&str] = &["node", "bun", "ts-node", "tsx", "babel-node"];

    if depth.substitutions < MAX_SUBSTITUTION_DEPTH {
        let inner = Depth {
            substitutions: depth.substitutions + 1,
            ..depth
        };
        for body in crate::scripts::command_substitutions(script) {
            collect_script_file_refs(body, context, inner, expansions, refs);
        }
    }
    let called = Depth {
        scripts: depth.scripts + 1,
        ..depth
    };
    for segment in script.split(&['&', '|', ';'][..]) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        let tokens: Vec<&str> = segment.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }
        match crate::scripts::declared_script_call(&tokens, context.scripts) {
            Some(DeclaredScriptCall::NoFileRefs) => continue,
            Some(DeclaredScriptCall::Command(command)) => {
                if depth.scripts < MAX_SCRIPT_INDIRECTION_DEPTH
                    && *expansions < MAX_SCRIPT_EXPANSIONS
                {
                    *expansions += 1;
                    collect_script_file_refs(&command, context, called, expansions, refs);
                }
                continue;
            }
            Some(DeclaredScriptCall::InPackages(commands)) => {
                for command in &commands {
                    if depth.scripts >= MAX_SCRIPT_INDIRECTION_DEPTH
                        || *expansions >= MAX_SCRIPT_EXPANSIONS
                    {
                        break;
                    }
                    *expansions += 1;
                    let scripts = context.scripts.for_package_command(command);
                    let mut package_refs = Vec::new();
                    collect_script_file_refs(
                        &command.command,
                        CommandRefContext {
                            ignored: context.ignored,
                            scripts: &scripts,
                        },
                        called,
                        expansions,
                        &mut package_refs,
                    );
                    refs.extend(
                        package_refs
                            .iter()
                            .map(|path| crate::scripts::rebase_path(&command.dir, path)),
                    );
                }
                continue;
            }
            Some(DeclaredScriptCall::UnknownBody) | None => {}
        }
        // A call of a script with an unknown body, such as
        // `npm run gen -- scripts/a.ts` with no `gen` body, has no binary;
        // scan the whole segment for script files. This includes
        // `yarn eslint src/a.ts` when `eslint` is a declared script: the
        // package manager runs the script, not the `eslint` binary.
        let invoked = crate::scripts::invoked_command(&tokens, 0, context.scripts);
        if invoked
            .as_ref()
            .is_some_and(crate::scripts::InvokedCommand::runs_in_other_packages)
        {
            continue;
        }
        let start = invoked.as_ref().map_or(0, |invoked| invoked.index);
        let resolve = |path: &str| {
            invoked
                .as_ref()
                .map_or_else(|| vec![path.to_string()], |invoked| invoked.file_refs(path))
        };

        let cmd = tokens[start];
        if context.ignored.contains(cmd) || crate::scripts::is_task_runner(cmd) {
            continue;
        }
        if crate::scripts::is_file_target_tool(cmd) {
            refs.extend(
                crate::scripts::file_target_tool_loaded_files(cmd, &tokens[start + 1..])
                    .iter()
                    .flat_map(|path| resolve(path)),
            );
            continue;
        }

        let (args, is_ref): (&[&str], fn(&str) -> bool) = if RUNNERS.contains(&cmd) {
            (&tokens[start + 1..], looks_like_file_path)
        } else {
            (&tokens[start..], looks_like_script_file)
        };
        refs.extend(
            args.iter()
                .filter(|token| !token.starts_with('-') && is_ref(token))
                .flat_map(|token| resolve(token)),
        );
    }
}

/// Check if a token looks like a file path argument (has a directory separator
/// or a script-like source file extension).
pub fn looks_like_file_path(token: &str) -> bool {
    if !crate::scripts::could_be_file_path(token) {
        return false;
    }
    let extensions = [
        ".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx", ".gts", ".gjs",
    ];
    if extensions.iter().any(|ext| token.ends_with(ext)) {
        return true;
    }
    token.starts_with("./")
        || token.starts_with("../")
        || (token.contains('/') && !token.starts_with('@') && !token.contains("://"))
}

pub use crate::scripts::looks_like_script_file;

#[cfg(test)]
mod tests {
    use super::*;

    fn refs(script: &str) -> Vec<String> {
        extract_script_file_refs(script, CommandRefContext::NONE)
    }

    fn with_scripts(scripts: &ScriptCatalog) -> CommandRefContext<'_> {
        CommandRefContext {
            ignored: IgnoredCommandEntries::NONE,
            scripts,
        }
    }

    #[expect(
        clippy::disallowed_types,
        reason = "ScriptCatalog takes the serde-deserialized std HashMap"
    )]
    fn catalog(scripts: &[(&str, &str)]) -> ScriptCatalog {
        let scripts: std::collections::HashMap<String, String> = scripts
            .iter()
            .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
            .collect();
        ScriptCatalog::from_scripts(&scripts)
    }

    #[test]
    fn script_call_of_a_linter_script_yields_no_target_refs() {
        let scripts = catalog(&[("lint", "eslint"), ("fmt", "prettier --check")]);
        for script in [
            "npm run lint -- src/a.ts",
            "yarn lint src/b.ts",
            "pnpm fmt src/c.ts",
            "pnpm run lint src/d.ts",
            "CI=1 npm run lint -- src/e.ts",
            "npm run lint src/f.ts",
            "npm run -s fmt --cache src/g.ts",
        ] {
            assert!(
                extract_script_file_refs(script, with_scripts(&scripts)).is_empty(),
                "`{script}` must not yield file refs"
            );
        }
    }

    #[test]
    fn script_call_resolves_to_the_body_with_forwarded_arguments() {
        let scripts = catalog(&[("gen", "my-codegen"), ("run-tool", "node scripts/tool.ts")]);
        assert_eq!(
            extract_script_file_refs("npm run gen -- src/gen-input.ts", with_scripts(&scripts)),
            vec!["src/gen-input.ts"]
        );
        let ignored = vec!["my-codegen".to_string()];
        assert!(
            extract_script_file_refs(
                "npm run gen -- src/gen-input.ts",
                CommandRefContext {
                    ignored: IgnoredCommandEntries::new(&ignored),
                    scripts: &scripts,
                }
            )
            .is_empty()
        );
        assert_eq!(
            extract_script_file_refs("npm run run-tool -- --out dist", with_scripts(&scripts)),
            vec!["scripts/tool.ts"]
        );
        assert_eq!(
            extract_script_file_refs("npm run gen src/gen-input.ts", with_scripts(&scripts)),
            vec!["src/gen-input.ts"]
        );
        assert!(extract_script_file_refs("npm run gen", with_scripts(&scripts)).is_empty());
    }

    #[test]
    fn script_call_of_an_unknown_script_scans_the_segment() {
        assert_eq!(
            refs("npm run gen -- src/gen-input.ts"),
            vec!["src/gen-input.ts"]
        );
    }

    #[test]
    fn script_call_cycles_terminate() {
        let scripts = catalog(&[("a", "npm run b -- x/a.ts"), ("b", "npm run a -- x/b.ts")]);
        let refs = extract_script_file_refs("npm run a -- x/c.ts", with_scripts(&scripts));
        assert!(refs.len() <= MAX_SCRIPT_EXPANSIONS * 3, "{refs:?}");
    }

    #[test]
    fn workspace_and_env_wrapper_forms_of_a_linter_yield_no_target_refs() {
        for script in [
            "pnpm --filter web exec eslint src/a.ts",
            "pnpm --filter=web exec eslint src/a.ts",
            "pnpm -F web exec eslint src/a.ts",
            "pnpm -r exec prettier --check src/b.ts",
            "pnpm --recursive --parallel exec eslint src/b.ts",
            "pnpm exec -r eslint src/b.ts",
            "dotenv -e .env.ci -- eslint src/g.ts",
            "dotenv -e .env.ci eslint src/g.ts",
            "dotenv -c production -- eslint src/g.ts",
            "dotenv -c -- eslint src/g.ts",
            "dotenv -v CI=1 -o -- eslint src/g.ts",
            "env -u HOME eslint src/g.ts",
            "env -i CI=1 eslint src/g.ts",
        ] {
            assert!(
                refs(script).is_empty(),
                "`{script}` must not yield file refs: {:?}",
                refs(script)
            );
        }
        assert_eq!(
            refs("dotenv -e .env.ci -- node scripts/run.ts"),
            vec!["scripts/run.ts"]
        );
        assert!(
            refs("pnpm --filter web exec tsx scripts/run.ts").is_empty(),
            "a runner in another workspace package resolves its file there"
        );
        assert_eq!(
            refs("pnpm -C packages/web exec tsx scripts/run.ts"),
            vec!["packages/web/scripts/run.ts"]
        );
    }

    #[test]
    fn commands_in_other_workspace_packages_yield_no_refs() {
        for script in [
            "yarn workspace web eslint src/a.ts",
            "yarn workspace web tsx scripts/run.ts",
            "yarn workspace web lint src/a.ts",
            "yarn workspaces foreach -A run lint src/a.ts",
            "yarn workspaces run lint src/a.ts",
            "pnpm --filter web eslint src/a.ts",
            "pnpm --filter web run lint src/a.ts",
            "pnpm -r run lint -- src/a.ts",
            "pnpm -r exec tsx scripts/run.ts",
            "npm -w web run lint -- src/a.ts",
            "npm run lint --workspaces -- src/a.ts",
            "turbo run lint -- src/a.ts",
            "npx turbo lint -- src/a.ts",
            "nx run-many -t lint -- src/a.ts",
            "lerna run lint -- src/a.ts",
        ] {
            assert!(
                refs(script).is_empty(),
                "`{script}` must not yield file refs: {:?}",
                refs(script)
            );
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_types,
        reason = "ScriptCatalog takes the serde-deserialized std HashMap"
    )]
    fn a_declared_script_named_after_a_linter_keeps_its_target_refs() {
        let scripts = catalog(&[("eslint", "node tools/check.js")]);
        assert_eq!(
            extract_script_file_refs(
                "varlock run -- yarn eslint src/a.ts",
                with_scripts(&scripts)
            ),
            vec!["tools/check.js", "src/a.ts"]
        );

        let mut ambiguous = catalog(&[("eslint", "node tools/check.js")]);
        ambiguous.merge_workspace_scripts(&std::collections::HashMap::from([(
            "eslint".to_string(),
            "eslint .".to_string(),
        )]));
        assert_eq!(
            extract_script_file_refs("yarn eslint src/a.ts", with_scripts(&ambiguous)),
            vec!["src/a.ts"],
            "an unknown body keeps the target, as for `npm run eslint -- src/a.ts`"
        );
    }

    #[test]
    fn a_wrapped_call_of_a_linter_script_yields_no_target_refs() {
        let scripts = catalog(&[("lint", "eslint")]);
        assert!(
            extract_script_file_refs("varlock run -- yarn lint src/a.ts", with_scripts(&scripts))
                .is_empty()
        );
    }

    #[test]
    fn script_node_runner() {
        let refs = refs("node utilities/generate-coverage-badge.js");
        assert_eq!(refs, vec!["utilities/generate-coverage-badge.js"]);
    }

    #[test]
    fn script_formatter_and_linter_targets_are_not_refs() {
        for script in [
            "oxfmt --check src/dead.ts",
            "eslint src/dead.ts",
            "npx prettier --write src/dead.ts",
            "pnpm exec oxlint src/dead.ts",
            "pnpm eslint src/dead.ts",
            "CI=1 eslint src/dead.ts",
        ] {
            assert!(
                refs(script).is_empty(),
                "`{script}` must not yield file refs"
            );
        }
        assert_eq!(
            refs("eslint src/dead.ts && node scripts/build.js"),
            vec!["scripts/build.js"]
        );
    }

    #[test]
    fn script_formatter_and_linter_keep_loaded_module_refs() {
        assert_eq!(refs("eslint -f ./tools/fmt.js src"), vec!["./tools/fmt.js"]);
        assert_eq!(
            refs("prettier --plugin=./p.mjs --check src/a.ts"),
            vec!["./p.mjs"]
        );
        assert_eq!(
            refs("npx stylelint --custom-formatter ./tools/fmt.js \"**/*.css\""),
            vec!["./tools/fmt.js"]
        );
    }

    #[test]
    fn script_ts_node_runner() {
        let refs = refs("ts-node scripts/seed.ts");
        assert_eq!(refs, vec!["scripts/seed.ts"]);
    }

    #[test]
    fn script_tsx_runner() {
        let refs = refs("tsx scripts/migrate.ts");
        assert_eq!(refs, vec!["scripts/migrate.ts"]);
    }

    #[test]
    fn script_bun_runner() {
        let refs = refs("bun scripts/build.ts");
        assert_eq!(refs, vec!["scripts/build.ts"]);
    }

    #[test]
    fn script_npx_prefix() {
        let refs = refs("npx ts-node scripts/generate.ts");
        assert_eq!(refs, vec!["scripts/generate.ts"]);
    }

    #[test]
    fn script_chained_commands() {
        let refs = refs("node scripts/build.js && node scripts/post-build.js");
        assert_eq!(refs, vec!["scripts/build.js", "scripts/post-build.js"]);
    }

    #[test]
    fn script_with_flags() {
        let refs = refs("node --experimental-specifier-resolution=node scripts/run.mjs");
        assert_eq!(refs, vec!["scripts/run.mjs"]);
    }

    #[test]
    #[expect(
        clippy::disallowed_types,
        reason = "WorkspacePackages takes the serde-deserialized std HashMap"
    )]
    fn a_command_in_a_named_workspace_package_resolves_there() {
        let web_scripts: std::collections::HashMap<String, String> = [
            ("gen".to_string(), "tsx".to_string()),
            ("lint".to_string(), "eslint".to_string()),
        ]
        .into_iter()
        .collect();
        let mut packages = crate::scripts::WorkspacePackages::default();
        packages.add("web", "packages/web", Some(&web_scripts));
        let scripts = catalog(&[]).with_workspaces(std::sync::Arc::new(packages), "");
        for command in [
            "yarn workspace web node scripts/a.ts",
            "pnpm --filter web exec tsx scripts/a.ts",
            "npm run -w web gen -- scripts/a.ts",
            "yarn workspace web gen scripts/a.ts",
        ] {
            assert_eq!(
                extract_script_file_refs(command, with_scripts(&scripts)),
                vec!["packages/web/scripts/a.ts"],
                "`{command}`"
            );
        }
        for command in [
            "yarn workspace web eslint src/a.ts",
            "npm run -w web lint -- src/a.ts",
            "yarn workspace docs node scripts/a.ts",
        ] {
            assert!(
                extract_script_file_refs(command, with_scripts(&scripts)).is_empty(),
                "`{command}`"
            );
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_types,
        reason = "WorkspacePackages takes the serde-deserialized std HashMap"
    )]
    fn a_command_in_every_package_or_a_package_directory_resolves_there() {
        let package_scripts: std::collections::HashMap<String, String> = [
            ("gen".to_string(), "tsx".to_string()),
            ("lint".to_string(), "eslint".to_string()),
        ]
        .into_iter()
        .collect();
        let mut packages = crate::scripts::WorkspacePackages::default();
        packages.add("web", "packages/web", Some(&package_scripts));
        packages.add("api", "packages/api", Some(&package_scripts));
        let scripts = catalog(&[]).with_workspaces(std::sync::Arc::new(packages), "");
        for command in [
            "pnpm -r exec tsx scripts/a.ts",
            "pnpm -r run gen -- scripts/a.ts",
            "npm --workspaces run gen -- scripts/a.ts",
            "yarn workspaces foreach -A run gen scripts/a.ts",
            "yarn workspaces run gen scripts/a.ts",
        ] {
            let mut refs = extract_script_file_refs(command, with_scripts(&scripts));
            refs.sort();
            assert_eq!(
                refs,
                vec!["packages/api/scripts/a.ts", "packages/web/scripts/a.ts"],
                "`{command}`"
            );
        }
        for command in [
            "pnpm -C packages/web run gen scripts/a.ts",
            "npm --prefix packages/web run gen -- scripts/a.ts",
            "yarn --cwd packages/web gen scripts/a.ts",
        ] {
            assert_eq!(
                extract_script_file_refs(command, with_scripts(&scripts)),
                vec!["packages/web/scripts/a.ts"],
                "`{command}`"
            );
        }
        for command in [
            "pnpm -r exec eslint src/a.ts",
            "pnpm -r run lint -- src/a.ts",
            "yarn --cwd packages/web lint src/a.ts",
        ] {
            assert!(
                extract_script_file_refs(command, with_scripts(&scripts)).is_empty(),
                "`{command}`"
            );
        }
    }

    #[test]
    fn script_no_file_ref() {
        let refs = refs("next build");
        assert!(refs.is_empty());
    }

    #[test]
    fn script_bare_file_path() {
        let refs = refs("echo done && node ./scripts/check.js");
        assert_eq!(refs, vec!["./scripts/check.js"]);
    }

    #[test]
    fn script_semicolon_separator() {
        let refs = refs("node scripts/a.js; node scripts/b.ts");
        assert_eq!(refs, vec!["scripts/a.js", "scripts/b.ts"]);
    }

    #[test]
    fn script_command_substitution_bodies() {
        assert_eq!(
            refs(r#"STAMP="$(node scripts/a.ts)" LABEL=`tsx scripts/b.ts`"#),
            vec!["scripts/a.ts", "scripts/b.ts"]
        );
        assert_eq!(
            refs(r#"X="$(echo "$(node scripts/inner.ts)")""#),
            vec!["scripts/inner.ts"]
        );
        assert!(refs("echo '$(node scripts/none.ts)'").is_empty());
    }

    #[test]
    fn file_path_with_extension() {
        assert!(looks_like_file_path("scripts/build.js"));
        assert!(looks_like_file_path("scripts/build.ts"));
        assert!(looks_like_file_path("scripts/build.mjs"));
    }

    #[test]
    fn file_path_with_slash() {
        assert!(looks_like_file_path("scripts/build"));
    }

    #[test]
    fn not_file_path() {
        assert!(!looks_like_file_path("--watch"));
        assert!(!looks_like_file_path("build"));
    }

    #[test]
    fn script_file_with_path() {
        assert!(looks_like_script_file("scripts/build.js"));
        assert!(looks_like_script_file("./scripts/build.ts"));
        assert!(looks_like_script_file("../scripts/build.mjs"));
    }

    #[test]
    fn not_script_file_bare_name() {
        assert!(!looks_like_script_file("webpack.js"));
        assert!(!looks_like_script_file("build"));
    }

    #[test]
    fn looks_like_file_path_rejects_gha_fragments() {
        assert!(!looks_like_file_path("${{ env.URL }}/api.ts"));
        assert!(!looks_like_file_path("}}/api/health.ts"));
    }

    #[test]
    fn looks_like_file_path_rejects_backslash_and_bracket_class() {
        assert!(!looks_like_file_path(r"path\to\file.ts"));
        assert!(!looks_like_file_path(".[]"));
        assert!(!looks_like_file_path("prefix/[^unclosed.ts"));
    }

    #[test]
    fn looks_like_file_path_passes_nextjs_dynamic_route() {
        assert!(looks_like_file_path("app/[id]/page.tsx"));
        assert!(looks_like_file_path("pages/[...slug].ts"));
    }

    #[test]
    fn looks_like_script_file_rejects_gha_and_regex_fragments() {
        assert!(!looks_like_script_file("${{ env.X }}/path.ts"));
        assert!(!looks_like_script_file(r"path\file.ts"));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// looks_like_file_path should never panic on arbitrary strings.
            /// The class carries the metacharacters `could_be_file_path`
            /// branches on (`$`, `{`, `}`, `[`, `]`, `\`), so the GitHub-Actions
            /// template guard, the backslash guard and the `[` bracket scan are
            /// all reachable instead of only the final fall-through.
            #[test]
            fn looks_like_file_path_no_panic(s in r"[a-zA-Z0-9_./@$\[\]{}\\-]{1,80}") {
                let _ = looks_like_file_path(&s);
            }

            /// looks_like_script_file should never panic on arbitrary strings.
            /// Same widened class as `looks_like_file_path_no_panic`: both open
            /// with `crate::scripts::could_be_file_path`, whose guards are all
            /// on `$`, `{`, `}`, `\` and `[`.
            #[test]
            fn looks_like_script_file_no_panic(s in r"[a-zA-Z0-9_./@$\[\]{}\\-]{1,80}") {
                let _ = looks_like_script_file(&s);
            }

            /// extract_script_file_refs should never panic on arbitrary input.
            /// Keeps the whitespace and `&`/`|`/`;` segment separators and adds
            /// the `could_be_file_path` metacharacters, so tokenisation and the
            /// per-token guards are fuzzed together.
            #[test]
            fn extract_script_file_refs_no_panic(s in r"[a-zA-Z0-9 _./@&|;$\[\]{}\\-]{1,200}") {
                let _ = refs(&s);
            }
        }
    }
}
