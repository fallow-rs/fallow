use super::*;

#[test]
fn script_externalizes_packages_reads_bun_build_and_esbuild() {
    for script in [
        "bun build ./src/index.ts --outdir dist --packages=external",
        "bun build ./src/index.ts --packages external --target node",
        "tsc --noEmit && bun build src/cli.ts --packages=external",
        "NODE_ENV=production esbuild src/index.ts --bundle --packages=external",
        "npx esbuild src/index.ts --bundle --packages external",
        "bunx esbuild src/index.ts --bundle \"--packages=external\"",
    ] {
        assert!(script_externalizes_packages(script), "{script}");
    }
}

#[test]
fn script_externalizes_packages_ignores_other_commands() {
    for script in [
        "bun build ./src/index.ts --outdir dist",
        "bun build ./src/index.ts --external react",
        "esbuild src/index.ts --bundle --external:react",
        "bun run build --packages=external",
        "node scripts/build.mjs --packages=external",
        "echo bun build --packages=external",
    ] {
        assert!(!script_externalizes_packages(script), "{script}");
    }
}

#[test]
fn script_program_files_reads_node_runner_programs() {
    for (script, expected) in [
        ("node build.mjs", "build.mjs"),
        ("node ./scripts/bundle", "./scripts/bundle"),
        ("NODE_ENV=production node --import tsx build.ts", "build.ts"),
        ("npx tsx scripts/build.ts --watch", "scripts/build.ts"),
        ("bun run scripts/build.ts", "scripts/build.ts"),
        ("bun --watch build.ts", "build.ts"),
        ("bun x tsx build.ts", "build.ts"),
        ("rm -rf dist && node build.mjs src/index.ts", "build.mjs"),
        (
            "tsx --tsconfig tsconfig.build.json scripts/build.ts",
            "scripts/build.ts",
        ),
        ("node scripts/contest.mjs", "scripts/contest.mjs"),
        ("node ./build", "./build"),
    ] {
        assert_eq!(script_program_files(script), vec![expected], "{script}");
    }
}

#[test]
fn script_program_files_ignores_tests_and_other_commands() {
    for script in [
        "vitest run test/plugin.test.ts",
        "node --test test/plugin.test.mjs",
        "tsx --test test/plugin.test.ts",
        "node --test-reporter=spec --test test/a.test.mjs",
        "bun test test/plugin.test.ts",
        "bun build ./src/index.ts --outdir dist",
        "bun run build",
        "node -e \"require('./build.js')\"",
        "jest test/plugin.test.ts",
        "node --import tsx test/plugin.test.ts",
        "tsx test/plugin.test.ts",
        "node test/run.mjs",
        "node tests/run.mjs",
        "node src/__tests__/run.mjs",
        "node src/plugin.spec.mjs",
        "node build",
    ] {
        assert!(script_program_files(script).is_empty(), "{script}");
    }
}

/// Analyze every script value without dependency context.
fn analyze_scripts(
    scripts: &HashMap<String, String>,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
) -> ScriptAnalysis {
    let mut result = ScriptAnalysis::default();
    for script_value in scripts.values() {
        accumulate_command(script_value, root, bin_map, &mut result);
    }
    result.dedupe_paths();
    result
}

/// Analyze every script value with dependency context and a catalog built
/// from the same scripts.
fn analyze_scripts_with_dependencies(
    scripts: &HashMap<String, String>,
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
) -> ScriptAnalysis {
    let catalog = ScriptCatalog::from_scripts(scripts);
    analyze_scripts_with_dependency_context(
        scripts,
        root,
        bin_map,
        declared_packages,
        &catalog,
        IgnoredCommandEntries::NONE,
    )
}

fn package_set(packages: &[&str]) -> FxHashSet<String> {
    packages.iter().map(|pkg| (*pkg).to_string()).collect()
}

/// Analyze a CI-style command against a project whose package.json declares
/// `scripts` and every package in `declared`.
fn analyze_ci_command(
    command: &str,
    scripts: &[(&str, &str)],
    declared: &[&str],
) -> ScriptAnalysis {
    let scripts: HashMap<String, String> = scripts
        .iter()
        .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
        .collect();
    let commands = vec![command.to_string()];
    analyze_commands_with_context(
        &commands,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(declared),
        &ScriptCatalog::from_scripts(&scripts),
        IgnoredCommandEntries::NONE,
    )
}

#[test]
fn normalize_root_level_strips_dot_slash() {
    assert_eq!(
        normalize_script_entry_pattern("", "./scripts/deploy.ts").as_deref(),
        Some("scripts/deploy.ts")
    );
}

#[test]
fn normalize_root_level_keeps_already_relative() {
    assert_eq!(
        normalize_script_entry_pattern("", "scripts/deploy.ts").as_deref(),
        Some("scripts/deploy.ts")
    );
}

#[test]
fn normalize_workspace_prefix_joins_path() {
    assert_eq!(
        normalize_script_entry_pattern("apps/api", "./scripts/deploy.ts").as_deref(),
        Some("apps/api/scripts/deploy.ts")
    );
}

#[test]
fn normalize_workspace_prefix_collapses_parent_segment() {
    assert_eq!(
        normalize_script_entry_pattern("apps/api", "../shared/scripts/deploy.ts").as_deref(),
        Some("apps/shared/scripts/deploy.ts")
    );
}

#[test]
fn normalize_workspace_prefix_collapses_two_parent_segments_to_root() {
    assert_eq!(
        normalize_script_entry_pattern("apps/api", "../../top.ts").as_deref(),
        Some("top.ts")
    );
}

#[test]
fn normalize_path_escaping_project_root_skipped() {
    assert_eq!(normalize_script_entry_pattern("", "../outside.ts"), None);
    assert_eq!(
        normalize_script_entry_pattern("apps/api", "../../../outside.ts"),
        None
    );
}

#[test]
fn normalize_absolute_path_skipped() {
    assert_eq!(normalize_script_entry_pattern("", "/etc/passwd"), None);
}

/// Regression test for issue #2592: an internal doubled separator (`//`)
/// must not be silently collapsed into a single `/`, which is exactly
/// what turned a jq filter's `//` alternative operator into what looked
/// like a truncated, malformed path in the reported warning.
#[test]
fn normalize_rejects_internal_double_slash_instead_of_silently_collapsing_it() {
    assert_eq!(
        normalize_script_entry_pattern(
            "",
            "[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]"
        ),
        None
    );
}

#[test]
fn normalize_tolerates_trailing_slash() {
    assert_eq!(
        normalize_script_entry_pattern("", "scripts/deploy/").as_deref(),
        Some("scripts/deploy")
    );
}

#[test]
fn normalize_empty_path_skipped() {
    assert_eq!(normalize_script_entry_pattern("", ""), None);
    assert_eq!(normalize_script_entry_pattern("apps/api", "./"), None);
}

#[test]
fn normalize_workspace_prefix_with_trailing_slash() {
    assert_eq!(
        normalize_script_entry_pattern("apps/api/", "./scripts/deploy.ts").as_deref(),
        Some("apps/api/scripts/deploy.ts")
    );
}

#[test]
fn simple_binary() {
    let cmds = parse_script("webpack");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "webpack");
}

#[test]
fn binary_with_args() {
    let cmds = parse_script("eslint src --ext .ts,.tsx");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "eslint");
}

#[test]
fn chained_commands() {
    let cmds = parse_script("tsc --noEmit && eslint src");
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].binary, "tsc");
    assert_eq!(cmds[1].binary, "eslint");
}

#[test]
fn semicolon_separator() {
    let cmds = parse_script("tsc; eslint src");
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].binary, "tsc");
    assert_eq!(cmds[1].binary, "eslint");
}

#[test]
fn or_chain() {
    let cmds = parse_script("tsc --noEmit || echo failed");
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].binary, "tsc");
    assert_eq!(cmds[1].binary, "echo");
}

#[test]
fn pipe_operator() {
    let cmds = parse_script("jest --json | tee results.json");
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].binary, "jest");
    assert_eq!(cmds[1].binary, "tee");
}

#[test]
fn npx_prefix() {
    let cmds = parse_script("npx eslint src");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "eslint");
}

#[test]
fn pnpx_prefix() {
    let cmds = parse_script("pnpx vitest run");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "vitest");
}

#[test]
fn npx_with_flags() {
    let cmds = parse_script("npx --yes --package @scope/tool eslint src");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "eslint");
}

#[test]
fn yarn_exec() {
    let cmds = parse_script("yarn exec jest");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "jest");
}

#[test]
fn pnpm_exec() {
    let cmds = parse_script("pnpm exec vitest run");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "vitest");
}

#[test]
fn pnpm_dlx() {
    let cmds = parse_script("pnpm dlx create-react-app my-app");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "create-react-app");
}

#[test]
fn npm_run_skipped() {
    let cmds = parse_script("npm run build");
    assert!(cmds.is_empty());
}

#[test]
fn yarn_run_skipped() {
    let cmds = parse_script("yarn run test");
    assert!(cmds.is_empty());
}

#[test]
fn bare_yarn_skipped() {
    let cmds = parse_script("yarn build");
    assert!(cmds.is_empty());
}

#[test]
fn cross_env_prefix() {
    let cmds = parse_script("cross-env NODE_ENV=production webpack");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "webpack");
}

#[test]
fn dotenv_prefix() {
    let cmds = parse_script("dotenv -- next build");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "next");
}

#[test]
fn env_var_assignment_prefix() {
    let cmds = parse_script("NODE_ENV=production webpack --mode production");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "webpack");
}

#[test]
fn multiple_env_vars() {
    let cmds = parse_script("NODE_ENV=test CI=true jest");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "jest");
}

#[test]
fn file_utility_arguments_are_not_file_args() {
    for script in [
        "cp ./README.md ./deno/lib",
        "node ./deno-build.mjs && cp ./README.md ./deno/lib",
        "mv dist/index.js lib/index.js",
        "cat src/banner.ts",
        "chmod +x bin/cli.js",
        "touch src/generated.ts",
        "mkdir -p lib && rm -rf lib/old.js",
        "echo src/banner.ts | tee dist/banner.ts",
        "ln -s ../src/cli.js bin/cli.js",
        "grep -l TODO src/a.ts && sed -i s/a/b/ src/b.ts",
        "rimraf dist/index.js",
        "npx cp src/a.js dist/a.js",
    ] {
        let cmds = parse_script(script);
        assert!(
            cmds.iter()
                .filter(|cmd| cmd.binary != "node")
                .all(|cmd| cmd.file_args.is_empty()),
            "`{script}` must not make utility arguments file args, got: {cmds:?}"
        );
    }
    let build = parse_script("node ./deno-build.mjs && cp ./README.md ./deno/lib");
    assert_eq!(build[0].file_args, vec!["./deno-build.mjs".to_string()]);
    // A program that runs another program keeps the entry point of that
    // program.
    let wrapped = parse_script("xargs node scripts/run.js");
    assert!(
        wrapped
            .iter()
            .any(|cmd| cmd.file_args.iter().any(|f| f == "scripts/run.js")),
        "xargs must keep the script that it runs, got: {wrapped:?}"
    );
}

