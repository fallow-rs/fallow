//! Test reach of the changed exports in `fallow audit --brief`.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::common::{commit_all, git, parse_json, run_fallow_in_root, run_fallow_raw};

const VITEST_PACKAGE: &str = r#"{"name":"reach","private":true,"type":"module","main":"src/main.ts","devDependencies":{"vitest":"^3.0.0","typescript":"^5.6.0"}}"#;

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (path, content) in files {
        let path = root.join(path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directory");
        }
        fs::write(path, content).expect("write fixture file");
    }
}

/// Commit `base`, then apply `head` as a second commit.
fn two_commit_project(root: &Path, base: &[(&str, &str)], head: &[(&str, &str)]) {
    write_files(root, base);
    git(root, &["init", "-b", "main"]);
    commit_all(root, "base");
    write_files(root, head);
    commit_all(root, "head");
}

fn brief_output(root: &Path, format: &str) -> crate::common::CommandOutput {
    let mut args = vec![
        "audit",
        "--root",
        root.to_str().expect("utf-8 root"),
        "--base",
        "main~1",
        "--brief",
        "--no-cache",
    ];
    if format == "json" {
        args.extend(["--format", "json", "--quiet"]);
    }
    let output = run_fallow_raw(&args);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    output
}

fn test_reach(root: &Path) -> Value {
    let brief = parse_json(&brief_output(root, "json"));
    brief["test_reach"].clone()
}

fn row<'a>(reach: &'a Value, file: &str, name: &str) -> &'a Value {
    reach["exports"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["file"] == file && row["export_name"] == name)
        })
        .unwrap_or_else(|| panic!("no row for {file}::{name}: {reach}"))
}

