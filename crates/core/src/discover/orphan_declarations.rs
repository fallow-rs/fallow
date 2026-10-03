//! Orphan module declaration files.
//!
//! The graph seeds every declaration file (`.d.ts`, `.d.mts`, `.d.cts`) as an
//! entry point, because TypeScript reads ambient declarations through the
//! tsconfig file set and not through imports. That is right for a script-style
//! declaration file or one with a `declare global` or `declare module` block,
//! which adds to the global scope. A module declaration file without such a
//! block adds nothing to the global scope: it matters only when something
//! points to it.
//!
//! [`find_orphan_module_declaration_files`] names the module declaration files
//! that nothing points to. The graph does not seed them, so an orphan is
//! reported as an unused file and its imports no longer keep other modules
//! reachable. A declaration file stays seeded when any of these holds:
//!
//! - it is a script file, or it has a top-level `declare global` or
//!   string-named `declare module` block, or it did not parse cleanly;
//! - a sibling with the same stem exists (`foo.js` or `foo.ts` next to
//!   `foo.d.ts`, or the asset `styles.css` next to `styles.css.d.ts`);
//! - a package.json in its directory chain names it through `types`,
//!   `typings`, `typesVersions` or an `exports` `types` condition;
//! - a `/// <reference path="..." />` directive points to it;
//! - a tsconfig or jsconfig in its directory chain covers it through `files`,
//!   or through `include` (the default `include` is everything below the
//!   config) without an `exclude` match, or through `compilerOptions.typeRoots`.
//!
//! A reachable import of an orphan still makes it reachable through the graph.

use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::extract::ModuleInfo;
use crate::graph::is_declaration_file_path;

use super::{DiscoveredFile, FileId};

/// Extensions of a JS or TS module that a same-stem declaration file types.
const SIBLING_MODULE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// The suffixes of a declaration file name.
const DECLARATION_SUFFIXES: &[&str] = &[".d.ts", ".d.mts", ".d.cts"];

/// The `exclude` TypeScript applies when a config sets none.
const DEFAULT_TSCONFIG_EXCLUDE: &[&str] = &["node_modules", "bower_components", "jspm_packages"];

/// Upper bound on a local `extends` chain, a guard against cycles.
const MAX_EXTENDS_DEPTH: usize = 16;

/// Name the module declaration files that nothing points to.
///
/// `root` bounds the directory-chain lookups for package.json and tsconfig
/// files. The result is empty for a project without such files, and the cost
/// past the first pass over `files` is limited to the candidates.
#[must_use]
pub fn find_orphan_module_declaration_files(
    root: &Path,
    files: &[DiscoveredFile],
    modules: &[ModuleInfo],
) -> FxHashSet<FileId> {
    let module_by_id: FxHashMap<FileId, &ModuleInfo> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();

    let candidates: Vec<&DiscoveredFile> = files
        .iter()
        .filter(|file| is_declaration_file_path(&file.path))
        .filter(|file| {
            module_by_id
                .get(&file.id)
                .is_some_and(|module| is_plain_module_declaration(module))
        })
        .collect();
    if candidates.is_empty() {
        return FxHashSet::default();
    }

    let referenced = triple_slash_reference_targets(files, &module_by_id);
    let mut lookups = DirectoryChainLookups::default();

    candidates
        .into_iter()
        .filter(|file| !referenced.contains(&normalize_lexically(&file.path)))
        .filter(|file| !has_same_stem_sibling(&file.path))
        .filter(|file| {
            !lookups.named_by_package_json(root, &file.path)
                && !lookups.covered_by_tsconfig(root, &file.path)
        })
        .map(|file| file.id)
        .collect()
}

/// A cleanly parsed module file without global declarations.
fn is_plain_module_declaration(module: &ModuleInfo) -> bool {
    !module.has_global_declarations && module.parse_error_count == 0 && !module.parse_panicked
}

/// Every path a `/// <reference path="..." />` directive in the project can
/// name, resolved against the directory of the file that holds it. A path
/// without a declaration suffix also names the `.d.ts` file TypeScript tries.
fn triple_slash_reference_targets(
    files: &[DiscoveredFile],
    module_by_id: &FxHashMap<FileId, &ModuleInfo>,
) -> FxHashSet<PathBuf> {
    let mut targets = FxHashSet::default();
    for file in files {
        let Some(module) = module_by_id.get(&file.id) else {
            continue;
        };
        let Some(dir) = file.path.parent() else {
            continue;
        };
        for reference in &module.triple_slash_reference_paths {
            let target = normalize_lexically(&dir.join(reference));
            let mut with_suffix = target.clone().into_os_string();
            with_suffix.push(".d.ts");
            targets.insert(PathBuf::from(with_suffix));
            targets.insert(target);
        }
    }
    targets
}

