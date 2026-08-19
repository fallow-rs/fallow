#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]
#![allow(
    clippy::significant_drop_tightening,
    reason = "the external Criterion macro owns the benchmark lifecycle"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, DeadCodeOptions, DuplicationMode,
    DuplicationOptions, EditorAnalysisSession, EngineHealthRunner, FeatureFlagsOptions,
    run_circular_dependencies, run_combined, run_feature_flags, run_health_with_runner,
};
use fallow_cli::{
    InspectBenchmarkCorpus, benchmark_dead_code_json, benchmark_fix_dry_run,
    benchmark_inspect_file_evidence_bundle_json, benchmark_list_json, benchmark_recommend_json,
    benchmark_rule_pack_test_json, benchmark_runtime_coverage_analyze_json,
    benchmark_security_json, benchmark_viz_html, create_inspect_benchmark_corpus,
};
use fallow_engine::{module_graph::impact_closure_for_changed_paths, session::AnalysisSession};
use fallow_extract::{
    cache::{CacheStore, module_to_cached},
    parse_all_files, parse_single_file,
};
use fallow_types::discover::{DiscoveredFile, FileId};
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;
const DEAD_CODE_FINDING_COUNT: usize = FIX_FILE_COUNT;
const FIX_FILE_COUNT: usize = 128;
const IMPACT_LAYER_COUNT: usize = 32;
const IMPACT_LAYER_WIDTH: usize = 16;
const INSPECT_CHILD_CALL_COUNT: usize = 6;
const LIST_FILE_COUNT: usize = 128;
const LIST_WORKSPACE_COUNT: usize = 8;
// Each workspace index is reported once as a default index and once from its
// package metadata, preserving both production entry-point sources.
const LIST_ENTRY_POINT_COUNT: usize = LIST_WORKSPACE_COUNT * 2;
const RECOMMEND_DECISION_COUNT: usize = 13;
const RECOMMEND_FRAMEWORK_COUNT: usize = 5;
const RECOMMEND_WORKSPACE_COUNT: usize = 64;
const RULE_PACK_FILE_COUNT: usize = 64;
const RULE_PACK_FINDINGS_PER_FILE: usize = 4;
const RUNTIME_COVERAGE_FILE_COUNT: usize = 128;
const RUNTIME_COVERAGE_FINDING_COUNT: usize = RUNTIME_COVERAGE_FILE_COUNT;
const RUNTIME_COVERAGE_HOT_PATH_COUNT: usize = RUNTIME_COVERAGE_FILE_COUNT / 2;
const SECURITY_FILE_COUNT: usize = 128;
const VIZ_MODULE_COUNT: usize = 64;

struct CommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
}

struct InspectCommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
    corpus: InspectBenchmarkCorpus,
}

struct ExtractCacheInput {
    _temp_dir: TempDir,
    files: Vec<DiscoveredFile>,
    cache: CacheStore,
}

struct EditorSessionInput {
    _temp_dir: TempDir,
    session: EditorAnalysisSession,
}

struct RuntimeCoverageInput {
    _temp_dir: TempDir,
    root: PathBuf,
    coverage_path: PathBuf,
    response_bytes: Vec<u8>,
}

fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    fs::write(path, source.as_ref()).unwrap();
}

fn create_inspect_project() -> InspectCommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"inspect-benchmark","type":"module"}"#,
    );
    write_file(
        &root,
        "src/target.ts",
        "export const target = (value: number) => value + 1;\n",
    );
    let corpus = create_inspect_benchmark_corpus(&root, BENCH_THREADS);
    InspectCommandInput {
        _temp_dir: temp_dir,
        root,
        corpus,
    }
}

fn analysis_options(root: &Path, no_cache: bool) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache,
        threads: Some(BENCH_THREADS),
        ..AnalysisOptions::default()
    }
}

fn is_source_path(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return false;
    };

    matches!(extension, "css" | "js" | "jsx" | "ts" | "tsx")
}

fn collect_source_paths(dir: &Path, paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("benchmark fixture directory is readable") {
        let entry = entry.expect("benchmark fixture entry is readable");
        let path = entry.path();
        if path.is_dir() {
            collect_source_paths(&path, paths);
        } else if is_source_path(&path) {
            paths.push(path);
        }
    }
}

fn discovered_source_files(root: &Path) -> Vec<DiscoveredFile> {
    let mut paths = Vec::new();
    collect_source_paths(root, &mut paths);
    paths.sort();

    paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| DiscoveredFile {
            id: FileId(u32::try_from(index).expect("benchmark fixture file count fits in u32")),
            size_bytes: fs::metadata(&path)
                .expect("benchmark fixture metadata is readable")
                .len(),
            path,
        })
        .collect()
}

