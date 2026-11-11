//! A lexical scan for the import specifiers of a source text whose imports
//! the facts do not hold.
//!
//! A module with a parse error, and an HTML page with an inline script, can
//! call a function of another module only after an import of that module.
//! An import names its module with a specifier string. The scan finds each
//! specifier string after `from`, `import`, `require` and the loader names of
//! test frameworks, also inside comments and strings, so a scan finds more
//! specifiers than the module has, never fewer. When the text can load a
//! module by a computed name, the scan returns [`Scan::Opaque`].

use std::path::{Component, Path, PathBuf};

use crate::discover::FileId;

/// The result of a scan of one source text.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Scan {
    /// The text can load a module whose name the scan cannot read.
    Opaque,
    /// Each specifier string of the text.
    Specifiers(Vec<String>),
}

/// Extensions that a specifier can name or leave out for a source module.
const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "vue", "svelte", "astro", "mdx",
];

/// Functions of test frameworks that load a module, as `import` does.
const LOADER_NAMES: &[&str] = &[
    "requireActual",
    "requireMock",
    "importActual",
    "importMock",
    "importOriginal",
    "importScripts",
];

/// Texts that load modules in a way that the scan does not read: a
/// `require` function made at run time, and an import map that maps a bare
/// name to a project file.
const OPAQUE_MARKERS: &[&str] = &["createRequire", "importmap"];

/// What one keyword of the text gives.
enum Step {
    Specifier(String),
    Opaque,
    Nothing,
}

/// Scan `text` for its import specifiers.
pub(super) fn scan(text: &str) -> Scan {
    if OPAQUE_MARKERS.iter().any(|marker| text.contains(marker)) {
        return Scan::Opaque;
    }
    let bytes = text.as_bytes();
    let mut specifiers = Vec::new();
    let mut index = 0;
    while let Some((start, end)) = next_identifier(bytes, index) {
        index = end;
        match keyword_step(text, start, end) {
            Step::Specifier(value) => specifiers.push(value),
            Step::Opaque => return Scan::Opaque,
            Step::Nothing => {}
        }
    }
    Scan::Specifiers(specifiers)
}

const fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

/// The next identifier at or after `from`, as a byte range.
fn next_identifier(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut start = from;
    while start < bytes.len() {
        let boundary = start == 0 || !is_ident_byte(bytes[start - 1]);
        if boundary && is_ident_byte(bytes[start]) {
            let mut end = start + 1;
            while end < bytes.len() && is_ident_byte(bytes[end]) {
                end += 1;
            }
            return Some((start, end));
        }
        start += 1;
    }
    None
}

fn keyword_step(text: &str, start: usize, end: usize) -> Step {
    let bytes = text.as_bytes();
    let word = &text[start..end];
    let cursor = skip_trivia(bytes, end);
    let next = bytes.get(cursor).copied();
    match word {
        // `Array.from("x")` is a member call, not an import.
        "from" if start == 0 || bytes[start - 1] != b'.' => literal_step(text, cursor),
        "import" => match next {
            Some(b'(') => call_step(text, cursor),
            Some(b'.') => import_meta_step(text, cursor),
            _ => literal_step(text, cursor),
        },
        "require" => match next {
            Some(b'(') => call_step(text, cursor),
            // `require.resolve(...)` and `require.context(...)` name modules
            // without a literal. `const load = require;` makes an alias.
            Some(b'.' | b';' | b',' | b')' | b']' | b'}' | b'=') | None => Step::Opaque,
            _ => Step::Nothing,
        },
        name if LOADER_NAMES.contains(&name) && next == Some(b'(') => call_step(text, cursor),
        _ => Step::Nothing,
    }
}

/// The specifier of `from "x"` or `import "x"`.
fn literal_step(text: &str, cursor: usize) -> Step {
    match literal_at(text, cursor) {
        Some((Literal::Plain(value), _)) => Step::Specifier(value),
        Some((Literal::Computed, _)) => Step::Opaque,
        None => Step::Nothing,
    }
}

/// The specifier of a loader call at the `(` at `cursor`. Each argument
/// that is not one plain literal, such as `"./locale-" + name`, is opaque.
fn call_step(text: &str, cursor: usize) -> Step {
    let bytes = text.as_bytes();
    let argument = skip_trivia(bytes, cursor + 1);
    let Some((Literal::Plain(value), after)) = literal_at(text, argument) else {
        return Step::Opaque;
    };
    match bytes.get(skip_trivia(bytes, after)) {
        Some(b')' | b',') => Step::Specifier(value),
        _ => Step::Opaque,
    }
}

/// `import.meta.glob(...)` loads each module that a pattern matches.
/// `import.meta.url` and `import.meta.env` load none.
fn import_meta_step(text: &str, cursor: usize) -> Step {
    let bytes = text.as_bytes();
    let Some((start, end)) = next_identifier(bytes, skip_trivia(bytes, cursor + 1)) else {
        return Step::Nothing;
    };
    if &text[start..end] != "meta" {
        return Step::Nothing;
    }
    let dot = skip_trivia(bytes, end);
    if bytes.get(dot) != Some(&b'.') {
        return Step::Nothing;
    }
    match next_identifier(bytes, skip_trivia(bytes, dot + 1)) {
        Some((start, end)) if text[start..end].starts_with("glob") => Step::Opaque,
        _ => Step::Nothing,
    }
}

