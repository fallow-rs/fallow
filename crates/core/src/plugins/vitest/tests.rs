use super::*;

fn resolve(source: &str) -> PluginResult {
    VitestPlugin.resolve_config(
        std::path::Path::new("vitest.config.ts"),
        source,
        std::path::Path::new("/project"),
    )
}

#[test]
fn include_entry_patterns_accept_every_extglob_form() {
    let result = resolve(
        "export default defineConfig({ test: {
            include: [
                'at/**/*.@(test|spec).ts',
                './neg/**/!(*.skip).spec.ts',
                'plain/*.(unit|e2e).ts',
            ],
            projects: [{ test: { include: ['proj/*.?(c)test.js'] } }],
        } });",
    );
    let matches = |path: &str| super::super::entry_rules_match(&result, path);
    assert!(matches("at/a/x.test.ts"));
    assert!(matches("at/b.spec.ts"));
    assert!(!matches("at/b.ts"));
    assert!(matches("neg/a/b.spec.ts"));
    assert!(!matches("neg/a/b.skip.spec.ts"));
    assert!(matches("plain/a.unit.ts"));
    assert!(!matches("plain/a.int.ts"));
    assert!(matches("proj/a.test.js"));
    assert!(matches("proj/a.ctest.js"));
}

#[test]
fn package_jsx_import_source_credits_the_package_without_a_rule() {
    let result = VitestPlugin.resolve_config(
        std::path::Path::new("/project/vitest.config.ts"),
        "export default defineConfig({ oxc: { jsx: { importSource: '@emotion/react' } } });",
        std::path::Path::new("/project"),
    );
    assert!(result.jsx_import_sources.is_empty());
    assert!(
        result
            .referenced_dependencies
            .contains(&"@emotion/react".to_string())
    );
}

fn jsx_rules(source: &str) -> Vec<(String, Vec<String>)> {
    let result = VitestPlugin.resolve_config(
        std::path::Path::new("/project/vitest.config.ts"),
        source,
        std::path::Path::new("/project"),
    );
    result
        .jsx_import_sources
        .iter()
        .map(|rule| {
            assert_eq!(rule.config_dir, std::path::Path::new("/project"));
            (rule.source.clone(), rule.include.clone())
        })
        .collect()
}

#[test]
fn jsx_import_source_at_root_uses_root_include() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { runtime: 'automatic', importSource: './src/jsx' } },
            test: { include: ['./src/**/*.test.tsx'] },
        });
    ";
    assert_eq!(
        jsx_rules(source),
        vec![(
            "./src/jsx".to_string(),
            vec!["src/**/*.test.tsx".to_string()]
        )]
    );
}

#[test]
fn jsx_import_source_at_root_falls_back_to_default_include() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './jsx' } },
        });
    ";
    assert_eq!(
        jsx_rules(source),
        vec![(
            "./jsx".to_string(),
            vec![VITEST_DEFAULT_INCLUDE.to_string()]
        )]
    );
}

#[test]
fn jsx_import_source_per_project() {
    let source = r"
        export default defineConfig({
            test: {
                include: ['src/**/*.spec.ts'],
                projects: [
                    './packages/*/vitest.config.ts',
                    {
                        oxc: { jsx: { runtime: 'automatic', importSource: './src/jsx' } },
                        extends: true,
                        test: {
                            include: ['src/**/(*.)+(spec|test).+(ts|tsx|js)'],
                            name: 'main',
                        },
                    },
                    {
                        oxc: { jsx: { importSource: './src/jsx/dom' } },
                        test: { name: 'dom' },
                    },
                ],
            },
        });
    ";
    assert_eq!(
        jsx_rules(source),
        vec![
            (
                "./src/jsx".to_string(),
                vec![
                    "src/**/*.spec.ts".to_string(),
                    // `globset` cannot repeat a group, so a `+(..)` group
                    // becomes its alternatives and then `*`.
                    "src/**/*.{spec,test}*.{ts,tsx,js}*".to_string()
                ]
            ),
            (
                "./src/jsx/dom".to_string(),
                vec!["src/**/*.spec.ts".to_string()]
            ),
        ]
    );
}

#[test]
fn inline_project_inherits_root_jsx_unless_extends_is_false() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './jsx' } },
            test: {
                projects: [
                    { extends: true, test: { include: ['a/**/*.test.tsx'] } },
                    { test: { include: ['b/**/*.test.tsx'] } },
                    { extends: false, test: { include: ['c/**/*.test.tsx'] } },
                    {
                        extends: false,
                        oxc: { jsx: { importSource: './own' } },
                        test: { include: ['d/**/*.test.tsx'] },
                    },
                    {
                        oxc: { jsx: { runtime: 'automatic' } },
                        test: { include: ['e/**/*.test.tsx'] },
                    },
                ],
            },
        });
    ";
    assert_eq!(
        jsx_rules(source),
        vec![
            ("./jsx".to_string(), vec!["a/**/*.test.tsx".to_string()]),
            ("./jsx".to_string(), vec!["b/**/*.test.tsx".to_string()]),
            ("./own".to_string(), vec!["d/**/*.test.tsx".to_string()]),
            ("./jsx".to_string(), vec!["e/**/*.test.tsx".to_string()]),
        ]
    );
}

#[test]
fn inherited_include_concatenates_and_extends_false_uses_the_default() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './jsx' } },
            test: {
                include: ['src/**/*.test.tsx'],
                projects: [
                    { test: { include: ['pkg/**/*.test.tsx'] } },
                    { extends: true },
                    { extends: false },
                    { extends: false, oxc: { jsx: { importSource: './own' } } },
                ],
            },
        });
    ";
    assert_eq!(
        jsx_rules(source),
        vec![
            (
                "./jsx".to_string(),
                vec![
                    "src/**/*.test.tsx".to_string(),
                    "pkg/**/*.test.tsx".to_string()
                ]
            ),
            ("./jsx".to_string(), vec!["src/**/*.test.tsx".to_string()]),
            (
                "./own".to_string(),
                vec![VITEST_DEFAULT_INCLUDE.to_string()]
            ),
        ]
    );
}

#[test]
fn include_is_relative_to_the_test_dir_or_the_project_root() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './jsx' } },
            test: {
                projects: [
                    { test: { root: 'pkg', include: ['src/**/*.test.tsx'] } },
                    { root: './app', test: { dir: 'tests' } },
                ],
            },
        });
    ";
    assert_eq!(
        jsx_rules(source),
        vec![
            (
                "./jsx".to_string(),
                vec!["pkg/src/**/*.test.tsx".to_string()]
            ),
            (
                "./jsx".to_string(),
                vec![format!("app/tests/{VITEST_DEFAULT_INCLUDE}")]
            ),
        ]
    );
}

