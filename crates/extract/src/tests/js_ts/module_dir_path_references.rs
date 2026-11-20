//! A path that code builds from the directory of the current module names a
//! project file: `path.join(__dirname, "scripts", "build.mjs")`.
//!
//! The base is `__dirname`, `import.meta.dirname`, `path.dirname` of the
//! module file, or a `const` that holds a join of one of them. The callee is
//! any `join` or `resolve`, because the base already proves a path join. Only
//! a path that ends in a script extension is a reference, so a directory path
//! never resolves to an index file. A process call with a static `cwd` option
//! references the script files in its argument array.

use crate::tests::parse_ts as parse_source;

fn references(source: &str) -> Vec<String> {
    parse_source(source)
        .dynamic_imports
        .iter()
        .map(|import| import.source.clone())
        .collect()
}

#[test]
fn a_join_on_dirname_references_the_file() {
    let source = "import * as NodePath from 'node:path';\n\
         export const agent = NodePath.join(__dirname, '../provider/fixtures/mock-agent.mjs');\n";
    assert_eq!(references(source), ["../provider/fixtures/mock-agent.mjs"]);
}

#[test]
fn a_join_with_several_segments_references_the_file() {
    let source = "const path = require('node:path');\n\
         const { pathToFileURL } = require('node:url');\n\
         export const load = () =>\n\
           import(pathToFileURL(path.join(__dirname, 'scripts', 'generate.mts')).href);\n";
    assert_eq!(references(source), ["./scripts/generate.mts"]);
}

#[test]
fn a_resolve_on_import_meta_dirname_references_the_file() {
    let source = "import { resolve } from 'node:path';\n\
         export const peer = resolve(import.meta.dirname, '../test/peer.ts');\n";
    assert_eq!(references(source), ["../test/peer.ts"]);
}

#[test]
fn a_join_of_another_path_library_references_the_file() {
    let source = "export function* run(Path) {\n\
           const path = yield* Path.Path;\n\
           return path.join(import.meta.dirname, 'fixtures/peer.mjs');\n\
         }\n";
    assert_eq!(references(source), ["./fixtures/peer.mjs"]);
}

#[test]
fn a_const_directory_is_a_base_in_a_nested_scope() {
    let source = "export function* run(Path) {\n\
           const path = yield* Path.Path;\n\
           const fixtureDir = path.join(import.meta.dirname, 'testFixtures');\n\
           return path.join(fixtureDir, 'peer.mjs');\n\
         }\n";
    assert_eq!(references(source), ["./testFixtures/peer.mjs"]);
}

#[test]
fn a_directory_path_is_not_a_reference() {
    let source = "import path from 'node:path';\n\
         export const dir = path.join(__dirname, 'src');\n\
         export const data = path.join(__dirname, 'data.json');\n";
    assert!(
        references(source).is_empty(),
        "a path without a script extension must not resolve to an index file"
    );
}

#[test]
fn a_join_on_another_base_is_not_a_reference() {
    let source = "import path from 'node:path';\n\
         export const sign = (repoRoot) => path.join(repoRoot, 'scripts/sign.ts');\n\
         export const tmp = path.join(process.cwd(), 'scripts/sign.ts');\n";
    assert!(
        references(source).is_empty(),
        "a base that is not the module directory is unknown before run time"
    );
}

#[test]
fn a_dynamic_segment_is_not_a_reference() {
    let source = "import path from 'node:path';\n\
         export const file = (name) => path.join(__dirname, name, 'run.ts');\n";
    assert!(references(source).is_empty());
}

#[test]
fn a_const_directory_is_a_base_only_in_its_scope() {
    let source = "import path from 'node:path';\n\
         export const a = () => { const dir = path.join(__dirname, 'a'); return path.join(dir, 'x.ts'); };\n\
         export const b = () => { const dir = path.join(__dirname, 'b'); return path.join(dir, 'x.ts'); };\n\
         export const c = (os) => { const dir = os.tmpdir(); return path.join(dir, 'x.ts'); };\n";
    assert_eq!(references(source), ["./a/x.ts", "./b/x.ts"]);
}