fn create_workspace_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-workspace",
  "private": true,
  "packageManager": "pnpm@10.0.0",
  "workspaces": ["apps/*", "packages/*"],
  "dependencies": {}
}"#,
    );
    write_file(
        &root,
        "pnpm-workspace.yaml",
        r#"
packages:
  - "apps/*"
  - "packages/*"
"#,
    );
    write_file(
        &root,
        "apps/web/package.json",
        r#"{"name":"@bench/web","main":"src/index.ts","dependencies":{"@bench/config":"workspace:*","@bench/shared":"workspace:*","@bench/ui":"workspace:*"}}"#,
    );
    write_file(
        &root,
        "apps/admin/package.json",
        r#"{"name":"@bench/admin","main":"src/index.ts","dependencies":{"@bench/shared":"workspace:*","@bench/ui":"workspace:*"}}"#,
    );
    write_file(
        &root,
        "packages/shared/package.json",
        r#"{"name":"@bench/shared","main":"src/index.ts"}"#,
    );
    write_file(
        &root,
        "packages/ui/package.json",
        r#"{"name":"@bench/ui","main":"src/index.ts","dependencies":{"react":"19.0.0"}}"#,
    );
    write_file(
        &root,
        "packages/config/package.json",
        r#"{"name":"@bench/config","main":"src/index.ts"}"#,
    );
    write_file(
        &root,
        "apps/web/src/index.ts",
        r#"
import { featureFlags } from "@bench/config";
import { formatUser } from "@bench/shared";
import { Card } from "@bench/ui";

export const render = (name: string) => Card({ title: `${formatUser(name)}:${featureFlags.checkout}` });
"#,
    );
    write_file(
        &root,
        "apps/admin/src/index.ts",
        r#"
import { formatUser } from "@bench/shared";
import { Card } from "@bench/ui";

export const renderAdmin = (name: string) => Card({ title: `admin:${formatUser(name)}` });
"#,
    );
    write_file(
        &root,
        "packages/shared/src/index.ts",
        r"
export const formatUser = (name: string): string => name.trim();
export const unusedSharedHelper = (name: string): string => name.toUpperCase();
",
    );
    write_file(
        &root,
        "packages/ui/src/index.ts",
        r#"
export const Card = ({ title }: { title: string }) => `<section>${title}</section>`;
export const UnusedCard = () => "<section>unused</section>";
"#,
    );
    write_file(
        &root,
        "packages/config/src/index.ts",
        r#"
export const featureFlags = { checkout: "new" } as const;
export const unusedExperiment = { search: "legacy" } as const;
"#,
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_warm_hash_workspace_project() -> ExtractCacheInput {
    let CommandInput {
        _temp_dir: temp_dir,
        root,
    } = create_workspace_project();
    let files = discovered_source_files(&root);
    let mut cache = CacheStore::new();

    for file in &files {
        let module = parse_single_file(file).expect("benchmark fixture parses");
        let cached = module_to_cached(
            &module,
            fallow_types::source_fingerprint::SourceFingerprint::new(1, 1),
        );
        cache.insert(&file.path, cached);
    }

    ExtractCacheInput {
        _temp_dir: temp_dir,
        files,
        cache,
    }
}

fn create_health_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-health-service",
  "private": true,
  "type": "module",
  "dependencies": {},
  "devDependencies": {
    "typescript": "5.8.0"
  }
}"#,
    );
    let mut source = String::from(
        r"
export function scoreOrder(input: { status: string; amount: number; flags: string[] }): number {
  let score = 0;
",
    );
    for i in 0..40 {
        writeln!(
            &mut source,
            r#"  if (input.flags.includes("flag{i}")) {{
    score += input.amount > {i} ? {i} : -{i};
  }}"#
        )
        .unwrap();
    }
    source.push_str(
        r#"
  if (input.status === "blocked") {
    return -score;
  }
  return score;
}
"#,
    );
    write_file(&root, "src/score.ts", source);
    write_file(
        &root,
        "src/index.ts",
        r#"
import { scoreOrder } from "./score";

console.log(scoreOrder({ status: "open", amount: 10, flags: ["flag1"] }));
"#,
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_circular_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-circulars",
  "private": true,
  "type": "module",
  "dependencies": {},
  "devDependencies": {
    "typescript": "5.8.0"
  }
}"#,
    );
    for domain in ["orders", "billing", "users"] {
        for index in 0..10 {
            let next = (index + 1) % 10;
            write_file(
                &root,
                &format!("src/domains/{domain}/node{index}.ts"),
                format!(
                    r#"
import {{ value{next} }} from "./node{next}";

export const value{index} = value{next} + {index};
"#
                ),
            );
        }
        write_file(
            &root,
            &format!("src/domains/{domain}/index.ts"),
            r#"export { value0 } from "./node0";"#,
        );
    }
    write_file(
        &root,
        "src/index.ts",
        r#"
import { value0 as orderValue } from "./domains/orders";
import { value0 as billingValue } from "./domains/billing";
import { value0 as userValue } from "./domains/users";

console.log(orderValue, billingValue, userValue);
"#,
    );

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_feature_flags_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-feature-flags",
  "private": true,
  "type": "module",
  "main": "src/index.ts",
  "dependencies": {
    "launchdarkly-node-server-sdk": "9.10.1"
  }
}"#,
    );

    let mut index_source = String::new();
    for index in 0..32 {
        writeln!(
            &mut index_source,
            "import {{ evaluate{index} }} from \"./features/feature{index}\";"
        )
        .unwrap();
        write_file(
            &root,
            &format!("src/features/feature{index}.ts"),
            format!(
                r#"
declare function useFlag(name: string): boolean;

export function evaluate{index}(): boolean {{
  const sdkEnabled = useFlag("checkout-{index}");
  if (process.env.FEATURE_CHECKOUT_{index}) {{
    return sdkEnabled;
  }}
  return false;
}}

export const unusedFallback{index} = (): boolean => false;
"#
            ),
        );
    }
    index_source.push_str("\nconsole.log(");
    for index in 0..32 {
        if index > 0 {
            index_source.push_str(", ");
        }
        write!(&mut index_source, "evaluate{index}").unwrap();
    }
    index_source.push_str(");\n");
    write_file(&root, "src/index.ts", index_source);

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_impact_closure_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-impact-closure",
  "private": true,
  "type": "module",
  "main": "src/index.ts"
}"#,
    );

    for layer in 0..IMPACT_LAYER_COUNT {
        for column in 0..IMPACT_LAYER_WIDTH {
            let source = if layer == 0 {
                format!("export const value{layer}_{column} = {column};\n")
            } else {
                let previous = layer - 1;
                format!(
                    "import {{ value{previous}_{column} }} from \"../layer{previous}/module{column}\";\n\
                     export const value{layer}_{column} = value{previous}_{column} + {layer};\n"
                )
            };
            write_file(
                &root,
                &format!("src/layer{layer}/module{column}.ts"),
                source,
            );
        }
    }

    let final_layer = IMPACT_LAYER_COUNT - 1;
    let mut index_source = String::new();
    for column in 0..IMPACT_LAYER_WIDTH {
        writeln!(
            &mut index_source,
            "import {{ value{final_layer}_{column} }} from \"./layer{final_layer}/module{column}\";"
        )
        .unwrap();
    }
    index_source.push_str("console.log(");
    for column in 0..IMPACT_LAYER_WIDTH {
        if column > 0 {
            index_source.push_str(", ");
        }
        write!(&mut index_source, "value{final_layer}_{column}").unwrap();
    }
    index_source.push_str(");\n");
    write_file(&root, "src/index.ts", index_source);

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_fix_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-fix-preview",
  "private": true,
  "type": "module",
  "main": "src/index.ts"
}"#,
    );

    let mut index_source = String::new();
    for index in 0..FIX_FILE_COUNT {
        writeln!(
            &mut index_source,
            "import {{ used{index} }} from \"./features/feature{index}\";"
        )
        .unwrap();
        writeln!(&mut index_source, "console.log(used{index});").unwrap();
        write_file(
            &root,
            &format!("src/features/feature{index}.ts"),
            format!(
                r"
export const used{index} = {index};
export const unused{index} = {index} * 2;
"
            ),
        );
    }
    write_file(&root, "src/index.ts", index_source);

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_security_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-security-framework-sinks",
  "private": true,
  "type": "module",
  "dependencies": { "express": "5.1.0" }
}"#,
    );

    for index in 0..SECURITY_FILE_COUNT {
        write_file(
            &root,
            &format!("src/routes/route{index}.ts"),
            format!(
                r#"
declare const app: {{
  post(path: string, handler: (req: unknown) => void): void;
}};

app.post("/run/{index}", (req) => {{
  eval(req);
  eval(req.body);
}});
"#
            ),
        );
    }

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_rule_pack_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-rule-pack-test",
  "private": true,
  "type": "module",
  "main": "src/index.ts",
  "dependencies": { "moment": "2.30.1" }
}"#,
    );
    write_file(
        &root,
        ".fallowrc.json",
        r#"{
  "rules": { "policy-violation": "warn" },
  "rulePacks": ["rule-packs/benchmark.jsonc"]
}"#,
    );
    write_file(
        &root,
        "rule-packs/benchmark.jsonc",
        r#"{
  "version": 1,
  "name": "benchmark-policy",
  "rules": [
    { "id": "no-console", "kind": "banned-call", "callees": ["console.log"] },
    { "id": "no-moment", "kind": "banned-import", "specifiers": ["moment"] },
    { "id": "no-network", "kind": "banned-effect", "effects": ["network"] },
    { "id": "no-legacy-api", "kind": "banned-export", "exports": ["legacyApi"] }
  ]
}"#,
    );

    let mut index_source = String::new();
    for index in 0..RULE_PACK_FILE_COUNT {
        writeln!(
            &mut index_source,
            "import {{ run as run{index} }} from \"./features/feature{index}\";"
        )
        .unwrap();
        write_file(
            &root,
            &format!("src/features/feature{index}.ts"),
            format!(
                r#"
import moment from "moment";

export const legacyApi = moment;
export function run(): Promise<Response> {{
  console.log("feature-{index}");
  return fetch("/api/feature-{index}");
}}
"#
            ),
        );
    }
    index_source.push_str("\nvoid Promise.all([\n");
    for index in 0..RULE_PACK_FILE_COUNT {
        writeln!(&mut index_source, "  run{index}(),").unwrap();
    }
    index_source.push_str("]);\n");
    write_file(&root, "src/index.ts", index_source);

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_recommend_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-recommend-workspace",
  "private": true,
  "packageManager": "pnpm@10.0.0",
  "workspaces": ["packages/*"]
}"#,
    );
    write_file(
        &root,
        "pnpm-workspace.yaml",
        "packages:\n  - \"packages/*\"\n",
    );
    write_file(&root, "tsconfig.json", "{}\n");
    write_file(&root, ".storybook/main.ts", "export default {};\n");

    let frameworks = ["next", "react", "vue", "svelte", "@angular/core"];
    let test_frameworks = ["vitest", "jest", "@playwright/test"];
    for index in 0..RECOMMEND_WORKSPACE_COUNT {
        let framework = frameworks[index % frameworks.len()];
        let test_framework = test_frameworks[index % test_frameworks.len()];
        write_file(
            &root,
            &format!("packages/package{index}/package.json"),
            format!(
                r#"{{
  "name": "@bench/package{index}",
  "private": true,
  "dependencies": {{ "{framework}": "1.0.0" }},
  "devDependencies": {{ "{test_framework}": "1.0.0" }}
}}"#,
            ),
        );
    }

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_runtime_coverage_project() -> RuntimeCoverageInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-runtime-coverage","private":true,"type":"module","main":"src/index.ts"}"#,
    );
    let coverage_path = write_runtime_coverage_sources(&root);

    RuntimeCoverageInput {
        _temp_dir: temp_dir,
        root,
        coverage_path,
        response_bytes: runtime_coverage_response_bytes(),
    }
}

