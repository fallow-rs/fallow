use std::path::{Path, PathBuf};

use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{
    DynamicImportInfo, DynamicImportPattern, ImportInfo, ImportedName, ModuleLoadMechanism,
    ReExportInfo, RequireCallInfo, SemanticFact, VitestModuleMockAction,
    VitestModuleMockOperationFact,
};

use super::dynamic_imports::{
    GlobMatcherCache, resolve_dynamic_imports, resolve_dynamic_patterns,
    resolve_single_dynamic_import,
};
use super::re_exports::resolve_re_exports;
use super::react_native;
use super::require_imports::{resolve_require_imports, resolve_single_require};
use super::specifier;
use super::static_imports::resolve_static_imports;
use super::types::{CanonicalizeCache, ResolveContext, TsconfigCache};
use super::upgrades::apply_specifier_upgrades;
use super::{
    ResolveAllImportsInput, ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport,
    final_replaced_module_targets, resolve_vitest_mock_operations,
};

fn dummy_span() -> Span {
    Span::new(0, 0)
}

/// Project root for tests that only exercise resolver *option* building and
/// never touch the filesystem. It deliberately does not exist on disk, so Yarn
/// PnP detection stays off.
fn options_only_root() -> &'static Path {
    Path::new("/project")
}

/// Build a minimal `ResolveContext` backed by a real resolver but with
/// empty lookup tables. Every specifier resolves to `NpmPackage` or
/// `Unresolvable`, which is fine , the tests focus on how helper functions
/// *transform* inputs into `ResolvedImport` / `ResolvedReExport` structs.
///
/// Under Miri this is a no-op: `oxc_resolver` uses the `statx` syscall
/// (via `rustix`) which Miri does not support.
#[cfg(not(miri))]
fn with_empty_ctx<F: FnOnce(&ResolveContext)>(f: F) {
    let root = PathBuf::from("/project");
    let resolver = specifier::create_resolver(&root, &[], &[]);
    let style_resolver = specifier::create_resolver(&root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let path_to_id = FxHashMap::default();
    let raw_path_to_id = FxHashMap::default();
    let workspace_roots = FxHashMap::default();
    let package_manifests = Vec::new();
    let condition_names = react_native::build_condition_names(&[], &[]);
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: false,
        condition_names: &condition_names,
        path_aliases: &[],
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root: &root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };
    f(&ctx);
}

#[cfg(miri)]
fn with_empty_ctx<F: FnOnce(&ResolveContext)>(_f: F) {}

fn make_import(source: &str, imported: ImportedName, local: &str) -> ImportInfo {
    ImportInfo {
        source: source.to_string(),
        imported_name: imported,
        local_name: local.to_string(),
        is_type_only: false,
        is_type_only_star: false,
        from_style: false,
        span: dummy_span(),
        source_span: Span::default(),
    }
}

fn make_re_export(source: &str, imported: &str, exported: &str) -> ReExportInfo {
    ReExportInfo {
        source: source.to_string(),
        imported_name: imported.to_string(),
        exported_name: exported.to_string(),
        is_type_only: false,
        span: oxc_span::Span::default(),
        statement_span: oxc_span::Span::new(0, 0),
        source_span: oxc_span::Span::new(0, 0),
    }
}

fn vitest_mock_fact(
    source: &str,
    call_start: u32,
    factory_replaces_original: bool,
) -> SemanticFact {
    SemanticFact::VitestModuleMockOperation(VitestModuleMockOperationFact {
        source: source.to_string(),
        call_start,
        action: VitestModuleMockAction::Mock {
            factory_replaces_original,
        },
    })
}

fn vitest_unmock_fact(source: &str, call_start: u32) -> SemanticFact {
    SemanticFact::VitestModuleMockOperation(VitestModuleMockOperationFact {
        source: source.to_string(),
        call_start,
        action: VitestModuleMockAction::Unmock,
    })
}

fn make_dynamic(
    source: &str,
    destructured: Vec<&str>,
    local_name: Option<&str>,
) -> DynamicImportInfo {
    DynamicImportInfo {
        source: source.to_string(),
        span: dummy_span(),
        destructured_names: destructured.into_iter().map(String::from).collect(),
        local_name: local_name.map(String::from),
        is_speculative: false,
    }
}

fn make_require(
    source: &str,
    destructured: Vec<&str>,
    local_name: Option<&str>,
) -> RequireCallInfo {
    RequireCallInfo {
        source: source.to_string(),
        span: dummy_span(),
        destructured_names: destructured.into_iter().map(String::from).collect(),
        local_name: local_name.map(String::from),
        source_span: oxc_span::Span::default(),
        is_type_only: false,
    }
}

/// Build a minimal `ResolvedModule` for `apply_specifier_upgrades` tests.
fn make_resolved_module(
    file_id: u32,
    imports: Vec<ResolvedImport>,
    dynamic_imports: Vec<ResolvedImport>,
    re_exports: Vec<ResolvedReExport>,
) -> ResolvedModule {
    ResolvedModule {
        file_id: FileId(file_id),
        path: PathBuf::from(format!("/project/src/file_{file_id}.ts")),
        exports: vec![].into(),
        re_exports,
        resolved_imports: imports,
        resolved_dynamic_imports: dynamic_imports,
        resolved_dynamic_patterns: vec![],
        member_accesses: vec![].into(),
        semantic_facts: std::sync::Arc::default(),
        whole_object_uses: std::sync::Arc::default(),
        has_cjs_exports: false,
        has_angular_component_template_url: false,
        unused_import_bindings: FxHashSet::default(),
        type_referenced_import_bindings: vec![],
        value_referenced_import_bindings: vec![],
        namespace_object_aliases: vec![],
        exported_factory_returns: std::sync::Arc::default(),
        exported_factory_return_object_shapes: std::sync::Arc::default(),
        type_member_types: std::sync::Arc::default(),
    }
}

fn make_resolved_import(source: &str, target: ResolveResult) -> ResolvedImport {
    ResolvedImport {
        info: make_import(source, ImportedName::Named("x".into()), "x"),
        target,
    }
}

