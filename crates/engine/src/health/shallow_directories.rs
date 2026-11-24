//! Shallow directories: directories whose interface is as large as what they
//! hide.
//!
//! An interface item is a module-scope function or an exported class. A
//! directory shows an item when a file outside the directory uses it, and it
//! hides an item that no outside file uses. The unit is recursive: a
//! directory holds the items of its subdirectories too.
//!
//! The directories are checked from the deepest up. A reported directory
//! claims its items, so a parent directory counts only the items that no
//! reported child claimed. Without this rule one wide child makes each of
//! its parents shallow too.

use std::path::{Path, PathBuf};

use fallow_config::ShallowDirectoriesConfig;
use fallow_graph::graph::{ExportNamespace, ModuleGraph, ModuleNode, ReferenceKind};
use fallow_output::{
    ShallowDirectoriesReport, ShallowDirectoryFinding, ShallowDirectoryItem,
    ShallowDirectoryItemKind,
};
use globset::{GlobSet, GlobSetBuilder};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::discover::FileId;
use crate::source::ModuleInfo;
use fallow_types::extract::{ExportName, MemberKind};

/// Shown items listed per directory. The rest is a count.
const MAX_LISTED_ITEMS: usize = 7;

/// Below this mean of shown items per file, a directory is a catalog.
const MIN_ITEMS_PER_FILE: f64 = 1.5;

/// A directory whose shown items come from fewer files is a file finding.
const MIN_SHOWN_FILES: usize = 2;

/// From this share of shown items with one parameter list, the directory is
/// a family of functions with one role.
const MAX_FAMILY_SHARE: f64 = 0.3;

/// A family needs at least this many functions with one parameter list.
const MIN_FAMILY_SIZE: usize = 3;

/// Below this mean of outside files per shown item, the interface has few
/// readers.
const MIN_MEAN_OUTSIDE_FILES: f64 = 2.0;

/// The parameter name that [`signature_of`] writes for a destructured
/// parameter.
const DESTRUCTURED_PARAM: &str = "{}";

/// File extensions that can be an anchor file.
const SCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// File extensions whose PascalCase functions and classes are UI components.
const COMPONENT_FILE_EXTENSIONS: &[&str] = &["tsx", "jsx"];

/// File-name markers of story files. A story renders a component the way a
/// test does, so it is not an outside user.
const STORY_FILE_MARKERS: &[&str] = &[".stories.", ".story."];

/// Inputs of [`find_shallow_directories`].
pub(super) struct ShallowDirectoriesInput<'a> {
    pub(super) graph: &'a ModuleGraph,
    pub(super) modules: &'a [ModuleInfo],
    pub(super) root: &'a Path,
    pub(super) config: &'a ShallowDirectoriesConfig,
}

/// One interface item before the per-directory counts.
struct Item {
    /// The exported name, or the local binding (else the file stem) of a
    /// default export.
    name: String,
    is_default: bool,
    file: FileId,
    /// Directory of the declaring file, relative to the root.
    dir: PathBuf,
    line: u32,
    kind: ShallowDirectoryItemKind,
    members: Option<u32>,
    /// Directories (relative to the root) of the files that use the item and
    /// count as outside users of some directory: no test, story or the
    /// declaring file itself.
    users: Vec<(FileId, PathBuf)>,
    /// Parameter names of a function item, for the shape rule.
    signature: Option<String>,
}

impl Item {
    /// Whether a file outside `dir` uses the item. `dir` holds the item.
    fn shown_in(&self, dir: &Path) -> bool {
        self.users.iter().any(|(_, user)| !user.starts_with(dir))
    }

    fn outside_files(&self, dir: &Path) -> u32 {
        let count = self
            .users
            .iter()
            .filter(|(_, user)| !user.starts_with(dir))
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }
}

