//! The project template of the surface differential. A seed gives one
//! monorepo with:
//!
//! - library packages with used and unused exports, an unused type export
//!   and sometimes an unreachable file;
//! - an app that imports one value from each library;
//! - a command-line tool (`jscpd`) that the app declares and only a sibling
//!   library configures, and one (`madge`) that the root declares and only
//!   that library configures (#3223). The report flags both, so a trace must
//!   give them no credit;
//! - two clone families: a short one on the earlier file set and a long one
//!   on the later file set. An order by duplicated lines puts the long family
//!   first, and the order by file set puts it last (#3390).

use std::fmt::Write as _;

use serde_json::json;

use super::project::{Project, Rng};

/// Build the project for `seed`.
pub fn generate(seed: u64) -> Project {
    let mut rng = Rng::new(seed);
    let mut project = Project::default();
    let libs = rng.range(1, 3);

    manifest(
        &mut project,
        "package.json",
        &json!({
            "name": "surface-root",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": { "madge": "^8.0.0" }
        }),
    );

    for lib in 0..libs {
        library(&mut project, &mut rng, lib);
    }
    app(&mut project, &mut rng, libs);
    clone_families(&mut project, &mut rng);
    project
}

fn manifest(project: &mut Project, path: &str, value: &serde_json::Value) {
    for section in ["dependencies", "devDependencies"] {
        if let Some(names) = value[section].as_object() {
            for name in names.keys() {
                project.declare(name, path);
            }
        }
    }
    let mut text = serde_json::to_string_pretty(value).expect("manifest JSON");
    text.push('\n');
    project.file(path, text);
}

fn library(project: &mut Project, rng: &mut Rng, lib: usize) {
    let dir = format!("packages/lib-{lib}");
    let mut package = json!({
        "name": format!("@surface/lib-{lib}"),
        "version": "1.0.0",
        "type": "module",
        "main": "src/index.ts",
        "dependencies": { "zod": "^3.23.0" }
    });
    if rng.chance(50) {
        package["dependencies"]["left-pad"] = json!("^1.3.0");
    }
    if lib == 0 {
        // The sibling configuration that credited the app's `jscpd` and the
        // root's `madge` in the trace before #3223.
        package["jscpd"] = json!({ "threshold": 1 });
        project.file(format!("{dir}/.jscpd.json"), "{ \"threshold\": 1 }\n");
        project.file(
            format!("{dir}/.madgerc"),
            "{ \"fileExtensions\": [\"ts\"] }\n",
        );
    }
    manifest(project, &format!("{dir}/package.json"), &package);

    let index = format!("{dir}/src/index.ts");
    let mut text = String::from("import { z } from 'zod';\n\n");
    let _ = writeln!(text, "export const schema{lib} = z.string();");
    let _ = writeln!(text, "export const value{lib} = (): string => 'lib-{lib}';");
    for extra in 0..rng.range(1, 3) {
        let _ = writeln!(text, "export const unused{lib}x{extra} = {extra};");
    }
    let _ = writeln!(text, "export type Shape{lib} = {{ id: number }};");
    project.file(&index, text);
    project.used_exports.push((index, format!("value{lib}")));

    if rng.chance(50) {
        project.file(
            format!("{dir}/src/orphan.ts"),
            format!("export const orphan{lib} = true;\n"),
        );
    }
}

fn app(project: &mut Project, rng: &mut Rng, libs: usize) {
    let dir = "packages/app";
    let mut dependencies = serde_json::Map::new();
    dependencies.insert("zod".into(), json!("^3.23.0"));
    for lib in 0..libs {
        dependencies.insert(format!("@surface/lib-{lib}"), json!("*"));
    }
    if rng.chance(50) {
        dependencies.insert("unused-dep".into(), json!("^1.0.0"));
    }
    manifest(
        project,
        &format!("{dir}/package.json"),
        &json!({
            "name": "app",
            "private": true,
            "type": "module",
            "main": "src/main.ts",
            "dependencies": dependencies,
            "devDependencies": { "jscpd": "^4.0.0" }
        }),
    );

    let mut main = String::from("import { z } from 'zod';\nimport { local } from './local';\n");
    let mut calls = vec!["z.number()".to_owned(), "local".to_owned()];
    for lib in 0..libs {
        let _ = writeln!(main, "import {{ value{lib} }} from '@surface/lib-{lib}';");
        calls.push(format!("value{lib}()"));
    }
    let _ = writeln!(main, "\nexport const run = () => [{}];", calls.join(", "));
    project.file(format!("{dir}/src/main.ts"), main);
    project.file(
        format!("{dir}/src/local.ts"),
        "export const local = 'app';\nexport const spare = 'app';\n",
    );
    project
        .used_exports
        .push((format!("{dir}/src/local.ts"), "local".to_owned()));
}

fn clone_families(project: &mut Project, rng: &mut Rng) {
    let dir = "packages/lib-0/src/copies";
    let short = duplicated_block("short", rng.range(2, 3));
    let long = duplicated_block("long", rng.range(6, 9));
    for (name, block) in [("a", &short), ("b", &short), ("y", &long), ("z", &long)] {
        project.file(format!("{dir}/{name}.ts"), block.clone());
    }
}

/// A function with `steps` branch blocks, long enough for the default
/// duplication thresholds.
fn duplicated_block(name: &str, steps: usize) -> String {
    let mut text = format!(
        "export function {name}(items: number[]): number {{\n  let sum = 0;\n  for (const item of items) {{\n"
    );
    for step in 0..steps {
        let _ = write!(
            text,
            "    if (item % {} === 0) {{\n      sum += item * {};\n    }} else {{\n      sum -= item + {};\n    }}\n",
            step + 2,
            step + 3,
            step + 7
        );
    }
    text.push_str("  }\n  return sum;\n}\n");
    text
}

/// A project whose parse cache is larger than one megabyte, so a
/// `FALLOW_CACHE_MAX_SIZE` of `1` must remove entries. The entry file
/// re-exports each module, so the exports are public and the report is
/// small.
pub fn large_cache() -> Project {
    let mut project = Project::default();
    manifest(
        &mut project,
        "package.json",
        &json!({ "name": "cache-root", "private": true, "main": "src/index.ts" }),
    );
    let mut index = String::new();
    for file in 0..LARGE_CACHE_FILES {
        let _ = writeln!(index, "export * from './m{file}';");
        let mut module = String::new();
        for export in 0..LARGE_CACHE_EXPORTS {
            let _ = writeln!(
                module,
                "export const v{file}x{export} = (input: number): number => input + {export};"
            );
        }
        project.file(format!("src/m{file}.ts"), module);
    }
    project.file("src/index.ts", index);
    project
}

const LARGE_CACHE_FILES: usize = 80;
const LARGE_CACHE_EXPORTS: usize = 900;
