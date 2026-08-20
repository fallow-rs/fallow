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
    benchmark_trace_clone_compact_json, benchmark_trace_graph_family_compact_json,
    run_circular_dependencies, run_combined, run_feature_flags, run_health_with_runner,
};
use fallow_cli::{
    AuditReviewBenchmarkCorpus, InspectBenchmarkCorpus, SecurityBlindSpotsBenchmarkResult,
    WatchFilterBenchmarkGlobalGitignore, benchmark_audit_review_brief_many_changed_files_json,
    benchmark_dead_code_json, benchmark_fix_dry_run, benchmark_inspect_file_evidence_bundle_json,
    benchmark_list_boundaries_json, benchmark_list_json, benchmark_recommend_json,
    benchmark_rule_pack_test_json, benchmark_runtime_coverage_analyze_json,
    benchmark_security_blind_spots_json, benchmark_security_json,
    benchmark_security_survivors_json, benchmark_viz_html, benchmark_watch_filter_initialization,
    create_audit_review_benchmark_corpus, create_inspect_benchmark_corpus,
    create_security_survivors_benchmark_corpus, create_watch_filter_benchmark_global_gitignore,
};
use fallow_config::{FallowConfig, OutputFormat};
use fallow_engine::{
    dead_code::DeadCodeAnalysisArtifacts,
    duplicates::{CloneFingerprintSet, DuplicationReport},
    module_graph::impact_closure_for_changed_paths,
    session::AnalysisSession,
};
use fallow_extract::{
    cache::{CacheStore, module_to_cached},
    parse_all_files, parse_single_file,
};
use fallow_types::{
    discover::{DiscoveredFile, FileId},
    extract::{SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason},
    results::SecurityUnresolvedCalleeDiagnostic,
};
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;
const AUDIT_REVIEW_CHANGED_FILE_COUNT: usize = 16;
const AUDIT_REVIEW_INTRODUCED_COUNT: usize = AUDIT_REVIEW_CHANGED_FILE_COUNT / 2;
const AUDIT_REVIEW_INHERITED_COUNT: usize = AUDIT_REVIEW_CHANGED_FILE_COUNT / 2;
const AUDIT_REVIEW_PUBLIC_API_ADDED_COUNT: usize = AUDIT_REVIEW_CHANGED_FILE_COUNT;
const AUDIT_REVIEW_DECISION_COUNT: usize = 4;
const DEAD_CODE_FINDING_COUNT: usize = FIX_FILE_COUNT;
const FIX_FILE_COUNT: usize = 128;
const IMPACT_LAYER_COUNT: usize = 32;
const IMPACT_LAYER_WIDTH: usize = 16;
const INSPECT_CHILD_CALL_COUNT: usize = 6;
const LIST_BOUNDARY_FILES_PER_ZONE: usize = 16;
const LIST_BOUNDARY_ZONE_COUNT: usize = 32;
const LIST_BOUNDARY_FILE_COUNT: usize = LIST_BOUNDARY_FILES_PER_ZONE * LIST_BOUNDARY_ZONE_COUNT;
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
const SECURITY_SURVIVOR_COUNT: usize = 86;
const SECURITY_DISMISSED_COUNT: usize = 85;
const SECURITY_NEEDS_HUMAN_REVIEW_COUNT: usize = 85;
const SECURITY_UNRESOLVED_CALLEE_COUNT: usize = 4_096;
const SECURITY_UNRESOLVED_CALLEE_FILE_COUNT: usize = 512;
const SECURITY_UNRESOLVED_CALLEE_GROUP_COUNT: usize = 10;
const SECURITY_UNRESOLVED_CALLEE_SAMPLE_COUNT: usize = 25;
const TRACE_CLONE_FILE_COUNT: usize = 64;
const TRACE_GRAPH_IMPORTER_COUNT: usize = 128;
const VIZ_MODULE_COUNT: usize = 64;
const WATCH_FILTER_FILES_PER_PACKAGE: usize = 16;
const WATCH_FILTER_PACKAGE_COUNT: usize = 64;
const WATCH_FILTER_PROJECT_MATCHER_COUNT: usize = WATCH_FILTER_PACKAGE_COUNT + 3;
const WATCH_FILTER_PROJECT_PATTERN_COUNT: usize = WATCH_FILTER_PACKAGE_COUNT * 2 + 4;