fn rows(reach: &Value) -> Vec<(String, String, String)> {
    reach["exports"]
        .as_array()
        .expect("exports array")
        .iter()
        .map(|row| {
            (
                row["reach"].as_str().unwrap_or_default().to_string(),
                row["file"].as_str().unwrap_or_default().to_string(),
                row["export_name"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// One changed export per reach class, the shape of the design probe.
fn t1_base() -> Vec<(&'static str, &'static str)> {
    vec![
        ("package.json", VITEST_PACKAGE),
        (
            "src/main.ts",
            "import { directExport, noneExport } from './lib';\nimport { runService } from './service';\nimport { consume } from './consumer';\nimport { barrelExport } from './feature';\nimport { noneFileExport } from './ui';\nconsole.log(directExport(1), noneExport(2), runService(3), consume('a'), barrelExport(4), noneFileExport());\n",
        ),
        (
            "src/lib.ts",
            "export function directExport(x: number): number {\n  return x + 1;\n}\nexport function transitiveExport(x: number): number {\n  return x * 2;\n}\nexport function noneExport(x: number): number {\n  return x - 1;\n}\nexport type Shape = { kind: string };\n",
        ),
        (
            "src/lib.test.ts",
            "import { expect, it } from 'vitest';\nimport { directExport } from './lib';\nit('direct', () => { expect(directExport(1)).toBe(2); });\n",
        ),
        (
            "src/service.ts",
            "import { transitiveExport } from './lib';\nexport const runService = (x: number): number => transitiveExport(x) + 1;\n",
        ),
        (
            "src/service.test.ts",
            "import { expect, it } from 'vitest';\nimport { runService } from './service';\nit('service', () => { expect(runService(1)).toBe(3); });\n",
        ),
        (
            "src/queue.ts",
            "export function mockedExport(id: string): string {\n  return id;\n}\n",
        ),
        (
            "src/consumer.ts",
            "import { mockedExport } from './queue';\nexport const consume = (id: string): string => mockedExport(id);\n",
        ),
        (
            "src/consumer.test.ts",
            "import { expect, it, vi } from 'vitest';\nvi.mock('./queue', () => ({ mockedExport: vi.fn(() => 'x') }));\nimport { mockedExport } from './queue';\nimport { consume } from './consumer';\nit('consume', () => { consume('a'); expect(mockedExport).toHaveBeenCalledWith('a'); });\n",
        ),
        (
            "src/feature/deep.ts",
            "export function barrelExport(x: number): number {\n  return x + 100;\n}\n",
        ),
        (
            "src/feature/index.ts",
            "export { barrelExport } from './deep';\n",
        ),
        (
            "src/feature/feature.test.ts",
            "import { expect, it } from 'vitest';\nimport { barrelExport } from './index';\nit('barrel', () => { expect(barrelExport(1)).toBe(101); });\n",
        ),
        (
            "src/ui.ts",
            "export function noneFileExport(): string {\n  return 'ui';\n}\n",
        ),
    ]
}

fn t1_head() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "src/feature/index.ts",
            "export { barrelExport } from './deep';\nexport { barrelExport as renamedBarrel } from './deep';\n",
        ),
        (
            "src/lib.ts",
            "export function directExport(x: number): number {\n  return x + 2;\n}\nexport function transitiveExport(x: number): number {\n  return x * 3;\n}\nexport function noneExport(x: number): number {\n  return x - 2;\n}\nexport type Shape = { kind: string; size?: number };\n",
        ),
        (
            "src/queue.ts",
            "export function mockedExport(id: string): string {\n  return id + '?';\n}\n",
        ),
        (
            "src/feature/deep.ts",
            "export function barrelExport(x: number): number {\n  return x + 200;\n}\n",
        ),
        (
            "src/ui.ts",
            "export function noneFileExport(): string {\n  return 'UI';\n}\nexport const addedExport = (): string => 'new';\n",
        ),
        (
            "src/consumer.test.ts",
            "import { expect, it, vi } from 'vitest';\nvi.mock('./queue', () => ({ mockedExport: vi.fn(() => 'y') }));\nimport { mockedExport } from './queue';\nimport { consume } from './consumer';\nit('consume', () => { consume('a'); expect(mockedExport).toHaveBeenCalledWith('a'); });\n",
        ),
    ]
}

#[test]
fn brief_reports_one_reach_class_per_changed_export() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    two_commit_project(root, &t1_base(), &t1_head());

    let reach = test_reach(root);

    assert_eq!(reach["status"], "measured", "{reach}");
    assert_eq!(reach["masking"], "active", "{reach}");
    assert_eq!(
        reach["total"], 7,
        "type-only exports and re-exports do not count: {reach}"
    );
    assert_eq!(
        reach["counts"],
        serde_json::json!({
            "direct": 2,
            "transitive": 1,
            "mocked-only": 1,
            "none": 1,
            "none-file": 2,
            "framework": 0
        }),
        "{reach}"
    );
    assert_eq!(
        rows(&reach),
        [
            ("mocked-only", "src/queue.ts", "mockedExport"),
            ("none", "src/lib.ts", "noneExport"),
            ("none-file", "src/ui.ts", "noneFileExport"),
            ("none-file", "src/ui.ts", "addedExport"),
            ("transitive", "src/lib.ts", "transitiveExport"),
        ]
        .map(|(class, file, name)| (class.to_string(), file.to_string(), name.to_string())),
        "class order, then modified before new: {reach}"
    );
    assert_eq!(reach["exports_omitted"], 0);

    let mocked = row(&reach, "src/queue.ts", "mockedExport");
    assert_eq!(mocked["change"], "modified");
    assert_eq!(mocked["tests"], serde_json::json!(["src/consumer.test.ts"]));
    assert_eq!(mocked["tests_total"], 1);
    assert_eq!(
        mocked["test_changed"], true,
        "the mocking test changed: {mocked}"
    );
    assert_eq!(
        mocked["mocked_targets"],
        serde_json::json!(["src/queue.ts"])
    );

    let added = row(&reach, "src/ui.ts", "addedExport");
    assert_eq!(added["change"], "new");
    let none = row(&reach, "src/lib.ts", "noneExport");
    for absent in ["tests", "tests_total", "test_changed", "mocked_targets"] {
        assert!(none.get(absent).is_none(), "{absent} on a none row: {none}");
    }
}

#[test]
fn brief_human_section_stays_within_eighty_columns() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    let mut base = t1_base();
    base.push((
        "src/a/very/deeply/nested/directory/structure/for/width/module.ts",
        "export function anExportWithAVeryLongNameForTheWidthCheck(): number {\n  return 1;\n}\n",
    ));
    base.push((
        "src/width.ts",
        "import { anExportWithAVeryLongNameForTheWidthCheck } from './a/very/deeply/nested/directory/structure/for/width/module';\nconsole.log(anExportWithAVeryLongNameForTheWidthCheck());\n",
    ));
    let mut head = t1_head();
    head.push((
        "src/a/very/deeply/nested/directory/structure/for/width/module.ts",
        "export function anExportWithAVeryLongNameForTheWidthCheck(): number {\n  return 2;\n}\n",
    ));
    two_commit_project(root, &base, &head);

    let output = brief_output(root, "human");
    let lines: Vec<&str> = output
        .stderr
        .lines()
        .skip_while(|line| !line.starts_with("  test reach:"))
        .take_while(|line| line.starts_with("  test reach:") || line.starts_with("    "))
        .collect();

    assert!(
        !lines.is_empty(),
        "no test reach section: {}",
        output.stderr
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("mocked only") && line.contains("tests mock src/queue.ts")),
        "{lines:#?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("(--format json for the full list)")),
        "more than three rows: {lines:#?}"
    );
    for line in &lines {
        assert!(line.chars().count() <= 80, "line over 80 columns: {line:?}");
    }
}

