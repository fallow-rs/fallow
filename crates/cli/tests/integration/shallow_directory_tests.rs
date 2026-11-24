//! Shallow directories in `fallow health --shallow-directories`.
//!
//! A directory is shallow when files outside it use at least as many of its
//! interface items as it keeps inside, and it shows at least
//! `health.shallowDirectories.minInterface` items.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;
use tempfile::tempdir;

use crate::common::{parse_json, run_fallow_in_root};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent directories");
    }
    std::fs::write(path, contents).expect("write file");
}

fn functions(names: &[&str]) -> String {
    names.iter().fold(String::new(), |mut out, name| {
        let _ = writeln!(
            out,
            "export function {name}({name}Input: string): string {{\n  return {name}Input;\n}}"
        );
        out
    })
}

fn calls(names: &[&str]) -> String {
    names
        .iter()
        .map(|name| format!("{name}('x')"))
        .collect::<Vec<_>>()
        .join(", ")
}

const UTILS_STRINGS: [&str; 5] = ["trim", "pad", "upper", "lower", "title"];
const UTILS_NUMBERS: [&str; 4] = ["clamp", "round", "floor", "ceil"];
const ENGINE_STEPS: [&str; 9] = ["s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9"];

/// How [`project_with`] shapes the fixture.
#[derive(Clone, Copy)]
struct Shape {
    /// Files outside `src/utils` that use each utility.
    consumers: usize,
    /// The package entry re-exports `src/utils` as public API.
    publish: bool,
}

const SHALLOW: Shape = Shape {
    consumers: 2,
    publish: false,
};

fn project(config: Option<&str>) -> tempfile::TempDir {
    project_with(config, SHALLOW)
}

/// - `src/utils` shows 9 functions and the `Cache` class through its barrel,
///   and hides 1 function: shallow. `Units` holds static data only, so it is
///   no item. A type-only import of `Cache` is no use.
/// - `src/engine` shows 1 function and hides 9 steps: deep. A test outside the
///   directory imports the 9 steps, and a test is not an outside user.
/// - `src/types` shows only types, constants and an enum: no interface items.
fn project_with(config: Option<&str>, shape: Shape) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let main = if shape.publish {
        "src/index.ts"
    } else {
        "src/main.ts"
    };
    write_file(
        &root.join("package.json"),
        &format!(r#"{{"name":"shallow","type":"module","main":"{main}"}}"#),
    );
    if let Some(config) = config {
        write_file(&root.join(".fallowrc.json"), config);
    }
    if shape.publish {
        write_file(
            &root.join("src/index.ts"),
            "export * from './utils';\nimport './main';\n",
        );
    }
    let utils: Vec<&str> = UTILS_STRINGS
        .iter()
        .chain(&UTILS_NUMBERS)
        .copied()
        .collect();
    let mut imports = String::new();
    for consumer in 1..shape.consumers {
        let _ = writeln!(imports, "import './consumer{consumer}';");
        write_file(
            &root.join(format!("src/consumer{consumer}.ts")),
            &format!(
                "import {{ Cache, Units, {} }} from './utils';\nconsole.log(new Cache(), Units, {});\n",
                utils.join(", "),
                calls(&utils),
            ),
        );
    }
    write_file(
        &root.join("src/main.ts"),
        &format!(
            "{imports}import {{ Cache, Units, {} }} from './utils';\n\
             import {{ run }} from './engine';\n\
             import {{ LIMIT, Mode, type Shape }} from './types';\n\
             const shape: Shape = {{ size: LIMIT }};\n\
             console.log(new Cache(), Units, shape, Mode.On, run('x'), {});\n",
            utils.join(", "),
            calls(&utils),
        ),
    );
    write_modules(root);
    dir
}