struct CommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
}

struct InspectCommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
    corpus: InspectBenchmarkCorpus,
}

struct AuditReviewCommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
    changed_files: Vec<PathBuf>,
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

struct WatchFilterInput {
    _temp_dir: TempDir,
    config: fallow_config::ResolvedConfig,
    global_gitignore: WatchFilterBenchmarkGlobalGitignore,
}

struct TraceGraphInput {
    _temp_dir: TempDir,
    root: PathBuf,
    artifacts: DeadCodeAnalysisArtifacts,
}

struct TraceCloneInput {
    _temp_dir: TempDir,
    root: PathBuf,
    report: DuplicationReport,
    target_file: String,
    target_line: usize,
    expected_fingerprint: String,
    expected_group_count: usize,
    expected_instance_count: usize,
}

fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    fs::write(path, source.as_ref()).unwrap();
}

fn create_trace_graph_project() -> TraceGraphInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-trace-graph","private":true,"type":"module","main":"src/index.ts","dependencies":{"trace-package":"1.0.0"}}"#,
    );
    write_file(
        &root,
        "src/000-shared.ts",
        "export const sharedValue = 42;\n",
    );

    let mut index_source = String::new();
    for index in 0..TRACE_GRAPH_IMPORTER_COUNT {
        write_file(
            &root,
            &format!("src/consumers/consumer{index}.ts"),
            format!(
                "import {{ sharedValue }} from '../000-shared';\nimport {{ traceHelper }} from 'trace-package';\nexport const value{index} = traceHelper(sharedValue + {index});\n"
            ),
        );
        writeln!(
            index_source,
            "import {{ value{index} }} from './consumers/consumer{index}';\nconsole.log(value{index});"
        )
        .unwrap();
    }
    write_file(&root, "src/index.ts", index_source);

    let session = AnalysisSession::load(&root, None).expect("trace graph session loads");
    let target = session
        .files()
        .iter()
        .find(|file| file.path.ends_with("src/000-shared.ts"))
        .expect("trace target is discovered");
    assert_eq!(
        target.id.0, 0,
        "the retained trace target must precede every non-matching importer"
    );
    let trace_root = session.root().to_path_buf();
    let artifacts = session
        .analyze_dead_code_with_artifacts(false, true)
        .expect("trace graph analysis succeeds");
    assert!(artifacts.graph.is_some());

    TraceGraphInput {
        _temp_dir: temp_dir,
        root: trace_root,
        artifacts,
    }
}

