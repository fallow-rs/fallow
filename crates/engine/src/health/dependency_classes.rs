//! CSS classes that a declared dependency or framework theme renders.
//!
//! A library or a theme puts its own classes on the DOM that it renders
//! (ProseMirror decorations, a map popup, the Docusaurus sidebar). Project
//! markup never writes these classes, so `unreferenced-css-class` must not
//! report a project override of them.
//!
//! Two checks credit such a class. Both use the dependencies of the root
//! `package.json` and of each workspace `package.json` whose folder holds the
//! stylesheet:
//!
//! - the class starts with the normalized name of a declared package
//!   (`maplibre-gl` covers `maplibregl-popup`);
//! - the class matches the known class list of a declared package
//!   ([`RUNTIME_CLASS_SOURCES`]).

use std::path::Path;

use fallow_config::ResolvedConfig;
use rustc_hash::FxHashSet;

/// Shortest normalized package name that counts as a class prefix. A shorter
/// name (`ms`, `vue`) matches too many project classes.
const MIN_DEP_PREFIX_LEN: usize = 6;

/// The classes that one library or theme renders at runtime.
struct RuntimeClassSource {
    /// Package names that bring the classes. A name that ends in `/` matches
    /// every package of that scope.
    packages: &'static [&'static str],
    /// Exact class names.
    classes: &'static [&'static str],
    /// Class name prefixes.
    prefixes: &'static [&'static str],
}

/// Known class lists of libraries and themes whose classes do not start with
/// the package name.
const RUNTIME_CLASS_SOURCES: &[RuntimeClassSource] = &[
    // The ProseMirror view: `ProseMirror`, `ProseMirror-selectednode`,
    // `ProseMirror-gapcursor`, `ProseMirror-widget`.
    RuntimeClassSource {
        packages: &[
            "prosemirror-view",
            "@tiptap/pm",
            "@tiptap/core",
            "@tiptap/react",
            "@tiptap/vue-2",
            "@tiptap/vue-3",
            "@tiptap/starter-kit",
        ],
        classes: &[],
        prefixes: &["ProseMirror"],
    },
    // The decorations and the wrapper of `prosemirror-tables`.
    RuntimeClassSource {
        packages: &[
            "prosemirror-tables",
            "@tiptap/pm",
            "@tiptap/extension-table",
        ],
        classes: &[
            "selectedCell",
            "column-resize-handle",
            "resize-cursor",
            "tableWrapper",
        ],
        prefixes: &[],
    },
    // The default classes of the Tiptap placeholder extension.
    RuntimeClassSource {
        packages: &["@tiptap/extension-placeholder", "@tiptap/extensions"],
        classes: &["is-empty", "is-editor-empty"],
        prefixes: &[],
    },
    // The Tiptap React node view wrapper: `node-<node type name>`.
    RuntimeClassSource {
        packages: &["@tiptap/react"],
        classes: &["react-renderer"],
        prefixes: &["node-"],
    },
    // The progress bar element of BProgress.
    RuntimeClassSource {
        packages: &["@bprogress/"],
        classes: &[],
        prefixes: &["bprogress"],
    },
    // The Docusaurus classic theme and its Infima CSS framework.
    RuntimeClassSource {
        packages: &["@docusaurus/preset-classic", "@docusaurus/theme-classic"],
        classes: &[
            "main-wrapper",
            "markdown",
            "container",
            "row",
            "col",
            "clean-btn",
            "clean-list",
            "alert",
            "avatar",
            "badge",
            "breadcrumbs",
            "button",
            "button-group",
            "card",
            "dropdown",
            "footer",
            "hero",
            "menu",
            "navbar",
            "navbar-sidebar",
            "pagination",
            "pagination-nav",
            "pills",
            "table-of-contents",
            "tabs",
        ],
        prefixes: &[
            "theme-",
            "docusaurus-",
            "alert--",
            "avatar__",
            "avatar--",
            "badge--",
            "breadcrumbs__",
            "breadcrumbs--",
            "button--",
            "card__",
            "card--",
            "col--",
            "dropdown__",
            "dropdown--",
            "footer__",
            "footer--",
            "hero__",
            "hero--",
            "menu__",
            "menu--",
            "navbar__",
            "navbar--",
            "navbar-sidebar__",
            "navbar-sidebar--",
            "pagination__",
            "pagination-nav__",
            "pills__",
            "table-of-contents__",
            "tabs__",
            "tabs--",
            "text--",
            "margin-",
            "padding-",
        ],
    },
];

/// The declared dependencies of one package folder.
struct DependencyScope {
    /// Project-root-relative folder with `/` separators, empty for the root.
    folder: String,
    name_prefixes: FxHashSet<String>,
    sources: Vec<&'static RuntimeClassSource>,
}

impl DependencyScope {
    fn covers_by_name(&self, normalized_class: &str) -> bool {
        self.name_prefixes
            .iter()
            .any(|prefix| normalized_class.starts_with(prefix.as_str()))
    }

    fn covers_by_list(&self, class: &str) -> bool {
        self.sources.iter().any(|source| {
            source.classes.contains(&class)
                || source
                    .prefixes
                    .iter()
                    .any(|prefix| class.starts_with(prefix))
        })
    }

