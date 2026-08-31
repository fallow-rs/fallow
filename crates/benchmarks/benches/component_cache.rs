#![expect(
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

//! Extraction cache component shard.
//!
//! Covers the three costs that make up the warm-run `parse_extract_ms` window
//! before any AST work happens: writing `cache.bin`, reading and decoding it,
//! and converting every `CachedModule` back into a `ModuleInfo`.
//!
//! The fixture is fully deterministic. Every module source is derived from its
//! index, with import specifiers, imported symbol names, and relative paths
//! drawn from small fixed vocabularies so the same strings repeat across
//! modules the way they do in a real project.
//!
//! `component_cache_store_save` includes an fsync through the atomic-write
//! path, so its local wall-clock numbers are noisier than the other two cases.

use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_extract::cache::{
    CacheStore, CachedModule, DEFAULT_CACHE_MAX_SIZE, cached_to_module_opts, module_to_cached,
};
use fallow_types::discover::FileId;
use fallow_types::source_fingerprint::SourceFingerprint;

/// Module count and per-module body sizes are tuned together so the encoded
/// store lands near 2.5 MB, the middle of the representative band asserted
/// below and the same order as a real 900-module TypeScript project.
const MODULE_COUNT: usize = 700;
const HELPER_FUNCTIONS: usize = 2;
const ASYNC_FUNCTIONS: usize = 1;
const CONFIG_HASH: u64 = 7;
const BASE_MTIME_NS: u64 = 1_700_000_000_000_000_000;

const PACKAGES: [&str; 40] = [
    "react",
    "react-dom",
    "zod",
    "lodash-es",
    "@tanstack/react-query",
    "date-fns",
    "clsx",
    "axios",
    "zustand",
    "immer",
    "rxjs",
    "uuid",
    "yup",
    "formik",
    "redux",
    "@reduxjs/toolkit",
    "next",
    "next/router",
    "vue",
    "pinia",
    "svelte",
    "solid-js",
    "preact",
    "graphql",
    "@apollo/client",
    "urql",
    "swr",
    "ky",
    "ofetch",
    "nanoid",
    "chalk",
    "commander",
    "minimist",
    "picocolors",
    "fast-glob",
    "globby",
    "tslib",
    "type-fest",
    "ts-pattern",
    "effect",
];

const SYMBOLS: [&str; 24] = [
    "useEffect",
    "useState",
    "useMemo",
    "useCallback",
    "useRef",
    "useContext",
    "z",
    "clsx",
    "format",
    "parseISO",
    "debounce",
    "throttle",
    "merge",
    "cloneDeep",
    "mapValues",
    "filterBy",
    "reduceBy",
    "createStore",
    "produce",
    "nanoid",
    "request",
    "serialize",
    "deserialize",
    "validate",
];

const RELATIVE_ROOTS: [&str; 8] = [
    "lib",
    "utils",
    "hooks",
    "components",
    "services",
    "models",
    "helpers",
    "state",
];

const RELATIVE_LEAVES: [&str; 10] = [
    "date", "string", "number", "array", "object", "http", "auth", "cache", "logger", "config",
];

fn package_specifier(index: usize) -> &'static str {
    PACKAGES[index % PACKAGES.len()]
}

fn symbol(index: usize) -> &'static str {
    SYMBOLS[index % SYMBOLS.len()]
}

fn relative_specifier(index: usize) -> String {
    let root = RELATIVE_ROOTS[index % RELATIVE_ROOTS.len()];
    let leaf = RELATIVE_LEAVES[(index / RELATIVE_ROOTS.len()) % RELATIVE_LEAVES.len()];
    format!("../{root}/{leaf}")
}

fn import_lines(index: usize, lines: &mut Vec<String>) {
    for slot in 0..6 {
        let package = package_specifier(index * 7 + slot * 3);
        let named = symbol(index * 5 + slot);
        let aliased = symbol(index * 5 + slot + 7);
        lines.push(format!(
            "import {{ {named}, {aliased} as {aliased}Alias{slot} }} from \"{package}\";"
        ));
    }
    lines.push(format!(
        "import defaultBinding{index} from \"{}\";",
        relative_specifier(index)
    ));
    lines.push(format!(
        "import helperBinding{index} from \"{}\";",
        relative_specifier(index + 13)
    ));
    lines.push(format!(
        "import * as namespaceBinding{index} from \"{}\";",
        package_specifier(index * 3 + 1)
    ));
    lines.push(format!(
        "import \"{}\";",
        relative_specifier(index * 11 + 5)
    ));
    lines.push(format!(
        "import {{ Status }} from \"{}\";",
        relative_specifier(index + 29)
    ));
    lines.push(String::new());
}

fn re_export_lines(index: usize, lines: &mut Vec<String>) {
    lines.push(format!(
        "export {{ {} as reExported{index} }} from \"{}\";",
        symbol(index * 3 + 2),
        package_specifier(index * 9 + 4)
    ));
    lines.push(format!(
        "export {{ {} as forwarded{index} }} from \"{}\";",
        symbol(index * 3 + 11),
        relative_specifier(index + 41)
    ));
    lines.push(String::new());
}