fn create_trace_clone_project() -> TraceCloneInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-trace-clone","private":true,"type":"module","main":"src/index.ts"}"#,
    );

    let mut index_source = String::new();
    for index in 0..TRACE_CLONE_FILE_COUNT {
        write_file(
            &root,
            &format!("src/clones/clone{index}.ts"),
            format!(
                "export function normalizeRecords(records: Array<{{ active: boolean; value: number }}>) {{\n  const active = records.filter((record) => record.active);\n  const values = active.map((record) => record.value);\n  const total = values.reduce((sum, value) => sum + value, 0);\n  const average = values.length === 0 ? 0 : total / values.length;\n  const maximum = values.reduce((current, value) => Math.max(current, value), 0);\n  return {{ total, average, maximum, count: values.length }};\n}}\n\nexport const cloneId = {index};\n"
            ),
        );
        writeln!(
            index_source,
            "import {{ normalizeRecords as normalizeRecords{index} }} from './clones/clone{index}';\nvoid normalizeRecords{index};"
        )
        .unwrap();
    }
    write_file(&root, "src/index.ts", index_source);

    let session = AnalysisSession::load(&root, None).expect("trace clone session loads");
    let trace_root = session.root().to_path_buf();
    let mut config = session.config().duplicates.clone();
    config.min_tokens = 35;
    config.min_lines = 5;
    config.min_occurrences = TRACE_CLONE_FILE_COUNT;
    let report = session.find_duplicates_with_defaults(&config, None).report;
    let group = report
        .clone_groups
        .iter()
        .max_by_key(|group| group.instances.len())
        .expect("trace clone fixture produces a group");
    let target = group
        .instances
        .last()
        .expect("trace clone group has instances");
    let target_file = target
        .file
        .strip_prefix(&trace_root)
        .expect("trace clone path is project-relative")
        .to_string_lossy()
        .replace('\\', "/");
    let target_line = target.start_line;
    let expected_fingerprint =
        CloneFingerprintSet::from_groups(&report.clone_groups).fingerprint_for_group(group);
    let expected_instance_count = group.instances.len();

    TraceCloneInput {
        _temp_dir: temp_dir,
        root: trace_root,
        report,
        target_file,
        target_line,
        expected_fingerprint,
        expected_group_count: 1,
        expected_instance_count,
    }
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

fn create_audit_review_project() -> AuditReviewCommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();
    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-audit-review","type":"module","main":"src/index.ts"}"#,
    );

    let mut changed_files = Vec::with_capacity(AUDIT_REVIEW_CHANGED_FILE_COUNT);
    let mut index_source = String::new();
    for index in 0..AUDIT_REVIEW_CHANGED_FILE_COUNT {
        let changed_relative = format!("src/changed/module{index}.ts");
        write_file(
            &root,
            &changed_relative,
            format!("export const used{index} = {index};\nexport const unused{index} = {index};\n"),
        );
        write_file(
            &root,
            &format!("src/consumers/consumer{index}.ts"),
            format!(
                "import {{ used{index} }} from \"../changed/module{index}\";\nexport const result{index} = used{index};\n"
            ),
        );
        writeln!(
            &mut index_source,
            "import {{ result{index} }} from \"./consumers/consumer{index}\";\nconsole.log(result{index});"
        )
        .unwrap();
        changed_files.push(root.join(changed_relative));
    }
    write_file(&root, "src/index.ts", index_source);

    AuditReviewCommandInput {
        _temp_dir: temp_dir,
        root,
        changed_files,
    }
}