/// Skip whitespace and comments.
fn skip_trivia(bytes: &[u8], mut index: usize) -> usize {
    loop {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let rest = bytes.get(index..).unwrap_or_default();
        if rest.starts_with(b"//") {
            index += rest
                .iter()
                .position(|&byte| byte == b'\n')
                .unwrap_or(rest.len());
        } else if rest.starts_with(b"/*") {
            index += rest
                .windows(2)
                .skip(2)
                .position(|pair| pair == b"*/")
                .map_or(rest.len(), |offset| offset + 4);
        } else {
            return index;
        }
    }
}

enum Literal {
    Plain(String),
    /// A template literal with a substitution.
    Computed,
}

/// The string literal that starts at `index`, and the index after it.
fn literal_at(text: &str, index: usize) -> Option<(Literal, usize)> {
    let quote = *text.as_bytes().get(index)?;
    if !matches!(quote, b'"' | b'\'' | b'`') {
        return None;
    }
    let body_start = index + 1;
    let close = text[body_start..].find(char::from(quote))? + body_start;
    let body = &text[body_start..close];
    if quote == b'`' && body.contains("${") {
        return Some((Literal::Computed, close + 1));
    }
    Some((Literal::Plain(body.to_string()), close + 1))
}

/// How a specifier can reach the modules of the project.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Target {
    /// The specifier cannot name a module of the project: a network URL or
    /// a data URL.
    External,
    /// Each module at this path, with or without a source extension, and
    /// each module below it.
    Path(PathBuf),
    /// Each module whose path ends with this path, for a specifier that
    /// starts with `/`. An HTML page reads it from the root of its server.
    RootRelative(String),
    /// A bare specifier, with its package name.
    Bare { specifier: String, package: String },
    /// The specifier can name each module, for example a `file:` URL.
    Unknown,
}

/// What a specifier from the module at `from` can name.
pub(super) fn target(from: &Path, specifier: &str) -> Target {
    let specifier = specifier
        .split(['?', '#'])
        .next()
        .filter(|head| !head.is_empty())
        .unwrap_or(specifier);
    if specifier.starts_with("file:") {
        return Target::Unknown;
    }
    if specifier.contains("://") || specifier.starts_with("data:") {
        return Target::External;
    }
    if specifier.starts_with('.') {
        let base = from.parent().unwrap_or_else(|| Path::new(""));
        return Target::Path(canonical_prefix(&normalize(&base.join(specifier))));
    }
    if let Some(rooted) = specifier.strip_prefix('/') {
        return Target::RootRelative(rooted.to_string());
    }
    Target::Bare {
        specifier: specifier.to_string(),
        package: package_name(specifier).to_string(),
    }
}