#[test]
fn extends_path_reads_the_named_config_and_credits_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path();
    std::fs::write(
        config_dir.join("base.config.ts"),
        "export default defineConfig({ oxc: { jsx: { importSource: './base-jsx' } }, test: { include: ['base/**/*.test.tsx'] } });",
    )
    .expect("write base config");
    let config_path = config_dir.join("vitest.config.ts");
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './root-jsx' } },
            test: {
                projects: [
                    { extends: './base.config.ts', test: { include: ['e/**/*.test.tsx'] } },
                    { extends: './vitest.config.ts', test: { include: ['s/**/*.test.tsx'] } },
                    { extends: './missing.config.ts', test: { include: ['m/**/*.test.tsx'] } },
                ],
            },
        });
    ";
    let result = VitestPlugin.resolve_config(&config_path, source, config_dir);
    let rules: Vec<(String, Vec<String>)> = result
        .jsx_import_sources
        .iter()
        .map(|rule| (rule.source.clone(), rule.include.clone()))
        .collect();
    assert_eq!(
        rules,
        vec![
            (
                "./base-jsx".to_string(),
                vec![
                    "base/**/*.test.tsx".to_string(),
                    "e/**/*.test.tsx".to_string()
                ]
            ),
            (
                "./root-jsx".to_string(),
                vec!["s/**/*.test.tsx".to_string()]
            ),
        ]
    );
    assert!(
        result
            .setup_files
            .contains(&config_dir.join("base.config.ts")),
        "the extended config file must be credited: {:?}",
        result.setup_files
    );
}

fn jsx_excludes(source: &str) -> Vec<Vec<String>> {
    VitestPlugin
        .resolve_config(
            std::path::Path::new("/project/vitest.config.ts"),
            source,
            std::path::Path::new("/project"),
        )
        .jsx_import_sources
        .into_iter()
        .map(|rule| rule.exclude)
        .collect()
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn exclude_defaults_to_the_vitest_default_and_a_config_exclude_replaces_it() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './jsx' } },
            test: {
                projects: [
                    { test: { name: 'default' } },
                    { test: { exclude: ['./src/skip', 'src/one.test.tsx', '!keep/**'] } },
                    { test: { exclude: ['/project/abs/**', '/elsewhere/**'] } },
                    { test: { include: ['src/**/*.test.tsx', '!src/gen/**'] } },
                ],
            },
        });
    ";
    assert_eq!(
        jsx_excludes(source),
        vec![
            strings(&["**/node_modules/**", "**/.git/**"]),
            strings(&[
                "src/skip",
                "src/skip/**",
                "src/one.test.tsx",
                "src/one.test.tsx/**",
            ]),
            strings(&["abs/**"]),
            strings(&["**/node_modules/**", "**/.git/**", "src/gen/**"]),
        ]
    );
}

#[test]
fn inherited_exclude_concatenates_and_follows_the_project_root() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './jsx' } },
            test: {
                exclude: ['skip/**'],
                projects: [
                    { test: { exclude: ['other/**'] } },
                    { test: { root: 'pkg' } },
                    { extends: false, oxc: { jsx: { importSource: './jsx' } } },
                ],
            },
        });
    ";
    assert_eq!(
        jsx_excludes(source),
        vec![
            strings(&["skip/**", "other/**"]),
            strings(&["pkg/skip/**"]),
            strings(&["**/node_modules/**", "**/.git/**"]),
        ]
    );
}

fn jsx_credits(path: &str, source: &str) -> Vec<(String, Vec<String>)> {
    VitestPlugin
        .resolve_config(
            std::path::Path::new(path),
            source,
            std::path::Path::new("/project"),
        )
        .jsx_package_credits
        .into_iter()
        .map(|rule| (rule.source, rule.include))
        .collect()
}

#[test]
fn config_without_import_source_credits_the_default_react_runtime() {
    let source = r"
        export default defineConfig({
            test: {
                projects: [
                    { test: { include: ['a/**/*.test.tsx'] } },
                    { oxc: false, test: { include: ['b/**/*.test.tsx'] } },
                    { oxc: { jsx: 'preserve' }, test: { include: ['c/**/*.test.tsx'] } },
                    {
                        oxc: { jsx: { runtime: 'classic' } },
                        test: { include: ['d/**/*.test.tsx'] },
                    },
                    {
                        oxc: { jsx: { importSource: './jsx' } },
                        test: { include: ['e/**/*.test.tsx'] },
                    },
                ],
            },
        });
    ";
    assert_eq!(
        jsx_credits("/project/vitest.config.ts", source),
        vec![("react".to_string(), strings(&["a/**/*.test.tsx"]))]
    );
    assert_eq!(
        jsx_credits(
            "/project/vitest.config.ts",
            "export default defineConfig({ test: {} });"
        ),
        vec![(
            "react".to_string(),
            vec![VITEST_DEFAULT_INCLUDE.to_string()]
        )]
    );
    assert!(
        jsx_credits(
            "/project/vitest.config.ts",
            "export default defineConfig({ oxc: false, test: {} });"
        )
        .is_empty()
    );
}

#[test]
fn project_config_files_named_by_path_are_credited() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path();
    std::fs::create_dir_all(config_dir.join("e2e")).expect("create e2e");
    std::fs::write(
        config_dir.join("e2e/vitest.e2e.config.ts"),
        "export default defineConfig({ oxc: { jsx: { importSource: './jsx' } } });",
    )
    .expect("write project config");
    std::fs::write(config_dir.join("e2e/helper.ts"), "export default {};").expect("write helper");
    let source = r"
        export default defineConfig({
            test: {
                projects: [
                    './e2e/vitest.e2e.config.ts',
                    './e2e/helper.ts',
                    './missing/vitest.config.ts',
                    './packages/*/vitest.unit.config.ts',
                    { test: { name: 'inline' } },
                ],
            },
        });
    ";
    let result =
        VitestPlugin.resolve_config(&config_dir.join("vitest.config.ts"), source, config_dir);
    assert_eq!(
        result.setup_files,
        vec![config_dir.join("e2e/vitest.e2e.config.ts")]
    );
    let rules: Vec<(&str, &Path)> = result
        .jsx_import_sources
        .iter()
        .map(|rule| (rule.source.as_str(), rule.config_dir.as_path()))
        .collect();
    assert_eq!(rules, vec![("./jsx", config_dir.join("e2e").as_path())]);
}