fn write_runtime_coverage_sources(root: &Path) -> PathBuf {
    let mut index_source = String::new();
    let mut scripts = Vec::with_capacity(RUNTIME_COVERAGE_FILE_COUNT);
    for index in 0..RUNTIME_COVERAGE_FILE_COUNT {
        writeln!(
            &mut index_source,
            "import {{ live_{index} }} from \"./module{index:03}\";"
        )
        .unwrap();
        let source = format!(
            "export function live_{index}(value: number): number {{ return value + {index}; }}\n\
             export function cold_{index}(value: number): number {{ return value - {index}; }}\n"
        );
        let relative = format!("src/module{index:03}.ts");
        write_file(root, &relative, &source);
        scripts.push(serde_json::json!({
            "scriptId": index.to_string(),
            "url": format!("file://{}", root.join(&relative).to_string_lossy()),
            "functions": [{
                "functionName": format!("live_{index}"),
                "ranges": [{
                    "startOffset": 0,
                    "endOffset": source.encode_utf16().count(),
                    "count": index + 1
                }],
                "isBlockCoverage": false
            }]
        }));
    }
    index_source.push_str("\nexport const values = [\n");
    for index in 0..RUNTIME_COVERAGE_FILE_COUNT {
        writeln!(&mut index_source, "  live_{index}({index}),").unwrap();
    }
    index_source.push_str("];\n");
    write_file(root, "src/index.ts", index_source);

    let coverage_path = root.join("coverage-final-v8.json");
    fs::write(
        &coverage_path,
        serde_json::to_vec(&serde_json::json!({"result": scripts})).unwrap(),
    )
    .unwrap();
    coverage_path
}

