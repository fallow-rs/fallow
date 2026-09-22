//! Regression tests for issue #2698: Module Federation `exposes` and `remotes`
//! were invisible to analysis.
//!
//! An `exposes` target is only ever loaded by a remote container at runtime, so
//! without reading the Federation config the exposed module and everything it
//! reaches looked like dead code. A `remotes` alias is supplied by the remote
//! container rather than by npm, but an unresolvable bare specifier is
//! classified as an npm package, so `import('checkout/Button')` surfaced as an
//! unlisted dependency named `checkout`.

use std::path::Path;

use super::common::{create_config, create_production_config, fixture_path};

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn unused_file_paths(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect()
}

fn unused_exports(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            format!(
                "{}:{}",
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name
            )
        })
        .collect()
}

fn unlisted_packages(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect()
}

fn unresolved_specifiers(results: &fallow_types::results::AnalysisResults) -> Vec<&str> {
    results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect()
}

fn contains_suffix(paths: &[String], suffix: &str) -> bool {
    paths.iter().any(|path| path.ends_with(suffix))
}

/// The producer's `module-federation.config.ts` declares a file target and an
/// extensionless directory target. Both must become entry points, while a file
/// the config does not name stays unused.
#[test]
fn exposed_targets_are_entry_points_and_unexposed_files_stay_unused() {
    let config = create_config(fixture_path("module-federation-producer"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results);

    for exposed in [
        "src/components/Button.tsx",
        "src/components/label.ts",
        "src/cart/index.ts",
    ] {
        assert!(
            !contains_suffix(&unused, exposed),
            "{exposed} is reachable from an exposes target, got {unused:?}"
        );
    }
    assert!(
        contains_suffix(&unused, "src/orphan.ts"),
        "a file no exposes target names must stay unused, got {unused:?}"
    );
}

/// Exposed files become entry points, so their exports follow
/// `includeEntryExports` like any other entry point instead of being
/// unconditionally exempt.
#[test]
fn exposed_entry_exports_follow_the_entry_export_setting() {
    let root = fixture_path("module-federation-producer");
    let exposed_default = "src/components/Button.tsx:default";

    let default_results =
        fallow_core::analyze(&create_config(root.clone())).expect("analysis should succeed");
    let reported = unused_exports(&default_results);
    assert!(
        !contains_suffix(&reported, exposed_default),
        "the exposed default export is not reported at default settings, got {reported:?}"
    );

    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let reported = unused_exports(&results);
    assert!(
        contains_suffix(&reported, exposed_default),
        "with include_entry_exports the exposed default export is reportable, got {reported:?}"
    );
}

#[test]
fn exposed_targets_survive_production_discovery() {
    let config = create_production_config(fixture_path("module-federation-producer"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results);

    assert!(
        !contains_suffix(&unused, "src/components/Button.tsx"),
        "the exposed component stays an entry point in production mode, got {unused:?}"
    );
}

/// A declared remote alias and its subpaths are provided by the remote
/// container. Every other bare specifier the project does not declare must keep
/// reporting.
#[test]
fn declared_remote_alias_is_not_an_unlisted_dependency() {
    let config = create_config(fixture_path("module-federation-consumer"));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unlisted = unlisted_packages(&results);

    assert!(
        !unlisted.contains(&"checkout"),
        "the declared remote alias is provided by the container, got {unlisted:?}"
    );
    assert!(
        unlisted.contains(&"checkout-ui"),
        "a sibling package name must not be covered by the alias, got {unlisted:?}"
    );
    assert!(
        unlisted.contains(&"genuinely-missing-pkg"),
        "an undeclared npm package must keep reporting, got {unlisted:?}"
    );

    let unresolved = unresolved_specifiers(&results);
    assert!(
        !unresolved.contains(&"checkout/Button"),
        "a remote subpath import must not surface as unresolved, got {unresolved:?}"
    );
}

/// An installed package with the same name as the alias still wins: the
/// provider rule only silences unlisted-dependency findings, so a declared
/// dependency keeps its usage credit.
#[test]
fn installed_package_with_the_alias_name_is_neither_unlisted_nor_unused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "mf-host",
            "private": true,
            "dependencies": { "checkout": "^1.0.0" },
            "devDependencies": { "@module-federation/enhanced": "^0.9.0", "webpack": "^5.98.0" }
        }"#,
    );
    write(
        &root.join("module-federation.config.ts"),
        r#"import { createModuleFederationConfig } from "@module-federation/enhanced";

           export default createModuleFederationConfig({
             name: "host",
             remotes: { checkout: "checkout@https://example.test/remoteEntry.js" },
           });"#,
    );
    write(
        &root.join("src/index.ts"),
        r#"import "checkout";
           export const mount = async (): Promise<unknown> => import("checkout/Button");"#,
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !unlisted_packages(&results).contains(&"checkout"),
        "a declared dependency is never unlisted, got {:?}",
        unlisted_packages(&results)
    );
    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"checkout"),
        "the import still credits the installed package, got {unused_deps:?}"
    );
}

