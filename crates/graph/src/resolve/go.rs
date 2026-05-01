//! Go import resolution.
//!
//! Resolves Go import paths to [`ResolveResult`]s following Go module semantics:
//!
//! - **Stdlib** — any import path whose first segment contains no dot (e.g. `"fmt"`,
//!   `"net/http"`, `"encoding/json"`) → `NpmPackage("go:<path>")`. The `go:` prefix
//!   makes stdlib packages distinguishable from npm packages in the unused-deps output.
//!
//! - **Internal** — import paths that start with the module's own path (from
//!   `go.mod`) → `GoPackage(file_ids)` where `file_ids` are all `.go` files in
//!   the resolved directory.
//!
//! - **External** — everything else → `NpmPackage("go:<path>")`, where `<path>`
//!   is the full module path (e.g. `"github.com/some/dep"`).  Package-level
//!   granularity (stripping the sub-package path) is applied so that all imports
//!   from the same Go module credit the same dep entry.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use fallow_config::{GoMod, GoWork};
use fallow_types::discover::FileId;
use fallow_types::extract::{ImportInfo, ImportedName};

use super::types::{ResolveResult, ResolvedImport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GoModule {
    pub module_path: String,
    pub root: PathBuf,
}

/// Context needed to resolve imports in a single Go file.
pub(super) struct GoResolveContext<'a> {
    /// Modules available in the current Go workspace, ordered by most-specific
    /// module path first so nested modules win.
    pub modules: &'a [GoModule],
    /// Maps absolute directory path → sorted list of `FileId`s for `.go` files
    /// in that directory. Built once before parallel resolution begins.
    pub dir_to_go_files: &'a FxHashMap<PathBuf, Vec<FileId>>,
}

/// Resolve all imports from a single Go source file.
pub(super) fn resolve_go_imports(
    ctx: &GoResolveContext<'_>,
    file_path: &Path,
    imports: &[ImportInfo],
) -> Vec<ResolvedImport> {
    imports
        .iter()
        .map(|info| ResolvedImport {
            info: info.clone(),
            target: resolve_go_import(ctx, file_path, &info.source, &info.imported_name),
        })
        .collect()
}

fn resolve_go_import(
    ctx: &GoResolveContext<'_>,
    _file_path: &Path,
    import_path: &str,
    imported_name: &ImportedName,
) -> ResolveResult {
    // Side-effect imports (`import _ "pkg"`) — record the package dep but don't
    // create symbol edges. We still need NpmPackage for dep-usage tracking.
    let _ = imported_name; // same resolution regardless of alias form

    // 1. Stdlib: first path segment has no dot.
    if is_go_stdlib(import_path) {
        return ResolveResult::NpmPackage(format!("go:{import_path}"));
    }

    // 2. Internal package: import path starts with the module's own module path.
    if let Some(go_module) = find_internal_module(ctx.modules, import_path) {
        let suffix = &import_path[go_module.module_path.len()..];
        let sub = suffix.trim_start_matches('/');
        let pkg_dir = if sub.is_empty() {
            go_module.root.clone()
        } else {
            go_module.root.join(sub)
        };

        if let Some(file_ids) = ctx.dir_to_go_files.get(&pkg_dir)
            && !file_ids.is_empty()
        {
            return ResolveResult::GoPackage(file_ids.clone());
        }
    }

    // 3. External package: if this import belongs to another module in the same
    // go.work workspace but that module has no scanned files, still preserve the
    // full workspace module path as the dependency key.
    if let Some(go_module) = find_internal_module(ctx.modules, import_path) {
        return ResolveResult::NpmPackage(format!("go:{}", go_module.module_path));
    }

    // 4. External package: use the top-level module path as the dep key.
    //    `github.com/foo/bar/sub` → dep key `github.com/foo/bar` (first 3 segments for
    //    domain-based paths, or the full path for single-segment vanity imports).
    let dep_key = extract_go_module_key(import_path);
    ResolveResult::NpmPackage(format!("go:{dep_key}"))
}

