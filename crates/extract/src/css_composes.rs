//! Import edges for CSS Modules `composes` declarations.
//!
//! `composes: base rounded from "./shared.module.css"` makes the bundler load
//! `shared.module.css` and use its `base` and `rounded` classes. The extractor
//! records the declaration as a named import of each class, the same edge as
//! `import { base, rounded } from "./shared.module.css"` in a script. The other
//! file is then reachable, and each named class counts as a used export.
//! `composes: name from global` and `composes: name` without `from` name no
//! file, so they add no import.

use std::sync::LazyLock;

use crate::css::{is_css_url_import, normalize_css_import_path, trimmed_match_with_span};
use crate::{ImportInfo, ImportedName};

/// A `composes` declaration that names another file, with a double-quoted or
/// single-quoted source. The class list cannot hold quotes or block
/// delimiters, so a match stays inside one declaration.
static CSS_COMPOSES_FROM_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"\bcomposes\s*:\s*([^;{}"']+?)\s+from\s+["']([^"']+)["']"#)
});

/// A CSS Modules class name in a `composes` list.
static CSS_COMPOSES_CLASS_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r"^-?[a-zA-Z_][a-zA-Z0-9_-]*$"));

/// Add a named import for each class of each `composes ... from "<file>"`
/// declaration in `stripped`, a CSS Modules source with its comments masked.
pub fn push_composes_from_imports(stripped: &str, imports: &mut Vec<ImportInfo>) {
    for cap in CSS_COMPOSES_FROM_RE.captures_iter(stripped) {
        let (Some(classes), Some(source_match)) = (cap.get(1), cap.get(2)) else {
            continue;
        };
        let (source, source_span) = trimmed_match_with_span(source_match);
        if source.is_empty() || is_css_url_import(&source) {
            continue;
        }
        let source = normalize_css_import_path(source, false);
        for class in classes
            .as_str()
            .split_whitespace()
            .filter(|class| CSS_COMPOSES_CLASS_RE.is_match(class))
        {
            imports.push(ImportInfo {
                source: source.clone(),
                imported_name: ImportedName::Named(class.to_string()),
                local_name: class.to_string(),
                is_type_only: false,
                is_type_only_star: false,
                from_style: false,
                span: source_span,
                source_span,
            });
        }
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use std::path::Path;

    use fallow_types::discover::FileId;

    use crate::ImportedName;
    use crate::css::parse_css_to_module;

    fn composes_imports(path: &str, source: &str) -> Vec<(String, String)> {
        parse_css_to_module(FileId(0), Path::new(path), source, 0)
            .imports
            .into_iter()
            .filter_map(|import| match import.imported_name {
                ImportedName::Named(name) => Some((import.source, name)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn composes_from_file_imports_each_named_class() {
        let source = ".a { composes: base rounded from \"./shared.module.css\"; }\n\
                      .b { composes: other from './other.module.css' }";
        assert_eq!(
            composes_imports("button.module.css", source),
            vec![
                ("./shared.module.css".to_string(), "base".to_string()),
                ("./shared.module.css".to_string(), "rounded".to_string()),
                ("./other.module.css".to_string(), "other".to_string()),
            ]
        );
    }

    #[test]
    fn composes_from_import_spans_the_source_specifier() {
        let source = ".a { composes: base from \"./shared.module.css\"; }";
        let info = parse_css_to_module(FileId(0), Path::new("a.module.css"), source, 0);
        let import = &info.imports[0];
        assert_eq!(
            &source[import.source_span.start as usize..import.source_span.end as usize],
            "./shared.module.css"
        );
    }

    #[test]
    fn composes_without_file_adds_no_import() {
        let source = ".base { color: red; }\n\
                      .a { composes: base; composes: g from global; }";
        assert!(composes_imports("a.module.css", source).is_empty());
    }

    #[test]
    fn composes_in_comment_or_plain_css_adds_no_import() {
        let commented = "/* .a { composes: base from \"./x.module.css\"; } */";
        assert!(composes_imports("a.module.css", commented).is_empty());
        let plain = ".a { composes: base from \"./x.module.css\"; }";
        assert!(composes_imports("a.css", plain).is_empty());
    }

    #[test]
    fn composes_from_scss_module_file() {
        let source = "// note\n.a { composes: base from \"./shared.module.scss\"; }";
        assert_eq!(
            composes_imports("a.module.scss", source),
            vec![("./shared.module.scss".to_string(), "base".to_string())]
        );
    }
}
