//! Edges that reference a file without running it.
//!
//! `require.resolve('./file')` gives only the path of the file, and an asset
//! loader request (`raw-loader!./file.js`) gives its text, bytes or a URL.
//! Neither edge runs the target. The file stays in use, but the edge does not
//! cross an architecture boundary, does not leak server code into a client
//! bundle, and a re-export through a loader does not forward the directive
//! context of its resource. A package that an asset loader reads is used at
//! build time, so it is not a production import.

use std::path::Path;

use fallow_config::{FallowConfig, OutputFormat};
use fallow_core::results::AnalysisResults;

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
    std::fs::write(path, content).expect("write file");
}

fn analyze_with_config_file(root: &Path) -> AnalysisResults {
    let (loaded, _) = FallowConfig::find_and_load(root)
        .expect("config discovery should succeed")
        .expect("the project has a config file");
    let config = loaded.resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true, None);
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn non_loading_edges_do_not_cross_a_boundary() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name":"boundary-edges","devDependencies":{"raw-loader":"^4.0.2"}}"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{
  "entry": ["src/ui/App.js", "src/ui/Bad.js"],
  "boundaries": {
    "zones": [
      { "name": "ui", "patterns": ["src/ui/**"] },
      { "name": "server", "patterns": ["src/server/**"] }
    ],
    "rules": [{ "from": "ui", "allow": [] }]
  }
}"#,
    );
    write(
        root,
        "src/ui/App.js",
        "export const jobPath = require.resolve('../server/job.js');\n\
         export { default as template } from 'raw-loader!../server/template.js';\n\
         import text from '!raw-loader!../server/notes.js';\n\
         export const notes = text;\n",
    );
    write(
        root,
        "src/ui/Bad.js",
        "import { run } from '../server/real.js';\nexport const bad = run;\n",
    );
    for name in ["job", "template", "notes", "real"] {
        write(
            root,
            &format!("src/server/{name}.js"),
            "export const run = () => 1;\nexport default run;\n",
        );
    }

    let results = analyze_with_config_file(root);
    let violations: Vec<(String, String)> = results
        .boundary_violations
        .iter()
        .map(|finding| {
            (
                relative(root, &finding.violation.from_path),
                relative(root, &finding.violation.to_path),
            )
        })
        .collect();
    assert_eq!(
        violations,
        [(
            "src/ui/Bad.js".to_string(),
            "src/server/real.js".to_string()
        )],
        "a path reference and an asset loader edge load nothing, so only the static import crosses the boundary"
    );
}

#[test]
fn non_loading_edges_do_not_leak_server_code_into_a_client_bundle() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name":"leak-edges","dependencies":{"next":"15.0.0","react":"19.0.0"},"devDependencies":{"raw-loader":"^4.0.2"}}"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{
  "entry": ["src/client-path.tsx", "src/client-text.tsx", "src/client-bad.tsx"],
  "rules": { "security-client-server-leak": "warn" }
}"#,
    );
    write(
        root,
        "src/client-path.tsx",
        "'use client';\nexport const workerPath = require.resolve('./server-path.ts');\n",
    );
    write(
        root,
        "src/client-text.tsx",
        "'use client';\nimport source from 'raw-loader!./server-text.ts';\nexport const text = source;\n",
    );
    write(
        root,
        "src/client-bad.tsx",
        "'use client';\nimport { url } from './server-real.ts';\nexport const bad = url;\n",
    );
    for name in ["server-path", "server-text", "server-real"] {
        write(
            root,
            &format!("src/{name}.ts"),
            "export const url = process.env.DATABASE_URL;\n",
        );
    }

    let results = analyze_with_config_file(root);
    let mut leaking: Vec<String> = results
        .security_findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.kind,
                fallow_core::results::SecurityFindingKind::ClientServerLeak
            )
        })
        .map(|finding| relative(root, &finding.path))
        .collect();
    leaking.sort();
    leaking.dedup();
    assert_eq!(
        leaking,
        ["src/client-bad.tsx"],
        "a path reference and an asset loader edge put no server code in the client bundle"
    );
}

#[test]
fn a_loader_re_export_is_not_a_client_or_server_origin() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name":"barrel-edges","dependencies":{"next":"15.0.0","react":"19.0.0","server-only":"0.0.1"},"devDependencies":{"raw-loader":"^4.0.2","worker-loader":"^3.0.8"}}"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{ "entry": ["src/loaders.ts", "src/plain.ts"] }"#,
    );
    write(
        root,
        "src/loaders.ts",
        "export * from 'raw-loader!./server.ts';\n\
         export { default as Worker } from 'worker-loader!./client-worker.ts';\n\
         export { Button } from './button.tsx';\n\
         export { default as source } from '!raw-loader!./other-server.ts';\n",
    );
    write(
        root,
        "src/plain.ts",
        "export { Button as PlainButton } from './button.tsx';\nexport { fetchUser } from './server.ts';\n",
    );
    write(
        root,
        "src/server.ts",
        "import 'server-only';\nexport const fetchUser = () => 1;\n",
    );
    write(
        root,
        "src/other-server.ts",
        "import 'server-only';\nexport default 1;\n",
    );
    write(
        root,
        "src/client-worker.ts",
        "'use client';\nexport default class W {}\n",
    );
    write(
        root,
        "src/button.tsx",
        "'use client';\nexport const Button = () => null;\n",
    );

    let results = analyze_with_config_file(root);
    let barrels: Vec<String> = results
        .mixed_client_server_barrels
        .iter()
        .map(|finding| relative(root, &finding.barrel.path))
        .collect();
    assert_eq!(
        barrels,
        ["src/plain.ts"],
        "a re-export through a loader forwards loader output, not the directive context of the resource"
    );
}

#[test]
fn a_package_that_an_asset_loader_reads_is_not_a_production_import() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write(
        root,
        "package.json",
        r#"{"name":"asset-package","main":"src/index.js","devDependencies":{"notes-pkg":"^1.0.0","worker-pkg":"^1.0.0","runtime-dev":"^1.0.0","raw-loader":"^4.0.2","worker-loader":"^3.0.8"}}"#,
    );
    write(root, ".fallowrc.json", "{}");
    write(
        root,
        "src/index.js",
        "import notes from '!raw-loader!notes-pkg/notes.txt';\n\
         import Work from 'worker-loader!worker-pkg/work.js';\n\
         import 'runtime-dev';\n\
         export default [notes, new Work()];\n",
    );

    let results = analyze_with_config_file(root);
    let mut dev_in_production: Vec<&str> = results
        .dev_dependencies_in_production
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    dev_in_production.sort_unstable();
    assert_eq!(
        dev_in_production,
        ["runtime-dev", "worker-pkg"],
        "an asset loader reads notes-pkg at build time; a worker loader runs worker-pkg"
    );
    let unused_dev: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        unused_dev.is_empty(),
        "every package is used, also the one behind an asset loader: {unused_dev:?}"
    );
}
