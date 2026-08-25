//! React Native and Expo platform extension support.

use std::path::Path;

use rustc_hash::FxHashMap;

use super::types::{RN_PLATFORM_PREFIXES, ResolveResult, ResolvedImport, ResolvedModule};
use fallow_types::discover::{DiscoveredFile, FileId};
use fallow_types::extract::{ImportInfo, ImportedName};

/// Whether the React Native or Expo plugin is active, the gate for every
/// Metro platform-extension behavior in the resolver and its consumers.
pub fn has_react_native_plugin(active_plugins: &[String]) -> bool {
    active_plugins
        .iter()
        .any(|p| p == "react-native" || p == "expo")
}

/// Source extensions that participate in Metro platform-extension resolution.
const RN_SOURCE_EXTS: &[&str] = &[".ts", ".tsx", ".js", ".jsx"];

/// Split a file or specifier basename into its stem and a Metro source
/// extension, when one is present.
fn split_source_ext(name: &str) -> (&str, Option<&str>) {
    for ext in RN_SOURCE_EXTS {
        if let Some(stem) = name.strip_suffix(ext) {
            return (stem, Some(ext));
        }
    }
    (name, None)
}

/// Strip a trailing platform segment (`.ios`, `.android`, ...) from a stem.
/// Returns the family base stem and whether a platform segment was present.
fn strip_platform_segment(stem: &str) -> (&str, bool) {
    for platform in RN_PLATFORM_PREFIXES {
        if let Some(base) = stem.strip_suffix(platform) {
            return (base, true);
        }
    }
    (stem, false)
}

/// Where a source file sits in a Metro platform-extension family: the
/// directory and base stem shared by `<stem>.<platform><ext>` and
/// `<stem><ext>`, plus whether this member carries a platform segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlatformFamilyKey<'a> {
    /// Directory containing the file.
    pub parent: &'a Path,
    /// File stem with the source extension and any platform segment removed.
    pub base: &'a str,
    /// Whether the file name carries a platform segment (`.ios`, `.web`, ...).
    pub is_platform_variant: bool,
}

/// Classify `path` by its Metro platform-extension family.
///
/// Membership is syntactic: files sharing `parent` and `base` belong to the
/// same family, and the caller decides whether enough members exist for the
/// family to matter. Returns `None` for files outside the Metro source
/// extensions, names without a stem, and paths without a parent directory.
pub fn platform_family_key(path: &Path) -> Option<PlatformFamilyKey<'_>> {
    let name = path.file_name()?.to_str()?;
    let (stem, ext) = split_source_ext(name);
    ext?;
    let (base, is_platform_variant) = strip_platform_segment(stem);
    if base.is_empty() {
        return None;
    }
    let parent = path.parent()?;
    Some(PlatformFamilyKey {
        parent,
        base,
        is_platform_variant,
    })
}

/// Whether an import specifier explicitly names a platform variant
/// (e.g. `./UserMenu.ios` or `./UserMenu.ios.tsx`), in which case the author
/// targeted one variant and the family must not be credited as a whole.
fn specifier_names_platform_variant(specifier: &str) -> bool {
    let basename = specifier.rsplit('/').next().unwrap_or(specifier);
    let (stem, _) = split_source_ext(basename);
    strip_platform_segment(stem).1
}

/// Metro platform-extension families among the discovered files, keyed by the
/// member [`FileId`]. A family is every file in one directory sharing a base
/// stem across `<stem>.<platform><ext>` and `<stem><ext>`, and only counts
/// when at least one platform variant exists alongside another member.
struct PlatformFamilies {
    family_of: FxHashMap<FileId, usize>,
    members: Vec<Vec<FileId>>,
}

impl PlatformFamilies {
    fn build(files: &[DiscoveredFile]) -> Self {
        let mut grouped: FxHashMap<(&Path, &str), Vec<(FileId, bool)>> = FxHashMap::default();
        for file in files {
            let Some(key) = platform_family_key(&file.path) else {
                continue;
            };
            grouped
                .entry((key.parent, key.base))
                .or_default()
                .push((file.id, key.is_platform_variant));
        }

        let mut family_of = FxHashMap::default();
        let mut members = Vec::new();
        for group in grouped.into_values() {
            if group.len() < 2 || !group.iter().any(|(_, is_platform)| *is_platform) {
                continue;
            }
            let mut ids: Vec<FileId> = group.into_iter().map(|(id, _)| id).collect();
            ids.sort_unstable_by_key(|id| id.0);
            let index = members.len();
            for id in &ids {
                family_of.insert(*id, index);
            }
            members.push(ids);
        }
        Self { family_of, members }
    }