#[test]
fn formatter_and_linter_targets_are_not_file_args() {
    for script in [
        "oxfmt --check \"**/*.ts\"",
        "oxfmt --check src/dead.ts",
        "eslint src/dead.ts",
        "oxlint src/",
        "prettier --write src/a.ts src/b.ts",
        "biome check src/dead.ts",
        "stylelint \"src/**/*.css\"",
        "npx eslint src/dead.ts",
        "pnpm exec prettier --check src/dead.ts",
        "./node_modules/.bin/eslint src/dead.ts",
        "varlock run -- eslint src/dead.ts",
        "varlock run -- CI=1 eslint src/dead.ts",
    ] {
        let cmds = parse_script(script);
        assert!(
            cmds.iter().all(|cmd| cmd.file_args.is_empty()),
            "`{script}` must not produce file args, got: {cmds:?}"
        );
    }
}

#[test]
fn package_manager_forms_of_a_linter_credit_it_without_entries() {
    for command in [
        "yarn run eslint src/dead.ts",
        "yarn eslint src/dead.ts",
        "bun run eslint src/dead.ts",
        "pnpm eslint src/dead.ts",
        "pnpm --silent exec eslint src/dead.ts",
        "npm exec -- eslint src/dead.ts",
        "npx -- eslint src/dead.ts",
        "cross-env CI=1 npx eslint src/dead.ts",
        "varlock run -- npx eslint src/dead.ts",
        "varlock run -- yarn run eslint src/dead.ts",
    ] {
        let result = analyze_ci_command(command, &[], &["eslint", "varlock"]);
        assert!(
            result.used_packages.contains("eslint"),
            "`{command}` did not credit eslint: {:?}",
            result.used_packages
        );
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn workspace_and_env_wrapper_forms_of_a_linter_credit_it_without_entries() {
    for command in [
        "pnpm --filter web exec eslint src/dead.ts",
        "pnpm --filter=web exec eslint src/dead.ts",
        "pnpm -F web exec eslint src/dead.ts",
        "pnpm -r exec eslint src/dead.ts",
        "pnpm --recursive --parallel exec eslint src/dead.ts",
        "pnpm -C packages/web exec eslint src/dead.ts",
        "pnpm exec -r -- eslint src/dead.ts",
        "dotenv -e .env.ci -- eslint src/dead.ts",
        "dotenv -e .env.ci -e .env -- eslint src/dead.ts",
        "dotenv -c production -- eslint src/dead.ts",
        "dotenv -c -- eslint src/dead.ts",
        "dotenv -v CI=1 --override -- eslint src/dead.ts",
        "env -u HOME eslint src/dead.ts",
        "env -i CI=1 eslint src/dead.ts",
    ] {
        let result = analyze_ci_command(command, &[], &["eslint"]);
        assert!(
            result.used_packages.contains("eslint"),
            "`{command}` did not credit eslint: {:?}",
            result.used_packages
        );
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn env_wrapper_forms_of_a_runner_keep_entries() {
    for command in [
        "dotenv -e .env.ci -- tsx scripts/run.ts",
        "pnpm exec tsx scripts/run.ts",
    ] {
        let result = analyze_ci_command(command, &[], &["tsx"]);
        assert_eq!(result.entry_files, vec!["scripts/run.ts"], "`{command}`");
    }
}

#[test]
fn parse_script_with_catalog_resolves_a_call_of_a_linter_script() {
    let scripts: HashMap<String, String> = HashMap::from([
        ("lint".to_string(), "eslint".to_string()),
        ("gen".to_string(), "mocha".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);
    for script in [
        "npm run lint -- src/a.ts",
        "yarn lint src/b.ts",
        "pnpm lint src/c.ts",
        "pnpm run lint src/d.ts",
    ] {
        let commands = parse_script_with_catalog(script, &catalog);
        assert!(
            commands.iter().all(|command| command.file_args.is_empty()),
            "`{script}`: {commands:?}"
        );
    }
    let commands = parse_script_with_catalog("npm run gen -- src/gen-input.ts", &catalog);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].file_args, vec!["src/gen-input.ts"]);
    assert_eq!(commands[0].file_args_command, "mocha");
    let ignored = vec!["mocha".to_string()];
    assert!(
        commands[0]
            .entry_files(IgnoredCommandEntries::new(&ignored))
            .is_empty()
    );
}

#[test]
fn declared_script_call_resolves_only_local_bodies_with_arguments() {
    let mut catalog =
        ScriptCatalog::from_scripts(&HashMap::from([("lint".to_string(), "eslint".to_string())]));
    catalog.merge_workspace_scripts(&HashMap::from([(
        "gen".to_string(),
        "my-codegen".to_string(),
    )]));
    let tokens = |command: &'static str| command.split_whitespace().collect::<Vec<_>>();
    assert_eq!(
        declared_script_call(&tokens("CI=1 npm run lint -- src/a.ts"), &catalog),
        Some(DeclaredScriptCall::Command("eslint src/a.ts".to_string()))
    );
    assert_eq!(
        declared_script_call(&tokens("npm run lint"), &catalog),
        Some(DeclaredScriptCall::NoFileRefs)
    );
    assert_eq!(
        declared_script_call(&tokens("npm run lint --fix src/a.ts -- --quiet"), &catalog),
        Some(DeclaredScriptCall::Command(
            "eslint src/a.ts --quiet".to_string()
        ))
    );
    assert_eq!(
        declared_script_call(&tokens("npm run -s lint --cache .cache src/a.ts"), &catalog),
        Some(DeclaredScriptCall::Command("eslint src/a.ts".to_string()))
    );
    for other_package in [
        "npm run -s lint -w web src/a.ts",
        "npm run lint --workspace=web src/a.ts",
        "npm run lint --workspaces -- src/a.ts",
        "pnpm --filter web lint src/a.ts",
        "pnpm -F web lint src/a.ts",
        "pnpm --filter=web lint src/a.ts",
    ] {
        assert_eq!(
            declared_script_call(&tokens(other_package), &catalog),
            Some(DeclaredScriptCall::NoFileRefs),
            "{other_package} runs the script in another package"
        );
    }
    assert_eq!(
        declared_script_call(&tokens("npm run lint --fix"), &catalog),
        Some(DeclaredScriptCall::NoFileRefs)
    );
    assert_eq!(
        declared_script_call(&tokens("npm run gen -- src/a.ts"), &catalog),
        Some(DeclaredScriptCall::NoFileRefs)
    );
    assert_eq!(
        declared_script_call(&tokens("npm run other -- src/a.ts"), &catalog),
        None
    );
}

#[test]
fn package_manager_run_of_an_undeclared_binary_credits_nothing() {
    let result = analyze_ci_command("yarn run eslint src/dead.ts", &[], &[]);
    assert!(
        result.used_packages.is_empty(),
        "{:?}",
        result.used_packages
    );
    assert!(result.entry_files.is_empty(), "{:?}", result.entry_files);
}

#[test]
fn package_manager_run_prefers_the_declared_script() {
    let result = analyze_ci_command(
        "yarn run eslint -- --fix",
        &[("eslint", "node scripts/lint.js")],
        &["eslint"],
    );
    assert_eq!(result.entry_files, vec!["scripts/lint.js"]);
}

#[test]
fn npm_run_never_runs_a_binary() {
    let result = analyze_ci_command("npm run eslint src/dead.ts", &[], &["eslint"]);
    assert!(!result.used_packages.contains("eslint"));
}

#[test]
fn wrapped_package_manager_linter_keeps_loaded_modules_only() {
    let result = analyze_ci_command(
        "varlock run -- npx eslint -f ./tools/fmt.js src/dead.ts",
        &[],
        &["eslint", "varlock"],
    );
    assert_eq!(result.entry_files, vec!["./tools/fmt.js"]);
}

#[test]
fn wrapped_runner_keeps_its_entry() {
    let result = analyze_ci_command("varlock run -- pnpm tsx scripts/seed.ts", &[], &["varlock"]);
    assert_eq!(result.entry_files, vec!["scripts/seed.ts"]);
}

#[test]
fn additional_formatters_linters_and_checkers_have_no_target_entries() {
    for script in [
        "textlint \"docs/**/*.md\" src/dead.ts",
        "eslint_d src/dead.ts",
        "semistandard src/dead.ts",
        "remark . src/dead.ts",
        "secretlint \"**/*\" src/dead.ts",
        "htmlhint src/dead.ts",
        "markuplint src/dead.ts",
        "alex src/dead.ts",
        "jscpd src/dead.ts",
        "madge --circular src/dead.ts",
        "depcruise src/dead.ts",
        "ember-template-lint src/dead.ts",
        "editorconfig-checker src/dead.ts",
        "rome check src/dead.ts",
    ] {
        let files: Vec<String> = parse_script(script)
            .into_iter()
            .flat_map(|cmd| cmd.file_args)
            .collect();
        assert!(files.is_empty(), "`{script}` produced file args {files:?}");
    }
}

#[test]
fn additional_tools_keep_loaded_module_paths() {
    let cases: [(&str, &str); 12] = [
        (
            "textlint --rulesdir ./rules docs",
            "./rules/*.{js,cjs,mjs,ts,cts,mts}",
        ),
        (
            "eslint . --rulesdir ./devEnv/eslint/rules/",
            "./devEnv/eslint/rules/*.{js,cjs,mjs,ts,cts,mts}",
        ),
        (
            "textlint --rulesdir=. ../src/content",
            "*.{js,cjs,mjs,ts,cts,mts}",
        ),
        (
            "tslint -r tools/rules src/a.ts",
            "tools/rules/*.{js,cjs,mjs,ts,cts,mts}",
        ),
        ("eslint --rulesdir @scope/rules src", ""),
        ("textlint -f ./tools/fmt.js docs", "./tools/fmt.js"),
        ("remark --use ./plugins/lint.mjs .", "./plugins/lint.mjs"),
        (
            "cspell --reporter ./tools/reporter.js .",
            "./tools/reporter.js",
        ),
        ("eslint --parser ./tools/parser.js src", "./tools/parser.js"),
        ("eslint_d -f ./tools/fmt.js src", "./tools/fmt.js"),
        (
            "madge --webpack-config webpack.config.js src",
            "webpack.config.js",
        ),
        (
            "htmlhint --rulesdir ./rules src",
            "./rules/*.{js,cjs,mjs,ts,cts,mts}",
        ),
    ];
    for (script, expected) in cases {
        let files: Vec<String> = parse_script(script)
            .into_iter()
            .flat_map(|cmd| cmd.file_args)
            .collect();
        let expected: Vec<&str> = if expected.is_empty() {
            Vec::new()
        } else {
            vec![expected]
        };
        assert_eq!(files, expected, "`{script}`");
    }
}

#[test]
fn ignored_command_entries_drop_file_args_but_keep_credit_and_config() {
    let scripts: HashMap<String, String> = [
        ("gen", "my-codegen -c codegen.config.ts \"src/**/*.ts\""),
        ("seed", "node scripts/seed.ts"),
        ("fmt", "varlock run -- npx my-codegen src/a.ts"),
    ]
    .into_iter()
    .map(|(name, body)| (name.to_string(), body.to_string()))
    .collect();
    let ignored = vec!["my-codegen".to_string()];
    let result = analyze_scripts_with_dependency_context(
        &scripts,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["my-codegen", "varlock"]),
        &ScriptCatalog::from_scripts(&scripts),
        IgnoredCommandEntries::new(&ignored),
    );
    assert!(result.used_packages.contains("my-codegen"));
    assert_eq!(result.config_files, vec!["codegen.config.ts"]);
    assert_eq!(result.entry_files, vec!["scripts/seed.ts"]);
}

#[test]
fn wildcard_ignores_every_command_entry() {
    let ignored = vec!["*".to_string()];
    let commands = vec![
        "node scripts/seed.ts".to_string(),
        "eslint -f ./tools/fmt.js src".to_string(),
    ];
    let result = analyze_commands_with_context(
        &commands,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["eslint"]),
        &ScriptCatalog::default(),
        IgnoredCommandEntries::new(&ignored),
    );
    assert!(result.used_packages.contains("eslint"));
    assert!(result.entry_files.is_empty(), "{:?}", result.entry_files);
}

#[test]
fn formatter_and_linter_keep_config_args() {
    let cmds = parse_script("eslint -c config/eslint.config.js src/dead.ts");
    assert_eq!(cmds[0].binary, "eslint");
    assert_eq!(cmds[0].config_args, vec!["config/eslint.config.js"]);
    assert!(cmds[0].file_args.is_empty());
}

#[test]
fn formatter_and_linter_keep_loaded_module_paths() {
    let cases: [(&str, &[&str]); 10] = [
        ("eslint -f ./tools/fmt.js src", &["./tools/fmt.js"]),
        ("npx eslint --format ./tools/fmt.js .", &["./tools/fmt.js"]),
        (
            "eslint --format=./tools/fmt.js src/a.ts",
            &["./tools/fmt.js"],
        ),
        ("eslint -f stylish src/a.ts", &[]),
        ("prettier --plugin=./p.mjs --check src/a.ts", &["./p.mjs"]),
        (
            "prettier --plugin ./tools/fmt.js --check src",
            &["./tools/fmt.js"],
        ),
        (
            "prettier --plugin prettier-plugin-foo --check src/a.ts",
            &[],
        ),
        (
            "stylelint --custom-formatter ./tools/fmt.js \"**/*.css\"",
            &["./tools/fmt.js"],
        ),
        (
            "varlock run -- eslint -f ./tools/fmt.js src/a.ts",
            &["./tools/fmt.js"],
        ),
        ("oxfmt --check src/dead.ts", &[]),
    ];
    for (script, expected) in cases {
        for cmd in parse_script(script) {
            let file_args: Vec<&str> = cmd.file_args.iter().map(String::as_str).collect();
            assert!(
                file_args.is_empty() || file_args == expected,
                "`{script}` produced file args {file_args:?}, expected {expected:?}"
            );
        }
        let all: Vec<String> = parse_script(script)
            .into_iter()
            .flat_map(|cmd| cmd.file_args)
            .collect();
        assert_eq!(all.is_empty(), expected.is_empty(), "`{script}`: {all:?}");
    }
}

#[test]
fn linter_chained_with_node_keeps_node_entry() {
    let cmds = parse_script("eslint src/dead.ts && node scripts/build.js");
    assert_eq!(cmds.len(), 2);
    assert!(cmds[0].file_args.is_empty());
    assert_eq!(cmds[1].file_args, vec!["scripts/build.js"]);
}

#[test]
fn command_substitution_bodies_are_parsed_as_commands() {
    let cmds = parse_script(r#"STAMP="$(node scripts/stamp.ts)" LABEL=`tsx scripts/label.ts`"#);
    let files: Vec<&str> = cmds
        .iter()
        .flat_map(|cmd| cmd.file_args.iter().map(String::as_str))
        .collect();
    assert_eq!(files, vec!["scripts/stamp.ts", "scripts/label.ts"]);
}

#[test]
fn nested_command_substitution_is_parsed() {
    let cmds = parse_script(r#"X="$(echo "$(node scripts/inner.ts)")""#);
    assert!(
        cmds.iter()
            .any(|cmd| cmd.binary == "node" && cmd.file_args == ["scripts/inner.ts"]),
        "{cmds:?}"
    );
}

#[test]
fn single_quoted_command_substitution_is_not_parsed() {
    let cmds = parse_script("echo '$(node scripts/none.ts)'");
    assert!(cmds.iter().all(|cmd| cmd.binary != "node"), "{cmds:?}");
}

#[test]
fn node_runner_file_args() {
    let cmds = parse_script("node scripts/build.js");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "node");
    assert_eq!(cmds[0].file_args, vec!["scripts/build.js"]);
}

#[test]
fn tsx_runner_file_args() {
    let cmds = parse_script("tsx scripts/migrate.ts");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "tsx");
    assert_eq!(cmds[0].file_args, vec!["scripts/migrate.ts"]);
}

#[test]
fn node_with_flags() {
    let cmds = parse_script("node --experimental-specifier-resolution=node scripts/run.mjs");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].file_args, vec!["scripts/run.mjs"]);
}