    fn contains(&self, rel: &str) -> bool {
        self.folder.is_empty()
            || rel
                .strip_prefix(self.folder.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// The dependency class evidence of the root package and every workspace.
#[derive(Default)]
pub(super) struct DependencyClasses {
    scopes: Vec<DependencyScope>,
}

impl DependencyClasses {
    /// Read the root `package.json` and the `package.json` of each workspace.
    pub(super) fn load(config: &ResolvedConfig) -> Self {
        let mut scopes = Vec::new();
        scopes.extend(DependencyScope::read(&config.root, String::new()));
        let workspaces = fallow_config::discover_workspaces_with_patterns(
            &config.root,
            &config.workspace_patterns,
        );
        for workspace in workspaces {
            let Ok(folder) = workspace.root.strip_prefix(&config.root) else {
                continue;
            };
            let folder = folder.to_string_lossy().replace('\\', "/");
            if folder.is_empty() {
                continue;
            }
            scopes.extend(DependencyScope::read(&workspace.root, folder));
        }
        Self { scopes }
    }

    /// True when a dependency that the stylesheet at `rel` can see renders
    /// `class`.
    ///
    /// The known class lists do not apply to a CSS module (`*.module.css`):
    /// the bundler renames each module class, so library markup cannot carry
    /// it, and a module class with a library name (`.card`, `.hero`) is the
    /// project's own class.
    pub(super) fn covers(&self, rel: &str, class: &str) -> bool {
        let normalized = normalize_dep_token(class);
        let module = is_css_module(rel);
        self.scopes.iter().any(|scope| {
            scope.contains(rel)
                && (scope.covers_by_name(&normalized) || (!module && scope.covers_by_list(class)))
        })
    }
}

impl DependencyScope {
    fn read(dir: &Path, folder: String) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
        let json = serde_json::from_str::<serde_json::Value>(&text).ok()?;
        let mut names: Vec<&str> = Vec::new();
        for key in ["dependencies", "devDependencies", "peerDependencies"] {
            if let Some(deps) = json.get(key).and_then(serde_json::Value::as_object) {
                names.extend(deps.keys().map(String::as_str));
            }
        }
        let name_prefixes = names
            .iter()
            .map(|name| normalize_dep_token(name.rsplit('/').next().unwrap_or(name)))
            .filter(|normalized| normalized.len() >= MIN_DEP_PREFIX_LEN)
            .collect();
        let sources = RUNTIME_CLASS_SOURCES
            .iter()
            .filter(|source| {
                names.iter().any(|name| {
                    source.packages.iter().any(|package| {
                        if package.ends_with('/') {
                            name.starts_with(package)
                        } else {
                            name == package
                        }
                    })
                })
            })
            .collect();
        Some(Self {
            folder,
            name_prefixes,
            sources,
        })
    }
}

fn is_css_module(rel: &str) -> bool {
    rel.rsplit('/')
        .next()
        .is_some_and(|name| name.contains(".module."))
}

fn normalize_dep_token(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(folder: &str, deps: &[&str]) -> DependencyScope {
        let dir = tempfile::tempdir().expect("temp dir");
        let deps: serde_json::Map<String, serde_json::Value> = deps
            .iter()
            .map(|name| ((*name).to_owned(), serde_json::Value::from("1.0.0")))
            .collect();
        std::fs::write(
            dir.path().join("package.json"),
            serde_json::json!({ "dependencies": deps }).to_string(),
        )
        .expect("write package.json");
        DependencyScope::read(dir.path(), folder.to_owned()).expect("scope")
    }

    fn covers(scope: &DependencyScope, class: &str) -> bool {
        scope.covers_by_name(&normalize_dep_token(class)) || scope.covers_by_list(class)
    }

    #[test]
    fn css_modules_get_no_known_list_credit() {
        let docs = scope("docs", &["@docusaurus/preset-classic", "maplibre-gl"]);
        let classes = DependencyClasses { scopes: vec![docs] };
        assert!(classes.covers("docs/src/css/custom.css", "hero"));
        assert!(!classes.covers("docs/src/pages/index.module.css", "hero"));
        assert!(classes.covers("docs/src/pages/index.module.css", "maplibregl-popup"));
    }

    #[test]
    fn package_name_prefix_and_known_lists_cover_classes() {
        let editor = scope(
            "packages/editor",
            &[
                "@tiptap/pm",
                "@tiptap/react",
                "maplibre-gl",
                "@bprogress/core",
            ],
        );
        for class in [
            "ProseMirror-selectednode",
            "selectedCell",
            "node-image",
            "maplibregl-popup",
            "bprogress",
        ] {
            assert!(covers(&editor, class), "{class}");
        }
        for class in ["menu__link", "is-editor-empty", "editor-shell"] {
            assert!(!covers(&editor, class), "{class}");
        }
    }

    #[test]
    fn a_workspace_scope_covers_only_its_own_folder() {
        let docs = scope("docs", &["@docusaurus/preset-classic"]);
        let classes = DependencyClasses { scopes: vec![docs] };
        assert!(classes.covers("docs/src/css/custom.css", "menu__link"));
        assert!(classes.covers("docs/src/css/custom.css", "theme-doc-markdown"));
        assert!(!classes.covers("docs-site/app.css", "menu__link"));
        assert!(!classes.covers("web/src/app.css", "menu__link"));
    }

    #[test]
    fn short_package_names_are_not_prefixes() {
        let root = scope("", &["vue", "ms"]);
        assert!(!covers(&root, "vue-card"));
        assert!(!covers(&root, "msg-box"));
    }
}