/// The package part of a bare specifier: `@scope/name` or `name`.
fn package_name(specifier: &str) -> &str {
    let mut parts = specifier.splitn(3, '/');
    let first = parts.next().unwrap_or("");
    if first.starts_with('@')
        && let Some(second) = parts.next()
    {
        return &specifier[..first.len() + 1 + second.len()];
    }
    first
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// The path with its longest existing ancestor resolved, so that a
/// specifier through a symlinked directory names the real module path.
fn canonical_prefix(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut current = path;
    loop {
        if let Ok(real) = dunce::canonicalize(current) {
            return rest.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (current.parent(), current.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                current = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn strip_source_extension(path: &str) -> &str {
    path.rsplit_once('.')
        .filter(|(stem, extension)| {
            !stem.is_empty() && !extension.contains('/') && SOURCE_EXTENSIONS.contains(extension)
        })
        .map_or(path, |(stem, _)| stem)
}

fn slash_lower(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// The paths of each module, built once for each run, lower case with `/`
/// separators. Matching ignores case, because a specifier resolves on a
/// file system that ignores case.
pub(super) struct ModuleIndex {
    /// The module, its full path, and its path without the extension.
    entries: Vec<(FileId, String, String)>,
}

impl ModuleIndex {
    pub(super) fn new<'p>(modules: impl Iterator<Item = (FileId, &'p Path)>) -> Self {
        let entries = modules
            .map(|(file, path)| {
                let full = slash_lower(path);
                let key = match full.rsplit_once('.') {
                    Some((stem, extension)) if !stem.is_empty() && !extension.contains('/') => {
                        stem.to_string()
                    }
                    _ => full.clone(),
                };
                (file, full, key)
            })
            .collect();
        Self { entries }
    }

    /// Each module that the path target can name: the path with or without
    /// a source extension, and each module below it, for a directory import.
    pub(super) fn under_path(&self, path: &Path) -> Vec<FileId> {
        let full = slash_lower(path);
        let wanted = [full.as_str(), strip_source_extension(&full)];
        self.select(|module, key| {
            wanted.iter().any(|wanted| {
                key == *wanted
                    || module == *wanted
                    || module
                        .strip_prefix(*wanted)
                        .is_some_and(|rest| rest.starts_with('/'))
            })
        })
    }

    /// Each module whose path ends with `suffix`, with or without a source
    /// extension, or lies below it.
    pub(super) fn with_suffix(&self, suffix: &str) -> Vec<FileId> {
        let full = format!("/{}", suffix.trim_start_matches('/').to_lowercase());
        let wanted = [full.as_str(), strip_source_extension(&full)];
        let below: Vec<String> = wanted.iter().map(|wanted| format!("{wanted}/")).collect();
        self.select(|module, key| {
            wanted
                .iter()
                .any(|wanted| key.ends_with(*wanted) || module.ends_with(*wanted))
                || below.iter().any(|below| module.contains(below.as_str()))
        })
    }

    fn select(&self, keep: impl Fn(&str, &str) -> bool) -> Vec<FileId> {
        self.entries
            .iter()
            .filter(|(_, module, key)| keep(module, key))
            .map(|(file, _, _)| *file)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn specifiers(text: &str) -> Vec<String> {
        match scan(text) {
            Scan::Specifiers(found) => found,
            Scan::Opaque => panic!("unexpected opaque scan of {text:?}"),
        }
    }

    #[test]
    fn finds_static_dynamic_require_and_loader_specifiers() {
        let text = "import a from './a';\nimport { b } from\n  /* why */ \"../b\";\nexport * from './c';\nimport './d.css';\nconst e = require('./e').value;\nconst f = await import(`./f`);\nconst g = jest.requireActual('./g');\nconst h = await vi.importActual('./h', {});\n";
        assert_eq!(
            specifiers(text),
            vec!["./a", "../b", "./c", "./d.css", "./e", "./f", "./g", "./h"]
        );
    }

    #[test]
    fn a_computed_module_name_is_opaque() {
        for text in [
            "const m = require(name);",
            "const m = await import(name);",
            "const m = await import(`./pages/${name}`);",
            "const m = await import('./locale-' + lang);",
            "const m = require('./icon-'.concat(n));",
            "const all = import.meta.glob('./pages/*.ts');",
            "const all = import.meta\n  .glob('./pages/*.ts');",
            "const ctx = require.context('./pages', true);",
            "const load = require;\nload('./a');",
            "const req = createRequire(import.meta.url);",
            "const mod = await importOriginal();",
            "<script type=\"importmap\">{}</script>",
        ] {
            assert_eq!(scan(text), Scan::Opaque, "{text}");
        }
    }

    #[test]
    fn prose_and_other_identifiers_give_no_specifier() {
        let text = "// the value comes from the server, this is important.\nconst chars = Array.from('abc');\nconst url = new URL('./w.js', import.meta.url);\nconst data = importData(source);\nif (requiresAuth(user)) {}\nconst important = 1;\n";
        assert_eq!(specifiers(text), Vec::<String>::new());
    }

    #[test]
    fn classifies_each_specifier_kind() {
        let from = Path::new("/p/src/pages/home.ts");
        assert_eq!(
            target(from, "../lib/format.js"),
            Target::Path(PathBuf::from("/p/src/lib/format.js"))
        );
        assert_eq!(
            target(from, "/src/main.tsx"),
            Target::RootRelative("src/main.tsx".into())
        );
        assert_eq!(
            target(from, "@acme/ui/button"),
            Target::Bare {
                specifier: "@acme/ui/button".into(),
                package: "@acme/ui".into()
            }
        );
        assert_eq!(
            target(from, "https://cdn.example.com/x.js"),
            Target::External
        );
        assert_eq!(target(from, "file:///p/src/a.ts"), Target::Unknown);
    }

    fn index(paths: &[&'static str]) -> ModuleIndex {
        ModuleIndex::new(
            paths
                .iter()
                .enumerate()
                .map(|(id, path)| (FileId(u32::try_from(id).expect("id")), Path::new(*path))),
        )
    }

    #[test]
    fn a_path_matches_with_or_without_extension_in_any_case_and_below() {
        let modules = index(&[
            "/p/src/lib/format.ts",
            "/p/src/lib/formatter.ts",
            "/p/src/counter.svelte.ts",
            "/p/src/user.service.ts",
            "/p/src/Utils.ts",
            "/p/src/lib/index.ts",
            "/p/src/library.ts",
        ]);
        let ids = |path: &str| modules.under_path(Path::new(path));
        assert_eq!(ids("/p/src/lib/format.js"), vec![FileId(0)]);
        assert_eq!(ids("/p/src/counter.svelte"), vec![FileId(2)]);
        assert_eq!(ids("/p/src/user.service"), vec![FileId(3)]);
        assert_eq!(ids("/p/src/utils"), vec![FileId(4)]);
        assert_eq!(ids("/p/src/lib"), vec![FileId(0), FileId(1), FileId(5)]);
        assert_eq!(modules.with_suffix("src/lib/format.ts"), vec![FileId(0)]);
        assert_eq!(modules.with_suffix("events/bus"), Vec::<FileId>::new());
    }
}
