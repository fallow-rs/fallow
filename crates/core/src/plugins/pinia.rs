//! Pinia Nuxt store auto-import plugin.

use std::path::{Path, PathBuf};

use fallow_config::{AutoImportKind, AutoImportRule};
use fallow_types::discover::FileId;
use fallow_types::extract::ExportName;

use super::Plugin;

const ENABLERS: &[&str] = &["@pinia/nuxt"];
const STORE_DIRS: &[&str] = &["stores", "app/stores"];
const STORE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"];

pub struct PiniaPlugin;

impl Plugin for PiniaPlugin {
    fn name(&self) -> &'static str {
        "pinia"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn auto_imports(&self, root: &Path) -> Vec<AutoImportRule> {
        let mut rules = Vec::new();
        for dir in STORE_DIRS {
            let base = root.join(dir);
            if base.is_dir() {
                collect_store_auto_imports(&base, &mut rules);
            }
        }
        rules
    }
}

fn collect_store_auto_imports(dir: &Path, rules: &mut Vec<AutoImportRule>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() || !has_store_extension(&path) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let module = fallow_extract::parse_source_to_module(FileId(0), &path, &source, 0, false);
        for export in module.exports {
            if export.is_type_only {
                continue;
            }
            if let ExportName::Named(name) = export.name
                && is_store_export_name(&name)
            {
                push_store_rule(rules, name, path.clone());
            }
        }
    }
}

fn has_store_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            STORE_EXTENSIONS
                .iter()
                .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

fn is_store_export_name(name: &str) -> bool {
    name.starts_with("use") && name.ends_with("Store") && name.len() > "useStore".len()
}

fn push_store_rule(rules: &mut Vec<AutoImportRule>, name: String, source: PathBuf) {
    if rules.iter().any(|rule| {
        rule.name == name && rule.source == source && rule.kind == AutoImportKind::Named
    }) {
        return;
    }
    rules.push(AutoImportRule {
        name,
        source,
        kind: AutoImportKind::Named,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(root: &Path, relative: &str, source: &str) {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dir");
        }
        std::fs::write(path, source).expect("write fixture file");
    }

    fn has_rule(rules: &[AutoImportRule], name: &str, relative: &str, root: &Path) -> bool {
        let source = root.join(relative);
        rules.iter().any(|rule| {
            rule.name == name && rule.source == source && rule.kind == AutoImportKind::Named
        })
    }

    #[test]
    fn enabler_is_pinia_nuxt() {
        let plugin = PiniaPlugin;
        assert_eq!(plugin.enablers(), &["@pinia/nuxt"]);
    }

    #[test]
    fn is_enabled_with_pinia_nuxt_dep() {
        let plugin = PiniaPlugin;
        let deps = vec!["@pinia/nuxt".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_not_enabled_without_pinia_nuxt_dep() {
        let plugin = PiniaPlugin;
        let deps = vec!["pinia".to_string(), "nuxt".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn auto_imports_emit_named_store_exports() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        write_file(
            root,
            "stores/user.ts",
            r#"
                export const useUserStore = defineStore("user", () => ({}));
                export const unusedStoreHelper = () => null;
            "#,
        );

        let rules = PiniaPlugin.auto_imports(root);

        assert!(has_rule(&rules, "useUserStore", "stores/user.ts", root));
        assert!(
            !has_rule(&rules, "unusedStoreHelper", "stores/user.ts", root),
            "non-store exports must not become Pinia auto-import providers"
        );
    }

    #[test]
    fn auto_imports_scan_app_stores() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        write_file(
            root,
            "app/stores/settings.ts",
            r#"
                export const useSettingsStore = defineStore("settings", () => ({}));
            "#,
        );

        let rules = PiniaPlugin.auto_imports(root);

        assert!(has_rule(
            &rules,
            "useSettingsStore",
            "app/stores/settings.ts",
            root
        ));
    }

    #[test]
    fn auto_imports_ignore_type_only_and_non_store_exports() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        write_file(
            root,
            "stores/user.ts",
            r"
                export type useTypeStore = { id: string };
                export interface useInterfaceStore { id: string }
                export const useUser = () => null;
                export const useStore = () => null;
            ",
        );

        let rules = PiniaPlugin.auto_imports(root);

        assert!(rules.is_empty(), "no export should match use<Name>Store");
    }

    #[test]
    fn auto_imports_do_not_recurse_into_nested_stores() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path();
        write_file(
            root,
            "stores/admin/user.ts",
            r#"
                export const useAdminStore = defineStore("admin", () => ({}));
            "#,
        );

        let rules = PiniaPlugin.auto_imports(root);

        assert!(
            !has_rule(&rules, "useAdminStore", "stores/admin/user.ts", root),
            "default Pinia storesDirs only scan direct store files"
        );
    }

    #[test]
    fn auto_imports_empty_without_store_dirs() {
        let tmp = tempfile::tempdir().expect("temp dir");

        let rules = PiniaPlugin.auto_imports(tmp.path());

        assert!(rules.is_empty());
    }
}