/// The mock, helper and setup-file shapes of the design panel probe.
fn mock_shape_base() -> Vec<(&'static str, &'static str)> {
    vec![
        ("package.json", VITEST_PACKAGE),
        (
            "vitest.config.ts",
            "import { defineConfig } from 'vitest/config';\nexport default defineConfig({ test: { setupFiles: ['./vitest.setup.ts'] } });\n",
        ),
        (
            "vitest.setup.ts",
            "import { initI18n } from './src/i18n';\ninitI18n();\n",
        ),
        (
            "src/main.ts",
            "import { initI18n } from './i18n';\nimport { barrelMocked } from './feat';\nimport { twoRoots } from './two';\nimport { cjsExport } from './cjs';\nimport { partial } from './partial';\nimport { auto } from './auto';\nimport { helped } from './helped';\nimport { typed } from './typed';\nimport { fixtureUsed } from './fixture-used';\nconsole.log(initI18n, barrelMocked, twoRoots, cjsExport, partial, auto, helped, typed, fixtureUsed);\n",
        ),
        (
            "src/i18n.ts",
            "export function initI18n(): number {\n  return 0;\n}\n",
        ),
        (
            "src/feat/deep.ts",
            "export function barrelMocked(): number {\n  return 1;\n}\n",
        ),
        (
            "src/feat/index.ts",
            "export { barrelMocked } from './deep';\n",
        ),
        (
            "src/feat/a.test.ts",
            "import { it, vi } from 'vitest';\nvi.mock('./index', () => ({ barrelMocked: vi.fn() }));\nimport { barrelMocked } from './index';\nit('a', () => { barrelMocked(); });\n",
        ),
        (
            "src/two.ts",
            "export function twoRoots(): number {\n  return 2;\n}\n",
        ),
        (
            "src/two.a.test.ts",
            "import { it, vi } from 'vitest';\nvi.mock('./two', () => ({ twoRoots: vi.fn() }));\nimport { twoRoots } from './two';\nit('a', () => { twoRoots(); });\n",
        ),
        (
            "src/two.b.test.ts",
            "import { it } from 'vitest';\nimport { twoRoots } from './two';\nit('b', () => { twoRoots(); });\n",
        ),
        (
            "src/cjs.ts",
            "export function cjsExport(): number {\n  return 3;\n}\n",
        ),
        (
            "src/cjs.test.ts",
            "import { it, vi } from 'vitest';\nvi.mock('./cjs', () => ({ cjsExport: vi.fn() }));\nconst { cjsExport } = require('./cjs');\nit('c', () => { cjsExport(); });\n",
        ),
        (
            "src/partial.ts",
            "export function partial(): number {\n  return 4;\n}\n",
        ),
        (
            "src/partial.test.ts",
            "import { it, vi } from 'vitest';\nvi.mock('./partial', async () => ({ ...(await vi.importActual('./partial')) }));\nimport { partial } from './partial';\nit('p', () => { partial(); });\n",
        ),
        (
            "src/auto.ts",
            "export function auto(): number {\n  return 5;\n}\n",
        ),
        (
            "src/auto.test.ts",
            "import { it, vi } from 'vitest';\nvi.mock('./auto');\nimport { auto } from './auto';\nit('p', () => { auto(); });\n",
        ),
        (
            "src/helped.ts",
            "export function helped(): number {\n  return 6;\n}\n",
        ),
        (
            "src/test-utils.ts",
            "import { helped } from './helped';\nexport const h = (): number => helped();\n",
        ),
        (
            "src/helped.test.ts",
            "import { h } from './test-utils';\nimport { it } from 'vitest';\nit('h', () => { h(); });\n",
        ),
        (
            "src/typed.ts",
            "export function typed(): number {\n  return 7;\n}\n",
        ),
        (
            "src/typed.test.ts",
            "import type { typed } from './typed';\nimport { it } from 'vitest';\ntype T = typeof typed;\nit('t', () => {});\n",
        ),
        (
            "src/fixture-used.ts",
            "export function fixtureUsed(): number {\n  return 8;\n}\n",
        ),
        (
            "tests/fixtures/app.ts",
            "import { fixtureUsed } from '../../src/fixture-used';\nexport const app = (): number => fixtureUsed();\n",
        ),
        (
            "tests/app.test.ts",
            "import { it } from 'vitest';\nimport { app } from './fixtures/app';\nit('app', () => { app(); });\n",
        ),
    ]
}

