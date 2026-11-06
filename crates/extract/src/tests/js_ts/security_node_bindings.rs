use crate::tests::parse_ts as parse_source;

fn sink_paths(source: &str) -> Vec<String> {
    parse_source(source)
        .security_sinks
        .iter()
        .map(|sink| sink.callee_path.clone())
        .collect()
}

fn assert_sink_path(source: &str, expected: &str) {
    let paths = sink_paths(source);
    assert!(
        paths.iter().any(|path| path == expected),
        "expected {expected}, observed {paths:?}"
    );
}

#[test]
fn default_import_of_child_process_gives_the_node_path() {
    assert_sink_path(
        "import cp from 'node:child_process';\nexport const run = (cmd: string) => cp.exec(cmd);\n",
        "node:child_process.exec",
    );
}

#[test]
fn namespace_import_of_child_process_gives_the_node_path() {
    assert_sink_path(
        "import * as cp from 'child_process';\nexport const run = (cmd: string) => cp.execSync(cmd);\n",
        "node:child_process.execSync",
    );
}

#[test]
fn default_and_namespace_imports_of_vm_give_the_node_path() {
    assert_sink_path(
        "import vm from 'vm';\nexport const run = (code: string) => vm.runInNewContext(code);\n",
        "node:vm.runInNewContext",
    );
    assert_sink_path(
        "import * as nodeVm from 'node:vm';\nexport const run = (code: string) => nodeVm.runInThisContext(code);\n",
        "node:vm.runInThisContext",
    );
}

#[test]
fn default_and_namespace_imports_of_fs_keep_the_written_path() {
    assert_sink_path(
        "import fs from 'node:fs';\nexport const read = (p: string) => fs.readFile(p, () => {});\n",
        "fs.readFile",
    );
    assert_sink_path(
        "import * as fs from 'fs/promises';\nexport const read = (p: string) => fs.readFile(p);\n",
        "fs.readFile",
    );
}

#[test]
fn a_parameter_hides_a_namespace_import() {
    let paths = sink_paths(
        "import * as cp from 'child_process';\nexport const run = (cp: { exec(c: string): void }, cmd: string) => cp.exec(cmd);\n",
    );
    assert!(
        !paths.iter().any(|path| path.starts_with("node:")),
        "a parameter must hide the import binding, observed {paths:?}"
    );
}

#[test]
fn type_only_namespace_import_gives_no_node_path() {
    let paths = sink_paths(
        "import type * as cp from 'child_process';\ndeclare const proc: typeof cp;\nexport const run = (cmd: string) => proc.exec(cmd);\n",
    );
    assert!(
        !paths.iter().any(|path| path.starts_with("node:")),
        "a type-only import binds no runtime value, observed {paths:?}"
    );
}

#[test]
fn destructured_require_records_the_local_name_of_each_property() {
    let info = parse_source(
        "const { exec: run, execSync, spawn: start = noop, [key]: computed, ...rest } = require('child_process');\n",
    );
    let call = &info.require_calls[0];
    assert_eq!(
        call.destructured_locals,
        [
            ("exec".to_string(), "run".to_string()),
            ("execSync".to_string(), "execSync".to_string()),
            ("spawn".to_string(), "start".to_string()),
        ]
    );
}

#[test]
fn member_binding_with_a_further_path_gives_the_node_path() {
    assert_sink_path(
        "import { promises as fsp } from 'node:fs';\nexport const read = (p: string) => fsp.readFile(p);\n",
        "node:fs.readFile",
    );
    assert_sink_path(
        "const { promises: fsp } = require('fs');\nexport const read = (p: string) => fsp.writeFile(p, '');\n",
        "node:fs.writeFile",
    );
}
