//! `audit` attribution of a finding on an import of a workspace package.
//!
//! The package manager links each workspace package into `node_modules`, and
//! `node_modules` is gitignored. The base worktree shares the `node_modules` of
//! the checkout under review, so the workspace links of the base point at the
//! packages of that checkout, outside the base root. The import must still
//! reach the package of the base, or the base misses each finding on that
//! edge and the head reports the finding as introduced.

#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use crate::common::{commit_all, git, parse_json, run_fallow_raw};

/// An `app` zone that may import only the public entry of the `lib` zone.
const BOUNDARIES: &str = r#"{
  "entry": ["src/main.ts"],
  "rules": {"boundary-violation": "warn"},
  "boundaries": {
    "zones": [
      {"name": "lib-public", "patterns": ["packages/lib/src/index.ts"]},
      {"name": "lib", "patterns": ["packages/lib/src/**"]},
      {"name": "app", "patterns": ["src/**"]}
    ],
    "rules": [{"from": "app", "allow": ["lib-public"]}]
  }
}"#;

const LIB_MANIFEST: &str = r#"{"name": "@acme/lib", "exports": {".": "./src/index.ts", "./internal": "./src/internal.ts"}}"#;

fn audit_against_head(dir: &Path) -> serde_json::Value {
    let output = run_fallow_raw(&[
        "audit",
        "--root",
        dir.to_str().unwrap(),
        "--base",
        "HEAD",
        "--format",
        "json",
        "--quiet",
    ]);
    parse_json(&output)
}

