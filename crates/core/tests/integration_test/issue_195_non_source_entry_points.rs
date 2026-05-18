//! Issue #195: false positives for files referenced from non-source artifacts.
//!
//! Each test fixture exercises one of the five reported cases:
//! A. Vite `css.preprocessorOptions.scss.additionalData`
//! B. SFC `<style lang="scss">` body imports + `<style src>` references
//! C. package.json `scripts` positional file arguments (root + workspace)
//! D. CI YAML positional file arguments (`.gitlab-ci.yml`, GitHub Actions)
//! E. Cypress `e2e.specPattern` / `component.specPattern` / `supportFile`

use super::common::{create_config, fixture_path};

fn unused_file_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

// ── Case A: Vite additionalData ──────────────────────────────────────

#[test]
fn vite_additional_data_seeds_scss_entries() {
    let root = fixture_path("issue-195-vite-additional-data");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"global.scss".to_string()),
        "global.scss is referenced from vite additionalData, should not be unused. Got: {names:?}"
    );
    assert!(
        !names.contains(&"_tokens.scss".to_string()),
        "_tokens.scss is reachable via global.scss @use, should not be unused. Got: {names:?}"
    );
}

#[test]
fn vite_additional_data_marks_scss_package_imports_used() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-195-vite-additional-data-package",
  "version": "0.0.0",
  "private": true,
  "dependencies": {
    "bootstrap": "5.0.0",
    "vite": "5.0.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write main");
    std::fs::write(
        root.join("vite.config.ts"),
        r#"import { defineConfig } from "vite";

export default defineConfig({
  css: {
    preprocessorOptions: {
      scss: { additionalData: `@use "bootstrap/scss/functions";` },
    },
  },
});"#,
    )
    .expect("write vite config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unused_dependencies
            .iter()
            .any(|dep| dep.dep.package_name == "bootstrap"),
        "bootstrap is referenced from vite additionalData and should be marked used"
    );
}

#[test]
fn vite_additional_data_marks_bare_scss_package_imports_used() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-195-vite-additional-data-bare-package",
  "version": "0.0.0",
  "private": true,
  "dependencies": {
    "bootstrap": "5.0.0",
    "vite": "5.0.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write main");
    std::fs::write(
        root.join("vite.config.ts"),
        r#"import { defineConfig } from "vite";

export default defineConfig({
  css: {
    preprocessorOptions: {
      scss: { additionalData: `@use "bootstrap";` },
    },
  },
});"#,
    )
    .expect("write vite config");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unused_dependencies
            .iter()
            .any(|dep| dep.dep.package_name == "bootstrap"),
        "bare bootstrap import from vite additionalData should be marked used"
    );
}

// ── Case B: SFC <style> blocks ───────────────────────────────────────

#[test]
fn sfc_style_block_scss_import_seeds_partial() {
    let root = fixture_path("issue-195-sfc-style-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"Foo.scss".to_string()),
        "Foo.scss is referenced from Foo.vue <style lang=\"scss\">@import 'Foo'</style> via SCSS \
         partial fallback (sibling file). Got: {names:?}"
    );
}

#[test]
fn sfc_style_src_attribute_seeds_external_file() {
    let root = fixture_path("issue-195-sfc-style-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"Bar.scss".to_string()),
        "Bar.scss is referenced from Bar.vue <style src=\"./Bar.scss\">. Got: {names:?}"
    );
}

#[test]
fn sfc_style_block_scss_import_marks_node_modules_package_used() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::create_dir_all(root.join("node_modules/bootstrap/scss"))
        .expect("create bootstrap scss dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-195-sfc-style-node-modules",
  "version": "0.0.0",
  "private": true,
  "dependencies": {
    "bootstrap": "5.0.0",
    "vite": "5.0.0",
    "vue": "3.0.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/main.ts"),
        r#"import App from "./App.vue";
export const app = App;"#,
    )
    .expect("write main");
    std::fs::write(
        root.join("src/App.vue"),
        r#"<template><div /></template>
<script setup lang="ts">
export const name = "App";
</script>
<style lang="scss">
@use "bootstrap/scss/functions";
</style>"#,
    )
    .expect("write vue file");
    std::fs::write(
        root.join("node_modules/bootstrap/scss/_functions.scss"),
        "$primary: red;",
    )
    .expect("write bootstrap partial");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unused_dependencies
            .iter()
            .any(|dep| dep.dep.package_name == "bootstrap"),
        "bootstrap is referenced from App.vue <style lang=\"scss\"> and should be marked used"
    );
    assert!(
        !results
            .unresolved_imports
            .iter()
            .any(|import| import.import.specifier.contains("bootstrap")),
        "bootstrap Sass import should resolve through node_modules, got unresolved: {:?}",
        results.unresolved_imports
    );
}

