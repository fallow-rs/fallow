#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Allocation tracking benchmark using dhat.
//!
//! This benchmark measures heap allocation statistics for the fallow analysis
//! pipeline. It uses a dedicated harness because dhat requires being the global
//! allocator.
//!
//! Run with: `cargo bench --bench allocations`
//!
//! Output is printed in the machine-parseable `key: value` format that
//! `.github/workflows/allocs.yml` parses.

#![expect(
    deprecated,
    reason = "Core-internal policy: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

mod helpers;
#[path = "../../benchmarks/benches/support/threads.rs"]
mod threads;

use std::fmt::Write as _;
use std::path::Path;

/// Source modules in the fixture. Half of them are reachable from the entry.
const MODULE_COUNT: usize = 100;
/// Modules behind each barrel file.
const BARREL_SIZE: usize = 10;

/// Run the analysis once under dhat and print the heap statistics.
///
/// The allocs workflow sets `RAYON_NUM_THREADS=1`. With one thread the counts
/// are almost identical between runs, so the CI alert can use a tight
/// threshold. The global rayon pool and the directory walk both follow that
/// variable.
fn main() {
    let temp_dir = tempfile::Builder::new()
        .prefix("fallow-bench-alloc-bench-")
        .tempdir()
        .unwrap();
    write_realistic_fixture(temp_dir.path());
    let mut config = helpers::make_config(temp_dir.path().to_path_buf(), true);
    config.threads = threads::bench_threads();

    let profiler = dhat::Profiler::builder().testing().build();

    let results = fallow_core::analyze(&config).expect("analysis of the fixture succeeds");

    let stats = dhat::HeapStats::get();
    drop(profiler);

    assert_fixture_coverage(temp_dir.path(), &results);

    #[expect(
        clippy::print_stdout,
        reason = "intentional bench output that the allocs workflow parses"
    )]
    {
        println!("alloc_total_bytes: {}", stats.total_bytes);
        println!("alloc_total_blocks: {}", stats.total_blocks);
        println!("alloc_max_bytes: {}", stats.max_bytes);
        println!("alloc_max_blocks: {}", stats.max_blocks);
    }
}

/// Write a project that exercises the common import shapes, not only
/// single-binding imports:
///
/// - multi-binding and type-only imports,
/// - barrel files with named, type and star re-exports,
/// - a namespace import,
/// - a global stylesheet and a CSS module with an unused class,
/// - one Vue single-file component.
fn write_realistic_fixture(root: &Path) {
    let src = root.join("src");
    std::fs::create_dir_all(src.join("styles")).unwrap();
    std::fs::create_dir_all(src.join("components")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name": "alloc-bench", "main": "src/index.ts", "dependencies": {"react": "^18", "vue": "^3"}}"#,
    )
    .unwrap();

    for i in 0..MODULE_COUNT {
        let content = format!(
            r"
export const value{i} = {i};
export function fn{i}() {{ return {i}; }}
export type Type{i} = {{ value: number }};
export const helper{i} = () => value{i} + 1;
"
        );
        std::fs::write(src.join(format!("module{i}.ts")), content).unwrap();
    }

    let used_count = MODULE_COUNT / 2;
    let mut index = String::new();
    index.push_str("import './styles/global.css';\n");
    index.push_str("import styles from './styles/button.module.css';\n");
    index.push_str("import Card from './components/Card.vue';\n");
    index.push_str("import * as ns from './module49';\n");
    for barrel in 0..used_count / BARREL_SIZE {
        let first = barrel * BARREL_SIZE;
        let last = first + BARREL_SIZE - 1;
        let mut body = String::new();
        for i in first..last {
            writeln!(body, "export {{ value{i}, fn{i} }} from './module{i}';").unwrap();
            writeln!(body, "export type {{ Type{i} }} from './module{i}';").unwrap();
        }
        writeln!(body, "export * from './module{last}';").unwrap();
        std::fs::write(src.join(format!("barrel{barrel}.ts")), body).unwrap();

        let names: Vec<String> = (first..last)
            .flat_map(|i| [format!("value{i}"), format!("fn{i}")])
            .chain([format!("helper{last}")])
            .collect();
        writeln!(
            index,
            "import {{ {} }} from './barrel{barrel}';",
            names.join(", ")
        )
        .unwrap();
        writeln!(
            index,
            "import type {{ Type{first} }} from './barrel{barrel}';"
        )
        .unwrap();
        writeln!(index, "console.log({});", names.join(", ")).unwrap();
        writeln!(
            index,
            "export const typed{barrel}: Type{first} = {{ value: {first} }};"
        )
        .unwrap();
    }
    index.push_str("console.log(styles.primary, styles.large, ns.value49, Card);\n");
    std::fs::write(src.join("index.ts"), index).unwrap();

    std::fs::write(
        src.join("styles/global.css"),
        "@import './tokens.css';\nbody { margin: 0; }\n/* layout */\n.page { display: grid; }\n",
    )
    .unwrap();
    std::fs::write(
        src.join("styles/tokens.css"),
        ":root { --space: 4px; --radius: 2px; }\n",
    )
    .unwrap();
    std::fs::write(
        src.join("styles/button.module.css"),
        ".primary { color: blue; }\n.large { padding: var(--space); }\n/* .ghost in a comment */\n.ghost { opacity: 0.5; }\n",
    )
    .unwrap();
    std::fs::write(
        src.join("components/Card.vue"),
        r#"<script setup lang="ts">
import { value0, fn1 } from '../barrel0';
import type { Type2 } from '../module2';
const props = defineProps<{ title: string; item: Type2 }>();
const total = value0 + fn1();
</script>

<template>
  <div class="card">
    <h2>{{ props.title }}</h2>
    <span>{{ total }}</span>
  </div>
</template>

<style scoped>
.card { border: 1px solid; }
</style>
"#,
    )
    .unwrap();
}

/// Fail the run when the fixture stops reaching the CSS, SFC and re-export
/// paths. A silent drop of one of these paths would make the counts look
/// better without a real improvement.
fn assert_fixture_coverage(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let relative = |path: &std::path::Path| -> String {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        path.strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let unused_exports: Vec<(String, String)> = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                relative(&finding.export.path),
                finding.export.export_name.clone(),
            )
        })
        .collect();
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(&finding.file.path))
        .collect();

    assert!(
        results.unresolved_imports.is_empty(),
        "every fixture import resolves"
    );
    assert!(
        unused_exports.contains(&(
            "src/styles/button.module.css".to_owned(),
            "ghost".to_owned()
        )),
        "the unused CSS module class is reported: {unused_exports:?}"
    );
    assert!(
        !unused_exports
            .iter()
            .any(|(path, name)| path == "src/module0.ts" && name == "fn0"),
        "an export used through a barrel re-export is not reported"
    );
    assert!(
        unused_files.contains(&"src/module99.ts".to_owned()),
        "the dead half of the modules is still reported: {unused_files:?}"
    );
    assert!(
        unused_exports.contains(&("src/module9.ts".to_owned(), "value9".to_owned())),
        "an export behind an unused star re-export is reported: {unused_exports:?}"
    );
    assert!(
        !unused_files.iter().any(|path| path.ends_with("Card.vue")),
        "the SFC is reachable from the entry point: {unused_files:?}"
    );
}