fn make_resolved_re_export(source: &str, target: ResolveResult) -> ResolvedReExport {
    ResolvedReExport {
        info: make_re_export(source, "x", "x"),
        target,
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn vitest_mock_operations_resolve_canonical_targets_and_abstain_on_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonicalize temp dir");
    let test_file = root.join("wrapper.test.ts");
    let dependency_file = root.join("dependency.ts");
    std::fs::write(&test_file, "").expect("write test file");
    std::fs::write(&dependency_file, "export const value = 1;").expect("write dependency");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/*":["*"]}}}"#,
    )
    .expect("write tsconfig");

    let resolver = specifier::create_resolver(&root, &[], &[]);
    let style_resolver = specifier::create_resolver(&root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let path_to_id = FxHashMap::from_iter([
        (test_file.as_path(), FileId(0)),
        (dependency_file.as_path(), FileId(1)),
    ]);
    let raw_path_to_id = path_to_id.clone();
    let workspace_roots = FxHashMap::default();
    let package_manifests = Vec::new();
    let condition_names = react_native::build_condition_names(&[], &[]);
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: false,
        condition_names: &condition_names,
        path_aliases: &[],
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root: &root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };
    let facts = [
        vitest_mock_fact("./dependency", 10, true),
        vitest_mock_fact("./missing", 20, true),
    ];

    let mut operations = resolve_vitest_mock_operations(FileId(0), &facts, &ctx, &test_file);
    apply_specifier_upgrades(&mut [], &mut operations);
    let targets = final_replaced_module_targets(operations);

    assert_eq!(
        targets,
        vec![super::ResolvedReplacedModuleTarget {
            source_file: FileId(0),
            target_file: FileId(1),
        }]
    );
    let facts = [
        vitest_mock_fact("@/dependency", 10, true),
        vitest_unmock_fact("./dependency.ts", 20),
    ];

    let mut operations = resolve_vitest_mock_operations(FileId(0), &facts, &ctx, &test_file);
    assert!(
        operations
            .iter()
            .all(|operation| operation.target.internal_file_id() == Some(FileId(1))),
        "alias and explicit-extension spellings should resolve to one canonical target: {operations:?}"
    );
    apply_specifier_upgrades(&mut [], &mut operations);
    assert!(
        final_replaced_module_targets(operations).is_empty(),
        "the final unmock must clear an equivalent aliased mock target"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn cross_tsconfig_bare_alias_upgrades_replacement_target() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonicalize temp dir");
    let aliased_dir = root.join("packages/app");
    let aliased_source = aliased_dir.join("src/source.ts");
    let test_file = root.join("tests/example.test.ts");
    let dependency_file = root.join("shared/dependency.ts");
    std::fs::create_dir_all(aliased_source.parent().expect("source parent"))
        .expect("create aliased source dir");
    std::fs::create_dir_all(test_file.parent().expect("test parent"))
        .expect("create test source dir");
    std::fs::create_dir_all(dependency_file.parent().expect("dependency parent"))
        .expect("create dependency dir");
    std::fs::write(
        aliased_dir.join("tsconfig.json"),
        r#"{
            "compilerOptions": {
                "baseUrl": ".",
                "paths": {
                    "shared-alias": ["../../shared/dependency.ts"]
                }
            }
        }"#,
    )
    .expect("write tsconfig");
    std::fs::write(&aliased_source, "").expect("write aliased source");
    std::fs::write(&test_file, "").expect("write test source");
    std::fs::write(&dependency_file, "export const value = 1;").expect("write dependency");

    let resolver = specifier::create_resolver(&root, &[], &[]);
    let style_resolver = specifier::create_resolver(&root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let path_to_id = FxHashMap::from_iter([
        (aliased_source.as_path(), FileId(0)),
        (test_file.as_path(), FileId(1)),
        (dependency_file.as_path(), FileId(2)),
    ]);
    let raw_path_to_id = path_to_id.clone();
    let workspace_roots = FxHashMap::default();
    let package_manifests = Vec::new();
    let condition_names = react_native::build_condition_names(&[], &[]);
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: false,
        condition_names: &condition_names,
        path_aliases: &[],
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root: &root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };

    let donor_imports = resolve_static_imports(
        &ctx,
        &aliased_source,
        &[make_import(
            "shared-alias",
            ImportedName::Named("value".to_string()),
            "value",
        )],
    );
    assert!(matches!(
        donor_imports[0].target,
        ResolveResult::InternalModule(FileId(2))
    ));

    let mut operations = resolve_vitest_mock_operations(
        FileId(1),
        &[vitest_mock_fact("shared-alias", 10, true)],
        &ctx,
        &test_file,
    );
    assert!(matches!(operations[0].target, ResolveResult::NpmPackage(_)));

    let mut modules = vec![make_resolved_module(0, donor_imports, vec![], vec![])];
    apply_specifier_upgrades(&mut modules, &mut operations);

    assert_eq!(operations[0].target.internal_file_id(), Some(FileId(2)));
}

#[test]
fn static_imports_named() {
    with_empty_ctx(|ctx| {
        let imports = vec![make_import(
            "react",
            ImportedName::Named("useState".into()),
            "useState",
        )];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].info.source, "react");
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Named(ref n) if n == "useState"
        ));
    });
}

#[test]
fn static_imports_default() {
    with_empty_ctx(|ctx| {
        let imports = vec![make_import("react", ImportedName::Default, "React")];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Default
        ));
        assert_eq!(result[0].info.local_name, "React");
    });
}

#[test]
fn static_imports_namespace() {
    with_empty_ctx(|ctx| {
        let imports = vec![make_import("lodash", ImportedName::Namespace, "_")];
        let file = Path::new("/project/src/utils.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Namespace
        ));
        assert_eq!(result[0].info.local_name, "_");
    });
}

#[test]
fn static_imports_side_effect() {
    with_empty_ctx(|ctx| {
        let imports = vec![make_import("./styles.css", ImportedName::SideEffect, "")];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::SideEffect
        ));
        assert_eq!(result[0].info.local_name, "");
    });
}

#[test]
fn static_imports_empty_list() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &[]);
        assert!(result.is_empty());
    });
}

#[test]
fn static_imports_multiple() {
    with_empty_ctx(|ctx| {
        let imports = vec![
            make_import("react", ImportedName::Default, "React"),
            make_import("react", ImportedName::Named("useState".into()), "useState"),
            make_import("lodash", ImportedName::Namespace, "_"),
        ];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].info.source, "react");
        assert_eq!(result[1].info.source, "react");
        assert_eq!(result[2].info.source, "lodash");
    });
}

#[test]
fn static_imports_preserves_type_only() {
    with_empty_ctx(|ctx| {
        let imports = vec![ImportInfo {
            source: "react".into(),
            imported_name: ImportedName::Named("FC".into()),
            local_name: "FC".into(),
            is_type_only: true,
            is_type_only_star: false,
            from_style: false,
            span: dummy_span(),
            source_span: Span::default(),
        }];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 1);
        assert!(result[0].info.is_type_only);
    });
}

#[test]
fn dynamic_import_with_destructured_names() {
    with_empty_ctx(|ctx| {
        let imp = make_dynamic("./utils", vec!["foo", "bar"], None);
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);

        assert_eq!(result.len(), 2);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Named(ref n) if n == "foo"
        ));
        assert_eq!(result[0].info.local_name, "foo");
        assert!(matches!(
            result[1].info.imported_name,
            ImportedName::Named(ref n) if n == "bar"
        ));
        assert_eq!(result[1].info.local_name, "bar");
        assert_eq!(result[0].info.source, "./utils");
        assert_eq!(result[1].info.source, "./utils");
        assert!(!result[0].info.is_type_only);
        assert!(!result[1].info.is_type_only);
    });
}

#[test]
fn dynamic_import_namespace_with_local_name() {
    with_empty_ctx(|ctx| {
        let imp = make_dynamic("./utils", vec![], Some("utils"));
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Namespace
        ));
        assert_eq!(result[0].info.local_name, "utils");
    });
}

#[test]
fn dynamic_import_side_effect() {
    with_empty_ctx(|ctx| {
        let imp = make_dynamic("./polyfill", vec![], None);
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::SideEffect
        ));
        assert_eq!(result[0].info.local_name, "");
        assert_eq!(result[0].info.source, "./polyfill");
    });
}

#[test]
fn dynamic_import_destructured_takes_priority_over_local_name() {
    with_empty_ctx(|ctx| {
        let imp = DynamicImportInfo {
            source: "./mod".into(),
            span: dummy_span(),
            destructured_names: vec!["a".into()],
            local_name: Some("mod".into()),
            is_speculative: false,
        };
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Named(ref n) if n == "a"
        ));
    });
}

#[test]
fn dynamic_imports_flattens_multiple() {
    with_empty_ctx(|ctx| {
        let imports = vec![
            make_dynamic("./a", vec!["x", "y"], None),
            make_dynamic("./b", vec![], Some("b")),
            make_dynamic("./c", vec![], None),
        ];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_dynamic_imports(ctx, file, &imports);

        assert_eq!(result.len(), 4);
    });
}

#[test]
fn dynamic_imports_empty_list() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = resolve_dynamic_imports(ctx, file, &[]);
        assert!(result.is_empty());
    });
}

#[test]
fn re_exports_maps_each_entry() {
    with_empty_ctx(|ctx| {
        let re_exports = vec![
            make_re_export("./utils", "helper", "helper"),
            make_re_export("./types", "*", "*"),
        ];
        let file = Path::new("/project/src/index.ts");
        let result = resolve_re_exports(ctx, file, &re_exports);

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].info.source, "./utils");
        assert_eq!(result[0].info.imported_name, "helper");
        assert_eq!(result[0].info.exported_name, "helper");
        assert_eq!(result[1].info.source, "./types");
        assert_eq!(result[1].info.imported_name, "*");
    });
}

#[test]
fn re_exports_empty_list() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/index.ts");
        let result = resolve_re_exports(ctx, file, &[]);
        assert!(result.is_empty());
    });
}

#[test]
fn re_exports_preserves_type_only() {
    with_empty_ctx(|ctx| {
        let re_exports = vec![ReExportInfo {
            source: "./types".into(),
            imported_name: "MyType".into(),
            exported_name: "MyType".into(),
            is_type_only: true,
            span: oxc_span::Span::default(),
            statement_span: oxc_span::Span::new(0, 0),
            source_span: oxc_span::Span::new(0, 0),
        }];
        let file = Path::new("/project/src/index.ts");
        let result = resolve_re_exports(ctx, file, &re_exports);

        assert_eq!(result.len(), 1);
        assert!(result[0].info.is_type_only);
    });
}

#[test]
fn require_namespace_without_destructuring() {
    with_empty_ctx(|ctx| {
        let req = make_require("fs", vec![], Some("fs"));
        let file = Path::new("/project/src/app.js");
        let result = resolve_single_require(ctx, file, &req);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Namespace
        ));
        assert_eq!(result[0].info.local_name, "fs");
        assert_eq!(result[0].info.source, "fs");
    });
}

#[test]
fn require_namespace_without_local_name() {
    with_empty_ctx(|ctx| {
        let req = make_require("./side-effect", vec![], None);
        let file = Path::new("/project/src/app.js");
        let result = resolve_single_require(ctx, file, &req);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Namespace
        ));
        assert_eq!(result[0].info.local_name, "");
    });
}

#[test]
fn require_with_destructured_names() {
    with_empty_ctx(|ctx| {
        let req = make_require("path", vec!["join", "resolve"], None);
        let file = Path::new("/project/src/app.js");
        let result = resolve_single_require(ctx, file, &req);

        assert_eq!(result.len(), 2);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Named(ref n) if n == "join"
        ));
        assert_eq!(result[0].info.local_name, "join");
        assert!(matches!(
            result[1].info.imported_name,
            ImportedName::Named(ref n) if n == "resolve"
        ));
        assert_eq!(result[1].info.local_name, "resolve");
        assert_eq!(result[0].info.source, "path");
        assert_eq!(result[1].info.source, "path");
    });
}

#[test]
fn require_destructured_is_not_type_only() {
    with_empty_ctx(|ctx| {
        let req = make_require("path", vec!["join"], None);
        let file = Path::new("/project/src/app.js");
        let result = resolve_single_require(ctx, file, &req);

        assert_eq!(result.len(), 1);
        assert!(!result[0].info.is_type_only);
    });
}

#[test]
fn require_imports_flattens_multiple() {
    with_empty_ctx(|ctx| {
        let reqs = vec![
            make_require("fs", vec![], Some("fs")),
            make_require("path", vec!["join", "resolve"], None),
        ];
        let file = Path::new("/project/src/app.js");
        let result = resolve_require_imports(ctx, file, &reqs);

        assert_eq!(result.len(), 3);
    });
}

#[test]
fn require_imports_empty_list() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.js");
        let result = resolve_require_imports(ctx, file, &[]);
        assert!(result.is_empty());
    });
}

#[test]
fn specifier_upgrades_npm_to_internal_with_package_identity() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "preact/hooks",
                ResolveResult::InternalModule(FileId(5)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![make_resolved_import(
                "preact/hooks",
                ResolveResult::NpmPackage("preact".into()),
            )],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        modules[0].resolved_imports[0].target,
        ResolveResult::InternalModule(FileId(5))
    ));
    assert!(matches!(
        &modules[1].resolved_imports[0].target,
        ResolveResult::InternalPackageModule {
            file_id: FileId(5),
            package_name,
        } if package_name == "preact"
    ));
}

#[test]
fn specifier_upgrades_noop_when_no_internal() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "lodash",
                ResolveResult::NpmPackage("lodash".into()),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![make_resolved_import(
                "lodash",
                ResolveResult::NpmPackage("lodash".into()),
            )],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        modules[0].resolved_imports[0].target,
        ResolveResult::NpmPackage(_)
    ));
    assert!(matches!(
        modules[1].resolved_imports[0].target,
        ResolveResult::NpmPackage(_)
    ));
}

#[test]
fn specifier_upgrades_empty_modules() {
    let mut modules: Vec<ResolvedModule> = vec![];
    apply_specifier_upgrades(&mut modules, &mut []);
    assert!(modules.is_empty());
}

#[test]
fn specifier_upgrades_skips_relative_specifiers() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "./utils",
                ResolveResult::InternalModule(FileId(5)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![make_resolved_import(
                "./utils",
                ResolveResult::NpmPackage("utils".into()),
            )],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        modules[1].resolved_imports[0].target,
        ResolveResult::NpmPackage(_)
    ));
}

#[test]
fn specifier_upgrades_applies_to_dynamic_imports() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![],
            vec![make_resolved_import(
                "preact/hooks",
                ResolveResult::InternalModule(FileId(5)),
            )],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![],
            vec![make_resolved_import(
                "preact/hooks",
                ResolveResult::NpmPackage("preact".into()),
            )],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        &modules[1].resolved_dynamic_imports[0].target,
        ResolveResult::InternalPackageModule {
            file_id: FileId(5),
            package_name,
        } if package_name == "preact"
    ));
}

#[test]
fn specifier_upgrades_applies_to_re_exports() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![],
            vec![],
            vec![make_resolved_re_export(
                "preact/hooks",
                ResolveResult::InternalModule(FileId(5)),
            )],
        ),
        make_resolved_module(
            1,
            vec![],
            vec![],
            vec![make_resolved_re_export(
                "preact/hooks",
                ResolveResult::NpmPackage("preact".into()),
            )],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        &modules[1].re_exports[0].target,
        ResolveResult::InternalPackageModule {
            file_id: FileId(5),
            package_name,
        } if package_name == "preact"
    ));
}

#[test]
fn specifier_upgrades_does_not_downgrade_internal() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "preact/hooks",
                ResolveResult::InternalModule(FileId(5)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![make_resolved_import(
                "preact/hooks",
                ResolveResult::InternalModule(FileId(5)),
            )],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        modules[0].resolved_imports[0].target,
        ResolveResult::InternalModule(FileId(5))
    ));
    assert!(matches!(
        modules[1].resolved_imports[0].target,
        ResolveResult::InternalModule(FileId(5))
    ));
}

#[test]
fn specifier_upgrades_first_internal_wins() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "shared-lib",
                ResolveResult::InternalModule(FileId(10)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![make_resolved_import(
                "shared-lib",
                ResolveResult::InternalModule(FileId(20)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            2,
            vec![make_resolved_import(
                "shared-lib",
                ResolveResult::NpmPackage("shared-lib".into()),
            )],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        &modules[2].resolved_imports[0].target,
        ResolveResult::InternalPackageModule {
            file_id: FileId(10),
            package_name,
        } if package_name == "shared-lib"
    ));
}

#[test]
fn specifier_upgrades_does_not_touch_unresolvable() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "my-lib",
                ResolveResult::InternalModule(FileId(1)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![ResolvedImport {
                info: make_import("my-lib", ImportedName::Default, "myLib"),
                target: ResolveResult::Unresolvable("my-lib".into()),
            }],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        modules[1].resolved_imports[0].target,
        ResolveResult::Unresolvable(_)
    ));
}

#[test]
fn specifier_upgrades_cross_import_and_re_export() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "@myorg/utils",
                ResolveResult::InternalModule(FileId(3)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![],
            vec![],
            vec![make_resolved_re_export(
                "@myorg/utils",
                ResolveResult::NpmPackage("@myorg/utils".into()),
            )],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        &modules[1].re_exports[0].target,
        ResolveResult::InternalPackageModule {
            file_id: FileId(3),
            package_name,
        } if package_name == "@myorg/utils"
    ));
}

#[test]
fn dynamic_patterns_matches_files_in_dir() {
    let from_dir = Path::new("/project/src");
    let patterns = vec![DynamicImportPattern {
        prefix: "./locales/".into(),
        suffix: Some(".json".into()),
        span: dummy_span(),
        mechanism: ModuleLoadMechanism::EsModule,
    }];
    let canonical_paths = vec![
        PathBuf::from("/project/src/locales/en.json"),
        PathBuf::from("/project/src/locales/fr.json"),
        PathBuf::from("/project/src/utils.ts"),
    ];
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/locales/en.json"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/locales/fr.json"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/src/utils.ts"),
            size_bytes: 100,
        },
    ];

    let result = resolve_dynamic_patterns(
        &GlobMatcherCache::default(),
        from_dir,
        &patterns,
        &canonical_paths,
        &files,
    );

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].1.len(), 2);
    assert!(result[0].1.contains(&FileId(0)));
    assert!(result[0].1.contains(&FileId(1)));
}

#[test]
fn dynamic_patterns_no_matches_returns_empty() {
    let from_dir = Path::new("/project/src");
    let patterns = vec![DynamicImportPattern {
        prefix: "./locales/".into(),
        suffix: Some(".json".into()),
        span: dummy_span(),
        mechanism: ModuleLoadMechanism::EsModule,
    }];
    let canonical_paths = vec![PathBuf::from("/project/src/utils.ts")];
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/utils.ts"),
        size_bytes: 100,
    }];

    let result = resolve_dynamic_patterns(
        &GlobMatcherCache::default(),
        from_dir,
        &patterns,
        &canonical_paths,
        &files,
    );

    assert!(result.is_empty());
}

#[test]
fn dynamic_patterns_empty_patterns_list() {
    let from_dir = Path::new("/project/src");
    let canonical_paths = vec![PathBuf::from("/project/src/utils.ts")];
    let files = vec![DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from("/project/src/utils.ts"),
        size_bytes: 100,
    }];

    let result = resolve_dynamic_patterns(
        &GlobMatcherCache::default(),
        from_dir,
        &[],
        &canonical_paths,
        &files,
    );
    assert!(result.is_empty());
}

#[test]
fn dynamic_patterns_glob_prefix_passthrough() {
    let from_dir = Path::new("/project/src");
    let patterns = vec![DynamicImportPattern {
        prefix: "./**/*.ts".into(),
        suffix: None,
        span: dummy_span(),
        mechanism: ModuleLoadMechanism::EsModule,
    }];
    let canonical_paths = vec![
        PathBuf::from("/project/src/utils.ts"),
        PathBuf::from("/project/src/deep/nested.ts"),
    ];
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/utils.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/deep/nested.ts"),
            size_bytes: 100,
        },
    ];

    let result = resolve_dynamic_patterns(
        &GlobMatcherCache::default(),
        from_dir,
        &patterns,
        &canonical_paths,
        &files,
    );

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].1.len(), 2);
}