/// The config file the plugin read is never itself dead code, including in a
/// workspace where the Federation dependency is hoisted to the root and each
/// package keeps its own config.
#[test]
fn a_nested_config_file_is_not_reported_as_unused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "mf-workspace",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": { "@module-federation/enhanced": "^0.9.0" }
        }"#,
    );
    write(
        &root.join("packages/host/package.json"),
        r#"{ "name": "@mf/host", "private": true }"#,
    );
    write(
        &root.join("packages/host/module-federation.config.ts"),
        r#"export default {
             name: "host",
             exposes: { "./Panel": "./src/Panel.tsx" },
           };"#,
    );
    write(
        &root.join("packages/host/src/Panel.tsx"),
        "export default (): string => \"panel\";",
    );
    write(
        &root.join("packages/host/src/orphan.ts"),
        "export const x = 1;",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results);

    assert!(
        !contains_suffix(&unused, "packages/host/module-federation.config.ts"),
        "the config the plugin read is used, got {unused:?}"
    );
    assert!(
        !contains_suffix(&unused, "packages/host/src/Panel.tsx"),
        "the exposed file is an entry point at this depth, got {unused:?}"
    );
    assert!(
        contains_suffix(&unused, "packages/host/src/orphan.ts"),
        "an unexposed file still reports, got {unused:?}"
    );
}

/// One config shape that holds Federation options, with both halves the issue
/// asks about: a producer that exposes one file beside an unexposed sibling, and
/// a consumer that imports the remote alias beside an undeclared package.
struct Shape {
    /// What the shape is, for the assertion message.
    name: &'static str,
    /// The config file, relative to the project root.
    config_file: &'static str,
    /// The config file contents.
    config: String,
    /// The dependencies the reading plugin needs to activate, as JSON members.
    dev_dependencies: &'static str,
}

/// The `remotes` mapping every shape declares, so one alias covers the consumer
/// half of each project.
const REMOTES: &str = r#"remotes: { checkout: "checkout@https://example.test/remoteEntry.js" }"#;

/// Build the project of one shape and analyze it.
fn analyze_shape(shape: &Shape) -> fallow_types::results::AnalysisResults {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        &format!(
            r#"{{ "name": "mf-shape", "private": true, "devDependencies": {{ {} }} }}"#,
            shape.dev_dependencies
        ),
    );
    write(&root.join(shape.config_file), &shape.config);
    write(
        &root.join("src/Panel.tsx"),
        r#"import "checkout/Button";
           import "genuinely-missing-pkg";
           export default (): string => "panel";"#,
    );
    write(&root.join("src/orphan.ts"), "export const x = 1;");

    let config = create_config(root.to_path_buf());
    fallow_core::analyze(&config).expect("analysis should succeed")
}

