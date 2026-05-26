//! Issue #624: Deno/Supabase Edge Functions.
//!
//! `jsr:` and URL imports must not surface as unresolved/unlisted noise, `npm:`
//! specifiers are normalized to their npm package (credited if declared, never
//! unlisted when used only via `npm:`), and `supabase/functions/*/index.*` files
//! are runtime entry roots rather than unused files. Shared code reached through
//! relative imports stays reachable; an unrelated orphan stays reportable.

use std::fs;
use std::path::Path;

use super::common::create_config;
use fallow_types::results::AnalysisResults;

#[test]
fn supabase_edge_functions_handle_deno_schemes_and_roots() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_supabase_project(root);

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // AC1 + AC4: jsr:, npm:, and URL imports produce no unresolved-import noise.
    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "Deno scheme/URL imports should not be unresolved, found: {unresolved:?}"
    );

    // AC1 + AC2: no unlisted-dependency noise. `@supabase/supabase-js` is
    // declared, `zod` is imported only via `npm:` (self-declaring), and the
    // `jsr:@std/path` import never reaches dependency accounting.
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        unlisted.is_empty(),
        "Deno scheme imports should not be unlisted dependencies, found: {unlisted:?}"
    );

    // AC2: a declared dependency imported via `npm:` is credited, not unused.
    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"@supabase/supabase-js"),
        "npm:@supabase/supabase-js should credit the declared dependency, unused: {unused_deps:?}"
    );

    // AC5: the Supabase CLI is invoked from scripts, credited as tooling.
    let unused_dev_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev_deps.contains(&"supabase"),
        "supabase CLI should be credited as tooling, unused dev: {unused_dev_deps:?}"
    );

    // AC3: function entry + shared code reachable; orphan outside the functions
    // tree stays reportable.
    let unused_files = unused_file_paths(&results, root);
    assert!(
        !unused_files.contains(&"supabase/functions/hello/index.ts".to_string()),
        "Supabase function entry should be a runtime root, unused: {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"supabase/functions/_shared/cors.ts".to_string()),
        "shared code imported by a function should be reachable, unused: {unused_files:?}"
    );
    assert!(
        unused_files.contains(&"src/orphan.ts".to_string()),
        "an orphan outside supabase/functions should still report as unused, unused: {unused_files:?}"
    );
}

fn write_supabase_project(root: &Path) {
    write_file(
        root,
        "package.json",
        r#"{
            "name": "supabase-edge-fixture",
            "private": true,
            "dependencies": {
                "@supabase/supabase-js": "^2.0.0"
            },
            "devDependencies": {
                "supabase": "^1.0.0"
            }
        }"#,
    );
    write_file(root, "supabase/config.toml", "project_id = \"fixture\"\n");
    write_file(
        root,
        "supabase/functions/hello/index.ts",
        r#"
            import { serve } from "https://deno.land/std@0.168.0/http/server.ts";
            import * as path from "jsr:@std/path";
            import { createClient } from "npm:@supabase/supabase-js@2";
            import { z } from "npm:zod@3";
            import { corsHeaders } from "../_shared/cors.ts";

            serve(() => {
                const client = createClient("url", "key");
                return new Response(z.string().parse(path.basename("/a/b")), {
                    headers: { ...corsHeaders, client: String(!!client) },
                });
            });
        "#,
    );
    write_file(
        root,
        "supabase/functions/_shared/cors.ts",
        r#"export const corsHeaders = { "Access-Control-Allow-Origin": "*" };
"#,
    );
    write_file(root, "src/orphan.ts", "export const orphan = true;\n");
}

fn write_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dir");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn unused_file_paths(results: &AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&finding.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}
