#![expect(deprecated, reason = "the rule tests run the full in-crate pipeline")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fallow_config::{FallowConfig, OutputFormat};
use rustc_hash::FxHashMap;

use super::{Abstain, FnId, GateStats, ReasonCount};
use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::results::{AnalysisResults, ConstantParameterReason};

/// The abstain reason of each function in each analyzed project, keyed by
/// the module path.
static ABSTAINED: Mutex<Vec<(PathBuf, String, &'static str)>> = Mutex::new(Vec::new());

/// The gate counts of each analyzed project, with the module paths of the
/// project.
static STATS: Mutex<Vec<(Vec<PathBuf>, GateStats)>> = Mutex::new(Vec::new());

pub(super) fn record(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    abstained: &FxHashMap<FnId, Abstain>,
    stats: Option<&GateStats>,
) {
    if let Some(stats) = stats {
        let paths = graph.modules.iter().map(|node| node.path.clone()).collect();
        STATS
            .lock()
            .expect("recorder lock")
            .push((paths, stats.clone()));
    }
    let by_file: FxHashMap<_, _> = modules.iter().map(|m| (m.file_id, m)).collect();
    let mut rows = ABSTAINED.lock().expect("recorder lock");
    for (&(file, index), reason) in abstained {
        let Some(node) = graph.modules.get(file.0 as usize) else {
            continue;
        };
        let Some(name) = by_file
            .get(&file)
            .and_then(|module| module.function_contracts.as_deref())
            .and_then(|facts| facts.declarations.get(index as usize))
            .map(|contract| contract.binding_name.to_string())
        else {
            continue;
        };
        rows.push((node.path.clone(), name, reason.code()));
    }
}

fn reason_for(root: &Path, file: &str, function: &str) -> Option<&'static str> {
    let path = root.join(file);
    ABSTAINED
        .lock()
        .expect("recorder lock")
        .iter()
        .find(|(module, name, _)| {
            name == function
                && (module == &path || dunce::canonicalize(module).ok().as_ref() == Some(&path))
        })
        .map(|(_, _, reason)| *reason)
}

struct Project {
    _directory: tempfile::TempDir,
    root: PathBuf,
    results: AnalysisResults,
}

type Row = (String, ConstantParameterReason, Option<String>);

impl Project {
    fn findings(&self, function: &str) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .results
            .constant_parameters
            .iter()
            .filter(|finding| finding.parameter.function_name == function)
            .map(|finding| {
                (
                    finding.parameter.parameter_name.clone(),
                    finding.parameter.reason,
                    finding
                        .parameter
                        .value
                        .as_ref()
                        .map(|value| value.text.clone()),
                )
            })
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows
    }

    fn reason(&self, file: &str, function: &str) -> Option<&'static str> {
        reason_for(&self.root, file, function)
    }

    fn stats(&self) -> GateStats {
        STATS
            .lock()
            .expect("recorder lock")
            .iter()
            .find(|(paths, _)| paths.iter().any(|path| path.starts_with(&self.root)))
            .map(|(_, stats)| stats.clone())
            .expect("gate stats of the project")
    }
}

fn write_project(
    package: &serde_json::Value,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("temporary project");
    let root = dunce::canonicalize(directory.path()).expect("canonical root");
    std::fs::write(root.join("package.json"), package.to_string()).expect("package.json");
    for (path, source) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
        std::fs::write(path, source).expect("source");
    }
    (directory, root)
}

fn analyze_with(
    package: &serde_json::Value,
    config: &serde_json::Value,
    files: &[(&str, &str)],
) -> Project {
    let (directory, root) = write_project(package, files);
    let mut config = config.clone();
    config["rules"] = serde_json::json!({ "constant-parameters": "warn" });
    let config: FallowConfig = serde_json::from_value(config).expect("valid config");
    let config = config.resolve(root.clone(), OutputFormat::Human, 2, true, true, None);
    let results = crate::analyze(&config).expect("analysis succeeds");
    Project {
        _directory: directory,
        root,
        results,
    }
}

fn private_package() -> serde_json::Value {
    serde_json::json!({ "name": "fixture", "private": true })
}

fn entry_config() -> serde_json::Value {
    serde_json::json!({ "entry": ["src/main.ts"] })
}

fn analyze(files: &[(&str, &str)]) -> Project {
    analyze_with(&private_package(), &entry_config(), files)
}

const FORMAT: &str = "export function format(value: number, compact: boolean, locale?: string) {\n  if (compact) return String(value);\n  return value.toLocaleString(locale);\n}\n";
const MAIN: &str =
    "import { format } from './format';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";

fn base_with<'a>(extra: &[(&'a str, &'a str)], main: &'a str) -> Vec<(&'a str, &'a str)> {
    let mut files = vec![("src/format.ts", FORMAT), ("src/main.ts", main)];
    files.extend_from_slice(extra);
    files
}

fn never_passed_locale() -> Vec<Row> {
    vec![(
        "locale".to_string(),
        ConstantParameterReason::NeverPassed,
        None,
    )]
}

fn assert_abstains(project: &Project, file: &str, function: &str, reason: &str) {
    assert!(
        project.findings(function).is_empty(),
        "{reason}: unexpected findings {:?}",
        project.findings(function)
    );
    assert_eq!(project.reason(file, function), Some(reason));
}

/// The control project reports both parameters, so each variant below
/// proves that its own condition stops the finding.
#[test]
fn reports_an_identical_argument_and_a_parameter_that_no_call_passes() {
    let project = analyze(&base_with(&[], MAIN));
    assert_eq!(
        project.findings("format"),
        vec![
            (
                "compact".to_string(),
                ConstantParameterReason::IdenticalArgument,
                Some("true".to_string())
            ),
            (
                "locale".to_string(),
                ConstantParameterReason::NeverPassed,
                None
            ),
        ]
    );
    let compact = project
        .results
        .constant_parameters
        .iter()
        .find(|finding| finding.parameter.parameter_name == "compact")
        .expect("compact finding");
    assert_eq!(compact.parameter.condition_reads, 1);
    assert_eq!(compact.parameter.call_site_count, 3);
    assert_eq!(compact.parameter.parameter_index, 1);
    assert_eq!(compact.parameter.line, 1);
    assert_eq!(compact.parameter.col, 38);
    assert_eq!(compact.parameter.inspected_call_sites.len(), 3);
    assert_eq!(compact.parameter.inspected_call_sites[0].line, 2);
}

#[test]
fn a_parse_error_blocks_each_function_that_the_broken_module_can_import() {
    let broken = "import { format } from './format';\nformat(4, false);\nexport const = ;\n";
    let main = "import { format } from './format';\nimport './broken';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    let project = analyze(&base_with(&[("src/broken.ts", broken)], main));
    assert_abstains(&project, "src/format.ts", "format", "parse-incomplete");
}

#[test]
fn a_parse_error_in_a_module_that_imports_no_project_module_keeps_the_findings() {
    let package = serde_json::json!({
        "name": "fixture",
        "private": true,
        "dependencies": { "react": "^18.0.0" }
    });
    let broken = "import React from 'react';\nimport { join } from 'node:path';\nconst url = new URL('https://example.com');\nexport const = ;\n";
    let declaration = "declare function broken(: void;\n";
    let project = analyze_with(
        &package,
        &entry_config(),
        &base_with(
            &[("src/broken.ts", broken), ("src/types.d.ts", declaration)],
            MAIN,
        ),
    );
    assert_eq!(project.findings("format").len(), 2);
}

