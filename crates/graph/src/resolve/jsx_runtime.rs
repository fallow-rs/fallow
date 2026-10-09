//! Graph edges for a JSX import source that a bundler or test config sets.
//!
//! With the automatic JSX runtime, the transform adds an import of
//! `<source>/jsx-dev-runtime` to each file with JSX. Vitest runs the
//! transform in dev mode, so the dev runtime is the module that the test
//! files import. No import statement shows that edge, so this pass adds it
//! from the plugin rules.

use std::path::{Component, Path, PathBuf};

use fallow_config::JsxImportSourceRule;
use fallow_types::extract::{ImportInfo, ImportedName, ModuleInfo};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use oxc_span::Span;
use rustc_hash::FxHashSet;

use super::specifier::resolve_import_specifier;
use super::types::{ResolveContext, ResolveResult, ResolvedImport};

/// The runtime module that the automatic JSX transform imports in dev mode.
const JSX_DEV_RUNTIME_SUBPATH: &str = "jsx-dev-runtime";

/// The bindings that the automatic JSX transform imports from
/// `<source>/jsx-dev-runtime`.
const JSX_DEV_RUNTIME_BINDINGS: [&str; 2] = ["jsxDEV", "Fragment"];

/// One config JSX import source with its compiled include globs.
pub(super) struct CompiledJsxRule<'a> {
    rule: &'a JsxImportSourceRule,
    include: GlobSet,
}

/// Compile the include globs of each rule. A rule without a valid glob
/// matches no file, so it is dropped.
pub(super) fn compile_jsx_rules(rules: &[JsxImportSourceRule]) -> Vec<CompiledJsxRule<'_>> {
    rules
        .iter()
        .filter_map(|rule| {
            let mut builder = GlobSetBuilder::new();
            let mut added = false;
            for pattern in &rule.include {
                if let Ok(glob) = GlobBuilder::new(pattern).literal_separator(true).build() {
                    builder.add(glob);
                    added = true;
                }
            }
            let include = builder.build().ok().filter(|_| added)?;
            Some(CompiledJsxRule { rule, include })
        })
        .collect()
}

