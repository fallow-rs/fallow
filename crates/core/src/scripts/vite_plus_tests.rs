#![expect(
    clippy::disallowed_types,
    reason = "ScriptCatalog takes the serde-deserialized std HashMap"
)]

use std::collections::HashMap;
use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::super::{
    BinaryConfigFile, IgnoredCommandEntries, ScriptAnalysis, ScriptCatalog,
    analyze_commands_with_context, referenced_package_scripts,
};
use super::{staged_commands, task_commands};

fn analyze(command: &str, scripts: &[(&str, &str)], declared: &[&str]) -> ScriptAnalysis {
    let scripts: HashMap<String, String> = scripts
        .iter()
        .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
        .collect();
    let declared: FxHashSet<String> = declared.iter().map(|name| (*name).to_string()).collect();
    analyze_commands_with_context(
        &[command.to_string()],
        Path::new("/nonexistent"),
        &FxHashMap::default(),
        &declared,
        &ScriptCatalog::from_scripts(&scripts),
        IgnoredCommandEntries::NONE,
    )
}

#[test]
fn every_vite_plus_binary_credits_vite_plus() {
    for command in [
        "vp test",
        "vp lint",
        "vp pack",
        "vp config",
        "vpr build",
        "vpx some-tool",
        "vp exec some-tool",
    ] {
        let result = analyze(command, &[], &["vite-plus"]);
        assert!(
            result.used_packages.contains("vite-plus"),
            "`{command}` did not credit vite-plus: {:?}",
            result.used_packages
        );
    }
}

#[test]
fn lint_and_format_targets_are_not_entries() {
    for command in [
        "vp lint scripts/a.ts",
        "vp lint --fix scripts/a.ts",
        "vp fmt scripts/a.ts",
        "vp format scripts/a.ts",
        "vp check --fix scripts/a.ts",
    ] {
        let result = analyze(command, &[], &["vite-plus"]);
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn subcommands_without_file_arguments_make_no_entry() {
    for command in [
        "vp config scripts/a.ts",
        "vp run scripts/a.ts",
        "vp install a.ts",
    ] {
        let result = analyze(command, &[], &["vite-plus"]);
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn test_config_argument_goes_to_vitest() {
    let result = analyze("vp test -c vitest.e2e.config.ts", &[], &["vite-plus"]);
    assert_eq!(
        result.binary_config_files,
        [BinaryConfigFile {
            binary: "vitest".to_string(),
            path: "vitest.e2e.config.ts".to_string(),
        }]
    );
}

#[test]
fn pack_file_arguments_are_entries() {
    let result = analyze("vp pack src/cli.ts", &[], &["vite-plus"]);
    assert_eq!(result.entry_files, ["src/cli.ts"]);
}

#[test]
fn exec_and_vpx_run_the_child_command() {
    for command in [
        "vp exec tsx scripts/gen.ts",
        "vp exec -- tsx scripts/gen.ts",
        "vp dlx tsx scripts/gen.ts",
        "vpx tsx scripts/gen.ts",
    ] {
        let result = analyze(command, &[], &["vite-plus", "tsx"]);
        assert!(
            result.used_packages.contains("tsx"),
            "`{command}` did not credit tsx: {:?}",
            result.used_packages
        );
        assert!(
            result
                .entry_files
                .iter()
                .any(|file| file == "scripts/gen.ts"),
            "`{command}` did not make the entry: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn exec_in_other_packages_makes_no_entry_here() {
    for command in [
        "vp exec -r tsx scripts/gen.ts",
        "vp exec --filter web tsx scripts/gen.ts",
        "vp exec --filter=web tsx scripts/gen.ts",
    ] {
        let result = analyze(command, &[], &["vite-plus", "tsx"]);
        assert!(
            result.entry_files.is_empty(),
            "`{command}` produced entries: {:?}",
            result.entry_files
        );
    }
}

#[test]
fn run_and_vpr_call_a_declared_script() {
    let scripts: HashMap<String, String> =
        HashMap::from([("gen".to_string(), "tsx scripts/gen.ts".to_string())]);
    let catalog = ScriptCatalog::from_scripts(&scripts);
    for command in [
        "vp run gen",
        "vp run --cache gen",
        "vp run --concurrency-limit 2 gen",
        "vpr gen",
        "vpr --verbose gen",
    ] {
        let names = referenced_package_scripts(command, &catalog);
        assert!(
            names.contains("gen"),
            "`{command}` did not call the script: {names:?}"
        );
    }
    assert!(referenced_package_scripts("vp run other", &catalog).is_empty());
}

fn write_config(source: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("tempdir");
    std::fs::write(temp.path().join("vite.config.ts"), source).expect("write config");
    temp
}

#[test]
fn staged_block_gives_string_and_array_commands() {
    let temp = write_config(
        r"
        import { defineConfig } from 'vite-plus';
        export default defineConfig({
            staged: {
                '*.ts': 'vp check --fix',
                '*.md': ['prettier --write', 'markdownlint'],
                '*.rs': (files) => `cargo fmt -- ${files.join(' ')}`,
            },
        });
        ",
    );
    assert_eq!(
        staged_commands(temp.path()),
        ["vp check --fix", "prettier --write", "markdownlint"]
    );
}

#[test]
fn run_tasks_give_commands_and_skip_a_task_with_cwd() {
    let temp = write_config(
        r"
        export default {
            run: {
                tasks: {
                    build: { command: 'tsc -p tsconfig.build.json', cache: true },
                    both: { command: ['node a.js', 'node b.js'] },
                    short: 'node scripts/short.ts',
                    elsewhere: { command: 'node a.js', cwd: 'subfolder' },
                },
            },
        };
        ",
    );
    assert_eq!(
        task_commands(temp.path()),
        [
            (
                "build".to_string(),
                "tsc -p tsconfig.build.json".to_string()
            ),
            ("both".to_string(), "node a.js && node b.js".to_string()),
            ("short".to_string(), "node scripts/short.ts".to_string()),
        ]
    );
}

#[test]
fn no_config_gives_no_commands() {
    let temp = tempfile::tempdir().expect("tempdir");
    assert!(staged_commands(temp.path()).is_empty());
    assert!(task_commands(temp.path()).is_empty());
}