fn create_audit_review_corpus(input: &AuditReviewCommandInput) -> AuditReviewBenchmarkCorpus {
    create_audit_review_benchmark_corpus(&input.root, &input.changed_files, BENCH_THREADS)
        .expect("audit review benchmark corpus builds")
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

fn create_security_unresolved_callee_corpus(
    root: &Path,
) -> Vec<SecurityUnresolvedCalleeDiagnostic> {
    (0..SECURITY_UNRESOLVED_CALLEE_COUNT)
        .rev()
        .map(|index| SecurityUnresolvedCalleeDiagnostic {
            path: root.join(format!(
                "src/routes/route{:03}.ts",
                index % SECURITY_UNRESOLVED_CALLEE_FILE_COUNT
            )),
            line: u32::try_from(index % 400 + 1).unwrap(),
            col: u32::try_from(index % 12).unwrap(),
            reason: match index % 3 {
                0 => SkippedSecurityCalleeReason::ComputedMember,
                1 => SkippedSecurityCalleeReason::DynamicDispatch,
                _ => SkippedSecurityCalleeReason::UnsupportedAssignmentObject,
            },
            expression_kind: match index % 4 {
                0 => SkippedSecurityCalleeExpressionKind::StaticMemberExpression,
                1 => SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
                2 => SkippedSecurityCalleeExpressionKind::Identifier,
                _ => SkippedSecurityCalleeExpressionKind::Other,
            },
        })
        .collect()
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

fn create_list_boundaries_project() -> CommandInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{
  "name": "bench-list-boundaries",
  "private": true,
  "type": "module"
}"#,
    );

    let zones = (0..LIST_BOUNDARY_ZONE_COUNT)
        .map(|zone| {
            serde_json::json!({
                "name": format!("zone-{zone:02}"),
                "patterns": [format!("src/zone-{zone:02}/*.ts")],
            })
        })
        .collect::<Vec<_>>();
    let rules = (0..LIST_BOUNDARY_ZONE_COUNT)
        .map(|zone| {
            serde_json::json!({
                "from": format!("zone-{zone:02}"),
                "allow": [format!("zone-{:02}", (zone + 1) % LIST_BOUNDARY_ZONE_COUNT)],
            })
        })
        .collect::<Vec<_>>();
    write_file(
        &root,
        ".fallowrc.json",
        serde_json::to_string(&serde_json::json!({
            "boundaries": {
                "zones": zones,
                "rules": rules,
            }
        }))
        .unwrap(),
    );

    for zone in 0..LIST_BOUNDARY_ZONE_COUNT {
        for file in 0..LIST_BOUNDARY_FILES_PER_ZONE {
            write_file(
                &root,
                &format!("src/zone-{zone:02}/module-{file:02}.ts"),
                format!("export const value_{zone:02}_{file:02} = {file};\n"),
            );
        }
    }

    CommandInput {
        _temp_dir: temp_dir,
        root,
    }
}

fn create_watch_filter_project() -> WatchFilterInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-watch-filter","private":true,"type":"module"}"#,
    );
    write_file(&root, ".git/info/exclude", "scratch/**\n");
    write_file(&root, ".gitignore", "dist/**\n*.log\n");
    write_file(&root, ".storybook/.gitignore", "storybook-static/**\n");

    // These matchers must not be discovered: one is in a disallowed hidden
    // directory and one is pruned by the resolved user ignore patterns.
    write_file(&root, ".cache/.gitignore", "generated/**\n");
    write_file(&root, "vendor/ignored/.gitignore", "generated/**\n");

    for package in 0..WATCH_FILTER_PACKAGE_COUNT {
        let package_root = format!("packages/pkg-{package:02}");
        write_file(
            &root,
            &format!("{package_root}/.gitignore"),
            "generated/**\n!generated/keep.ts\n",
        );
        for file in 0..WATCH_FILTER_FILES_PER_PACKAGE {
            let directory = if file % 2 == 0 { "src" } else { "generated" };
            write_file(
                &root,
                &format!("{package_root}/{directory}/module-{file:02}.ts"),
                format!("export const value_{package:02}_{file:02} = {file};\n"),
            );
        }
    }

    let config = FallowConfig {
        ignore_patterns: vec![
            "vendor/ignored".to_string(),
            "vendor/ignored/**".to_string(),
        ],
        ..FallowConfig::default()
    }
    .resolve(root, OutputFormat::Json, BENCH_THREADS, false, true, None);
    let global_gitignore = create_watch_filter_benchmark_global_gitignore();

    WatchFilterInput {
        _temp_dir: temp_dir,
        config,
        global_gitignore,
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

fn stable_audit_review_brief_many_changed_files_json(c: &mut Criterion) {
    let input = create_audit_review_project();
    let mut corpus = create_audit_review_corpus(&input);
    let warmup = benchmark_audit_review_brief_many_changed_files_json(&mut corpus);
    assert_eq!(warmup.0, std::process::ExitCode::SUCCESS);
    assert_eq!(warmup.1, AUDIT_REVIEW_INTRODUCED_COUNT);
    assert_eq!(warmup.2, AUDIT_REVIEW_INHERITED_COUNT);
    assert_eq!(warmup.3, AUDIT_REVIEW_PUBLIC_API_ADDED_COUNT);
    assert_eq!(warmup.4, AUDIT_REVIEW_DECISION_COUNT);
    assert!(warmup.5 > 0);

    c.bench_function(
        "stable_audit_review_brief_many_changed_files_json",
        |bencher| {
            bencher.iter(|| {
                let result = benchmark_audit_review_brief_many_changed_files_json(&mut corpus);
                assert_eq!(result.0, std::process::ExitCode::SUCCESS);
                assert_eq!(result.1, AUDIT_REVIEW_INTRODUCED_COUNT);
                assert_eq!(result.2, AUDIT_REVIEW_INHERITED_COUNT);
                assert_eq!(result.3, AUDIT_REVIEW_PUBLIC_API_ADDED_COUNT);
                assert_eq!(result.4, AUDIT_REVIEW_DECISION_COUNT);
                assert!(result.5 > 0);
                result
            });
        },
    );
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

fn stable_security_survivors_verdict_join_json(c: &mut Criterion) {
    let input = create_security_project();
    let corpus = create_security_survivors_benchmark_corpus(&input.root, BENCH_THREADS)
        .expect("security survivors benchmark corpus");
    let result = benchmark_security_survivors_json(&corpus);
    assert_eq!(result.0, std::process::ExitCode::SUCCESS);
    assert_eq!(result.1, SECURITY_SURVIVOR_COUNT);
    assert_eq!(result.2, SECURITY_DISMISSED_COUNT);
    assert_eq!(result.3, SECURITY_NEEDS_HUMAN_REVIEW_COUNT);
    assert_eq!(result.4, 0);
    assert!(result.5 > 0);

    c.bench_function("stable_security_survivors_verdict_join_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_security_survivors_json(&corpus);
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, SECURITY_SURVIVOR_COUNT);
            assert_eq!(result.2, SECURITY_DISMISSED_COUNT);
            assert_eq!(result.3, SECURITY_NEEDS_HUMAN_REVIEW_COUNT);
            assert_eq!(result.4, 0);
            assert!(result.5 > 0);
            result
        });
    });
}

