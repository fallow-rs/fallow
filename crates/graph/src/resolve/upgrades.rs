//! Post-resolution specifier upgrade pass.
//!
//! Fixes non-deterministic resolution of bare specifiers that arises from per-file
//! tsconfig path alias discovery (`TsconfigDiscovery::Auto`). The same specifier
//! (e.g., `preact/hooks`) may resolve to `InternalModule` in files under a tsconfig
//! with matching path aliases, but to `NpmPackage` in files without such aliases.
//!
//! This pass scans all resolved imports and re-exports to find bare specifiers where
//! at least one file resolved to `InternalModule`, then upgrades all `NpmPackage`
//! results for that specifier to `InternalModule`. This is correct because if any
//! tsconfig maps a specifier to a project source file, that file is the canonical
//! origin.
//!
//! Run once after all parallel resolution completes, as the final step in
//! [`super::resolve_all_imports`].

use rustc_hash::FxHashMap;

use super::ResolvedModule;
use super::path_info::is_bare_specifier;
use super::types::ResolveResult;

/// Post-resolution pass: deterministic specifier upgrade.
///
/// With `TsconfigDiscovery::Auto`, the same bare specifier (e.g., `preact/hooks`)
/// may resolve to `InternalModule` from files under a tsconfig with path aliases
/// but `NpmPackage` from files without such aliases. The parallel resolution cache
/// makes the per-file result depend on which thread resolved first (non-deterministic).
///
/// Scans all resolved imports/re-exports to find bare specifiers where ANY file resolved
/// to `InternalModule`. For those specifiers, upgrades all `NpmPackage` results to
/// `InternalModule`. This is correct because if any tsconfig context maps a specifier to
/// a project source file, that source file IS the origin of the package.
///
/// Note: if two tsconfigs map the same specifier to different `FileId`s, the first one
/// encountered (by module order = `FileId` order) wins. This is deterministic but may be
/// imprecise for that edge case , both files get connected regardless.
pub(super) fn apply_specifier_upgrades(resolved: &mut [ResolvedModule]) {
    let mut specifier_upgrades: FxHashMap<String, ResolveResult> = FxHashMap::default();
    for module in resolved.iter() {
        for imp in module
            .resolved_imports
            .iter()
            .chain(module.resolved_dynamic_imports.iter())
        {
            if is_bare_specifier(&imp.info.source) && imp.target.internal_file_id().is_some() {
                specifier_upgrades
                    .entry(imp.info.source.clone())
                    .or_insert_with(|| imp.target.clone());
            }
        }
        for re in &module.re_exports {
            if is_bare_specifier(&re.info.source) && re.target.internal_file_id().is_some() {
                specifier_upgrades
                    .entry(re.info.source.clone())
                    .or_insert_with(|| re.target.clone());
            }
        }
    }

    if specifier_upgrades.is_empty() {
        return;
    }

    for module in resolved.iter_mut() {
        for imp in module
            .resolved_imports
            .iter_mut()
            .chain(module.resolved_dynamic_imports.iter_mut())
        {
            if matches!(imp.target, ResolveResult::NpmPackage(_))
                && let Some(target) = specifier_upgrades.get(&imp.info.source)
            {
                imp.target = target.clone();
            }
        }
        for re in &mut module.re_exports {
            if matches!(re.target, ResolveResult::NpmPackage(_))
                && let Some(target) = specifier_upgrades.get(&re.info.source)
            {
                re.target = target.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use rustc_hash::FxHashSet;

    use fallow_types::discover::FileId;
    use fallow_types::extract::{ImportInfo, ImportedName, ReExportInfo};
    use oxc_span::Span;

    use super::super::types::{ResolvedImport, ResolvedReExport};
    use super::*;

    /// Build a minimal `ResolvedModule` with no imports or re-exports.
    fn empty_module(file_id: FileId) -> ResolvedModule {
        ResolvedModule {
            file_id,
            path: PathBuf::from(format!("/project/src/file_{}.ts", file_id.0)),
            exports: vec![],
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
        }
    }

    /// Build a `ResolvedImport` with the given specifier and target.
    fn make_import(source: &str, target: ResolveResult) -> ResolvedImport {
        ResolvedImport {
            info: ImportInfo {
                source: source.to_string(),
                imported_name: ImportedName::Default,
                local_name: "x".to_string(),
                is_type_only: false,
                from_style: false,
                span: Span::new(0, 0),
                source_span: Span::new(0, 0),
            },
            target,
        }
    }

    /// Build a `ResolvedReExport` with the given specifier and target.
    fn make_re_export(source: &str, target: ResolveResult) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: source.to_string(),
                imported_name: "*".to_string(),
                exported_name: "*".to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
            },
            target,
        }
    }

    #[test]
    fn empty_modules_no_crash() {
        let mut resolved: Vec<ResolvedModule> = vec![];
        apply_specifier_upgrades(&mut resolved);
        assert!(resolved.is_empty());
    }

    #[test]
    fn all_internal_no_changes() {
        let mut m = empty_module(FileId(0));
        m.resolved_imports = vec![
            make_import("preact/hooks", ResolveResult::InternalModule(FileId(1))),
            make_import("preact", ResolveResult::InternalModule(FileId(2))),
        ];
        let mut resolved = vec![m];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[0].resolved_imports[0].target,
            ResolveResult::InternalModule(FileId(1))
        ));
        assert!(matches!(
            resolved[0].resolved_imports[1].target,
            ResolveResult::InternalModule(FileId(2))
        ));
    }

    #[test]
    fn single_import_upgraded_from_npm_to_internal() {
        let mut m0 = empty_module(FileId(0));
        m0.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(10)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::NpmPackage("preact".to_string()),
        )];

        let mut resolved = vec![m0, m1];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[0].resolved_imports[0].target,
            ResolveResult::InternalModule(FileId(10))
        ));
        assert!(matches!(
            resolved[1].resolved_imports[0].target,
            ResolveResult::InternalModule(FileId(10))
        ));
    }

    #[test]
    fn re_export_specifier_upgraded() {
        let mut m0 = empty_module(FileId(0));
        m0.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(10)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.re_exports = vec![make_re_export(
            "preact/hooks",
            ResolveResult::NpmPackage("preact".to_string()),
        )];

        let mut resolved = vec![m0, m1];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[1].re_exports[0].target,
            ResolveResult::InternalModule(FileId(10))
        ));
    }

    #[test]
    fn multiple_imports_mixed_only_npm_upgraded() {
        let mut m0 = empty_module(FileId(0));
        m0.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(10)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.resolved_imports = vec![
            make_import("preact/hooks", ResolveResult::InternalModule(FileId(10))),
            make_import(
                "preact/hooks",
                ResolveResult::NpmPackage("preact".to_string()),
            ),
        ];

        let mut resolved = vec![m0, m1];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[1].resolved_imports[0].target,
            ResolveResult::InternalModule(FileId(10))
        ));
        assert!(matches!(
            resolved[1].resolved_imports[1].target,
            ResolveResult::InternalModule(FileId(10))
        ));
    }

    #[test]
    fn upgrade_map_empty_no_changes() {
        let mut m = empty_module(FileId(0));
        m.resolved_imports = vec![
            make_import("lodash", ResolveResult::NpmPackage("lodash".to_string())),
            make_import("react", ResolveResult::NpmPackage("react".to_string())),
        ];
        let mut resolved = vec![m];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[0].resolved_imports[0].target,
            ResolveResult::NpmPackage(_)
        ));
        assert!(matches!(
            resolved[0].resolved_imports[1].target,
            ResolveResult::NpmPackage(_)
        ));
    }

    #[test]
    fn specifier_not_in_upgrade_map_unchanged() {
        let mut m0 = empty_module(FileId(0));
        m0.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(10)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.resolved_imports = vec![make_import(
            "lodash",
            ResolveResult::NpmPackage("lodash".to_string()),
        )];

        let mut resolved = vec![m0, m1];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[1].resolved_imports[0].target,
            ResolveResult::NpmPackage(_)
        ));
    }

    #[test]
    fn dynamic_imports_also_upgraded() {
        let mut m0 = empty_module(FileId(0));
        m0.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(10)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.resolved_dynamic_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::NpmPackage("preact".to_string()),
        )];

        let mut resolved = vec![m0, m1];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[1].resolved_dynamic_imports[0].target,
            ResolveResult::InternalModule(FileId(10))
        ));
    }

    #[test]
    fn relative_specifier_not_treated_as_bare() {
        let mut m0 = empty_module(FileId(0));
        m0.resolved_imports = vec![make_import(
            "./utils",
            ResolveResult::InternalModule(FileId(5)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.resolved_imports = vec![make_import(
            "./utils",
            ResolveResult::NpmPackage("utils".to_string()),
        )];

        let mut resolved = vec![m0, m1];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[1].resolved_imports[0].target,
            ResolveResult::NpmPackage(_)
        ));
    }

    #[test]
    fn first_internal_file_id_wins() {
        let mut m0 = empty_module(FileId(0));
        m0.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(10)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(20)),
        )];

        let mut m2 = empty_module(FileId(2));
        m2.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::NpmPackage("preact".to_string()),
        )];

        let mut resolved = vec![m0, m1, m2];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[2].resolved_imports[0].target,
            ResolveResult::InternalModule(FileId(10))
        ));
    }

    #[test]
    fn re_export_internal_creates_upgrade_entry() {
        let mut m0 = empty_module(FileId(0));
        m0.re_exports = vec![make_re_export(
            "preact/hooks",
            ResolveResult::InternalModule(FileId(10)),
        )];

        let mut m1 = empty_module(FileId(1));
        m1.resolved_imports = vec![make_import(
            "preact/hooks",
            ResolveResult::NpmPackage("preact".to_string()),
        )];

        let mut resolved = vec![m0, m1];
        apply_specifier_upgrades(&mut resolved);

        assert!(matches!(
            resolved[1].resolved_imports[0].target,
            ResolveResult::InternalModule(FileId(10))
        ));
    }
}