fn runtime_coverage_response_bytes() -> Vec<u8> {
    let findings = (0..RUNTIME_COVERAGE_FINDING_COUNT)
        .rev()
        .map(|index| {
            let verdict = match index % 4 {
                0 => "safe_to_delete",
                1 => "review_required",
                2 => "coverage_unavailable",
                _ => "low_traffic",
            };
            serde_json::json!({
                "id": format!("fallow:prod:{index:016x}"),
                "file": format!("src/module{index:03}.ts"),
                "function": format!("cold_{index}"),
                "line": 2,
                "verdict": verdict,
                "invocations": if index % 4 == 2 { None } else { Some(index as u64) },
                "confidence": if index % 2 == 0 { "high" } else { "medium" },
                "evidence": {
                    "static_status": if index % 4 == 0 { "unused" } else { "used" },
                    "test_coverage": "not_covered",
                    "v8_tracking": if index % 4 == 2 { "untracked" } else { "tracked" },
                    "untracked_reason": if index % 4 == 2 { Some("lazy_parsed") } else { None },
                    "observation_days": 14,
                    "deployments_observed": 4
                },
                "actions": [{
                    "kind": "review",
                    "description": "Review runtime evidence before changing this function.",
                    "auto_fixable": false
                }]
            })
        })
        .collect::<Vec<_>>();
    let hot_paths = (0..RUNTIME_COVERAGE_HOT_PATH_COUNT)
        .rev()
        .map(|index| {
            serde_json::json!({
                "id": format!("fallow:hot:{index:016x}"),
                "file": format!("src/module{index:03}.ts"),
                "function": format!("live_{index}"),
                "line": 1,
                "end_line": 1,
                "invocations": 10_000 + index,
                "percentile": 100 - (index % 50),
                "identity": null
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&serde_json::json!({
        "protocol_version": "0.8.0",
        "verdict": "cold-code-detected",
        "summary": {
            "functions_tracked": RUNTIME_COVERAGE_FILE_COUNT * 2,
            "functions_hit": RUNTIME_COVERAGE_FILE_COUNT,
            "functions_unhit": RUNTIME_COVERAGE_FILE_COUNT,
            "functions_untracked": 0,
            "coverage_percent": 50.0,
            "trace_count": 1_000_000,
            "period_days": 14,
            "deployments_seen": 4,
            "capture_quality": {
                "window_seconds": 1_209_600,
                "instances_observed": 4,
                "lazy_parse_warning": false,
                "untracked_ratio_percent": 0.0
            }
        },
        "findings": findings,
        "hot_paths": hot_paths,
        "blast_radius": [],
        "importance": [],
        "watermark": null,
        "errors": [],
        "warnings": []
    }))
    .unwrap()
}

fn create_list_inventory_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-list-inventory",
  "private": true,
  "type": "module",
  "workspaces": ["packages/*"]
}"#,
    );

    let files_per_workspace = LIST_FILE_COUNT / LIST_WORKSPACE_COUNT;
    for workspace in 0..LIST_WORKSPACE_COUNT {
        write_file(
            &root,
            &format!("packages/pkg{workspace}/package.json"),
            format!(
                r#"{{
  "name": "@bench/pkg{workspace}",
  "private": true,
  "type": "module",
  "main": "src/index.ts"
}}"#
            ),
        );
        for file in 0..files_per_workspace {
            let name = if file == 0 {
                "index.ts".to_string()
            } else {
                format!("module{file}.ts")
            };
            write_file(
                &root,
                &format!("packages/pkg{workspace}/src/{name}"),
                format!("export const value{workspace}_{file} = {file};\n"),
            );
        }
    }

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_viz_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-viz-html",
  "private": true,
  "type": "module",
  "main": "src/index.ts"
}"#,
    );

    let mut index_source = String::new();
    for index in 0..VIZ_MODULE_COUNT {
        writeln!(
            index_source,
            "import {{ value{index} }} from './features/feature{index}.js';"
        )
        .unwrap();
        write_file(
            &root,
            &format!("src/features/feature{index}.ts"),
            format!("export const value{index} = {{ id: {index}, label: 'feature-{index}' }};\n"),
        );
    }
    index_source.push_str("\nexport const features = [\n");
    for index in 0..VIZ_MODULE_COUNT {
        writeln!(index_source, "  value{index},").unwrap();
    }
    index_source.push_str("];\n");
    write_file(&root, "src/index.ts", index_source);

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_warm_complexity_health_project() -> CommandInput {
    let input = create_health_project();
    let options = ComplexityOptions {
        analysis: analysis_options(&input.root, false),
        complexity: true,
        file_scores: true,
        hotspots: true,
        targets: true,
        ..ComplexityOptions::default()
    };
    let _ = run_health_with_runner(&options, &EngineHealthRunner)
        .expect("warm complexity cache priming succeeds");
    input
}