fn workspace_files(broken: &str) -> Vec<(&str, &str)> {
    vec![
        (
            "packages/ui/package.json",
            r#"{ "name": "@acme/ui", "private": true }"#,
        ),
        ("packages/ui/src/format.ts", FORMAT),
        (
            "packages/app/package.json",
            r#"{ "name": "app", "private": true, "dependencies": { "@acme/ui": "workspace:*" } }"#,
        ),
        (
            "packages/app/src/main.ts",
            "import { format } from '@acme/ui/src/format';\nimport './broken';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n",
        ),
        ("packages/app/src/broken.ts", broken),
    ]
}

#[test]
fn a_parse_error_blocks_the_workspace_package_that_it_imports() {
    let package = serde_json::json!({
        "name": "root",
        "private": true,
        "workspaces": ["packages/*"]
    });
    let config = serde_json::json!({ "entry": ["packages/app/src/main.ts"] });
    let control = analyze_with(
        &package,
        &config,
        &workspace_files("export const value = 1;\n"),
    );
    assert_eq!(control.findings("format").len(), 2);

    let project = analyze_with(
        &package,
        &config,
        &workspace_files("import { format } from '@acme/ui/src/format';\nexport const = ;\n"),
    );
    assert_abstains(
        &project,
        "packages/ui/src/format.ts",
        "format",
        "parse-incomplete",
    );
}

#[test]
fn an_inline_script_with_a_root_relative_import_abstains() {
    let html = "<!doctype html>\n<script type=\"module\">\n  import { format } from '/src/format.ts';\n  format(4, false);\n</script>\n";
    let config = serde_json::json!({ "entry": ["src/main.ts", "index.html"] });
    let project = analyze_with(
        &private_package(),
        &config,
        &base_with(&[("index.html", html)], MAIN),
    );
    assert_abstains(&project, "src/format.ts", "format", "template-callers");
}

#[test]
fn a_dependency_name_that_a_project_path_reuses_blocks_that_path() {
    let package = serde_json::json!({
        "name": "fixture",
        "private": true,
        "dependencies": { "lib": "^1.0.0" }
    });
    let main = "import { format } from './lib/format';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    let broken = "import { format } from 'lib/format';\nexport const = ;\n";
    let project = analyze_with(
        &package,
        &entry_config(),
        &[
            ("src/lib/format.ts", FORMAT),
            ("src/main.ts", main),
            ("src/broken.ts", broken),
        ],
    );
    assert_abstains(&project, "src/lib/format.ts", "format", "parse-incomplete");
}

#[test]
fn a_parse_error_with_a_computed_or_unknown_import_blocks_each_export() {
    for broken in [
        "const m = await import('./form' + 'at');\nexport const = ;\n",
        "const m = require(name);\nexport const = ;\n",
        "const m = import(`./${name}`);\nexport const = ;\n",
        "import { format } from '@/format';\nexport const = ;\n",
    ] {
        let project = analyze(&base_with(&[("src/broken.ts", broken)], MAIN));
        assert_abstains(&project, "src/format.ts", "format", "parse-incomplete");
    }
}

#[test]
fn an_open_contract_abstains() {
    let format = "export function format(value: number, compact: boolean) {\n  if (compact) return arguments.length;\n  return value;\n}\n";
    let project = analyze(&[("src/format.ts", format), ("src/main.ts", MAIN)]);
    assert_abstains(&project, "src/format.ts", "format", "open-contract");
}

#[test]
fn a_public_api_export_abstains() {
    let package = serde_json::json!({
        "name": "fixture",
        "version": "1.0.0",
        "exports": { "./format": "./src/format.ts" }
    });
    let project = analyze_with(&package, &entry_config(), &base_with(&[], MAIN));
    assert_abstains(&project, "src/format.ts", "format", "public-api");
}

#[test]
fn a_public_visibility_tag_abstains() {
    let format = "/** @public */\nexport function format(value: number, compact: boolean) {\n  return compact ? value : 0;\n}\n";
    let project = analyze(&[("src/format.ts", format), ("src/main.ts", MAIN)]);
    assert_abstains(&project, "src/format.ts", "format", "public-api");
}

#[test]
fn an_entry_point_export_abstains() {
    let config = serde_json::json!({ "entry": ["src/main.ts", "src/format.ts"] });
    let project = analyze_with(&private_package(), &config, &base_with(&[], MAIN));
    assert_abstains(&project, "src/format.ts", "format", "entry-export");
}

#[test]
fn a_dynamically_loaded_module_abstains() {
    let config = serde_json::json!({
        "entry": ["src/main.ts"],
        "dynamicallyLoaded": ["src/format.ts"]
    });
    let project = analyze_with(&private_package(), &config, &base_with(&[], MAIN));
    assert_abstains(&project, "src/format.ts", "format", "entry-export");
}

#[test]
fn a_dynamic_import_abstains() {
    let main = "import { format } from './format';\nformat(1, true);\nformat(2, true);\nformat(3, true);\nexport const later = () => import('./format');\n";
    let project = analyze(&base_with(&[], main));
    assert_abstains(&project, "src/format.ts", "format", "dynamic-import");
}

const OPAQUE: &str = "const name = process.argv[2];\n";
const OPAQUE_REASON: &str = "opaque-module-load";

fn main_with(load: &str) -> String {
    format!("{MAIN}{OPAQUE}{load}")
}

fn analyze_load(extra: &[(&str, &str)], load: &str) -> Project {
    let main = main_with(load);
    analyze(&base_with(extra, &main))
}

#[test]
fn a_load_with_an_unknown_source_that_reads_the_export_name_abstains() {
    for load in [
        "const mod = await import(name);\nmod.format(4, false);\n",
        "const mod = await import(name);\nmod?.format?.(4, false);\n",
        "const mod = await import(name);\nmod['format'](4, false);\n",
        "const { format: f, default: h } = await import(name);\nf(4, false);\n",
        "const mod = await import(name);\nif ('format' in mod) console.log(1);\n",
        "import(name).then((m) => m.format(4, false));\n",
        "import(name).then(({ format }) => format(4, false));\n",
        "require(name).format(4, false);\n",
        "const mod = require(name);\nmod.format(4, false);\n",
        "(await import(name)).format(4, false);\n",
    ] {
        let project = analyze_load(&[], load);
        assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
    }
}

#[test]
fn a_load_with_an_unknown_source_that_reads_another_name_keeps_the_finding() {
    for load in [
        "const mod = await import(name);\nmod.other(4, false);\n",
        "const { other } = await import(name);\nother();\n",
        "import(name).then((m) => m.other());\n",
        "require(name).other();\n",
    ] {
        let project = analyze_load(&[], load);
        assert_eq!(project.findings("format").len(), 2, "{load}");
        assert_eq!(project.reason("src/format.ts", "format"), None, "{load}");
    }
}