/// Find the shallow directories of the project.
pub(super) fn find_shallow_directories(
    input: &ShallowDirectoriesInput<'_>,
) -> ShallowDirectoriesReport {
    let ignore = build_ignore_set(&input.config.ignore);
    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        input.modules.iter().map(|m| (m.file_id, m)).collect();
    let relative_paths: FxHashMap<FileId, PathBuf> = input
        .graph
        .modules
        .iter()
        .map(|node| (node.file_id, relative_to(input.root, &node.path)))
        .collect();

    let published = published_exports(input.graph);
    let items: Vec<Item> = input
        .graph
        .modules
        .iter()
        .filter(|node| !is_excluded_file(input.root, &relative_paths[&node.file_id]))
        .filter_map(|node| {
            let module = modules_by_id.get(&node.file_id)?;
            Some(collect_file_items(
                node,
                module,
                input.root,
                &relative_paths,
                &published,
            ))
        })
        .flatten()
        .collect();

    let directories =
        evaluate_directories(input.root, &items, &relative_paths, &ignore, input.config);
    ShallowDirectoriesReport {
        min_interface: input.config.min_interface,
        total: directories.len(),
        directories,
    }
}

/// The exports that an entry point re-exports, directly or through a chain of
/// re-exports, as `(file, export name)` pairs. Such an export is public API:
/// the package shows it on purpose, so it is no item.
fn published_exports(graph: &ModuleGraph) -> FxHashSet<(FileId, String)> {
    let nodes: FxHashMap<FileId, &ModuleNode> = graph
        .modules
        .iter()
        .map(|node| (node.file_id, node))
        .collect();
    let mut published: FxHashSet<(FileId, String)> = FxHashSet::default();
    let mut star_seen: FxHashSet<FileId> = FxHashSet::default();
    let mut queue: Vec<(FileId, Option<String>)> = graph
        .modules
        .iter()
        .filter(|node| node.is_entry_point())
        .map(|node| (node.file_id, None))
        .collect();
    while let Some((file, name)) = queue.pop() {
        let Some(node) = nodes.get(&file) else {
            continue;
        };
        for edge in &node.re_exports {
            if edge.is_type_only {
                continue;
            }
            let forwards = name
                .as_deref()
                .is_none_or(|name| edge.exported_name == name || edge.exported_name == STAR);
            if !forwards {
                continue;
            }
            if edge.imported_name == STAR {
                // `export * from` and `export * as ns from` forward every
                // export of the source module.
                let wanted = match (name.as_deref(), edge.exported_name.as_str()) {
                    (Some(name), STAR) => Some(name.to_string()),
                    _ => None,
                };
                if wanted.is_none() {
                    if !star_seen.insert(edge.source_file) {
                        continue;
                    }
                    if let Some(source) = nodes.get(&edge.source_file) {
                        for export in &source.exports {
                            published.insert((edge.source_file, export.name.to_string()));
                        }
                    }
                } else if let Some(wanted) = wanted.as_deref() {
                    published.insert((edge.source_file, wanted.to_string()));
                }
                queue.push((edge.source_file, wanted));
            } else if published.insert((edge.source_file, edge.imported_name.clone())) {
                queue.push((edge.source_file, Some(edge.imported_name.clone())));
            }
        }
    }
    published
}

/// The name that a star re-export edge carries.
const STAR: &str = "*";

fn build_ignore_set(patterns: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = fallow_config::compile_path_glob(pattern) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

fn relative_to(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// Test, test support and story files neither hold items nor count as
/// outside users.
fn is_excluded_file(root: &Path, relative: &Path) -> bool {
    if crate::test_paths::is_test_path(root, relative) {
        return true;
    }
    relative
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            STORY_FILE_MARKERS
                .iter()
                .any(|marker| name.contains(marker))
        })
}

