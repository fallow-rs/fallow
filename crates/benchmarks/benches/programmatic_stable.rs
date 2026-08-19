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
    benchmark_fix_dry_run, benchmark_list_json, benchmark_security_json, benchmark_viz_html,
};
use fallow_extract::{
    cache::{CacheStore, module_to_cached},
    parse_all_files, parse_single_file,
};
use fallow_types::discover::{DiscoveredFile, FileId};
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;
const FIX_FILE_COUNT: usize = 128;
const LIST_FILE_COUNT: usize = 128;
const LIST_WORKSPACE_COUNT: usize = 8;
// Each workspace index is reported once as a default index and once from its
// package metadata, preserving both production entry-point sources.
const LIST_ENTRY_POINT_COUNT: usize = LIST_WORKSPACE_COUNT * 2;
const SECURITY_FILE_COUNT: usize = 128;
const VIZ_MODULE_COUNT: usize = 64;

struct CommandInput {
    _temp_dir: TempDir,
    root: PathBuf,
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

fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    fs::write(path, source.as_ref()).unwrap();
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
    stable_fix_dry_run_many_exports,
    stable_security_many_framework_sinks_json,
    stable_list_workspace_inventory_json,
    stable_viz_project_html
);
criterion_main!(benches);