#[test]
fn static_import_unresolvable_relative_path() {
    with_empty_ctx(|ctx| {
        let imports = vec![make_import(
            "./nonexistent",
            ImportedName::Default,
            "missing",
        )];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].target, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn static_import_bare_specifier_becomes_npm_package() {
    with_empty_ctx(|ctx| {
        let imports = vec![make_import("react", ImportedName::Default, "React")];
        let file = Path::new("/project/src/app.ts");
        let result = resolve_static_imports(ctx, file, &imports);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].target,
            ResolveResult::NpmPackage(ref pkg) if pkg == "react"
        ));
    });
}

#[test]
fn require_bare_specifier_retains_commonjs_provenance() {
    with_empty_ctx(|ctx| {
        let req = make_require("express", vec![], Some("express"));
        let file = Path::new("/project/src/app.js");
        let result = resolve_single_require(ctx, file, &req);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].target,
            ResolveResult::CommonJsNpmPackage(ref pkg) if pkg == "express"
        ));
    });
}

#[test]
fn dynamic_import_unresolvable() {
    with_empty_ctx(|ctx| {
        let imp = make_dynamic("./missing-module", vec![], None);
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].target, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn re_export_unresolvable() {
    with_empty_ctx(|ctx| {
        let re_exports = vec![make_re_export("./missing", "foo", "foo")];
        let file = Path::new("/project/src/index.ts");
        let result = resolve_re_exports(ctx, file, &re_exports);

        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].target, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn specifier_upgrades_re_export_triggers_import_upgrade() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![],
            vec![],
            vec![make_resolved_re_export(
                "@myorg/shared",
                ResolveResult::InternalModule(FileId(5)),
            )],
        ),
        make_resolved_module(
            1,
            vec![make_resolved_import(
                "@myorg/shared",
                ResolveResult::NpmPackage("@myorg/shared".into()),
            )],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        &modules[1].resolved_imports[0].target,
        ResolveResult::InternalPackageModule {
            file_id: FileId(5),
            package_name,
        } if package_name == "@myorg/shared"
    ));
}

#[test]
fn specifier_upgrades_re_export_triggers_dynamic_import_upgrade() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![],
            vec![],
            vec![make_resolved_re_export(
                "my-workspace-pkg",
                ResolveResult::InternalModule(FileId(7)),
            )],
        ),
        make_resolved_module(
            1,
            vec![],
            vec![make_resolved_import(
                "my-workspace-pkg",
                ResolveResult::NpmPackage("my-workspace-pkg".into()),
            )],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        &modules[1].resolved_dynamic_imports[0].target,
        ResolveResult::InternalPackageModule {
            file_id: FileId(7),
            package_name,
        } if package_name == "my-workspace-pkg"
    ));
}