/// The items that one file declares: its module-scope functions and its
/// exported classes.
fn collect_file_items(
    node: &ModuleNode,
    module: &ModuleInfo,
    root: &Path,
    relative_paths: &FxHashMap<FileId, PathBuf>,
    published: &FxHashSet<(FileId, String)>,
) -> Vec<Item> {
    let relative = &relative_paths[&node.file_id];
    let dir = relative.parent().map(Path::to_path_buf).unwrap_or_default();
    let line_of = |offset: u32| {
        fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, offset).0
    };

    // Local binding name of each value export, by export name.
    let export_bindings: FxHashMap<&ExportName, &str> = module
        .exports
        .iter()
        .filter(|export| !export.is_type_only)
        .filter_map(|export| {
            let binding = export.local_name.as_deref().or(match &export.name {
                ExportName::Named(name) => Some(name.as_str()),
                ExportName::Default => None,
            })?;
            Some((&export.name, binding))
        })
        .collect();

    let mut items = Vec::new();
    let mut exported_bindings: FxHashSet<&str> = FxHashSet::default();
    for symbol in node.exports.iter().filter(|symbol| !symbol.is_type_only) {
        if published.contains(&(node.file_id, symbol.name.to_string())) {
            continue;
        }
        let users = outside_users(symbol, node.file_id, root, relative_paths);
        let kind = item_kind(symbol, module, export_bindings.get(&symbol.name).copied());
        let Some(kind) = kind else {
            continue;
        };
        if is_component(
            relative,
            &symbol.name,
            export_bindings.get(&symbol.name).copied(),
        ) {
            continue;
        }
        if let Some(binding) = export_bindings.get(&symbol.name) {
            exported_bindings.insert(binding);
        }
        let members = (kind == ShallowDirectoryItemKind::Class).then(|| {
            let count = symbol
                .members
                .iter()
                .filter(|member| is_public_class_member(member.kind, member.is_protected))
                .count();
            u32::try_from(count).unwrap_or(u32::MAX)
        });
        let name = match &symbol.name {
            ExportName::Named(name) => name.clone(),
            ExportName::Default => export_bindings
                .get(&symbol.name)
                .map(|binding| (*binding).to_string())
                .or_else(|| file_stem(relative))
                .unwrap_or_else(|| "default".to_string()),
        };
        items.push(Item {
            name,
            is_default: matches!(symbol.name, ExportName::Default),
            file: node.file_id,
            dir: dir.clone(),
            line: line_of(symbol.span.start),
            kind,
            members,
            users,
            signature: export_bindings
                .get(&symbol.name)
                .and_then(|binding| signature_of(module, binding)),
        });
    }

    // Module-scope functions that no export shows are hidden items.
    if let Some(contracts) = module.function_contracts.as_deref() {
        for contract in &contracts.declarations {
            if exported_bindings.contains(&*contract.binding_name)
                || is_component_name(relative, &contract.binding_name)
            {
                continue;
            }
            items.push(Item {
                name: contract.binding_name.to_string(),
                is_default: false,
                file: node.file_id,
                dir: dir.clone(),
                line: line_of(contract.span_start),
                kind: ShallowDirectoryItemKind::Function,
                members: None,
                users: Vec::new(),
                signature: None,
            });
        }
    }
    items
}

/// A UI component is a unit of its own with props as its interface, so it is
/// no item. A component kit directory then reports nothing.
fn is_component(relative: &Path, name: &ExportName, binding: Option<&str>) -> bool {
    let name = match name {
        ExportName::Named(name) => name.as_str(),
        ExportName::Default => match binding {
            Some(binding) => binding,
            None => return is_component_file(relative),
        },
    };
    is_component_name(relative, name)
}

fn is_component_name(relative: &Path, name: &str) -> bool {
    is_component_file(relative) && name.starts_with(|c: char| c.is_ascii_uppercase())
}

fn is_component_file(relative: &Path) -> bool {
    relative
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| COMPONENT_FILE_EXTENSIONS.contains(&ext))
}

fn file_stem(relative: &Path) -> Option<String> {
    relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.split('.').next().unwrap_or(stem).to_string())
}

fn is_public_class_member(kind: MemberKind, is_protected: bool) -> bool {
    matches!(kind, MemberKind::ClassMethod | MemberKind::ClassProperty) && !is_protected
}

/// Whether an export is a function or a class item. A function export binds a
/// module-scope function (call facts). A class export has class members.
fn item_kind(
    symbol: &fallow_graph::graph::ExportSymbol,
    module: &ModuleInfo,
    binding: Option<&str>,
) -> Option<ShallowDirectoryItemKind> {
    // A class without a method holds data only, like a constant.
    if symbol
        .members
        .iter()
        .any(|member| member.kind == MemberKind::ClassMethod)
    {
        return Some(ShallowDirectoryItemKind::Class);
    }
    if symbol
        .members
        .iter()
        .any(|member| member.kind == MemberKind::ClassProperty)
    {
        return None;
    }
    let binding = binding?;
    let contracts = module.function_contracts.as_deref()?;
    contracts
        .declarations
        .iter()
        .any(|contract| &*contract.binding_name == binding)
        .then_some(ShallowDirectoryItemKind::Function)
}