fn assert_security_blind_spots_benchmark_result(result: &SecurityBlindSpotsBenchmarkResult) {
    let expected_groups = [
        (
            SkippedSecurityCalleeReason::ComputedMember,
            SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
            3,
            "src/routes/route001.ts",
        ),
        (
            SkippedSecurityCalleeReason::ComputedMember,
            SkippedSecurityCalleeExpressionKind::StaticMemberExpression,
            3,
            "src/routes/route000.ts",
        ),
        (
            SkippedSecurityCalleeReason::DynamicDispatch,
            SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
            3,
            "src/routes/route001.ts",
        ),
        (
            SkippedSecurityCalleeReason::DynamicDispatch,
            SkippedSecurityCalleeExpressionKind::Identifier,
            3,
            "src/routes/route002.ts",
        ),
        (
            SkippedSecurityCalleeReason::UnsupportedAssignmentObject,
            SkippedSecurityCalleeExpressionKind::Identifier,
            3,
            "src/routes/route002.ts",
        ),
        (
            SkippedSecurityCalleeReason::UnsupportedAssignmentObject,
            SkippedSecurityCalleeExpressionKind::StaticMemberExpression,
            3,
            "src/routes/route000.ts",
        ),
        (
            SkippedSecurityCalleeReason::ComputedMember,
            SkippedSecurityCalleeExpressionKind::Identifier,
            2,
            "src/routes/route002.ts",
        ),
        (
            SkippedSecurityCalleeReason::DynamicDispatch,
            SkippedSecurityCalleeExpressionKind::StaticMemberExpression,
            2,
            "src/routes/route000.ts",
        ),
        (
            SkippedSecurityCalleeReason::UnsupportedAssignmentObject,
            SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
            2,
            "src/routes/route001.ts",
        ),
        (
            SkippedSecurityCalleeReason::ComputedMember,
            SkippedSecurityCalleeExpressionKind::Other,
            1,
            "src/routes/route003.ts",
        ),
    ];

    assert_eq!(
        result.output.summary.unresolved_callee_sites,
        SECURITY_UNRESOLVED_CALLEE_COUNT
    );
    assert_eq!(
        result.output.summary.sampled_callee_sites,
        SECURITY_UNRESOLVED_CALLEE_SAMPLE_COUNT
    );
    assert_eq!(
        result.output.groups.len(),
        SECURITY_UNRESOLVED_CALLEE_GROUP_COUNT
    );
    for (group, (reason, expression_kind, sampled_count, path)) in
        result.output.groups.iter().zip(expected_groups)
    {
        assert_eq!(group.reason, reason);
        assert_eq!(group.expression_kind, expression_kind);
        assert_eq!(group.sampled_count, sampled_count);
        assert_eq!(group.files.len(), 1);
        assert_eq!(group.files[0].path, path);
        assert_eq!(group.files[0].sampled_count, sampled_count);
    }
    assert!(result.rendered_bytes > 0);
}

