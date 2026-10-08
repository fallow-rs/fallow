#![expect(clippy::unwrap_used, reason = "tests keep fixture setup concise")]

use std::path::{Path, PathBuf};

use fallow_graph::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
use fallow_types::trace_usage::{SitePageRequest, UsageSite};

use super::*;

const ROOT: &str = "/project";
const PACKAGE: &str = "pkg";

struct Project {
    graph: ModuleGraph,
    modules: Vec<ModuleInfo>,
    paths: Vec<String>,
}

impl Project {
    /// Parse the files and resolve `./name` imports to the file `src/name.*`
    /// and every bare specifier to an npm package. The first file is the
    /// entry point.
    fn new(files: &[(&str, &str)]) -> Self {
        let root = Path::new(ROOT);
        let paths: Vec<String> = files.iter().map(|(path, _)| (*path).to_owned()).collect();
        let modules: Vec<ModuleInfo> = files
            .iter()
            .enumerate()
            .map(|(index, (path, source))| {
                fallow_extract::parse_from_content(
                    FileId(u32::try_from(index).unwrap()),
                    &root.join(path),
                    source,
                )
            })
            .collect();
        let resolve = |source: &str| -> ResolveResult {
            if let Some(name) = source.strip_prefix("./") {
                let found = paths.iter().position(|path| {
                    Path::new(path)
                        .file_stem()
                        .is_some_and(|stem| stem.to_string_lossy() == name)
                });
                return found.map_or_else(
                    || ResolveResult::Unresolvable(source.to_owned()),
                    |index| ResolveResult::InternalModule(FileId(u32::try_from(index).unwrap())),
                );
            }
            ResolveResult::NpmPackage(fallow_graph::resolve::extract_package_name(source))
        };
        let resolved: Vec<ResolvedModule> = modules
            .iter()
            .map(|module| ResolvedModule {
                file_id: module.file_id,
                path: root.join(&paths[module.file_id.0 as usize]),
                exports: std::sync::Arc::clone(&module.exports),
                resolved_imports: module
                    .imports
                    .iter()
                    .map(|info| ResolvedImport {
                        info: info.clone(),
                        target: resolve(&info.source),
                    })
                    .collect(),
                ..Default::default()
            })
            .collect();
        let discovered: Vec<DiscoveredFile> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| DiscoveredFile {
                id: FileId(u32::try_from(index).unwrap()),
                path: root.join(path),
                size_bytes: 100,
            })
            .collect();
        let entry_points = vec![EntryPoint {
            path: root.join(&paths[0]),
            source: EntryPointSource::PackageJsonMain,
        }];
        let graph = ModuleGraph::build(&resolved, &entry_points, &discovered);
        Self {
            graph,
            modules,
            paths,
        }
    }

    /// The files that import the package.
    fn importers(&self) -> Vec<PathBuf> {
        self.modules
            .iter()
            .filter(|module| module.imports.iter().any(|import| import.source == PACKAGE))
            .map(|module| PathBuf::from(&self.paths[module.file_id.0 as usize]))
            .collect()
    }

    fn usage(&self, query: &DependencyUsageQuery) -> Result<DependencyUsage, UsageError> {
        dependency_usage(
            &self.graph,
            &self.modules,
            Path::new(ROOT),
            PACKAGE,
            &self.importers(),
            query,
        )
    }

    fn sites(&self, specifiers: &[&str]) -> Vec<UsageSite> {
        let query = DependencyUsageQuery::new(
            specifiers.iter().map(|name| (*name).to_owned()).collect(),
            Some(SitePageRequest {
                limit: 500,
                cursor: None,
            }),
            None,
        )
        .unwrap();
        self.usage(&query).unwrap().sites.unwrap().items
    }
}

fn counts_only() -> DependencyUsageQuery {
    DependencyUsageQuery::default()
}

fn specifier<'a>(usage: &'a DependencyUsage, name: &str) -> &'a SpecifierUsage {
    usage
        .specifiers
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("{name} is in {:?}", usage.specifiers))
}

/// (file, specifier, local name, kind, via) of each site.
fn rows(sites: &[UsageSite]) -> Vec<(String, Option<String>, Option<String>, UsageSiteKind)> {
    sites
        .iter()
        .map(|site| {
            (
                site.file.clone(),
                site.specifier.clone(),
                site.local_name.clone(),
                site.kind,
            )
        })
        .collect()
}

