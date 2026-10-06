use std::fs;

use fallow_config::{FallowConfig, OutputFormat, RulesConfig};

use super::common::{create_config, fixture_path};

/// Issue #2226: a user-written literal import of an `X/__mocks__` package
/// carries no runner semantics, so it is reported as an unlisted dependency
/// under Vitest, exactly as under Jest.
#[test]
fn vitest_literal_mocks_import_flagged_as_unlisted_dep() {
    let root = fixture_path("vitest-mocks-virtual");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unlisted_names.contains(&"@aws-sdk/__mocks__"),
        "literal @aws-sdk/__mocks__ import should be flagged as an unlisted dependency, got: {unlisted_names:?}"
    );
}

/// Jest side of the issue #2226 parity matrix: the same literal import is
/// flagged in a Jest project too.
#[test]
fn jest_literal_mocks_import_flagged_as_unlisted_dep() {
    let root = fixture_path("issue-2226-jest-mocks-literal");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unlisted_names.contains(&"@aws-sdk/__mocks__"),
        "literal @aws-sdk/__mocks__ import should be flagged as an unlisted dependency, got: {unlisted_names:?}"
    );
}

/// Issue #2226: literal `X/__mocks__` imports are flagged in workspace
/// monorepos too; the removed suffix suppression no longer masks them.
#[test]
fn vitest_literal_mocks_imports_flagged_in_workspace_monorepo() {
    let root = fixture_path("vitest-mocks-workspace");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    for specifier in &[
        "@aws-sdk/__mocks__",
        "@supabase/__mocks__",
        "@sentry/__mocks__",
    ] {
        assert!(
            unlisted_names.contains(specifier),
            "{specifier} should be flagged as an unlisted dependency in workspace monorepo, got: {unlisted_names:?}"
        );
    }
}

#[test]
fn unlisted_dependencies_detected() {
    let root = fixture_path("unlisted-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unlisted_names.contains(&"some-pkg"),
        "some-pkg should be detected as unlisted dependency, found: {unlisted_names:?}"
    );
}

#[test]
fn unlisted_re_export_dependency_reports_re_export_line() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "unlisted-re-export",
  "main": "src/index.ts"
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const local = 1;\nexport { default as pad } from 'left-pad';\n",
    )
    .expect("write source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let finding = results
        .unlisted_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "left-pad")
        .expect("left-pad re-export should be reported as unlisted");

    assert_eq!(finding.dep.imported_from.len(), 1);
    assert_eq!(finding.dep.imported_from[0].line, 2);
}

#[test]
fn unresolved_imports_detected() {
    let root = fixture_path("unresolved-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert!(
        unresolved_specifiers.contains(&"./nonexistent"),
        "\"./nonexistent\" should be detected as unresolved import, found: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"./missing-re-export"),
        "named re-export source should be detected as unresolved import, found: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"./missing-star-re-export"),
        "star re-export source should be detected as unresolved import, found: {unresolved_specifiers:?}"
    );
}

#[test]
fn ignore_unresolved_imports_config_suppresses_matching_specifiers() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("create src dir");
    fs::create_dir_all(root.join("node_modules/@example/icons")).expect("create package dir");
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "ignore-unresolved-imports-config",
  "main": "src/index.ts",
  "dependencies": {
    "@example/icons": "1.0.0"
  }
}"#,
    )
    .expect("write package.json");
    fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "ignoreUnresolvedImports": [
    "@example/icons",
    "@example/icons/**",
    "../generated/**"
  ]
}"#,
    )
    .expect("write fallow config");
    fs::write(
        root.join("node_modules/@example/icons/package.json"),
        r#"{
  "name": "@example/icons",
  "version": "1.0.0",
  "exports": {
    ".": "./dist/index.js",
    "./metadata": "./dist/metadata.js"
  }
}"#,
    )
    .expect("write package manifest");
    fs::write(
        root.join("src/index.ts"),
        r#"import { Icon } from "@example/icons";
import { metadata } from "@example/icons/metadata";
import { generated } from "../generated/client";
import { local } from "./still-missing";

export const main = () => [Icon, metadata, generated, local];
"#,
    )
    .expect("write source");

    let (loaded, _) = FallowConfig::find_and_load(root)
        .expect("config discovery should succeed")
        .expect("fixture config should be discovered");
    let config = loaded.resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();

    assert_eq!(
        unresolved_specifiers,
        vec!["./still-missing"],
        "config-loaded ignoreUnresolvedImports should suppress bare package, package subpath, and parent-relative generated specifiers"
    );
}

#[test]
fn unused_dev_dependency_detected() {
    let root = fixture_path("unused-dev-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unused_dev_dep_names.contains(&"my-custom-dev-tool"),
        "my-custom-dev-tool should be detected as unused dev dependency, found: {unused_dev_dep_names:?}"
    );
}

#[test]
fn unused_optional_dependency_detected() {
    let root = fixture_path("optional-deps");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_optional_dep_names: Vec<&str> = results
        .unused_optional_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unused_optional_dep_names.contains(&"unused-optional-pkg"),
        "unused-optional-pkg should be detected as unused optional dependency, found: {unused_optional_dep_names:?}"
    );
}

#[test]
fn napi_rs_optional_prebuild_dependencies_are_not_reported() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("create src dir");
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "@srcmap/codec",
  "main": "src/index.ts",
  "optionalDependencies": {
    "@srcmap/codec-darwin-arm64": "1.0.0",
    "@srcmap/codec-linux-x64-gnu": "1.0.0",
    "unused-optional-pkg": "1.0.0"
  },
  "napi": {
    "binaryName": "srcmap-codec",
    "targets": [
      "aarch64-apple-darwin",
      "x86_64-unknown-linux-gnu"
    ]
  }
}"#,
    )
    .expect("write package.json");
    fs::write(root.join("src/index.ts"), "export const value = 1;\n").expect("write source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_optional_dep_names: Vec<&str> = results
        .unused_optional_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_optional_dep_names.contains(&"@srcmap/codec-darwin-arm64"),
        "generated napi-rs optional package should be credited, found: {unused_optional_dep_names:?}"
    );
    assert!(
        !unused_optional_dep_names.contains(&"@srcmap/codec-linux-x64-gnu"),
        "generated napi-rs optional package should be credited, found: {unused_optional_dep_names:?}"
    );
    assert!(
        unused_optional_dep_names.contains(&"unused-optional-pkg"),
        "unrelated optional package should still be reported, found: {unused_optional_dep_names:?}"
    );
}