/// Assert both halves for one shape: the exposed target is an entry point and
/// its sibling is not, the remote alias is provided and an undeclared package is
/// not.
fn assert_shape_is_read(shape: &Shape) {
    let results = analyze_shape(shape);
    let unused = unused_file_paths(&results);
    assert!(
        !contains_suffix(&unused, "src/Panel.tsx"),
        "{}: the exposed target is an entry point, got {unused:?}",
        shape.name
    );
    assert!(
        contains_suffix(&unused, "src/orphan.ts"),
        "{}: an unexposed sibling still reports, got {unused:?}",
        shape.name
    );

    let unlisted = unlisted_packages(&results);
    assert!(
        !unlisted.contains(&"checkout"),
        "{}: the remote alias is provided by the container, got {unlisted:?}",
        shape.name
    );
    assert!(
        unlisted.contains(&"genuinely-missing-pkg"),
        "{}: an undeclared package still reports, got {unlisted:?}",
        shape.name
    );

    let unresolved = unresolved_specifiers(&results);
    assert!(
        !unresolved.contains(&"checkout/Button"),
        "{}: a remote subpath import is not unresolved, got {unresolved:?}",
        shape.name
    );
}

/// A bundler uses a string element of the `exposes` array both as the public
/// name and as the module request, so the element names a local file.
#[test]
fn exposes_array_form_targets_are_entry_points() {
    assert_shape_is_read(&Shape {
        name: "array exposes",
        config_file: "module-federation.config.ts",
        config: format!(
            r#"export default {{
                 name: "host",
                 exposes: ["./src/Panel.tsx"],
                 {REMOTES},
               }};"#
        ),
        dev_dependencies: r#""@module-federation/enhanced": "^0.9.0""#,
    });
}

/// Vite flattens a nested plugin array, so a Federation call one level down is
/// part of the same build.
#[test]
fn nested_plugin_array_federation_is_read() {
    assert_shape_is_read(&Shape {
        name: "nested plugin array",
        config_file: "vite.config.ts",
        config: format!(
            r#"import {{ federation }} from "@module-federation/vite";

               export default defineConfig({{
                 plugins: [
                   [
                     react(),
                     federation({{
                       name: "host",
                       exposes: {{ "./Panel": "./src/Panel.tsx" }},
                       {REMOTES},
                     }}),
                   ],
                   other(),
                 ],
               }});"#
        ),
        dev_dependencies: r#""vite": "^6.0.0", "@module-federation/vite": "^1.0.0""#,
    });
}

/// A plugin list held by a variable is the shape a config takes as soon as it
/// builds the list conditionally.
#[test]
fn plugins_identifier_federation_is_read() {
    assert_shape_is_read(&Shape {
        name: "plugins identifier",
        config_file: "webpack.config.js",
        config: format!(
            r#"const {{
                 ModuleFederationPlugin,
               }} = require("@module-federation/enhanced/webpack");

               const plugins = [
                 new ModuleFederationPlugin({{
                   name: "host",
                   exposes: {{ "./Panel": "./src/Panel.tsx" }},
                   {REMOTES},
                 }}),
               ];

               module.exports = {{ plugins }};"#
        ),
        dev_dependencies: r#""webpack": "^5.98.0", "@module-federation/enhanced": "^0.9.0""#,
    });
}

/// An rsbuild config holds its rspack plugin list under `tools.rspack`.
#[test]
fn tool_key_plugin_array_federation_is_read() {
    assert_shape_is_read(&Shape {
        name: "tools.rspack.plugins",
        config_file: "rsbuild.config.ts",
        config: format!(
            r#"import {{ ModuleFederationPlugin }} from "@module-federation/enhanced/rspack";

               export default {{
                 tools: {{
                   rspack: {{
                     plugins: [
                       new ModuleFederationPlugin({{
                         name: "host",
                         exposes: {{ "./Panel": "./src/Panel.tsx" }},
                         {REMOTES},
                       }}),
                     ],
                   }},
                 }},
               }};"#
        ),
        dev_dependencies: r#""@rsbuild/core": "^1.0.0", "@module-federation/enhanced": "^0.9.0""#,
    });
}