fn mock_shape_head() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "src/i18n.ts",
            "export function initI18n(): number {\n  return 10;\n}\n",
        ),
        (
            "src/feat/deep.ts",
            "export function barrelMocked(): number {\n  return 11;\n}\n",
        ),
        (
            "src/two.ts",
            "export function twoRoots(): number {\n  return 12;\n}\n",
        ),
        (
            "src/cjs.ts",
            "export function cjsExport(): number {\n  return 13;\n}\n",
        ),
        (
            "src/partial.ts",
            "export function partial(): number {\n  return 14;\n}\n",
        ),
        (
            "src/auto.ts",
            "export function auto(): number {\n  return 15;\n}\n",
        ),
        (
            "src/helped.ts",
            "export function helped(): number {\n  return 16;\n}\n",
        ),
        (
            "src/typed.ts",
            "export function typed(): number {\n  return 17;\n}\n",
        ),
        (
            "src/fixture-used.ts",
            "export function fixtureUsed(): number {\n  return 18;\n}\n",
        ),
    ]
}

#[test]
fn brief_classifies_the_mock_and_helper_shapes() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    two_commit_project(root, &mock_shape_base(), &mock_shape_head());

    let reach = test_reach(root);
    let mocked = row(&reach, "src/feat/deep.ts", "barrelMocked");
    assert_eq!(mocked["reach"], "mocked-only", "{reach}");
    assert_eq!(
        mocked["mocked_targets"],
        serde_json::json!(["src/feat/index.ts"]),
        "the mocked module is the barrel: {mocked}"
    );
    assert_eq!(mocked["tests"], serde_json::json!(["src/feat/a.test.ts"]));
    assert_eq!(
        row(&reach, "src/helped.ts", "helped")["reach"],
        "transitive"
    );
    assert_eq!(
        row(&reach, "src/typed.ts", "typed")["reach"],
        "none",
        "a type-only test import gives no reach: {reach}"
    );
    assert_eq!(
        row(&reach, "src/i18n.ts", "initI18n")["reach"],
        "none-file",
        "a setup file is not a test root: {reach}"
    );
    // An unmasked second root, a CommonJS `require`, `importActual` and an
    // automock all keep direct reach. A file under a test directory is test
    // code even inside `fixtures/`.
    assert_eq!(reach["counts"]["direct"], 5, "{reach}");
    let listed: Vec<(String, String, String)> = rows(&reach);
    for name in ["twoRoots", "cjsExport", "partial", "auto", "fixtureUsed"] {
        assert!(
            listed.iter().all(|(_, _, listed_name)| listed_name != name),
            "{name} is direct and not listed: {reach}"
        );
    }
}