fn run_programmatic_combined_session(input: &CommandInput) {
    let duplication = DuplicationOptions {
        mode: Some(DuplicationMode::Mild),
        min_tokens: Some(35),
        min_lines: Some(5),
        min_occurrences: Some(2),
        ..DuplicationOptions::default()
    };
    let health = ComplexityOptions {
        complexity: true,
        file_scores: true,
        score: true,
        ..ComplexityOptions::default()
    };

    let options = CombinedOptions {
        analysis: analysis_options(&input.root, true),
        duplication_options: duplication,
        health_options: health,
        ..CombinedOptions::default()
    };

    let _ = run_combined(&options).expect("combined succeeds");
}

fn create_editor_session_workspace_project() -> EditorSessionInput {
    let CommandInput {
        _temp_dir: temp_dir,
        root,
    } = create_workspace_project();
    let session = EditorAnalysisSession::load(&root, None).expect("editor session loads");
    EditorSessionInput {
        _temp_dir: temp_dir,
        session,
    }
}

fn stable_combined_workspace_programmatic_session_reuse(c: &mut Criterion) {
    c.bench_function(
        "stable_combined_workspace_programmatic_session_reuse",
        |bencher| {
            bencher.iter_batched_ref(
                create_workspace_project,
                |input| run_programmatic_combined_session(input),
                BatchSize::LargeInput,
            );
        },
    );
}