#[test]
fn node_eval_no_file() {
    let cmds = parse_script("node -e \"console.log('hi')\"");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "node");
    assert!(cmds[0].file_args.is_empty());
}

#[test]
fn node_multiple_files() {
    let cmds = parse_script("node --test file1.mjs file2.mjs");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].file_args, vec!["file1.mjs", "file2.mjs"]);
}

#[test]
fn config_equals() {
    let cmds = parse_script("webpack --config=webpack.prod.js");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "webpack");
    assert_eq!(cmds[0].config_args, vec!["webpack.prod.js"]);
}

#[test]
fn config_space() {
    let cmds = parse_script("jest --config jest.config.ts");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "jest");
    assert_eq!(cmds[0].config_args, vec!["jest.config.ts"]);
}

#[test]
fn config_short_flag() {
    let cmds = parse_script("eslint -c .eslintrc.json src");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "eslint");
    assert_eq!(cmds[0].config_args, vec![".eslintrc.json"]);
}

#[test]
fn tsc_maps_to_typescript() {
    let pkg = resolve_binary_to_packages("tsc", Path::new("/nonexistent"), &FxHashMap::default());
    assert_eq!(pkg, ["typescript"]);
}

#[test]
fn ng_maps_to_angular_cli() {
    let pkg = resolve_binary_to_packages("ng", Path::new("/nonexistent"), &FxHashMap::default());
    assert_eq!(pkg, ["@angular/cli"]);
}

#[test]
fn biome_maps_to_biomejs() {
    let pkg = resolve_binary_to_packages("biome", Path::new("/nonexistent"), &FxHashMap::default());
    assert_eq!(pkg, ["@biomejs/biome"]);
}

#[test]
fn unknown_binary_is_identity() {
    let pkg = resolve_binary_to_packages(
        "my-custom-tool",
        Path::new("/nonexistent"),
        &FxHashMap::default(),
    );
    assert_eq!(pkg, ["my-custom-tool"]);
}

#[test]
fn run_s_maps_to_npm_run_all_and_its_fork() {
    let pkg = resolve_binary_to_packages("run-s", Path::new("/nonexistent"), &FxHashMap::default());
    assert_eq!(pkg, ["npm-run-all", "npm-run-all2"]);
}

#[test]
fn bin_path_regular_package() {
    let path = std::path::Path::new("../webpack/bin/webpack.js");
    assert_eq!(
        resolve::extract_package_from_bin_path(path),
        Some("webpack".to_string())
    );
}

#[test]
fn bin_path_scoped_package() {
    let path = std::path::Path::new("../@babel/cli/bin/babel.js");
    assert_eq!(
        resolve::extract_package_from_bin_path(path),
        Some("@babel/cli".to_string())
    );
}

#[test]
fn builtin_commands_not_tracked() {
    let scripts: HashMap<String, String> =
        std::iter::once(("postinstall".to_string(), "echo done".to_string())).collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    assert!(result.used_packages.is_empty());
}

#[test]
fn analyze_extracts_binaries() {
    let scripts: HashMap<String, String> = [
        ("build".to_string(), "tsc --noEmit && webpack".to_string()),
        ("lint".to_string(), "eslint src".to_string()),
        ("test".to_string(), "jest".to_string()),
    ]
    .into_iter()
    .collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    assert!(result.used_packages.contains("typescript"));
    assert!(result.used_packages.contains("webpack"));
    assert!(result.used_packages.contains("eslint"));
    assert!(result.used_packages.contains("jest"));
}

#[test]
fn analyze_extracts_config_files() {
    let scripts: HashMap<String, String> = std::iter::once((
        "build".to_string(),
        "webpack --config webpack.prod.js".to_string(),
    ))
    .collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    assert!(result.config_files.contains(&"webpack.prod.js".to_string()));
}

#[test]
fn analyze_keeps_binary_of_each_config_file() {
    let scripts: HashMap<String, String> = [
        ("snap", "npx vitest run --config vitest.snap.config.ts"),
        ("e2e", "pnpm exec vitest -c=./vitest.e2e.config.ts"),
        (
            "build",
            "cross-env NODE_ENV=production webpack --config webpack.prod.js",
        ),
    ]
    .into_iter()
    .map(|(name, body)| (name.to_string(), body.to_string()))
    .collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    let mut pairs: Vec<(&str, &str)> = result
        .binary_config_files
        .iter()
        .map(|file| (file.binary.as_str(), file.path.as_str()))
        .collect();
    pairs.sort_unstable();
    assert_eq!(
        pairs,
        vec![
            ("vitest", "./vitest.e2e.config.ts"),
            ("vitest", "vitest.snap.config.ts"),
            ("webpack", "webpack.prod.js"),
        ]
    );
}

#[test]
fn analyze_extracts_entry_files() {
    let scripts: HashMap<String, String> =
        std::iter::once(("seed".to_string(), "ts-node scripts/seed.ts".to_string())).collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    assert!(result.entry_files.contains(&"scripts/seed.ts".to_string()));
    assert!(result.used_packages.contains("ts-node"));
}

#[test]
fn analyze_extracts_k6_run_entry_file_and_binary() {
    let scripts: HashMap<String, String> =
        std::iter::once(("load".to_string(), "k6 run load/smoke.k6.js".to_string())).collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());

    assert!(result.entry_files.contains(&"load/smoke.k6.js".to_string()));
    assert!(result.used_packages.contains("k6"));
}

#[test]
fn analyze_cross_env_with_config() {
    let scripts: HashMap<String, String> = std::iter::once((
        "build".to_string(),
        "cross-env NODE_ENV=production webpack --config webpack.prod.js".to_string(),
    ))
    .collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    assert!(result.used_packages.contains("cross-env"));
    assert!(result.used_packages.contains("webpack"));
    assert!(result.config_files.contains(&"webpack.prod.js".to_string()));
}

#[test]
fn analyze_complex_script() {
    let scripts: HashMap<String, String> = std::iter::once((
        "ci".to_string(),
        "cross-env CI=true npm run build && jest --config jest.ci.js --coverage".to_string(),
    ))
    .collect();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    assert!(result.used_packages.contains("cross-env"));
    assert!(result.used_packages.contains("jest"));
    assert!(!result.used_packages.contains("npm"));
    assert!(result.config_files.contains(&"jest.ci.js".to_string()));
}

#[test]
fn analyze_scripts_with_dependencies_credits_pnpm_bare_declared_binary() {
    let scripts = HashMap::from([(
        "viteinfo".to_string(),
        "pnpm envinfo --system --npmPackages '{vite,@vitejs/*}' --binaries --browsers".to_string(),
    )]);
    let result = analyze_scripts_with_dependencies(
        &scripts,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["envinfo"]),
    );
    assert!(result.used_packages.contains("envinfo"));
}

#[test]
fn analyze_scripts_with_dependencies_credits_pnpm_silent_binary() {
    let scripts = HashMap::from([(
        "viteinfo".to_string(),
        "pnpm --silent envinfo --system".to_string(),
    )]);
    let result = analyze_scripts_with_dependencies(
        &scripts,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["envinfo"]),
    );
    assert!(result.used_packages.contains("envinfo"));
}