#[test]
fn a_discarded_load_with_an_unknown_source_blocks_nothing() {
    for load in [
        "await import(name);\n",
        "require(name);\n",
        "void import(name);\n",
        "import(name).catch(() => {});\n",
        "import(name).then(() => console.log('loaded'));\n",
    ] {
        let project = analyze_load(&[], load);
        assert_eq!(project.findings("format").len(), 2, "{load}");
    }
}

#[test]
fn a_load_with_an_unknown_source_whose_value_escapes_blocks_each_export() {
    for load in [
        "export const later = () => import(name);\n",
        "export async function get() { return await import(name); }\n",
        "const mod = await import(name);\nconst alias = mod;\nalias.other();\n",
        "let mod;\nmod = await import(name);\nmod.other();\n",
        "console.log(await import(name));\n",
        "await Promise.all([import(name)]);\n",
        "export const box = { mod: await import(name) };\n",
        "export const list = [await import(name)];\n",
        "const mod = await import(name);\nexport { mod };\n",
        "export const copy = { ...(await import(name)) };\n",
        "Object.assign({}, await import(name));\n",
        "const mod = await import(name);\nfor (const key in mod) console.log(key);\n",
        "const mod = await import(name);\nObject.values(mod);\n",
        "const mod = await import(name);\nmod[process.argv[3]](4, false);\n",
        "require(name)();\n",
        "new (require(name))();\n",
        "const { other, ...rest } = await import(name);\n",
        "import(name).then(console.log);\n",
        "export const later = () => import(`${name}`);\n",
        "export const later = () => require(name);\n",
        "const r = require;\nr(name);\n",
    ] {
        let project = analyze_load(&[], load);
        assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
    }
}

#[test]
fn a_default_read_blocks_only_the_default_export() {
    let pad = "export default function pad(s: string, n: number) { return s + n; }\n";
    let load = "import pad from './pad';\npad('a', 1);\npad('b', 1);\n(await import(name)).default('c', 2);\n";
    let project = analyze_load(&[("src/pad.ts", pad)], load);
    assert_eq!(project.findings("format").len(), 2);
    assert_abstains(&project, "src/pad.ts", "pad", OPAQUE_REASON);
}

#[test]
fn a_read_through_a_re_export_blocks_the_origin_function() {
    for (barrel, load) in [
        (
            "export { format as leftPad } from './format';\n",
            "import './barrel';\nconst mod = await import(name);\nmod.leftPad(4, false);\n",
        ),
        (
            "export * as ns from './format';\n",
            "import './barrel';\nconst mod = await import(name);\nmod.ns.format(4, false);\n",
        ),
    ] {
        let project = analyze_load(&[("src/barrel.ts", barrel)], load);
        assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
    }
}

const LOADER: &str = "export const load = async (path: string) => await import(path);\n";
const MIGRATE: &str =
    "export function migrate(step: number, dry: boolean) { return dry ? step : step + 1; }\n";

/// A project whose `src/run.ts` holds `run`, and whose `src/load.ts` holds
/// `loader`. `format` and `migrate` each have two calls with a constant
/// argument.
fn analyze_loader(loader: &str, run: &str) -> Project {
    let main = format!(
        "import {{ migrate }} from './migrate';\nimport './run';\n{MAIN}migrate(1, true);\nmigrate(2, true);\n"
    );
    analyze(&base_with(
        &[
            ("src/load.ts", loader),
            ("src/migrate.ts", MIGRATE),
            ("src/run.ts", run),
        ],
        &main,
    ))
}

fn assert_blocks_only_migrate(project: &Project, case: &str) {
    assert_eq!(project.findings("format").len(), 2, "{case}");
    assert_eq!(project.reason("src/format.ts", "format"), None, "{case}");
    assert!(project.findings("migrate").is_empty(), "{case}");
    assert_eq!(
        project.reason("src/migrate.ts", "migrate"),
        Some(OPAQUE_REASON),
        "{case}"
    );
}

#[test]
fn a_loader_helper_blocks_only_the_names_that_its_callers_read() {
    let run = "import { load } from './load';\nconst name = process.argv[2];\nconst mod = await load(name);\nmod.migrate();\n";
    let project = analyze_loader(LOADER, run);
    assert_blocks_only_migrate(&project, run);
}

#[test]
fn each_read_of_a_loader_result_blocks_the_name() {
    let head = "import { load } from './load';\nconst name = process.argv[2];\n";
    for read in [
        "(await load(name)).migrate();\n",
        "const { migrate } = await load(name);\n",
        "load(name).then((m) => m.migrate());\n",
        "load(name).catch(() => null).then((m) => m.migrate());\n",
        "if ('migrate' in (await load(name))) console.log(1);\n",
        "await load(name);\nconst mod = await load(name);\nmod.migrate();\n",
    ] {
        let run = format!("{head}{read}");
        assert_blocks_only_migrate(&analyze_loader(LOADER, &run), &run);
    }
}

#[test]
fn each_loader_shape_counts_as_a_loader() {
    let run = "import { load } from './load';\nconst name = process.argv[2];\nconst mod = await load(name);\nmod.migrate();\n";
    let sync =
        "import { load } from './load';\nconst name = process.argv[2];\nload(name).migrate();\n";
    let then = "import { load } from './load';\nconst name = process.argv[2];\nload(name).then((m) => m.migrate());\n";
    for (loader, run) in [
        (
            "export async function load(path: string) {\n  const mod = await import(path);\n  return mod;\n}\n",
            run,
        ),
        (
            "export function load(path: string) {\n  return import(path);\n}\n",
            then,
        ),
        (
            "export const load = (path: string) => import(path).catch(() => null);\n",
            run,
        ),
        (
            "export const load = function (path: string) { return require(path); };\n",
            sync,
        ),
        (
            "export const load = (path: string) => require(path);\n",
            sync,
        ),
        (
            "const raw = (path: string) => import(path);\nexport const load = async (path: string) => raw(path);\n",
            run,
        ),
    ] {
        assert_blocks_only_migrate(&analyze_loader(loader, run), loader);
    }
}

#[test]
fn a_loader_that_its_own_module_calls_blocks_the_names() {
    // The import keeps `run.ts` a module: the top-level bindings of a
    // classic script are globals that other scripts can call.
    let run = format!(
        "import './migrate';\n{LOADER}const name = process.argv[2];\n(await load(name)).migrate();\n"
    )
    .replace("export const load", "const load");
    assert_blocks_only_migrate(&analyze_loader(LOADER, &run), &run);
}

#[test]
fn a_function_that_returns_a_loader_result_is_a_loader() {
    let loader = format!(
        "{LOADER}export async function open(path: string) {{\n  const mod = await load(path);\n  return mod;\n}}\n"
    );
    let run = "import { open } from './load';\nconst name = process.argv[2];\nconst mod = await open(name);\nmod.migrate();\n";
    assert_blocks_only_migrate(&analyze_loader(&loader, run), run);
}

#[test]
fn a_loader_that_a_barrel_re_exports_blocks_the_names() {
    let main = format!(
        "import {{ migrate }} from './migrate';\nimport './run';\n{MAIN}migrate(1, true);\nmigrate(2, true);\n"
    );
    let run = "import { load } from './barrel';\nconst name = process.argv[2];\n(await load(name)).migrate();\n";
    let project = analyze(&base_with(
        &[
            ("src/load.ts", LOADER),
            ("src/barrel.ts", "export { load } from './load';\n"),
            ("src/migrate.ts", MIGRATE),
            ("src/run.ts", run),
        ],
        &main,
    ));
    assert_blocks_only_migrate(&project, run);
}