/// The file name of a declaration file without its declaration suffix.
fn declaration_stem(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    DECLARATION_SUFFIXES
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
        .filter(|stem| !stem.is_empty())
}

/// Whether a JS or TS module, or an asset, with the same stem sits next to the
/// declaration file.
fn has_same_stem_sibling(path: &Path) -> bool {
    let (Some(dir), Some(stem)) = (path.parent(), declaration_stem(path)) else {
        return false;
    };
    dir.join(stem).is_file()
        || SIBLING_MODULE_EXTENSIONS
            .iter()
            .any(|extension| dir.join(format!("{stem}.{extension}")).is_file())
}

/// Per-directory caches for the package.json and tsconfig lookups, shared by
/// every candidate of one run.
#[derive(Default)]
struct DirectoryChainLookups {
    package_type_targets: FxHashMap<PathBuf, Vec<PathTarget>>,
    tsconfig_scopes: FxHashMap<PathBuf, Vec<TsconfigScope>>,
}

impl DirectoryChainLookups {
    fn named_by_package_json(&mut self, root: &Path, path: &Path) -> bool {
        directory_chain(root, path).any(|dir| {
            self.package_type_targets
                .entry(dir.to_path_buf())
                .or_insert_with(|| package_type_targets(dir))
                .iter()
                .any(|target| target.matches(path))
        })
    }

    fn covered_by_tsconfig(&mut self, root: &Path, path: &Path) -> bool {
        directory_chain(root, path).any(|dir| {
            self.tsconfig_scopes
                .entry(dir.to_path_buf())
                .or_insert_with(|| directory_tsconfig_scopes(dir))
                .iter()
                .any(|scope| scope.covers(path))
        })
    }
}

/// The ancestors of `path`, from its directory up to and including `root`.
fn directory_chain<'a>(root: &'a Path, path: &'a Path) -> impl Iterator<Item = &'a Path> {
    path.ancestors()
        .skip(1)
        .take_while(move |dir| dir.starts_with(root))
}

/// A path a package.json or tsconfig field names: one exact file, or a glob.
enum PathTarget {
    Exact(PathBuf),
    Glob(GlobMatcher),
}

impl PathTarget {
    fn matches(&self, path: &Path) -> bool {
        match self {
            Self::Exact(target) => target == path,
            Self::Glob(matcher) => matcher.is_match(slash_path(path)),
        }
    }
}

/// The declaration targets the package.json in `dir` names. A package.json
/// that exists but does not parse names everything, so it never hides a file.
fn package_type_targets(dir: &Path) -> Vec<PathTarget> {
    let Ok(content) = std::fs::read_to_string(dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&content) else {
        return glob_target(dir, "**").into_iter().collect();
    };

    let mut raw_targets: Vec<&str> = ["types", "typings"]
        .iter()
        .filter_map(|field| manifest.get(field).and_then(serde_json::Value::as_str))
        .collect();
    if let Some(versions) = manifest.get("typesVersions") {
        collect_strings(versions, &mut raw_targets);
    }
    if let Some(exports) = manifest.get("exports") {
        collect_export_type_targets(exports, false, &mut raw_targets);
    }

    raw_targets
        .into_iter()
        .flat_map(|raw| package_target(dir, raw))
        .collect()
}

/// Every string inside `value`, at any depth.
fn collect_strings<'a>(value: &'a serde_json::Value, out: &mut Vec<&'a str>) {
    match value {
        serde_json::Value::String(text) => out.push(text),
        serde_json::Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        serde_json::Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

/// The `exports` strings below a `types` condition (`types`, `types@<range>`),
/// plus every `exports` string with a declaration suffix.
fn collect_export_type_targets<'a>(
    value: &'a serde_json::Value,
    under_types: bool,
    out: &mut Vec<&'a str>,
) {
    match value {
        serde_json::Value::String(text) => {
            if under_types || DECLARATION_SUFFIXES.iter().any(|s| text.ends_with(s)) {
                out.push(text);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_export_type_targets(item, under_types, out);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                let is_types_condition = key == "types" || key.starts_with("types@");
                collect_export_type_targets(item, under_types || is_types_condition, out);
            }
        }
        _ => {}
    }
}