#[test]
fn napi_rs_optional_prebuild_dependencies_are_not_reported_in_workspaces() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    let package_root = root.join("packages/native");
    fs::create_dir_all(package_root.join("src")).expect("create workspace package");
    fs::write(
        root.join("package.json"),
        r#"{
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .expect("write root package.json");
    fs::write(
        package_root.join("package.json"),
        r#"{
  "name": "@oxc-coverage-instrument/binding",
  "main": "src/index.ts",
  "optionalDependencies": {
    "@oxc-coverage-instrument/binding-win32-arm64-msvc": "1.0.0",
    "@oxc-coverage-instrument/binding-wasm32-wasi": "1.0.0",
    "@oxc-coverage-instrument/binding-wasm32-wasi-singlethreaded": "1.0.0",
    "unused-optional-pkg": "1.0.0"
  },
  "napi": {
    "packageName": "@oxc-coverage-instrument/binding",
    "binaryName": "coverage-instrument",
    "targets": [
      "aarch64-pc-windows-msvc",
      "wasm32-wasip1",
      "wasm32-wasip1-threads"
    ]
  }
}"#,
    )
    .expect("write workspace package.json");
    fs::write(
        package_root.join("src/index.ts"),
        "export const value = 1;\n",
    )
    .expect("write source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_optional_dep_names: Vec<&str> = results
        .unused_optional_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    for generated in [
        "@oxc-coverage-instrument/binding-win32-arm64-msvc",
        "@oxc-coverage-instrument/binding-wasm32-wasi",
        "@oxc-coverage-instrument/binding-wasm32-wasi-singlethreaded",
    ] {
        assert!(
            !unused_optional_dep_names.contains(&generated),
            "generated napi-rs optional package should be credited in a workspace, found: {unused_optional_dep_names:?}"
        );
    }
    assert!(
        unused_optional_dep_names.contains(&"unused-optional-pkg"),
        "unrelated workspace optional package should still be reported, found: {unused_optional_dep_names:?}"
    );
}

#[test]
fn napi_rs_optional_prebuild_credits_are_workspace_scoped() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    let native_root = root.join("packages/native");
    let other_root = root.join("packages/other");
    fs::create_dir_all(native_root.join("src")).expect("create native workspace package");
    fs::create_dir_all(other_root.join("src")).expect("create other workspace package");
    fs::write(
        root.join("package.json"),
        r#"{
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .expect("write root package.json");
    fs::write(
        native_root.join("package.json"),
        r#"{
  "name": "native",
  "main": "src/index.ts",
  "optionalDependencies": {
    "native-linux-x64-gnu": "1.0.0"
  },
  "napi": {
    "targets": ["x86_64-unknown-linux-gnu"]
  }
}"#,
    )
    .expect("write native package.json");
    fs::write(
        native_root.join("src/index.ts"),
        "export const value = 1;\n",
    )
    .expect("write native source");
    fs::write(
        other_root.join("package.json"),
        r#"{
  "name": "other",
  "main": "src/index.ts",
  "optionalDependencies": {
    "native-linux-x64-gnu": "1.0.0"
  }
}"#,
    )
    .expect("write other package.json");
    fs::write(other_root.join("src/index.ts"), "export const value = 1;\n")
        .expect("write other source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_optional_dependencies.iter().any(|dep| {
            dep.dep.package_name == "native-linux-x64-gnu"
                && dep.dep.path.ends_with("packages/other/package.json")
        }),
        "same-named optional dependency in a non-napi workspace should still be reported, found: {:?}",
        results.unused_optional_dependencies
    );
    assert!(
        !results.unused_optional_dependencies.iter().any(|dep| {
            dep.dep.package_name == "native-linux-x64-gnu"
                && dep.dep.path.ends_with("packages/native/package.json")
        }),
        "napi-generated optional dependency should be credited only in its declaring workspace, found: {:?}",
        results.unused_optional_dependencies
    );
}

#[test]
fn unused_workspace_dependency_reports_other_workspace_usage() {
    let root = fixture_path("cross-workspace-dependency-context");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dep = results
        .unused_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "lodash-es")
        .expect("lodash-es should be unused in the shared workspace");

    assert!(
        dep.dep.path.ends_with("packages/shared/package.json"),
        "finding should point at the workspace that declares lodash-es, got {}",
        dep.dep.path.display()
    );
    assert_eq!(
        dep.dep.used_in_workspaces,
        vec![root.join("packages/consumer")],
        "unused dependency should identify the sibling workspace importing it"
    );

    let unlisted = results
        .unlisted_dependencies
        .iter()
        .find(|dep| dep.dep.package_name == "lodash-es")
        .expect("lodash-es should be unlisted in the consumer workspace");
    assert_eq!(
        unlisted.dep.imported_from.len(),
        1,
        "lodash-es should have one unlisted import site"
    );
    assert!(
        unlisted.dep.imported_from[0]
            .path
            .ends_with("packages/consumer/src/index.ts"),
        "finding should point at the importing consumer file, got {}",
        unlisted.dep.imported_from[0].path.display()
    );
}

#[test]
fn nested_workspace_dependency_usage_belongs_to_deepest_workspace() {
    let root = fixture_path("nested-workspace-dependency-ownership");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let parent_path = root.join("packages/parent/package.json");
    let child_path = root.join("packages/parent/packages/child/package.json");
    let shared_lib_findings: Vec<_> = results
        .unused_dependencies
        .iter()
        .filter(|dep| dep.dep.package_name == "shared-lib")
        .collect();

    assert_eq!(
        shared_lib_findings.len(),
        1,
        "only the parent declaration should be unused: {shared_lib_findings:?}"
    );
    assert_eq!(shared_lib_findings[0].dep.path, parent_path);
    assert_eq!(
        shared_lib_findings[0].dep.used_in_workspaces,
        vec![root.join("packages/parent/packages/child")],
        "the parent finding should identify the nested child as the owner of usage"
    );
    assert!(
        results
            .unused_dependencies
            .iter()
            .all(|dep| dep.dep.path != child_path || dep.dep.package_name != "shared-lib"),
        "the nested child declaration should be credited for its import"
    );
}