#[test]
fn brief_orders_default_sfc_and_multi_declarator_exports() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    let base: Vec<(&str, &str)> = vec![
        ("package.json", VITEST_PACKAGE),
        (
            "src/main.ts",
            "import Widget, { b, a } from './pair';\nimport Gauge, { scale } from './Gauge.vue';\nimport { kept } from './kept';\nconsole.log(Widget, a, b, Gauge, scale, kept);\n",
        ),
        (
            "src/pair.ts",
            "export const a = 1, b = 2;\nexport default function Widget(): string {\n  return 'w';\n}\n",
        ),
        (
            "src/Gauge.vue",
            "<template>\n  <div />\n</template>\n\n<script lang=\"ts\">\nexport function scale(n: number): number {\n  return n * 2;\n}\nexport default {};\n</script>\n",
        ),
        ("src/kept.ts", "export const kept = 1;\n"),
        (
            "src/kept.test.ts",
            "import { it } from 'vitest';\nimport { kept } from './kept';\nit('k', () => { kept; });\n",
        ),
    ];
    let head: Vec<(&str, &str)> = vec![
        (
            "src/pair.ts",
            "export const a = 10, b = 20;\nexport default function Widget(): string {\n  return 'W';\n}\n",
        ),
        (
            "src/Gauge.vue",
            "<template>\n  <div />\n</template>\n\n<script lang=\"ts\">\nexport function scale(n: number): number {\n  return n * 3;\n}\nexport default {};\n</script>\n",
        ),
    ];
    two_commit_project(root, &base, &head);

    let reach = test_reach(root);
    let listed: Vec<(String, String)> = rows(&reach)
        .into_iter()
        .map(|(_, file, name)| (file, name))
        .collect();

    assert_eq!(
        listed,
        [
            ("src/Gauge.vue", "scale"),
            ("src/pair.ts", "default"),
            ("src/pair.ts", "a"),
            ("src/pair.ts", "b"),
        ]
        .map(|(file, name)| (file.to_string(), name.to_string())),
        "modified before new, then file, line and name: {reach}"
    );
    assert_eq!(row(&reach, "src/Gauge.vue", "scale")["line"], 6);
    // Every line of the one-line declaration is an added line, so both
    // declarators count as new. They share the line, so the name decides.
    assert_eq!(row(&reach, "src/pair.ts", "a")["change"], "new");
    assert_eq!(row(&reach, "src/pair.ts", "default")["change"], "modified");
}