/// One package.json target: a `*` pattern matches across directories, the way
/// a subpath pattern substitutes any string; an exact path also names the
/// `.d.ts` file TypeScript tries when the path has no declaration suffix.
fn package_target(dir: &Path, raw: &str) -> Vec<PathTarget> {
    if raw.contains('*') {
        return glob_target(dir, raw).into_iter().collect();
    }
    let target = normalize_lexically(&dir.join(raw));
    let mut with_suffix = target.clone().into_os_string();
    with_suffix.push(".d.ts");
    vec![
        PathTarget::Exact(target),
        PathTarget::Exact(PathBuf::from(with_suffix)),
    ]
}

/// The file set of one tsconfig, after its local `extends` chain.
struct TsconfigScope {
    files: Vec<PathBuf>,
    include: Vec<GlobMatcher>,
    exclude: Vec<GlobMatcher>,
    type_roots: Vec<PathBuf>,
}

impl TsconfigScope {
    /// A scope that covers every file, for a config that does not parse.
    fn everything(dir: &Path) -> Self {
        Self {
            files: Vec::new(),
            include: pattern_matchers(dir, "**/*"),
            exclude: Vec::new(),
            type_roots: Vec::new(),
        }
    }

    fn covers(&self, path: &Path) -> bool {
        if self.files.iter().any(|file| file == path)
            || self
                .type_roots
                .iter()
                .any(|type_root| path.starts_with(type_root))
        {
            return true;
        }
        let text = slash_path(path);
        self.include.iter().any(|glob| glob.is_match(&text))
            && !self.exclude.iter().any(|glob| glob.is_match(&text))
    }
}

/// The scopes of every `tsconfig.json`, `tsconfig.*.json` and `jsconfig.json`
/// in `dir`.
fn directory_tsconfig_scopes(dir: &Path) -> Vec<TsconfigScope> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut configs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_tsconfig_name)
        })
        .filter(|path| path.is_file())
        .collect();
    configs.sort_unstable();
    configs
        .iter()
        .map(|config| load_tsconfig_scope(config))
        .collect()
}

fn is_tsconfig_name(name: &str) -> bool {
    name == "jsconfig.json"
        || name
            .strip_prefix("tsconfig")
            .and_then(|rest| rest.strip_suffix("json"))
            .is_some_and(|middle| {
                middle == "." || (middle.starts_with('.') && middle.ends_with('.'))
            })
}

/// The `files`, `include`, `exclude` and `typeRoots` fields of a config
/// after its local `extends` chain. Each field keeps the directory of the
/// config that set it, as TypeScript resolves them.
#[derive(Default)]
struct RawScope {
    files: Option<Vec<PathBuf>>,
    include: Option<(PathBuf, Vec<String>)>,
    exclude: Option<(PathBuf, Vec<String>)>,
    type_roots: Option<Vec<PathBuf>>,
}

fn load_tsconfig_scope(config: &Path) -> TsconfigScope {
    let dir = config.parent().unwrap_or(config);
    let mut visited = FxHashSet::default();
    let Some(raw) = load_raw_scope(config, &mut visited, 0) else {
        return TsconfigScope::everything(dir);
    };

    let include = match (&raw.include, &raw.files) {
        (Some((base, patterns)), _) => patterns
            .iter()
            .flat_map(|pattern| pattern_matchers(base, pattern))
            .collect(),
        (None, Some(_)) => Vec::new(),
        (None, None) => pattern_matchers(dir, "**/*"),
    };
    let exclude = match &raw.exclude {
        Some((base, patterns)) => patterns
            .iter()
            .flat_map(|pattern| pattern_matchers(base, pattern))
            .collect(),
        None => DEFAULT_TSCONFIG_EXCLUDE
            .iter()
            .flat_map(|pattern| pattern_matchers(dir, pattern))
            .collect(),
    };
    TsconfigScope {
        files: raw.files.unwrap_or_default(),
        include,
        exclude,
        type_roots: raw.type_roots.unwrap_or_default(),
    }
}

