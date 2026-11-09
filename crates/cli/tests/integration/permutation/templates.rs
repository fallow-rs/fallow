//! Project templates for the permutation oracle. A template and a seed give
//! one monorepo. Each monorepo has library packages with used and unused
//! exports, a duplicated block, a complex function, and used and unused
//! dependencies. The framework templates add apps whose framework alias
//! (`$lib` or `~/`) has the same target names in each app (issue #4287).

use std::fmt::Write as _;

use super::project::{Project, Rng};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// Library packages and plain TypeScript apps.
    Packages,
    /// Library packages and SvelteKit apps that import from `$lib`.
    SvelteKit,
    /// Library packages and Nuxt apps that import from `~/`.
    Nuxt,
}

pub const TEMPLATES: [Template; 3] = [Template::Packages, Template::SvelteKit, Template::Nuxt];

const APP_NAMES: [&str; 3] = ["alpha", "beta", "gamma"];

/// Build the project of `template` for `seed`.
pub fn generate(template: Template, seed: u64) -> Project {
    let mut rng = Rng::new(seed);
    let mut project = Project::default();
    let libs = rng.range(2, 4);
    let apps = rng.range(2, 3);

    let mut patterns = vec!["packages/*".to_owned(), "apps/*".to_owned()];
    if rng.chance(50) {
        // A second glob over a member that `packages/*` already matches.
        patterns.push("packages/lib-0".to_owned());
    }
    rng.shuffle(&mut patterns);
    let patterns = patterns
        .iter()
        .map(|pattern| format!("\"{pattern}\""))
        .collect::<Vec<_>>()
        .join(",");
    project.file(
        "package.json",
        format!(
            r#"{{"name":"root","private":true,"type":"module","workspaces":[{patterns}],"devDependencies":{{"typescript":"^5.6.0"}}}}"#
        ),
    );

    for lib in 0..libs {
        library(&mut project, &mut rng, lib, libs);
    }
    for app in APP_NAMES.iter().take(apps) {
        match template {
            Template::Packages => plain_app(&mut project, &mut rng, app, libs),
            Template::SvelteKit => sveltekit_app(&mut project, app),
            Template::Nuxt => nuxt_app(&mut project, app),
        }
    }

    project.dependency = "zod";
    project.symbol = "packages/lib-0/src/index.ts:value0";
    project.changed(
        "packages/lib-0/src/added.ts",
        format!(
            "export const added = 1;\n\n{}",
            complex_function("addedBranches", 14)
        ),
    );
    project.changed(
        format!("apps/{}/src/extra.ts", APP_NAMES[0]),
        "export const extra = () => 'extra';\n",
    );
    project
}

fn library(project: &mut Project, rng: &mut Rng, lib: usize, libs: usize) {
    let dir = format!("packages/lib-{lib}");
    let unused_dependency = if rng.chance(50) {
        r#","left-pad":"^1.3.0""#
    } else {
        ""
    };
    project.file(
        format!("{dir}/package.json"),
        format!(
            r#"{{"name":"@demo/lib-{lib}","version":"1.0.0","type":"module","main":"src/index.ts","dependencies":{{"zod":"^3.23.0"{unused_dependency}}}}}"#
        ),
    );

    let mut index = String::from("import { z } from 'zod';\n");
    if lib + 1 < libs && rng.chance(60) {
        let next = lib + 1;
        let _ = writeln!(index, "import {{ value{next} }} from '@demo/lib-{next}';");
        let _ = writeln!(index, "export const chained{lib} = () => value{next}();");
    }
    let _ = writeln!(index, "export const schema{lib} = z.string();");
    let _ = writeln!(
        index,
        "export const value{lib} = (): string => 'lib-{lib}';"
    );
    for extra in 0..rng.range(1, 3) {
        let _ = writeln!(index, "export const unused{lib}x{extra} = {extra};");
    }
    let _ = writeln!(index, "export {{ helper{lib} }} from './helper';");
    index.push_str(&complex_function(
        &format!("branches{lib}"),
        rng.range(10, 16),
    ));
    project.file(format!("{dir}/src/index.ts"), index);
    project.file(
        format!("{dir}/src/helper.ts"),
        format!("export const helper{lib} = (input: number): number => input * {lib};\n"),
    );
    project.file(format!("{dir}/src/copy.ts"), duplicated_block(lib));
    if rng.chance(50) {
        project.file(
            format!("{dir}/src/orphan.ts"),
            format!("export const orphan{lib} = true;\n"),
        );
    }
}