fn unlisted_sites_for(
    results: &fallow_types::results::AnalysisResults,
    package_name: &str,
) -> Vec<std::path::PathBuf> {
    results
        .unlisted_dependencies
        .iter()
        .filter(|dep| dep.dep.package_name == package_name)
        .flat_map(|dep| dep.dep.imported_from.iter().map(|site| site.path.clone()))
        .collect()
}

/// A workspace file may use a package that only an ancestor manifest declares
/// when the workspace is private or the file is not production code. A
/// production file of a publishable workspace keeps the strict check, because
/// consumers of the published package do not get the ancestor's dependency.
#[test]
fn ancestor_manifest_satisfies_private_and_non_production_imports() {
    let root = fixture_path("workspace-ancestor-manifest-dependencies");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unlisted_sites_for(&results, "root-runtime-lib"),
        vec![root.join("packages/public-lib/src/index.ts")],
        "only the production file of the publishable workspace stays unlisted"
    );
    assert!(
        unlisted_sites_for(&results, "root-test-helper").is_empty(),
        "a test file may use the root devDependency"
    );
    assert!(
        unlisted_sites_for(&results, "build-kit").is_empty(),
        "a build script of a nested workspace may use the ancestor workspace's dependency"
    );
}

/// An import that an ancestor workspace's declaration satisfies counts as a use
/// of that declaration. An import that the strict check still reports as
/// unlisted does not.
#[test]
fn ancestor_declaration_is_credited_when_it_satisfies_a_descendant_import() {
    let root = fixture_path("workspace-ancestor-manifest-dependencies");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let reported = unused_dependency_names_for(&results, "packages/tool/package.json");
    assert!(
        !reported.iter().any(|name| name == "build-kit"),
        "the nested build script uses the tool declaration of build-kit, got: {reported:?}"
    );
    assert!(
        reported.iter().any(|name| name == "cli-runtime-lib"),
        "a production import of the publishable nested workspace does not use the tool declaration, got: {reported:?}"
    );
    assert_eq!(
        unlisted_sites_for(&results, "cli-runtime-lib"),
        vec![root.join("packages/tool/packages/cli/src/index.ts")],
        "the production import stays unlisted in the nested workspace"
    );
}

/// A `peerDependencies` entry installs nothing. A peer-only entry in the
/// owning workspace or in an intermediate ancestor does not stop the walk to
/// the ancestor workspace that installs the package.
#[test]
fn ancestor_credit_passes_peer_only_declarations() {
    let root = fixture_path("workspace-ancestor-peer-only-chain");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let manifest = root.join("packages/kit/package.json");
    let reported: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .filter(|dep| dep.dep.path == manifest)
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        reported.is_empty(),
        "the kit devDependencies satisfy the nested imports, got: {reported:?}"
    );
    assert!(unlisted_sites_for(&results, "build-kit").is_empty());
    assert!(unlisted_sites_for(&results, "format-kit").is_empty());
}

/// Each import credits the nearest manifest that installs the package. A root
/// declaration stays used only for an importer outside every workspace or an
/// importer whose workspace chain does not install the package.
#[test]
fn root_declaration_is_credited_only_through_the_nearest_manifest() {
    let root = fixture_path("root-dependency-nearest-manifest-credit");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let names_at = |deps: Vec<&fallow_types::results::UnusedDependency>, manifest: &str| {
        let path = root.join(manifest);
        let mut names: Vec<String> = deps
            .into_iter()
            .filter(|dep| dep.path == path)
            .map(|dep| dep.package_name.clone())
            .collect();
        names.sort();
        names
    };
    let prod: Vec<_> = results.unused_dependencies.iter().map(|d| &d.dep).collect();
    let dev: Vec<_> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| &d.dep)
        .collect();

    assert_eq!(
        names_at(prod.clone(), "package.json"),
        vec!["shared-runtime".to_string(), "tool-lib".to_string()],
        "a root declaration that every importer reaches through a nearer manifest is unused"
    );
    assert!(
        names_at(dev.clone(), "package.json").is_empty(),
        "a peer-only workspace declaration installs nothing, so the root devDependency stays used"
    );
    assert_eq!(
        names_at(prod.clone(), "packages/tool/package.json"),
        vec!["shared-runtime".to_string()],
        "a workspace declaration that nothing in the workspace imports is still reported"
    );
    assert!(
        names_at(prod.clone(), "packages/app/package.json").is_empty(),
        "the workspace that imports shared-runtime keeps its declaration"
    );
}

/// A root finding for a package that a nearer manifest supplies names the
/// workspaces that declare the package and import it. Without this context the
/// finding reads as "never imported", which the import graph contradicts.
#[test]
fn root_finding_names_the_workspaces_that_declare_and_import_the_package() {
    let root = fixture_path("root-dependency-nearest-manifest-credit");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let finding_at = |name: &str, manifest: &str| {
        let path = root.join(manifest);
        results
            .unused_dependencies
            .iter()
            .map(|finding| &finding.dep)
            .find(|dep| dep.package_name == name && dep.path == path)
            .unwrap_or_else(|| panic!("{name} should be unused in {manifest}"))
            .clone()
    };

    let shared = finding_at("shared-runtime", "package.json");
    assert_eq!(
        shared.declared_and_imported_in,
        vec![root.join("packages/app")],
        "the app workspace declares shared-runtime and imports it"
    );
    assert!(shared.used_in_workspaces.is_empty());

    let tool_lib = finding_at("tool-lib", "package.json");
    assert_eq!(
        tool_lib.declared_and_imported_in,
        vec![root.join("packages/tool")],
        "the nested cli import uses the declaration of the tool workspace"
    );

    let workspace = finding_at("shared-runtime", "packages/tool/package.json");
    assert!(
        workspace.declared_and_imported_in.is_empty(),
        "a workspace finding keeps its context in used_in_workspaces"
    );
    assert_eq!(
        workspace.used_in_workspaces,
        vec![root.join("packages/app")]
    );
}

