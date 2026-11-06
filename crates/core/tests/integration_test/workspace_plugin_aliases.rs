//! Plugin path aliases from a workspace package apply only to the files of
//! that package. Two SvelteKit apps both map `$lib` to their own `src/lib`,
//! and two Nuxt apps both map `~/` to their own source directory.

use super::common::create_config;
use std::path::Path;

const APPS: [&str; 2] = ["alpha", "beta"];

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, source).expect("fixture file");
}

fn workspace_root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    write(
        dir.path(),
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","workspaces":["apps/*"]}"#,
    );
    dir
}

/// Write one SvelteKit app. `files_lib` sets `kit.files.lib` when present.
fn sveltekit_app(root: &Path, app: &str, with_tsconfig: bool, files_lib: Option<&str>) {
    let lib = files_lib.unwrap_or("src/lib");
    write(
        root,
        &format!("apps/{app}/package.json"),
        &format!(
            r#"{{"name":"{app}","private":true,"type":"module","devDependencies":{{"@sveltejs/kit":"^2.16.0","svelte":"^5.0.0","vite":"^6.2.6"}}}}"#
        ),
    );
    let kit = files_lib.map_or_else(String::new, |dir| format!("files: {{ lib: '{dir}' }}"));
    write(
        root,
        &format!("apps/{app}/svelte.config.js"),
        &format!("export default {{ kit: {{ {kit} }} }};\n"),
    );
    if with_tsconfig {
        write(
            root,
            &format!("apps/{app}/tsconfig.json"),
            &format!(
                r#"{{"compilerOptions":{{"paths":{{"$lib":["./{lib}"],"$lib/*":["./{lib}/*"]}}}}}}"#
            ),
        );
    }
    write(
        root,
        &format!("apps/{app}/{lib}/shared.svelte.ts"),
        &format!("export const {app} = $state({{ app: '{app}' }});\n"),
    );
    write(
        root,
        &format!("apps/{app}/{lib}/util.ts"),
        &format!("export const util = '{app}';\n"),
    );
    write(
        root,
        &format!("apps/{app}/src/routes/+page.svelte"),
        &format!(
            "<script lang=\"ts\">\n\timport {{ util }} from '$lib/util';\n\timport {{ {app} }} from '$lib/shared.svelte';\n</script>\n<p>{{util}} {{{app}.app}}</p>\n"
        ),
    );
}

fn nuxt_app(root: &Path, app: &str) {
    write(
        root,
        &format!("apps/{app}/package.json"),
        &format!(
            r#"{{"name":"{app}","private":true,"type":"module","dependencies":{{"nuxt":"^3.0.0"}}}}"#
        ),
    );
    write(
        root,
        &format!("apps/{app}/nuxt.config.ts"),
        "export default defineNuxtConfig({});\n",
    );
    write(
        root,
        &format!("apps/{app}/app/utils/format.ts"),
        &format!("export const format = '{app}';\n"),
    );
    write(
        root,
        &format!("apps/{app}/app/app.vue"),
        "<script setup lang=\"ts\">\nimport { format } from '~/utils/format';\n</script>\n<template><p>{{ format }}</p></template>\n",
    );
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .expect("project-relative path")
        .to_string_lossy()
        .replace('\\', "/")
}

/// The project-relative targets of each import edge of `from`.
fn imports_of(root: &Path, from: &str) -> Vec<String> {
    let config = create_config(root.to_path_buf());
    let graph = fallow_core::analyze_with_trace(&config)
        .expect("analysis with graph")
        .graph
        .expect("retained graph");
    let source = graph
        .modules
        .iter()
        .find(|module| relative(root, &module.path) == from)
        .unwrap_or_else(|| panic!("importer discovered: {from}"));
    let mut targets: Vec<String> = graph
        .edges_for(source.file_id)
        .into_iter()
        .map(|target| relative(root, &graph.modules[target.0 as usize].path))
        .collect();
    targets.sort();
    targets
}

fn unused_files(root: &Path) -> Vec<String> {
    let config = create_config(root.to_path_buf());
    let mut files: Vec<String> = fallow_core::analyze(&config)
        .expect("analysis")
        .unused_files
        .iter()
        .map(|file| relative(root, &file.file.path))
        .collect();
    files.sort();
    files
}

fn assert_each_app_imports_its_own_lib(root: &Path, lib: &str) {
    for app in APPS {
        assert_eq!(
            imports_of(root, &format!("apps/{app}/src/routes/+page.svelte")),
            vec![
                format!("apps/{app}/{lib}/shared.svelte.ts"),
                format!("apps/{app}/{lib}/util.ts"),
            ],
            "the {app} page must import only from its own $lib"
        );
    }
    assert_eq!(unused_files(root), Vec::<String>::new());
}

#[test]
fn sveltekit_lib_alias_stays_in_its_workspace_with_tsconfig_paths() {
    let dir = workspace_root();
    for app in APPS {
        sveltekit_app(dir.path(), app, true, None);
    }
    assert_each_app_imports_its_own_lib(dir.path(), "src/lib");
}

#[test]
fn sveltekit_lib_alias_stays_in_its_workspace_without_tsconfig_paths() {
    let dir = workspace_root();
    for app in APPS {
        sveltekit_app(dir.path(), app, false, None);
    }
    assert_each_app_imports_its_own_lib(dir.path(), "src/lib");
}

#[test]
fn sveltekit_lib_alias_follows_kit_files_lib() {
    let dir = workspace_root();
    for app in APPS {
        sveltekit_app(dir.path(), app, false, Some("src/library"));
    }
    assert_each_app_imports_its_own_lib(dir.path(), "src/library");
}

#[test]
fn sveltekit_lib_alias_does_not_fall_back_to_another_workspace() {
    let dir = workspace_root();
    for app in APPS {
        sveltekit_app(dir.path(), app, false, None);
    }
    std::fs::remove_file(dir.path().join("apps/alpha/src/lib/shared.svelte.ts"))
        .expect("remove alpha rune module");

    let imports = imports_of(dir.path(), "apps/alpha/src/routes/+page.svelte");
    assert_eq!(imports, vec!["apps/alpha/src/lib/util.ts".to_string()]);
    assert_eq!(unused_files(dir.path()), Vec::<String>::new());
}

#[test]
fn root_alias_still_applies_inside_a_workspace() {
    let dir = workspace_root();
    write(
        dir.path(),
        "package.json",
        r#"{"name":"repro","private":true,"type":"module","workspaces":["apps/*"],"devDependencies":{"vite":"^6.2.6"}}"#,
    );
    write(
        dir.path(),
        "vite.config.ts",
        "import { fileURLToPath } from 'node:url';\nexport default { resolve: { alias: { '@shared': fileURLToPath(new URL('./shared', import.meta.url)) } } };\n",
    );
    write(dir.path(), "shared/tokens.ts", "export const tokens = 1;\n");
    for app in APPS {
        sveltekit_app(dir.path(), app, false, None);
        write(
            dir.path(),
            &format!("apps/{app}/src/lib/util.ts"),
            "import { tokens } from '@shared/tokens';\nexport const util = tokens;\n",
        );
    }
    for app in APPS {
        assert_eq!(
            imports_of(dir.path(), &format!("apps/{app}/src/lib/util.ts")),
            vec!["shared/tokens.ts".to_string()],
            "a root alias must still apply to the files of {app}"
        );
    }
}

#[test]
fn nuxt_source_alias_stays_in_its_workspace() {
    let dir = workspace_root();
    for app in APPS {
        nuxt_app(dir.path(), app);
    }
    for app in APPS {
        assert_eq!(
            imports_of(dir.path(), &format!("apps/{app}/app/app.vue")),
            vec![format!("apps/{app}/app/utils/format.ts")],
            "the {app} app must import only from its own source directory"
        );
    }
}