#[test]
fn a_loader_result_or_a_loader_that_escapes_blocks_each_export() {
    let head = "import { load } from './load';\nconst name = process.argv[2];\n";
    for read in [
        "console.log(await load(name));\n",
        "const mod = await load(name);\nconst alias = mod;\nalias.migrate();\n",
        "const promise = load(name);\n",
        "[name].map(load);\n",
        "load.call(null, name);\n",
        "load?.(name);\n",
        "const mod = await import(name);\nmod.load('x');\n",
    ] {
        let run = format!("{head}{read}");
        let project = analyze_loader(LOADER, &run);
        assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
    }
}

#[test]
fn a_loader_that_an_entry_point_exports_blocks_each_export() {
    let main = format!("{MAIN}export const load = async (path: string) => await import(path);\n");
    let project = analyze(&base_with(&[], &main));
    assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
}

#[test]
fn a_data_file_suffix_is_not_an_unknown_source() {
    let data = "console.log(await import(`@/locales/${name}.json`));\n";
    assert_eq!(analyze_load(&[], data).findings("format").len(), 2);
    let relative = "console.log(await import(`./locales/${name}.json`));\n";
    assert_eq!(analyze_load(&[], relative).findings("format").len(), 2);
    let required = "console.log(require('@/styles/' + name + '.css'));\n";
    assert_eq!(analyze_load(&[], required).findings("format").len(), 2);
    let mapped = "console.log(await import(`#i18n/${name}.json`));\n";
    let project = analyze_load(&[], mapped);
    assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
}

#[test]
fn a_require_of_an_unknown_source_in_another_module_abstains() {
    let main = format!("import './legacy';\n{MAIN}");
    let legacy = "const mod = require(process.env.TARGET);\nmod.format(4, false);\n";
    let project = analyze(&base_with(&[("src/legacy.js", legacy)], &main));
    assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
    let stats = project.stats();
    let opaque = stats
        .reasons
        .iter()
        .find(|(reason, _)| reason.code() == OPAQUE_REASON)
        .map(|(_, count)| *count);
    assert_eq!(
        opaque,
        Some(ReasonCount {
            first: 1,
            any: 1,
            only: 1
        })
    );
}

#[test]
fn a_dynamic_import_pattern_blocks_only_the_files_it_matches() {
    let plugin = "export function boot(level: number) { return level; }\n";
    let load = "export const later = () => import(`./plugins/${name}.ts`);\n";
    let project = analyze_load(&[("src/plugins/boot.ts", plugin)], load);
    assert_eq!(project.findings("format").len(), 2);
    assert_eq!(project.reason("src/format.ts", "format"), None);
    assert_eq!(
        project.reason("src/plugins/boot.ts", "boot"),
        Some("dynamic-import")
    );
}

#[test]
fn a_function_that_no_export_exposes_stays_a_candidate() {
    let local = "function scale(n: number, flag: boolean) { return flag ? n : 0; }\nscale(1, true);\nscale(2, true);\n";
    let project = analyze_load(
        &[],
        &format!(
            "{local}const mod = await import(name);\nmod.scale(4, false);\nconsole.log(mod);\n"
        ),
    );
    assert_eq!(
        project.findings("scale"),
        vec![(
            "flag".to_string(),
            ConstantParameterReason::IdenticalArgument,
            Some("true".to_string())
        )]
    );
    assert_abstains(&project, "src/format.ts", "format", OPAQUE_REASON);
}

#[test]
fn an_opaque_load_in_a_test_file_counts_only_when_the_file_is_analyzed() {
    let test = "const name = process.argv[2];\ntest('x', async () => {\n  const mod = await import(name);\n  mod.format(4, false);\n});\n";
    let files = base_with(&[("src/format.test.ts", test)], MAIN);
    let default = analyze(&files);
    assert_abstains(&default, "src/format.ts", "format", OPAQUE_REASON);
    let production = analyze_with(
        &private_package(),
        &serde_json::json!({ "entry": ["src/main.ts"], "production": true }),
        &files,
    );
    assert_eq!(production.findings("format").len(), 2);
}

#[test]
fn an_opaque_load_blocks_the_type_aware_class_candidates() {
    let store = "export class Store {\n  save(id: string, retries: number): string { return id + retries; }\n}\n";
    let main = "import { Store } from './store';\nconst store = new Store();\nstore.save('a', 1);\nstore.save('b', 1);\n";
    let config = serde_json::json!({ "entry": ["src/main.ts"], "typeAware": { "enabled": true } });
    let classes = |main: &str| -> Vec<String> {
        analyze_with(
            &private_package(),
            &config,
            &[("src/store.ts", store), ("src/main.ts", main)],
        )
        .results
        .constant_parameter_candidates
        .classes
        .iter()
        .map(|class| class.class_name.clone())
        .collect()
    };
    assert_eq!(classes(main), vec!["Store".to_string()]);
    let other = format!("{main}{OPAQUE}const mod = await import(name);\nmod.Other;\n");
    assert_eq!(classes(&other), vec!["Store".to_string()]);
    let read = format!("{main}{OPAQUE}const mod = await import(name);\nnew mod.Store();\n");
    assert!(classes(&read).is_empty());
    let escape = format!("{main}{OPAQUE}export const later = () => import(name);\n");
    assert!(classes(&escape).is_empty());
}

#[test]
fn a_require_of_the_module_abstains() {
    let main = "import { format } from './format';\nimport './legacy';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    let legacy = "const { format } = require('./format');\nformat(1, false);\n";
    let project = analyze(&base_with(&[("src/legacy.js", legacy)], main));
    assert_abstains(&project, "src/format.ts", "format", "commonjs");
}

#[test]
fn commonjs_exports_abstain() {
    let main =
        "import { scale } from './scale';\nscale(1, true);\nscale(2, true);\nscale(3, true);\n";
    let scale =
        "export function scale(n, flag) { return flag ? n : 0; }\nmodule.exports.extra = 1;\n";
    let project = analyze(&[("src/main.ts", main), ("src/scale.js", scale)]);
    assert_abstains(&project, "src/scale.js", "scale", "commonjs");
}

#[test]
fn a_namespace_that_escapes_abstains() {
    let main = "import * as fmt from './format';\nfmt.format(1, true);\nfmt.format(2, true);\nfmt.format(3, true);\nexport const all = [fmt];\n";
    let project = analyze(&base_with(&[], main));
    assert_abstains(&project, "src/format.ts", "format", "namespace-escape");
}

#[test]
fn namespace_calls_count_as_calls() {
    let main = "import * as fmt from './format';\nfmt.format(1, true);\nfmt.format(2, true);\nfmt.format(3, true);\n";
    let project = analyze(&base_with(&[], main));
    assert_eq!(project.findings("format").len(), 2);
}

#[test]
fn a_value_reference_of_an_import_abstains() {
    let main = "import { format } from './format';\nformat(1, true);\nformat(2, true);\nformat(3, true);\nexport const mapped = [1].map(format);\n";
    let project = analyze(&base_with(&[], main));
    assert_abstains(&project, "src/format.ts", "format", "value-reference");
}