/// Read one config and the local configs it extends. `None` when a config in
/// the chain exists but does not parse, so the caller can keep every file.
/// A package `extends` (`@tsconfig/node20`) contributes no file-set fields.
fn load_raw_scope(
    config: &Path,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) -> Option<RawScope> {
    if depth > MAX_EXTENDS_DEPTH || !visited.insert(config.to_path_buf()) {
        return Some(RawScope::default());
    }
    let Ok(content) = std::fs::read_to_string(config) else {
        return Some(RawScope::default());
    };
    let value: serde_json::Value =
        fallow_config::jsonc::parse_to_value(content.trim_start_matches('\u{FEFF}')).ok()?;
    let dir = config.parent().unwrap_or(config);

    let mut scope = RawScope::default();
    for base in extends_paths(&value, dir) {
        let inherited = load_raw_scope(&base, visited, depth + 1)?;
        scope.files = inherited.files.or(scope.files);
        scope.include = inherited.include.or(scope.include);
        scope.exclude = inherited.exclude.or(scope.exclude);
        scope.type_roots = inherited.type_roots.or(scope.type_roots);
    }

    if let Some(files) = string_array(&value, "files") {
        scope.files = Some(
            files
                .iter()
                .map(|file| normalize_lexically(&dir.join(file)))
                .collect(),
        );
    }
    if let Some(include) = string_array(&value, "include") {
        scope.include = Some((dir.to_path_buf(), include));
    }
    if let Some(exclude) = string_array(&value, "exclude") {
        scope.exclude = Some((dir.to_path_buf(), exclude));
    }
    if let Some(type_roots) = value
        .get("compilerOptions")
        .and_then(|options| string_array(options, "typeRoots"))
    {
        scope.type_roots = Some(
            type_roots
                .iter()
                .map(|type_root| normalize_lexically(&dir.join(type_root)))
                .collect(),
        );
    }
    Some(scope)
}

/// The local config files an `extends` value names, in order.
fn extends_paths(value: &serde_json::Value, dir: &Path) -> Vec<PathBuf> {
    let entries: Vec<&str> = match value.get("extends") {
        Some(serde_json::Value::String(entry)) => vec![entry.as_str()],
        Some(serde_json::Value::Array(entries)) => entries
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect(),
        _ => Vec::new(),
    };
    entries
        .into_iter()
        .filter(|entry| entry.starts_with('.') || Path::new(entry).is_absolute())
        .map(|entry| {
            let path = normalize_lexically(&dir.join(entry));
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                path
            } else {
                let mut with_json = path.into_os_string();
                with_json.push(".json");
                PathBuf::from(with_json)
            }
        })
        .collect()
}

fn string_array(value: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    value.get(key)?.as_array().map(|items| {
        items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect()
    })
}

/// Matchers for one tsconfig `include` or `exclude` pattern, relative to
/// `base`. A pattern whose last segment has no wildcard also matches the
/// directory subtree it names (`src` matches `src/**/*`), as TypeScript reads
/// a segment without a file extension as a directory.
fn pattern_matchers(base: &Path, pattern: &str) -> Vec<GlobMatcher> {
    let last_segment = pattern
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    let mut patterns = vec![pattern.trim_end_matches('/').to_string()];
    if !last_segment.contains(['*', '?']) {
        patterns.push(format!("{}/**/*", pattern.trim_end_matches('/')));
    }
    patterns
        .iter()
        .filter_map(|pattern| relative_glob(base, pattern, true))
        .collect()
}

/// A package.json glob target. Its `*` crosses directories, the way a subpath
/// pattern substitutes any string.
fn glob_target(base: &Path, pattern: &str) -> Option<PathTarget> {
    relative_glob(base, pattern, false).map(PathTarget::Glob)
}

/// Compile `pattern` relative to `base` into a matcher over absolute,
/// forward-slash paths. Leading `.` and `..` segments resolve against `base`
/// before the glob is built, and the literal base is escaped. With
/// `literal_separator`, `*` stops at `/` and only `**` crosses directories.
fn relative_glob(base: &Path, pattern: &str, literal_separator: bool) -> Option<GlobMatcher> {
    let mut literal = normalize_lexically(base);
    let mut rest: Vec<&str> = Vec::new();
    for segment in pattern.split('/').filter(|segment| !segment.is_empty()) {
        if !rest.is_empty() || segment.contains(['*', '?', '[', '{']) {
            rest.push(segment);
            continue;
        }
        match segment {
            "." => {}
            ".." => {
                literal.pop();
            }
            _ => literal.push(segment),
        }
    }
    let mut glob = globset::escape(&slash_path(&literal));
    for segment in rest {
        glob.push('/');
        glob.push_str(segment);
    }
    GlobBuilder::new(&glob)
        .literal_separator(literal_separator)
        .build()
        .ok()
        .map(|glob| glob.compile_matcher())
}

