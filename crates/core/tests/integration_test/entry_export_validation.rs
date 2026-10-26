use super::common::{create_config, fixture_path};

#[test]
fn entry_exports_skipped_by_default() {
    let root = fixture_path("entry-export-validation");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"meatdata"),
        "meatdata should not be flagged (entry exports skipped by default), found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"config"),
        "config should not be flagged (entry exports skipped by default), found: {unused_export_names:?}"
    );
}

#[test]
fn entry_exports_detected_when_include_entry_exports_enabled() {
    let root = fixture_path("entry-export-validation");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"meatdata"),
        "meatdata should be flagged with include_entry_exports, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"config"),
        "config should be flagged with include_entry_exports, found: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names.contains(&"helper"),
        "helper should not be flagged (imported by consumer.ts), found: {unused_export_names:?}"
    );
}

#[test]
fn entry_exports_detected_via_config_file_include_entry_exports() {
    let root = fixture_path("entry-export-validation-config");
    let (loaded, _path) = fallow_config::FallowConfig::find_and_load(&root)
        .expect("config load")
        .expect("fixture has .fallowrc.json");
    assert!(loaded.include_entry_exports);
    let config = loaded.resolve(
        root,
        fallow_config::OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"meatdata"),
        "meatdata should be flagged via config file, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"config"),
        "config should be flagged via config file, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"helper"),
        "helper should not be flagged (imported by consumer.ts), found: {unused_export_names:?}"
    );
}

#[test]
fn vitest_config_default_export_is_framework_used_with_include_entry_exports() {
    let root = fixture_path("vitest-include-entry-exports-workspace");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports.iter().any(|(path, export)| {
            path == "packages/web/charts/vitest.config.mts" && export == "default"
        }),
        "Vitest config default export should be framework-used, found: {unused_exports:?}"
    );
}

#[test]
fn vite_config_default_export_is_framework_used_with_include_entry_exports() {
    let root = fixture_path("vite-include-entry-exports-workspace");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports.iter().any(|(path, export)| {
            path == "packages/web/charts/vite.config.mts" && export == "default"
        }),
        "Vite config default export should be framework-used, found: {unused_exports:?}"
    );
}

#[test]
fn postcss_config_default_export_is_framework_used_with_include_entry_exports() {
    // Issue #3691: PostCSS reads only the default export of postcss.config.*,
    // so under includeEntryExports it is used and a named export is not.
    let root = fixture_path("postcss-include-entry-exports");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();

    assert!(
        !unused_exports
            .iter()
            .any(|(path, export)| path == "postcss.config.mjs" && export == "default"),
        "PostCSS config default export should be framework-used, found: {unused_exports:?}"
    );
    assert!(
        unused_exports
            .iter()
            .any(|(path, export)| path == "postcss.config.mjs" && export == "unusedHelper"),
        "a named export of postcss.config.mjs is not read by PostCSS, found: {unused_exports:?}"
    );
}

#[test]
fn commonjs_postcss_config_keys_are_credited_through_the_default_export() {
    // Issue #3876: `module.exports = { plugins }` is the default export that
    // PostCSS loads, so its keys are not unused exports.
    let root = fixture_path("postcss-cjs-config");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let config_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .filter(|export| export.export.path.ends_with("postcss.config.cjs"))
        .map(|export| export.export.export_name.as_str())
        .collect();
    assert!(
        config_exports.is_empty(),
        "keys of a CommonJS postcss.config.cjs should be credited, found: {config_exports:?}"
    );
    assert!(
        results.unused_exports.iter().any(|export| {
            export.export.path.ends_with("src/helpers.cjs")
                && export.export.export_name == "unusedKey"
        }),
        "a CommonJS module without a config credit keeps its findings: {:?}",
        results.unused_exports
    );
}

fn config_default_credit_findings(include_entry_exports: bool) -> Vec<(String, String)> {
    let root = fixture_path("config-default-export-credit");
    let mut config = create_config(root);
    config.include_entry_exports = include_entry_exports;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut findings: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();
    findings.sort();
    findings
}

fn config_file_exports_findings(include_entry_exports: bool) -> Vec<(String, String)> {
    let root = fixture_path("config-file-exports-credit");
    let mut config = create_config(root);
    config.include_entry_exports = include_entry_exports;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut findings: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();
    findings.sort();
    findings
}

fn finding(path: &str, export: &str) -> (String, String) {
    (path.to_owned(), export.to_owned())
}

#[test]
fn config_file_default_export_is_used_with_include_entry_exports() {
    // Issue #3691: a tool loads its config file and reads the default export.
    // Every form of the default export gets the credit: an object, a
    // defineConfig(...) call, an arrow function, an async function and
    // `export { config as default }`. A config file of a workspace package
    // gets the credit too. A named export of a config file stays reportable.
    let findings = config_default_credit_findings(true);

    for credited in [
        "tailwind.config.js",
        "playwright.config.js",
        "jest.config.js",
        "rollup.config.js",
        "typedoc.config.js",
        "graphql.config.js",
        "gatsby-config.js",
        "packages/web/uno.config.js",
    ] {
        assert!(
            !findings.contains(&finding(credited, "default")),
            "the default export of {credited} is read by its tool, found: {findings:?}"
        );
    }
    assert!(
        findings.contains(&finding("tailwind.config.js", "unusedHelper")),
        "a named export of a config file stays reportable, found: {findings:?}"
    );
}