#[test]
fn project_glob_entries_read_the_matching_config_files() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config_dir = temp.path();
    for dir in ["runtime/r1", "runtime/r2", "node_modules/x/runtime/r3"] {
        std::fs::create_dir_all(config_dir.join(dir)).expect("create dir");
    }
    let project = "export default defineConfig({ oxc: { jsx: { importSource: './jsx' } } });";
    for file in [
        "runtime/r1/vitest.e2e.config.ts",
        "runtime/r2/vitest.e2e.config.ts",
        "node_modules/x/runtime/r3/vitest.e2e.config.ts",
    ] {
        std::fs::write(config_dir.join(file), project).expect("write project config");
    }
    let source = r"
        export default defineConfig({
            test: {
                projects: [
                    'runtime/*/vitest.e2e.config.ts',
                    './**/r3/vitest.e2e.config.ts',
                    'runtime/{r1,r2}/vitest.e2e.config.ts',
                ],
            },
        });
    ";
    let result =
        VitestPlugin.resolve_config(&config_dir.join("vitest.config.ts"), source, config_dir);
    assert_eq!(
        result.setup_files,
        vec![
            config_dir.join("runtime/r1/vitest.e2e.config.ts"),
            config_dir.join("runtime/r2/vitest.e2e.config.ts"),
        ]
    );
    let dirs: Vec<PathBuf> = result
        .jsx_import_sources
        .iter()
        .map(|rule| rule.config_dir.clone())
        .collect();
    assert_eq!(
        dirs,
        vec![config_dir.join("runtime/r1"), config_dir.join("runtime/r2")]
    );
}

/// A pnpm workspace links each dependency, and each workspace package,
/// into `node_modules`. Workspace packages that depend on each other
/// make a symlink cycle. A project glob with a `**` after its base must
/// not walk through these links.
#[cfg(unix)]
#[test]
fn project_glob_does_not_walk_node_modules_symlinks() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().to_path_buf();
    for app in ["a", "b"] {
        let dir = root.join("apps").join(app);
        std::fs::create_dir_all(dir.join("node_modules")).expect("create dir");
        std::fs::write(
            dir.join("vitest.e2e.config.ts"),
            "export default defineConfig({ test: {} });",
        )
        .expect("write project config");
        // Three links back to the workspaces: each walk level that
        // follows them multiplies the number of paths by three.
        for (index, target) in ["../../a", "../../b", "../.."].iter().enumerate() {
            std::os::unix::fs::symlink(target, dir.join(format!("node_modules/l{index}")))
                .expect("create symlink");
        }
    }
    let source =
        "export default defineConfig({ test: { projects: ['apps/**/vitest.e2e.config.ts'] } });";
    let config = root.join("vitest.config.ts");
    let (sender, receiver) = std::sync::mpsc::channel();
    let walk_root = root.clone();
    std::thread::spawn(move || {
        let result = VitestPlugin.resolve_config(&config, source, &walk_root);
        let _ = sender.send(result.setup_files);
    });
    let setup_files = receiver
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("the project glob walked into the node_modules symlinks");
    assert_eq!(
        setup_files,
        vec![
            root.join("apps/a/vitest.e2e.config.ts"),
            root.join("apps/b/vitest.e2e.config.ts"),
        ]
    );
}

/// A project glob whose literal base is inside `node_modules` names a
/// dependency, not a project of the analyzed repository.
#[test]
fn project_glob_with_a_node_modules_base_gives_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let config = root.join("node_modules/pkg/a/vitest.config.ts");
    std::fs::create_dir_all(config.parent().expect("parent")).expect("create dir");
    std::fs::write(&config, "").expect("write config");
    assert!(project_config_paths("node_modules/pkg/*/vitest.config.ts", root, root).is_empty());
    assert!(
        project_config_paths("*/vitest.config.ts", &root.join("node_modules/pkg"), root).is_empty()
    );
}

#[test]
fn vite_config_next_to_a_vitest_config_gives_no_jsx_rule() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path();
    let vite_config = dir.join("vite.config.ts");
    let vite_source = "export default defineConfig({ oxc: { jsx: { importSource: './jsx' } } });";
    let rules = |dir: &Path| {
        let result = VitestPlugin.resolve_config(&vite_config, vite_source, dir);
        (
            result.jsx_import_sources.len(),
            result.jsx_package_credits.len(),
        )
    };
    // Without a vitest config, Vitest loads the vite config.
    assert_eq!(rules(dir), (1, 0));

    std::fs::write(
        dir.join("vitest.config.ts"),
        "export default defineConfig({ test: {} });",
    )
    .expect("write vitest config");
    assert_eq!(rules(dir), (0, 0));

    // A vitest config that merges the vite config gets its settings.
    std::fs::write(
        dir.join("vitest.config.ts"),
        "import viteConfig from './vite.config';\nexport default mergeConfig(viteConfig, defineConfig({ test: {} }));",
    )
    .expect("write merging vitest config");
    assert_eq!(rules(dir), (1, 0));

    let plain = VitestPlugin.resolve_config(&vite_config, "export default defineConfig({});", dir);
    assert_eq!(plain.jsx_package_credits.len(), 1);
}

#[test]
fn vitest4_inline_project_inherits_only_with_extends_true() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = temp.path();
    std::fs::write(
        dir.join("package.json"),
        r#"{ "devDependencies": { "vitest": "^4.1.0" } }"#,
    )
    .expect("write package.json");
    let source = r"
        export default defineConfig({
            oxc: { jsx: { importSource: './jsx' } },
            test: {
                projects: [
                    { test: { include: ['a/**/*.test.tsx'] } },
                    { extends: true, test: { include: ['b/**/*.test.tsx'] } },
                ],
            },
        });
    ";
    let rules = |dir: &Path| -> Vec<(String, Vec<String>)> {
        VitestPlugin
            .resolve_config(&dir.join("vitest.config.ts"), source, dir)
            .jsx_import_sources
            .into_iter()
            .map(|rule| (rule.source, rule.include))
            .collect()
    };
    assert_eq!(
        rules(dir),
        vec![("./jsx".to_string(), strings(&["b/**/*.test.tsx"]))]
    );

    // The installed version wins over the declared range.
    std::fs::create_dir_all(dir.join("node_modules/vitest")).expect("create vitest dir");
    std::fs::write(
        dir.join("node_modules/vitest/package.json"),
        r#"{ "name": "vitest", "version": "5.0.1" }"#,
    )
    .expect("write installed vitest");
    assert_eq!(
        rules(dir),
        vec![
            ("./jsx".to_string(), strings(&["a/**/*.test.tsx"])),
            ("./jsx".to_string(), strings(&["b/**/*.test.tsx"])),
        ]
    );
}