fn plain_app(project: &mut Project, rng: &mut Rng, app: &str, libs: usize) {
    let dir = format!("apps/{app}");
    let lib = rng.range(0, libs - 1);
    project.file(
        format!("{dir}/package.json"),
        format!(
            r#"{{"name":"{app}","private":true,"type":"module","main":"src/main.ts","dependencies":{{"@demo/lib-{lib}":"*","zod":"^3.23.0"}}}}"#
        ),
    );
    project.file(
        format!("{dir}/src/main.ts"),
        format!(
            "import {{ value{lib}, schema{lib} }} from '@demo/lib-{lib}';\nimport {{ z }} from 'zod';\nimport {{ local }} from './local';\n\nexport const run = () => [value{lib}(), schema{lib}.parse('x'), z.number(), local];\n"
        ),
    );
    project.file(
        format!("{dir}/src/local.ts"),
        format!("export const local = '{app}';\nexport const spare = '{app}';\n"),
    );
}

/// A SvelteKit app whose `$lib` files have the same names in each app.
fn sveltekit_app(project: &mut Project, app: &str) {
    let dir = format!("apps/{app}");
    project.file(
        format!("{dir}/package.json"),
        format!(
            r#"{{"name":"{app}","private":true,"type":"module","devDependencies":{{"@sveltejs/kit":"^2.16.0","svelte":"^5.0.0","vite":"^6.2.6"}}}}"#
        ),
    );
    project.file(
        format!("{dir}/svelte.config.js"),
        "export default { kit: {} };\n",
    );
    project.file(
        format!("{dir}/src/lib/util.ts"),
        format!("export const util = '{app}';\nexport const only_{app} = 1;\n"),
    );
    project.file(
        format!("{dir}/src/routes/+page.svelte"),
        "<script lang=\"ts\">\n\timport { util } from '$lib/util';\n</script>\n<p>{util}</p>\n",
    );
}

/// A Nuxt app whose `~/` files have the same names in each app.
fn nuxt_app(project: &mut Project, app: &str) {
    let dir = format!("apps/{app}");
    project.file(
        format!("{dir}/package.json"),
        format!(r#"{{"name":"{app}","private":true,"type":"module","dependencies":{{"nuxt":"^3.0.0"}}}}"#),
    );
    project.file(
        format!("{dir}/nuxt.config.ts"),
        "export default defineNuxtConfig({});\n",
    );
    project.file(
        format!("{dir}/app/utils/format.ts"),
        format!("export const format = '{app}';\nexport const only_{app} = 1;\n"),
    );
    project.file(
        format!("{dir}/app/app.vue"),
        "<script setup lang=\"ts\">\nimport { format } from '~/utils/format';\n</script>\n<template><p>{{ format }}</p></template>\n",
    );
}

/// A function with `count` branches, above the default complexity limits
/// when `count` is high enough.
fn complex_function(name: &str, count: usize) -> String {
    let mut body =
        format!("export function {name}(a: number, b: boolean): number {{\n  let total = 0;\n");
    for step in 0..count {
        let _ = writeln!(
            body,
            "  if (a > {step} && b) {{ total += {step}; }} else if (a < -{step}) {{ total -= 1; }}"
        );
    }
    body.push_str("  return total;\n}\n");
    body
}

/// The same block in each library, so `dupes` reports one clone group.
fn duplicated_block(lib: usize) -> String {
    format!(
        "export function copy{lib}(items: number[]): number {{\n  let sum = 0;\n  for (const item of items) {{\n    if (item % 2 === 0) {{\n      sum += item * 3;\n    }} else {{\n      sum -= item + 7;\n    }}\n    if (sum > 1000) {{\n      sum = sum % 997;\n    }}\n  }}\n  const scaled = sum * 11 + items.length;\n  return scaled > 0 ? scaled : -scaled;\n}}\n"
    )
}