#[test]
fn package_less_tsconfig_reference_credits_nearest_package_workspace() {
    let project = tempfile::tempdir().expect("create temp dir");
    let root = project.path();
    let parent = root.join("packages/parent");
    let referenced = parent.join("referenced");
    std::fs::create_dir_all(referenced.join("src")).expect("create referenced source dir");

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "package-less-tsconfig-reference",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .expect("write root package.json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
  "files": [],
  "references": [{"path": "./packages/parent/referenced"}]
}"#,
    )
    .expect("write root tsconfig.json");
    std::fs::write(
        parent.join("package.json"),
        r#"{
  "name": "parent",
  "version": "1.0.0",
  "dependencies": {
    "shared-lib": "1.0.0"
  }
}"#,
    )
    .expect("write parent package.json");
    std::fs::write(
        referenced.join("tsconfig.json"),
        r#"{"compilerOptions":{"composite":true},"include":["src"]}"#,
    )
    .expect("write referenced tsconfig.json");
    std::fs::write(
        referenced.join("src/index.ts"),
        "import { value } from \"shared-lib\";\nexport const referenced = value;\n",
    )
    .expect("write referenced source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results
            .unused_dependencies
            .iter()
            .all(|dep| dep.dep.path != parent.join("package.json")
                || dep.dep.package_name != "shared-lib"),
        "a package-less project reference must credit dependency usage to its nearest package-owning workspace: {:?}",
        results.unused_dependencies
    );
}

#[test]
fn peer_dependency_of_used_installed_package_is_not_unused() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::create_dir_all(root.join("node_modules/react-dom")).expect("create react-dom dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "peer-dep-repro",
  "private": true,
  "dependencies": {
    "react": "18.3.1",
    "react-dom": "18.3.1",
    "left-pad": "1.3.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.tsx"),
        "import { createRoot } from 'react-dom/client';\ncreateRoot(document.body).render('hello');\n",
    )
    .expect("write source");
    std::fs::write(
        root.join("node_modules/react-dom/package.json"),
        r#"{"name":"react-dom","peerDependencies":{"react":"^18.3.1"}}"#,
    )
    .expect("write react-dom package");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dep_names.contains(&"react"),
        "react is required as react-dom's peer dependency and must not be reported: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"left-pad"),
        "unrelated unused dependencies should still be reported: {unused_dep_names:?}"
    );
}

#[test]
fn peer_dependency_of_parent_installed_package_is_not_unused() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let parent = tmp.path().join("monorepo");
    let root = parent.join("packages/app");
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::create_dir_all(parent.join("node_modules/react-dom"))
        .expect("create parent react-dom dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "peer-dep-hoisted-repro",
  "private": true,
  "dependencies": {
    "react": "18.3.1",
    "react-dom": "18.3.1",
    "left-pad": "1.3.0"
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.tsx"),
        "import { createRoot } from 'react-dom/client';\ncreateRoot(document.body).render('hello');\n",
    )
    .expect("write source");
    std::fs::write(
        parent.join("node_modules/react-dom/package.json"),
        r#"{
  "name": "react-dom",
  "peerDependencies": {"react": "^18.3.1"},
  "exports": {"./client": "./client.js"}
}"#,
    )
    .expect("write react-dom package");
    std::fs::write(
        parent.join("node_modules/react-dom/client.js"),
        "export function createRoot() { return { render() {} }; }\n",
    )
    .expect("write react-dom client");

    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        !unused_dep_names.contains(&"react"),
        "react is required as parent-installed react-dom's peer dependency and must not be reported: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"left-pad"),
        "unrelated unused dependencies should still be reported: {unused_dep_names:?}"
    );
}

#[test]
fn optional_peer_of_used_dependency_is_not_unused() {
    let root = fixture_path("optional-peer-of-used-dependency");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    unused_dep_names.sort_unstable();

    // `host` is imported and declares `opt-peer` as an optional peer, so the
    // listed `opt-peer` turns on a host feature. `unused-host` is not imported,
    // so its optional peer `peer-of-unused` gets no credit.
    assert_eq!(
        unused_dep_names,
        vec!["peer-of-unused", "unused-host"],
        "only the optional peer of a used host is credited"
    );
}

#[test]
fn dev_dependency_listed_as_own_peer_is_not_unused() {
    let root = fixture_path("dev-dependency-listed-as-own-peer");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused_dev: Vec<(String, &str)> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| {
            let manifest = d
                .dep
                .path
                .strip_prefix(&root)
                .unwrap_or(&d.dep.path)
                .to_string_lossy()
                .replace('\\', "/");
            (manifest, d.dep.package_name.as_str())
        })
        .collect();
    unused_dev.sort_unstable();

    // `react` (root, optional peer) and `react-dom` (workspace, required peer)
    // are dev copies of the package's own peer. `left-pad` and `is-odd` have no
    // peer entry and stay reported.
    assert_eq!(
        unused_dev,
        vec![
            ("package.json".to_string(), "left-pad"),
            ("packages/lib/package.json".to_string(), "is-odd"),
        ],
        "a devDependency listed as the same manifest's peer is credited"
    );
}

#[test]
fn subpath_imports_resolve_correctly() {
    let root = fixture_path("subpath-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "# imports should resolve via package.json imports field, got unresolved: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|u| u.import.specifier.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        results.unlisted_dependencies.is_empty(),
        "# imports should not be reported as unlisted deps, got: {:?}",
        results
            .unlisted_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_str())
            .collect::<Vec<_>>()
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();
    assert!(
        unused_export_names.contains(&"unused"),
        "unused export should still be detected, got: {unused_export_names:?}"
    );
}

#[test]
fn package_imports_missing_dist_resolve_to_source() {
    let root = fixture_path("package-imports-missing-dist");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"#nitro/runtime/task"),
        "manifest-mapped runtime import should resolve, got: {unresolved_specifiers:?}"
    );
    assert!(
        !unresolved_specifiers.contains(&"#nitro/virtual/polyfills"),
        "manifest-mapped virtual import should resolve, got: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"#nitro/runtime/missing"),
        "manifest match without a source target should stay unresolved: {unresolved_specifiers:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"#other/alias"),
        "unmatched hash alias should stay unresolved: {unresolved_specifiers:?}"
    );

    assert!(
        results.unlisted_dependencies.is_empty(),
        "root self import and package imports should not become unlisted deps: {:?}",
        results
            .unlisted_dependencies
            .iter()
            .map(|d| d.dep.package_name.as_str())
            .collect::<Vec<_>>()
    );

    assert!(
        results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/runtime/internal/orphan.ts")),
        "unrelated source files should still be reported as unused"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/runtime/internal/task.ts")),
        "runtime task source should be reachable through imports fallback"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/runtime/virtual/polyfills.ts")),
        "virtual polyfills source should be reachable through imports fallback"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/self.ts")),
        "root self package export should resolve back to source"
    );
}