#[test]
fn vitest_range_max_major_reads_the_upper_major() {
    assert_eq!(range_max_major("^4.0.0"), Some(4));
    assert_eq!(range_max_major("~4.1.11"), Some(4));
    assert_eq!(range_max_major("4.x"), Some(4));
    assert_eq!(range_max_major("^4.0.0 || ^5.0.0"), Some(5));
    assert_eq!(range_max_major("npm:vitest@^3.2.0"), Some(3));
    assert_eq!(range_max_major(">=4"), None);
    assert_eq!(range_max_major("latest"), None);
    assert_eq!(range_max_major("catalog:"), None);
    assert_eq!(range_max_major("*"), None);
}

#[test]
fn vitest_config_names_follow_the_vitest_pattern() {
    for name in [
        "vitest.config.ts",
        "vite.config.mjs",
        "vitest.e2e.config.ts",
        "vite.browser-node.config.js",
    ] {
        assert!(is_vitest_config_name(name), "{name}");
    }
    for name in [
        "vitest.setup.ts",
        "vitest.a.b.config.ts",
        "jest.e2e.config.ts",
    ] {
        assert!(!is_vitest_config_name(name), "{name}");
    }
}

#[test]
fn classic_jsx_runtime_adds_no_rule() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { runtime: 'automatic', importSource: './jsx' } },
            test: {
                projects: [
                    {
                        extends: true,
                        oxc: { jsx: { runtime: 'classic' } },
                        test: { include: ['a/**/*.test.tsx'] },
                    },
                    {
                        esbuild: { jsx: 'transform', jsxImportSource: 'solid-js' },
                        test: { include: ['b/**/*.test.tsx'] },
                    },
                    {
                        oxc: { jsx: 'preserve' },
                        test: { include: ['c/**/*.test.tsx'] },
                    },
                ],
            },
        });
    ";
    assert!(jsx_rules(source).is_empty());
}

#[test]
fn esbuild_jsx_import_source_is_read() {
    let source = r"
        export default defineConfig({
            esbuild: { jsx: 'automatic', jsxImportSource: './src/jsx' },
            test: { include: ['test/**/*.test.tsx'] },
        });
    ";
    assert_eq!(
        jsx_rules(source),
        vec![(
            "./src/jsx".to_string(),
            vec!["test/**/*.test.tsx".to_string()]
        )]
    );
}

#[test]
fn config_without_jsx_import_source_adds_no_rule() {
    let source = r"
        export default defineConfig({
            oxc: { jsx: { runtime: 'automatic' } },
            test: { include: ['src/**/*.test.tsx'] },
        });
    ";
    assert!(jsx_rules(source).is_empty());
}

#[test]
fn plain_groups_become_exactly_one_groups() {
    assert_eq!(
        vitest_extglob("./src/**/(*.)+(spec|test).+(ts|tsx|js)"),
        "src/**/@(*.)+(spec|test).+(ts|tsx|js)"
    );
    assert_eq!(vitest_extglob("src/(a|(b))/*.ts"), "src/@(a|@(b))/*.ts");
    assert_eq!(vitest_extglob(r"src/\(a\)/*.ts"), r"src/\(a\)/*.ts");
    assert_eq!(vitest_extglob("src/!(skip)/*.ts"), "src/!(skip)/*.ts");
}

#[test]
fn include_and_exclude_accept_every_extglob_form() {
    let result = VitestPlugin.resolve_config(
        Path::new("/project/vitest.config.ts"),
        r"export default defineConfig({
            oxc: { jsx: { runtime: 'automatic', importSource: './src/jsx' } },
            test: {
                include: [
                    'neg/**/!(*.skip).test.tsx',
                    'opt/*.test.?(m)jsx',
                    'plus/*.+(a|b).test.tsx',
                    'star/x*(y).test.tsx',
                    'at/*.@(unit|e2e).tsx',
                    'plain/(*.)+(spec|check).tsx',
                ],
                exclude: ['at/*.@(e2e).tsx'],
            },
        });",
        Path::new("/project"),
    );
    let rules = fallow_graph::resolve::compile_jsx_rules(&result.jsx_import_sources);
    let matches = |path: &str| {
        rules
            .iter()
            .any(|rule| rule.matches(&Path::new("/project").join(path)))
    };

    assert!(matches("neg/a.test.tsx"));
    assert!(matches("neg/deep/a.test.tsx"));
    assert!(!matches("neg/a.skip.test.tsx"));
    // The negated group of one include does not exclude the files that
    // another include matches.
    assert!(matches("opt/a.skip.test.jsx"));
    assert!(matches("opt/a.test.mjsx"));
    assert!(matches("plus/c.a.test.tsx"));
    assert!(matches("plus/c.ab.test.tsx"));
    assert!(!matches("plus/c.test.tsx"));
    assert!(matches("star/x.test.tsx"));
    assert!(matches("star/xyy.test.tsx"));
    assert!(matches("at/a.unit.tsx"));
    assert!(!matches("at/a.e2e.tsx"));
    assert!(matches("plain/a.spec.tsx"));
    assert!(!matches("plain/a.other.tsx"));
}

/// Issue #2226: neither runner gives literal `X/__mocks__` imports special
/// meaning, so Vitest declares no virtual package suffixes and a literal
/// import of such a package is reported as unlisted, matching Jest.
#[test]
fn no_virtual_package_suffixes_declared() {
    assert!(
        VitestPlugin.virtual_package_suffixes().is_empty(),
        "VitestPlugin must not suppress /__mocks__ package names"
    );
}

#[test]
fn reporters_string_array() {
    let source = r#"
        export default {
            test: {
                reporters: ["default", "vitest-sonar-reporter"]
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"vitest-sonar-reporter".to_string())
    );
}

#[test]
fn reporters_tuple_format() {
    let source = r#"
        export default {
            test: {
                reporters: ["default", ["vitest-sonar-reporter", { outputFile: "report.xml" }]]
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"vitest-sonar-reporter".to_string())
    );
}

#[test]
fn reporters_builtin_filtered() {
    let source = r#"
        export default {
            test: {
                reporters: ["default", "verbose", "json", "junit", "html"]
            }
        };
    "#;
    let result = resolve(source);
    let non_import_deps: Vec<_> = result
        .referenced_dependencies
        .iter()
        .filter(|d| !d.contains('/') || d.starts_with('@'))
        .collect();
    assert!(
        non_import_deps.is_empty(),
        "Built-in reporters should not be referenced dependencies: {non_import_deps:?}"
    );
}

#[test]
fn reporters_scoped_package() {
    let source = r#"
        export default {
            test: {
                reporters: ["@vitest/reporter-html"]
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"@vitest/reporter-html".to_string())
    );
}