#[test]
fn specifier_upgrades_does_not_upgrade_external_file() {
    let mut modules = vec![
        make_resolved_module(
            0,
            vec![make_resolved_import(
                "shared-lib",
                ResolveResult::InternalModule(FileId(3)),
            )],
            vec![],
            vec![],
        ),
        make_resolved_module(
            1,
            vec![ResolvedImport {
                info: make_import("shared-lib", ImportedName::Default, "lib"),
                target: ResolveResult::ExternalFile(PathBuf::from(
                    "/node_modules/shared-lib/index.js",
                )),
            }],
            vec![],
            vec![],
        ),
    ];

    apply_specifier_upgrades(&mut modules, &mut []);

    assert!(matches!(
        modules[1].resolved_imports[0].target,
        ResolveResult::ExternalFile(_)
    ));
}

#[test]
fn dynamic_patterns_prefix_without_suffix() {
    let from_dir = Path::new("/project/src");
    let patterns = vec![DynamicImportPattern {
        prefix: "./pages/".into(),
        suffix: None,
        span: dummy_span(),
        mechanism: ModuleLoadMechanism::EsModule,
    }];
    let canonical_paths = vec![
        PathBuf::from("/project/src/pages/Home.tsx"),
        PathBuf::from("/project/src/pages/About.tsx"),
        PathBuf::from("/project/src/utils.ts"),
    ];
    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/pages/Home.tsx"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/pages/About.tsx"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/src/utils.ts"),
            size_bytes: 100,
        },
    ];

    let result = resolve_dynamic_patterns(
        &GlobMatcherCache::default(),
        from_dir,
        &patterns,
        &canonical_paths,
        &files,
    );

    assert_eq!(result.len(), 1);
    assert_eq!(result[0].1.len(), 2);
    assert!(result[0].1.contains(&FileId(0)));
    assert!(result[0].1.contains(&FileId(1)));
}

#[test]
fn dynamic_patterns_empty_canonical_paths() {
    let from_dir = Path::new("/project/src");
    let patterns = vec![DynamicImportPattern {
        prefix: "./locales/".into(),
        suffix: Some(".json".into()),
        span: dummy_span(),
        mechanism: ModuleLoadMechanism::EsModule,
    }];

    let result =
        resolve_dynamic_patterns(&GlobMatcherCache::default(), from_dir, &patterns, &[], &[]);
    assert!(result.is_empty());
}

#[test]
fn require_destructured_empty_names_uses_namespace() {
    with_empty_ctx(|ctx| {
        let req = RequireCallInfo {
            source: "path".into(),
            span: dummy_span(),
            destructured_names: vec![],
            local_name: None,
            source_span: oxc_span::Span::default(),
            is_type_only: false,
        };
        let file = Path::new("/project/src/app.js");
        let result = resolve_single_require(ctx, file, &req);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::Namespace
        ));
    });
}

#[test]
fn dynamic_import_empty_destructured_and_no_local_is_side_effect() {
    with_empty_ctx(|ctx| {
        let imp = DynamicImportInfo {
            source: "./init".into(),
            span: dummy_span(),
            destructured_names: vec![],
            local_name: None,
            is_speculative: false,
        };
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);

        assert_eq!(result.len(), 1);
        assert!(matches!(
            result[0].info.imported_name,
            ImportedName::SideEffect
        ));
        assert_eq!(result[0].info.local_name, "");
    });
}

#[test]
fn speculative_dynamic_import_drops_when_unresolvable() {
    with_empty_ctx(|ctx| {
        let imp = DynamicImportInfo {
            source: "./services/__mocks__/api".into(),
            span: dummy_span(),
            destructured_names: vec![],
            local_name: Some(String::new()),
            is_speculative: true,
        };
        let file = Path::new("/project/src/app.test.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);
        assert!(
            result.is_empty(),
            "speculative imports whose target is Unresolvable must be dropped, got: {result:?}"
        );
    });
}

#[test]
fn speculative_dynamic_import_drops_when_package_space() {
    with_empty_ctx(|ctx| {
        // `jest.mock('@bacons/apple-targets')` synthesizes the sibling
        // candidate `@bacons/__mocks__/apple-targets`. Bare specifiers are
        // never Unresolvable, so without the package-space drop this would
        // surface as phantom package `@bacons/__mocks__` (issue #2213).
        let imp = DynamicImportInfo {
            source: "@bacons/__mocks__/apple-targets".into(),
            span: dummy_span(),
            destructured_names: vec![],
            local_name: Some(String::new()),
            is_speculative: true,
        };
        let file = Path::new("/project/src/app.test.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);
        assert!(
            result.is_empty(),
            "speculative imports landing in package space must be dropped, got: {result:?}"
        );
    });
}

#[test]
fn non_speculative_dynamic_import_keeps_package_entry() {
    with_empty_ctx(|ctx| {
        let imp = DynamicImportInfo {
            source: "@bacons/apple-targets".into(),
            span: dummy_span(),
            destructured_names: vec![],
            local_name: None,
            is_speculative: false,
        };
        let file = Path::new("/project/src/app.test.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].target, ResolveResult::NpmPackage(_)));
    });
}

#[test]
fn non_speculative_dynamic_import_keeps_unresolvable_entry() {
    with_empty_ctx(|ctx| {
        let imp = DynamicImportInfo {
            source: "./missing".into(),
            span: dummy_span(),
            destructured_names: vec![],
            local_name: None,
            is_speculative: false,
        };
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);
        assert_eq!(result.len(), 1);
        assert!(matches!(result[0].target, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn dynamic_import_preserves_source_span() {
    with_empty_ctx(|ctx| {
        let imp = DynamicImportInfo {
            source: "./lazy".into(),
            span: Span::new(42, 84),
            destructured_names: vec!["x".into()],
            local_name: None,
            is_speculative: false,
        };
        let file = Path::new("/project/src/app.ts");
        let result = resolve_single_dynamic_import(ctx, file, &imp);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].info.span.start, 42);
        assert_eq!(result[0].info.span.end, 84);
    });
}

#[test]
fn specifier_upgrade_preserves_actual_require_mechanism() {
    with_empty_ctx(|ctx| {
        let require_imports = resolve_single_require(
            ctx,
            Path::new("/project/src/consumer.js"),
            &make_require("shared-package", vec![], Some("shared")),
        );
        assert!(matches!(
            require_imports[0].target,
            ResolveResult::CommonJsNpmPackage(_)
        ));

        let mut modules = vec![
            make_resolved_module(
                0,
                vec![make_resolved_import(
                    "shared-package",
                    ResolveResult::InternalModule(FileId(2)),
                )],
                vec![],
                vec![],
            ),
            make_resolved_module(1, require_imports, vec![], vec![]),
        ];
        apply_specifier_upgrades(&mut modules, &mut []);

        assert!(matches!(
            &modules[1].resolved_imports[0].target,
            ResolveResult::CommonJsInternalPackageModule {
                file_id: FileId(2),
                package_name,
            } if package_name == "shared-package"
        ));
    });
}

#[test]
fn specifier_https_url_returns_external_file() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result =
            specifier::resolve_specifier(ctx, file, "https://cdn.example.com/lib.js", false);

        assert!(
            matches!(result, ResolveResult::ExternalFile(ref p) if p.to_str().unwrap() == "https://cdn.example.com/lib.js")
        );
    });
}

#[test]
fn specifier_http_url_returns_external_file() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "http://example.com/module.js", false);

        assert!(
            matches!(result, ResolveResult::ExternalFile(ref p) if p.to_str().unwrap() == "http://example.com/module.js")
        );
    });
}

