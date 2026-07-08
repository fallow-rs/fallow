#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]
#![expect(
    deprecated,
    reason = "Core-internal policy: benchmark exercises the workspace path-dep fallow_core::analyze surface"
)]

use std::path::PathBuf;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use rustc_hash::FxHashSet;
use tempfile::TempDir;

mod helpers;

struct ParseFileInput {
    _temp_dir: TempDir,
    file: fallow_core::discover::DiscoveredFile,
}

struct ConfigInput {
    _temp_dir: TempDir,
    config: fallow_config::ResolvedConfig,
}

fn create_parse_file_input() -> ParseFileInput {
    let temp_dir = tempfile::Builder::new()
        .prefix("fallow-bench-parse-")
        .tempdir()
        .unwrap();

    let test_file = temp_dir.path().join("bench.ts");
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

    ParseFileInput {
        _temp_dir: temp_dir,
        file,
    }
}

fn parse_single_file(c: &mut Criterion) {
    c.bench_function("parse_single_file", |bencher| {
        bencher.iter_batched_ref(
            create_parse_file_input,
            |input| fallow_core::extract::parse_single_file(&input.file),
            BatchSize::LargeInput,
        );
    });
}

fn create_full_pipeline_input() -> ConfigInput {
    let temp_dir = tempfile::Builder::new()
        .prefix("fallow-bench-project-")
        .tempdir()
        .unwrap();
    let root = temp_dir.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();

    std::fs::write(
        root.join("package.json"),
        r#"{"name": "bench-project", "main": "src/index.ts", "dependencies": {"react": "^18"}}"#,
    )
    .unwrap();

    for i in 0..10 {
        let content = format!(
            r"
export const value{i} = {i};
export function fn{i}() {{ return {i}; }}
export type Type{i} = {{ value: number }};
"
        );
        std::fs::write(root.join(format!("src/module{i}.ts")), content).unwrap();
    }

    let imports: Vec<String> = (0..5)
        .map(|i| format!("import {{ value{i} }} from './module{i}';"))
        .collect();
    let uses: Vec<String> = (0..5).map(|i| format!("console.log(value{i});")).collect();
    std::fs::write(
        root.join("src/index.ts"),
        format!("{}\n{}\n", imports.join("\n"), uses.join("\n")),
    )
    .unwrap();

    let config = helpers::create_test_config(root);

    ConfigInput {
        _temp_dir: temp_dir,
        config,
    }
}

fn full_pipeline_10_files(c: &mut Criterion) {
    c.bench_function("full_pipeline_10_files", |bencher| {
        bencher.iter_batched_ref(
            create_full_pipeline_input,
            |input| fallow_core::analyze(&input.config),
            BatchSize::LargeInput,
        );
    });
}

fn create_synthetic_config_input(name: &str, file_count: usize) -> ConfigInput {
    let (temp_dir, config) = helpers::create_synthetic_project(name, file_count);
    ConfigInput {
        _temp_dir: temp_dir,
        config,
    }
}

fn full_pipeline_100_files(c: &mut Criterion) {
    c.bench_function("full_pipeline_100_files", |bencher| {
        bencher.iter_batched_ref(
            || create_synthetic_config_input("100", 100),
            |input| fallow_core::analyze(&input.config),
            BatchSize::LargeInput,
        );
    });
}

fn full_pipeline_1000_files(c: &mut Criterion) {
    c.bench_function("full_pipeline_1000_files", |bencher| {
        bencher.iter_batched_ref(
            || create_synthetic_config_input("1000", 1000),
            |input| fallow_core::analyze(&input.config),
            BatchSize::LargeInput,
        );
    });
}