#[test]
fn opted_out_config_patterns_keep_default_export_reportable() {
    // Gatsby reads named API exports from gatsby-node. `codegen*` also
    // matches source files such as src/codegenUtils.js. The Codegen,
    // Serverless and Vercel credit is anchored at the root, so only the root
    // config file gets it.
    let findings = config_default_credit_findings(true);

    for credited in ["codegen.js", "serverless.js", "vercel.js"] {
        assert!(
            !findings.contains(&finding(credited, "default")),
            "the root config file {credited} gets the anchored credit, found: {findings:?}"
        );
    }
    for reported in ["gatsby-node.js", "src/codegenUtils.js"] {
        assert!(
            findings.contains(&finding(reported, "default")),
            "the default export of {reported} gets no credit, found: {findings:?}"
        );
    }
}

#[test]
fn config_default_credit_does_not_change_default_mode() {
    // Without includeEntryExports a config file is an entry point, and fallow
    // skips its exports. The credit must not make fallow analyse the file, so
    // the unused named export of tailwind.config.js stays unreported.
    let findings = config_default_credit_findings(false);

    assert_eq!(
        findings,
        vec![finding("src/codegenUtils.js", "default")],
        "default-mode output must not change"
    );
}

#[test]
fn config_file_exports_are_used_with_include_entry_exports() {
    // These tools list their config file only as always-used, or read named
    // exports. Each config file gets the credit for the exports its tool reads.
    let findings = config_file_exports_findings(true);

    for credited in [
        "capacitor.config.ts",
        "sanity.config.ts",
        "sanity.cli.ts",
        "nitro.config.ts",
        "svgo.config.js",
        "svgr.config.js",
        ".svgrrc.js",
        "kysely.config.ts",
        "openapi-ts.config.ts",
        "remix.config.js",
        "i18next.config.ts",
        ".vitepress/config.ts",
        "cspell.config.js",
        "package.config.ts",
        "forge.config.js",
        "electron-builder.js",
        "codegen.ts",
        "serverless.ts",
        "cucumber.js",
    ] {
        assert!(
            !findings.contains(&finding(credited, "default")),
            "the default export of {credited} is read by its tool, found: {findings:?}"
        );
    }
    assert!(
        !findings.contains(&finding("cucumber.js", "ci")),
        "cucumber-js reads each named export as a profile, found: {findings:?}"
    );
    assert!(
        !findings.contains(&finding("vercel.ts", "config")),
        "@vercel/config reads the named exports of vercel.ts, found: {findings:?}"
    );
    assert!(
        findings.contains(&finding("capacitor.config.ts", "unusedHelper")),
        "a named export that the tool does not read stays reportable, found: {findings:?}"
    );
}

#[test]
fn app_source_and_helper_modules_keep_default_export_reportable() {
    // src/i18n.ts is application source, not a tool config file. The credit
    // is anchored, so a helper module in src/lib and a config copy in a
    // subdirectory get none.
    let findings = config_file_exports_findings(true);

    for reported in [
        "src/i18n.ts",
        "src/lib/serverless.ts",
        "config/svgo.config.js",
    ] {
        assert!(
            findings.contains(&finding(reported, "default")),
            "the default export of {reported} gets no credit, found: {findings:?}"
        );
    }
}

#[test]
fn config_file_exports_do_not_change_default_mode() {
    // Without includeEntryExports, config files are entry points and fallow
    // skips their exports. The credit must not make fallow analyse them.
    let findings = config_file_exports_findings(false);

    assert_eq!(
        findings,
        vec![
            finding("config/svgo.config.js", "default"),
            finding("src/lib/serverless.ts", "default"),
        ],
        "default-mode output must not change"
    );
}

#[test]
fn storybook_exports_are_framework_used_with_include_entry_exports() {
    let root = fixture_path("storybook-include-entry-exports");
    let mut config = create_config(root);
    config.include_entry_exports = true;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|export| {
            (
                export
                    .export
                    .path
                    .strip_prefix(&config.root)
                    .unwrap_or(&export.export.path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                export.export.export_name.clone(),
            )
        })
        .collect();

    for (path, export) in [
        ("src/Button.stories.ts", "default"),
        ("src/Button.stories.ts", "Default"),
        ("src/Button.stories.ts", "Secondary"),
        (".storybook/preview.ts", "parameters"),
        (".storybook/preview.ts", "decorators"),
    ] {
        assert!(
            !unused_exports
                .iter()
                .any(|(unused_path, unused_export)| unused_path == path && unused_export == export),
            "{path}:{export} should be framework-used, found: {unused_exports:?}"
        );
    }
}