#[test]
fn analyze_scripts_with_dependencies_credits_varlock_run_binary() {
    for command in [
        "varlock run -- is-ci",
        "varlock run -p ./env/ -p ./env/.env.schema -- is-ci",
        "ENVIRONMENT=development varlock run -- is-ci",
        "varlock run -- pnpm is-ci",
        "pnpm exec varlock run -- is-ci",
        "varlock run -- varlock run -- is-ci",
    ] {
        let scripts = HashMap::from([("ci".to_string(), command.to_string())]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["varlock", "is-ci"]),
        );
        assert_eq!(
            result.used_packages,
            package_set(&["varlock", "is-ci"]),
            "{command}"
        );
        assert!(result.entry_files.is_empty(), "{command}");
    }
}

#[test]
fn analyze_scripts_with_dependencies_varlock_preserves_pnpm_exclusions() {
    for command in [
        "varlock run -- pnpm build",
        "varlock run -- pnpm install",
        "varlock run -- pnpm audit",
        "varlock run -- pnpm add lodash",
        "varlock run -- pnpm start",
        "varlock run -- pnpm test",
    ] {
        let scripts = HashMap::from([
            ("build".to_string(), "echo build".to_string()),
            ("check".to_string(), command.to_string()),
        ]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&[
                "varlock", "build", "install", "audit", "add", "lodash", "start", "test",
            ]),
        );
        assert_eq!(result.used_packages, package_set(&["varlock"]), "{command}");
    }
}

#[test]
fn analyze_scripts_with_dependencies_varlock_preserves_executable_file() {
    for command in [
        "varlock run -- './scripts/worker.js'",
        "varlock run -- bun './scripts/worker.js'",
        "varlock run -- node './scripts/worker.js'",
    ] {
        let scripts = HashMap::from([("start".to_string(), command.to_string())]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["varlock"]),
        );
        assert_eq!(result.entry_files, vec!["./scripts/worker.js"], "{command}");
        let result = analyze_command(command, Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(result.entry_files, vec!["./scripts/worker.js"], "{command}");
    }
}

#[test]
fn analyze_scripts_with_dependencies_varlock_requires_run_and_separator() {
    for command in [
        "varlock",
        "varlock run",
        "varlock run --",
        "varlock run is-ci",
        "varlock printenv -- is-ci",
    ] {
        let scripts = HashMap::from([("ci".to_string(), command.to_string())]);
        let result = analyze_scripts_with_dependencies(
            &scripts,
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["varlock", "is-ci"]),
        );
        assert_eq!(result.used_packages, package_set(&["varlock"]), "{command}");
    }
}

#[test]
fn analyze_scripts_with_dependencies_skips_pnpm_script_name_collision() {
    let scripts = HashMap::from([
        ("build".to_string(), "echo build".to_string()),
        ("check".to_string(), "pnpm build".to_string()),
    ]);
    let result = analyze_scripts_with_dependencies(
        &scripts,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["build"]),
    );
    assert!(!result.used_packages.contains("build"));
}

#[test]
fn analyze_scripts_with_dependencies_skips_pnpm_builtin_commands() {
    let scripts = HashMap::from([(
        "ci".to_string(),
        "pnpm install && pnpm audit && pnpm add lodash && pnpm start && pnpm test".to_string(),
    )]);
    let result = analyze_scripts_with_dependencies(
        &scripts,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["install", "audit", "add", "start", "test"]),
    );
    assert!(result.used_packages.is_empty());
}

#[test]
fn analyze_scripts_with_dependencies_credits_pnpm_divergent_bin_map() {
    let scripts = HashMap::from([(
        "lint".to_string(),
        "pnpm attw --profile esm-only --pack .".to_string(),
    )]);
    let mut bin_map = FxHashMap::default();
    bin_map.insert("attw".to_string(), "@arethetypeswrong/cli".to_string());
    let result = analyze_scripts_with_dependencies(
        &scripts,
        Path::new("/nonexistent"),
        &bin_map,
        &package_set(&["@arethetypeswrong/cli"]),
    );
    assert!(result.used_packages.contains("@arethetypeswrong/cli"));
}

#[test]
fn parse_script_keeps_bare_pnpm_syntax_only_behavior() {
    let cmds = parse_script("pnpm envinfo --system");
    assert!(cmds.is_empty());
}

#[test]
fn env_assignment_valid() {
    assert!(is_env_assignment("NODE_ENV=production"));
    assert!(is_env_assignment("CI=true"));
    assert!(is_env_assignment("PORT=3000"));
}

#[test]
fn env_assignment_invalid() {
    assert!(!is_env_assignment("--config"));
    assert!(!is_env_assignment("webpack"));
    assert!(!is_env_assignment("./scripts/build.js"));
}

#[test]
fn split_respects_quotes() {
    let segments = shell::split_shell_operators("echo 'a && b' && jest");
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[1].trim(), "jest");
}

#[test]
fn split_double_quotes() {
    let segments = shell::split_shell_operators("echo \"a || b\" || jest");
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[1].trim(), "jest");
}

#[test]
fn background_operator_splits_commands() {
    let cmds = parse_script("tsc --watch & webpack --watch");
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].binary, "tsc");
    assert_eq!(cmds[1].binary, "webpack");
}

#[test]
fn double_ampersand_still_works() {
    let cmds = parse_script("tsc --watch && webpack --watch");
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].binary, "tsc");
    assert_eq!(cmds[1].binary, "webpack");
}

#[test]
fn multiple_background_operators() {
    let cmds = parse_script("server & client & proxy");
    assert_eq!(cmds.len(), 3);
    assert_eq!(cmds[0].binary, "server");
    assert_eq!(cmds[1].binary, "client");
    assert_eq!(cmds[2].binary, "proxy");
}

#[test]
fn production_script_start() {
    assert!(super::is_production_script("start"));
    assert!(super::is_production_script("prestart"));
    assert!(super::is_production_script("poststart"));
}

#[test]
fn production_script_build() {
    assert!(super::is_production_script("build"));
    assert!(super::is_production_script("prebuild"));
    assert!(super::is_production_script("postbuild"));
    assert!(super::is_production_script("build:prod"));
    assert!(super::is_production_script("build:esm"));
}

#[test]
fn production_script_serve_preview() {
    assert!(super::is_production_script("serve"));
    assert!(super::is_production_script("preview"));
    assert!(super::is_production_script("prepare"));
}

#[test]
fn non_production_scripts() {
    assert!(!super::is_production_script("test"));
    assert!(!super::is_production_script("lint"));
    assert!(!super::is_production_script("dev"));
    assert!(!super::is_production_script("storybook"));
    assert!(!super::is_production_script("typecheck"));
    assert!(!super::is_production_script("format"));
    assert!(!super::is_production_script("e2e"));
}

#[test]
fn mixed_operators_all_binaries_detected() {
    let cmds = parse_script("build && serve & watch || fallback");
    assert_eq!(cmds.len(), 4);
    assert_eq!(cmds[0].binary, "build");
    assert_eq!(cmds[1].binary, "serve");
    assert_eq!(cmds[2].binary, "watch");
    assert_eq!(cmds[3].binary, "fallback");
}

#[test]
fn background_with_env_vars() {
    let cmds = parse_script("NODE_ENV=production server &");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "server");
}

#[test]
fn trailing_background_operator() {
    let cmds = parse_script("webpack --watch &");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "webpack");
}

#[test]
fn filter_keeps_production_scripts() {
    let scripts: HashMap<String, String> = [
        ("build".to_string(), "webpack".to_string()),
        ("start".to_string(), "node server.js".to_string()),
        ("test".to_string(), "jest".to_string()),
        ("lint".to_string(), "eslint src".to_string()),
        ("dev".to_string(), "next dev".to_string()),
    ]
    .into_iter()
    .collect();

    let filtered = filter_production_scripts(&scripts);
    assert!(filtered.contains_key("build"));
    assert!(filtered.contains_key("start"));
    assert!(!filtered.contains_key("test"));
    assert!(!filtered.contains_key("lint"));
    assert!(!filtered.contains_key("dev"));
}

#[test]
fn npm_run_forwards_call_site_flags_into_script_body() {
    let result = analyze_ci_command(
        "npm run lint -- --format gha",
        &[("lint", "eslint .")],
        &["eslint", "eslint-formatter-gha"],
    );
    assert!(result.used_packages.contains("eslint"));
    assert!(result.used_packages.contains("eslint-formatter-gha"));
}

#[test]
fn yarn_script_without_double_dash_forwards_call_site_flags() {
    let result = analyze_ci_command(
        "yarn lint --format gha",
        &[("lint", "eslint .")],
        &["eslint", "eslint-formatter-gha"],
    );
    assert!(result.used_packages.contains("eslint-formatter-gha"));
}

#[test]
fn pnpm_and_bun_script_forms_forward_call_site_flags() {
    for command in ["pnpm lint --format gha", "bun run lint --format gha"] {
        let result = analyze_ci_command(
            command,
            &[("lint", "eslint .")],
            &["eslint", "eslint-formatter-gha"],
        );
        assert!(
            result.used_packages.contains("eslint-formatter-gha"),
            "{command} credited nothing"
        );
    }
}

#[test]
fn script_body_flags_and_call_site_flags_are_both_credited() {
    let result = analyze_ci_command(
        "npm run lint -- --format gha",
        &[("lint", "eslint . --format json")],
        &["eslint"],
    );
    assert!(result.used_packages.contains("eslint-formatter-json"));
    assert!(result.used_packages.contains("eslint-formatter-gha"));
}

#[test]
fn unknown_script_name_credits_nothing() {
    let result = analyze_ci_command(
        "npm run typecheck -- --format gha",
        &[("lint", "eslint .")],
        &["eslint"],
    );
    assert!(result.used_packages.is_empty());
}

#[test]
fn npm_run_without_double_dash_drops_call_site_flags() {
    let result = analyze_ci_command(
        "npm run lint --format gha",
        &[("lint", "eslint .")],
        &["eslint"],
    );
    assert!(
        !result.used_packages.contains("eslint-formatter-gha"),
        "npm consumes `--format` itself: {:?}",
        result.used_packages
    );
}

#[test]
fn npm_run_forwards_positional_arguments_without_double_dash() {
    let result = analyze_ci_command(
        "npm run gen src/input.ts",
        &[("gen", "mocha scripts/gen.ts")],
        &[],
    );
    assert!(
        result
            .entry_files
            .iter()
            .any(|file| file.ends_with("src/input.ts")),
        "npm forwards `src/input.ts` to the script: {:?}",
        result.entry_files
    );
}

#[test]
fn package_manager_builtin_is_not_a_script_invocation() {
    let result = analyze_ci_command(
        "yarn install --frozen-lockfile",
        &[("install", "eslint .")],
        &["eslint"],
    );
    assert!(result.used_packages.is_empty());
}

#[test]
fn mutually_recursive_scripts_terminate() {
    let result = analyze_ci_command(
        "npm run a -- --fix",
        &[
            ("a", "npm run b -- --format gha"),
            ("b", "npm run a -- --format json"),
        ],
        &["eslint"],
    );
    assert!(result.used_packages.is_empty());
}

#[test]
fn self_recursive_script_terminates_after_one_expansion() {
    let result = analyze_ci_command(
        "npm run loop -- --fix",
        &[("loop", "eslint . && npm run loop -- --format gha")],
        &["eslint"],
    );
    assert!(result.used_packages.contains("eslint"));
    assert!(!result.used_packages.contains("eslint-formatter-gha"));
}