fn constant_lines(index: usize, lines: &mut Vec<String>) {
    lines.push(format!("export const RETRY_LIMIT_{index} = {};", index % 7));
    lines.push(format!(
        "export const FEATURE_KEY_{index} = \"feature-{}\";",
        index % 40
    ));
    lines.push(format!("export const DEFAULT_OPTIONS_{index} = {{"));
    lines.push("  mode: \"strict\",".to_owned());
    lines.push(format!("  retries: {},", index % 5));
    lines.push(format!("  label: \"module-{index}\","));
    lines.push("  verbose: false,".to_owned());
    lines.push("};".to_owned());
    lines.push(String::new());
}

fn interface_lines(index: usize, lines: &mut Vec<String>) {
    lines.push(format!("export interface ModuleContext{index} {{"));
    lines.push("  id: string;".to_owned());
    lines.push("  label: string;".to_owned());
    lines.push("  retries: number;".to_owned());
    lines.push("  tags: string[];".to_owned());
    lines.push("  createdAt: string;".to_owned());
    lines.push("  nested: { key: string; value: number };".to_owned());
    lines.push("}".to_owned());
    lines.push(String::new());
    lines.push(format!(
        "export type ModuleKey{index} = keyof ModuleContext{index};"
    ));
    lines.push(format!(
        "export type ModuleHandler{index} = (context: ModuleContext{index}) => Promise<void>;"
    ));
    lines.push(String::new());
}

fn helper_function_lines(index: usize, slot: usize, lines: &mut Vec<String>) {
    lines.push(format!(
        "export function computeScore{index}_{slot}(context: ModuleContext{index}, weight: number): number {{"
    ));
    lines.push("  let total = 0;".to_owned());
    lines.push(format!("  if (context.retries > RETRY_LIMIT_{index}) {{"));
    lines.push("    total += weight * 2;".to_owned());
    lines.push("  } else if (context.retries > 0) {".to_owned());
    lines.push("    total += weight;".to_owned());
    lines.push("  }".to_owned());
    lines.push("  for (const tag of context.tags) {".to_owned());
    lines.push("    if (tag.startsWith(\"beta\") && weight > 1) {".to_owned());
    lines.push("      total += 1;".to_owned());
    lines.push("    }".to_owned());
    lines.push("    if (tag === Status.Active) {".to_owned());
    lines.push("      total += 3;".to_owned());
    lines.push("    } else if (tag === Status.Archived) {".to_owned());
    lines.push("      total -= 1;".to_owned());
    lines.push("    }".to_owned());
    lines.push("  }".to_owned());
    lines.push("  const mode = context.label.length > 8 ? \"long\" : \"short\";".to_owned());
    lines.push("  if (process.env.NODE_ENV === \"production\" && mode === \"long\") {".to_owned());
    lines.push(format!(
        "    console.warn(\"score\", total, {}(context.label));",
        symbol(index + slot)
    ));
    lines.push("  }".to_owned());
    lines.push("  return total > 0 ? total : 0;".to_owned());
    lines.push("}".to_owned());
    lines.push(String::new());
}

fn async_function_lines(index: usize, slot: usize, lines: &mut Vec<String>) {
    lines.push(format!(
        "export async function loadModule{index}_{slot}(key: string): Promise<number> {{"
    ));
    lines.push(format!(
        "  const loaded = await import(\"{}\");",
        relative_specifier(index + slot * 17)
    ));
    lines.push("  if (!loaded) {".to_owned());
    lines.push("    return 0;".to_owned());
    lines.push("  }".to_owned());
    lines.push(format!(
        "  const vendor = await import(\"{}\");",
        package_specifier(index * 5 + slot)
    ));
    lines.push("  if (key.length > 3 && vendor) {".to_owned());
    lines.push(format!(
        "    return defaultBinding{index}(loaded, key).length;"
    ));
    lines.push("  }".to_owned());
    lines.push("  return key.length;".to_owned());
    lines.push("}".to_owned());
    lines.push(String::new());
}

fn class_lines(index: usize, lines: &mut Vec<String>) {
    lines.push(format!("export class ModuleController{index} {{"));
    lines.push(format!("  private readonly retries = RETRY_LIMIT_{index};"));
    lines.push("  constructor(private readonly label: string) {}".to_owned());
    lines.push("  handle(value: number): number {".to_owned());
    lines.push("    if (value > this.retries) {".to_owned());
    lines.push("      return value - this.retries;".to_owned());
    lines.push("    }".to_owned());
    lines.push("    return value;".to_owned());
    lines.push("  }".to_owned());
    lines.push("  describe(): string {".to_owned());
    lines.push(format!(
        "    return `${{Status.Active}}-${{this.label}}-${{FEATURE_KEY_{index}}}`;"
    ));
    lines.push("  }".to_owned());
    lines.push("}".to_owned());
    lines.push(String::new());
    lines.push(format!(
        "export default helperBinding{index}(namespaceBinding{index});"
    ));
    lines.push(String::new());
}