fn find_internal_module<'a>(modules: &'a [GoModule], import_path: &str) -> Option<&'a GoModule> {
    modules.iter().find(|go_module| {
        import_path == go_module.module_path
            || import_path
                .strip_prefix(go_module.module_path.as_str())
                .is_some_and(|suffix| suffix.starts_with('/'))
    })
}

pub(super) fn discover_go_modules(root: &Path) -> Vec<GoModule> {
    let mut modules = Vec::new();

    if let Some(go_mod) = GoMod::from_dir(root)
        && !go_mod.module_path.is_empty()
    {
        modules.push(GoModule {
            module_path: go_mod.module_path,
            root: root.to_path_buf(),
        });
    }

    if let Some(go_work) = GoWork::from_dir(root) {
        for rel in go_work.uses {
            let module_root = root.join(rel);
            let Some(go_mod) = GoMod::from_dir(&module_root) else {
                continue;
            };
            if go_mod.module_path.is_empty() {
                continue;
            }
            let module = GoModule {
                module_path: go_mod.module_path,
                root: module_root,
            };
            if !modules.iter().any(|existing| existing.root == module.root) {
                modules.push(module);
            }
        }
    }

    modules.sort_by(|a, b| {
        b.module_path
            .len()
            .cmp(&a.module_path.len())
            .then_with(|| a.root.cmp(&b.root))
    });
    modules
}

/// Return `true` if `import_path` is a Go standard library package.
///
/// Go stdlib packages have no dot (`.`) in their first path segment.
/// All external packages use domain names like `github.com/...` or `gopkg.in/...`.
pub fn is_go_stdlib(import_path: &str) -> bool {
    let first_seg = import_path.split('/').next().unwrap_or(import_path);
    !first_seg.contains('.')
}

/// Extract the top-level module key from an external Go import path.
///
/// For domain-based paths (which have a dot in the first segment), the module
/// key is the first three slash-separated segments (domain/owner/repo), which
/// is the standard Go module naming convention.
///
/// Examples:
/// - `"github.com/foo/bar/sub/pkg"` → `"github.com/foo/bar"`
/// - `"gopkg.in/yaml.v3"` → `"gopkg.in/yaml.v3"` (two segments for gopkg.in)
/// - `"example.com/mylib"` → `"example.com/mylib"`
fn extract_go_module_key(import_path: &str) -> &str {
    match import_path.splitn(4, '/').count() {
        0..=2 => import_path,
        // github.com / owner / repo / sub … → take first 3 segments
        _ => {
            // Find the byte offset of the 3rd slash (if any).
            let mut slash_count = 0;
            for (i, b) in import_path.bytes().enumerate() {
                if b == b'/' {
                    slash_count += 1;
                    if slash_count == 3 {
                        return &import_path[..i];
                    }
                }
            }
            import_path
        }
    }
}

