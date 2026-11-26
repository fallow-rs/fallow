//! Boundary presets that keep callers on the public entry files of a module:
//! the `deep-modules` preset and the `presetPublicFiles` opt-in of the
//! `bulletproof` and `feature-sliced` presets.

use std::path::Path;

use crate::common::{
    commit_all, fallow_bin, git, parse_json, run_fallow_in_root, run_fallow_raw_in_dir,
    scrub_analysis_env,
};

fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(path, body).expect("write file");
}

/// The sorted `from -> to` pairs of the boundary violations of a run.
fn violation_pairs(root: &Path) -> Vec<String> {
    let output = run_fallow_in_root(
        "architecture",
        root,
        &["--boundaries", "--format", "json", "--quiet"],
    );
    assert_ne!(output.code, 2, "run failed: {}", output.stderr);
    let json = parse_json(&output);
    let mut pairs: Vec<String> = json["boundary_violations"]
        .as_array()
        .expect("boundary_violations array")
        .iter()
        .map(|v| {
            format!(
                "{} -> {}",
                v["from_path"].as_str().expect("from_path"),
                v["to_path"].as_str().expect("to_path")
            )
        })
        .collect();
    pairs.sort();
    pairs
}

fn write_config(root: &Path, boundaries: &str) {
    write(
        root,
        ".fallowrc.json",
        &format!(
            r#"{{
  "entry": ["src/main.ts", "src/**/*.test.ts", "tests/**/*.test.ts"],
  "boundaries": {boundaries}
}}"#
        ),
    );
}

/// A project with two modules under `module_root`, a loose file under the
/// root, a colocated test and an out-of-tree test.
fn deep_modules_project(module_root: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let m = module_root;
    write(
        root,
        "package.json",
        r#"{ "name": "deep-modules-fixture" }"#,
    );
    write_config(root, r#"{ "preset": "deep-modules" }"#);
    write(
        root,
        "src/main.ts",
        &format!(
            "import {{ charge }} from './{m}/billing';\n\
             import {{ secret }} from './{m}/billing/index.internal';\n\
             import {{ draft }} from './{m}/orders/internal';\n\
             import {{ helper }} from './{m}/shared';\n\
             charge(draft + secret + helper);\n"
        ),
    );
    write(
        root,
        &format!("src/{m}/billing/index.ts"),
        "export { charge } from './charge';\n",
    );
    write(
        root,
        &format!("src/{m}/billing/index.internal.ts"),
        "export const secret = 1;\n",
    );
    write(
        root,
        &format!("src/{m}/billing/charge.ts"),
        "export const charge = (n: number): number => n;\n",
    );
    write(
        root,
        &format!("src/{m}/billing/charge.test.ts"),
        "import { charge } from './charge';\ncharge(1);\n",
    );
    write(
        root,
        &format!("src/{m}/orders/index.ts"),
        "import { charge } from '../billing';\nexport { draft } from './internal';\ncharge(0);\n",
    );
    write(
        root,
        &format!("src/{m}/orders/internal.ts"),
        "export const draft = 2;\n",
    );
    write(
        root,
        &format!("src/{m}/shared.ts"),
        "import { charge } from './billing/charge';\nexport const helper = charge(3);\n",
    );
    write(
        root,
        "tests/orders.test.ts",
        &format!(
            "import {{ charge }} from '../src/{m}/billing';\n\
             import {{ draft }} from '../src/{m}/orders/internal';\n\
             charge(draft);\n"
        ),
    );
    dir
}

fn expected_deep_modules_violations(m: &str) -> Vec<String> {
    let mut expected = vec![
        format!("src/main.ts -> src/{m}/billing/index.internal.ts"),
        format!("src/main.ts -> src/{m}/orders/internal.ts"),
        format!("src/{m}/shared.ts -> src/{m}/billing/charge.ts"),
        format!("tests/orders.test.ts -> src/{m}/orders/internal.ts"),
    ];
    expected.sort();
    expected
}

#[test]
fn deep_modules_keeps_callers_on_the_index_file() {
    let dir = deep_modules_project("modules");
    assert_eq!(
        violation_pairs(dir.path()),
        expected_deep_modules_violations("modules"),
        "an import through index.ts and a colocated test are allowed; an \
         outside import of an internal file, of index.internal.ts and an \
         out-of-tree test are violations"
    );
}

#[test]
fn deep_modules_falls_back_to_the_features_folder() {
    let dir = deep_modules_project("features");
    assert_eq!(
        violation_pairs(dir.path()),
        expected_deep_modules_violations("features"),
    );
}

#[test]
fn deep_modules_prefers_the_modules_folder() {
    let dir = deep_modules_project("modules");
    write(
        dir.path(),
        "src/features/legacy/a.ts",
        "export const a = 1;\n",
    );
    write(
        dir.path(),
        "src/features/legacy/b.ts",
        "import { a } from './a';\nexport const b = a;\n",
    );
    assert_eq!(
        violation_pairs(dir.path()),
        expected_deep_modules_violations("modules"),
    );
}

#[test]
fn deep_modules_without_a_module_folder_exits_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "no-modules" }"#);
    write_config(root, r#"{ "preset": "deep-modules" }"#);
    write(root, "src/main.ts", "export const a = 1;\n");

    let output = run_fallow_in_root("check", root, &["--quiet"]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        output.stderr.contains("src/modules") && output.stderr.contains("src/features"),
        "the error names both folders: {}",
        output.stderr
    );
}