/// `@module-federation/nextjs-mf` registers the plugin inside the
/// `webpack(config)` hook of `next.config.*`.
#[test]
fn next_config_webpack_hook_exposes_are_entry_points() {
    assert_shape_is_read(&Shape {
        name: "next.config webpack hook",
        config_file: "next.config.js",
        config: format!(
            r#"const NextFederationPlugin = require("@module-federation/nextjs-mf");

               module.exports = {{
                 webpack(config, options) {{
                   config.plugins.push(
                     new NextFederationPlugin({{
                       name: "shop",
                       filename: "static/chunks/remoteEntry.js",
                       exposes: {{ "./Panel": "./src/Panel.tsx" }},
                       {REMOTES},
                     }}),
                   );
                   return config;
                 }},
               }};"#
        ),
        dev_dependencies: r#""next": "^15.0.0", "@module-federation/nextjs-mf": "^8.0.0""#,
    });
}

/// Options held by a `const` above the plugin list are the most common real
/// shape.
#[test]
fn options_bound_to_a_local_const_are_read() {
    assert_shape_is_read(&Shape {
        name: "const options",
        config_file: "webpack.config.js",
        config: format!(
            r#"const {{
                 ModuleFederationPlugin,
               }} = require("@module-federation/enhanced/webpack");

               const mfConfig = {{
                 name: "host",
                 exposes: {{ "./Panel": "./src/Panel.tsx" }},
                 {REMOTES},
               }};

               module.exports = {{ plugins: [new ModuleFederationPlugin(mfConfig)] }};"#
        ),
        dev_dependencies: r#""webpack": "^5.98.0", "@module-federation/enhanced": "^0.9.0""#,
    });
}

/// A bundler derives the request scope of a `remotes` array element from the
/// whole container location, which is never a bare specifier a provider rule can
/// cover, so the array form of `remotes` stays unread and the import keeps
/// reporting.
#[test]
fn remotes_array_form_still_reports_the_unlisted_alias() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "mf-consumer",
            "private": true,
            "devDependencies": { "@module-federation/enhanced": "^0.9.0" }
        }"#,
    );
    write(
        &root.join("module-federation.config.ts"),
        r#"export default {
             name: "host",
             exposes: { "./Panel": "./src/Panel.tsx" },
             remotes: ["checkout@https://example.test/remoteEntry.js"],
           };"#,
    );
    write(
        &root.join("src/Panel.tsx"),
        r#"import "checkout/Button";
           export default (): string => "panel";"#,
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        unlisted_packages(&results).contains(&"checkout"),
        "an alias fallow did not read cannot be provided, got {:?}",
        unlisted_packages(&results)
    );
    assert!(
        !contains_suffix(&unused_file_paths(&results), "src/Panel.tsx"),
        "the readable `exposes` beside it is still read, got {:?}",
        unused_file_paths(&results)
    );
}

/// A widened search must keep the shape gate. A project that does not use
/// Module Federation registers nothing, whatever a call in its config is named.
#[test]
fn a_call_named_like_a_federation_plugin_registers_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "no-federation",
            "private": true,
            "devDependencies": { "vite": "^6.0.0" }
        }"#,
    );
    write(
        &root.join("vite.config.ts"),
        r#"export default defineConfig({
             plugins: [federation("graphql-schema", { batch: true })],
           });"#,
    );
    write(&root.join("src/main.ts"), r#"import "checkout/Button";"#);
    write(&root.join("src/orphan.ts"), "export const x = 1;");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        unlisted_packages(&results).contains(&"checkout"),
        "no remote was declared, so the import reports, got {:?}",
        unlisted_packages(&results)
    );
    assert!(
        contains_suffix(&unused_file_paths(&results), "src/orphan.ts"),
        "no entry point was registered, got {:?}",
        unused_file_paths(&results)
    );
}