#[test]
fn specifier_data_url_returns_external_file() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(
            ctx,
            file,
            "data:text/javascript,export default 42",
            false,
        );

        assert!(
            matches!(result, ResolveResult::ExternalFile(ref p) if p.to_str().unwrap() == "data:text/javascript,export default 42")
        );
    });
}

#[test]
fn specifier_custom_protocol_returns_external_file() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "vscode://extension/my-ext", false);

        assert!(matches!(result, ResolveResult::ExternalFile(_)));
    });
}

#[test]
fn specifier_jsr_scheme_returns_external_file() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/supabase/functions/hello/index.ts");
        let result = specifier::resolve_specifier(ctx, file, "jsr:@std/path", false);

        assert!(
            matches!(result, ResolveResult::ExternalFile(ref p) if p.to_str().unwrap() == "jsr:@std/path")
        );
    });
}

#[test]
fn specifier_npm_scheme_scoped_credits_package() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/supabase/functions/hello/index.ts");
        let result = specifier::resolve_specifier(ctx, file, "npm:@supabase/supabase-js@2", false);

        assert!(
            matches!(result, ResolveResult::NpmPackage(ref name) if name == "@supabase/supabase-js"),
            "expected NpmPackage(@supabase/supabase-js), got {result:?}"
        );
    });
}

#[test]
fn specifier_npm_scheme_unscoped_credits_package() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/supabase/functions/hello/index.ts");
        let result = specifier::resolve_specifier(ctx, file, "npm:zod@3", false);

        assert!(
            matches!(result, ResolveResult::NpmPackage(ref name) if name == "zod"),
            "expected NpmPackage(zod), got {result:?}"
        );
    });
}

#[test]
fn specifier_bare_npm_scheme_returns_external_file() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/supabase/functions/hello/index.ts");
        let result = specifier::resolve_specifier(ctx, file, "npm:", false);

        assert!(matches!(result, ResolveResult::ExternalFile(_)));
    });
}

#[test]
#[cfg_attr(miri, ignore)]
fn pnpm_package_source_alias_preserves_declared_import_name() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    let source_pkg = root
        .join("node_modules/.pnpm/unstorage-nightly@2.0.0-alpha.5/node_modules/unstorage-nightly");
    std::fs::create_dir_all(&source_pkg).unwrap();
    std::fs::write(
        source_pkg.join("index.js"),
        "export const createStorage = () => ({});",
    )
    .unwrap();
    std::fs::write(
        source_pkg.join("package.json"),
        r#"{"name":"unstorage-nightly","exports":"./index.js"}"#,
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_pkg, root.join("node_modules/unstorage")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source_pkg, root.join("node_modules/unstorage")).unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let style_resolver = specifier::create_resolver(root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let path_to_id = FxHashMap::default();
    let raw_path_to_id = FxHashMap::default();
    let workspace_roots = FxHashMap::default();
    let package_manifests = Vec::new();
    let condition_names = react_native::build_condition_names(&[], &[]);
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: false,
        condition_names: &condition_names,
        path_aliases: &[],
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };

    let result = specifier::resolve_specifier(&ctx, &root.join("app.ts"), "unstorage", false);
    assert!(
        matches!(result, ResolveResult::NpmPackage(ref name) if name == "unstorage"),
        "expected package usage to preserve declared import name, got {result:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn pnpm_jsr_package_source_alias_preserves_declared_import_name() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    let source_pkg = root.join("node_modules/.pnpm/@jsr+std__csv@1.0.6/node_modules/@jsr/std__csv");
    std::fs::create_dir_all(&source_pkg).unwrap();
    std::fs::write(
        source_pkg.join("stringify.js"),
        "export const stringify = () => '';",
    )
    .unwrap();
    std::fs::write(
        source_pkg.join("package.json"),
        r#"{"name":"@jsr/std__csv","exports":{"./stringify":"./stringify.js"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("node_modules/@std")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&source_pkg, root.join("node_modules/@std/csv")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(&source_pkg, root.join("node_modules/@std/csv")).unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let style_resolver = specifier::create_resolver(root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let path_to_id = FxHashMap::default();
    let raw_path_to_id = FxHashMap::default();
    let workspace_roots = FxHashMap::default();
    let package_manifests = Vec::new();
    let condition_names = react_native::build_condition_names(&[], &[]);
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: false,
        condition_names: &condition_names,
        path_aliases: &[],
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };

    let result =
        specifier::resolve_specifier(&ctx, &root.join("app.ts"), "@std/csv/stringify", false);
    assert!(
        matches!(result, ResolveResult::NpmPackage(ref name) if name == "@std/csv"),
        "expected package usage to preserve declared import name, got {result:?}"
    );
}

#[test]
fn package_usage_name_prefers_declared_name_for_bare_specifiers() {
    // pnpm source alias: declared `unstorage` resolved into a `.pnpm` store dir
    // whose inner package is `unstorage-nightly`. Credit the declared name.
    let resolved =
        Path::new("/p/node_modules/.pnpm/unstorage-nightly@2.0.0/node_modules/unstorage-nightly");
    assert_eq!(
        specifier::package_usage_name_for_resolved_package("unstorage", resolved),
        Some("unstorage".to_string()),
    );

    // Scoped subpath bare specifier reduces to the package name.
    let resolved_scoped =
        Path::new("/p/node_modules/.pnpm/@jsr+std__csv@1.0.6/node_modules/@jsr/std__csv");
    assert_eq!(
        specifier::package_usage_name_for_resolved_package("@std/csv/stringify", resolved_scoped),
        Some("@std/csv".to_string()),
    );

    // Common case (declared name == source name) is unchanged.
    let resolved_plain = Path::new("/p/node_modules/lodash");
    assert_eq!(
        specifier::package_usage_name_for_resolved_package("lodash", resolved_plain),
        Some("lodash".to_string()),
    );

    // Path aliases that are bare (Node.js `#imports`) must NOT be credited as the
    // package name: they can map to an external package whose real name is only on
    // the resolved path. Keep the resolved-package name instead.
    let resolved_import_map = Path::new("/p/node_modules/polyfill-pkg/index.js");
    assert_eq!(
        specifier::package_usage_name_for_resolved_package("#polyfill", resolved_import_map),
        Some("polyfill-pkg".to_string()),
    );

    // Not under node_modules: no package-usage credit.
    assert_eq!(
        specifier::package_usage_name_for_resolved_package("unstorage", Path::new("/p/src/x.ts")),
        None,
    );
}

#[test]
fn external_bare_specifier_package_usage_credits_declared_package() {
    assert_eq!(
        specifier::package_usage_name_for_external_bare_specifier("@mre/shared"),
        Some("@mre/shared".to_string()),
    );
    assert_eq!(
        specifier::package_usage_name_for_external_bare_specifier("@mre/shared/subpath"),
        Some("@mre/shared".to_string()),
    );
    assert_eq!(
        specifier::package_usage_name_for_external_bare_specifier("@mre"),
        None,
    );
    assert_eq!(
        specifier::package_usage_name_for_external_bare_specifier("#polyfill"),
        None,
    );
    assert_eq!(
        specifier::package_usage_name_for_external_bare_specifier("@/shared"),
        None,
    );
}