#[test]
fn reporters_missing_does_not_error() {
    let source = r#"
        export default {
            test: {
                include: ["**/*.test.ts"]
            }
        };
    "#;
    let result = resolve(source);
    assert!(result.referenced_dependencies.is_empty());
}

#[test]
fn credits_react_babel_plugin_dependencies() {
    let source = r#"
        import { defineConfig } from "vitest/config";
        import react from "@vitejs/plugin-react";

        export default defineConfig({
            plugins: [
                react({
                    babel: {
                        plugins: [["module:@preact/signals-react-transform", {}]],
                        presets: ["@babel/preset-react"],
                    },
                }),
            ],
        });
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"@preact/signals-react-transform".to_string()),
        "React Babel plugin dependency should be credited: {:?}",
        result.referenced_dependencies
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"@babel/preset-react".to_string()),
        "React Babel preset dependency should be credited: {:?}",
        result.referenced_dependencies
    );
}

#[test]
fn custom_environment() {
    let source = r#"
        export default {
            test: {
                environment: "custom-env"
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"vitest-environment-custom-env".to_string())
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"custom-env".to_string())
    );
}

/// `edge-runtime` is a built-in vitest environment backed by the optional
/// peer `@edge-runtime/vm`. Crediting the shorthand names instead exempted an
/// unrelated CLI package and left the real dependency reported as unused.
#[test]
fn builtin_edge_runtime_environment_credits_vm_package() {
    let source = r#"
        export default {
            test: {
                environment: "edge-runtime"
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"@edge-runtime/vm".to_string()),
        "expected @edge-runtime/vm, got {:?}",
        result.referenced_dependencies
    );
    assert!(
        !result
            .referenced_dependencies
            .contains(&"vitest-environment-edge-runtime".to_string()),
        "no such package exists"
    );
    assert!(
        !result
            .referenced_dependencies
            .contains(&"edge-runtime".to_string()),
        "the bare name is an unrelated package and must not be exempted"
    );
}

#[test]
fn coverage_provider_custom() {
    let source = r#"
        export default {
            test: {
                coverage: {
                    provider: "custom-provider"
                }
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"@vitest/coverage-custom-provider".to_string())
    );
}

#[test]
fn coverage_provider_builtin_filtered() {
    let source = r#"
        export default {
            test: {
                coverage: {
                    provider: "v8"
                }
            }
        };
    "#;
    let result = resolve(source);
    assert!(result.referenced_dependencies.is_empty());
}

#[test]
fn coverage_provider_istanbul_builtin() {
    let source = r#"
        export default {
            test: {
                coverage: {
                    provider: "istanbul"
                }
            }
        };
    "#;
    let result = resolve(source);
    assert!(result.referenced_dependencies.is_empty());
}

#[test]
fn typecheck_checker_vue_tsc() {
    let source = r#"
        export default {
            test: {
                typecheck: {
                    checker: "vue-tsc"
                }
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"vue-tsc".to_string())
    );
}

#[test]
fn typecheck_checker_tsc_builtin() {
    let source = r#"
        export default {
            test: {
                typecheck: {
                    checker: "tsc"
                }
            }
        };
    "#;
    let result = resolve(source);
    assert!(result.referenced_dependencies.is_empty());
}

#[test]
fn browser_provider_playwright() {
    let source = r#"
        export default {
            test: {
                browser: {
                    provider: "playwright"
                }
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result
            .referenced_dependencies
            .contains(&"@vitest/browser".to_string())
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"playwright".to_string())
    );
}

#[test]
fn browser_provider_preview_builtin() {
    let source = r#"
        export default {
            test: {
                browser: {
                    provider: "preview"
                }
            }
        };
    "#;
    let result = resolve(source);
    assert!(result.referenced_dependencies.is_empty());
}

#[test]
fn test_include_sets_replace_entry_patterns() {
    let source = r#"
        export default {
            test: {
                include: ["src/**/*.test.ts"]
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        result.replace_entry_patterns,
        "test.include should trigger replacement of static entry patterns"
    );
    assert_eq!(
        result.entry_patterns,
        vec!["src/**/*.test.ts", VITEST_DEFAULT_BENCH_INCLUDE]
    );
}

#[test]
fn test_include_keeps_the_default_bench_include() {
    let result = resolve(r#"export default { test: { include: ["test/**/*.test.ts"] } };"#);
    assert!(result.replace_entry_patterns);
    assert!(
        result
            .entry_patterns
            .iter()
            .any(|rule| rule.pattern == VITEST_DEFAULT_BENCH_INCLUDE),
        "a replacing test.include must keep the bench files: {:?}",
        result.entry_patterns
    );
}

#[test]
fn benchmark_include_replaces_the_default_bench_include() {
    let result = resolve(
        r#"export default {
            test: {
                include: ["test/**/*.test.ts"],
                benchmark: { include: ["perf/**/*.perf.ts"] },
            },
        };"#,
    );
    assert_eq!(
        result.entry_patterns,
        vec!["test/**/*.test.ts", "perf/**/*.perf.ts"]
    );
}

#[test]
fn benchmark_include_without_test_include_adds_entries() {
    let result =
        resolve(r#"export default { test: { benchmark: { include: ["perf/*.perf.ts"] } } };"#);
    assert!(!result.replace_entry_patterns);
    assert_eq!(result.entry_patterns, vec!["perf/*.perf.ts"]);
}

#[test]
fn default_bench_pattern_matches_benchmark_and_module_extensions() {
    let glob = globset::Glob::new(VITEST_DEFAULT_BENCH_INCLUDE)
        .unwrap()
        .compile_matcher();
    for path in [
        "a.bench.ts",
        "x/a.benchmark.mts",
        "a.bench.cjs",
        "a.benchmark.jsx",
    ] {
        assert!(glob.is_match(path), "{path}");
    }
    assert!(ENTRY_PATTERNS.contains(&VITEST_DEFAULT_BENCH_INCLUDE));
}

fn resolve_at(config: &str, source: &str) -> PluginResult {
    VitestPlugin.resolve_config(Path::new(config), source, Path::new("/project"))
}

#[test]
fn nested_config_include_is_scoped_to_its_directory() {
    let result = resolve_at(
        "/project/fixtures/app/vite.config.ts",
        r#"export default {
            test: {
                include: ["./src/**/*.test.ts"],
                projects: [{ test: { include: ["unit.test.js"] } }],
            },
        };"#,
    );
    assert!(
        !result.replace_entry_patterns,
        "a config below the root must not replace the defaults of the project"
    );
    // The inline project merges the root include, so that glob is also an
    // entry of the project, with the project excludes.
    assert_eq!(
        entry_globs(&result),
        vec!["fixtures/app/src/**/*.test.ts", "fixtures/app/unit.test.js"]
    );
}

#[test]
fn script_config_below_the_root_resolves_globs_against_the_root() {
    let result = resolve_at(
        "/project/config/vitest.ci.ts",
        r#"export default {
            test: {
                include: ["src/**/*.test.ts"],
                reporters: ["./scripts/ci-reporter.ts"],
            },
        };"#,
    );
    assert!(result.replace_entry_patterns);
    assert_eq!(
        result.entry_patterns,
        vec!["src/**/*.test.ts", VITEST_DEFAULT_BENCH_INCLUDE]
    );
    assert!(
        result
            .setup_files
            .contains(&PathBuf::from("/project/scripts/ci-reporter.ts"))
    );
}