/// Directories of the files that use an export. A re-export is not a use:
/// the graph credits the origin export with each file that imports it through
/// the barrel.
fn outside_users(
    symbol: &fallow_graph::graph::ExportSymbol,
    own_file: FileId,
    root: &Path,
    relative_paths: &FxHashMap<FileId, PathBuf>,
) -> Vec<(FileId, PathBuf)> {
    let mut seen: FxHashSet<FileId> = FxHashSet::default();
    symbol
        .references
        .iter()
        .filter(|reference| {
            reference.namespace == ExportNamespace::Value
                && reference.from_file != own_file
                && !matches!(
                    reference.kind,
                    ReferenceKind::ReExport | ReferenceKind::SideEffectImport
                )
        })
        .filter(|reference| seen.insert(reference.from_file))
        .filter_map(|reference| {
            relative_paths
                .get(&reference.from_file)
                .map(|path| (reference.from_file, path))
        })
        .filter(|(_, path)| !is_excluded_file(root, path))
        .map(|(file, path)| {
            (
                file,
                path.parent().map(Path::to_path_buf).unwrap_or_default(),
            )
        })
        .collect()
}

/// Whether a directory that passes the counts acts as one module. Three
/// shapes do not, and a directory of such a shape claims no items, so its
/// parent directory counts them:
///
/// - A catalog: the shown items come about one per file (one hook, model or
///   task per file), or all from one file. Each file is then the unit that
///   hides its code.
/// - A family: many shown functions share one parameter list, for example a
///   set of editor commands `(state, dispatch)`. The width is the product.
/// - Low reuse: each shown item has about one outside user. Such an item is a
///   helper in the wrong place, and the wide interface costs few readers.
fn is_module(dir: &Path, shown: &[usize], items: &[Item]) -> bool {
    let files: FxHashSet<FileId> = shown.iter().map(|&index| items[index].file).collect();
    if files.len() < MIN_SHOWN_FILES || ratio(shown.len(), files.len()) < MIN_ITEMS_PER_FILE {
        return false;
    }
    if family_share(shown, items) >= MAX_FAMILY_SHARE {
        return false;
    }
    let outside_uses: usize = shown
        .iter()
        .map(|&index| items[index].outside_files(dir) as usize)
        .sum();
    ratio(outside_uses, shown.len()) >= MIN_MEAN_OUTSIDE_FILES
}

/// The share of shown items that have the most common parameter list. Only
/// lists with a named parameter count: an empty list or one destructured
/// object says little about the role of a function.
fn family_share(shown: &[usize], items: &[Item]) -> f64 {
    let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
    for &index in shown {
        if let Some(signature) = items[index].signature.as_deref()
            && signature
                .split(',')
                .any(|param| param != DESTRUCTURED_PARAM && !param.is_empty())
        {
            *counts.entry(signature).or_default() += 1;
        }
    }
    let largest = counts.values().copied().max().unwrap_or(0);
    if largest < MIN_FAMILY_SIZE {
        return 0.0;
    }
    ratio(largest, shown.len())
}

#[expect(
    clippy::cast_precision_loss,
    reason = "item and file counts stay far below 2^52"
)]
fn ratio(part: usize, whole: usize) -> f64 {
    part as f64 / whole.max(1) as f64
}