#[test]
fn brief_labels_only_literal_framework_configuration() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    let base: Vec<(&str, &str)> = vec![
        (
            "package.json",
            r#"{"name":"next-app","private":true,"dependencies":{"next":"15.0.0","react":"19.0.0","@tanstack/react-router":"1.0.0"},"devDependencies":{"vitest":"^3.0.0"}}"#,
        ),
        (
            "app/page.tsx",
            "import { buildTitle } from '../lib/title';\nexport const dynamic = 'auto';\nexport const metadata = { title: buildTitle('a') };\nexport default function Page() {\n  return null;\n}\n",
        ),
        (
            "app/api/items/route.ts",
            "export async function GET() {\n  return new Response('a');\n}\n",
        ),
        (
            "lib/title.ts",
            "export const buildTitle = (s: string): string => s;\n",
        ),
        (
            "lib/title.test.ts",
            "import { it } from 'vitest';\nimport { buildTitle } from './title';\nit('t', () => { buildTitle('x'); });\n",
        ),
        (
            "src/routes/index.tsx",
            "import { createFileRoute } from '@tanstack/react-router';\nexport const Route = createFileRoute('/')({ component: () => null });\n",
        ),
    ];
    let head: Vec<(&str, &str)> = vec![
        (
            "app/page.tsx",
            "import { buildTitle } from '../lib/title';\nexport const dynamic = 'force-dynamic';\nexport const metadata = { title: buildTitle('b') };\nexport default function Page() {\n  return 'page';\n}\n",
        ),
        (
            "app/api/items/route.ts",
            "export async function GET() {\n  return new Response('b');\n}\n",
        ),
        (
            "src/routes/index.tsx",
            "import { createFileRoute } from '@tanstack/react-router';\nexport const Route = createFileRoute('/')({ component: () => 'x' });\n",
        ),
    ];
    two_commit_project(root, &base, &head);

    let reach = test_reach(root);

    assert_eq!(reach["counts"]["framework"], 1, "{reach}");
    let listed: Vec<String> = rows(&reach).into_iter().map(|(_, _, name)| name).collect();
    assert!(!listed.contains(&"dynamic".to_string()), "{reach}");
    for name in ["metadata", "default", "GET", "Route"] {
        assert!(
            listed.contains(&name.to_string()),
            "{name} keeps a reach class: {reach}"
        );
    }
}

#[test]
fn brief_reports_no_tests_for_a_project_without_test_files() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    two_commit_project(
        root,
        &[
            ("package.json", r#"{"name":"plain","main":"src/main.ts"}"#),
            (
                "src/main.ts",
                "import { a } from './a';\nconsole.log(a());\n",
            ),
            (
                "src/a.ts",
                "export function a(): number {\n  return 1;\n}\n",
            ),
        ],
        &[(
            "src/a.ts",
            "export function a(): number {\n  return 2;\n}\n",
        )],
    );

    let reach = test_reach(root);

    assert_eq!(reach["status"], "no-tests", "{reach}");
    assert_eq!(reach["masking"], "no-mocks", "{reach}");
    assert_eq!(reach["total"], 1);
    assert_eq!(reach["counts"]["none-file"], 1);
    assert_eq!(reach["exports"], serde_json::json!([]));

    let human = brief_output(root, "human");
    assert!(
        human
            .stderr
            .contains("  test reach: no test files in this project; 1 changed export has no test"),
        "{}",
        human.stderr
    );
}

#[test]
fn brief_measures_a_project_with_custom_test_globs() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    two_commit_project(
        root,
        &[
            ("package.json", VITEST_PACKAGE),
            (
                "vitest.config.ts",
                "import { defineConfig } from 'vitest/config';\nexport default defineConfig({ test: { include: ['checks/**/*.check.ts'] } });\n",
            ),
            (
                "src/main.ts",
                "import { a } from './a';\nconsole.log(a());\n",
            ),
            (
                "src/a.ts",
                "export function a(): number {\n  return 1;\n}\n",
            ),
            (
                "checks/a.check.ts",
                "import { it } from 'vitest';\nimport { a } from '../src/a';\nit('a', () => { a(); });\n",
            ),
        ],
        &[(
            "src/a.ts",
            "export function a(): number {\n  return 2;\n}\n",
        )],
    );

    let reach = test_reach(root);

    assert_eq!(reach["status"], "measured", "{reach}");
    assert_eq!(reach["masking"], "no-mocks", "{reach}");
    assert_eq!(reach["counts"]["direct"], 1, "{reach}");
}