fn stable_security_blind_spots_unresolved_callees_json(c: &mut Criterion) {
    let input = create_security_project();
    let diagnostics = create_security_unresolved_callee_corpus(&input.root);
    let result = benchmark_security_blind_spots_json(&input.root, &diagnostics);
    assert_security_blind_spots_benchmark_result(&result);

    c.bench_function(
        "stable_security_blind_spots_unresolved_callees_json",
        |bencher| {
            bencher.iter(|| {
                let result = benchmark_security_blind_spots_json(&input.root, &diagnostics);
                assert_security_blind_spots_benchmark_result(&result);
                result
            });
        },
    );
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

fn stable_list_boundaries_many_zones_json(c: &mut Criterion) {
    let input = create_list_boundaries_project();

    let (status, zone_count, rule_count, matched_file_count, rendered_bytes) =
        benchmark_list_boundaries_json(&input.root, BENCH_THREADS);
    assert_eq!(status, std::process::ExitCode::SUCCESS);
    assert_eq!(zone_count, LIST_BOUNDARY_ZONE_COUNT);
    assert_eq!(rule_count, LIST_BOUNDARY_ZONE_COUNT);
    assert_eq!(matched_file_count, LIST_BOUNDARY_FILE_COUNT);
    assert!(rendered_bytes > 0);

    c.bench_function("stable_list_boundaries_many_zones_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_list_boundaries_json(&input.root, BENCH_THREADS);
            assert_eq!(result.0, std::process::ExitCode::SUCCESS);
            assert_eq!(result.1, LIST_BOUNDARY_ZONE_COUNT);
            assert_eq!(result.2, LIST_BOUNDARY_ZONE_COUNT);
            assert_eq!(result.3, LIST_BOUNDARY_FILE_COUNT);
            assert!(result.4 > 0);
            result
        });
    });
}

fn stable_watch_filter_initialization_nested_gitignores(c: &mut Criterion) {
    let input = create_watch_filter_project();

    let result = benchmark_watch_filter_initialization(&input.config, &input.global_gitignore);
    assert_eq!(result.0, WATCH_FILTER_PROJECT_MATCHER_COUNT);
    assert_eq!(result.1, WATCH_FILTER_PROJECT_PATTERN_COUNT);

    c.bench_function(
        "stable_watch_filter_initialization_nested_gitignores",
        |bencher| {
            bencher.iter(|| {
                let result =
                    benchmark_watch_filter_initialization(&input.config, &input.global_gitignore);
                assert_eq!(result.0, WATCH_FILTER_PROJECT_MATCHER_COUNT);
                assert_eq!(result.1, WATCH_FILTER_PROJECT_PATTERN_COUNT);
                result
            });
        },
    );
}