#[test]
fn deep_modules_lists_the_public_files() {
    let dir = deep_modules_project("modules");
    let output = run_fallow_in_root("list", dir.path(), &["--boundaries"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    let text = format!("{}{}", output.stdout, output.stderr);
    assert!(
        text.contains("modules/billing") && text.contains("index.{ts,tsx,mts,cts,js,jsx,mjs,cjs}"),
        "the listing shows each module zone and its public files: {text}"
    );
}

fn bulletproof_project(boundaries: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "bulletproof-fixture" }"#);
    write_config(root, boundaries);
    write(
        root,
        "src/main.ts",
        "import { page } from './app/page';\npage();\n",
    );
    write(
        root,
        "src/app/page.ts",
        "import { login } from '../features/auth';\n\
         import { token } from '../features/auth/api/token';\n\
         export const page = (): string => login(token);\n",
    );
    write(
        root,
        "src/features/auth/index.ts",
        "export { login } from './login';\n",
    );
    write(
        root,
        "src/features/auth/login.ts",
        "export const login = (t: string): string => t;\n",
    );
    write(
        root,
        "src/features/auth/api/token.ts",
        "export const token = 'x';\n",
    );
    dir
}

#[test]
fn bulletproof_default_findings_do_not_change() {
    let dir = bulletproof_project(r#"{ "preset": "bulletproof" }"#);
    assert_eq!(violation_pairs(dir.path()), Vec::<String>::new());
}

#[test]
fn bulletproof_opt_in_keeps_callers_on_the_feature_index() {
    let dir = bulletproof_project(r#"{ "preset": "bulletproof", "presetPublicFiles": true }"#);
    assert_eq!(
        violation_pairs(dir.path()),
        vec!["src/app/page.ts -> src/features/auth/api/token.ts".to_owned()],
    );
}

fn feature_sliced_project(boundaries: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "fsd-fixture" }"#);
    write_config(root, boundaries);
    write(
        root,
        "src/main.ts",
        "import { page } from './pages/home';\npage();\n",
    );
    write(
        root,
        "src/pages/home/index.ts",
        "import { login } from '../../features/login';\n\
         import { model } from '../../features/login/model';\n\
         import { user } from '../../entities/user/model/user';\n\
         export const page = (): string => login + model + user;\n",
    );
    write(
        root,
        "src/features/login/index.ts",
        "export { model as login } from './model';\n",
    );
    write(
        root,
        "src/features/login/model.ts",
        "import { forOrder } from '../../entities/user/@x/order';\nexport const model = forOrder;\n",
    );
    write(
        root,
        "src/entities/user/model/user.ts",
        "export const user = 'u';\n",
    );
    write(
        root,
        "src/entities/user/@x/order.ts",
        "export const forOrder = 'o';\n",
    );
    dir
}

#[test]
fn feature_sliced_default_findings_do_not_change() {
    let dir = feature_sliced_project(r#"{ "preset": "feature-sliced" }"#);
    assert_eq!(violation_pairs(dir.path()), Vec::<String>::new());
}

#[test]
fn feature_sliced_opt_in_keeps_callers_on_the_slice_public_api() {
    let dir =
        feature_sliced_project(r#"{ "preset": "feature-sliced", "presetPublicFiles": true }"#);
    assert_eq!(
        violation_pairs(dir.path()),
        vec![
            "src/pages/home/index.ts -> src/entities/user/model/user.ts".to_owned(),
            "src/pages/home/index.ts -> src/features/login/model.ts".to_owned(),
        ],
    );
}

#[test]
fn preset_public_files_without_a_supporting_preset_exits_2() {
    for boundaries in [
        r#"{ "preset": "layered", "presetPublicFiles": true }"#,
        r#"{ "presetPublicFiles": true, "zones": [{ "name": "a", "patterns": ["src/**"] }] }"#,
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(root, "package.json", r#"{ "name": "skew" }"#);
        write_config(root, boundaries);
        write(root, "src/main.ts", "export const a = 1;\n");
        let output = run_fallow_in_root("check", root, &["--quiet"]);
        assert_eq!(output.code, 2, "{boundaries}: {}", output.stderr);
        assert!(
            output.stderr.contains("presetPublicFiles"),
            "{boundaries}: {}",
            output.stderr
        );
    }
}

#[test]
fn deep_modules_does_not_warn_for_its_empty_optional_zones() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "quiet-zones" }"#);
    write_config(
        root,
        r#"{ "preset": "deep-modules", "zones": [{ "name": "scripts", "patterns": ["scripts/**"] }] }"#,
    );
    write(
        root,
        "src/modules/billing/index.ts",
        "export const charge = 1;\n",
    );
    write(
        root,
        "src/main.ts",
        "import { charge } from './modules/billing';\nconsole.log(charge);\n",
    );

    let mut cmd = std::process::Command::new(fallow_bin());
    cmd.env_remove("RUST_LOG").env("NO_COLOR", "1");
    scrub_analysis_env(&mut cmd);
    cmd.args(["architecture", "--boundaries", "--no-cache", "--root"])
        .arg(root);
    let stderr = String::from_utf8_lossy(&cmd.output().expect("run fallow").stderr).to_string();
    for zone in ["modules", "app", "tests"] {
        assert!(
            !stderr.contains(&format!("boundary zone '{zone}' matched 0 files")),
            "the preset zone '{zone}' may be empty: {stderr}"
        );
    }
    assert!(
        stderr.contains("boundary zone 'scripts' matched 0 files"),
        "a user zone still warns: {stderr}"
    );
}

#[test]
fn deep_modules_keeps_the_files_of_user_zones() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "user-zones" }"#);
    write_config(
        root,
        r#"{
    "preset": "deep-modules",
    "zones": [
      { "name": "modules", "patterns": ["src/domains/**"], "autoDiscover": ["src/domains"], "publicFiles": ["index.ts"] },
      { "name": "shared", "patterns": ["src/shared/**"] }
    ],
    "rules": [{ "from": "shared", "allow": [] }]
  }"#,
    );
    write(
        root,
        "src/main.ts",
        "import { charge } from './domains/billing/charge';\nimport { s } from './shared/s';\nconsole.log(charge, s);\n",
    );
    write(
        root,
        "src/domains/billing/index.ts",
        "export { charge } from './charge';\n",
    );
    write(
        root,
        "src/domains/billing/charge.ts",
        "export const charge = 1;\n",
    );
    write(
        root,
        "src/shared/s.ts",
        "import { charge } from '../domains/billing';\nexport const s = charge;\n",
    );
    assert_eq!(
        violation_pairs(root),
        vec![
            "src/main.ts -> src/domains/billing/charge.ts".to_owned(),
            "src/shared/s.ts -> src/domains/billing/index.ts".to_owned(),
        ],
        "the replaced modules zone and the shared zone keep their files: the \
         internal import is reported, and the shared rule applies"
    );
}