#[test]
fn package_imports_external_targets_credit_dependency_usage() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r##"{
  "name": "imports-external-target",
  "main": "src/index.ts",
  "imports": {
    "#pad": "left-pad"
  },
  "dependencies": {
    "left-pad": "1.3.0",
    "unused": "1.0.0"
  }
}"##,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import pad from '#pad';\nexport const value = pad('x', 2);\n",
    )
    .expect("write source");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"#pad"),
        "package imports external target should resolve: {unresolved_specifiers:?}"
    );

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"left-pad"),
        "external target dependency should be credited as used: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"unused"),
        "unrelated dependency should still be reported unused: {unused_dep_names:?}"
    );
}

#[cfg(unix)]
#[test]
fn issue_1008_pnpm_workspace_dependency_imported_from_subpackage_root() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let monorepo = tmp.path();
    let consumer = monorepo.join("packages/consumer");
    let shared = monorepo.join("packages/shared");

    std::fs::create_dir_all(consumer.join("src")).expect("create consumer src");
    std::fs::create_dir_all(consumer.join("node_modules/@mre")).expect("create consumer scope");
    std::fs::create_dir_all(shared.join("src")).expect("create shared src");

    std::fs::write(
        monorepo.join("package.json"),
        r#"{
  "name": "issue-1008-root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .expect("write root package.json");
    std::fs::write(
        monorepo.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n",
    )
    .expect("write pnpm workspace");
    std::fs::write(
        consumer.join("package.json"),
        r#"{
  "name": "@mre/consumer",
  "private": true,
  "type": "module",
  "dependencies": {
    "@mre/shared": "workspace:*",
    "left-pad": "1.3.0"
  }
}"#,
    )
    .expect("write consumer package.json");
    std::fs::write(
        shared.join("package.json"),
        r#"{
  "name": "@mre/shared",
  "private": true,
  "type": "module",
  "exports": {
    ".": "./src/index.ts"
  }
}"#,
    )
    .expect("write shared package.json");
    std::fs::write(
        consumer.join("src/index.ts"),
        "import { formatPayload } from '@mre/shared';\nexport const value = formatPayload('ok');\n",
    )
    .expect("write consumer source");
    std::fs::write(
        shared.join("src/index.ts"),
        "export const formatPayload = (value: string): string => value;\n",
    )
    .expect("write shared source");
    std::os::unix::fs::symlink("../../../shared", consumer.join("node_modules/@mre/shared"))
        .expect("symlink workspace package");

    let config = create_config(consumer);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"@mre/shared"),
        "imported workspace dependency should be credited through pnpm symlink: {unused_dep_names:?}"
    );
    assert!(
        unused_dep_names.contains(&"left-pad"),
        "unrelated dependency should still be reported unused: {unused_dep_names:?}"
    );

    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"@mre/shared"),
        "workspace dependency import should not become unresolved: {unresolved_specifiers:?}"
    );
}

#[test]
fn package_imports_array_fallback_resolves_reachable_target() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r##"{
  "name": "imports-array-fallback",
  "main": "src/index.ts",
  "imports": {
    "#public/feature": ["./dist/missing.js", "./src/feature.ts"]
  }
}"##,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { feature } from '#public/feature';\nexport const value = feature();\n",
    )
    .expect("write index");
    std::fs::write(
        root.join("src/feature.ts"),
        "export function feature() { return 'ok'; }\n",
    )
    .expect("write feature");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"#public/feature"),
        "array fallback should resolve to the reachable target: {unresolved_specifiers:?}"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/feature.ts")),
        "array fallback target should be reachable"
    );
}

#[test]
fn package_exports_array_fallback_resolves_self_package_source() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "self-array-fallback",
  "main": "src/index.ts",
  "exports": {
    "./public-feature": ["./dist/missing.js", "./src/feature.ts"]
  }
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import { feature } from 'self-array-fallback/public-feature';\nexport const value = feature();\n",
    )
    .expect("write index");
    std::fs::write(
        root.join("src/feature.ts"),
        "export function feature() { return 'ok'; }\n",
    )
    .expect("write feature");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved_specifiers.contains(&"self-array-fallback/public-feature"),
        "self-package exports array fallback should resolve: {unresolved_specifiers:?}"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("src/feature.ts")),
        "self-package exports array fallback target should be reachable"
    );
}

#[test]
fn ignore_patterns_applied_to_workspace_package_json_for_unused_deps() {
    let root = fixture_path("ignore-patterns-workspace-package-json");
    let config = FallowConfig {
        type_aware: fallow_config::TypeAwareConfig::default(),
        schema: None,
        minimum_version: None,
        extends: vec![],
        entry: vec![],
        ignore_patterns: vec!["**/dist/**".to_string()],
        ignore_findings: vec![],
        framework: vec![],
        workspaces: None,
        ignore_dependencies: vec![],
        ignore_command_entries: vec![],
        ignore_unresolved_imports: vec![],
        ignore_exports: vec![],
        ignore_catalog_references: vec![],
        ignore_dependency_overrides: vec![],
        ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
        used_class_members: vec![],
        ignore_decorators: vec![],
        unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
        circular_dependencies: fallow_config::CircularDependenciesConfig::default(),
        duplicates: fallow_config::DuplicatesConfig::default(),
        similar_code: fallow_config::SimilarCodeConfig::default(),
        health: fallow_config::HealthConfig::default(),
        rules: RulesConfig::default(),
        boundaries: fallow_config::BoundaryConfig::default(),
        production: false.into(),
        plugins: vec![],
        rule_packs: vec![],
        dynamically_loaded: vec![],
        overrides: vec![],
        regression: None,
        audit: fallow_config::AuditConfig::default(),
        codeowners: None,
        public_packages: vec![],
        flags: fallow_config::FlagsConfig::default(),
        security: fallow_config::SecurityConfig::default(),
        fix: fallow_config::FixConfig::default(),
        resolve: fallow_config::ResolveConfig::default(),
        sealed: false,
        include_entry_exports: false,
        auto_imports: false,
        fail_on_parse_error: false,
        cache: fallow_config::CacheConfig::default(),
    }
    .resolve(root, OutputFormat::Human, 4, true, true, None);

    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dist_findings: Vec<String> = results
        .unused_dependencies
        .iter()
        .filter(|d| {
            d.dep
                .path
                .components()
                .any(|c| matches!(c, std::path::Component::Normal(s) if s == "dist"))
        })
        .map(|d| format!("{} -> {}", d.dep.package_name, d.dep.path.display()))
        .collect();
    assert!(
        dist_findings.is_empty(),
        "deps from dist/package.json must not be reported when dist/ is ignored: {dist_findings:?}"
    );

    let reported: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        reported.contains(&"is-odd"),
        "real unused dep `is-odd` should still be reported, got: {reported:?}"
    );
}