    fn siblings(&self, target: FileId) -> Option<&[FileId]> {
        self.family_of
            .get(&target)
            .map(|index| self.members[*index].as_slice())
    }
}

/// Rebuild a project-internal [`ResolveResult`] against a sibling file,
/// preserving the original edge kind and package attribution.
fn retarget(result: &ResolveResult, sibling: FileId) -> Option<ResolveResult> {
    match result {
        ResolveResult::InternalModule(_) => Some(ResolveResult::InternalModule(sibling)),
        ResolveResult::CommonJsInternalModule(_) => {
            Some(ResolveResult::CommonJsInternalModule(sibling))
        }
        ResolveResult::SyntheticAutoImport(_) => Some(ResolveResult::SyntheticAutoImport(sibling)),
        ResolveResult::InternalPackageModule { package_name, .. } => {
            Some(ResolveResult::InternalPackageModule {
                file_id: sibling,
                package_name: package_name.clone(),
            })
        }
        ResolveResult::CommonJsInternalPackageModule { package_name, .. } => {
            Some(ResolveResult::CommonJsInternalPackageModule {
                file_id: sibling,
                package_name: package_name.clone(),
            })
        }
        ResolveResult::ExternalFile(_)
        | ResolveResult::NpmPackage(_)
        | ResolveResult::CommonJsNpmPackage(_)
        | ResolveResult::Unresolvable(_) => None,
    }
}

/// Expand `imports` with sibling edges for every platform-extension family
/// member, appending the extra edges to `extra`.
fn expand_family_imports(
    imports: &[ResolvedImport],
    families: &PlatformFamilies,
    extra: &mut Vec<ResolvedImport>,
) {
    for import in imports {
        if specifier_names_platform_variant(&import.info.source) {
            continue;
        }
        let Some(target) = import.target.internal_file_id() else {
            continue;
        };
        let Some(siblings) = families.siblings(target) else {
            continue;
        };
        for sibling in siblings {
            if *sibling == target {
                continue;
            }
            if let Some(retargeted) = retarget(&import.target, *sibling) {
                extra.push(ResolvedImport {
                    info: import.info.clone(),
                    target: retargeted,
                });
            }
        }
    }
}

/// Credit whole Metro platform-extension families when the RN/Expo plugin is
/// active.
///
/// Metro resolves `./UserMenu` to `UserMenu.ios.tsx` on iOS and to
/// `UserMenu.tsx` (or `.android.tsx`, `.native.tsx`, ...) elsewhere, so a
/// specifier that resolved to one family member reaches every member at
/// runtime. The resolver picks a single winner per platform-extension order;
/// this pass appends edges to the remaining family members so none are
/// reported as unused files and their matching exports stay credited. Imports
/// that explicitly name a platform variant keep their single edge.
pub(super) fn synthesize_platform_family_edges(
    resolved: &mut [ResolvedModule],
    files: &[DiscoveredFile],
    active_plugins: &[String],
) {
    if !has_react_native_plugin(active_plugins) {
        return;
    }
    let families = PlatformFamilies::build(files);
    if families.members.is_empty() {
        return;
    }

    for module in resolved.iter_mut() {
        let mut extra = Vec::new();
        expand_family_imports(&module.resolved_imports, &families, &mut extra);
        expand_family_imports(&module.resolved_dynamic_imports, &families, &mut extra);

        for re_export in &module.re_exports {
            if specifier_names_platform_variant(&re_export.info.source) {
                continue;
            }
            let Some(target) = re_export.target.internal_file_id() else {
                continue;
            };
            let Some(siblings) = families.siblings(target) else {
                continue;
            };
            for sibling in siblings {
                if *sibling == target {
                    continue;
                }
                // Re-export propagation keeps its single resolved source; a
                // side-effect edge is enough to keep the sibling reachable.
                extra.push(ResolvedImport {
                    info: ImportInfo {
                        source: re_export.info.source.clone(),
                        imported_name: ImportedName::SideEffect,
                        local_name: String::new(),
                        is_type_only: re_export.info.is_type_only,
                        is_type_only_star: false,
                        from_style: false,
                        span: oxc_span::Span::default(),
                        source_span: oxc_span::Span::default(),
                    },
                    target: ResolveResult::InternalModule(*sibling),
                });
            }
        }

        module.resolved_imports.extend(extra);
    }
}