/// A path spelled with forward slashes, the form the globs match.
fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Resolve `.` and `..` components without touching the file system.
fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(id: u32, has_global_declarations: bool, references: &[&str]) -> ModuleInfo {
        ModuleInfo {
            has_global_declarations,
            triple_slash_reference_paths: references.iter().map(|r| (*r).to_string()).collect(),
            ..ModuleInfo::empty(FileId(id))
        }
    }

    fn write(root: &Path, relative: &str, content: &str) -> DiscoveredFile {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(&path, content).expect("write file");
        DiscoveredFile {
            id: FileId(0),
            path,
            size_bytes: 1,
        }
    }

    /// Analyze one declaration file (id 0) next to the given extra files and
    /// modules; return whether it is an orphan.
    fn is_orphan(root: &Path, declaration: &str, module_info: ModuleInfo) -> bool {
        is_orphan_with(root, declaration, module_info, &[])
    }

    fn is_orphan_with(
        root: &Path,
        declaration: &str,
        module_info: ModuleInfo,
        others: &[(DiscoveredFile, ModuleInfo)],
    ) -> bool {
        let mut files = vec![DiscoveredFile {
            id: FileId(0),
            path: root.join(declaration),
            size_bytes: 1,
        }];
        let mut modules = vec![module_info];
        for (file, info) in others {
            files.push(file.clone());
            modules.push(info.clone());
        }
        find_orphan_module_declaration_files(root, &files, &modules).contains(&FileId(0))
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        write(dir.path(), "types/orphan.d.ts", "export interface A {}\n");
        dir
    }

    #[test]
    fn module_declaration_without_a_consumer_is_an_orphan() {
        let dir = project();
        assert!(is_orphan(
            dir.path(),
            "types/orphan.d.ts",
            module(0, false, &[])
        ));
    }

    #[test]
    fn global_declarations_and_parse_errors_keep_the_file() {
        let dir = project();
        assert!(!is_orphan(
            dir.path(),
            "types/orphan.d.ts",
            module(0, true, &[])
        ));
        let broken = ModuleInfo {
            parse_error_count: 1,
            ..module(0, false, &[])
        };
        assert!(!is_orphan(dir.path(), "types/orphan.d.ts", broken));
    }

    #[test]
    fn same_stem_module_or_asset_sibling_keeps_the_file() {
        for sibling in ["types/orphan.js", "types/orphan.ts", "types/orphan.mjs"] {
            let dir = project();
            write(dir.path(), sibling, "export {};\n");
            assert!(
                !is_orphan(dir.path(), "types/orphan.d.ts", module(0, false, &[])),
                "{sibling} must keep the declaration file"
            );
        }
        let dir = tempfile::tempdir().expect("temp dir");
        write(dir.path(), "src/styles.css", "a {}\n");
        write(
            dir.path(),
            "src/styles.css.d.ts",
            "export const a: string;\n",
        );
        assert!(!is_orphan(
            dir.path(),
            "src/styles.css.d.ts",
            module(0, false, &[])
        ));
    }

    #[test]
    fn package_json_type_fields_keep_the_file() {
        for manifest in [
            r#"{"types": "types/orphan.d.ts"}"#,
            r#"{"typings": "./types/orphan"}"#,
            r#"{"typesVersions": {"*": {"*": ["types/*"]}}}"#,
            r#"{"exports": {".": {"types": "./types/orphan.d.ts", "default": "./index.js"}}}"#,
            r#"{"exports": {"./*": {"types@>=5": "./types/*.d.ts"}}}"#,
            r#"{"exports": {".": {"import": {"types": "./types/orphan.d.ts"}}}}"#,
        ] {
            let dir = project();
            write(dir.path(), "package.json", manifest);
            assert!(
                !is_orphan(dir.path(), "types/orphan.d.ts", module(0, false, &[])),
                "{manifest} must keep the declaration file"
            );
        }
    }

    #[test]
    fn package_json_without_a_type_field_for_the_file_keeps_nothing() {
        let dir = project();
        write(
            dir.path(),
            "package.json",
            r#"{"types": "types/other.d.ts", "exports": {".": {"default": "./types/orphan.js"}}}"#,
        );
        assert!(is_orphan(
            dir.path(),
            "types/orphan.d.ts",
            module(0, false, &[])
        ));
    }

    #[test]
    fn triple_slash_reference_keeps_the_file() {
        for reference in ["../types/orphan.d.ts", "../types/orphan"] {
            let dir = project();
            let referrer = write(dir.path(), "src/index.ts", "export {};\n");
            let referrer = DiscoveredFile {
                id: FileId(1),
                ..referrer
            };
            assert!(
                !is_orphan_with(
                    dir.path(),
                    "types/orphan.d.ts",
                    module(0, false, &[]),
                    &[(referrer, module(1, false, &[reference]))],
                ),
                "{reference} must keep the declaration file"
            );
        }
    }

    #[test]
    fn tsconfig_without_include_or_files_covers_its_subtree() {
        let dir = project();
        write(dir.path(), "tsconfig.json", r#"{"compilerOptions": {}}"#);
        assert!(!is_orphan(
            dir.path(),
            "types/orphan.d.ts",
            module(0, false, &[])
        ));
    }

    #[test]
    fn tsconfig_include_files_and_type_roots_cover_the_file() {
        for config in [
            r#"{"include": ["types"]}"#,
            r#"{"include": ["types/**/*.d.ts"]}"#,
            r#"{"include": ["./types/*"]}"#,
            r#"{"files": ["types/orphan.d.ts"]}"#,
            r#"{"files": ["types/orphan.d.ts"], "exclude": ["types"]}"#,
            r#"{"include": ["src"], "compilerOptions": {"typeRoots": ["./types"]}}"#,
            "{ // comment\n \"include\": [\"types\",],\n}",
            "{ not json",
        ] {
            let dir = project();
            write(dir.path(), "tsconfig.json", config);
            assert!(
                !is_orphan(dir.path(), "types/orphan.d.ts", module(0, false, &[])),
                "{config} must keep the declaration file"
            );
        }
    }

    #[test]
    fn tsconfig_that_does_not_cover_the_file_keeps_nothing() {
        for config in [
            r#"{"include": ["src"]}"#,
            r#"{"include": ["types/*.ts"], "exclude": ["types/orphan.d.ts"]}"#,
            r#"{"include": ["types"], "exclude": ["types"]}"#,
            r#"{"files": [], "references": [{"path": "./src"}]}"#,
            r#"{"include": ["**/*"], "exclude": ["types/**"]}"#,
        ] {
            let dir = project();
            write(dir.path(), "tsconfig.json", config);
            assert!(
                is_orphan(dir.path(), "types/orphan.d.ts", module(0, false, &[])),
                "{config} must not keep the declaration file"
            );
        }
    }

    #[test]
    fn tsconfig_variant_and_local_extends_are_read() {
        let dir = project();
        write(dir.path(), "tsconfig.json", r#"{"files": []}"#);
        write(
            dir.path(),
            "tsconfig.types.json",
            r#"{"extends": "./configs/base"}"#,
        );
        write(
            dir.path(),
            "configs/base.json",
            r#"{"include": ["../types"]}"#,
        );
        assert!(!is_orphan(
            dir.path(),
            "types/orphan.d.ts",
            module(0, false, &[])
        ));
    }

    #[test]
    fn package_extends_adds_no_file_set() {
        let dir = project();
        write(
            dir.path(),
            "tsconfig.json",
            r#"{"extends": "@tsconfig/strictest/tsconfig.json", "include": ["src"]}"#,
        );
        assert!(is_orphan(
            dir.path(),
            "types/orphan.d.ts",
            module(0, false, &[])
        ));
    }

    #[test]
    fn nested_tsconfig_in_the_directory_chain_counts() {
        let dir = project();
        write(dir.path(), "tsconfig.json", r#"{"include": ["src"]}"#);
        write(dir.path(), "types/tsconfig.json", "{}");
        assert!(!is_orphan(
            dir.path(),
            "types/orphan.d.ts",
            module(0, false, &[])
        ));
    }

    #[test]
    fn declaration_stem_strips_every_declaration_suffix() {
        assert_eq!(declaration_stem(Path::new("a/b.d.ts")), Some("b"));
        assert_eq!(declaration_stem(Path::new("a/b.d.mts")), Some("b"));
        assert_eq!(declaration_stem(Path::new("a/b.css.d.cts")), Some("b.css"));
        assert_eq!(declaration_stem(Path::new("a/.d.ts")), None);
    }
}