/// Per-directory counts, deepest directory first, with the claim rule.
fn evaluate_directories(
    root: &Path,
    items: &[Item],
    relative_paths: &FxHashMap<FileId, PathBuf>,
    ignore: &GlobSet,
    config: &ShallowDirectoriesConfig,
) -> Vec<ShallowDirectoryFinding> {
    let mut items_by_dir: FxHashMap<PathBuf, Vec<usize>> = FxHashMap::default();
    for (index, item) in items.iter().enumerate() {
        for dir in item
            .dir
            .ancestors()
            .filter(|dir| !dir.as_os_str().is_empty())
        {
            items_by_dir
                .entry(dir.to_path_buf())
                .or_default()
                .push(index);
        }
    }
    let mut dirs: Vec<&PathBuf> = items_by_dir.keys().collect();
    dirs.sort_by(|a, b| {
        b.components()
            .count()
            .cmp(&a.components().count())
            .then_with(|| a.cmp(b))
    });

    let mut claimed = vec![false; items.len()];
    let mut findings = Vec::new();
    for dir in dirs {
        let members: Vec<usize> = items_by_dir[dir]
            .iter()
            .copied()
            .filter(|&index| !claimed[index])
            .collect();
        let (shown, hidden): (Vec<usize>, Vec<usize>) = members
            .iter()
            .partition(|&&index| items[index].shown_in(dir));
        let shown_count = u32::try_from(shown.len()).unwrap_or(u32::MAX);
        let hidden_count = u32::try_from(hidden.len()).unwrap_or(u32::MAX);
        if shown_count < config.min_interface || shown_count < hidden_count {
            continue;
        }
        if !is_module(dir, &shown, items) {
            continue;
        }
        for &index in &members {
            claimed[index] = true;
        }
        if ignore.is_match(dir) {
            continue;
        }
        findings.push(build_finding(
            root,
            dir,
            &shown,
            hidden_count,
            items,
            relative_paths,
        ));
    }
    findings.sort_by(|a, b| {
        b.shown_count
            .cmp(&a.shown_count)
            .then_with(|| a.path.cmp(&b.path))
    });
    findings
}

fn build_finding(
    root: &Path,
    dir: &Path,
    shown: &[usize],
    hidden_count: u32,
    items: &[Item],
    relative_paths: &FxHashMap<FileId, PathBuf>,
) -> ShallowDirectoryFinding {
    let mut listed: Vec<ShallowDirectoryItem> = shown
        .iter()
        .map(|&index| {
            let item = &items[index];
            ShallowDirectoryItem {
                name: item.name.clone(),
                is_default: item.is_default,
                path: relative_paths[&item.file].clone(),
                line: item.line,
                kind: item.kind,
                outside_files: item.outside_files(dir),
                members: item.members,
            }
        })
        .collect();
    listed.sort_by(|a, b| {
        b.outside_files
            .cmp(&a.outside_files)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    let more_items =
        u32::try_from(listed.len().saturating_sub(MAX_LISTED_ITEMS)).unwrap_or(u32::MAX);
    listed.truncate(MAX_LISTED_ITEMS);
    let relative_dir = dir.to_string_lossy().replace('\\', "/");
    let shown_files: FxHashSet<FileId> = shown.iter().map(|&index| items[index].file).collect();
    ShallowDirectoryFinding {
        path: dir.to_path_buf(),
        anchor_file: anchor_file(dir, relative_paths, root),
        shown_count: u32::try_from(shown.len()).unwrap_or(u32::MAX),
        shown_files: u32::try_from(shown_files.len()).unwrap_or(u32::MAX),
        hidden_count,
        items: listed,
        more_items,
        actions: ShallowDirectoryFinding::actions_for(&relative_dir),
    }
}

/// The `index` file directly in the directory, else the first file below it.
fn anchor_file(dir: &Path, relative_paths: &FxHashMap<FileId, PathBuf>, root: &Path) -> PathBuf {
    let mut below: Vec<&PathBuf> = relative_paths
        .values()
        .filter(|path| {
            path.starts_with(dir) && !is_excluded_file(root, path) && is_script_file(path)
        })
        .collect();
    below.sort();
    below
        .iter()
        .find(|path| {
            path.parent() == Some(dir)
                && path
                    .file_stem()
                    .is_some_and(|stem| stem.eq_ignore_ascii_case("index"))
        })
        .or_else(|| below.first())
        .map_or_else(|| dir.to_path_buf(), |path| (*path).clone())
}

fn is_script_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SCRIPT_EXTENSIONS.contains(&ext))
}

fn signature_of(module: &ModuleInfo, binding: &str) -> Option<String> {
    let contracts = module.function_contracts.as_deref()?;
    let contract = contracts
        .declarations
        .iter()
        .find(|contract| &*contract.binding_name == binding)?;
    Some(
        contract
            .params
            .iter()
            .map(|param| param.name.as_deref().unwrap_or(DESTRUCTURED_PARAM))
            .collect::<Vec<_>>()
            .join(","),
    )
}