#[test]
fn deep_script_chain_stops_at_the_depth_limit() {
    let chain: Vec<(String, String)> = (0..12)
        .map(|step| {
            (
                format!("s{step}"),
                if step == 11 {
                    "eslint .".to_string()
                } else {
                    format!("npm run s{} -- --cache", step + 1)
                },
            )
        })
        .collect();
    let scripts: Vec<(&str, &str)> = chain
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect();
    let result = analyze_ci_command("npm run s0 -- --fix", &scripts, &["eslint"]);
    assert!(result.used_packages.is_empty());

    let shallow = analyze_ci_command("npm run s9 -- --fix", &scripts, &["eslint"]);
    assert!(shallow.used_packages.contains("eslint"));
}

#[test]
fn ambiguous_script_body_is_not_followed() {
    let mut catalog = ScriptCatalog::from_scripts(&HashMap::from([(
        "lint".to_string(),
        "eslint .".to_string(),
    )]));
    catalog.merge_scripts(&HashMap::from([(
        "lint".to_string(),
        "biome check".to_string(),
    )]));
    let commands = vec!["npm run lint -- --format gha".to_string()];
    let result = analyze_commands_with_context(
        &commands,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["eslint", "biome"]),
        &catalog,
        IgnoredCommandEntries::NONE,
    );
    assert!(catalog.contains("lint"));
    assert!(result.used_packages.is_empty());
}

/// A third package restating one of the conflicting bodies must not undo the
/// ambiguity: which body wins would otherwise depend on workspace order.
#[test]
fn ambiguity_survives_a_third_package_restating_the_first_body() {
    let mut catalog = ScriptCatalog::from_scripts(&HashMap::from([(
        "lint".to_string(),
        "eslint .".to_string(),
    )]));
    catalog.merge_scripts(&HashMap::from([(
        "lint".to_string(),
        "biome check".to_string(),
    )]));
    catalog.merge_scripts(&HashMap::from([(
        "lint".to_string(),
        "eslint .".to_string(),
    )]));
    let commands = vec!["npm run lint -- --format gha".to_string()];
    let result = analyze_commands_with_context(
        &commands,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["eslint", "biome", "eslint-formatter-gha"]),
        &catalog,
        IgnoredCommandEntries::NONE,
    );
    assert!(catalog.contains("lint"));
    assert!(result.used_packages.is_empty());
}

/// The cycle guard only rejects names on the current path, so a branching
/// chain fans out per path. The expansion budget bounds the total work.
#[test]
fn branching_script_chain_is_bounded_by_the_expansion_budget() {
    let levels = MAX_SCRIPT_INDIRECTION_DEPTH;
    let chain: Vec<(String, String)> = (0..levels)
        .map(|step| {
            (
                format!("s{step}"),
                if step + 1 == levels {
                    "tsx leaf.js".to_string()
                } else {
                    format!(
                        "npm run s{next} -- --a && npm run s{next} -- --b",
                        next = step + 1
                    )
                },
            )
        })
        .collect();
    let scripts: Vec<(&str, &str)> = chain
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect();
    let result = analyze_ci_command("npm run s0 -- --go", &scripts, &["tsx"]);

    assert!(result.used_packages.contains("tsx"));
    assert!(!result.entry_files.is_empty());
    assert!(
        result.entry_files.len() <= MAX_SCRIPT_EXPANSIONS,
        "expansion budget should cap leaf visits, got {}",
        result.entry_files.len()
    );
}

/// Under production filtering only the bodies are filtered, so an
/// argument-bearing call still resolves to the script name but cannot reach
/// the dev-only body behind it.
#[test]
fn production_filtered_catalog_does_not_reach_a_dev_script_body() {
    let scripts = HashMap::from([
        (
            "build".to_string(),
            "npm run lint -- --fix && vite build".to_string(),
        ),
        ("lint".to_string(), "eslint .".to_string()),
    ]);
    let filtered = filter_production_scripts(&scripts);
    let result = analyze_scripts_with_dependency_context(
        &filtered,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["eslint", "vite"]),
        &ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered),
        IgnoredCommandEntries::NONE,
    );
    assert!(result.used_packages.contains("vite"));
    assert!(!result.used_packages.contains("eslint"));
}

/// The filtered catalog keeps every declared name, so a body reached
/// through indirection is unreachable while the name itself still resolves.
#[test]
fn filtered_catalog_keeps_names_and_drops_bodies() {
    let scripts = HashMap::from([
        ("build".to_string(), "vite build".to_string()),
        ("lint".to_string(), "eslint .".to_string()),
    ]);
    let filtered = filter_production_scripts(&scripts);
    let catalog = ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered);
    assert!(catalog.contains("lint"));
    assert!(catalog.contains("build"));
    assert!(catalog.body("lint").is_none());
    assert!(catalog.body("build").is_some());
}

/// A body reached both as its own script and through indirection must not
/// contribute the same entry file twice.
#[test]
fn repeated_expansion_does_not_duplicate_entry_files() {
    let scripts = HashMap::from([
        (
            "build".to_string(),
            "npm run bundle -- --minify && npm run bundle -- --watch".to_string(),
        ),
        (
            "bundle".to_string(),
            "esbuild scripts/bundle.js".to_string(),
        ),
    ]);
    let result = analyze_scripts_with_dependency_context(
        &scripts,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["esbuild"]),
        &ScriptCatalog::from_scripts(&scripts),
        IgnoredCommandEntries::NONE,
    );
    assert_eq!(result.entry_files, vec!["scripts/bundle.js".to_string()]);
}

/// A body merged from another workspace package still credits dependencies,
/// but its file arguments are relative to that package, not to the analysis
/// root, so they must not become entry files here.
#[test]
fn workspace_body_credits_packages_without_leaking_file_arguments() {
    let mut catalog = ScriptCatalog::default();
    catalog.merge_workspace_scripts(&HashMap::from([(
        "build".to_string(),
        "esbuild scripts/bundle.js".to_string(),
    )]));
    let commands = vec!["npm run build -- --mode ci".to_string()];
    let result = analyze_commands_with_context(
        &commands,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["esbuild"]),
        &catalog,
        IgnoredCommandEntries::NONE,
    );
    assert!(result.used_packages.contains("esbuild"));
    assert!(result.entry_files.is_empty());
}

#[test]
fn local_body_still_contributes_file_arguments() {
    let result = analyze_ci_command(
        "npm run build -- --mode ci",
        &[("build", "esbuild scripts/bundle.js")],
        &["esbuild"],
    );
    assert!(result.used_packages.contains("esbuild"));
    assert!(
        result
            .entry_files
            .contains(&"scripts/bundle.js".to_string())
    );
}

/// Built the same way as the production callers build it, so the guard this
/// test covers is the one that actually runs.
#[test]
fn production_filtered_context_skips_non_production_script_name() {
    let scripts = HashMap::from([
        ("build".to_string(), "pnpm lint".to_string()),
        ("lint".to_string(), "eslint src".to_string()),
    ]);
    let filtered = filter_production_scripts(&scripts);
    let result = analyze_scripts_with_dependency_context(
        &filtered,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["lint"]),
        &ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered),
        IgnoredCommandEntries::NONE,
    );
    assert!(!result.used_packages.contains("lint"));
}

/// `pnpm <name>` runs the declared script, never a same-named binary. A
/// filtered-out script keeps its name for exactly that reason: dropping it
/// would credit the dependency and pick up the remaining tokens as files.
#[test]
fn filtered_script_name_shadowing_a_dependency_bin_credits_nothing() {
    let scripts = HashMap::from([
        (
            "build".to_string(),
            "pnpm lint --fix src/app.ts".to_string(),
        ),
        ("lint".to_string(), "eslint src".to_string()),
    ]);
    let filtered = filter_production_scripts(&scripts);
    let result = analyze_scripts_with_dependency_context(
        &filtered,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &package_set(&["lint", "eslint"]),
        &ScriptCatalog::from_scripts_with_bodies(&scripts, &filtered),
        IgnoredCommandEntries::NONE,
    );
    assert!(!result.used_packages.contains("lint"));
    assert!(!result.used_packages.contains("eslint"));
    assert!(result.entry_files.is_empty());
}

#[test]
fn looks_like_file_path_with_known_extensions() {
    assert!(super::looks_like_file_path("src/app.ts"));
    assert!(super::looks_like_file_path("config.json"));
    assert!(super::looks_like_file_path("setup.yaml"));
    assert!(super::looks_like_file_path("rollup.config.mjs"));
    assert!(super::looks_like_file_path("test.spec.tsx"));
    assert!(super::looks_like_file_path("file.toml"));
}

#[test]
fn looks_like_file_path_with_relative_prefix() {
    assert!(super::looks_like_file_path("./scripts/build"));
    assert!(super::looks_like_file_path("../shared/utils"));
}

#[test]
fn looks_like_file_path_with_slash_but_not_scope() {
    assert!(super::looks_like_file_path("src/components/Button"));
    assert!(!super::looks_like_file_path("@scope/package")); // scoped package
}

#[test]
fn looks_like_file_path_url_not_file() {
    assert!(!super::looks_like_file_path("https://example.com/path"));
}

#[test]
fn looks_like_file_path_bare_word_not_file() {
    assert!(!super::looks_like_file_path("webpack"));
    assert!(!super::looks_like_file_path("--mode"));
    assert!(!super::looks_like_file_path("production"));
}

#[test]
fn looks_like_file_path_github_actions_expression_not_file() {
    assert!(!super::looks_like_file_path(
        r#""${{ env.ENVIRONMENT_URL }}/api/health/ready""#
    ));
    assert!(!super::looks_like_file_path("}}/api/health/ready\""));
    assert!(!super::looks_like_file_path("${{ env.BASE_URL }}"));
}

#[test]
fn looks_like_file_path_js_array_literal_not_file() {
    assert!(!super::looks_like_file_path("[packages/app/public]"));
    assert!(!super::looks_like_file_path("[packages/a/internal,"));
    assert!(super::looks_like_file_path("src/app/[id]/page.tsx"));
    assert!(super::looks_like_file_path("src/app/[...slug]/page.tsx"));
}

#[test]
fn looks_like_file_path_jq_array_iterator_not_file() {
    assert!(!super::looks_like_file_path(".[]"));
    assert!(!super::looks_like_file_path("'.[]'"));
}

/// Regression test for issue #2592: a quoted jq filter using the `//`
/// alternative operator was picked up as a file path candidate purely
/// because it contains a `/`, and its whitespace-separated words are the
/// distinguishing signal that it is shell/filter syntax, not a path.
#[test]
fn looks_like_file_path_jq_alternative_operator_not_file() {
    assert!(!super::looks_like_file_path(
        "[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]"
    ));
}

#[test]
fn looks_like_file_path_regex_fragment_not_file() {
    assert!(!super::looks_like_file_path(r")\./[^"));
    assert!(!super::looks_like_file_path(r"path\with\backslash"));
    assert!(!super::looks_like_file_path("prefix/[^unclosed"));
}

#[test]
fn looks_like_file_path_valid_nextjs_dynamic_route() {
    assert!(super::looks_like_file_path("app/[id]/page.tsx"));
    assert!(super::looks_like_file_path("pages/[...slug].ts"));
}