#[test]
fn specifier_html_root_relative_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/public/index.html");
        let result = specifier::resolve_specifier(ctx, file, "/src/main.tsx", false);

        assert!(
            matches!(result, ResolveResult::Unresolvable(ref s) if s == "/src/main.tsx"),
            "HTML root-relative path that fails resolution should be Unresolvable"
        );
    });
}

#[test]
fn specifier_html_root_relative_deep_path_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/nested/deep/page.html");
        let result = specifier::resolve_specifier(ctx, file, "/assets/styles/main.css", false);

        assert!(
            matches!(result, ResolveResult::Unresolvable(ref s) if s == "/assets/styles/main.css")
        );
    });
}

#[test]
fn specifier_root_relative_in_ts_file_unresolvable_when_missing() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "/usr/local/lib/something", false);

        assert!(matches!(
            result,
            ResolveResult::Unresolvable(ref s) if s == "/usr/local/lib/something"
        ));
    });
}

#[test]
fn specifier_path_alias_hash_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "#internal/utils", false);

        assert!(
            matches!(result, ResolveResult::Unresolvable(ref s) if s == "#internal/utils"),
            "Failed path alias resolution should be Unresolvable, not NpmPackage"
        );
    });
}

#[test]
fn specifier_path_alias_tilde_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "~/components/Button", false);

        assert!(matches!(result, ResolveResult::Unresolvable(ref s) if s == "~/components/Button"));
    });
}

#[test]
fn specifier_path_alias_double_tilde_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "~~/utils/helpers", false);

        assert!(matches!(result, ResolveResult::Unresolvable(ref s) if s == "~~/utils/helpers"));
    });
}

#[test]
fn specifier_path_alias_at_slash_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "@/components/Foo", false);

        assert!(matches!(result, ResolveResult::Unresolvable(ref s) if s == "@/components/Foo"));
    });
}

#[test]
fn specifier_pascal_scope_alias_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "@Components/Button", false);

        assert!(matches!(result, ResolveResult::Unresolvable(ref s) if s == "@Components/Button"));
    });
}

#[test]
#[cfg_attr(miri, ignore)] // oxc_resolver uses statx syscall unsupported by Miri
fn specifier_plugin_alias_match_returns_unresolvable() {
    let root = PathBuf::from("/project");
    let resolver = specifier::create_resolver(&root, &[], &[]);
    let style_resolver = specifier::create_resolver(&root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let path_to_id = FxHashMap::default();
    let raw_path_to_id = FxHashMap::default();
    let workspace_roots = FxHashMap::default();
    let package_manifests = Vec::new();
    let condition_names = react_native::build_condition_names(&[], &[]);
    let aliases = vec![("$lib/".to_string(), "src/lib/".to_string())];
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: false,
        condition_names: &condition_names,
        path_aliases: &aliases,
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root: &root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };

    let file = Path::new("/project/src/app.ts");
    let result = specifier::resolve_specifier(&ctx, file, "$lib/utils", false);

    assert!(
        matches!(result, ResolveResult::Unresolvable(ref s) if s == "$lib/utils"),
        "Plugin alias that fails resolution should be Unresolvable"
    );
}

#[test]
fn specifier_bare_scoped_package_returns_npm_package() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "@babel/core/transform", false);

        assert!(
            matches!(result, ResolveResult::NpmPackage(ref pkg) if pkg == "@babel/core"),
            "Scoped bare specifier should extract package name correctly"
        );
    });
}

#[test]
fn specifier_bare_unscoped_package_returns_npm_package() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "lodash/merge", false);

        assert!(matches!(result, ResolveResult::NpmPackage(ref pkg) if pkg == "lodash"));
    });
}

#[test]
fn specifier_invalid_package_name_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "$DIR", false);

        assert!(matches!(result, ResolveResult::Unresolvable(ref s) if s == "$DIR"));
    });
}

#[test]
fn specifier_bundler_internal_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result =
            specifier::resolve_specifier(ctx, file, "raw-loader?esModule=false!./data.csv", false);

        assert!(matches!(result, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn specifier_double_underscore_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "__barrel_optimize__", false);

        assert!(matches!(result, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn specifier_pure_numeric_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "123", false);

        assert!(matches!(result, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn specifier_relative_path_missing_is_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "./nonexistent/module", false);

        assert!(matches!(result, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn specifier_parent_relative_path_missing_is_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/deep/nested/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "../../missing", false);

        assert!(matches!(result, ResolveResult::Unresolvable(_)));
    });
}

#[test]
fn specifier_at_at_slash_returns_unresolvable() {
    with_empty_ctx(|ctx| {
        let file = Path::new("/project/src/app.ts");
        let result = specifier::resolve_specifier(ctx, file, "@@/shared/utils", false);

        assert!(matches!(result, ResolveResult::Unresolvable(ref s) if s == "@@/shared/utils"));
    });
}

/// Write a minimal inlined Yarn PnP manifest at `root` that maps the bare
/// package `foo@npm:1.0.0` to an unplugged (unzipped) install directory, plus
/// `src/app.ts` as an issuer, and return the package directory. Mirrors the
/// shape Yarn 4 writes: a `null` top-level entry, one workspace at `./`, and
/// per-package `packageLocation` values relative to the manifest.
fn write_yarn_pnp_fixture(root: &Path) -> PathBuf {
    let package_dir = root.join(".yarn/unplugged/foo-npm-1.0.0-0123456789/node_modules/foo");
    std::fs::create_dir_all(&package_dir).unwrap();
    std::fs::write(
        package_dir.join("package.json"),
        r#"{"name":"foo","version":"1.0.0","main":"index.js"}"#,
    )
    .unwrap();
    std::fs::write(package_dir.join("index.js"), "module.exports = 'foo';\n").unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/app.ts"), "import foo from 'foo';\n").unwrap();

    let manifest = concat!(
        "#!/usr/bin/env node\n",
        "/* eslint-disable */\n",
        "\"use strict\";\n",
        "\n",
        "const RAW_RUNTIME_STATE =\n",
        "'{\"dependencyTreeRoots\":[{\"name\":\"app\",\"reference\":\"workspace:.\"}],",
        "\"enableTopLevelFallback\":true,",
        "\"ignorePatternData\":null,",
        "\"fallbackExclusionList\":[[\"app\",[\"workspace:.\"]]],",
        "\"fallbackPool\":[],",
        "\"packageRegistryData\":[",
        "[null,[[null,{\"packageLocation\":\"./\",\"packageDependencies\":[],",
        "\"linkType\":\"SOFT\"}]]],",
        "[\"app\",[[\"workspace:.\",{\"packageLocation\":\"./\",",
        "\"packageDependencies\":[[\"app\",\"workspace:.\"],[\"foo\",\"npm:1.0.0\"]],",
        "\"linkType\":\"SOFT\"}]]],",
        "[\"foo\",[[\"npm:1.0.0\",{\"packageLocation\":",
        "\"./.yarn/unplugged/foo-npm-1.0.0-0123456789/node_modules/foo/\",",
        "\"packageDependencies\":[[\"foo\",\"npm:1.0.0\"]],\"linkType\":\"HARD\"}]]]",
        "]}';\n",
        "\n",
        "function $$SETUP_STATE(hydrateRuntimeState, basePath) {\n",
        "  return hydrateRuntimeState(JSON.parse(RAW_RUNTIME_STATE), ",
        "{basePath: basePath || __dirname});\n",
        "}\n",
    );
    std::fs::write(root.join(".pnp.cjs"), manifest).unwrap();
    package_dir
}

/// Yarn PnP projects have no `node_modules`; bare specifiers go through the
/// inlined `.pnp.cjs` manifest instead. oxc locates that manifest from the
/// resolver's `cwd` (falling back to the process working directory) and fallow
/// never chdirs, so the resolver must anchor it to the project itself. The
/// test therefore runs with the process cwd elsewhere and must not chdir.
#[test]
#[cfg_attr(miri, ignore)] // oxc_resolver uses statx syscall unsupported by Miri
fn yarn_pnp_bare_specifier_resolves_through_the_manifest_without_chdir() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let package_dir = write_yarn_pnp_fixture(&root);
    assert_ne!(
        std::env::current_dir().ok().as_deref(),
        Some(root.as_path()),
        "the test must exercise cwd-independent manifest discovery"
    );

    let resolver = specifier::create_resolver(&root, &[], &[]);
    let resolved = resolver
        .resolve_file(root.join("src/app.ts"), "foo")
        .expect("foo resolves through .pnp.cjs");

    assert_eq!(resolved.full_path(), package_dir.join("index.js"));
}