/// Build the resolver extension list, optionally prepending React Native platform
/// extensions when the RN/Expo plugin is active.
pub(super) fn build_extensions(active_plugins: &[String]) -> Vec<String> {
    let base: Vec<String> = vec![
        ".ts".into(),
        ".tsx".into(),
        ".mts".into(),
        ".cts".into(),
        ".gts".into(),
        ".js".into(),
        ".jsx".into(),
        ".mjs".into(),
        ".cjs".into(),
        ".gjs".into(),
        ".d.ts".into(),
        ".d.mts".into(),
        ".d.cts".into(),
        ".json".into(),
        ".vue".into(),
        ".svelte".into(),
        ".astro".into(),
        ".mdx".into(),
        ".css".into(),
        ".scss".into(),
        ".graphql".into(),
        ".gql".into(),
    ];

    if has_react_native_plugin(active_plugins) {
        let source_exts = [".ts", ".tsx", ".js", ".jsx"];
        let mut rn_extensions: Vec<String> = Vec::new();
        for platform in RN_PLATFORM_PREFIXES {
            for ext in &source_exts {
                rn_extensions.push(format!("{platform}{ext}"));
            }
        }
        rn_extensions.extend(base);
        rn_extensions
    } else {
        base
    }
}

/// Build the resolver `condition_names` list.
///
/// Baseline conditions (in priority order): `development`, `import`, `require`,
/// `default`, `types`, `node`. `development` is included so that package.json
/// `exports` / `imports` entries declaring a `development` branch (a widely
/// used community condition, supported by Vite, Vitest, esbuild, and Rollup)
/// resolve to their source files instead of compiled `dist/` output. See
/// <https://nodejs.org/api/packages.html#community-conditions-definitions>.
///
/// When the React Native or Expo plugin is active, `react-native` and
/// `browser` are prepended ahead of the baseline for Metro-style resolution.
/// User-supplied `extra_conditions` are prepended ahead of everything else
/// so they take highest priority.
pub(super) fn build_condition_names(
    active_plugins: &[String],
    extra_conditions: &[String],
) -> Vec<String> {
    let mut names = vec![
        "development".into(),
        "import".into(),
        "require".into(),
        "default".into(),
        "types".into(),
        "node".into(),
    ];
    if has_react_native_plugin(active_plugins) {
        names.insert(0, "react-native".into());
        names.insert(1, "browser".into());
    }
    for extra in extra_conditions.iter().rev() {
        names.insert(0, extra.clone());
    }
    let mut seen: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
    names.retain(|name| seen.insert(name.clone()));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_react_native_plugin_active() {
        let plugins = vec!["react-native".to_string(), "typescript".to_string()];
        assert!(has_react_native_plugin(&plugins));
    }

    #[test]
    fn test_has_expo_plugin_active() {
        let plugins = vec!["expo".to_string(), "typescript".to_string()];
        assert!(has_react_native_plugin(&plugins));
    }

    #[test]
    fn test_has_react_native_plugin_inactive() {
        let plugins = vec!["nextjs".to_string(), "typescript".to_string()];
        assert!(!has_react_native_plugin(&plugins));
    }

    #[test]
    fn test_rn_platform_extensions_prepended() {
        let no_rn = build_extensions(&[]);
        let rn_plugins = vec!["react-native".to_string()];
        let with_rn = build_extensions(&rn_plugins);

        assert_eq!(no_rn[0], ".ts");

        assert_eq!(with_rn[0], ".web.ts");
        assert_eq!(with_rn[1], ".web.tsx");
        assert_eq!(with_rn[2], ".web.js");
        assert_eq!(with_rn[3], ".web.jsx");

        assert!(with_rn.len() > no_rn.len());
        assert_eq!(
            with_rn.len(),
            no_rn.len() + 16,
            "should add 16 platform extensions (4 platforms x 4 exts)"
        );
    }

    #[test]
    fn test_rn_condition_names_prepended() {
        let no_rn = build_condition_names(&[], &[]);
        let rn_plugins = vec!["react-native".to_string()];
        let with_rn = build_condition_names(&rn_plugins, &[]);

        assert_eq!(no_rn[0], "development");

        assert_eq!(with_rn[0], "react-native");
        assert_eq!(with_rn[1], "browser");
        assert_eq!(with_rn[2], "development");
    }

    #[test]
    fn test_development_condition_in_baseline() {
        let names = build_condition_names(&[], &[]);
        assert!(
            names.contains(&"development".to_string()),
            "`development` must be part of the default condition set"
        );
    }

    #[test]
    fn test_extra_conditions_prepended_before_baseline() {
        let names = build_condition_names(&[], &["worker".to_string(), "edge-light".to_string()]);
        assert_eq!(names[0], "worker");
        assert_eq!(names[1], "edge-light");
        assert_eq!(names[2], "development");
    }

    #[test]
    fn test_extra_conditions_prepended_before_rn() {
        let rn_plugins = vec!["react-native".to_string()];
        let names = build_condition_names(&rn_plugins, &["worker".to_string()]);
        assert_eq!(names[0], "worker");
        assert_eq!(names[1], "react-native");
        assert_eq!(names[2], "browser");
        assert_eq!(names[3], "development");
    }

    #[test]
    fn test_duplicate_baseline_condition_from_user_is_deduped() {
        let names = build_condition_names(&[], &["development".to_string()]);
        let dev_count = names.iter().filter(|n| *n == "development").count();
        assert_eq!(dev_count, 1, "`development` should appear exactly once");
        assert_eq!(
            names[0], "development",
            "user-supplied entry keeps its position"
        );
    }

    #[test]
    fn test_specifier_names_platform_variant() {
        assert!(specifier_names_platform_variant("./UserMenu.ios"));
        assert!(specifier_names_platform_variant("./UserMenu.android.tsx"));
        assert!(specifier_names_platform_variant("../deep/UserMenu.native"));
        assert!(!specifier_names_platform_variant("./UserMenu"));
        assert!(!specifier_names_platform_variant("./UserMenu.tsx"));
        assert!(!specifier_names_platform_variant("./ios/UserMenu"));
    }

    #[test]
    fn test_platform_family_key_marks_platform_variants() {
        let key = platform_family_key(Path::new("src/components/UserMenu.ios.tsx"))
            .expect("source file has a family key");
        assert_eq!(key.parent, Path::new("src/components"));
        assert_eq!(key.base, "UserMenu");
        assert!(key.is_platform_variant);

        let key = platform_family_key(Path::new("src/components/UserMenu.tsx"))
            .expect("source file has a family key");
        assert_eq!(key.parent, Path::new("src/components"));
        assert_eq!(key.base, "UserMenu");
        assert!(!key.is_platform_variant);
    }

    #[test]
    fn test_platform_family_key_covers_every_platform_and_source_extension() {
        for platform in RN_PLATFORM_PREFIXES {
            for ext in RN_SOURCE_EXTS {
                let path = format!("src/Button{platform}{ext}");
                let key = platform_family_key(Path::new(&path)).expect("family key");
                assert_eq!(key.base, "Button", "{path}");
                assert!(key.is_platform_variant, "{path}");
            }
        }
    }

    #[test]
    fn test_platform_family_key_rejects_non_source_files() {
        assert_eq!(platform_family_key(Path::new("src/UserMenu.css")), None);
        assert_eq!(
            platform_family_key(Path::new("src/UserMenu.ios.json")),
            None
        );
        assert_eq!(platform_family_key(Path::new("src/UserMenu.ios")), None);
        assert_eq!(
            platform_family_key(Path::new("src/.ios.tsx")),
            None,
            "a bare platform segment has no base stem"
        );
    }

    fn discovered(id: u32, path: &str) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: std::path::PathBuf::from(path),
            size_bytes: 0,
        }
    }

    #[test]
    fn test_platform_families_group_base_and_variants() {
        let files = vec![
            discovered(0, "src/UserMenu.tsx"),
            discovered(1, "src/UserMenu.ios.tsx"),
            discovered(2, "src/UserMenu.android.tsx"),
            discovered(3, "src/Other.tsx"),
            discovered(4, "src/nested/UserMenu.tsx"),
        ];
        let families = PlatformFamilies::build(&files);

        assert_eq!(families.members.len(), 1);
        assert_eq!(
            families.siblings(FileId(0)),
            Some([FileId(0), FileId(1), FileId(2)].as_slice())
        );
        assert_eq!(families.siblings(FileId(1)), families.siblings(FileId(0)));
        assert_eq!(families.siblings(FileId(3)), None);
        assert_eq!(
            families.siblings(FileId(4)),
            None,
            "same stem in a different directory is not part of the family"
        );
    }

    #[test]
    fn test_platform_families_require_a_platform_variant() {
        let files = vec![
            discovered(0, "src/Button.ts"),
            discovered(1, "src/Button.tsx"),
        ];
        let families = PlatformFamilies::build(&files);
        assert!(
            families.members.is_empty(),
            "same-stem files without a platform variant are not a Metro family"
        );
    }

    #[test]
    fn test_duplicate_user_conditions_are_deduped_preserving_first() {
        let names = build_condition_names(
            &[],
            &[
                "worker".to_string(),
                "edge-light".to_string(),
                "worker".to_string(),
            ],
        );
        let worker_count = names.iter().filter(|n| *n == "worker").count();
        assert_eq!(worker_count, 1);
        assert_eq!(names[0], "worker");
        assert_eq!(names[1], "edge-light");
    }
}
