#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fallow_api::{AnalysisOptions, DeadCodeOptions, run_dead_code};
use fallow_core::{
    discover::{DiscoveredFile, FileId},
    extract::parse_single_file,
};
use tempfile::TempDir;

const BENCH_THREADS: usize = 4;
const REPRESENTATIVE_TYPES_SOURCE: &str = include_str!("../fixtures/representative-types.ts");

struct RealSourceInput {
    _temp_dir: TempDir,
    root: PathBuf,
    file: DiscoveredFile,
}

fn write_file(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("fixture file has parent")).unwrap();
    std::fs::write(path, source).unwrap();
}

fn create_representative_types_project() -> RealSourceInput {
    let temp_dir = TempDir::new().unwrap();
    let root = temp_dir.path().to_path_buf();

    write_file(
        &root,
        "package.json",
        r#"{"name":"bench-representative-types","private":true,"type":"module","main":"src/types.ts","dependencies":{}}"#,
    );
    write_file(&root, "src/types.ts", REPRESENTATIVE_TYPES_SOURCE);
    write_file(
        &root,
        "src/errors.ts",
        r"
export const defaultErrorMap = {};
export const getErrorMap = () => defaultErrorMap;
export type IssueData = { code?: string };
export type StringValidation = unknown;
export type ZodCustomIssue = { code: string };
export class ZodError extends Error {}
export type ZodErrorMap = unknown;
export type ZodIssue = unknown;
export const ZodIssueCode = {};
",
    );
    write_file(
        &root,
        "src/helpers/enumUtil.ts",
        "export const enumUtil = {};\n",
    );
    write_file(
        &root,
        "src/helpers/errorUtil.ts",
        "export const errorUtil = {};\n",
    );
    write_file(
        &root,
        "src/helpers/parseUtil.ts",
        r#"
export const DIRTY = "dirty";
export const INVALID = "invalid";
export const OK = "ok";
export const addIssueToContext = () => {};
export const isAborted = () => false;
export const isAsync = () => false;
export const isDirty = () => false;
export const isValid = () => true;
export const makeIssue = () => ({});
export type AsyncParseReturnType<T = unknown> = Promise<T>;
export type ParseContext = { common?: unknown };
export type ParseInput = { data: unknown };
export type ParseParams = unknown;
export type ParsePath = Array<string | number>;
export type ParseReturnType<T = unknown> = T;
export class ParseStatus {}
export type SyncParseReturnType<T = unknown> = T;
"#,
    );
    write_file(
        &root,
        "src/helpers/partialUtil.ts",
        "export const partialUtil = {};\n",
    );
    write_file(
        &root,
        "src/helpers/typeAliases.ts",
        "export type Primitive = string | number | boolean | null | undefined;\n",
    );
    write_file(
        &root,
        "src/helpers/util.ts",
        r#"
export const getParsedType = () => "unknown";
export const objectUtil = {};
export const util = {};
export type ZodParsedType = string;
"#,
    );
    write_file(
        &root,
        "src/standard-schema.ts",
        "export type StandardSchemaV1 = unknown;\n",
    );

    let file_path = root.join("src/types.ts");
    let file = DiscoveredFile {
        id: FileId(0),
        size_bytes: std::fs::metadata(&file_path).unwrap().len(),
        path: file_path,
    };

    RealSourceInput {
        _temp_dir: temp_dir,
        root,
        file,
    }
}

fn representative_types_parse(c: &mut Criterion) {
    c.bench_function("representative_types_parse", |bencher| {
        bencher.iter_batched_ref(
            create_representative_types_project,
            |input| parse_single_file(&input.file),
            BatchSize::LargeInput,
        );
    });
}

fn representative_types_dead_code(c: &mut Criterion) {
    c.bench_function("representative_types_dead_code", |bencher| {
        bencher.iter_batched_ref(
            create_representative_types_project,
            |input| {
                let options = DeadCodeOptions {
                    analysis: AnalysisOptions {
                        root: Some(input.root.clone()),
                        no_cache: true,
                        threads: Some(BENCH_THREADS),
                        ..AnalysisOptions::default()
                    },
                    include_entry_exports: true,
                    ..DeadCodeOptions::default()
                };
                run_dead_code(&options)
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    representative_types_parse,
    representative_types_dead_code
);
criterion_main!(benches);