/// Yarn writes the manifest only at the workspace root. Analyzing one package
/// of a PnP monorepo must still find it and resolve through it.
#[test]
#[cfg_attr(miri, ignore)] // oxc_resolver uses statx syscall unsupported by Miri
fn yarn_pnp_sub_package_root_resolves_through_the_workspace_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let package_dir = write_yarn_pnp_fixture(&root);
    let app_root = root.join("packages/app");
    std::fs::create_dir_all(app_root.join("src")).unwrap();
    std::fs::write(app_root.join("src/index.ts"), "import foo from 'foo';\n").unwrap();

    let resolver = specifier::create_resolver(&app_root, &[], &[]);
    let resolved = resolver
        .resolve_file(app_root.join("src/index.ts"), "foo")
        .expect("foo resolves through the ancestor .pnp.cjs");

    assert_eq!(resolved.full_path(), package_dir.join("index.js"));
}

/// The style resolver is derived from the main one and shares its cache; it
/// must resolve through the same manifest.
#[test]
#[cfg_attr(miri, ignore)] // oxc_resolver uses statx syscall unsupported by Miri
fn yarn_pnp_style_resolver_derived_via_clone_with_options_resolves_too() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let package_dir = write_yarn_pnp_fixture(&root);

    let resolver = specifier::create_resolver(&root, &[], &[]);
    let style_resolver = resolver.clone_with_options(specifier::build_resolve_options(
        &root,
        &[],
        &["style".to_string()],
    ));
    let resolved = style_resolver
        .resolve_file(root.join("src/app.ts"), "foo")
        .expect("foo resolves through .pnp.cjs");

    assert_eq!(resolved.full_path(), package_dir.join("index.js"));
}