#[test]
fn a_call_through_a_member_path_abstains() {
    let main = "import { format } from './format';\nformat(1, true);\nformat(2, true);\nformat(3, true);\nformat.call(null, 4, false);\n";
    let project = analyze(&base_with(&[], main));
    assert_abstains(&project, "src/format.ts", "format", "value-reference");
}

#[test]
fn a_local_escape_abstains() {
    let main = "function pick(a: number, b: boolean) { return b ? a : 0; }\npick(1, true);\npick(2, true);\npick(3, true);\nexport const later = [pick];\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert_abstains(&project, "src/main.ts", "pick", "value-reference");
}

#[test]
fn an_unresolved_call_target_with_the_same_name_abstains() {
    let main = "import { format } from './format';\nimport './other';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    let other = "import { format } from './missing';\nformat(1, false);\nexport {};\n";
    let project = analyze(&base_with(&[("src/other.ts", other)], main));
    assert_abstains(&project, "src/format.ts", "format", "unresolved-target");
}

#[test]
fn a_template_host_that_imports_the_function_abstains() {
    let main = "import { format } from './format';\nimport './View.vue';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    let view = "<script setup lang=\"ts\">\nimport { format } from './format';\n</script>\n<template><p>{{ format(1, false) }}</p></template>\n";
    let project = analyze(&base_with(&[("src/View.vue", view)], main));
    assert_abstains(&project, "src/format.ts", "format", "template-callers");
}

#[test]
fn a_direct_eval_in_a_caller_abstains() {
    let main = "import { format } from './format';\nformat(1, true);\nformat(2, true);\nformat(3, true);\neval('format(4, false)');\n";
    let project = analyze(&base_with(&[], main));
    assert_abstains(&project, "src/format.ts", "format", "direct-eval");
}

#[test]
fn a_classic_script_abstains() {
    let script = "function greet(name, loud) { return loud ? name : ''; }\ngreet('a', true);\ngreet('b', true);\ngreet('c', true);\n";
    let config = serde_json::json!({ "entry": ["src/legacy.js"] });
    let project = analyze_with(&private_package(), &config, &[("src/legacy.js", script)]);
    assert_abstains(&project, "src/legacy.js", "greet", "classic-script");

    let module = format!("{script}export const marker = 1;\n");
    let project = analyze_with(&private_package(), &config, &[("src/legacy.js", &module)]);
    assert_eq!(project.findings("greet").len(), 1);
}

#[test]
fn fewer_than_two_calls_abstain() {
    let main = "import { format } from './format';\nformat(1, true);\n";
    let project = analyze(&base_with(&[], main));
    assert_abstains(&project, "src/format.ts", "format", "too-few-calls");
}

#[test]
fn calls_through_a_re_export_chain_reach_the_origin() {
    let main = "import { format } from './barrel';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    let barrel = "export { format } from './inner';\n";
    let inner = "export * from './format';\n";
    let project = analyze(&base_with(
        &[("src/barrel.ts", barrel), ("src/inner.ts", inner)],
        main,
    ));
    assert_eq!(project.findings("format").len(), 2);
}

#[test]
fn two_imports_of_one_function_merge_their_calls() {
    let main = "import { format } from './format';\nimport { format as again } from './barrel';\nformat(1, true);\nformat(2, true);\nagain(3, false);\n";
    let barrel = "export { format } from './format';\n";
    let project = analyze(&base_with(&[("src/barrel.ts", barrel)], main));
    assert_eq!(project.findings("format"), never_passed_locale());
}

#[test]
fn a_varied_value_gives_no_finding() {
    let main = "import { format } from './format';\nformat(1, true);\nformat(2, true);\nformat(3, false, 'nl');\n";
    let project = analyze(&base_with(&[], main));
    assert!(project.findings("format").is_empty());
}

#[test]
fn negative_zero_and_zero_are_different_values() {
    let main = "function scale(n: number, by: number) { return n * by; }\nscale(1, 0);\nscale(2, 0);\nscale(3, -0);\nexport {};\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert!(project.findings("scale").is_empty());
}

#[test]
fn an_omitted_argument_and_undefined_are_different() {
    let main = "function pick(a: number, b?: number) { return b ?? a; }\npick(1);\npick(2);\npick(3, undefined);\nexport {};\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert!(project.findings("pick").is_empty());
}

#[test]
fn each_call_passing_undefined_is_an_identical_argument() {
    let main = "function pick(a: number, b?: number) { return b ?? a; }\npick(1, undefined);\npick(2, undefined);\npick(3, undefined);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert_eq!(
        project.findings("pick"),
        vec![(
            "b".to_string(),
            ConstantParameterReason::IdenticalArgument,
            Some("undefined".to_string())
        )]
    );
}

#[test]
fn a_required_parameter_that_no_call_passes_gives_no_finding() {
    let main = "function pick(a, b) { return b ?? a; }\npick(1);\npick(2);\npick(3);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert!(project.findings("pick").is_empty());
}

#[test]
fn written_rest_and_array_pattern_parameters_give_no_finding() {
    let main = "function pick(a, b, [d], { c }, ...rest) { b = !b; return [a, b, c, d, rest]; }\npick(1, true, [1], { c: 1 }, 1);\npick(2, true, [1], { c: 1 }, 1);\npick(3, true, [1], { c: 1 }, 1);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    // Only the option key of the object pattern gets a finding.
    assert_eq!(project.findings("pick"), vec![identical("c", "1")]);
}

#[test]
fn a_spread_argument_hides_the_later_positions() {
    let main = "function pick(a: number, b: boolean) { return b ? a : 0; }\npick(1, true);\npick(2, true);\npick(...([3, false] as [number, boolean]));\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert!(project.findings("pick").is_empty());
}

#[test]
fn a_self_call_that_passes_the_parameter_on_is_neutral() {
    let main = "function walk(node: number, deep: boolean): number {\n  return deep && node > 0 ? walk(node - 1, deep) : node;\n}\nwalk(1, true);\nwalk(2, true);\nwalk(3, true);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert_eq!(
        project.findings("walk"),
        vec![(
            "deep".to_string(),
            ConstantParameterReason::IdenticalArgument,
            Some("true".to_string())
        )]
    );
}

#[test]
fn a_parameter_that_a_self_call_passes_on_is_not_never_passed() {
    let main = "function walk(node: number, seen?: number[]): number {\n  const list = seen ?? [];\n  list.push(node);\n  return node > 0 ? walk(node - 1, seen) : list.length;\n}\nwalk(1);\nwalk(2);\nwalk(3);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert!(project.findings("walk").is_empty());
}

#[test]
fn a_self_call_that_moves_a_parameter_is_unknown() {
    let main = "function swap(a: boolean, b: boolean): boolean {\n  return a ? swap(b, a) : b;\n}\nswap(true, true);\nswap(true, true);\nswap(true, true);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert!(project.findings("swap").is_empty());
}