#[test]
fn a_parameter_shadows_a_const_directory() {
    let source = "import path from 'node:path';\n\
         const root = path.join(__dirname, 'a');\n\
         export const f = (root) => path.join(root, 'x.ts');\n\
         export const g = () => path.join(root, 'y.ts');\n";
    assert_eq!(references(source), ["./a/y.ts"]);
}

#[test]
fn an_import_or_a_require_of_the_path_credits_the_module() {
    let source = "import path from 'node:path';\n\
         export const a = () => import(path.join(__dirname, 'a.mjs'));\n\
         export const b = require(path.join(__dirname, 'b.js'));\n\
         export const c = path.join(__dirname, 'c.js');\n";
    let info = parse_source(source);
    let credits: Vec<(&str, bool)> = info
        .dynamic_imports
        .iter()
        .map(|import| (import.source.as_str(), import.local_name.is_some()))
        .collect();
    assert_eq!(
        credits,
        [("./a.mjs", true), ("./b.js", true), ("./c.js", false)],
        "only a consumer that loads the module credits its exports"
    );
}

#[test]
fn a_fork_of_a_const_path_is_recorded_one_time() {
    let source = "import { fork } from 'node:child_process';\n\
         import path from 'node:path';\n\
         const CHILD = path.join(__dirname, 'child.js');\n\
         export const run = () => fork(CHILD);\n\
         export const load = () => import(path.join(__dirname, 'child.js'));\n";
    let info = parse_source(source);
    let credits: Vec<(&str, bool)> = info
        .dynamic_imports
        .iter()
        .map(|import| (import.source.as_str(), import.local_name.is_some()))
        .collect();
    assert_eq!(
        credits,
        [("./child.js", false), ("./child.js", true)],
        "the fork replaces the reference of the declaration and keeps the import"
    );
}

#[test]
fn a_fork_target_is_recorded_one_time() {
    let source = "import { fork } from 'node:child_process';\n\
         import path from 'node:path';\n\
         fork(path.join(__dirname, 'worker.js'));\n";
    assert_eq!(references(source), ["./worker.js"]);
}

#[test]
fn a_process_call_with_a_static_cwd_references_its_script_arguments() {
    let source = "import * as NodeChildProcess from 'node:child_process';\n\
         import * as NodePath from 'node:path';\n\
         import * as NodeURL from 'node:url';\n\
         const REPO_ROOT = NodePath.resolve(NodePath.dirname(NodeURL.fileURLToPath(import.meta.url)), '..');\n\
         export const run = (out) =>\n\
           NodeChildProcess.spawn(process.execPath, ['scripts/behavior/run.ts', '--out', out], {\n\
             cwd: REPO_ROOT,\n\
             stdio: 'inherit',\n\
           });\n";
    assert_eq!(references(source), ["../scripts/behavior/run.ts"]);
}

#[test]
fn a_process_call_without_a_static_cwd_references_nothing() {
    let source = "import { spawn } from 'node:child_process';\n\
         export const a = () => spawn('node', ['scripts/run.ts']);\n\
         export const b = (cwd) => spawn('node', ['scripts/run.ts'], { cwd });\n";
    assert!(
        references(source).is_empty(),
        "the working directory is unknown before run time"
    );
}

#[test]
fn a_join_of_a_local_path_binding_is_a_reference_and_not_a_fork() {
    let info = parse_source(
        "import { fork } from 'node:child_process';\n\
         import path from 'node:path';\n\
         function run(path) { fork(path.join(__dirname, 'child.js')); }",
    );

    let sources: Vec<&str> = info
        .dynamic_imports
        .iter()
        .map(|import| import.source.as_str())
        .collect();
    assert_eq!(sources, ["./child.js"]);
    let out_of_thread = info.semantic_facts.iter().any(|fact| {
        matches!(
            fact,
            fallow_types::extract::SemanticFact::ImportLoadKindOverride(fact)
                if fact.kind == fallow_types::extract::ImportLoadKind::OutOfThread
        )
    });
    assert!(
        !out_of_thread,
        "a local path binding is not node:path, so the call is not a fork of the file"
    );
}