/// Resolve the config JSX runtime imports of one module.
///
/// A module gets an edge only when extraction set `jsx_runtime_from_config`
/// (JSX and no runtime pragma) and its path relative to the config directory
/// matches the include globs of a rule. A relative source that does not
/// resolve from the module resolves from the config directory, as Vite does.
/// A source that resolves from neither place adds no edge, so a config value
/// never causes an unresolved-import finding.
pub(super) fn resolve_config_jsx_runtime_imports(
    ctx: &ResolveContext<'_>,
    file_path: &Path,
    module: &ModuleInfo,
    rules: &[CompiledJsxRule<'_>],
) -> Vec<ResolvedImport> {
    if !module.jsx_runtime_from_config || rules.is_empty() {
        return Vec::new();
    }
    let mut seen: FxHashSet<&str> = FxHashSet::default();
    let mut imports = Vec::new();
    for compiled in rules {
        let rule = compiled.rule;
        let Ok(relative) = file_path.strip_prefix(&rule.config_dir) else {
            continue;
        };
        if !compiled.include.is_match(relative) || !seen.insert(rule.source.as_str()) {
            continue;
        }
        let specifier = jsx_dev_runtime_specifier(&rule.source);
        let Some(target) = resolve_runtime(ctx, file_path, rule, &specifier) else {
            continue;
        };
        imports.extend(
            JSX_DEV_RUNTIME_BINDINGS
                .iter()
                .map(|binding| ResolvedImport {
                    info: ImportInfo {
                        source: specifier.clone(),
                        imported_name: ImportedName::Named((*binding).to_string()),
                        local_name: String::new(),
                        is_type_only: false,
                        is_type_only_star: false,
                        from_style: false,
                        span: Span::default(),
                        source_span: Span::default(),
                    },
                    target: target.clone(),
                }),
        );
    }
    imports
}

fn resolve_runtime(
    ctx: &ResolveContext<'_>,
    file_path: &Path,
    rule: &JsxImportSourceRule,
    specifier: &str,
) -> Option<ResolveResult> {
    let from_file = resolve_import_specifier(ctx, file_path, specifier, false);
    if !matches!(from_file, ResolveResult::Unresolvable(_)) {
        return Some(from_file);
    }
    if !is_relative(&rule.source) {
        return None;
    }
    // The resolver reads a leading `/` as root-relative, so the fallback
    // spells the config-relative target relative to the module directory.
    let from_config = config_relative_specifier(file_path.parent()?, &rule.config_dir, specifier)?;
    match resolve_import_specifier(ctx, file_path, &from_config, false) {
        ResolveResult::Unresolvable(_) => None,
        target => Some(target),
    }
}

fn is_relative(source: &str) -> bool {
    matches!(source, "." | "..") || source.starts_with("./") || source.starts_with("../")
}

/// The specifier that the automatic JSX transform imports in dev mode:
/// `<source>/jsx-dev-runtime`, with one separator.
fn jsx_dev_runtime_specifier(import_source: &str) -> String {
    let base = import_source.trim_end_matches('/');
    if base.is_empty() {
        return format!("/{JSX_DEV_RUNTIME_SUBPATH}");
    }
    format!("{base}/{JSX_DEV_RUNTIME_SUBPATH}")
}

/// Spell `specifier`, relative to `config_dir`, as a specifier relative to
/// `file_dir`. `None` when the two directories share no root, for example on
/// two Windows drives.
fn config_relative_specifier(
    file_dir: &Path,
    config_dir: &Path,
    specifier: &str,
) -> Option<String> {
    let target = lexical_join(config_dir, specifier);
    let from: Vec<Component<'_>> = file_dir.components().collect();
    let to: Vec<Component<'_>> = target.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    if parts.is_empty() {
        parts.push(".".to_string());
    }
    for component in &to[common..] {
        parts.push(component.as_os_str().to_str()?.to_string());
    }
    Some(parts.join("/"))
}

/// Join a relative specifier to a directory and remove `.` and `..`
/// segments.
fn lexical_join(dir: &Path, relative: &str) -> PathBuf {
    let mut joined = dir.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                joined.pop();
            }
            other => joined.push(other.as_os_str()),
        }
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_runtime_specifier_has_one_separator() {
        assert_eq!(
            jsx_dev_runtime_specifier("preact"),
            "preact/jsx-dev-runtime"
        );
        assert_eq!(
            jsx_dev_runtime_specifier("./src/jsx/"),
            "./src/jsx/jsx-dev-runtime"
        );
        assert_eq!(jsx_dev_runtime_specifier("."), "./jsx-dev-runtime");
    }

    #[test]
    fn lexical_join_removes_dot_segments() {
        assert_eq!(
            lexical_join(Path::new("/project/app"), "../src/./jsx/jsx-dev-runtime"),
            PathBuf::from("/project/src/jsx/jsx-dev-runtime")
        );
    }

    #[test]
    fn config_relative_specifier_starts_from_the_module_directory() {
        assert_eq!(
            config_relative_specifier(
                Path::new("/project/src/deep"),
                Path::new("/project"),
                "./src/jsx/jsx-dev-runtime"
            )
            .as_deref(),
            Some("../jsx/jsx-dev-runtime")
        );
        assert_eq!(
            config_relative_specifier(Path::new("/project"), Path::new("/project"), "./jsx/x")
                .as_deref(),
            Some("./jsx/x")
        );
    }

    #[test]
    fn relative_sources() {
        assert!(is_relative("./src/jsx"));
        assert!(is_relative("../jsx"));
        assert!(is_relative("."));
        assert!(!is_relative("preact"));
        assert!(!is_relative("@emotion/react"));
    }
}