struct ReExportInput {
    files: Vec<fallow_core::discover::DiscoveredFile>,
    resolved_modules: Vec<fallow_core::resolve::ResolvedModule>,
    entry_points: Vec<fallow_core::discover::EntryPoint>,
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "bench file/span counts are trivially small"
)]
#[expect(
    clippy::too_many_lines,
    reason = "benchmark with extensive fixture setup"
)]
fn create_re_export_input() -> ReExportInput {
    use fallow_core::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use fallow_core::extract::{
        ExportInfo, ExportName, ImportInfo, ImportedName, ReExportInfo, VisibilityTag,
    };
    use fallow_core::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};

    let source_count = 20;
    let barrel_count = 10;
    let exports_per_source = 10;
    let total_files = 1 + barrel_count + source_count; // entry + barrels + sources

    let mut files: Vec<DiscoveredFile> = Vec::with_capacity(total_files);
    let mut resolved_modules: Vec<ResolvedModule> = Vec::with_capacity(total_files);

    let barrel_start: u32 = 1;
    let source_start: u32 = barrel_start + barrel_count as u32;

    files.push(DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/entry.ts"),
        size_bytes: 100,
    });

    let entry_imports: Vec<ResolvedImport> = (0..barrel_count)
        .flat_map(|b| {
            let barrel_id = FileId(barrel_start + b as u32);
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
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
    });

    for b in 0..barrel_count {
        let barrel_id = FileId(barrel_start + b as u32);
        files.push(DiscoveredFile {
            id: barrel_id,
            path: PathBuf::from(format!("/project/src/barrel{b}.ts")),
            size_bytes: 50,
        });

        let mut re_exports: Vec<ResolvedReExport> = Vec::new();

        if b < barrel_count / 2 {
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
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
        });
    }

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
                        expected_unused_reason: None,
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
                        expected_unused_reason: None,
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
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
        });
    }

    let entry_points = vec![EntryPoint {
        path: PathBuf::from("/project/src/entry.ts"),
        source: EntryPointSource::PackageJsonMain,
    }];

    ReExportInput {
        files,
        resolved_modules,
        entry_points,
    }
}

fn resolve_re_export_chains(c: &mut Criterion) {
    c.bench_function("resolve_re_export_chains", |bencher| {
        bencher.iter_batched_ref(
            create_re_export_input,
            |input| {
                fallow_core::graph::ModuleGraph::build(
                    &input.resolved_modules,
                    &input.entry_points,
                    &input.files,
                );
            },
            BatchSize::LargeInput,
        );
    });
}