#[test]
fn deep_modules_audit_accepts_a_base_without_the_module_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(root, "package.json", r#"{ "name": "audit-base" }"#);
    write(root, "src/main.ts", "export const a = 1;\n");
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "core.autocrlf", "false"]);
    commit_all(root, "base");
    git(root, &["checkout", "-b", "head"]);
    write_config(root, r#"{ "preset": "deep-modules" }"#);
    write(
        root,
        "src/modules/billing/index.ts",
        "export { charge } from './charge';\n",
    );
    write(
        root,
        "src/modules/billing/charge.ts",
        "export const charge = 1;\n",
    );
    write(
        root,
        "src/main.ts",
        "import { charge } from './modules/billing/charge';\nexport const a = charge;\n",
    );
    commit_all(root, "head");

    let root_arg = root.to_str().expect("utf-8 root");
    let output = run_fallow_raw_in_dir(
        root,
        &[
            "audit", "--root", root_arg, "--base", "main", "--format", "json", "--quiet",
            "--no-css",
        ],
    );
    assert!(
        output.code == 0 || output.code == 1,
        "the base revision has no module folder, but the audit runs. stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
    let json = parse_json(&output);
    let violations = json["dead_code"]["boundary_violations"]
        .as_array()
        .expect("boundary_violations array");
    assert_eq!(violations.len(), 1, "{json}");
    assert_eq!(violations[0]["to_path"], "src/modules/billing/charge.ts");
}