fn stable_trace_graph_family_compact_json(c: &mut Criterion) {
    let input = create_trace_graph_project();
    let expected = (
        TRACE_GRAPH_IMPORTER_COUNT,
        1,
        TRACE_GRAPH_IMPORTER_COUNT,
        TRACE_GRAPH_IMPORTER_COUNT,
    );

    let graph = input
        .artifacts
        .graph
        .as_ref()
        .expect("trace graph is retained");
    let result = benchmark_trace_graph_family_compact_json(
        graph,
        &input.root,
        &input.artifacts.script_used_packages,
    )
    .expect("trace graph family benchmark succeeds");
    assert_eq!((result.0, result.1, result.2, result.3), expected);
    assert!(result.4 > 0);

    c.bench_function("stable_trace_graph_family_compact_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_trace_graph_family_compact_json(
                graph,
                &input.root,
                &input.artifacts.script_used_packages,
            )
            .expect("trace graph family benchmark succeeds");
            assert_eq!((result.0, result.1, result.2, result.3), expected);
            assert!(result.4 > 0);
            result
        });
    });
}

fn stable_trace_clone_compact_json(c: &mut Criterion) {
    let input = create_trace_clone_project();
    assert_eq!(input.expected_instance_count, TRACE_CLONE_FILE_COUNT);

    let result = benchmark_trace_clone_compact_json(
        &input.report,
        &input.root,
        &input.target_file,
        input.target_line,
        &input.expected_fingerprint,
    )
    .expect("trace clone benchmark succeeds");
    assert_eq!(result.location_file, PathBuf::from(&input.target_file));
    assert_eq!(result.location_line, input.target_line);
    assert_eq!(result.location_fingerprint, input.expected_fingerprint);
    assert_eq!(result.fingerprint_fingerprint, input.expected_fingerprint);
    assert_eq!(result.location_group_count, input.expected_group_count);
    assert_eq!(result.fingerprint_group_count, input.expected_group_count);
    assert_eq!(
        result.location_instance_count,
        input.expected_instance_count
    );
    assert_eq!(
        result.fingerprint_instance_count,
        input.expected_instance_count
    );
    assert!(result.rendered_bytes > 0);

    c.bench_function("stable_trace_clone_compact_json", |bencher| {
        bencher.iter(|| {
            let result = benchmark_trace_clone_compact_json(
                &input.report,
                &input.root,
                &input.target_file,
                input.target_line,
                &input.expected_fingerprint,
            )
            .expect("trace clone benchmark succeeds");
            assert_eq!(result.location_file, PathBuf::from(&input.target_file));
            assert_eq!(result.location_line, input.target_line);
            assert_eq!(result.location_fingerprint, input.expected_fingerprint);
            assert_eq!(result.fingerprint_fingerprint, input.expected_fingerprint);
            assert_eq!(result.location_group_count, input.expected_group_count);
            assert_eq!(result.fingerprint_group_count, input.expected_group_count);
            assert_eq!(
                result.location_instance_count,
                input.expected_instance_count
            );
            assert_eq!(
                result.fingerprint_instance_count,
                input.expected_instance_count
            );
            assert!(result.rendered_bytes > 0);
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
    stable_audit_review_brief_many_changed_files_json,
    stable_audit_impact_closure_many_files,
    stable_fix_dry_run_many_exports,
    stable_inspect_file_evidence_bundle_json,
    stable_dead_code_many_exports_json,
    stable_security_many_framework_sinks_json,
    stable_security_survivors_verdict_join_json,
    stable_security_blind_spots_unresolved_callees_json,
    stable_rule_pack_policy_analysis_json,
    stable_recommend_workspace_json,
    stable_coverage_analyze_local_runtime_json,
    stable_list_workspace_inventory_json,
    stable_list_boundaries_many_zones_json,
    stable_watch_filter_initialization_nested_gitignores,
    stable_trace_graph_family_compact_json,
    stable_trace_clone_compact_json,
    stable_viz_project_html
);
criterion_main!(benches);
