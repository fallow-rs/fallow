use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use rustc_hash::FxHashSet;

mod helpers;

fn bench_parse_file(c: &mut Criterion) {
    // Create a temporary file with typical TypeScript content
    let temp_dir = std::env::temp_dir().join("fallow-bench");
    std::fs::create_dir_all(&temp_dir).unwrap();

    let test_file = temp_dir.join("bench.ts");
    std::fs::write(
        &test_file,
        r"
import { useState, useEffect, useCallback, useMemo } from 'react';
import type { FC, ReactNode, MouseEvent } from 'react';
import * as lodash from 'lodash';
import axios from 'axios';

export interface Props {
    name: string;
    age: number;
    children?: ReactNode;
}

export type Status = 'active' | 'inactive' | 'pending';

export enum Color {
    Red = 'red',
    Green = 'green',
    Blue = 'blue',
}

export class UserService {
    private baseUrl: string;

    constructor(baseUrl: string) {
        this.baseUrl = baseUrl;
    }

    async getUser(id: number) {
        return axios.get(`${this.baseUrl}/users/${id}`);
    }

    async listUsers() {
        return axios.get(`${this.baseUrl}/users`);
    }
}

export const useUser = (id: number) => {
    const [user, setUser] = useState(null);
    const [loading, setLoading] = useState(true);

    useEffect(() => {
        const service = new UserService('/api');
        service.getUser(id).then(res => {
            setUser(res.data);
            setLoading(false);
        });
    }, [id]);

    return { user, loading };
};

export const formatName = (first: string, last: string): string => {
    return `${first} ${last}`;
};

export const capitalize = (s: string): string => {
    return s.charAt(0).toUpperCase() + s.slice(1);
};

export default function App({ name, age }: Props) {
    const { user, loading } = useUser(1);
    const fullName = useMemo(() => formatName(name, 'Doe'), [name]);

    const handleClick = useCallback((e: MouseEvent) => {
        console.log(e);
    }, []);

    if (loading) return null;

    return null;
}
",
    )
    .unwrap();

    let file = fallow_core::discover::DiscoveredFile {
        id: fallow_core::discover::FileId(0),
        path: test_file.clone(),
        size_bytes: std::fs::metadata(&test_file).unwrap().len(),
    };

    c.bench_function("parse_single_file", |b| {
        b.iter(|| {
            let _ = fallow_core::extract::parse_single_file(&file);
        });
    });

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn bench_full_pipeline(c: &mut Criterion) {
    // Create a small test project
    let temp_dir = std::env::temp_dir().join("fallow-bench-project");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();

    // Create package.json
    std::fs::write(
        temp_dir.join("package.json"),
        r#"{"name": "bench-project", "main": "src/index.ts", "dependencies": {"react": "^18"}}"#,
    )
    .unwrap();

    // Create 10 source files
    for i in 0..10 {
        let content = format!(
            r"
export const value{i} = {i};
export function fn{i}() {{ return {i}; }}
export type Type{i} = {{ value: number }};
"
        );
        std::fs::write(temp_dir.join(format!("src/module{i}.ts")), content).unwrap();
    }

    // Create index that imports some
    let imports: Vec<String> = (0..5)
        .map(|i| format!("import {{ value{i} }} from './module{i}';"))
        .collect();
    let uses: Vec<String> = (0..5).map(|i| format!("console.log(value{i});")).collect();
    std::fs::write(
        temp_dir.join("src/index.ts"),
        format!("{}\n{}\n", imports.join("\n"), uses.join("\n")),
    )
    .unwrap();

    let config = helpers::create_test_config(temp_dir.clone());

    c.bench_function("full_pipeline_10_files", |b| {
        b.iter(|| {
            let _ = fallow_core::analyze(&config);
        });
    });

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn bench_full_pipeline_100(c: &mut Criterion) {
    let (temp_dir, config) = helpers::create_synthetic_project("100", 100);

    c.bench_function("full_pipeline_100_files", |b| {
        b.iter(|| {
            let _ = fallow_core::analyze(&config);
        });
    });

    let _ = std::fs::remove_dir_all(&temp_dir);
}

fn bench_full_pipeline_1000(c: &mut Criterion) {
    let (temp_dir, config) = helpers::create_synthetic_project("1000", 1000);

    c.bench_function("full_pipeline_1000_files", |b| {
        b.iter(|| {
            let _ = fallow_core::analyze(&config);
        });
    });

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "bench file/span counts are trivially small"
)]
#[expect(
    clippy::too_many_lines,
    reason = "benchmark with extensive fixture setup"
)]
fn bench_resolve_re_export_chains(c: &mut Criterion) {
    use fallow_core::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_core::extract::{
        ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
    };
    use fallow_core::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};

    // Build a graph with multiple re-export chains:
    //
    //   entry.ts -> barrel1.ts -> barrel2.ts -> source_a.ts
    //                                        -> source_b.ts
    //            -> barrel3.ts -> source_c.ts
    //
    // Each source file has 10 exports. Barrel files re-export all of them.
    // This exercises the iterative re-export chain resolution with the HashSet optimization.

    let source_count = 20;
    let barrel_count = 10;
    let exports_per_source = 10;
    let total_files = 1 + barrel_count + source_count; // entry + barrels + sources

    let mut files: Vec<DiscoveredFile> = Vec::with_capacity(total_files);
    let mut resolved_modules: Vec<ResolvedModule> = Vec::with_capacity(total_files);

    // FileId layout:
    //   0        = entry.ts
    //   1..=B    = barrel files (barrel_count)
    //   B+1..=N  = source files (source_count)
    let barrel_start: u32 = 1;
    let source_start: u32 = barrel_start + barrel_count as u32;

    // --- entry.ts (id=0) ---
    files.push(DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/entry.ts"),
        size_bytes: 100,
    });

    // Entry imports from each barrel
    let entry_imports: Vec<ResolvedImport> = (0..barrel_count)
        .flat_map(|b| {
            let barrel_id = FileId(barrel_start + b as u32);
            // Import the first 3 re-exported symbols from each barrel
            (0..3).map(move |e| ResolvedImport {
                info: ImportInfo {
                    source: format!("./barrel{b}"),
                    imported_name: ImportedName::Named(format!("value{e}")),
                    local_name: format!("barrel{b}_value{e}"),
                    is_type_only: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 10),
                    source_span: oxc_span::Span::default(),
                },
                target: ResolveResult::InternalModule(barrel_id),
            })
        })
        .collect();

    resolved_modules.push(ResolvedModule {
        file_id: FileId(0),
        path: PathBuf::from("/project/src/entry.ts"),
        exports: vec![],
        re_exports: vec![],
        resolved_imports: entry_imports,
        resolved_dynamic_imports: vec![],
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![],
        whole_object_uses: vec![],
        has_cjs_exports: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
    });

    // --- Barrel files ---
    // Each barrel re-exports from 2 sources (creating chains).
    // barrels 0..4 also re-export from barrel 5..9, forming 2-level chains.
    for b in 0..barrel_count {
        let barrel_id = FileId(barrel_start + b as u32);
        files.push(DiscoveredFile {
            id: barrel_id,
            path: PathBuf::from(format!("/project/src/barrel{b}.ts")),
            size_bytes: 50,
        });

        let mut re_exports: Vec<ResolvedReExport> = Vec::new();

        if b < barrel_count / 2 {
            // First half of barrels re-export from a second-tier barrel (chaining)
            let chained_barrel = barrel_count / 2 + (b % (barrel_count / 2));
            let chained_id = FileId(barrel_start + chained_barrel as u32);
            for e in 0..exports_per_source {
                re_exports.push(ResolvedReExport {
                    info: ReExportInfo {
                        source: format!("./barrel{chained_barrel}"),
                        imported_name: format!("value{e}"),
                        exported_name: format!("value{e}"),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(chained_id),
                });
            }
        } else {
            // Second half of barrels re-export directly from source files
            let src_a = (b * 2) % source_count;
            let src_b = (b * 2 + 1) % source_count;
            let src_a_id = FileId(source_start + src_a as u32);
            let src_b_id = FileId(source_start + src_b as u32);

            for e in 0..exports_per_source {
                re_exports.push(ResolvedReExport {
                    info: ReExportInfo {
                        source: format!("./source{src_a}"),
                        imported_name: format!("value{e}"),
                        exported_name: format!("value{e}"),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(src_a_id),
                });
                re_exports.push(ResolvedReExport {
                    info: ReExportInfo {
                        source: format!("./source{src_b}"),
                        imported_name: format!("fn{e}"),
                        exported_name: format!("fn{e}"),
                        is_type_only: false,
                        span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(src_b_id),
                });
            }
        }

        resolved_modules.push(ResolvedModule {
            file_id: barrel_id,
            path: PathBuf::from(format!("/project/src/barrel{b}.ts")),
            exports: vec![],
            re_exports,
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
        });
    }

    // --- Source files ---
    for s in 0..source_count {
        let source_id = FileId(source_start + s as u32);
        files.push(DiscoveredFile {
            id: source_id,
            path: PathBuf::from(format!("/project/src/source{s}.ts")),
            size_bytes: 200,
        });

        let exports: Vec<ExportInfo> = (0..exports_per_source)
            .flat_map(|e| {
                vec![
                    ExportInfo {
                        name: ExportName::Named(format!("value{e}")),
                        local_name: Some(format!("value{e}")),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(0, 20),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                    ExportInfo {
                        name: ExportName::Named(format!("fn{e}")),
                        local_name: Some(format!("fn{e}")),
                        is_type_only: false,
                        visibility: VisibilityTag::None,
                        span: oxc_span::Span::new(25, 45),
                        members: vec![],
                        is_side_effect_used: false,
                        super_class: None,
                    },
                ]
            })
            .collect();

        resolved_modules.push(ResolvedModule {
            file_id: source_id,
            path: PathBuf::from(format!("/project/src/source{s}.ts")),
            exports,
            re_exports: vec![],
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
        });
    }

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/entry.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    c.bench_function("resolve_re_export_chains", |b| {
        b.iter(|| {
            fallow_core::graph::ModuleGraph::build(&resolved_modules, &entry_points, &files);
        });
    });
}

#[expect(
    clippy::too_many_lines,
    reason = "benchmark with extensive fixture setup"
)]
fn bench_cache_round_trip(c: &mut Criterion) {
    use fallow_core::cache::{cached_to_module, module_to_cached};
    use fallow_core::discover::FileId;
    use fallow_core::extract::{
        DynamicImportInfo, ExportInfo, ExportName, ImportInfo, ImportedName, MemberAccess,
        MemberInfo, MemberKind, ModuleInfo, ReExportInfo, RequireCallInfo, VisibilityTag,
    };

    // Build a representative ModuleInfo with realistic data:
    // imports, exports (including enums and classes with members), re-exports,
    // dynamic imports, require calls, and member accesses.
    let module = ModuleInfo {
        file_id: FileId(0),
        exports: vec![
            ExportInfo {
                name: ExportName::Named("UserService".to_string()),
                local_name: Some("UserService".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(100, 500),
                members: vec![
                    MemberInfo {
                        name: "getUser".to_string(),
                        kind: MemberKind::ClassMethod,
                        span: oxc_span::Span::new(200, 300),
                        has_decorator: false,
                    },
                    MemberInfo {
                        name: "listUsers".to_string(),
                        kind: MemberKind::ClassMethod,
                        span: oxc_span::Span::new(310, 400),
                        has_decorator: false,
                    },
                    MemberInfo {
                        name: "baseUrl".to_string(),
                        kind: MemberKind::ClassProperty,
                        span: oxc_span::Span::new(120, 150),
                        has_decorator: false,
                    },
                ],
                is_side_effect_used: false,
                super_class: None,
            },
            ExportInfo {
                name: ExportName::Named("Status".to_string()),
                local_name: Some("Status".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(550, 700),
                members: vec![
                    MemberInfo {
                        name: "Active".to_string(),
                        kind: MemberKind::EnumMember,
                        span: oxc_span::Span::new(570, 590),
                        has_decorator: false,
                    },
                    MemberInfo {
                        name: "Inactive".to_string(),
                        kind: MemberKind::EnumMember,
                        span: oxc_span::Span::new(595, 620),
                        has_decorator: false,
                    },
                    MemberInfo {
                        name: "Pending".to_string(),
                        kind: MemberKind::EnumMember,
                        span: oxc_span::Span::new(625, 650),
                        has_decorator: false,
                    },
                ],
                is_side_effect_used: false,
                super_class: None,
            },
            ExportInfo {
                name: ExportName::Default,
                local_name: None,
                is_type_only: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(800, 1200),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
            ExportInfo {
                name: ExportName::Named("Props".to_string()),
                local_name: Some("Props".to_string()),
                is_type_only: true,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(10, 80),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
            ExportInfo {
                name: ExportName::Named("formatName".to_string()),
                local_name: Some("formatName".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                span: oxc_span::Span::new(720, 780),
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            },
        ],
        imports: vec![
            ImportInfo {
                source: "react".to_string(),
                imported_name: ImportedName::Named("useState".to_string()),
                local_name: "useState".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 50),
                source_span: oxc_span::Span::default(),
            },
            ImportInfo {
                source: "react".to_string(),
                imported_name: ImportedName::Named("useEffect".to_string()),
                local_name: "useEffect".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(0, 50),
                source_span: oxc_span::Span::default(),
            },
            ImportInfo {
                source: "react".to_string(),
                imported_name: ImportedName::Default,
                local_name: "React".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(55, 80),
                source_span: oxc_span::Span::default(),
            },
            ImportInfo {
                source: "lodash".to_string(),
                imported_name: ImportedName::Namespace,
                local_name: "lodash".to_string(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(85, 110),
                source_span: oxc_span::Span::default(),
            },
            ImportInfo {
                source: "./styles.css".to_string(),
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                from_style: false,
                span: oxc_span::Span::new(115, 140),
                source_span: oxc_span::Span::default(),
            },
            ImportInfo {
                source: "./types".to_string(),
                imported_name: ImportedName::Named("Config".to_string()),
                local_name: "Config".to_string(),
                is_type_only: true,
                from_style: false,
                span: oxc_span::Span::new(145, 180),
                source_span: oxc_span::Span::default(),
            },
        ],
        re_exports: vec![
            ReExportInfo {
                source: "./utils".to_string(),
                imported_name: "capitalize".to_string(),
                exported_name: "capitalize".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            ReExportInfo {
                source: "./helpers".to_string(),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
        ],
        dynamic_imports: vec![DynamicImportInfo {
            source: "./lazy-component".to_string(),
            span: oxc_span::Span::new(900, 940),
            destructured_names: vec![],
            local_name: None,
        }],
        require_calls: vec![RequireCallInfo {
            source: "fs".to_string(),
            span: oxc_span::Span::new(950, 970),
            destructured_names: vec![],
            local_name: None,
        }],
        member_accesses: vec![
            MemberAccess {
                object: "Status".to_string(),
                member: "Active".to_string(),
            },
            MemberAccess {
                object: "lodash".to_string(),
                member: "merge".to_string(),
            },
            MemberAccess {
                object: "console".to_string(),
                member: "log".to_string(),
            },
        ],
        whole_object_uses: vec![],
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        content_hash: 0xDEAD_BEEF_CAFE_1234,
        suppressions: vec![],
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        line_offsets: vec![0],
        complexity: Vec::new(),
        flag_uses: vec![],
        class_heritage: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
    };

    c.bench_function("cache_round_trip", |b| {
        b.iter(|| {
            let cached = module_to_cached(&module, 0, 0);
            let _restored = cached_to_module(&cached, FileId(0));
        });
    });
}

// ── Dupe detection benchmarks ──────────────────────────────────────

fn make_hashed_tokens(hashes: &[u64]) -> Vec<fallow_core::duplicates::normalize::HashedToken> {
    hashes
        .iter()
        .enumerate()
        .map(
            |(i, &hash)| fallow_core::duplicates::normalize::HashedToken {
                hash,
                original_index: i,
            },
        )
        .collect()
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "bench span values are trivially small"
)]
fn make_file_tokens_for(count: usize) -> fallow_core::duplicates::tokenize::FileTokens {
    use fallow_core::duplicates::tokenize::{FileTokens, SourceToken, TokenKind};
    use oxc_span::Span;

    let tokens: Vec<SourceToken> = (0..count)
        .map(|i| SourceToken {
            kind: TokenKind::Identifier(format!("t{i}")),
            span: Span::new((i * 3) as u32, (i * 3 + 2) as u32),
        })
        .collect();

    let mut source = String::with_capacity(count * 4);
    for i in 0..count {
        source.push_str("xx");
        if i < count - 1 {
            source.push('\n');
        }
    }
    let line_count = source.lines().count().max(1);
    FileTokens {
        tokens,
        source,
        line_count,
    }
}

type DupeInput = Vec<(
    PathBuf,
    Vec<fallow_core::duplicates::normalize::HashedToken>,
    fallow_core::duplicates::tokenize::FileTokens,
)>;

/// Build N identical files with `tokens_per_file` tokens each.
fn make_identical_files(n: usize, tokens_per_file: usize) -> DupeInput {
    let hashes: Vec<u64> = (1..=tokens_per_file as u64).collect();
    (0..n)
        .map(|i| {
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(tokens_per_file),
            )
        })
        .collect()
}

/// Build files with diverse content (low duplication).
fn make_diverse_files(n: usize, tokens_per_file: usize) -> DupeInput {
    (0..n)
        .map(|i| {
            let base = (i * tokens_per_file * 10) as u64;
            let hashes: Vec<u64> = (base..base + tokens_per_file as u64).collect();
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                make_hashed_tokens(&hashes),
                make_file_tokens_for(tokens_per_file),
            )
        })
        .collect()
}

fn bench_dupe_detect_2x500(c: &mut Criterion) {
    use fallow_core::duplicates::detect::CloneDetector;
    let data = make_identical_files(2, 500);
    c.bench_function("dupe_detect_2x500_identical", |b| {
        b.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_dupe_detect_2x2000(c: &mut Criterion) {
    use fallow_core::duplicates::detect::CloneDetector;
    let data = make_identical_files(2, 2000);
    c.bench_function("dupe_detect_2x2000_identical", |b| {
        b.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_dupe_detect_10x500(c: &mut Criterion) {
    use fallow_core::duplicates::detect::CloneDetector;
    let data = make_identical_files(10, 500);
    c.bench_function("dupe_detect_10x500_identical", |b| {
        b.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_dupe_detect_50x200_diverse(c: &mut Criterion) {
    use fallow_core::duplicates::detect::CloneDetector;
    let data = make_diverse_files(50, 200);
    c.bench_function("dupe_detect_50x200_diverse", |b| {
        b.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_dupe_detect_100x200_mixed(c: &mut Criterion) {
    use fallow_core::duplicates::detect::CloneDetector;
    // 20 identical + 80 diverse
    let hashes: Vec<u64> = (1..=200).collect();
    let data: DupeInput = (0..100)
        .map(|i| {
            let h = if i < 20 {
                make_hashed_tokens(&hashes)
            } else {
                let base = (i * 10000) as u64;
                let unique_hashes: Vec<u64> = (base..base + 200).collect();
                make_hashed_tokens(&unique_hashes)
            };
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                h,
                make_file_tokens_for(200),
            )
        })
        .collect();

    c.bench_function("dupe_detect_100x200_mixed", |b| {
        b.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_dupe_detect_100x200_mixed_focused(c: &mut Criterion) {
    use fallow_core::duplicates::detect::CloneDetector;
    use rustc_hash::FxHashSet;

    let hashes: Vec<u64> = (1..=200).collect();
    let data: DupeInput = (0..100)
        .map(|i| {
            let h = if i < 20 {
                make_hashed_tokens(&hashes)
            } else {
                let base = (i * 10000) as u64;
                let unique_hashes: Vec<u64> = (base..base + 200).collect();
                make_hashed_tokens(&unique_hashes)
            };
            (
                PathBuf::from(format!("dir{i}/file{i}.ts")),
                h,
                make_file_tokens_for(200),
            )
        })
        .collect();
    let focus: FxHashSet<PathBuf> = std::iter::once(PathBuf::from("dir0/file0.ts")).collect();

    c.bench_function("dupe_detect_100x200_mixed_focused", |b| {
        b.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect_touching_files(d, &focus),
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_dupe_suffix_array_only(c: &mut Criterion) {
    // Benchmark just the suffix array construction on a large input
    // to isolate its cost. We access it through the public detect() API.
    use fallow_core::duplicates::detect::CloneDetector;
    let data = make_identical_files(2, 5000);
    c.bench_function("dupe_detect_2x5000_identical", |b| {
        b.iter_batched(
            || data.clone(),
            |d| CloneDetector::new(30, 5, false).detect(d),
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_parse_file,
    bench_full_pipeline,
    bench_full_pipeline_100,
    bench_full_pipeline_1000,
    bench_resolve_re_export_chains,
    bench_cache_round_trip,
);

criterion_group!(
    dupe_benches,
    bench_dupe_detect_2x500,
    bench_dupe_detect_2x2000,
    bench_dupe_detect_10x500,
    bench_dupe_detect_50x200_diverse,
    bench_dupe_detect_100x200_mixed,
    bench_dupe_detect_100x200_mixed_focused,
    bench_dupe_suffix_array_only,
);

criterion_main!(benches, dupe_benches);