/// The `lib` workspace package and an importer that reaches a private module
/// of `lib` through the package name.
fn fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("packages/lib/src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "workspace-link", "private": true, "workspaces": ["packages/*"], "dependencies": {"@acme/lib": "*"}}"#,
    )
    .unwrap();
    fs::write(dir.join(".gitignore"), "node_modules\n.fallow\n").unwrap();
    fs::write(dir.join(".fallowrc.json"), BOUNDARIES).unwrap();
    fs::write(dir.join("packages/lib/package.json"), LIB_MANIFEST).unwrap();
    fs::write(
        dir.join("packages/lib/src/index.ts"),
        "export const api = 1;\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/lib/src/internal.ts"),
        "export const secret = 2;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.ts"),
        "import { api } from '@acme/lib';\nimport { secret } from '@acme/lib/internal';\nconsole.log(api, secret);\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("node_modules/@acme")).unwrap();
    std::os::unix::fs::symlink("../../packages/lib", dir.join("node_modules/@acme/lib")).unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "baseline");
    tmp
}

#[test]
fn a_boundary_violation_through_a_workspace_package_link_stays_inherited() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(
        dir.join("src/main.ts"),
        "import { api } from '@acme/lib';\nimport { secret } from '@acme/lib/internal';\nconsole.log(api, secret, 3);\n",
    )
    .unwrap();

    let json = audit_against_head(dir);

    let violations = json["dead_code"]["boundary_violations"]
        .as_array()
        .unwrap_or_else(|| panic!("boundary violations missing: {json}"));
    assert_eq!(violations.len(), 1, "{json}");
    assert_eq!(violations[0]["to_path"], "packages/lib/src/internal.ts");
    assert_eq!(violations[0]["introduced"], false, "{json}");
    assert_eq!(json["attribution"]["dead_code_introduced"], 0, "{json}");
    assert_eq!(json["verdict"], "pass", "{json}");
}

/// pnpm installs `"lib-alias": "workspace:@acme/lib@*"` as the workspace
/// package `@acme/lib` under the name `lib-alias`. The base must resolve the
/// alias to its own `packages/lib` too.
#[test]
fn a_boundary_violation_through_a_workspace_alias_link_stays_inherited() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::create_dir_all(dir.join("packages/lib/src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "workspace-link", "private": true, "workspaces": ["packages/*"], "dependencies": {"lib-alias": "workspace:@acme/lib@*"}}"#,
    )
    .unwrap();
    fs::write(dir.join(".gitignore"), "node_modules\n.fallow\n").unwrap();
    fs::write(dir.join(".fallowrc.json"), BOUNDARIES).unwrap();
    fs::write(dir.join("packages/lib/package.json"), LIB_MANIFEST).unwrap();
    fs::write(
        dir.join("packages/lib/src/index.ts"),
        "export const api = 1;\n",
    )
    .unwrap();
    fs::write(
        dir.join("packages/lib/src/internal.ts"),
        "export const secret = 2;\n",
    )
    .unwrap();
    fs::write(
        dir.join("src/main.ts"),
        "import { secret } from 'lib-alias/internal';\nconsole.log(secret);\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("node_modules")).unwrap();
    std::os::unix::fs::symlink("../packages/lib", dir.join("node_modules/lib-alias")).unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "baseline");
    fs::write(
        dir.join("src/main.ts"),
        "import { secret } from 'lib-alias/internal';\nconsole.log(secret, 3);\n",
    )
    .unwrap();

    let json = audit_against_head(dir);

    let violations = json["dead_code"]["boundary_violations"]
        .as_array()
        .unwrap_or_else(|| panic!("boundary violations missing: {json}"));
    assert_eq!(violations.len(), 1, "{json}");
    assert_eq!(violations[0]["to_path"], "packages/lib/src/internal.ts");
    assert_eq!(violations[0]["introduced"], false, "{json}");
    assert_eq!(json["attribution"]["dead_code_introduced"], 0, "{json}");
}

/// A SvelteKit app in a workspace package. Its `tsconfig.json` extends the
/// generated and gitignored `.svelte-kit/tsconfig.json`, which holds the path
/// aliases of the app. The base must load that alias too, or `values.ts` is
/// an unused file on the base and its unused export `d` looks introduced.
#[test]
fn an_unused_export_behind_a_generated_svelte_kit_alias_stays_inherited() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    let app = dir.join("packages/app");
    fs::create_dir_all(app.join("src/lib")).unwrap();
    fs::create_dir_all(app.join(".svelte-kit")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "workspace-link", "private": true}"#,
    )
    .unwrap();
    fs::write(
        dir.join("pnpm-workspace.yaml"),
        "packages:\n  - 'packages/*'\n",
    )
    .unwrap();
    fs::write(
        dir.join(".gitignore"),
        "node_modules\n.fallow\n.svelte-kit\n",
    )
    .unwrap();
    fs::write(
        app.join("package.json"),
        r#"{"name": "@acme/app", "private": true, "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(
        app.join("tsconfig.json"),
        r#"{"extends": "./.svelte-kit/tsconfig.json"}"#,
    )
    .unwrap();
    fs::write(
        app.join(".svelte-kit/tsconfig.json"),
        r#"{"compilerOptions": {"paths": {"~lib/*": ["../src/lib/*"]}}}"#,
    )
    .unwrap();
    fs::write(
        app.join("src/lib/values.ts"),
        "export const c = 1;\nexport const d = 2;\n",
    )
    .unwrap();
    fs::write(
        app.join("src/index.ts"),
        "import { c } from '~lib/values';\nconsole.log(c);\n",
    )
    .unwrap();
    git(dir, &["init", "-b", "main"]);
    commit_all(dir, "baseline");
    fs::write(
        app.join("src/index.ts"),
        "import { c } from '~lib/values';\nconsole.log(c, 3);\n",
    )
    .unwrap();

    let json = audit_against_head(dir);

    // `values.ts` is not a changed file, so the audit reports its unused
    // export only when the change created it.
    let unused_exports = json["dead_code"]["unused_exports"]
        .as_array()
        .unwrap_or_else(|| panic!("unused exports missing: {json}"));
    assert!(
        unused_exports
            .iter()
            .all(|export| export["introduced"] != true),
        "{json}"
    );
    assert_eq!(json["attribution"]["dead_code_introduced"], 0, "{json}");
    assert_eq!(json["verdict"], "pass", "{json}");
}