#[test]
fn brief_caps_the_attention_list_and_counts_the_rest() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    let declarations = |value: u32| -> String {
        (0..30)
            .map(|index| format!("export const value{index:02} = (): number => {value};\n"))
            .collect::<Vec<_>>()
            .concat()
    };
    let main = format!(
        "import {{ {} }} from './many';\nconsole.log({});\n",
        (0..30)
            .map(|index| format!("value{index:02}"))
            .collect::<Vec<_>>()
            .join(", "),
        (0..30)
            .map(|index| format!("value{index:02}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    let base_many = declarations(1);
    let head_many = declarations(2);
    two_commit_project(
        root,
        &[
            ("package.json", VITEST_PACKAGE),
            ("src/main.ts", &main),
            ("src/many.ts", &base_many),
            (
                "src/other.test.ts",
                "import { it } from 'vitest';\nit('o', () => {});\n",
            ),
        ],
        &[("src/many.ts", &head_many)],
    );

    let reach = test_reach(root);

    assert_eq!(reach["total"], 30, "{reach}");
    assert_eq!(reach["exports"].as_array().map(Vec::len), Some(25));
    assert_eq!(reach["exports_omitted"], 5);
}

#[test]
fn brief_reports_capped_masking_past_the_profile_cap() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    const ROOTS: usize = 1025;
    let mut files: Vec<(String, String)> = vec![
        ("package.json".to_string(), VITEST_PACKAGE.to_string()),
        (
            "src/main.ts".to_string(),
            "import { target } from './target';\nconsole.log(target());\n".to_string(),
        ),
        (
            "src/target.ts".to_string(),
            "export function target(): number {\n  return 1;\n}\n".to_string(),
        ),
        (
            "src/target.test.ts".to_string(),
            "import { it, vi } from 'vitest';\nvi.mock('./target', () => ({ target: vi.fn() }));\nimport { target } from './target';\nit('t', () => { target(); });\n".to_string(),
        ),
    ];
    for index in 0..ROOTS {
        files.push((
            format!("src/m/m{index}.ts"),
            format!("export const m{index} = {index};\n"),
        ));
        files.push((
            format!("src/m/m{index}.test.ts"),
            format!(
                "import {{ it, vi }} from 'vitest';\nvi.mock('./m{index}', () => ({{ m{index}: 0 }}));\nimport {{ m{index} }} from './m{index}';\nit('m', () => {{ m{index}; }});\n"
            ),
        ));
    }
    let base: Vec<(&str, &str)> = files
        .iter()
        .map(|(path, content)| (path.as_str(), content.as_str()))
        .collect();
    two_commit_project(
        root,
        &base,
        &[(
            "src/target.ts",
            "export function target(): number {\n  return 2;\n}\n",
        )],
    );

    let reach = test_reach(root);

    assert_eq!(reach["masking"], "capped", "{reach}");
    assert_eq!(
        reach["counts"]["mocked-only"], 0,
        "mock detection is off past the cap: {reach}"
    );
    assert_eq!(reach["counts"]["direct"], 1, "{reach}");
    let human = brief_output(root, "human");
    assert!(
        human
            .stderr
            .contains("mock detection is off: too many distinct mock sets"),
        "{}",
        human.stderr
    );
}

#[test]
fn coverage_gap_note_names_a_mocked_only_export() {
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path();
    write_files(root, &t1_base());

    let output = parse_json(&run_fallow_in_root(
        "health",
        root,
        &[
            "--coverage-gaps",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
        ],
    ));
    let exports = output["coverage_gaps"]["exports"]
        .as_array()
        .unwrap_or_else(|| panic!("coverage gap exports: {output}"));
    let note = |name: &str| {
        exports
            .iter()
            .find(|export| export["export_name"] == name)
            .and_then(|export| export["actions"][0]["note"].as_str())
            .unwrap_or_else(|| panic!("no gap for {name}: {output}"))
            .to_string()
    };

    assert_eq!(
        note("mockedExport"),
        "This export is runtime-reachable, but only tests that mock this module reference it"
    );
    assert_eq!(
        note("noneFileExport"),
        "This export is runtime-reachable but no test-reachable module references it"
    );
}