#[test]
fn could_be_file_path_passes_bare_names() {
    assert!(super::could_be_file_path("deploy.log"));
    assert!(super::could_be_file_path("Makefile"));
    assert!(super::could_be_file_path("Cargo.lock"));
}

#[test]
fn could_be_file_path_passes_balanced_mustache() {
    assert!(super::could_be_file_path("templates/{{name}}.hbs"));
    assert!(super::could_be_file_path("{{partial}}.html"));
}

#[test]
fn could_be_file_path_rejects_ghs_fragments() {
    assert!(!super::could_be_file_path("${{ env.X }}"));
    assert!(!super::could_be_file_path("}}/path"));
}

#[test]
fn could_be_file_path_rejects_regex_and_jq_fragments() {
    assert!(!super::could_be_file_path(r")\./[^"));
    assert!(!super::could_be_file_path(".[]"));
}

#[test]
fn extract_config_arg_with_equals() {
    assert_eq!(
        super::extract_config_arg("--config=webpack.prod.js", None),
        Some("webpack.prod.js".to_string())
    );
}

#[test]
fn extract_config_arg_short_with_equals() {
    assert_eq!(
        super::extract_config_arg("-c=.eslintrc.json", None),
        Some(".eslintrc.json".to_string())
    );
}

#[test]
fn extract_config_arg_with_next_token() {
    assert_eq!(
        super::extract_config_arg("--config", Some("jest.config.ts")),
        Some("jest.config.ts".to_string())
    );
}

#[test]
fn extract_config_arg_short_with_next_token() {
    assert_eq!(
        super::extract_config_arg("-c", Some(".eslintrc.json")),
        Some(".eslintrc.json".to_string())
    );
}

#[test]
fn extract_config_arg_next_is_flag_returns_none() {
    assert_eq!(
        super::extract_config_arg("--config", Some("--verbose")),
        None
    );
}

#[test]
fn extract_config_arg_no_match() {
    assert_eq!(super::extract_config_arg("--verbose", None), None);
    assert_eq!(super::extract_config_arg("src/index.ts", None), None);
}

#[test]
fn extract_config_arg_empty_equals_returns_none() {
    assert_eq!(super::extract_config_arg("--config=", None), None);
    assert_eq!(super::extract_config_arg("-c=", None), None);
}

#[test]
fn node_require_flag_skips_next_arg() {
    let cmds = parse_script("node -r tsconfig-paths/register ./src/server.ts");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "node");
    assert!(cmds[0].file_args.contains(&"./src/server.ts".to_string()));
    assert!(
        !cmds[0]
            .file_args
            .contains(&"tsconfig-paths/register".to_string())
    );
}

#[test]
fn node_eval_skips_next_arg() {
    let cmds = parse_script("node --eval \"console.log(1)\" scripts/run.js");
    assert_eq!(cmds.len(), 1);
    assert!(cmds[0].file_args.contains(&"scripts/run.js".to_string()));
}

#[test]
fn production_script_prepublish_only() {
    assert!(super::is_production_script("prepublishOnly"));
}

#[test]
fn production_script_postinstall() {
    assert!(super::is_production_script("postinstall"));
}

#[test]
fn production_script_preserve_is_not_production() {
    assert!(super::is_production_script("preserve"));
}

#[test]
fn production_script_preinstall() {
    assert!(super::is_production_script("preinstall"));
}

#[test]
fn production_script_namespaced() {
    assert!(super::is_production_script("build:esm"));
    assert!(super::is_production_script("start:dev"));
    assert!(!super::is_production_script("test:unit"));
    assert!(!super::is_production_script("lint:fix"));
}

#[test]
fn env_assignment_empty_value() {
    assert!(is_env_assignment("KEY="));
}

#[test]
fn env_assignment_equals_at_start_is_not_assignment() {
    assert!(!is_env_assignment("=value"));
}

#[test]
fn parse_empty_script() {
    let cmds = parse_script("");
    assert!(cmds.is_empty());
}

#[test]
fn parse_whitespace_only_script() {
    let cmds = parse_script("   ");
    assert!(cmds.is_empty());
}

#[test]
fn analyze_scripts_empty_scripts() {
    let scripts: HashMap<String, String> = HashMap::new();
    let result = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
    assert!(result.used_packages.is_empty());
    assert!(result.config_files.is_empty());
    assert!(result.entry_files.is_empty());
}

#[test]
fn bun_treated_as_package_manager() {
    let cmds = parse_script("bun scripts/build.ts");
    assert_eq!(cmds.len(), 1, "bare `bun <file>` runs the file");
    assert_eq!(cmds[0].binary, "bun");
    assert_eq!(cmds[0].file_args, vec!["scripts/build.ts"]);
    assert!(
        parse_script("bun dev").is_empty(),
        "bare `bun <name>` runs the script with that name"
    );
}

#[test]
fn bun_exec_extracts_binary() {
    let cmds = parse_script("bun exec vitest run");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "vitest");
}

#[test]
fn bun_runtime_flag_extracts_binary() {
    let cmds = parse_script("bun --bun prek install");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "prek");
}

#[test]
fn bun_multiple_runtime_flags_extract_binary() {
    let cmds = parse_script("bun --bun --watch prek");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "prek");
}

#[test]
fn bun_runtime_flag_before_run_is_script() {
    let cmds = parse_script("bun --watch run dev");
    assert!(cmds.is_empty());
}

#[test]
fn bun_unknown_flag_credits_nothing() {
    let cmds = parse_script("bun --filter foo run build");
    assert!(cmds.is_empty());
}

#[test]
fn bun_x_extracts_binary() {
    let cmds = parse_script("bun x cowsay hello");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "cowsay");
}

#[test]
fn concurrently_with_npm_prefix() {
    let scripts = HashMap::from([(
        "dev".to_string(),
        "concurrently \"npm:server\" \"npm:worker\"".to_string(),
    )]);
    let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
    assert!(result.used_packages.contains("concurrently"));
    assert!(!result.used_packages.contains("server"));
    assert!(!result.used_packages.contains("worker"));
    assert!(!result.used_packages.contains("npm:server"));
}

#[test]
fn run_p_with_bare_script_names() {
    let scripts = HashMap::from([("dev".to_string(), "run-p server worker".to_string())]);
    let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
    assert!(result.used_packages.contains("npm-run-all"));
    assert!(!result.used_packages.contains("server"));
    assert!(!result.used_packages.contains("worker"));
}

#[test]
fn run_s_with_bare_script_names() {
    let scripts = HashMap::from([("build".to_string(), "run-s clean compile".to_string())]);
    let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
    assert!(result.used_packages.contains("npm-run-all"));
    assert!(!result.used_packages.contains("clean"));
    assert!(!result.used_packages.contains("compile"));
}

#[test]
fn npm_run_all_with_script_names() {
    let scripts = HashMap::from([(
        "dev".to_string(),
        "npm-run-all --parallel server worker".to_string(),
    )]);
    let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
    assert!(result.used_packages.contains("npm-run-all"));
    assert!(!result.used_packages.contains("server"));
    assert!(!result.used_packages.contains("worker"));
}

#[test]
fn concurrently_with_flags_before_args() {
    let scripts = HashMap::from([(
        "dev".to_string(),
        "concurrently --kill-others \"npm:server\" \"npm:worker\"".to_string(),
    )]);
    let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
    assert!(result.used_packages.contains("concurrently"));
    assert!(!result.used_packages.contains("server"));
    assert!(!result.used_packages.contains("worker"));
    assert!(!result.used_packages.contains("kill-others"));
}

#[test]
fn concurrently_unquoted_npm_prefix() {
    let scripts = HashMap::from([(
        "dev".to_string(),
        "concurrently npm:dev npm:test".to_string(),
    )]);
    let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
    assert!(result.used_packages.contains("concurrently"));
    assert!(!result.used_packages.contains("dev"));
    assert!(!result.used_packages.contains("test"));
    assert!(!result.used_packages.contains("npm:dev"));
}

#[test]
fn run_p_with_npm_prefix() {
    let scripts = HashMap::from([(
        "dev".to_string(),
        "run-p \"npm:server\" \"npm:worker\"".to_string(),
    )]);
    let result = analyze_scripts(&scripts, Path::new("/fake"), &FxHashMap::default());
    assert!(result.used_packages.contains("npm-run-all"));
    assert!(!result.used_packages.contains("server"));
}

#[test]
fn node_test_quoted_glob_strips_quotes() {
    // Regression test for issue #841: quoted glob args kept their quotes,
    // causing looks_like_file_path to reject them and the entry pattern to
    // match zero files.
    let cmds = parse_script("node --test --import tsx 'src/**/*.test.ts'");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "node");
    // The surrounding quotes must be stripped from the glob.
    assert!(
        cmds[0].file_args.contains(&"src/**/*.test.ts".to_string()),
        "expected unquoted glob in file_args, got: {:?}",
        cmds[0].file_args
    );
    assert!(
        !cmds[0]
            .file_args
            .iter()
            .any(|f| f.starts_with('\'') || f.ends_with('\'')),
        "file_args must not contain surrounding single quotes"
    );
}

#[test]
fn bare_node_test_script_records_default_test_patterns() {
    for script in [
        "node --test",
        "NODE_ENV=test node --test",
        "node --experimental-strip-types --test",
        "node --test --watch",
        "node --test --import tsx --test-reporter spec",
    ] {
        let cmds = parse_script(script);
        assert_eq!(cmds.len(), 1, "`{script}`");
        assert_eq!(cmds[0].binary, "node", "`{script}`");
        assert!(
            cmds[0]
                .file_args
                .contains(&"**/*.test.{js,mjs,cjs,ts,mts,cts}".to_string()),
            "`{script}` produced {:?}",
            cmds[0].file_args
        );
        assert!(
            !cmds[0]
                .file_args
                .iter()
                .any(|arg| arg == "tsx" || arg == "spec"),
            "`{script}` recorded a flag value: {:?}",
            cmds[0].file_args
        );
    }
}

#[test]
fn node_test_with_file_argument_records_only_that_file() {
    let cmds = parse_script("node --test test/only.test.ts");
    assert_eq!(cmds[0].file_args, vec!["test/only.test.ts"]);
}

#[test]
fn node_test_setup_import_keeps_default_test_patterns() {
    let cmds = parse_script("node --import ./setup.ts --test");
    assert!(cmds[0].file_args.contains(&"./setup.ts".to_string()));
    assert!(
        cmds[0]
            .file_args
            .contains(&"**/test/**/*.{js,mjs,cjs,ts,mts,cts}".to_string())
    );
}

#[test]
fn ignored_node_command_drops_default_test_patterns() {
    let ignored = vec!["node".to_string()];
    let cmds = parse_script("node --test");
    assert!(
        cmds[0]
            .entry_files(IgnoredCommandEntries::new(&ignored))
            .is_empty()
    );
}