#[test]
fn name_rules_cover_named_default_and_namespace_forms() {
    let project = Project::new(&[(
        "src/main.ts",
        "import { useSelector as pick } from 'pkg';\n\
         import Pkg from 'pkg';\n\
         import * as RR from 'pkg';\n\
         pick(1);\n\
         Pkg();\n\
         RR.useStore();\n\
         consume(RR);\n\
         const { a } = RR;\n\
         export const all = [a];\n",
    )]);
    let sites = project.sites(&[]);
    let names: Vec<_> = sites
        .iter()
        .map(|site| (site.specifier.as_deref().unwrap(), site.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("useSelector", UsageSiteKind::Call),
            ("default", UsageSiteKind::Call),
            ("useStore", UsageSiteKind::Call),
            ("*", UsageSiteKind::NonCallReference),
            ("*", UsageSiteKind::ValueAlias),
        ]
    );
    let usage = project.usage(&counts_only()).unwrap();
    assert_eq!(specifier(&usage, "useStore").file_count, 1);
    assert_eq!(specifier(&usage, "*").unresolved.value_alias, 1);
}

#[test]
fn site_kinds_add_up_to_the_total() {
    let project = Project::new(&[
        (
            "src/main.ts",
            "import { useAppSelector } from './hooks';\n\
             import { Provider, useStore } from 'pkg';\n\
             useAppSelector(1);\n\
             const s = useStore;\n\
             export const all = [s, Provider, useAppSelector];\n",
        ),
        (
            "src/hooks.ts",
            "import { useSelector } from 'pkg';\n\
             export const useAppSelector = useSelector.withTypes();\n\
             useSelector(2);\n",
        ),
    ]);
    let usage = project
        .usage(
            &DependencyUsageQuery::new(
                Vec::new(),
                Some(SitePageRequest {
                    limit: 500,
                    cursor: None,
                }),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    let page = usage.sites.as_ref().unwrap();
    let wrapper_definitions = page
        .items
        .iter()
        .filter(|site| site.kind == UsageSiteKind::WrapperDefinition)
        .count();
    let counted: usize = usage
        .specifiers
        .iter()
        .map(|entry| {
            let unresolved = &entry.unresolved;
            entry.call_site_count
                + entry
                    .wrappers
                    .iter()
                    .map(|wrapper| wrapper.call_site_count)
                    .sum::<usize>()
                + unresolved.value_alias
                + unresolved.non_call_reference
                + unresolved.jsx_element
                + unresolved.re_export
                + unresolved.nested_wrapper
        })
        .sum();
    assert_eq!(counted + wrapper_definitions, page.total, "{usage:#?}");
    assert_eq!(page.total, page.items.len());
}

#[test]
fn a_wrapper_definition_replaces_the_call_at_the_same_offset() {
    let project = Project::new(&[(
        "src/hooks.ts",
        "import { useSelector } from 'pkg';\n\
         export const useAppSelector = useSelector.withTypes();\n",
    )]);
    let sites = project.sites(&[]);
    assert_eq!(sites.len(), 1, "{sites:#?}");
    assert_eq!(sites[0].kind, UsageSiteKind::WrapperDefinition);
    assert_eq!(sites[0].member.as_deref(), Some("withTypes"));
    let usage = project.usage(&counts_only()).unwrap();
    let entry = specifier(&usage, "useSelector");
    assert_eq!(entry.call_site_count, 0);
    assert_eq!(entry.wrappers.len(), 1);
    assert_eq!(entry.wrappers[0].shape, WrapperShape::Call);
    assert_eq!(entry.wrappers[0].line, 2);
}

#[test]
fn an_initializer_call_that_is_not_exported_is_a_call() {
    let project = Project::new(&[(
        "src/local.ts",
        "import { useSelector } from 'pkg';\n\
         const select = useSelector.withTypes();\n\
         export const read = () => select(1);\n",
    )]);
    let usage = project.usage(&counts_only()).unwrap();
    let entry = specifier(&usage, "useSelector");
    assert_eq!(entry.call_site_count, 1);
    assert!(entry.wrappers.is_empty());
}

#[test]
fn wrapper_detection_reads_the_export_list() {
    let project = Project::new(&[
        (
            "src/main.ts",
            "import { usePick, useHidden } from './local';\n\
             import Counter from './counter';\n\
             usePick(1);\n\
             export const all = [useHidden, Counter];\n",
        ),
        (
            "src/local.ts",
            "import { useSelector, connect } from 'pkg';\n\
             const typedPick = useSelector;\n\
             export { typedPick as usePick };\n\
             const hidden = useSelector;\n\
             export type { hidden as useHidden };\n",
        ),
        (
            "src/counter.ts",
            "import { connect } from 'pkg';\n\
             const C = () => null;\n\
             export default connect(1)(C);\n",
        ),
    ]);
    let usage = project.usage(&counts_only()).unwrap();
    let entry = specifier(&usage, "useSelector");
    let wrappers: Vec<_> = entry
        .wrappers
        .iter()
        .map(|wrapper| {
            (
                wrapper.export.as_str(),
                wrapper.shape,
                wrapper.call_site_count,
            )
        })
        .collect();
    assert_eq!(wrappers, vec![("usePick", WrapperShape::Alias, 1)]);
    assert_eq!(entry.unresolved.value_alias, 1, "{entry:#?}");
    let connect = specifier(&usage, "connect");
    assert_eq!(connect.call_site_count, 1);
    assert!(connect.wrappers.is_empty());
}

#[test]
fn a_wrapper_exported_again_is_a_nested_wrapper() {
    let project = Project::new(&[
        (
            "src/nested.ts",
            "import { useAppSelector } from './hooks';\n\
             export const useCount = useAppSelector;\n",
        ),
        (
            "src/hooks.ts",
            "import { useSelector } from 'pkg';\n\
             export const useAppSelector = useSelector.withTypes();\n",
        ),
    ]);
    let sites = project.sites(&["useSelector"]);
    let nested: Vec<_> = sites
        .iter()
        .filter(|site| site.kind == UsageSiteKind::NestedWrapper)
        .map(|site| (site.file.as_str(), site.via.as_deref()))
        .collect();
    assert_eq!(
        nested,
        vec![("src/nested.ts", Some("src/hooks.ts:useAppSelector"))]
    );
}

#[test]
fn a_binding_without_a_site_is_counted() {
    let project = Project::new(&[(
        "src/view.vue",
        "<script setup lang=\"ts\">\nimport { useStore } from 'pkg';\n</script>\n\
         <template><p>{{ useStore() }}</p></template>\n",
    )]);
    let usage = project.usage(&counts_only()).unwrap();
    let entry = specifier(&usage, "useStore");
    assert_eq!(entry.file_count, 1);
    assert_eq!(entry.unresolved.binding_without_site, 1, "{entry:#?}");
}

#[test]
fn a_value_import_read_only_as_a_type_is_type_only() {
    let project = Project::new(&[(
        "src/slice.ts",
        "import { PayloadAction, unusedHelper } from 'pkg';\n\
         export type Action = PayloadAction<number>;\n",
    )]);
    let usage = project.usage(&counts_only()).unwrap();
    let entry = specifier(&usage, "PayloadAction");
    assert_eq!(entry.file_count, 1);
    assert_eq!(entry.type_only_file_count, 1);
    assert_eq!(entry.unresolved.binding_without_site, 0, "{entry:#?}");
    // A binding with no reference at all stays a runtime binding.
    let unused = specifier(&usage, "unusedHelper");
    assert_eq!(unused.type_only_file_count, 0);
    assert_eq!(unused.unresolved.binding_without_site, 1);
}

#[test]
fn sites_sort_by_file_line_column_and_kind() {
    let project = Project::new(&[
        ("src/b.ts", "import { x } from 'pkg';\nx();\nx();\n"),
        (
            "src/a.ts",
            "import { y } from 'pkg';\nexport const z = [y];\ny();\n",
        ),
    ]);
    let sites = project.sites(&[]);
    let order: Vec<_> = sites
        .iter()
        .map(|site| (site.file.as_str(), site.line, site.kind))
        .collect();
    assert_eq!(
        order,
        vec![
            ("src/a.ts", 2, UsageSiteKind::NonCallReference),
            ("src/a.ts", 3, UsageSiteKind::Call),
            ("src/b.ts", 2, UsageSiteKind::Call),
            ("src/b.ts", 3, UsageSiteKind::Call),
        ]
    );
    assert_eq!(
        rows(&sites[..1]),
        vec![(
            "src/a.ts".to_owned(),
            Some("y".to_owned()),
            Some("y".to_owned()),
            UsageSiteKind::NonCallReference
        )]
    );
}

fn paged_project() -> Project {
    Project::new(&[(
        "src/main.ts",
        "import { a, b } from 'pkg';\na();\na();\nb();\nb();\nb();\n",
    )])
}

fn page(
    project: &Project,
    specifiers: &[&str],
    cursor: Option<String>,
) -> Result<UsageSitePage, UsageError> {
    let query = match DependencyUsageQuery::new(
        specifiers.iter().map(|name| (*name).to_owned()).collect(),
        Some(SitePageRequest { limit: 2, cursor }),
        None,
    ) {
        Ok(query) => query,
        Err(err) => panic!("the query is valid: {err}"),
    };
    project
        .usage(&query)
        .map(|usage| usage.sites.unwrap_or_else(|| panic!("the page is present")))
}

#[test]
fn a_cursor_walks_every_page_once() {
    let project = paged_project();
    let mut lines = Vec::new();
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let current = page(&project, &[], cursor).unwrap();
        assert_eq!(current.total, 5);
        lines.extend(current.items.iter().map(|site| site.line));
        pages += 1;
        match current.next_cursor {
            Some(next) => {
                assert!(next.starts_with("v1."));
                cursor = Some(next);
            }
            None => break,
        }
    }
    assert_eq!(pages, 3);
    assert_eq!(lines, vec![2, 3, 4, 5, 6]);
}

#[test]
fn an_invalid_or_foreign_cursor_is_rejected() {
    let project = paged_project();
    let first = page(&project, &[], None).unwrap();
    let cursor = first.next_cursor.unwrap();
    assert_eq!(
        page(&project, &["b"], Some(cursor.clone())).unwrap_err(),
        UsageError::InvalidCursor
    );
    for token in ["", "v1.", "v1.zz", "v2.00", "nonsense"] {
        assert_eq!(
            page(&project, &[], Some(token.to_owned())).unwrap_err(),
            UsageError::InvalidCursor,
            "{token}"
        );
    }
    let truncated = &cursor[..cursor.len() - 2];
    assert_eq!(
        page(&project, &[], Some(truncated.to_owned())).unwrap_err(),
        UsageError::InvalidCursor
    );
}

#[test]
fn a_selected_name_that_no_file_imports_has_zero_counts() {
    let project = paged_project();
    let query = DependencyUsageQuery::new(vec!["missing".to_owned()], None, None).unwrap();
    let usage = project.usage(&query).unwrap();
    assert_eq!(usage.specifiers.len(), 1);
    assert_eq!(usage.specifiers[0].name, "missing");
    assert_eq!(usage.specifiers[0].file_count, 0);
}

fn chain_project() -> Project {
    Project::new(&[
        (
            "src/a.ts",
            "import { b } from './b';\nexport const a = b;\n",
        ),
        (
            "src/b.ts",
            "import { c } from './c';\nexport const b = c;\n",
        ),
        (
            "src/c.ts",
            "import { d } from './d';\nexport const c = d;\n",
        ),
        (
            "src/d.ts",
            "import { x } from 'pkg';\nexport const d = x();\n",
        ),
    ])
}

fn closure(project: &Project, depth: u32) -> ConsumerClosure {
    let query = DependencyUsageQuery::new(Vec::new(), None, Some(depth)).unwrap();
    project.usage(&query).unwrap().closure.unwrap()
}

#[test]
fn a_closure_cut_at_its_depth_is_truncated() {
    let project = chain_project();
    let shallow = closure(&project, 1);
    assert_eq!(
        shallow.files,
        vec![ClosureFile {
            file: "src/c.ts".to_owned(),
            depth: 1
        }]
    );
    assert!(shallow.truncated);

    let deep = closure(&project, 3);
    let files: Vec<_> = deep
        .files
        .iter()
        .map(|file| (file.file.as_str(), file.depth))
        .collect();
    assert_eq!(
        files,
        vec![("src/c.ts", 1), ("src/b.ts", 2), ("src/a.ts", 3)]
    );
    assert_eq!(deep.file_count, 3);
    assert!(!deep.truncated);
}