#[test]
fn a_suppression_on_the_function_or_the_parameter_hides_the_finding() {
    let above_function = format!("// fallow-ignore-next-line constant-parameter\n{FORMAT}");
    let project = analyze(&[("src/format.ts", &above_function), ("src/main.ts", MAIN)]);
    assert!(project.findings("format").is_empty());
    assert!(project.results.stale_suppressions.is_empty());

    let above_parameter = "export function format(\n  value: number,\n  // fallow-ignore-next-line constant-parameter\n  compact: boolean,\n  locale?: string,\n) {\n  if (compact) return String(value);\n  return value.toLocaleString(locale);\n}\n";
    let project = analyze(&[("src/format.ts", above_parameter), ("src/main.ts", MAIN)]);
    assert_eq!(project.findings("format"), never_passed_locale());
}

#[test]
fn a_test_caller_counts_in_the_default_mode() {
    let test = "import { format } from './format';\ntest('x', () => format(4, false));\n";
    let project = analyze(&base_with(&[("src/format.test.ts", test)], MAIN));
    assert_eq!(project.findings("format"), never_passed_locale());
}

#[test]
fn the_rule_is_off_by_default() {
    let (_directory, root) = write_project(
        &private_package(),
        &[("src/format.ts", FORMAT), ("src/main.ts", MAIN)],
    );
    let config: FallowConfig = serde_json::from_value(entry_config()).expect("config");
    let config = config.resolve(root, OutputFormat::Human, 2, true, true, None);
    let results = crate::analyze(&config).expect("analysis succeeds");
    assert!(results.constant_parameters.is_empty());
}

#[test]
fn a_framework_auto_import_abstains() {
    let package = serde_json::json!({
        "name": "fixture",
        "private": true,
        "dependencies": { "nuxt": "*" }
    });
    let composable = "export function useFlag(n: number, on: boolean) {\n  return on ? n : 0;\n}\n";
    let plugin = "export default defineNuxtPlugin(() => {\n  useFlag(1, true);\n  useFlag(2, true);\n  useFlag(3, true);\n});\n";
    let project = analyze_with(
        &package,
        &serde_json::json!({ "autoImports": true }),
        &[
            ("nuxt.config.ts", "export default defineNuxtConfig({});\n"),
            ("composables/useFlag.ts", composable),
            ("plugins/flag.ts", plugin),
        ],
    );
    assert!(project.findings("useFlag").is_empty());
    let reason = project.reason("composables/useFlag.ts", "useFlag");
    assert!(reason == Some("auto-import"), "{reason:?}");
}

#[test]
fn a_value_reference_in_a_module_without_functions_abstains() {
    let main = "import { format } from './format';\nimport './use';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    for user in [
        "import { format } from './format';\nexport const mapped = [1].map(format);\n",
        "import { format } from './format';\n[1].map(format);\nexport {};\n",
    ] {
        let project = analyze(&base_with(&[("src/use.ts", user)], main));
        assert_abstains(&project, "src/format.ts", "format", "value-reference");
    }
}

#[test]
fn a_jsx_use_in_a_module_without_functions_abstains() {
    let card = "export function Card(props: { a: number }) {\n  return props.a;\n}\n";
    let main = "import { Card } from './Card';\nimport './view';\nCard({ a: 1 });\nCard({ a: 1 });\nCard({ a: 1 });\n";
    let view = "import { Card } from './Card';\nexport const element = <Card a={2} />;\n";
    let config = serde_json::json!({ "entry": ["src/main.tsx"] });
    let project = analyze_with(
        &private_package(),
        &config,
        &[
            ("src/Card.tsx", card),
            ("src/main.tsx", main),
            ("src/view.tsx", view),
        ],
    );
    assert_abstains(&project, "src/Card.tsx", "Card", "value-reference");
}

#[test]
fn an_inline_module_script_in_html_abstains() {
    let html = "<!doctype html>\n<script type=\"module\">\n  import { format } from './src/format.ts';\n  format(4, false);\n</script>\n";
    let config = serde_json::json!({ "entry": ["src/main.ts", "index.html"] });
    let project = analyze_with(
        &private_package(),
        &config,
        &base_with(&[("index.html", html)], MAIN),
    );
    assert_abstains(&project, "src/format.ts", "format", "template-callers");

    let html_with_src =
        "<!doctype html>\n<script type=\"module\" src=\"./src/main.ts\"></script>\n";
    let project = analyze_with(
        &private_package(),
        &config,
        &base_with(&[("index.html", html_with_src)], MAIN),
    );
    assert_eq!(project.findings("format").len(), 2);
}

#[test]
fn an_inline_script_that_imports_no_module_keeps_the_findings() {
    let html = "<!doctype html>\n<script type=\"module\">\n  const title = 'from the page';\n  document.title = title;\n</script>\n<script>\n  window.name = 'page';\n</script>\n";
    let config = serde_json::json!({ "entry": ["src/main.ts", "index.html"] });
    let project = analyze_with(
        &private_package(),
        &config,
        &base_with(&[("index.html", html)], MAIN),
    );
    assert_eq!(project.findings("format").len(), 2);
}

#[test]
fn a_template_host_blocks_only_what_it_imports() {
    let main = "import { format } from './format';\nimport './View.vue';\nformat(1, true);\nformat(2, true);\nformat(3, true);\n";
    let view = "<script setup lang=\"ts\">\nimport { other } from './other';\n</script>\n<template><p>{{ other() }}</p></template>\n";
    let other = "export function other() {\n  return 1;\n}\n";
    let project = analyze(&base_with(
        &[("src/View.vue", view), ("src/other.ts", other)],
        main,
    ));
    assert_eq!(project.findings("format").len(), 2);
}

#[test]
fn an_inline_classic_script_with_a_dynamic_import_abstains() {
    let html = "<!doctype html>\n<script>\n  import('./src/format.ts').then((m) => m.format(4, false));\n</script>\n";
    let config = serde_json::json!({ "entry": ["src/main.ts", "index.html"] });
    let project = analyze_with(
        &private_package(),
        &config,
        &base_with(&[("index.html", html)], MAIN),
    );
    assert_abstains(&project, "src/format.ts", "format", "template-callers");
}

#[test]
fn the_gate_stats_count_the_first_any_and_only_reason() {
    let format = "export function format(value: number, compact: boolean, locale?: string) {\n  if (compact) return String(value);\n  return value.toLocaleString(locale);\n}\nexport function few(value: number) {\n  return value;\n}\n";
    let main = "import { format, few } from './format';\nformat(1, true);\nformat(2, true);\nformat(3, true);\nfew(1);\nexport function both(value: number) {\n  return arguments.length + value;\n}\nexport function onlyEntry(value: number) {\n  return value;\n}\n";
    let orphan = "export function orphan(value: number) {\n  return value;\n}\n";
    let project = analyze(&[
        ("src/format.ts", format),
        ("src/main.ts", main),
        ("src/orphan.ts", orphan),
    ]);
    let count = |first, any, only| ReasonCount { first, any, only };
    assert_eq!(
        project.stats(),
        GateStats {
            contracts: 5,
            reachable: 4,
            passed: 1,
            findings: 2,
            reasons: vec![
                (Abstain::OpenContract, count(0, 1, 0)),
                (Abstain::EntryExport, count(2, 2, 1)),
                (Abstain::TooFewCalls, count(1, 1, 1)),
            ],
        }
    );
}