fn create_module_source(index: usize) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(220);
    import_lines(index, &mut lines);
    re_export_lines(index, &mut lines);
    constant_lines(index, &mut lines);
    interface_lines(index, &mut lines);
    for slot in 0..HELPER_FUNCTIONS {
        helper_function_lines(index, slot, &mut lines);
    }
    for slot in 0..ASYNC_FUNCTIONS {
        async_function_lines(index, slot, &mut lines);
    }
    class_lines(index, &mut lines);
    lines.join("\n")
}

struct CacheFixture {
    dir: tempfile::TempDir,
    paths: Vec<PathBuf>,
    modules: Vec<CachedModule>,
}

fn create_cache_fixture() -> CacheFixture {
    let dir = tempfile::tempdir().expect("cache fixture tempdir");
    let root = dir.path().to_path_buf();
    let mut paths = Vec::with_capacity(MODULE_COUNT);
    let mut modules = Vec::with_capacity(MODULE_COUNT);
    for index in 0..MODULE_COUNT {
        let path = root.join(format!("src/feature-{}/module-{index}.ts", index % 40));
        let source = create_module_source(index);
        let file_id = FileId(u32::try_from(index).expect("fixture index fits in u32"));
        let module = fallow_extract::parse_from_content(file_id, &path, &source);
        let fingerprint = SourceFingerprint::new(
            BASE_MTIME_NS + u64::try_from(index).expect("fixture index fits in u64"),
            u64::try_from(source.len()).expect("fixture source length fits in u64"),
        );
        modules.push(module_to_cached(&module, fingerprint));
        paths.push(path);
    }
    CacheFixture {
        dir,
        paths,
        modules,
    }
}

fn populated_store(fixture: &CacheFixture) -> CacheStore {
    let mut store = CacheStore::new();
    for (path, module) in fixture.paths.iter().zip(fixture.modules.iter()) {
        store.insert(path, module.clone());
    }
    store
}

/// Fail loudly when a bitcode packing change or a shrunken fixture would make
/// this shard unrepresentative of the real warm-cache decode window.
fn assert_representative_size(cache_dir: &Path) {
    let size = std::fs::metadata(cache_dir.join("cache.bin"))
        .expect("cache.bin metadata")
        .len();
    assert!(
        (1_000_000..=4_000_000).contains(&size),
        "component_cache fixture must stay representative: cache.bin is {size} bytes"
    );
}

fn component_cache_store_save(c: &mut Criterion) {
    let fixture = create_cache_fixture();
    let mut store = populated_store(&fixture);
    store
        .save(fixture.dir.path(), CONFIG_HASH, DEFAULT_CACHE_MAX_SIZE)
        .expect("fixture save");
    assert_representative_size(fixture.dir.path());

    // Repeating `save` on the same store is safe and does not mutate entries:
    // eviction only runs when the encoded size passes 80% of `max_size_bytes`,
    // and a ~2 MB store against the 256 MB default never gets close. Do not
    // "fix" this into `iter_batched`; the batched setup would hide the encode.
    c.bench_function("component_cache_store_save", |bencher| {
        bencher.iter(|| store.save(fixture.dir.path(), CONFIG_HASH, DEFAULT_CACHE_MAX_SIZE));
    });
}

fn component_cache_store_load(c: &mut Criterion) {
    let fixture = create_cache_fixture();
    let mut store = populated_store(&fixture);
    store
        .save(fixture.dir.path(), CONFIG_HASH, DEFAULT_CACHE_MAX_SIZE)
        .expect("fixture save");
    assert_representative_size(fixture.dir.path());

    c.bench_function("component_cache_store_load", |bencher| {
        bencher.iter(|| {
            CacheStore::load(fixture.dir.path(), CONFIG_HASH, DEFAULT_CACHE_MAX_SIZE)
                .expect("warm cache loads")
        });
    });
}

fn component_cache_cached_to_module(c: &mut Criterion) {
    let fixture = create_cache_fixture();
    let mut store = populated_store(&fixture);
    store
        .save(fixture.dir.path(), CONFIG_HASH, DEFAULT_CACHE_MAX_SIZE)
        .expect("fixture save");
    assert_representative_size(fixture.dir.path());
    let paths = &fixture.paths;

    // The store is loaded inside `iter_batched` setup on purpose: a store
    // hoisted out of the loop would let a lazily decoding implementation pay
    // its decode once and measure pure conversion afterwards, which is not
    // comparable against an eagerly decoding one.
    c.bench_function("component_cache_cached_to_module", |bencher| {
        bencher.iter_batched(
            || {
                CacheStore::load(fixture.dir.path(), CONFIG_HASH, DEFAULT_CACHE_MAX_SIZE)
                    .expect("warm cache loads")
            },
            |store| {
                let mut modules = Vec::with_capacity(paths.len());
                for (index, path) in paths.iter().enumerate() {
                    let cached = store.get_by_path_only(path).expect("entry present");
                    modules.push(cached_to_module_opts(
                        cached,
                        FileId(u32::try_from(index).expect("fixture index fits in u32")),
                        true,
                    ));
                }
                modules
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    component_cache_store_save,
    component_cache_store_load,
    component_cache_cached_to_module
);
criterion_main!(benches);