#[expect(
    clippy::too_many_lines,
    reason = "benchmark with extensive fixture setup"
)]
fn create_cache_round_trip_input() -> fallow_core::extract::ModuleInfo {
    use fallow_core::discover::FileId;
    use fallow_core::extract::{
        DynamicImportInfo, ExportInfo, ExportName, ImportInfo, ImportedName, MemberAccess,
        MemberInfo, MemberKind, ModuleInfo, ReExportInfo, RequireCallInfo, VisibilityTag,
    };

    ModuleInfo {
        file_id: FileId(0),
        exports: vec![
            ExportInfo {
                name: ExportName::Named("UserService".to_string()),
                local_name: Some("UserService".to_string()),
                is_type_only: false,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: oxc_span::Span::new(100, 500),
                members: vec![
                    MemberInfo {
                        name: "getUser".to_string(),
                        kind: MemberKind::ClassMethod,
                        span: oxc_span::Span::new(200, 300),
                        has_decorator: false,
                        decorator_names: Vec::new(),
                        is_instance_returning_static: false,
                        is_self_returning: false,
                    },
                    MemberInfo {
                        name: "listUsers".to_string(),
                        kind: MemberKind::ClassMethod,
                        span: oxc_span::Span::new(310, 400),
                        has_decorator: false,
                        decorator_names: Vec::new(),
                        is_instance_returning_static: false,
                        is_self_returning: false,
                    },
                    MemberInfo {
                        name: "baseUrl".to_string(),
                        kind: MemberKind::ClassProperty,
                        span: oxc_span::Span::new(120, 150),
                        has_decorator: false,
                        decorator_names: Vec::new(),
                        is_instance_returning_static: false,
                        is_self_returning: false,
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
                expected_unused_reason: None,
                span: oxc_span::Span::new(550, 700),
                members: vec![
                    MemberInfo {
                        name: "Active".to_string(),
                        kind: MemberKind::EnumMember,
                        span: oxc_span::Span::new(570, 590),
                        has_decorator: false,
                        decorator_names: Vec::new(),
                        is_instance_returning_static: false,
                        is_self_returning: false,
                    },
                    MemberInfo {
                        name: "Inactive".to_string(),
                        kind: MemberKind::EnumMember,
                        span: oxc_span::Span::new(595, 620),
                        has_decorator: false,
                        decorator_names: Vec::new(),
                        is_instance_returning_static: false,
                        is_self_returning: false,
                    },
                    MemberInfo {
                        name: "Pending".to_string(),
                        kind: MemberKind::EnumMember,
                        span: oxc_span::Span::new(625, 650),
                        has_decorator: false,
                        decorator_names: Vec::new(),
                        is_instance_returning_static: false,
                        is_self_returning: false,
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
                expected_unused_reason: None,
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
                expected_unused_reason: None,
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
                expected_unused_reason: None,
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
            is_speculative: false,
        }],
        require_calls: vec![RequireCallInfo {
            source: "fs".to_string(),
            span: oxc_span::Span::new(950, 970),
            destructured_names: vec![],
            local_name: None,
            source_span: oxc_span::Span::default(),
        }],
        package_path_references: Box::default(),
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
        semantic_facts: Box::default(),
        whole_object_uses: Box::default(),
        dynamic_import_patterns: vec![],
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        content_hash: 0xDEAD_BEEF_CAFE_1234,
        suppressions: vec![],
        unknown_suppression_kinds: vec![],
        unused_import_bindings: vec![],
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        line_offsets: vec![0],
        complexity: Vec::new(),
        flag_uses: vec![],
        class_heritage: vec![],
        exported_factory_returns: Box::default(),
        type_member_types: Box::default(),
        injection_tokens: vec![],
        local_type_declarations: Vec::new(),
        public_signature_type_references: Vec::new(),
        namespace_object_aliases: Vec::new(),
        iconify_prefixes: Vec::new(),
        iconify_icon_names: Vec::new(),
        auto_import_candidates: Vec::new(),
        directives: Vec::new(),
        client_only_dynamic_import_spans: Vec::new(),
        security_sinks: Vec::new(),
        security_sinks_skipped: 0,
        security_unresolved_callee_sites: Vec::new(),
        tainted_bindings: Vec::new(),
        sanitized_sink_args: Vec::new(),
        security_control_sites: Vec::new(),
        callee_uses: Vec::new(),
        misplaced_directives: Vec::new(),
        inline_server_action_exports: Vec::new(),
        di_key_sites: Vec::new(),
        has_dynamic_provide: false,
        referenced_import_bindings: Vec::new(),
        component_props: Vec::new(),
        has_props_attrs_fallthrough: false,
        has_define_expose: false,
        has_define_model: false,
        has_unharvestable_props: false,
        component_emits: Vec::new(),
        angular_inputs: Vec::new(),
        angular_outputs: Vec::new(),
        has_unharvestable_emits: false,
        has_dynamic_emit: false,
        has_emit_whole_object_use: false,
        load_return_keys: Vec::new(),
        has_unharvestable_load: false,
        has_load_data_whole_use: false,
        has_page_data_store_whole_use: false,
        component_functions: Vec::new(),
        react_props: Vec::new(),
        hook_uses: Vec::new(),
        render_edges: Vec::new(),
        svelte_dispatched_events: Vec::new(),
        svelte_listened_events: Vec::new(),
        angular_component_selectors: Vec::new(),
        registered_custom_elements: Vec::new(),
        used_custom_element_tags: Vec::new(),
        angular_used_selectors: Vec::new(),
        angular_entry_component_refs: Vec::new(),
        has_dynamic_component_render: false,
        has_dynamic_dispatch: false,
    }
}

fn cache_round_trip(c: &mut Criterion) {
    use fallow_core::cache::{cached_to_module, module_to_cached};
    use fallow_core::discover::FileId;
    use fallow_types::source_fingerprint::SourceFingerprint;

    c.bench_function("cache_round_trip", |bencher| {
        bencher.iter_batched_ref(
            create_cache_round_trip_input,
            |module| {
                let cached = module_to_cached(module, SourceFingerprint::new(0, 0));
                let _ = cached_to_module(&cached, FileId(0));
            },
            BatchSize::LargeInput,
        );
    });
}

criterion_group!(
    benches,
    parse_single_file,
    full_pipeline_10_files,
    full_pipeline_100_files,
    full_pipeline_1000_files,
    resolve_re_export_chains,
    cache_round_trip
);
criterion_main!(benches);