fn identical(name: &str, text: &str) -> (String, ConstantParameterReason, Option<String>) {
    (
        name.to_string(),
        ConstantParameterReason::IdenticalArgument,
        Some(text.to_string()),
    )
}

fn never_passed(name: &str) -> (String, ConstantParameterReason, Option<String>) {
    (name.to_string(), ConstantParameterReason::NeverPassed, None)
}

const CONSTANTS: &str = "export const LIMIT = 10;\nexport const OTHER = 10;\nexport let MUTABLE = 10;\nexport const COMPUTED = Math.max(1, 2);\nexport enum Mode { Grid, List }\n";
const LOAD: &str = "import { Mode } from './constants';\nexport function load(limit: number, mode: Mode) {\n  return [limit, mode];\n}\n";

fn binding_project(main: &str) -> Project {
    analyze(&[
        ("src/constants.ts", CONSTANTS),
        ("src/load.ts", LOAD),
        ("src/barrel.ts", "export * from './constants';\n"),
        ("src/main.ts", main),
    ])
}

#[test]
fn the_same_const_or_enum_member_is_an_identical_argument() {
    let main = "import { load } from './load';\nimport { LIMIT, Mode } from './constants';\nimport * as barrel from './barrel';\nimport { LIMIT as Renamed } from './barrel';\nload(LIMIT, Mode.Grid);\nload(barrel.LIMIT, barrel.Mode.Grid);\nload(Renamed, Mode.Grid);\n";
    let project = binding_project(main);
    assert_eq!(
        project.findings("load"),
        vec![identical("limit", "LIMIT"), identical("mode", "Mode.Grid")]
    );
    let limit = &project.results.constant_parameters[0].parameter;
    assert_eq!(
        limit.value.as_ref().map(|value| value.kind),
        Some(crate::results::ConstantParameterValueKind::Binding)
    );
    assert_eq!(limit.option_key, None);
}

#[test]
fn a_local_const_is_an_identical_argument_in_its_own_module() {
    let main = "const SIZE = 'large';\nfunction render(size: string) { return size; }\nrender(SIZE);\nrender(SIZE);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert_eq!(project.findings("render"), vec![identical("size", "SIZE")]);
}

#[test]
fn different_bindings_or_a_binding_and_its_literal_give_no_finding() {
    // Two members of one enum.
    let main = "import { load } from './load';\nimport { LIMIT, Mode } from './constants';\nload(LIMIT, Mode.Grid);\nload(LIMIT, Mode.List);\n";
    assert_eq!(
        binding_project(main).findings("load"),
        vec![identical("limit", "LIMIT")]
    );
    for main in [
        // Two constants with the same value are two bindings.
        "import { load } from './load';\nimport { LIMIT, OTHER, Mode } from './constants';\nload(LIMIT, Mode.Grid);\nload(OTHER, Mode.Grid);\n",
        // A binding and its literal value.
        "import { load } from './load';\nimport { LIMIT, Mode } from './constants';\nload(LIMIT, Mode.Grid);\nload(10, Mode.Grid);\n",
    ] {
        assert_eq!(
            binding_project(main).findings("load"),
            vec![identical("mode", "Mode.Grid")],
            "{main}"
        );
    }
}

#[test]
fn a_mutable_or_computed_binding_or_a_folded_value_is_unknown() {
    for main in [
        "import { load } from './load';\nimport { MUTABLE } from './constants';\nload(MUTABLE, 0);\nload(MUTABLE, 0);\n",
        "import { load } from './load';\nimport { COMPUTED } from './constants';\nload(COMPUTED, 0);\nload(COMPUTED, 0);\n",
        "import { load } from './load';\nload(1 + 1, 0);\nload(1 + 1, 0);\n",
        "import { load } from './load';\nload([1] as any, 0);\nload([1] as any, 0);\n",
        "import { load } from './load';\nload({ a: 1 } as any, 0);\nload({ a: 1 } as any, 0);\n",
        "import { load } from './load';\nconst local = { a: 1 };\nload(local as any, 0);\nload(local as any, 0);\n",
    ] {
        let project = binding_project(main);
        assert_eq!(
            project.findings("load"),
            vec![identical("mode", "0")],
            "{main}"
        );
    }
}

#[test]
fn local_constants_of_two_modules_are_two_bindings() {
    let first = "import { load } from './load';\nconst LIMIT = 10;\nload(LIMIT, 0);\n";
    let second = "import { load } from './load';\nconst LIMIT = 10;\nload(LIMIT, 0);\n";
    let main = "import './first';\nimport './second';\n";
    let project = analyze(&[
        ("src/constants.ts", CONSTANTS),
        ("src/load.ts", LOAD),
        ("src/first.ts", first),
        ("src/second.ts", second),
        ("src/main.ts", main),
    ]);
    assert_eq!(project.findings("load"), vec![identical("mode", "0")]);
}

#[test]
fn an_accumulator_that_only_the_recursion_passes_gives_no_finding() {
    let main = "function sum(xs: number[], total = 0): number {\n  return xs.length ? sum(xs.slice(1), total + xs[0]) : total;\n}\nsum([1]);\nsum([2]);\nsum([3]);\nfunction collect(node: number, seen: number[] = []): number[] {\n  seen.push(node);\n  return node > 0 ? collect(node - 1, seen) : seen;\n}\ncollect(1);\ncollect(2);\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert!(project.findings("sum").is_empty());
    assert!(project.findings("collect").is_empty());
}

const REQUEST: &str = "export function request(url: string, { retries = 3, signal, mode, cache: store }: { retries?: number; signal?: AbortSignal; mode?: string; cache?: string } = {}) {\n  if (store) return url;\n  return [url, retries, signal, mode];\n}\n";

fn request_project(main: &str) -> Project {
    analyze(&[("src/request.ts", REQUEST), ("src/main.ts", main)])
}

#[test]
fn an_option_key_that_no_call_sets_or_that_each_call_sets_alike_is_reported() {
    let main = "import { request } from './request';\nimport { signal } from './signal';\nrequest('a', { mode: 'cors' });\nrequest('b', { mode: 'cors', signal });\nrequest('c', { mode: 'cors', cache: 'no-store' });\n";
    let signal = "export const signal = new AbortController().signal;\n";
    let project = analyze(&[
        ("src/request.ts", REQUEST),
        ("src/signal.ts", signal),
        ("src/main.ts", main),
    ]);
    assert_eq!(
        project.findings("request"),
        vec![identical("mode", "cors"), never_passed("retries")]
    );
    let retries = project
        .results
        .constant_parameters
        .iter()
        .map(|finding| &finding.parameter)
        .find(|parameter| parameter.parameter_name == "retries")
        .expect("retries finding");
    assert_eq!(retries.option_key.as_deref(), Some("retries"));
    assert!(retries.has_default);
    assert_eq!(retries.parameter_index, 1);
    assert_eq!(retries.label(), "{ retries }");
    assert_eq!(retries.line, 1);
}