fn stable_editor_workspace_repeated_session_analysis(c: &mut Criterion) {
    c.bench_function(
        "stable_editor_workspace_repeated_session_analysis",
        |bencher| {
            bencher.iter_batched_ref(
                create_editor_session_workspace_project,
                |input| {
                    input
                        .session
                        .analyze_project_with(&input.session.config().duplicates, true)
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn stable_extract_workspace_monorepo_warm_hash_hit(c: &mut Criterion) {
    c.bench_function(
        "stable_extract_workspace_monorepo_warm_hash_hit",
        |bencher| {
            bencher.iter_batched_ref(
                create_warm_hash_workspace_project,
                |input| {
                    let result = parse_all_files(&input.files, Some(&input.cache), false);
                    assert_eq!(result.cache_hits, input.files.len());
                    assert_eq!(result.cache_misses, 0);
                    result
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn stable_health_complex_service_warm_complexity_hit(c: &mut Criterion) {
    c.bench_function(
        "stable_health_complex_service_warm_complexity_hit",
        |bencher| {
            bencher.iter_batched_ref(
                create_warm_complexity_health_project,
                |input| {
                    let options = ComplexityOptions {
                        analysis: analysis_options(&input.root, false),
                        complexity: true,
                        file_scores: true,
                        hotspots: true,
                        targets: true,
                        ..ComplexityOptions::default()
                    };
                    run_health_with_runner(&options, &EngineHealthRunner)
                },
                BatchSize::LargeInput,
            );
        },
    );
}

fn stable_circular_dependencies_domain_cycles(c: &mut Criterion) {
    c.bench_function("stable_circular_dependencies_domain_cycles", |bencher| {
        bencher.iter_batched_ref(
            create_circular_project,
            |input| {
                let options = DeadCodeOptions {
                    analysis: analysis_options(&input.root, true),
                    ..DeadCodeOptions::default()
                };
                let output = run_circular_dependencies(&options)
                    .expect("circular-dependency benchmark succeeds");
                assert!(!output.circular_dependencies().is_empty());
                output
            },
            BatchSize::LargeInput,
        );
    });
}

fn stable_feature_flags_workspace_analysis(c: &mut Criterion) {
    c.bench_function("stable_feature_flags_workspace_analysis", |bencher| {
        bencher.iter_batched_ref(
            create_feature_flags_project,
            |input| {
                let options = FeatureFlagsOptions {
                    analysis: analysis_options(&input.root, true),
                    top: None,
                };
                let output = run_feature_flags(&options).expect("feature-flags benchmark succeeds");
                assert_eq!(output.total_flags(), 64);
                output
            },
            BatchSize::LargeInput,
        );
    });
}

fn stable_audit_impact_closure_many_files(c: &mut Criterion) {
    let input = create_impact_closure_project();
    let session = AnalysisSession::load(&input.root, None).expect("impact session loads");
    let artifacts = session
        .analyze_dead_code_with_artifacts(false, true)
        .expect("impact graph analysis succeeds");
    let graph = artifacts.graph.expect("impact graph is retained");
    let changed_files = (0..IMPACT_LAYER_WIDTH)
        .map(|column| input.root.join(format!("src/layer0/module{column}.ts")))
        .collect();

    let warmup = impact_closure_for_changed_paths(&graph, &input.root, &changed_files)
        .expect("changed fixture paths resolve");
    assert_eq!(warmup.in_diff.len(), IMPACT_LAYER_WIDTH);
    assert_eq!(
        warmup.affected_not_shown.len(),
        (IMPACT_LAYER_COUNT - 1) * IMPACT_LAYER_WIDTH + 1
    );
    assert_eq!(warmup.coordination_gap.len(), IMPACT_LAYER_WIDTH);

    c.bench_function("stable_audit_impact_closure_many_files", |bencher| {
        bencher.iter(|| {
            impact_closure_for_changed_paths(&graph, &input.root, &changed_files)
                .expect("changed fixture paths resolve")
        });
    });
}

fn stable_fix_dry_run_many_exports(c: &mut Criterion) {
    let input = create_fix_project();
    let representative_path = input.root.join("src/features/feature0.ts");
    let original_source = fs::read_to_string(&representative_path).unwrap();

    let (status, fix_count) = benchmark_fix_dry_run(&input.root, BENCH_THREADS);
    assert_eq!(status, std::process::ExitCode::SUCCESS);
    assert_eq!(fix_count, FIX_FILE_COUNT);
    assert_eq!(
        fs::read_to_string(&representative_path).unwrap(),
        original_source
    );

    c.bench_function("stable_fix_dry_run_many_exports", |bencher| {
        bencher.iter(|| {
            let (status, fix_count) = benchmark_fix_dry_run(&input.root, BENCH_THREADS);
            assert_eq!(status, std::process::ExitCode::SUCCESS);
            assert_eq!(fix_count, FIX_FILE_COUNT);
            (status, fix_count)
        });
    });

    assert_eq!(
        fs::read_to_string(representative_path).unwrap(),
        original_source
    );
}

fn stable_inspect_file_evidence_bundle_json(c: &mut Criterion) {
    let input = create_inspect_project();

    let result =
        benchmark_inspect_file_evidence_bundle_json(&input.root, BENCH_THREADS, &input.corpus);
    assert_eq!(result.0, std::process::ExitCode::SUCCESS);
    assert_eq!(result.1, INSPECT_CHILD_CALL_COUNT);
    assert!(result.2 > 0);

    c.bench_function("stable_inspect_file_evidence_bundle_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_inspect_file_evidence_bundle_json(
                &input.root,
                BENCH_THREADS,
                &input.corpus,
            );
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, INSPECT_CHILD_CALL_COUNT);
            assert!(result.2 > 0);
            result
        });
    });
}

fn stable_dead_code_many_exports_json(c: &mut Criterion) {
    let input = create_fix_project();

    let (status, issue_count, rendered_bytes) =
        benchmark_dead_code_json(&input.root, BENCH_THREADS);
    assert_eq!(status, std::process::ExitCode::SUCCESS);
    assert_eq!(issue_count, DEAD_CODE_FINDING_COUNT);
    assert!(rendered_bytes > 0);

    c.bench_function("stable_dead_code_many_exports_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_dead_code_json(&input.root, BENCH_THREADS);
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, DEAD_CODE_FINDING_COUNT);
            assert!(result.2 > 0);
            result
        });
    });
}

fn stable_security_many_framework_sinks_json(c: &mut Criterion) {
    let input = create_security_project();
    let expected_findings = SECURITY_FILE_COUNT * 2;

    let (status, finding_count, rendered_bytes) =
        benchmark_security_json(&input.root, BENCH_THREADS);
    assert_eq!(status, std::process::ExitCode::SUCCESS);
    assert_eq!(finding_count, expected_findings);
    assert!(rendered_bytes > 0);

    c.bench_function("stable_security_many_framework_sinks_json", |bencher| {
        bencher.iter(|| {
            let (status, finding_count, rendered_bytes) =
                benchmark_security_json(&input.root, BENCH_THREADS);
            assert_eq!(status, std::process::ExitCode::SUCCESS);
            assert_eq!(finding_count, expected_findings);
            assert!(rendered_bytes > 0);
            (status, finding_count, rendered_bytes)
        });
    });
}

fn stable_rule_pack_policy_analysis_json(c: &mut Criterion) {
    let input = create_rule_pack_project();
    let expected_findings = RULE_PACK_FILE_COUNT * RULE_PACK_FINDINGS_PER_FILE;

    let (status, finding_count, rendered_bytes) =
        benchmark_rule_pack_test_json(&input.root, BENCH_THREADS);
    assert_eq!(status, std::process::ExitCode::SUCCESS);
    assert_eq!(finding_count, expected_findings);
    assert!(rendered_bytes > 0);

    c.bench_function("stable_rule_pack_policy_analysis_json", |bencher| {
        bencher.iter(|| {
            let (status, finding_count, rendered_bytes) =
                benchmark_rule_pack_test_json(&input.root, BENCH_THREADS);
            assert_eq!(status, std::process::ExitCode::SUCCESS);
            assert_eq!(finding_count, expected_findings);
            assert!(rendered_bytes > 0);
            (status, finding_count, rendered_bytes)
        });
    });
}

fn stable_recommend_workspace_json(c: &mut Criterion) {
    let input = create_recommend_project();

    let result = benchmark_recommend_json(&input.root);
    assert_eq!(result.0, std::process::ExitCode::SUCCESS);
    assert_eq!(result.1, RECOMMEND_DECISION_COUNT);
    assert_eq!(result.2, RECOMMEND_FRAMEWORK_COUNT);
    assert!(result.3);
    assert!(result.4 > 0);

    c.bench_function("stable_recommend_workspace_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_recommend_json(&input.root);
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, RECOMMEND_DECISION_COUNT);
            assert_eq!(result.2, RECOMMEND_FRAMEWORK_COUNT);
            assert!(result.3);
            assert!(result.4 > 0);
            result
        });
    });
}