#[test]
fn named_variant_config_below_the_root_resolves_globs_against_the_root() {
    let result = resolve_at(
        "/project/config/vitest.config.e2e.ts",
        r#"export default { test: { include: ["e2e/**/*.test.ts"] } };"#,
    );
    assert!(result.replace_entry_patterns);
    assert_eq!(
        result.entry_patterns,
        vec!["e2e/**/*.test.ts", VITEST_DEFAULT_BENCH_INCLUDE]
    );
}

#[test]
fn root_config_include_still_replaces() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        r#"export default { test: { include: ["src/**/*.test.ts"] } };"#,
    );
    assert!(result.replace_entry_patterns);
    assert!(result.accumulate_config_entry_patterns);
}

#[test]
fn shadowed_vite_config_include_is_ignored() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("vitest.config.ts"), "export default {};").unwrap();
    let vite = dir.path().join("vite.config.ts");
    let result = VitestPlugin.resolve_config(
        &vite,
        r#"export default { test: { include: ["src/**/*.test.ts"] } };"#,
        dir.path(),
    );
    assert!(!result.replace_entry_patterns);
    assert!(result.entry_patterns.is_empty());
}

#[test]
fn vite_config_that_the_vitest_config_names_is_not_shadowed() {
    let dir = tempfile::tempdir().unwrap();
    let vite = dir.path().join("vite.config.ts");
    for vitest_source in [
        "export default { test: { projects: ['./vite.config.ts'] } };",
        "export default { test: { projects: [{ extends: './vite.config.ts' }] } };",
        "import base from './vite.config';\nexport default base;",
    ] {
        std::fs::write(dir.path().join("vitest.config.ts"), vitest_source).unwrap();
        assert!(!is_shadowed_vite_config(&vite), "{vitest_source}");
    }
    std::fs::write(dir.path().join("vitest.config.ts"), "export default {};").unwrap();
    assert!(is_shadowed_vite_config(&vite));
}

#[test]
fn path_reporter_is_a_file_not_a_dependency() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        r#"export default {
            test: {
                reporters: ["default", "./scripts/ci-reporter.ts", ["../shared/r.ts", {}], "vitest-sonar-reporter"],
            },
        };"#,
    );
    assert_eq!(
        result.referenced_dependencies,
        vec!["vitest-sonar-reporter"]
    );
    assert!(
        result
            .setup_files
            .contains(&PathBuf::from("/project/scripts/ci-reporter.ts")),
        "{:?}",
        result.setup_files
    );
    assert!(
        !result.setup_files.contains(&PathBuf::from("/shared/r.ts")),
        "a reporter path outside the root credits no file"
    );
}

#[test]
fn reporter_behind_a_conditional_spread_is_read() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        r#"const isCI = Boolean(process.env.CI);
        const config = defineConfig({
            test: {
                reporters: [
                    "default",
                    ...(isCI ? [["json", { outputFile: "r.json" }] as ["json", object]] : []),
                    ...(process.env.REPORT === "1" ? ["./scripts/ci-reporter.ts"] : []),
                    ...(isCI && ["vitest-sonar-reporter"]),
                ],
            },
        });
        export default config;"#,
    );
    assert_eq!(
        result.referenced_dependencies,
        vec!["vitest-sonar-reporter"]
    );
    assert!(
        result
            .setup_files
            .contains(&PathBuf::from("/project/scripts/ci-reporter.ts")),
        "{:?}",
        result.setup_files
    );
}

#[test]
fn no_test_include_keeps_defaults() {
    let source = r#"
        export default {
            test: {
                environment: "jsdom"
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        !result.replace_entry_patterns,
        "without test.include, static patterns should be kept"
    );
    assert!(result.entry_patterns.is_empty());
}

#[test]
fn project_level_include_does_not_replace_defaults() {
    let source = r#"
        export default {
            test: {
                projects: [
                    {
                        test: {
                            name: "unit-jsdom",
                            include: ["packages/vue/**/*.spec.ts"],
                        }
                    }
                ]
            }
        };
    "#;
    let result = resolve(source);
    assert!(
        !result.replace_entry_patterns,
        "project-level test.include should not replace static defaults"
    );
    assert_eq!(result.entry_patterns, vec!["packages/vue/**/*.spec.ts"]);
}

fn resolve_abs(source: &str) -> PluginResult {
    VitestPlugin.resolve_config(
        std::path::Path::new("/project/vitest.config.ts"),
        source,
        std::path::Path::new("/project"),
    )
}

#[test]
fn test_alias_object_form_virtual_module() {
    let source = r#"
        export default {
            test: {
                alias: { vscode: "./test/mock/vscode.js" }
            }
        };
    "#;
    let result = resolve_abs(source);
    assert_eq!(
        result.path_aliases,
        vec![("vscode".to_string(), "test/mock/vscode.js".to_string())]
    );
    assert!(
        result
            .setup_files
            .contains(&std::path::PathBuf::from("/project/test/mock/vscode.js")),
        "local mock file should be seeded as a support entry point: {:?}",
        result.setup_files
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"vscode".to_string()),
        "bare-package alias key should be credited as referenced"
    );
}

#[test]
fn test_alias_array_form_with_find_replacement() {
    let source = r#"
        export default {
            test: {
                alias: [{ find: "vscode", replacement: "./test/mock/vscode.js" }]
            }
        };
    "#;
    let result = resolve_abs(source);
    assert_eq!(
        result.path_aliases,
        vec![("vscode".to_string(), "test/mock/vscode.js".to_string())]
    );
    assert!(
        result
            .setup_files
            .contains(&std::path::PathBuf::from("/project/test/mock/vscode.js"))
    );
}