/// Without a manifest the resolver stays on the `node_modules` path: a missing
/// bare specifier is a plain miss, not a PnP manifest error.
#[test]
#[cfg_attr(miri, ignore)] // oxc_resolver uses statx syscall unsupported by Miri
fn non_pnp_root_reports_missing_bare_specifier_as_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    std::fs::write(root.join("package.json"), r#"{"name":"app"}"#).unwrap();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/app.ts"), "import foo from 'foo';\n").unwrap();

    let resolver = specifier::create_resolver(&root, &[], &[]);
    let err = resolver
        .resolve_file(root.join("src/app.ts"), "foo")
        .expect_err("no node_modules and no manifest");

    assert!(
        matches!(err, oxc_resolver::ResolveError::NotFound(_)),
        "expected NotFound, got {err:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn create_resolver_without_plugins() {
    let _resolver = specifier::create_resolver(options_only_root(), &[], &[]);
}

#[test]
#[cfg_attr(miri, ignore)]
fn create_resolver_with_react_native_plugin() {
    let plugins = vec!["react-native".to_string()];
    let _resolver = specifier::create_resolver(options_only_root(), &plugins, &[]);
}

#[test]
#[cfg_attr(miri, ignore)]
fn create_resolver_with_expo_plugin() {
    let plugins = vec!["expo".to_string()];
    let _resolver = specifier::create_resolver(options_only_root(), &plugins, &[]);
}

#[test]
#[cfg_attr(miri, ignore)]
fn create_resolver_with_multiple_plugins() {
    let plugins = vec![
        "react-native".to_string(),
        "typescript".to_string(),
        "jest".to_string(),
    ];
    let _resolver = specifier::create_resolver(options_only_root(), &plugins, &[]);
}

#[test]
#[cfg_attr(miri, ignore)]
fn create_resolver_with_custom_conditions() {
    let conditions = vec!["worker".to_string(), "edge-light".to_string()];
    let _resolver = specifier::create_resolver(options_only_root(), &[], &conditions);
}

#[test]
#[cfg_attr(miri, ignore)] // oxc_resolver uses statx syscall unsupported by Miri
fn resolve_prefers_js_over_dts_when_both_exist() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    std::fs::write(root.join("utils.js"), "export const helper = 1;").unwrap();
    std::fs::write(
        root.join("utils.d.ts"),
        "export declare const helper: number;",
    )
    .unwrap();
    std::fs::write(root.join("app.ts"), "import { helper } from './utils';").unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let from_file = root.join("app.ts");
    let result = resolver.resolve_file(&from_file, "./utils");

    assert!(result.is_ok(), "should resolve ./utils successfully");
    let resolved_path = result.unwrap().into_path_buf();
    let resolved_name = resolved_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(
        resolved_name, "utils.js",
        "should resolve to utils.js (runtime), not utils.d.ts (declaration)"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn resolve_prefers_ts_over_dts_when_both_exist() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    std::fs::write(root.join("utils.ts"), "export const helper = 1;").unwrap();
    std::fs::write(
        root.join("utils.d.ts"),
        "export declare const helper: number;",
    )
    .unwrap();
    std::fs::write(root.join("app.ts"), "import { helper } from './utils';").unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let from_file = root.join("app.ts");
    let result = resolver.resolve_file(&from_file, "./utils");

    assert!(result.is_ok(), "should resolve ./utils successfully");
    let resolved_path = result.unwrap().into_path_buf();
    let resolved_name = resolved_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(
        resolved_name, "utils.ts",
        "should resolve to utils.ts (runtime), not utils.d.ts (declaration)"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn resolve_falls_back_to_dts_when_no_runtime_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    std::fs::write(root.join("types.d.ts"), "export declare const x: number;").unwrap();
    std::fs::write(root.join("app.ts"), "import { x } from './types';").unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let from_file = root.join("app.ts");
    let result = resolver.resolve_file(&from_file, "./types");

    assert!(result.is_ok(), "should resolve ./types to .d.ts fallback");
    let resolved_path = result.unwrap().into_path_buf();
    let resolved_name = resolved_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(
        resolved_name, "types.d.ts",
        "should resolve to types.d.ts when no runtime file exists"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn resolve_js_specifier_falls_back_to_dts_when_no_runtime_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    std::fs::write(
        root.join("contract.d.ts"),
        "export declare const x: number;",
    )
    .unwrap();
    std::fs::write(root.join("app.ts"), "import { x } from './contract.js';").unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let from_file = root.join("app.ts");
    let result = resolver.resolve_file(&from_file, "./contract.js");

    assert!(
        result.is_ok(),
        "should resolve ./contract.js to the declaration-only module"
    );
    let resolved_path = result.unwrap().into_path_buf();
    let resolved_name = resolved_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(
        resolved_name, "contract.d.ts",
        "a .js specifier should reach contract.d.ts when no runtime file exists"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn resolve_js_specifier_prefers_runtime_ts_over_dts() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    std::fs::write(root.join("contract.ts"), "export const x = 1;").unwrap();
    std::fs::write(
        root.join("contract.d.ts"),
        "export declare const x: number;",
    )
    .unwrap();
    std::fs::write(root.join("app.ts"), "import { x } from './contract.js';").unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let from_file = root.join("app.ts");
    let result = resolver.resolve_file(&from_file, "./contract.js");

    assert!(result.is_ok(), "should resolve ./contract.js successfully");
    let resolved_path = result.unwrap().into_path_buf();
    let resolved_name = resolved_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(
        resolved_name, "contract.ts",
        "runtime contract.ts must win over contract.d.ts for a .js specifier"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn resolve_honors_development_condition_by_default() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("pkg/src")).unwrap();
    std::fs::create_dir_all(root.join("pkg/dist")).unwrap();
    std::fs::write(root.join("pkg/src/index.ts"), "export const src = 1;").unwrap();
    std::fs::write(root.join("pkg/dist/index.js"), "export const dist = 1;").unwrap();
    std::fs::write(
        root.join("pkg/package.json"),
        r#"{
            "name": "pkg",
            "exports": {
                ".": {
                    "development": "./src/index.ts",
                    "import": "./dist/index.js"
                }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(root.join("app.ts"), "import { src } from 'pkg';").unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{"name": "app-root", "dependencies": {"pkg": "file:./pkg"}}"#,
    )
    .unwrap();
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("pkg"), root.join("node_modules/pkg")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(root.join("pkg"), root.join("node_modules/pkg")).unwrap();

    let resolver = specifier::create_resolver(root, &[], &[]);
    let from_file = root.join("app.ts");
    let resolved = resolver
        .resolve_file(&from_file, "pkg")
        .expect("pkg should resolve via exports");
    let resolved_path = resolved.into_path_buf();
    assert!(
        resolved_path.ends_with("pkg/src/index.ts")
            || resolved_path.ends_with("pkg\\src\\index.ts"),
        "expected development branch (src/index.ts), got {}",
        resolved_path.display()
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn resolve_honors_user_supplied_conditions_before_baseline() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("pkg/src")).unwrap();
    std::fs::write(
        root.join("pkg/src/index.worker.ts"),
        "export const worker = 1;",
    )
    .unwrap();
    std::fs::write(root.join("pkg/src/index.ts"), "export const src = 1;").unwrap();
    std::fs::write(
        root.join("pkg/package.json"),
        r#"{
            "name": "pkg",
            "exports": {
                ".": {
                    "worker": "./src/index.worker.ts",
                    "development": "./src/index.ts",
                    "import": "./src/index.ts"
                }
            }
        }"#,
    )
    .unwrap();
    std::fs::write(root.join("app.ts"), "import 'pkg';").unwrap();
    std::fs::write(root.join("package.json"), r#"{"name": "app-root"}"#).unwrap();
    std::fs::create_dir_all(root.join("node_modules")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("pkg"), root.join("node_modules/pkg")).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(root.join("pkg"), root.join("node_modules/pkg")).unwrap();

    let resolver = specifier::create_resolver(root, &[], &["worker".to_string()]);
    let from_file = root.join("app.ts");
    let resolved = resolver
        .resolve_file(&from_file, "pkg")
        .expect("pkg should resolve via exports");
    let resolved_path = resolved.into_path_buf();
    assert!(
        resolved_path.ends_with("index.worker.ts"),
        "expected user-supplied worker branch, got {}",
        resolved_path.display()
    );
}

/// Regression test for issue #838: a bare `"@"` plugin alias must not swallow
/// `@scope/pkg` imports. Before the fix, `specifier.starts_with("@")` matched
/// every scoped package and routed it into the alias branch, returning
/// `Unresolvable` instead of `NpmPackage`.
#[test]
#[cfg_attr(miri, ignore)] // oxc_resolver uses statx syscall unsupported by Miri
fn bare_at_alias_does_not_swallow_scoped_npm_packages() {
    let root = PathBuf::from("/project");
    let resolver = specifier::create_resolver(&root, &[], &[]);
    let style_resolver = specifier::create_resolver(&root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let path_to_id = FxHashMap::default();
    let raw_path_to_id = FxHashMap::default();
    let workspace_roots = FxHashMap::default();
    let package_manifests = Vec::new();
    let condition_names = react_native::build_condition_names(&[], &[]);
    // Register a bare "@" alias pointing to "./src", the problematic case.
    let aliases = vec![("@".to_string(), "src".to_string())];
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: false,
        condition_names: &condition_names,
        path_aliases: &aliases,
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root: &root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };

    let file = Path::new("/project/src/app.ts");

    // A scoped npm package must NOT enter the alias branch and must resolve as
    // NpmPackage, not Unresolvable.
    let result = specifier::resolve_specifier(&ctx, file, "@radix-ui/react-checkbox", false);
    assert!(
        matches!(result, ResolveResult::NpmPackage(ref pkg) if pkg == "@radix-ui/react-checkbox"),
        "bare '@' alias must not capture @scope/pkg; got {result:?}"
    );

    // "@/foo" DOES start with "@" and ends with a "/" continuation so it
    // should still enter the alias branch (and fail resolution since the file
    // does not exist, returning Unresolvable, not NpmPackage).
    let result_alias = specifier::resolve_specifier(&ctx, file, "@/foo", false);
    assert!(
        matches!(result_alias, ResolveResult::Unresolvable(_)),
        "'@/foo' should enter the alias branch and return Unresolvable; got {result_alias:?}"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
#[expect(
    clippy::too_many_lines,
    reason = "single resolver fixture keeps scoped import-map assertions together"
)]
fn deno_import_maps_follow_nearest_package_scope_and_declaring_base() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    let member = root.join("packages/member");
    std::fs::create_dir_all(member.join("src")).unwrap();
    std::fs::create_dir_all(member.join("member_shared")).unwrap();
    std::fs::create_dir_all(root.join("root_deep")).unwrap();

    std::fs::write(root.join("package.json"), r#"{"name":"root"}"#).unwrap();
    std::fs::write(
        root.join("deno.json"),
        r#"{
          "imports": {
            "exact": "./root_exact.ts",
            "shared/": "./root_shared/",
            "shared/deep/": "./root_deep/",
            "std": "jsr:@std/assert@1",
            "chalk": "npm:chalk@5"
          }
        }"#,
    )
    .unwrap();
    std::fs::write(member.join("package.json"), r#"{"name":"member"}"#).unwrap();
    std::fs::write(
        member.join("deno.json"),
        r#"{
          "imports": {
            "member": "./src/member.ts",
            "missing": "./wrong.ts",
            "shared/": "./member_shared/"
          }
        }"#,
    )
    .unwrap();

    let root_exact = root.join("root_exact.ts");
    let root_deep = root.join("root_deep/value.ts");
    let member_exact = member.join("src/member.ts");
    let member_override = member.join("member_shared/value.ts");
    let importer_relative_collision = member.join("src/wrong.ts");
    for path in [
        &root_exact,
        &root_deep,
        &member_exact,
        &member_override,
        &importer_relative_collision,
    ] {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "export const value = 1;").unwrap();
    }

    let workspaces = vec![fallow_config::WorkspaceInfo {
        root: member.clone(),
        name: "member".to_string(),
        is_internal_dependency: false,
    }];
    let input = ResolveAllImportsInput {
        modules: &[],
        files: &[],
        workspaces: &workspaces,
        active_plugins: &[],
        path_aliases: &[],
        auto_imports: &[],
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root,
        extra_conditions: &[],
    };
    let canonical_ws_roots = vec![dunce::canonicalize(&member).unwrap()];
    let package_manifests = super::build_package_manifests(&input, &canonical_ws_roots);

    let resolver = specifier::create_resolver(root, &[], &[]);
    let style_resolver = specifier::create_resolver(root, &[], &["style".to_string()]);
    let extensions = react_native::build_extensions(&[]);
    let mut raw_path_to_id = FxHashMap::default();
    raw_path_to_id.insert(root_exact.as_path(), FileId(1));
    raw_path_to_id.insert(root_deep.as_path(), FileId(2));
    raw_path_to_id.insert(member_exact.as_path(), FileId(3));
    raw_path_to_id.insert(member_override.as_path(), FileId(4));
    raw_path_to_id.insert(importer_relative_collision.as_path(), FileId(5));
    let canonical_targets = [
        &root_exact,
        &root_deep,
        &member_exact,
        &member_override,
        &importer_relative_collision,
    ]
    .map(|path| dunce::canonicalize(path).unwrap());
    let path_to_id: FxHashMap<&Path, FileId> = canonical_targets
        .iter()
        .enumerate()
        .map(|(index, path)| (path.as_path(), FileId(index as u32 + 1)))
        .collect();
    let workspace_roots = FxHashMap::default();
    let condition_names = react_native::build_condition_names(&[], &[]);
    let tsconfig_warned = std::sync::Mutex::new(FxHashSet::default());
    let tsconfig_cache = TsconfigCache::default();
    let canonicalize_cache = CanonicalizeCache::default();
    let ctx = ResolveContext {
        resolver: &resolver,
        style_resolver: &style_resolver,
        extensions: &extensions,
        path_to_id: &path_to_id,
        raw_path_to_id: &raw_path_to_id,
        workspace_roots: &workspace_roots,
        package_manifests: &package_manifests,
        has_deno_import_maps: package_manifests
            .iter()
            .any(|manifest| !manifest.deno_import_map.is_empty()),
        condition_names: &condition_names,
        path_aliases: &[],
        scss_include_paths: &[],
        static_dir_mappings: &[],
        root,
        canonical_fallback: None,
        tsconfig_warned: &tsconfig_warned,
        tsconfig_cache: &tsconfig_cache,
        canonicalize_cache: &canonicalize_cache,
    };

    let root_importer = root.join("app.ts");
    let member_importer = member.join("src/app.ts");
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &root_importer, "exact", false),
        ResolveResult::InternalModule(FileId(1))
    ));
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &member_importer, "exact", false),
        ResolveResult::InternalModule(FileId(1))
    ));
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &member_importer, "member", false),
        ResolveResult::InternalModule(FileId(3))
    ));
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &member_importer, "shared/value.ts", false),
        ResolveResult::InternalModule(FileId(4))
    ));
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &member_importer, "shared/deep/value.ts", false),
        ResolveResult::InternalModule(FileId(2))
    ));
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &member_importer, "missing", false),
        ResolveResult::Unresolvable(ref target) if target == "./wrong.ts"
    ));
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &member_importer, "std", false),
        ResolveResult::ExternalFile(ref path) if path == Path::new("jsr:@std/assert@1")
    ));
    assert!(matches!(
        specifier::resolve_specifier(&ctx, &member_importer, "chalk", false),
        ResolveResult::NpmPackage(ref name) if name == "chalk"
    ));
}