#[test]
fn test_runner_file_arguments_are_test_entry_files() {
    for (script, expected) in [
        (
            "node --test --test-reporter=spec test/a.test.mjs test/b.test.mjs",
            vec!["test/a.test.mjs", "test/b.test.mjs"],
        ),
        (
            "node --experimental-test-coverage --test 'scripts/*.test.mjs'",
            vec!["scripts/*.test.mjs"],
        ),
        (
            "node --test --import tsx src/a.test.ts",
            vec!["src/a.test.ts"],
        ),
        ("bun test ./test/a.test.ts", vec!["./test/a.test.ts"]),
        (
            "deno test --allow-read tests/a_test.ts",
            vec!["tests/a_test.ts"],
        ),
        (
            "cross-env TZ=UTC node --test test/a.test.mjs",
            vec!["test/a.test.mjs"],
        ),
        (
            "varlock run -- node --test test/a.test.mjs",
            vec!["test/a.test.mjs"],
        ),
    ] {
        let result = analyze_command(script, Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(result.test_entry_files, expected, "`{script}`");
        assert!(
            expected
                .iter()
                .all(|file| result.entry_files.contains(&(*file).to_string())),
            "`{script}` keeps the files as entry files: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn borp_scripts_record_test_entry_files() {
    let borp_default = vec!["**/*.test.{js,mjs,cjs,ts,mts,cts}"];
    for (script, expected) in [
        ("borp -p 'test/**/*.js'", vec!["test/**/*.js"]),
        (
            "borp --pattern \"test/**/*.test.ts\"",
            vec!["test/**/*.test.ts"],
        ),
        (
            "borp --pattern=test/**/*.mjs --coverage",
            vec!["test/**/*.mjs"],
        ),
        ("npx borp -C -p 'test/*.js'", vec!["test/*.js"]),
        ("borp", borp_default.clone()),
        ("borp --coverage --check-coverage -c 1", borp_default),
        (
            "borp -C -c 1 test/a.test.js test/b.test.js",
            vec!["test/a.test.js", "test/b.test.js"],
        ),
        (
            "borp -p 'src/**/*.js' test/a.test.js",
            vec!["test/a.test.js"],
        ),
        (
            "borp 'test/**/*.test.js' '!test/fixtures/**'",
            vec!["test/**/*.test.js"],
        ),
        (
            "borp -r ./test/reporter.js -i 'test/fixtures/**' -p 'test/*.js'",
            vec!["test/*.js"],
        ),
    ] {
        let result = analyze_command(script, Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(result.test_entry_files, expected, "`{script}`");
        assert!(
            expected
                .iter()
                .all(|file| result.entry_files.contains(&(*file).to_string())),
            "`{script}` keeps the files as entry files: {:?}",
            result.entry_files
        );
        assert!(
            !result.entry_files.iter().any(|file| file.starts_with('!')),
            "`{script}` keeps no exclusion glob as an entry file: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn borp_flag_values_that_name_no_test_file_are_not_entry_files() {
    let result = analyze_command(
        "borp -p 'src/**/*.js' -i 'test/fixtures/**' -X 'test/**' test/a.test.js",
        Path::new("/nonexistent"),
        &FxHashMap::default(),
    );
    assert_eq!(result.entry_files, vec!["test/a.test.js"]);
}

#[test]
fn node_flag_values_are_not_test_entry_files() {
    for (script, tests) in [
        (
            "node --test --test-coverage-include 'src/**/*.mjs' test/a.test.mjs",
            vec!["test/a.test.mjs"],
        ),
        (
            "node --test --test-reporter ./test/reporter.mjs test/a.test.mjs",
            vec!["test/a.test.mjs"],
        ),
        (
            "node --test --experimental-loader ./loader.mjs test/a.test.mjs",
            vec!["test/a.test.mjs"],
        ),
        (
            "node --test --test-global-setup ./test/global.mjs test/a.test.mjs",
            vec!["test/a.test.mjs"],
        ),
        (
            "node --test --import ./test/setup.mjs test/a.test.mjs",
            vec!["./test/setup.mjs", "test/a.test.mjs"],
        ),
        (
            "cross-env CI=1 node --test --test-reporter ./test/reporter.mjs test/a.test.mjs",
            vec!["test/a.test.mjs"],
        ),
    ] {
        let result = analyze_command(script, Path::new("/nonexistent"), &FxHashMap::default());
        assert_eq!(result.test_entry_files, tests, "`{script}`");
    }
}

#[test]
fn node_flag_values_that_name_no_module_are_not_entry_files() {
    let result = analyze_command(
        "node --test --test-coverage-include 'src/**/*.mjs' --test-coverage-exclude test/fixtures/a.mjs test/a.test.mjs",
        Path::new("/nonexistent"),
        &FxHashMap::default(),
    );
    assert_eq!(result.entry_files, vec!["test/a.test.mjs"]);
}

#[test]
fn node_flag_values_that_name_a_module_stay_entry_files() {
    let result = analyze_command(
        "node --test --test-reporter ./test/reporter.mjs --experimental-loader ./loader.mjs test/a.test.mjs",
        Path::new("/nonexistent"),
        &FxHashMap::default(),
    );
    for file in ["./test/reporter.mjs", "./loader.mjs", "test/a.test.mjs"] {
        assert!(
            result.entry_files.contains(&file.to_string()),
            "{file} in {:?}",
            result.entry_files
        );
    }
}

#[test]
fn bun_test_in_a_script_named_test_runs_the_test_runner() {
    let scripts = HashMap::from([("test".to_string(), "bun test ./test/a.test.ts".to_string())]);
    let result = analyze_scripts_with_dependencies(
        &scripts,
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &FxHashSet::default(),
    );
    assert_eq!(result.test_entry_files, vec!["./test/a.test.ts"]);
    assert!(result.entry_files.contains(&"./test/a.test.ts".to_string()));
}

#[test]
fn other_script_file_arguments_are_not_test_entry_files() {
    for script in [
        "node scripts/build.mjs --test",
        "tsx scripts/test.ts",
        "bun run test",
        "deno run scripts/test.ts",
        "node --test-reporter=spec scripts/run.mjs",
    ] {
        let result = analyze_command(script, Path::new("/nonexistent"), &FxHashMap::default());
        assert!(
            result.test_entry_files.is_empty(),
            "`{script}` produced {:?}",
            result.test_entry_files
        );
    }
}

#[test]
fn node_test_unquoted_glob_still_works() {
    // Unquoted globs must continue to be extracted correctly.
    let cmds = parse_script("node --test src/**/*.test.ts");
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].binary, "node");
    assert!(cmds[0].file_args.contains(&"src/**/*.test.ts".to_string()));
}

#[test]
fn referenced_scripts_include_plain_npm_run_without_forwarded_args() {
    let scripts = HashMap::from([
        ("start".to_string(), "npm run serve".to_string()),
        ("serve".to_string(), "node src/server.ts".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);

    let referenced = referenced_package_scripts(&scripts["start"], &catalog);

    assert_eq!(referenced, FxHashSet::from_iter(["serve".to_string()]));
}

#[test]
fn referenced_scripts_follow_complete_command_wrappers() {
    let scripts = HashMap::from([("launch".to_string(), "next start".to_string())]);
    let catalog = ScriptCatalog::from_scripts(&scripts);
    for command in [
        "varlock run -- npm run launch",
        "varlock run -- cross-env NODE_ENV=production npm run launch",
        "varlock run -- varlock run -- pnpm run launch",
    ] {
        assert_eq!(
            referenced_package_scripts(command, &catalog),
            FxHashSet::from_iter(["launch".to_string()]),
            "{command}"
        );
    }
    for command in [
        "varlock check -- npm run launch",
        "varlock run npm run launch",
        "varlock run -- pnpm --filter app run launch",
        "varlock run -- npm --prefix packages/app run launch",
    ] {
        assert_eq!(
            referenced_package_scripts(command, &catalog),
            FxHashSet::default(),
            "{command}"
        );
    }
}

#[test]
fn referenced_scripts_honor_pnpm_silent_shorthand() {
    let scripts = HashMap::from([
        ("start".to_string(), "pnpm -s serve".to_string()),
        ("serve".to_string(), "node src/server.ts".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);

    let referenced = referenced_package_scripts(&scripts["start"], &catalog);

    assert_eq!(referenced, FxHashSet::from_iter(["serve".to_string()]));
}

#[test]
fn referenced_scripts_honor_package_manager_options() {
    let scripts = HashMap::from([
        ("start".to_string(), "npm --silent run serve".to_string()),
        ("serve".to_string(), "node src/server.ts".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);
    assert_eq!(
        referenced_package_scripts(&scripts["start"], &catalog),
        FxHashSet::from_iter(["serve".to_string()])
    );

    let command = "pnpm --filter app run serve";
    assert_eq!(
        referenced_package_scripts(command, &catalog),
        FxHashSet::default(),
        "workspace-qualified calls must not promote a same-named local script"
    );
}

#[test]
fn referenced_scripts_include_multiplexer_targets() {
    let scripts = HashMap::from([
        (
            "start".to_string(),
            "run-p serve worker && concurrently \"npm:monitor\"".to_string(),
        ),
        ("serve".to_string(), "node src/server.ts".to_string()),
        ("worker".to_string(), "node src/worker.ts".to_string()),
        ("monitor".to_string(), "node src/monitor.ts".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);

    let referenced = referenced_package_scripts(&scripts["start"], &catalog);

    assert_eq!(
        referenced,
        FxHashSet::from_iter([
            "serve".to_string(),
            "worker".to_string(),
            "monitor".to_string(),
        ])
    );
}

#[test]
fn referenced_scripts_skip_multiplexer_option_values() {
    let scripts = HashMap::from([
        (
            "start".to_string(),
            "concurrently --names serve npm:worker npm:api".to_string(),
        ),
        ("serve".to_string(), "node src/server.ts".to_string()),
        ("worker".to_string(), "node src/worker.ts".to_string()),
        ("api".to_string(), "node src/api.ts".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);

    let referenced = referenced_package_scripts(&scripts["start"], &catalog);

    assert_eq!(
        referenced,
        FxHashSet::from_iter(["worker".to_string(), "api".to_string()])
    );
}

#[test]
fn referenced_scripts_include_bun_implicit_target() {
    let scripts = HashMap::from([
        ("start".to_string(), "bun serve".to_string()),
        ("serve".to_string(), "bun src/server.ts".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);

    let referenced = referenced_package_scripts(&scripts["start"], &catalog);

    assert_eq!(referenced, FxHashSet::from_iter(["serve".to_string()]));
}

#[test]
fn referenced_workspace_scripts_preserve_package_identity() {
    let mut packages = WorkspacePackages::default();
    let serve =
        std::collections::HashMap::from([("serve".to_string(), "node src/server.ts".to_string())]);
    packages.add("@scope/api", "packages/api", Some(&serve));
    packages.add("@scope/web", "packages/web", None);
    let catalog = ScriptCatalog::default().with_workspaces(std::sync::Arc::new(packages), "");
    for command in [
        "pnpm --filter @scope/api run serve",
        "pnpm --filter=@scope/api run serve",
        "npm --workspace @scope/api run serve",
        "npm --workspace=@scope/api run serve",
        "yarn workspace @scope/api serve",
        "pnpm -r run serve",
        "pnpm --recursive serve",
        "pnpm -C packages/api run serve",
        "pnpm --dir=packages/api serve",
        "npm --prefix packages/api run serve",
        "npm run serve --prefix packages/api",
        "yarn --cwd packages/api serve",
        "yarn --cwd=packages/api run serve",
        "yarn workspaces foreach -A run serve",
        "yarn workspaces run serve",
        "npm -ws run serve",
    ] {
        assert_eq!(
            referenced_workspace_scripts(command, &catalog),
            vec![("packages/api".to_string(), "serve".to_string())],
            "failed to parse {command}"
        );
    }
    for command in [
        "pnpm --filter @scope/web run serve",
        "pnpm -C packages/other run serve",
        "yarn workspaces foreach --since run serve",
        "pnpm run serve",
    ] {
        assert!(
            referenced_workspace_scripts(command, &catalog).is_empty(),
            "{command} calls no script of a workspace package"
        );
    }
}

#[test]
fn concurrently_parses_quoted_inline_commands() {
    let cmds = parse_script(
        r#"concurrently -n api,web -c red,blue --kill-others-on-fail "tsx watch src/api.ts" "vite" npm:worker"#,
    );
    let binaries: Vec<&str> = cmds.iter().map(|cmd| cmd.binary.as_str()).collect();
    assert_eq!(binaries, vec!["concurrently", "tsx", "vite"]);
    assert_eq!(cmds[1].file_args, vec!["src/api.ts".to_string()]);
}

#[test]
fn concurrently_teardown_value_is_a_command() {
    let cmds = parse_script(r#"concurrently --teardown "node scripts/stop.js" "node src/a.js""#);
    let binaries: Vec<&str> = cmds.iter().map(|cmd| cmd.binary.as_str()).collect();
    assert_eq!(binaries, vec!["concurrently", "node", "node"]);
    assert_eq!(cmds[1].file_args, vec!["scripts/stop.js".to_string()]);
    assert_eq!(cmds[2].file_args, vec!["src/a.js".to_string()]);
}

#[test]
fn concurrently_passthrough_arguments_are_not_commands() {
    let cmds = parse_script(r#"concurrently -P "node src/a.js {1}" -- extra.js"#);
    let binaries: Vec<&str> = cmds.iter().map(|cmd| cmd.binary.as_str()).collect();
    assert_eq!(binaries, vec!["concurrently", "node"]);
}

#[test]
fn run_p_positionals_stay_script_names() {
    let cmds = parse_script(r#"run-p "tsx src/api.ts" serve"#);
    let binaries: Vec<&str> = cmds.iter().map(|cmd| cmd.binary.as_str()).collect();
    assert_eq!(binaries, vec!["run-p"]);
}

#[test]
fn referenced_scripts_include_concurrently_inline_calls() {
    let scripts = HashMap::from([
        (
            "start".to_string(),
            r#"concurrently "npm run serve" "node src/a.js""#.to_string(),
        ),
        ("serve".to_string(), "node src/server.ts".to_string()),
    ]);
    let catalog = ScriptCatalog::from_scripts(&scripts);

    let referenced = referenced_package_scripts(&scripts["start"], &catalog);

    assert_eq!(referenced, FxHashSet::from_iter(["serve".to_string()]));
}

#[test]
fn referenced_workspace_scripts_include_concurrently_inline_calls() {
    let mut packages = WorkspacePackages::default();
    let dev = std::collections::HashMap::from([(
        "dev".to_string(),
        "tsx watch src/server.ts".to_string(),
    )]);
    packages.add("@scope/api", "packages/api", Some(&dev));
    packages.add("@scope/web", "packages/web", Some(&dev));
    let catalog = ScriptCatalog::default().with_workspaces(std::sync::Arc::new(packages), "");
    let command =
        r#"concurrently -n api,web "npm run dev -w @scope/api" "pnpm --filter @scope/web dev""#;

    assert_eq!(
        referenced_workspace_scripts(command, &catalog),
        vec![
            ("packages/api".to_string(), "dev".to_string()),
            ("packages/web".to_string(), "dev".to_string()),
        ]
    );
}

#[test]
fn varlock_preserves_quoted_argument_boundaries() {
    for command in [
        r#"varlock run -p "./env -- is-ci ignored" -- publint"#,
        "varlock run -p './env -- is-ci ignored' -- publint",
        r"varlock run -p ./env\ --\ is-ci\ ignored -- publint",
        r#"varlock run -p "./env \" -- is-ci ignored" -- publint"#,
    ] {
        let result = analyze_scripts_with_dependencies(
            &HashMap::from([("check".to_string(), command.to_string())]),
            Path::new("/nonexistent"),
            &FxHashMap::default(),
            &package_set(&["varlock", "is-ci", "publint"]),
        );
        assert_eq!(
            result.used_packages,
            package_set(&["varlock", "publint"]),
            "{command}"
        );
    }
}

#[test]
fn varlock_forwards_quoted_arguments_to_package_scripts() {
    let result = analyze_ci_command(
        r#"varlock run -p "./env with spaces" -- npm run child -- -p "./env -- is-ci ignored" -- publint"#,
        &[("child", "varlock run")],
        &["varlock", "is-ci", "publint"],
    );
    assert_eq!(result.used_packages, package_set(&["varlock", "publint"]));
}

#[test]
fn varlock_preserves_quoted_child_paths() {
    let commands = parse_script(r#"varlock run -- node "./scripts/worker with spaces.js""#);
    assert!(commands.iter().any(|command| {
        command
            .file_args
            .contains(&"./scripts/worker with spaces.js".to_string())
    }));
}

#[test]
fn varlock_does_not_guess_through_unbalanced_or_dynamic_words() {
    // The body of a command substitution is a command of its own.
    for (command, binaries) in [
        (
            r#"varlock run -p "./env -- is-ci ignored -- publint"#,
            vec!["varlock"],
        ),
        (
            "varlock run -p './env -- is-ci ignored -- publint",
            vec!["varlock"],
        ),
        (
            "varlock run -p $(echo -- is-ci) -- publint",
            vec!["varlock", "echo"],
        ),
        (
            "varlock run -p `echo -- is-ci` -- publint",
            vec!["varlock", "echo"],
        ),
    ] {
        let commands = parse_script(command);
        assert_eq!(
            commands
                .iter()
                .map(|command| command.binary.as_str())
                .collect::<Vec<_>>(),
            binaries,
            "{command}"
        );
    }
}

#[test]
fn varlock_keeps_known_child_before_dynamic_arguments() {
    // The body of a command substitution is a command of its own.
    for (command, substituted) in [
        ("varlock run -- vite --host=$(hostname)", "hostname"),
        ("varlock run -- vite $(pwd)", "pwd"),
        ("varlock run -- vite `pwd`", "pwd"),
    ] {
        let commands = parse_script(command);
        assert_eq!(
            commands
                .iter()
                .map(|command| command.binary.as_str())
                .collect::<Vec<_>>(),
            vec!["varlock", "vite", substituted],
            "{command}"
        );
    }
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// parse_script should never panic on arbitrary input.
        #[test]
        fn parse_script_no_panic(s in "[a-zA-Z0-9 _./@&|;=\"'-]{1,200}") {
            let _ = parse_script(&s);
        }

        /// split_shell_operators should never panic on arbitrary input.
        #[test]
        fn split_shell_operators_no_panic(s in "[a-zA-Z0-9 _./@&|;=\"'-]{1,200}") {
            let _ = shell::split_shell_operators(&s);
        }

        /// When parse_script returns commands, binary names should be non-empty.
        #[test]
        fn parsed_binaries_are_non_empty(
            binary in "[a-z][a-z0-9-]{0,20}",
            args in "[a-zA-Z0-9 _./=-]{0,50}",
        ) {
            let script = format!("{binary} {args}");
            let commands = parse_script(&script);
            for cmd in &commands {
                prop_assert!(!cmd.binary.is_empty(), "Binary name should never be empty");
            }
        }

        /// analyze_scripts should never panic on arbitrary script values.
        #[test]
        fn analyze_scripts_no_panic(
            name in "[a-z]{1,10}",
            value in "[a-zA-Z0-9 _./@&|;=-]{1,100}",
        ) {
            let scripts: HashMap<String, String> = std::iter::once((name, value)).collect();
            let _ = analyze_scripts(&scripts, Path::new("/nonexistent"), &FxHashMap::default());
        }

        /// is_env_assignment should never panic on arbitrary input.
        #[test]
        fn is_env_assignment_no_panic(s in "[a-zA-Z0-9_=./-]{1,50}") {
            let _ = is_env_assignment(&s);
        }

        /// resolve_binary_to_packages should always return non-empty package names.
        #[test]
        fn resolve_binary_always_non_empty(binary in "[a-z][a-z0-9-]{0,20}") {
            let result = resolve_binary_to_packages(&binary, Path::new("/nonexistent"), &FxHashMap::default());
            prop_assert!(!result.is_empty(), "Package list should never be empty");
            prop_assert!(result.iter().all(|pkg| !pkg.is_empty()), "Package name should never be empty");
        }

        /// Chained scripts should produce at least as many commands as operators + 1
        /// when each segment is a valid binary (excluding package managers and builtins).
        #[test]
        fn chained_binaries_produce_multiple_commands(
            bins in prop::collection::vec("[a-z][a-z0-9]{0,10}", 2..5),
        ) {
            let reserved = ["npm", "npx", "yarn", "pnpm", "pnpx", "bun", "bunx",
                "node", "env", "cross", "sh", "bash", "exec", "sudo", "nohup"];
            prop_assume!(!bins.iter().any(|b| reserved.contains(&b.as_str())));
            let script = bins.join(" && ");
            let commands = parse_script(&script);
            prop_assert!(
                commands.len() >= 2,
                "Chained commands should produce multiple parsed commands, got {}",
                commands.len()
            );
        }
    }
}

#[test]
fn script_binaries_skip_downloaded_commands_only() {
    for (script, expected) in [
        (
            "npx create-thing && gnome-extensions pack",
            vec!["gnome-extensions"],
        ),
        ("pnpm dlx create-thing", vec![]),
        ("pnpm --filter web dlx create-thing", vec![]),
        ("yarn dlx create-thing", vec![]),
        ("bunx tool", vec![]),
        ("bun x tool", vec![]),
        ("npm x tool", vec![]),
        ("cross-env A=1 npx tool", vec![]),
        ("A=1 hyprctl reload", vec!["hyprctl"]),
    ] {
        assert_eq!(super::script_binaries(script), expected, "{script}");
    }
}

#[test]
fn script_binaries_keep_a_script_named_x() {
    for script in ["yarn x", "pnpm x"] {
        assert!(
            !super::downloads_its_command(script),
            "{script} runs the script `x`, so it is not a download"
        );
    }
}

#[test]
fn node_modules_bin_path_credits_the_package_of_the_binary() {
    let mut bin_map = FxHashMap::default();
    bin_map.insert("kysely-codegen".to_string(), "kysely-codegen".to_string());
    for script in [
        "./node_modules/.bin/kysely-codegen --out-file x.ts",
        "node_modules/.bin/kysely-codegen",
        "../../node_modules/.bin/kysely-codegen",
        "node ./node_modules/.bin/kysely-codegen",
        "tsx node_modules/.bin/kysely-codegen",
        "$(npm bin)/kysely-codegen",
    ] {
        let result = analyze_command(script, Path::new("/nonexistent"), &bin_map);
        assert!(
            result.used_packages.contains("kysely-codegen"),
            "{script}: {:?}",
            result.used_packages
        );
    }
}

#[cfg(unix)]
#[test]
fn node_modules_bin_path_follows_the_link_relative_to_the_given_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let workspace = dir.path().join("packages/app");
    let bin_dir = dir.path().join("node_modules/.bin");
    std::fs::create_dir_all(&workspace).expect("workspace dir");
    std::fs::create_dir_all(&bin_dir).expect("bin dir");
    std::os::unix::fs::symlink("../@scope/gen/cli.js", bin_dir.join("gen")).expect("bin link");

    let result = analyze_command(
        "../../node_modules/.bin/gen",
        &workspace,
        &FxHashMap::default(),
    );
    assert!(
        result.used_packages.contains("@scope/gen"),
        "{:?}",
        result.used_packages
    );
}
