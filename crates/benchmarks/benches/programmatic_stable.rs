#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, DuplicationMode, DuplicationOptions,
    EditorAnalysisSession, EngineHealthRunner, run_combined, run_health_with_runner,
};
use fallow_extract::{
    cache::{CacheStore, module_to_cached},
    parse_all_files, parse_single_file,
};
use fallow_types::discover::{DiscoveredFile, FileId};
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;

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

criterion_group!(
    benches,
    stable_combined_workspace_programmatic_session_reuse,
    stable_editor_workspace_repeated_session_analysis,
    stable_extract_workspace_monorepo_warm_hash_hit,
    stable_health_complex_service_warm_complexity_hit
);
criterion_main!(benches);