/// Write a monorepo root that discovers every directory under `packages/`.
fn write_private_bundle_root(root: &std::path::Path) {
    fs::write(
        root.join("package.json"),
        r#"{
  "name": "bundle-root",
  "private": true,
  "workspaces": ["packages/*"]
}"#,
    )
    .expect("write root package.json");
}

/// Write one workspace package with a manifest and a single source file.
fn write_bundle_workspace(root: &std::path::Path, name: &str, manifest: &str, source: &str) {
    let ws_root = root.join("packages").join(name);
    fs::create_dir_all(ws_root.join("src")).expect("create workspace package");
    fs::write(ws_root.join("package.json"), manifest).expect("write workspace package.json");
    fs::write(ws_root.join("src/index.ts"), source).expect("write workspace source");
}

fn unused_dependency_names_for(
    results: &fallow_types::results::AnalysisResults,
    workspace_suffix: &str,
) -> Vec<String> {
    results
        .unused_dependencies
        .iter()
        .filter(|d| d.dep.path.ends_with(workspace_suffix))
        .map(|d| d.dep.package_name.clone())
        .collect()
}

/// Discussion #2244: a private, unpublished sibling workspace is inlined into
/// its consumer's build, so the third-party packages the sibling imports have
/// to be resolvable from the consumer's own manifest. Declaring them there is
/// correct, not dead weight.
#[test]
fn private_sibling_bundled_dependency_is_credited_to_the_consumer() {
    let root = fixture_path("private-workspace-bundled-dependencies");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let reported = unused_dependency_names_for(&results, "packages/workspace-a/package.json");

    assert!(
        !reported.iter().any(|name| name == "lodash-es"),
        "lodash-es is bundled from the private `shared` workspace and must not be reported in workspace-a, got: {reported:?}"
    );
    assert!(
        reported.iter().any(|name| name == "nothing-uses-this"),
        "a dependency no workspace imports must still be reported, got: {reported:?}"
    );
}

/// A consumer whose build leaves every package external does not inline the
/// private sibling, so the sibling's packages do not need the consumer's
/// declaration. The signal is esbuild `packages: 'external'` in a build file
/// or `bun build --packages=external` in a package script.
#[test]
fn externalizing_consumer_gets_no_bundled_credit() {
    for fixture in [
        "private-workspace-externalized-esbuild",
        "private-workspace-externalized-bun-build",
    ] {
        let config = create_config(fixture_path(fixture));
        let results = fallow_core::analyze(&config).expect("analysis should succeed");

        let reported = unused_dependency_names_for(&results, "packages/consumer/package.json");
        assert_eq!(
            reported,
            vec!["lodash-es".to_string()],
            "{fixture}: the externalized sibling's lodash-es is not credited to the consumer"
        );
    }
}

/// A published sibling is installed from the registry with its own dependency
/// tree, so the consumer never needs the sibling's packages hoisted. Crediting
/// them there would hide a real finding.
#[test]
fn published_sibling_does_not_credit_bundled_dependency() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_private_bundle_root(root);
    write_bundle_workspace(
        root,
        "shared",
        r#"{
  "name": "shared",
  "version": "1.0.0",
  "main": "src/index.ts",
  "dependencies": {
    "lodash-es": "4.17.21"
  }
}"#,
        "import { cloneDeep } from \"lodash-es\";\n\nexport const clone = cloneDeep;\n",
    );
    write_bundle_workspace(
        root,
        "consumer",
        r#"{
  "name": "consumer",
  "version": "1.0.0",
  "main": "src/index.ts",
  "dependencies": {
    "lodash-es": "4.17.21",
    "shared": "1.0.0"
  }
}"#,
        "import { clone } from \"shared\";\n\nexport const value = clone;\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let reported = unused_dependency_names_for(&results, "packages/consumer/package.json");
    assert!(
        reported.iter().any(|name| name == "lodash-es"),
        "a published sibling's dependency must stay reported in the consumer, got: {reported:?}"
    );
}

/// The bundle follows the private edge as far as it goes: a private sibling of
/// a private sibling is inlined into the consumer too.
#[test]
fn transitive_private_chain_credits_the_deepest_sibling_imports() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_private_bundle_root(root);
    write_bundle_workspace(
        root,
        "deep",
        r#"{
  "name": "deep",
  "version": "1.0.0",
  "private": true,
  "main": "src/index.ts",
  "dependencies": {
    "lodash-es": "4.17.21"
  }
}"#,
        "import { cloneDeep } from \"lodash-es\";\n\nexport const clone = cloneDeep;\n",
    );
    write_bundle_workspace(
        root,
        "mid",
        r#"{
  "name": "mid",
  "version": "1.0.0",
  "private": true,
  "main": "src/index.ts",
  "dependencies": {
    "deep": "1.0.0"
  }
}"#,
        "import { clone } from \"deep\";\n\nexport const midClone = clone;\n",
    );
    write_bundle_workspace(
        root,
        "app",
        r#"{
  "name": "app",
  "version": "1.0.0",
  "main": "src/index.ts",
  "dependencies": {
    "lodash-es": "4.17.21",
    "mid": "1.0.0",
    "nothing-uses-this": "1.0.0"
  }
}"#,
        "import { midClone } from \"mid\";\n\nexport const value = midClone;\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let reported = unused_dependency_names_for(&results, "packages/app/package.json");
    assert!(
        !reported.iter().any(|name| name == "lodash-es"),
        "lodash-es reaches app through mid -> deep and must not be reported, got: {reported:?}"
    );
    assert!(
        reported.iter().any(|name| name == "nothing-uses-this"),
        "a dependency outside the bundled closure must still be reported, got: {reported:?}"
    );
}