fn stable_coverage_analyze_local_runtime_json(c: &mut Criterion) {
    let input = create_runtime_coverage_project();

    let result = benchmark_runtime_coverage_analyze_json(
        &input.root,
        &input.coverage_path,
        &input.response_bytes,
        BENCH_THREADS,
    );
    assert_eq!(result.0, std::process::ExitCode::SUCCESS);
    assert_eq!(result.1, RUNTIME_COVERAGE_FINDING_COUNT);
    assert_eq!(result.2, RUNTIME_COVERAGE_HOT_PATH_COUNT);
    assert!(
        result.3 > 50_000,
        "request must include the broad static inventory"
    );
    let output: serde_json::Value = serde_json::from_str(&result.4).unwrap();
    assert_eq!(
        output["runtime_coverage"]["findings"][0]["path"],
        "src/module000.ts"
    );
    assert_eq!(
        output["runtime_coverage"]["hot_paths"][0]["path"],
        "src/module063.ts"
    );

    c.bench_function("stable_coverage_analyze_local_runtime_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_runtime_coverage_analyze_json(
                &input.root,
                &input.coverage_path,
                &input.response_bytes,
                BENCH_THREADS,
            );
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, RUNTIME_COVERAGE_FINDING_COUNT);
            assert_eq!(result.2, RUNTIME_COVERAGE_HOT_PATH_COUNT);
            assert!(result.3 > 50_000);
            assert!(!result.4.is_empty());
            result
        });
    });
}