/// Build the directory → sorted `FileId` list index for all `.go` files in the
/// discovered file set. Called once during resolver setup.
pub(super) fn build_go_dir_index(
    files: &[fallow_types::discover::DiscoveredFile],
) -> FxHashMap<PathBuf, Vec<FileId>> {
    let mut map: FxHashMap<PathBuf, Vec<FileId>> = FxHashMap::default();
    for file in files {
        if file
            .path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|ext| ext == "go")
            && let Some(dir) = file.path.parent()
        {
            map.entry(dir.to_path_buf()).or_default().push(file.id);
        }
    }
    // Sort each list for deterministic edge ordering.
    for ids in map.values_mut() {
        ids.sort_unstable_by_key(|id| id.0);
    }
    map
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdlib_no_dot_first_segment() {
        assert!(is_go_stdlib("fmt"));
        assert!(is_go_stdlib("net/http"));
        assert!(is_go_stdlib("encoding/json"));
        assert!(is_go_stdlib("iter"));
        assert!(is_go_stdlib("unique"));
        assert!(is_go_stdlib("testing"));
        assert!(is_go_stdlib("math/rand/v2"));
    }

    #[test]
    fn external_has_dot_first_segment() {
        assert!(!is_go_stdlib("github.com/foo/bar"));
        assert!(!is_go_stdlib("gopkg.in/yaml.v3"));
        assert!(!is_go_stdlib("golang.org/x/net"));
        assert!(!is_go_stdlib("k8s.io/client-go"));
    }

    #[test]
    fn module_key_three_segments() {
        assert_eq!(
            extract_go_module_key("github.com/foo/bar/sub/pkg"),
            "github.com/foo/bar"
        );
    }

    #[test]
    fn module_key_two_segments() {
        assert_eq!(
            extract_go_module_key("gopkg.in/yaml.v3"),
            "gopkg.in/yaml.v3"
        );
    }

    #[test]
    fn module_key_exactly_three() {
        assert_eq!(
            extract_go_module_key("github.com/foo/bar"),
            "github.com/foo/bar"
        );
    }

    #[test]
    fn module_key_single() {
        assert_eq!(extract_go_module_key("example.com"), "example.com");
    }

    #[test]
    fn resolves_stdlib_to_npm_package() {
        let ctx = GoResolveContext {
            modules: &[GoModule {
                module_path: "github.com/myorg/myproject".to_string(),
                root: PathBuf::from("/project"),
            }],
            dir_to_go_files: &FxHashMap::default(),
        };
        let info = make_import("fmt");
        let result = resolve_go_import(&ctx, Path::new("/project/main.go"), "fmt", &info);
        assert!(matches!(result, ResolveResult::NpmPackage(s) if s == "go:fmt"));
    }

    #[test]
    fn resolves_external_to_npm_package() {
        let ctx = GoResolveContext {
            modules: &[GoModule {
                module_path: "github.com/myorg/myproject".to_string(),
                root: PathBuf::from("/project"),
            }],
            dir_to_go_files: &FxHashMap::default(),
        };
        let info = make_import("github.com/some/dep");
        let result = resolve_go_import(
            &ctx,
            Path::new("/project/main.go"),
            "github.com/some/dep",
            &info,
        );
        assert!(matches!(result, ResolveResult::NpmPackage(s) if s == "go:github.com/some/dep"));
    }

    fn make_import(_path: &str) -> ImportedName {
        ImportedName::Default
    }

    #[test]
    fn internal_module_prefix_requires_path_boundary() {
        let ctx = GoResolveContext {
            modules: &[GoModule {
                module_path: "github.com/myorg/proj".to_string(),
                root: PathBuf::from("/project"),
            }],
            dir_to_go_files: &FxHashMap::default(),
        };
        let result = resolve_go_import(
            &ctx,
            Path::new("/project/main.go"),
            "github.com/myorg/proj2/pkg",
            &ImportedName::Default,
        );
        assert!(matches!(
            result,
            ResolveResult::NpmPackage(ref s) if s == "go:github.com/myorg/proj2"
        ));
    }

    #[test]
    fn workspace_nested_module_wins_longest_prefix_match() {
        let root_module = GoModule {
            module_path: "github.com/acme/root".to_string(),
            root: PathBuf::from("/project"),
        };
        let nested_module = GoModule {
            module_path: "github.com/acme/root/tools".to_string(),
            root: PathBuf::from("/project/tools"),
        };
        let dir_index =
            FxHashMap::from_iter([(PathBuf::from("/project/tools/pkg"), vec![FileId(7)])]);
        let modules = vec![nested_module, root_module];
        let ctx = GoResolveContext {
            modules: &modules,
            dir_to_go_files: &dir_index,
        };

        let result = resolve_go_import(
            &ctx,
            Path::new("/project/main.go"),
            "github.com/acme/root/tools/pkg",
            &ImportedName::Namespace,
        );
        assert!(matches!(result, ResolveResult::GoPackage(ids) if ids == vec![FileId(7)]));
    }
}