/// The `utils`, `engine` and `types` directories of [`project_with`].
fn write_modules(root: &Path) {
    write_file(
        &root.join("src/types/uses.ts"),
        "import type { Cache } from '../utils';\nexport type Store = Cache;\n",
    );
    write_file(
        &root.join("src/utils/index.ts"),
        "export * from './strings';\nexport * from './numbers';\n",
    );
    write_file(
        &root.join("src/utils/strings.ts"),
        &format!(
            "{}export class Cache {{\n  get(key: string): string {{\n    return key;\n  }}\n}}\n\
             export class Units {{\n  static readonly A = 1;\n}}\n",
            functions(&UTILS_STRINGS)
        ),
    );
    write_file(
        &root.join("src/utils/numbers.ts"),
        &format!(
            "{}function helper(value: string): string {{\n  return value;\n}}\nhelper('x');\n",
            functions(&UTILS_NUMBERS)
        ),
    );
    write_file(
        &root.join("src/engine/index.ts"),
        "export { run } from './run';\n",
    );
    write_file(
        &root.join("src/engine/run.ts"),
        &format!(
            "import {{ {} }} from './steps';\n\
             export function run(value: string): string {{\n  return [{}].join(value);\n}}\n",
            ENGINE_STEPS.join(", "),
            ENGINE_STEPS
                .iter()
                .map(|name| format!("{name}(value)"))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    );
    write_file(&root.join("src/engine/steps.ts"), &functions(&ENGINE_STEPS));
    write_file(
        &root.join("tests/engine.test.ts"),
        &format!(
            "import {{ {} }} from '../src/engine/steps';\nconsole.log({});\n",
            ENGINE_STEPS.join(", "),
            ENGINE_STEPS
                .iter()
                .map(|name| format!("{name}('x')"))
                .collect::<Vec<_>>()
                .join(", "),
        ),
    );
    write_file(
        &root.join("src/types/index.ts"),
        "export interface Shape { size: number }\n\
         export type Size = number;\n\
         export const LIMIT = 10;\n\
         export enum Mode { On, Off }\n",
    );
}

fn shallow_report(root: &Path, extra: &[&str]) -> Value {
    let mut args = vec!["--shallow-directories", "--format", "json", "--quiet"];
    args.extend_from_slice(extra);
    let output = run_fallow_in_root("health", root, &args);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    parse_json(&output)
}

fn directory_paths(report: &Value) -> Vec<&str> {
    report["shallow_directories"]["directories"]
        .as_array()
        .map(|dirs| {
            dirs.iter()
                .map(|dir| dir["path"].as_str().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn reports_a_directory_that_shows_as_much_as_it_hides() {
    let dir = project(None);
    let report = shallow_report(dir.path(), &[]);

    assert!(
        report["sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| section == "shallow-directories"),
        "sections: {}",
        report["sections"]
    );
    assert_eq!(directory_paths(&report), ["src/utils"], "{report:#}");

    let utils = &report["shallow_directories"]["directories"][0];
    assert_eq!(utils["anchor_file"], "src/utils/index.ts");
    assert_eq!(utils["shown_count"], 10);
    assert_eq!(utils["shown_files"], 2);
    assert_eq!(utils["hidden_count"], 1);
    assert_eq!(utils["items"].as_array().unwrap().len(), 7);
    assert_eq!(utils["more_items"], 3);
    let cache = utils["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "Cache")
        .expect("Cache is a shown class");
    assert_eq!(cache["kind"], "class");
    assert_eq!(cache["members"], 1);
    assert_eq!(cache["outside_files"], 2, "a type-only import is no use");
    assert_eq!(utils["actions"][0]["type"], "narrow-interface");
    assert_eq!(utils["actions"][1]["type"], "ignore-directory");
    assert_eq!(report["shallow_directories"]["min_interface"], 8);
}

#[test]
fn min_interface_raises_the_bar() {
    let dir = project(Some(
        r#"{"health":{"shallowDirectories":{"minInterface":11}}}"#,
    ));
    let report = shallow_report(dir.path(), &[]);
    assert!(directory_paths(&report).is_empty(), "{report:#}");
    assert_eq!(report["shallow_directories"]["min_interface"], 11);
}

#[test]
fn ignore_glob_hides_a_directory() {
    let dir = project(Some(
        r#"{"health":{"shallowDirectories":{"ignore":["src/utils"]}}}"#,
    ));
    let report = shallow_report(dir.path(), &[]);
    assert!(directory_paths(&report).is_empty(), "{report:#}");
}

#[test]
fn a_plain_health_run_does_not_compute_the_section() {
    let dir = project(None);
    let output = run_fallow_in_root("health", dir.path(), &["--format", "json", "--quiet"]);
    let report = parse_json(&output);
    assert!(report.get("shallow_directories").is_none(), "{report:#}");
    assert!(
        !report["sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|section| section == "shallow-directories")
    );
}

const HOOKS: [&str; 9] = [
    "useA", "useB", "useC", "useD", "useE", "useF", "useG", "useH", "useI",
];
const COMPONENTS: [&str; 9] = [
    "Button",
    "ButtonIcon",
    "ButtonLabel",
    "Dialog",
    "DialogTitle",
    "DialogBody",
    "Menu",
    "MenuItem",
    "MenuGroup",
];

/// - `src/hooks` holds one hook per file: a catalog of modules, not a module.
/// - `src/ui` shows 9 components: each component is a unit of its own.
fn catalog_project() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let root = dir.path();
    write_file(
        &root.join("package.json"),
        r#"{"name":"catalog","type":"module","main":"src/main.tsx"}"#,
    );
    let mut main = String::new();
    for hook in HOOKS {
        let _ = writeln!(main, "import {{ {hook} }} from './hooks/{hook}';");
        write_file(
            &root.join(format!("src/hooks/{hook}.ts")),
            &functions(&[hook]),
        );
    }
    let _ = writeln!(main, "import {{ {} }} from './ui';", COMPONENTS.join(", "));
    let _ = writeln!(main, "console.log({});", calls(&HOOKS));
    let _ = writeln!(main, "console.log({});", COMPONENTS.join(", "));
    write_file(&root.join("src/main.tsx"), &main);
    write_file(
        &root.join("src/ui/index.ts"),
        "export * from './button';\nexport * from './dialog';\nexport * from './menu';\n",
    );
    for (file, names) in [
        ("button", &COMPONENTS[0..3]),
        ("dialog", &COMPONENTS[3..6]),
        ("menu", &COMPONENTS[6..9]),
    ] {
        let source = names.iter().fold(String::new(), |mut out, name| {
            let _ = writeln!(out, "export function {name}() {{\n  return <div />;\n}}");
            out
        });
        write_file(&root.join(format!("src/ui/{file}.tsx")), &source);
    }
    dir
}

#[test]
fn catalogs_and_component_kits_are_not_shallow() {
    let dir = catalog_project();
    let report = shallow_report(dir.path(), &[]);
    assert!(directory_paths(&report).is_empty(), "{report:#}");
}

#[test]
fn top_cuts_the_list_but_keeps_the_total() {
    let dir = project(Some(
        r#"{"health":{"shallowDirectories":{"minInterface":1}}}"#,
    ));
    let report = shallow_report(dir.path(), &["--top", "0"]);
    assert!(directory_paths(&report).is_empty(), "{report:#}");
    assert_eq!(report["shallow_directories"]["total"], 1, "{report:#}");
}

#[test]
fn an_interface_with_one_reader_per_item_is_not_shallow() {
    let dir = project_with(
        None,
        Shape {
            consumers: 1,
            ..SHALLOW
        },
    );
    let report = shallow_report(dir.path(), &[]);
    assert!(directory_paths(&report).is_empty(), "{report:#}");
}

#[test]
fn exports_that_the_package_entry_publishes_are_no_items() {
    let dir = project_with(
        None,
        Shape {
            publish: true,
            ..SHALLOW
        },
    );
    let report = shallow_report(dir.path(), &[]);
    assert!(directory_paths(&report).is_empty(), "{report:#}");
}