fn stable_list_workspace_inventory_json(c: &mut Criterion) {
    let input = create_list_inventory_project();

    let (status, file_count, entry_point_count, workspace_count, rendered_bytes) =
        benchmark_list_json(&input.root, BENCH_THREADS);
    assert_eq!(status, std::process::ExitCode::SUCCESS);
    assert_eq!(file_count, LIST_FILE_COUNT);
    assert_eq!(entry_point_count, LIST_ENTRY_POINT_COUNT);
    assert_eq!(workspace_count, LIST_WORKSPACE_COUNT);
    assert!(rendered_bytes > 0);

    c.bench_function("stable_list_workspace_inventory_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_list_json(&input.root, BENCH_THREADS);
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, LIST_FILE_COUNT);
            assert_eq!(result.2, LIST_ENTRY_POINT_COUNT);
            assert_eq!(result.3, LIST_WORKSPACE_COUNT);
            assert!(result.4 > 0);
            result
        });
    });
}

fn stable_viz_project_html(c: &mut Criterion) {
    let input = create_viz_project();
    let expected_files = VIZ_MODULE_COUNT + 1;

    let (status, file_count, edge_count, rendered_bytes) =
        benchmark_viz_html(&input.root, BENCH_THREADS);
    assert_eq!(status, std::process::ExitCode::SUCCESS);
    assert_eq!(file_count, expected_files);
    assert_eq!(edge_count, VIZ_MODULE_COUNT);
    assert!(rendered_bytes > 0);

    c.bench_function("stable_viz_project_html", |bencher| {
        bencher.iter(|| {
            let result = benchmark_viz_html(&input.root, BENCH_THREADS);
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, expected_files);
            assert_eq!(result.2, VIZ_MODULE_COUNT);
            assert!(result.3 > 0);
            result
        });
    });
}

criterion_group!(
    benches,
    stable_combined_workspace_programmatic_session_reuse,
    stable_editor_workspace_repeated_session_analysis,
    stable_extract_workspace_monorepo_warm_hash_hit,
    stable_health_complex_service_warm_complexity_hit,
    stable_circular_dependencies_domain_cycles,
    stable_feature_flags_workspace_analysis,
    stable_audit_impact_closure_many_files,
    stable_fix_dry_run_many_exports,
    stable_inspect_file_evidence_bundle_json,
    stable_dead_code_many_exports_json,
    stable_security_many_framework_sinks_json,
    stable_rule_pack_policy_analysis_json,
    stable_recommend_workspace_json,
    stable_coverage_analyze_local_runtime_json,
    stable_list_workspace_inventory_json,
    stable_viz_project_html
);
criterion_main!(benches);