/// Workspace dependency graphs can be cyclic. The closure must terminate and
/// still credit what the cycle bundles.
#[test]
fn cyclic_private_workspace_graph_terminates() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_private_bundle_root(root);
    write_bundle_workspace(
        root,
        "alpha",
        r#"{
  "name": "alpha",
  "version": "1.0.0",
  "private": true,
  "main": "src/index.ts",
  "dependencies": {
    "beta": "1.0.0",
    "lodash-es": "4.17.21"
  }
}"#,
        "export const alpha = 1;\n",
    );
    write_bundle_workspace(
        root,
        "beta",
        r#"{
  "name": "beta",
  "version": "1.0.0",
  "private": true,
  "main": "src/index.ts",
  "dependencies": {
    "alpha": "1.0.0",
    "lodash-es": "4.17.21"
  }
}"#,
        "import { cloneDeep } from \"lodash-es\";\n\nexport const clone = cloneDeep;\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let reported = unused_dependency_names_for(&results, "packages/alpha/package.json");
    assert!(
        !reported.iter().any(|name| name == "lodash-es"),
        "the cycle must resolve and credit beta's import to alpha, got: {reported:?}"
    );
}

/// A private sibling's devDependencies are build-time needs of that sibling,
/// never inlined into the consumer's output, so they stay out of the closure.
#[test]
fn dev_dependency_of_a_private_sibling_is_not_credited() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_private_bundle_root(root);
    write_bundle_workspace(
        root,
        "tool",
        r#"{
  "name": "tool",
  "version": "1.0.0",
  "private": true,
  "main": "src/index.ts",
  "devDependencies": {
    "lodash-es": "4.17.21"
  }
}"#,
        "import { cloneDeep } from \"lodash-es\";\n\nexport const clone = cloneDeep;\n",
    );
    write_bundle_workspace(
        root,
        "consumer",
        r#"{
  "name": "consumer",
  "version": "1.0.0",
  "main": "src/index.ts",
  "dependencies": {
    "lodash-es": "4.17.21",
    "tool": "1.0.0"
  }
}"#,
        "import { clone } from \"tool\";\n\nexport const value = clone;\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let reported = unused_dependency_names_for(&results, "packages/consumer/package.json");
    assert!(
        reported.iter().any(|name| name == "lodash-es"),
        "a dev-only declaration in the private sibling must not credit the consumer, got: {reported:?}"
    );
}

/// An import with an empty specifier list (`import {} from 'pkg'` or
/// `import type {} from 'pkg'`) still names the package. It credits the
/// package as a dependency, and a relative form keeps the target file
/// reachable.
#[test]
fn empty_specifier_list_import_credits_package_and_file() {
    let root = fixture_path("empty-specifier-import-credit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dep_names.contains(&"@x/settings"),
        "`import {{}} from` should credit the dependency, found: {unused_dep_names:?}"
    );
    assert!(
        !unused_dev_dep_names.contains(&"@x/settings"),
        "`import type {{}} from` should credit the dev dependency, found: {unused_dev_dep_names:?}"
    );
    assert!(
        !results
            .unused_files
            .iter()
            .any(|f| f.file.path.ends_with("packages/app/src/augment.ts")),
        "`import type {{}} from './augment'` should keep the file reachable"
    );
}

/// A top-level `declare module 'pkg' { ... }` in a module file augments the
/// package, and TypeScript requires `pkg` to resolve. It credits the package
/// as a type-only use. A `declare module` in a script file declares an
/// ambient shim and credits nothing. Neither form reports an unlisted
/// dependency.
#[test]
fn module_augmentation_credits_package() {
    let root = fixture_path("module-augmentation-package-credit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev_dep_names.contains(&"@x/slots"),
        "the augmentation in a module file should credit the package, found: {unused_dev_dep_names:?}"
    );
    assert!(
        unused_dev_dep_names.contains(&"ambient-lib"),
        "the ambient declaration in a script file must not credit the package, found: {unused_dev_dep_names:?}"
    );

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert_eq!(
        unlisted,
        vec!["missing-lib"],
        "only the real import may report an unlisted dependency"
    );
}