// ── Case C: package.json scripts positional args ─────────────────────

#[test]
fn package_json_script_entry_files_seed_root_level_paths() {
    let root = fixture_path("issue-195-script-entry-files");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"deploy.ts".to_string()),
        "scripts/deploy.ts is referenced from package.json `deploy` script (`tsc \
         ./scripts/deploy.ts`). Got: {names:?}"
    );
}

#[test]
fn workspace_package_script_entry_files_use_workspace_prefix() {
    let root = fixture_path("issue-195-script-entry-files-workspace");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    let deploy_unused = paths
        .iter()
        .any(|p| p.ends_with("apps/api/scripts/deploy.ts"));
    assert!(
        !deploy_unused,
        "apps/api/scripts/deploy.ts is referenced from apps/api/package.json `deploy` script and \
         must use the workspace prefix when joined into the entry pattern. Got unused: {paths:?}"
    );
}

// Note: production-mode behaviour for `scripts/deploy.ts` is shaped by the
// existing `discover_entry_points_with_warnings_impl` path
// (`crates/core/src/discover/entry_points.rs`) which seeds file refs from ALL
// scripts regardless of production mode. The Case C addition in
// `analyze_all_scripts` only fires in non-production mode (`filter_production_scripts`
// strips non-prod scripts first), but cannot make production mode strictly
// conservative on its own because the discovery layer also seeds. Strictly
// production-conservative script seeding is out of scope for this fix.

// ── Case D: CI YAML positional args ──────────────────────────────────

#[test]
fn ci_yaml_file_args_seed_entry_points() {
    let root = fixture_path("issue-195-ci-file-args");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"gitlab-deploy.ts".to_string()),
        "scripts/gitlab-deploy.ts is referenced from .gitlab-ci.yml. Got: {names:?}"
    );
    assert!(
        !names.contains(&"gh-deploy.ts".to_string()),
        "scripts/gh-deploy.ts is referenced from .github/workflows/deploy.yml. Got: {names:?}"
    );
}

// ── Case E: Cypress specPattern + supportFile ────────────────────────

#[test]
fn cypress_spec_pattern_outside_default_dir_seeds_entry() {
    let root = fixture_path("issue-195-cypress-spec-pattern");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names = unused_file_names(&results);
    assert!(
        !names.contains(&"login.cy.ts".to_string()),
        "tests/integration/login.cy.ts is referenced from cypress.config.ts e2e.specPattern \
         (outside the default cypress/** location). Got: {names:?}"
    );
    assert!(
        !names.contains(&"Foo.cy.ts".to_string()),
        "src/components/Foo.cy.ts is referenced from cypress.config.ts component.specPattern. \
         Got: {names:?}"
    );
}

#[test]
fn cypress_support_file_string_seeds_entry() {
    let root = fixture_path("issue-195-cypress-spec-pattern");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let paths = unused_file_paths(&results);
    let support_unused = paths.iter().any(|p| p.ends_with("tests/support/index.ts"));
    assert!(
        !support_unused,
        "tests/support/index.ts is referenced from cypress.config.ts e2e.supportFile. \
         Got unused: {paths:?}"
    );
}

#[test]
fn cypress_default_component_spec_pattern_seeds_entry() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src/components")).expect("create component dir");
    std::fs::create_dir_all(root.join("tests")).expect("create tests dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "issue-195-cypress-default-component-spec",
  "version": "0.0.0",
  "private": true,
  "dependencies": {
    "vue": "3.0.0"
  },
  "devDependencies": {
    "cypress": "13.0.0",
    "vite": "5.0.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(root.join("src/main.ts"), "export const main = 1;").expect("write main");
    std::fs::write(
        root.join("cypress.config.ts"),
        r#"import { defineConfig } from "cypress";

export default defineConfig({
  component: {
    devServer: { framework: "vue", bundler: "vite" },
  },
});"#,
    )
    .expect("write cypress config");
    std::fs::write(
        root.join("src/components/Foo.vue"),
        r#"<template><div>Foo</div></template>
<script setup lang="ts">
export const foo = true;
</script>"#,
    )
    .expect("write component");
    std::fs::write(
        root.join("tests/Foo.cy.ts"),
        r#"import Foo from "../src/components/Foo.vue";

describe("Foo", () => {
  it("mounts", () => {
    cy.mount(Foo);
  });
});"#,
    )
    .expect("write cypress spec");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let paths = unused_file_paths(&results);

    assert!(
        !paths.iter().any(|p| p.ends_with("tests/Foo.cy.ts")),
        "Cypress's default component specPattern should seed tests/Foo.cy.ts. Got unused: {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.ends_with("src/components/Foo.vue")),
        "component imported by the default Cypress spec should be reachable. Got unused: {paths:?}"
    );
}