#[test]
fn test_alias_resolve_replacement_for_scoped_mock() {
    let source = r#"
        import { resolve } from "node:path";
        export default {
            test: {
                alias: {
                    "@scope/pkg": resolve(__dirname, "__mocks__/@scope/pkg.ts")
                }
            }
        };
    "#;
    let result = resolve_abs(source);
    assert_eq!(
        result.path_aliases,
        vec![(
            "@scope/pkg".to_string(),
            "__mocks__/@scope/pkg.ts".to_string()
        )]
    );
    assert!(
        result.setup_files.contains(&std::path::PathBuf::from(
            "/project/__mocks__/@scope/pkg.ts"
        )),
        "scoped mock file should be seeded: {:?}",
        result.setup_files
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"@scope/pkg".to_string()),
        "aliased real dependency should stay credited"
    );
}

#[test]
fn test_alias_projects_nested() {
    let source = r#"
        export default {
            test: {
                projects: [
                    {
                        test: {
                            name: "unit",
                            alias: { vscode: "./test/mock/vscode.js" }
                        }
                    }
                ]
            }
        };
    "#;
    let result = resolve_abs(source);
    assert_eq!(
        result.path_aliases,
        vec![("vscode".to_string(), "test/mock/vscode.js".to_string())]
    );
    assert!(
        result
            .setup_files
            .contains(&std::path::PathBuf::from("/project/test/mock/vscode.js"))
    );
}

#[test]
fn test_alias_projects_nested_new_url_pathname() {
    let source = r#"
        export default {
            test: {
                projects: [
                    {
                        test: {
                            alias: {
                                "test-alias-from-vitest": new URL("./space/test-alias-to.ts", import.meta.url).pathname
                            }
                        }
                    }
                ]
            }
        };
    "#;
    let result = resolve_abs(source);
    assert_eq!(
        result.path_aliases,
        vec![(
            "test-alias-from-vitest".to_string(),
            "space/test-alias-to.ts".to_string()
        )]
    );
    assert!(
        result
            .setup_files
            .contains(&std::path::PathBuf::from("/project/space/test-alias-to.ts"))
    );
}

#[test]
fn test_alias_directory_target_not_seeded_as_entry_point() {
    let source = r#"
        export default {
            test: {
                alias: { "@/": "./src" }
            }
        };
    "#;
    let result = resolve_abs(source);
    assert_eq!(
        result.path_aliases,
        vec![("@/".to_string(), "src".to_string())]
    );
    assert!(
        result.setup_files.is_empty(),
        "directory alias target should not be seeded: {:?}",
        result.setup_files
    );
}

#[test]
fn test_alias_package_to_package_credits_both_no_path_alias() {
    let source = r#"
        export default {
            test: {
                alias: { "lodash-es": "lodash" }
            }
        };
    "#;
    let result = resolve_abs(source);
    assert!(
        result.path_aliases.is_empty(),
        "package-to-package alias should emit no path alias: {:?}",
        result.path_aliases
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"lodash".to_string()),
        "alias target package should be credited"
    );
    assert!(
        result
            .referenced_dependencies
            .contains(&"lodash-es".to_string()),
        "alias source package should be credited"
    );
}

#[test]
fn test_alias_regexp_key_skipped_without_panic() {
    let source = r#"
        export default {
            test: {
                alias: [{ find: /^msw\/(.*)/, replacement: "./test/mock/msw.js" }]
            }
        };
    "#;
    let result = resolve_abs(source);
    assert!(
        result.path_aliases.is_empty(),
        "RegExp alias key should be skipped: {:?}",
        result.path_aliases
    );
}

#[test]
fn top_level_resolve_alias_extracted_from_vitest_config() {
    let source = r#"
        import { resolve } from "node:path";
        export default {
            resolve: {
                alias: { "vite/module-runner": resolve(__dirname, "src/module-runner/index.ts") }
            },
            test: { include: ["**/*.spec.ts"] }
        };
    "#;
    let result = resolve_abs(source);
    assert!(
        result.path_aliases.contains(&(
            "vite/module-runner".to_string(),
            "src/module-runner/index.ts".to_string()
        )),
        "top-level resolve.alias must be extracted: {:?}",
        result.path_aliases
    );
}

#[test]
fn project_level_resolve_alias_extracted() {
    let source = r#"
        export default {
            test: {
                projects: [
                    { test: { name: "browser" }, resolve: { alias: { "test-alias-from-vite": "./mock/to.ts" } } }
                ]
            }
        };
    "#;
    let result = resolve_abs(source);
    assert!(
        result
            .path_aliases
            .contains(&("test-alias-from-vite".to_string(), "mock/to.ts".to_string())),
        "project-level resolve.alias must be extracted: {:?}",
        result.path_aliases
    );
}

#[test]
fn function_form_define_config_test_alias_extracted() {
    let source = r#"
        import { defineConfig } from "vitest/config";
        export default defineConfig(() => ({
            test: { alias: { vscode: "./test/mock/vscode.ts" } }
        }));
    "#;
    let result = resolve_abs(source);
    assert!(
        result
            .path_aliases
            .contains(&("vscode".to_string(), "test/mock/vscode.ts".to_string())),
        "function-form defineConfig test.alias must be extracted: {:?}",
        result.path_aliases
    );
}

/// A `test.projects` entry that names a config outside the project root
/// is not read: plain paths, absolute paths, globs, and a `.*` segment
/// that the glob matcher also matches against `..`.
#[test]
fn project_entries_outside_the_root_are_not_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("project");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::create_dir_all(&outside).expect("create outside");
    std::fs::write(
        outside.join("vitest.e2e.config.ts"),
        "export default defineConfig({ oxc: { jsx: { importSource: './jsx' } } });",
    )
    .expect("write outside config");
    let source = format!(
        r"
        export default defineConfig({{
            test: {{
                projects: [
                    '../outside/vitest.e2e.config.ts',
                    '{}',
                    '../outside/vitest.*.config.ts',
                    '.*/outside/vitest.e2e.config.ts',
                ],
            }},
        }});
    ",
        outside
            .join("vitest.e2e.config.ts")
            .display()
            .to_string()
            .replace('\\', "/")
    );
    let result = VitestPlugin.resolve_config(&root.join("vitest.config.ts"), &source, &root);
    assert!(
        result.setup_files.is_empty(),
        "a config outside the project root must not be read, got: {:?}",
        result.setup_files
    );
    assert!(result.jsx_import_sources.is_empty());
}

/// A workspace package config can name a config in a sibling directory.
/// The sibling is outside the workspace root but inside the project root.
#[test]
fn project_entries_with_parent_segments_inside_the_project_root_are_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    let web = root.join("apps/web");
    std::fs::create_dir_all(&web).expect("create web");
    std::fs::create_dir_all(root.join("apps/shared")).expect("create shared");
    std::fs::write(
        root.join("apps/shared/vitest.e2e.config.ts"),
        "export default defineConfig({});",
    )
    .expect("write shared config");
    let source = r"
        export default defineConfig({
            test: { projects: ['../shared/vitest.e2e.config.ts', '../shared/vitest.*.config.ts'] },
        });
    ";
    let result =
        VitestPlugin.resolve_config_in_project(&web.join("vitest.config.ts"), source, &web, root);
    assert_eq!(
        result.setup_files,
        vec![root.join("apps/shared/vitest.e2e.config.ts")]
    );
}