/// A plugin credits its own tooling devDependencies only with evidence that
/// the project runs the tool: a config file of its own, its config in
/// package.json, or a script, CI workflow or git hook that invokes it. A
/// declared package alone activates the plugin and is no evidence.
#[test]
fn plugin_tooling_dev_dependency_needs_config_or_reference() {
    let root = fixture_path("plugin-tooling-credit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    unused_dev_dep_names.sort_unstable();

    // Credited, each by a different kind of evidence:
    // c8 (`.c8rc.json`), lefthook (`lefthook.yml`), simple-git-hooks (its
    // package.json key), mocha (a script), ts-mocha (the mocha plugin, whose
    // tool a script runs), syncpack (a simple-git-hooks command), size-limit
    // (a lefthook command) and markdownlint-cli2 (a husky hook).
    assert_eq!(
        unused_dev_dep_names,
        vec!["commitizen", "cz-conventional-changelog", "karma", "nyc"],
        "only the tooling devDependencies without a config file or a reference should be reported"
    );
}

/// A `@types/X` devDependency is credited only when the project declares X,
/// imports X or names X in a tsconfig `types` entry, or when X is an ambient
/// global type package such as `node` or `jest`.
#[test]
fn types_dev_dependency_needs_target_or_ambient_globals() {
    let root = fixture_path("types-package-credit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    unused_dev_dep_names.sort_unstable();

    // Credited: @types/react (react is declared), @types/scope__pkg
    // (@scope/pkg is declared), @types/geojson (a type-only import of
    // geojson), @types/ws (a tsconfig `types` entry), and the ambient globals
    // @types/node, @types/jest and bun-types.
    assert_eq!(
        unused_dev_dep_names,
        vec!["@types/better-sqlite3", "@types/uuid"],
        "only the type packages without a target, a tsconfig entry or ambient globals should be reported"
    );
}

/// A command-line tool from the tooling catalogue is credited only when a
/// package.json script, a CI workflow or a git hook runs it, or when its own
/// config file exists. A catalogue entry that is not a command-line tool,
/// such as `sass`, keeps its credit.
#[test]
fn catalogue_cli_dev_dependency_needs_reference_or_config() {
    let root = fixture_path("catalogue-cli-credit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    unused_dev_dep_names.sort_unstable();

    // Credited: concurrently (a script), rimraf (a CI workflow), cross-env (a
    // husky hook), prettier (a lint-staged command and `.prettierrc`),
    // lint-staged (its package.json key), jscpd (`.jscpd.json`), madge
    // (`.madgerc`) and sass (not a command-line tool).
    assert_eq!(
        unused_dev_dep_names,
        vec!["npm-run-all", "oxlint", "tsx"],
        "only the command-line tools without a reference or a config file should be reported"
    );
}

/// A bundler such as Next.js runs postcss.config.* with its own copy of
/// PostCSS, so the project often declares no `postcss` package. The packages
/// that the config names in `plugins` are still in use.
#[test]
fn postcss_config_credits_plugins_without_postcss_dependency() {
    let root = fixture_path("postcss-config-without-postcss-dependency");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        unused_dev_dep_names.is_empty(),
        "postcss.config.mjs names every devDependency: {unused_dev_dep_names:?}"
    );

    let unused_files: Vec<&str> = results
        .unused_files
        .iter()
        .filter_map(|f| f.file.path.file_name())
        .filter_map(|f| f.to_str())
        .collect();
    assert!(
        !unused_files.contains(&"postcss.config.mjs"),
        "the bundler loads postcss.config.mjs: {unused_files:?}"
    );
}

/// A script binary credits the package that installs it, also when
/// `node_modules` is missing. The binary name differs from the package name
/// here: `run-p` comes from `npm-run-all2`, `ember` from `ember-cli`, `ncu`
/// from `npm-check-updates`, and so on.
#[test]
fn script_binary_credits_installing_package_without_node_modules() {
    let root = fixture_path("script-binary-package-credit");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    unused_dev_dep_names.sort_unstable();

    assert_eq!(
        unused_dev_dep_names,
        vec!["unused-control"],
        "each tool that a script runs must be credited through its binary name"
    );
}

/// A tool that the root manifest declares is credited when its config file is
/// only in a workspace package that does not declare the tool. A root-hoisted
/// devDependency with a config file per package is a usual monorepo layout.
#[test]
fn plugin_tooling_root_dev_dependency_credited_by_workspace_config() {
    let root = fixture_path("plugin-tooling-workspace-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_dev_dep_names: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    unused_dev_dep_names.sort_unstable();

    // c8, karma and prettier have a config file in `packages/app`. nyc has no
    // config file and no reference anywhere.
    assert_eq!(
        unused_dev_dep_names,
        vec!["nyc"],
        "a config file in a workspace package should credit the root tooling devDependency"
    );
}

fn unused_dev_dependency_names(fixture: &str) -> Vec<String> {
    let config = create_config(fixture_path(fixture));
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut names: Vec<String> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .collect();
    names.sort_unstable();
    names
}

/// Jest loads a TypeScript config file through `ts-node`, so `ts-node` is
/// used when `jest.config.ts` exists. Another runtime loader that nothing
/// loads stays unused.
#[test]
fn jest_typescript_config_credits_its_config_loader() {
    assert_eq!(
        unused_dev_dependency_names("tool-config-loader-jest"),
        vec!["tsx"],
        "ts-node loads jest.config.ts and must not be reported"
    );
}

/// Mocha loads the packages in the `require` and `node-option` entries of
/// its config file. A loader that the config does not name stays unused.
#[test]
fn mocha_config_credits_required_loaders() {
    assert_eq!(
        unused_dev_dependency_names("tool-config-loader-mocha"),
        vec!["@swc/register"],
        "tsx (require) and ts-node (node-option loader) must not be reported"
    );
}

/// nodemon reads its config from the package.json `nodemonConfig` key.
#[test]
fn nodemon_package_json_config_key_credits_nodemon() {
    assert!(
        unused_dev_dependency_names("tool-config-loader-nodemon").is_empty(),
        "nodemon has its config under nodemonConfig and must not be reported"
    );
}

/// A root declaration that every importer reaches through a nearer manifest
/// gets one `unused-dependency` finding. The test-only and type-only checks
/// read the same importers, so they do not also tell the user to move the
/// root entry to `devDependencies`.
#[test]
fn root_dependency_used_only_through_a_workspace_gets_one_finding() {
    let root = fixture_path("root-dependency-test-or-type-only-through-workspace");
    let root_manifest = root.join("package.json");
    let root_names = |deps: Vec<&fallow_types::results::UnusedDependency>| {
        let mut names: Vec<String> = deps
            .into_iter()
            .filter(|dep| dep.path == root_manifest)
            .map(|dep| dep.package_name.clone())
            .collect();
        names.sort();
        names
    };

    let default =
        fallow_core::analyze(&create_config(root.clone())).expect("analysis should succeed");
    assert_eq!(
        root_names(default.unused_dependencies.iter().map(|d| &d.dep).collect()),
        vec!["assert-lib".to_string(), "schema-lib".to_string()],
        "no importer uses the root declarations"
    );
    let test_only: Vec<&str> = default
        .test_only_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        test_only.is_empty(),
        "an unused root entry must not also get a test-only finding, got: {test_only:?}"
    );

    let production = fallow_core::analyze(&super::common::create_production_config(root))
        .expect("production analysis should succeed");
    assert!(
        root_names(
            production
                .unused_dependencies
                .iter()
                .map(|d| &d.dep)
                .collect()
        )
        .contains(&"schema-lib".to_string()),
        "the root schema-lib declaration is unused in production mode"
    );
    let type_only: Vec<&str> = production
        .type_only_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        type_only.is_empty(),
        "an unused root entry must not also get a type-only finding, got: {type_only:?}"
    );
}