#[test]
fn an_omitted_options_object_with_an_empty_default_sets_no_key() {
    let main = "import { request } from './request';\nrequest('a');\nrequest('b', undefined);\nrequest('c', {});\n";
    let project = request_project(main);
    assert_eq!(
        project.findings("request"),
        vec![
            never_passed("mode"),
            never_passed("retries"),
            never_passed("signal"),
            never_passed("store"),
        ]
    );
    let store = project
        .results
        .constant_parameters
        .iter()
        .map(|finding| &finding.parameter)
        .find(|parameter| parameter.parameter_name == "store")
        .expect("store finding");
    assert_eq!(store.label(), "{ cache: store }");
    assert_eq!(store.condition_reads, 1);
}

#[test]
fn an_options_object_that_the_facts_cannot_read_gives_no_key_finding() {
    for main in [
        "import { request } from './request';\nconst options = { mode: 'cors' };\nrequest('a', { mode: 'cors' });\nrequest('b', options);\n",
        "import { request } from './request';\nconst extra = {};\nrequest('a', { mode: 'cors' });\nrequest('b', { mode: 'cors', ...extra });\n",
        "import { request } from './request';\nconst key = 'retries';\nrequest('a', { mode: 'cors' });\nrequest('b', { mode: 'cors', [key]: 1 });\n",
        "import { request } from './request';\nconst proto = { retries: 1 };\nrequest('a', { mode: 'cors' });\nrequest('b', { mode: 'cors', __proto__: proto });\n",
    ] {
        let project = request_project(main);
        assert!(project.findings("request").is_empty(), "{main}");
    }
}

#[test]
fn a_default_object_with_keys_or_a_written_key_gives_no_key_finding() {
    let request = "export function request(url: string, { retries }: { retries?: number } = { retries: 5 }) {\n  return [url, retries];\n}\nexport function write(url: string, { retries }: { retries?: number } = {}) {\n  retries = (retries ?? 0) + 1;\n  return [url, retries];\n}\nexport function required(url: string, { retries }: { retries?: number }) {\n  return [url, retries];\n}\n";
    let main = "import { request, write, required } from './request';\nrequest('a');\nrequest('b');\nwrite('a');\nwrite('b');\nrequired('a', {});\nrequired('b', {});\nrequired('c' as any);\n";
    let project = analyze(&[("src/request.ts", request), ("src/main.ts", main)]);
    assert!(project.findings("request").is_empty());
    assert!(project.findings("write").is_empty());
    assert!(project.findings("required").is_empty());
}

#[test]
fn a_self_call_that_passes_an_option_key_on_is_neutral() {
    let main = "function walk(node: number, { depth = 0, seen }: { depth?: number; seen?: number[] } = {}): number {\n  return node > 0 ? walk(node - 1, { depth, seen }) : depth;\n}\nwalk(1, { depth: 1 });\nwalk(2, { depth: 1 });\nexport const marker = 1;\n";
    let project = analyze(&[("src/main.ts", main)]);
    assert_eq!(project.findings("walk"), vec![identical("depth", "1")]);
}

#[test]
fn a_function_that_a_test_file_declares_is_not_a_candidate() {
    let helper =
        "export function makeUser(name: string, admin: boolean) {\n  return { name, admin };\n}\n";
    let test = "import { makeUser } from './helpers';\nimport { format } from '../format';\nfunction build(flag: boolean) { return flag; }\nbuild(true);\nbuild(true);\nmakeUser('a', true);\nmakeUser('b', true);\nformat(4, true);\n";
    let project = analyze(&base_with(
        &[
            ("src/__tests__/helpers.ts", helper),
            ("src/__tests__/user.test.ts", test),
        ],
        MAIN,
    ));
    assert_abstains(
        &project,
        "src/__tests__/helpers.ts",
        "makeUser",
        "test-file",
    );
    assert_abstains(&project, "src/__tests__/user.test.ts", "build", "test-file");
    // The test file stays a caller of the functions of other files.
    let rows = project.findings("format");
    assert!(rows.iter().any(|row| row.0 == "compact"), "{rows:?}");
}

const RUNTIME_OP: &str = "export function op(kind: string, compact: boolean) {\n  return compact ? kind : kind + '!';\n}\nfunction inner(flag: boolean) {\n  return flag;\n}\nexport const run = () => {\n  op('a', true);\n  op('b', true);\n  inner(true);\n  inner(true);\n};\n";

/// A project whose main module calls `run` and then has `extra`.
fn runtime_project(extra: &str) -> Project {
    let main = format!("import {{ run }} from './runtime/op';\nrun();\n{extra}");
    let other = "export const other = 1;\n";
    analyze(&[
        ("src/runtime/op.ts", RUNTIME_OP),
        ("src/runtime/other.ts", other),
        ("src/main.ts", main.as_str()),
    ])
}

#[test]
fn a_module_that_an_asset_import_reads_abstains() {
    // A `?raw` or `?url` import, or an inline loader, reads the module as
    // text or as a file, so a copy of the code can have callers that the
    // project does not hold.
    for extra in [
        "import source from './runtime/op.ts?raw';\nexport const text = source;\n",
        "import href from './runtime/op.ts?url';\nexport const text = href;\n",
        "import Worker from './runtime/op.ts?worker&inline';\nexport const text = Worker;\n",
        "export { default as text } from './runtime/op.ts?raw';\n",
        "export const load = () => import('./runtime/op.ts?raw');\n",
        "import source from '!!raw-loader!./runtime/op.ts';\nexport const text = source;\n",
    ] {
        let project = runtime_project(extra);
        assert_abstains(&project, "src/runtime/op.ts", "op", "asset-import");
        assert_abstains(&project, "src/runtime/op.ts", "inner", "asset-import");
    }
}

#[test]
fn an_asset_import_of_another_module_or_a_type_keeps_the_findings() {
    for extra in [
        "",
        "import source from './runtime/other.ts?raw';\nexport const text = source;\n",
        "import type Source from './runtime/op.ts?raw';\nexport type Text = Source;\n",
    ] {
        let project = runtime_project(extra);
        assert_eq!(
            project.findings("op"),
            vec![identical("compact", "true")],
            "{extra}"
        );
        assert_eq!(
            project.findings("inner"),
            vec![identical("flag", "true")],
            "{extra}"
        );
    }
}

/// The calls of an importer do not reach a function behind a wrapped default
/// export, so the rule must not report from the local calls alone.
#[test]
fn a_wrapped_default_export_gives_no_finding() {
    for wrapped in ["pad as Fn", "pad satisfies Fn", "pad!", "(pad)"] {
        let pad = format!(
            "type Fn = (s: string, n: number) => string;\nfunction pad(s: string, n: number) {{ return s + n; }}\npad('x', 1);\npad('y', 1);\nexport default {wrapped};\n"
        );
        let project = analyze(&[
            ("src/pad.ts", pad.as_str()),
            ("src/main.ts", "import pad from './pad';\npad('z', 2);\n"),
        ]);
        assert_abstains(&project, "src/pad.ts", "pad", "value-reference");
    }
}

#[test]
fn a_bare_default_export_counts_the_calls_of_the_importer() {
    let project = analyze(&[
        (
            "src/pad.ts",
            "function pad(s: string, n: number) { return s + n; }\npad('x', 1);\npad('y', 1);\nexport default pad;\n",
        ),
        ("src/main.ts", "import pad from './pad';\npad('z', 2);\n"),
    ]);
    assert!(project.findings("pad").is_empty());
}