/// An inline project that extends a config outside the project root does
/// not read that config.
#[test]
fn inline_project_extends_outside_the_root_is_not_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path().join("project");
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::create_dir_all(temp.path().join("outside")).expect("create outside");
    std::fs::write(
        temp.path().join("outside/vite.config.ts"),
        "export default defineConfig({ oxc: { jsx: { importSource: 'preact' } } });",
    )
    .expect("write outside config");
    let source = r"
        export default defineConfig({
            test: { projects: [{ extends: '../outside/vite.config.ts', test: { name: 'a' } }] },
        });
    ";
    let result = VitestPlugin.resolve_config(&root.join("vitest.config.ts"), source, &root);
    assert!(
        !result
            .referenced_dependencies
            .contains(&"preact".to_string()),
        "a config outside the project root must not be read, got: {:?}",
        result.referenced_dependencies
    );
    assert!(
        result.setup_files.is_empty(),
        "got: {:?}",
        result.setup_files
    );
}

/// An inline project that extends a config inside the project root still
/// reads it.
#[test]
fn inline_project_extends_inside_the_root_is_read() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::write(
        root.join("vite.base.config.ts"),
        "export default defineConfig({ oxc: { jsx: { importSource: 'preact' } } });",
    )
    .expect("write base config");
    let source = r"
        export default defineConfig({
            test: { projects: [{ extends: './vite.base.config.ts', test: { name: 'a' } }] },
        });
    ";
    let result = VitestPlugin.resolve_config(&root.join("vitest.config.ts"), source, root);
    assert!(
        result
            .referenced_dependencies
            .contains(&"preact".to_string()),
        "got: {:?}",
        result.referenced_dependencies
    );
    assert_eq!(result.setup_files, vec![root.join("vite.base.config.ts")]);
}

fn entry_globs(result: &PluginResult) -> Vec<&str> {
    let mut patterns: Vec<&str> = result
        .entry_patterns
        .iter()
        .map(|rule| rule.pattern.as_str())
        .collect();
    patterns.sort_unstable();
    patterns.dedup();
    patterns
}

#[test]
fn test_root_and_dir_prefix_the_includes() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        r#"export default {
            test: { root: "app", dir: "tests", include: ["**/*.check.ts"] },
        };"#,
    );
    assert!(result.replace_entry_patterns);
    assert_eq!(
        entry_globs(&result),
        vec![
            "app/tests/**/*.check.ts",
            "app/tests/**/*.{bench,benchmark}.{js,jsx,ts,tsx,mjs,cjs,mts,cts}"
        ]
    );
}

#[test]
fn vite_root_is_the_vitest_root_and_the_base_of_setup_files() {
    let result = resolve_at(
        "/project/vite.config.ts",
        r#"export default { root: "web", test: { setupFiles: ["./setup.ts"] } };"#,
    );
    assert!(
        result
            .setup_files
            .contains(&PathBuf::from("/project/web/setup.ts")),
        "{:?}",
        result.setup_files
    );
}

#[test]
fn include_dir_outside_the_root_adds_no_entries() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        r#"export default { test: { root: "..", include: ["**/*.test.ts"] } };"#,
    );
    assert!(!result.replace_entry_patterns);
    assert!(result.entry_patterns.is_empty());
}

#[test]
fn inline_project_root_and_default_include_scope_its_entries() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        r#"export default {
            test: {
                projects: [
                    { extends: false, test: { root: "pkg", include: ["spec/**/*.ts"] } },
                    { extends: false, test: { root: "lib" } },
                ],
            },
        };"#,
    );
    assert_eq!(
        entry_globs(&result),
        vec![
            "lib/**/*.{test,spec}.{js,jsx,ts,tsx,mjs,cjs,mts,cts}",
            "pkg/spec/**/*.ts"
        ]
    );
    let pkg = result
        .entry_patterns
        .iter()
        .find(|rule| rule.pattern == "pkg/spec/**/*.ts")
        .unwrap();
    assert!(
        pkg.exclude_globs
            .contains(&"pkg/**/node_modules/**".to_string()),
        "{:?}",
        pkg.exclude_globs
    );
}

#[test]
fn inherited_root_applies_once_to_an_inline_project() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        r#"export default {
            root: "packages/a",
            test: { projects: [{ test: { name: "u", include: ["spec/**/*.ck.ts"] } }] },
        };"#,
    );
    assert_eq!(entry_globs(&result), vec!["packages/a/spec/**/*.ck.ts"]);
}

#[test]
fn projects_that_are_not_an_array_literal_add_no_inline_entries() {
    let result = resolve_at(
        "/project/vitest.config.ts",
        "export default { test: { projects: PROJECTS } };",
    );
    assert!(result.entry_patterns.is_empty());
}

#[test]
fn project_object_in_a_const_credits_its_setup_files() {
    let result = resolve(
        r#"const UNIT = { test: { name: "unit", setupFiles: ["./test/setup.ts"] } };
        export default { test: { projects: [UNIT] } };"#,
    );
    assert!(
        result
            .setup_files
            .contains(&std::path::PathBuf::from("/project/test/setup.ts")),
        "the setup file of a project in a const must be credited: {:?}",
        result.setup_files
    );
}

#[test]
fn test_object_and_setup_file_in_consts_are_credited() {
    let result = resolve(
        r#"const SETUP = "./test/setup.ts";
        const INCLUDE = ["spec/**/*.check.ts"];
        const TEST = { include: INCLUDE, setupFiles: SETUP };
        export default defineConfig({ test: TEST });"#,
    );
    assert!(
        result
            .setup_files
            .contains(&std::path::PathBuf::from("/project/test/setup.ts")),
        "a setup file in a const must be credited: {:?}",
        result.setup_files
    );
    assert!(
        super::super::entry_rules_match(&result, "spec/a.check.ts"),
        "the include patterns in a const must apply"
    );
}

#[test]
fn const_that_the_file_writes_to_does_not_resolve() {
    let result = resolve(
        r#"let TEST = { setupFiles: ["./test/old.ts"] };
        TEST = { setupFiles: ["./test/new.ts"] };
        export default { test: TEST };"#,
    );
    assert!(
        result.setup_files.is_empty(),
        "a binding that the file writes to must not resolve: {:?}",
        result.setup_files
    );
}