/// Two packages each read their own `next.config.js`. The Next.js reader and the
/// Federation reader run over one file, and neither package takes the other's
/// declarations.
#[test]
fn a_workspace_reads_each_next_config_on_its_own() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "mf-workspace",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": {
                "next": "^15.0.0",
                "@module-federation/nextjs-mf": "^8.0.0"
            }
        }"#,
    );
    for (package, exposed) in [("shop", "Panel"), ("admin", "Widget")] {
        write(
            &root.join(format!("packages/{package}/package.json")),
            &format!(r#"{{ "name": "@mf/{package}", "private": true }}"#),
        );
        write(
            &root.join(format!("packages/{package}/next.config.js")),
            &format!(
                r#"const NextFederationPlugin = require("@module-federation/nextjs-mf");

                   module.exports = {{
                     pageExtensions: ["tsx"],
                     webpack(config) {{
                       config.plugins.push(
                         new NextFederationPlugin({{
                           name: "{package}",
                           exposes: {{ "./{exposed}": "./src/{exposed}.tsx" }},
                         }}),
                       );
                       return config;
                     }},
                   }};"#
            ),
        );
        write(
            &root.join(format!("packages/{package}/src/{exposed}.tsx")),
            &format!("export default (): string => \"{package}\";"),
        );
        write(
            &root.join(format!("packages/{package}/src/orphan.ts")),
            "export const x = 1;",
        );
    }

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&results);

    for exposed in [
        "packages/shop/src/Panel.tsx",
        "packages/admin/src/Widget.tsx",
    ] {
        assert!(
            !contains_suffix(&unused, exposed),
            "{exposed} is an entry point of its own package, got {unused:?}"
        );
    }
    for orphan in [
        "packages/shop/src/orphan.ts",
        "packages/admin/src/orphan.ts",
    ] {
        assert!(
            contains_suffix(&unused, orphan),
            "{orphan} is exposed by neither config, got {unused:?}"
        );
    }
}

/// The provider rule covers only the directory tree that declared the alias, so
/// a sibling package importing the same specifier without declaring the remote
/// keeps reporting.
#[test]
fn remote_alias_does_not_leak_to_a_sibling_workspace_package() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        &root.join("package.json"),
        r#"{
            "name": "mf-workspace",
            "private": true,
            "workspaces": ["packages/*"],
            "devDependencies": { "@module-federation/enhanced": "^0.9.0" }
        }"#,
    );
    write(
        &root.join("packages/host/package.json"),
        r#"{
            "name": "@mf/host",
            "private": true,
            "devDependencies": { "@module-federation/enhanced": "^0.9.0" }
        }"#,
    );
    write(
        &root.join("packages/host/module-federation.config.ts"),
        r#"import { createModuleFederationConfig } from "@module-federation/enhanced";

           export default createModuleFederationConfig({
             name: "host",
             remotes: { checkout: "checkout@https://example.test/remoteEntry.js" },
           });"#,
    );
    write(
        &root.join("packages/host/src/index.ts"),
        r#"import "checkout/Button";
           export const mount = (): string => "host";"#,
    );
    write(
        &root.join("packages/other/package.json"),
        r#"{ "name": "@mf/other", "private": true }"#,
    );
    write(
        &root.join("packages/other/src/index.ts"),
        r#"import "checkout/Button";
           export const mount = (): string => "other";"#,
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let sites: Vec<String> = results
        .unlisted_dependencies
        .iter()
        .filter(|finding| finding.dep.package_name == "checkout")
        .flat_map(|finding| &finding.dep.imported_from)
        .map(|site| site.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        sites.iter().any(|path| path.contains("packages/other/")),
        "the package that declared no remote still reports, got {sites:?}"
    );
    assert!(
        !sites.iter().any(|path| path.contains("packages/host/")),
        "the declaring package is covered by its own remote, got {sites:?}"
    );
}
