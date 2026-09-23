//! Fixture builders that the `programmatic_commands` and
//! `programmatic_stable` benches share, so that both suites measure the
//! same workspace fixture.

use std::fs;
use std::path::{Path, PathBuf};

use fallow_api::{
    AnalysisOptions, CombinedOptions, ComplexityOptions, DuplicationMode, DuplicationOptions,
    EditorAnalysisSession, run_combined,
};
use fallow_extract::{
    cache::{CacheStore, module_to_cached},
    parse_single_file,
};
use fallow_types::discover::{DiscoveredFile, FileId};
use tempfile::TempDir;

pub const BENCH_THREADS: usize = 4;

pub struct CommandInput {
    pub _temp_dir: TempDir,
    pub root: PathBuf,
}

pub struct ExtractCacheInput {
    pub _temp_dir: TempDir,
    pub files: Vec<DiscoveredFile>,
    pub cache: CacheStore,
}

pub struct EditorSessionInput {
    pub _temp_dir: TempDir,
    pub session: EditorAnalysisSession,
}

pub fn write_file(root: &Path, path: &str, source: impl AsRef<str>) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    fs::write(path, source.as_ref()).unwrap();
}

pub fn analysis_options(root: &Path, no_cache: bool) -> AnalysisOptions {
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

pub fn create_workspace_project() -> CommandInput {
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

pub fn create_warm_hash_workspace_project() -> ExtractCacheInput {
    let CommandInput {
        _temp_dir: temp_dir,
        root,
    } = create_workspace_project();
    let files = discovered_source_files(&root);
    let mut cache = CacheStore::new(&root);

    for file in &files {
        let module = parse_single_file(file).expect("benchmark fixture parses");
        let cached = module_to_cached(
            &module,
            fallow_types::source_fingerprint::SourceFingerprint::new(1, 1),
            false,
        );
        cache.insert(&file.path, cached);
    }

    ExtractCacheInput {
        _temp_dir: temp_dir,
        files,
        cache,
    }
}

pub fn run_programmatic_combined_session(input: &CommandInput) {
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

pub fn create_editor_session_workspace_project() -> EditorSessionInput {
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
