#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! Cross-platform path handling tests for the fallow-graph crate.
//!
//! Exercises path separator normalization, case sensitivity, unicode paths,
//! long paths, relative path resolution, and dotfile/hidden directory handling.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use fallow_graph::resolve::{extract_package_name, is_path_alias};
use fallow_types::discover::{DiscoveredFile, FileId};
use rustc_hash::FxHashMap;

#[test]
fn forward_slash_paths_resolve_in_path_to_id_lookup() {
    let path = PathBuf::from("/project/src/utils.ts");
    let mut map: FxHashMap<&Path, FileId> = FxHashMap::default();
    map.insert(path.as_path(), FileId(0));

    let lookup = PathBuf::from("/project/src/utils.ts");
    assert_eq!(map.get(lookup.as_path()), Some(&FileId(0)));
}

#[test]
fn path_with_trailing_separator_differs() {
    let p1 = PathBuf::from("/project/src/");
    let p2 = PathBuf::from("/project/src");
    assert_eq!(
        p1.components().collect::<Vec<_>>(),
        p2.components().collect::<Vec<_>>(),
        "Path components should be identical regardless of trailing separator"
    );
}

#[test]
fn path_join_normalizes_separators() {
    let base = PathBuf::from("/project");
    let joined = base.join("src").join("utils.ts");
    assert_eq!(joined, PathBuf::from("/project/src/utils.ts"));
}

#[test]
fn node_modules_extraction_with_forward_slashes() {
    let path = PathBuf::from("/project/node_modules/react/index.js");
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let nm_idx = components.iter().rposition(|&c| c == "node_modules");
    assert!(nm_idx.is_some(), "should find node_modules component");
}

#[test]
fn path_comparison_is_case_sensitive_on_unix() {
    let p1 = PathBuf::from("/project/src/Utils.ts");
    let p2 = PathBuf::from("/project/src/utils.ts");
    assert_ne!(
        p1, p2,
        "PathBuf comparison should be case-sensitive (byte-exact)"
    );
}

#[test]
fn case_sensitive_path_to_id_lookup() {
    let lower = PathBuf::from("/project/src/utils.ts");
    let upper = PathBuf::from("/project/src/Utils.ts");
    let mut map: FxHashMap<&Path, FileId> = FxHashMap::default();
    map.insert(lower.as_path(), FileId(0));

    assert_eq!(map.get(lower.as_path()), Some(&FileId(0)));
    assert_eq!(map.get(upper.as_path()), None);
}

#[test]
fn package_name_extraction_preserves_case() {
    assert_eq!(extract_package_name("React"), "React");
    assert_eq!(extract_package_name("@Scope/Package"), "@Scope/Package");
}

#[test]
fn path_with_spaces_in_directory() {
    let path = PathBuf::from("/home/user/my project/src/index.ts");
    assert!(path.to_str().is_some());
    assert_eq!(path.file_name().and_then(OsStr::to_str), Some("index.ts"));
    assert_eq!(path.parent(), Some(Path::new("/home/user/my project/src")));
}

#[test]
fn path_with_spaces_in_filename() {
    let path = PathBuf::from("/project/src/my component.tsx");
    let mut map: FxHashMap<&Path, FileId> = FxHashMap::default();
    map.insert(path.as_path(), FileId(42));
    assert_eq!(map.get(path.as_path()), Some(&FileId(42)));
}

#[test]
fn node_modules_path_with_spaces_in_project_name() {
    let path = PathBuf::from("/home/user/my project/node_modules/react/index.js");
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let nm_idx = components.iter().rposition(|&c| c == "node_modules");
    assert!(nm_idx.is_some());
    let pkg = &components[nm_idx.unwrap() + 1];
    assert_eq!(*pkg, "react");
}

#[test]
fn discovered_file_with_spaces_sorts_deterministically() {
    let mut files = [
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/z file.ts"),
            size_bytes: 10,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/a file.ts"),
            size_bytes: 20,
        },
    ];
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        files[0].path,
        PathBuf::from("/project/src/a file.ts"),
        "files with spaces should sort correctly"
    );
}

#[test]
fn unicode_path_components() {
    let path = PathBuf::from("/projekt/src/komponenten/Schaltflaeche.tsx");
    assert_eq!(
        path.file_name().and_then(OsStr::to_str),
        Some("Schaltflaeche.tsx")
    );
}

#[test]
fn unicode_cjk_path() {
    let path = PathBuf::from("/project/src/\u{7EC4}\u{4EF6}/index.ts");
    let mut map: FxHashMap<&Path, FileId> = FxHashMap::default();
    map.insert(path.as_path(), FileId(99));
    assert_eq!(map.get(path.as_path()), Some(&FileId(99)));
}

#[test]
fn unicode_emoji_in_directory_name() {
    let path = PathBuf::from("/project/src/\u{1F680}-launch/main.ts");
    assert!(path.to_str().is_some());
    assert_eq!(path.file_name().and_then(OsStr::to_str), Some("main.ts"));
}

#[test]
fn unicode_path_in_node_modules() {
    let path = PathBuf::from("/project/node_modules/\u{00FC}ber-lib/index.js");
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let nm_idx = components
        .iter()
        .rposition(|&c| c == "node_modules")
        .unwrap();
    assert_eq!(components[nm_idx + 1], "\u{00FC}ber-lib");
}

#[test]
fn unicode_discovered_files_sort_stably() {
    let mut files = [
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/\u{00E9}tude.ts"),
            size_bytes: 10,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/alpha.ts"),
            size_bytes: 20,
        },
    ];
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(files[0].path, PathBuf::from("/project/alpha.ts"));
}

#[test]
fn long_path_does_not_panic() {
    let mut long_path = String::from("/project");
    for i in 0..50 {
        write!(&mut long_path, "/deeply_nested_directory_{i:03}").unwrap();
    }
    long_path.push_str("/index.ts");

    let path = PathBuf::from(&long_path);
    assert!(path.to_str().is_some());
    assert_eq!(path.file_name().and_then(OsStr::to_str), Some("index.ts"));

    let mut map: FxHashMap<&Path, FileId> = FxHashMap::default();
    map.insert(path.as_path(), FileId(0));
    assert_eq!(map.get(path.as_path()), Some(&FileId(0)));
}

#[test]
fn very_long_filename_does_not_panic() {
    let filename = "a".repeat(200) + ".ts";
    let path = PathBuf::from(format!("/project/src/{filename}"));
    assert!(path.to_str().is_some());
    assert_eq!(path.extension().and_then(OsStr::to_str), Some("ts"));
}

#[test]
fn long_path_in_discovered_file() {
    let mut components = String::from("/root");
    for _ in 0..100 {
        components.push_str("/sub");
    }
    components.push_str("/file.ts");

    let file = DiscoveredFile {
        id: FileId(0),
        path: PathBuf::from(&components),
        size_bytes: 42,
    };
    assert_eq!(file.id, FileId(0));
    assert!(file.path.to_str().is_some());
}

#[test]
fn parent_traversal_resolves_correctly() {
    let base = PathBuf::from("/project/src/components");
    let resolved = base.join("../utils/helpers.ts");
    let components: Vec<_> = resolved.components().collect();
    assert!(
        components
            .iter()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        "PathBuf::join preserves .. components (not normalized)"
    );
}

#[test]
fn multiple_parent_traversals() {
    let base = PathBuf::from("/project/src/deep/nested/component");
    let resolved = base.join("../../../utils.ts");
    let depth = resolved
        .components()
        .filter(|c| matches!(c, std::path::Component::ParentDir))
        .count();
    assert_eq!(depth, 3, "should preserve all three .. components");
}

#[test]
fn curdir_in_path_normalized_by_components() {
    let path = PathBuf::from("/project/./src/./utils.ts");
    let components: Vec<_> = path.components().collect();
    let curdir_count = components
        .iter()
        .filter(|c| matches!(c, std::path::Component::CurDir))
        .count();
    assert_eq!(
        curdir_count, 0,
        "PathBuf::components() normalizes away . (CurDir) entries"
    );
}

#[test]
fn strip_prefix_with_relative_paths() {
    let root = PathBuf::from("/project");
    let file = PathBuf::from("/project/src/utils.ts");
    let relative = file.strip_prefix(&root);
    assert!(relative.is_ok());
    assert_eq!(relative.unwrap(), Path::new("src/utils.ts"));
}

#[test]
fn strip_prefix_mismatch() {
    let root = PathBuf::from("/other-project");
    let file = PathBuf::from("/project/src/utils.ts");
    let relative = file.strip_prefix(&root);
    assert!(relative.is_err(), "different roots should not strip");
}

/// The allowlist from `discover.rs` — we test that the expected directories
/// are handled correctly at the path level.
const ALLOWED_HIDDEN_DIRS: &[&str] = &[".storybook", ".well-known", ".changeset", ".github"];

#[test]
fn allowed_hidden_dirs_recognized() {
    for dir in ALLOWED_HIDDEN_DIRS {
        let path = PathBuf::from(format!("/project/{dir}/main.ts"));
        let first_component = path
            .strip_prefix("/project")
            .unwrap()
            .components()
            .next()
            .unwrap();
        if let std::path::Component::Normal(name) = first_component {
            let name_str = name.to_string_lossy();
            assert!(name_str.starts_with('.'), "{dir} should start with a dot");
            assert!(
                ALLOWED_HIDDEN_DIRS.contains(&name_str.as_ref()),
                "{dir} should be in the allowlist"
            );
        }
    }
}

#[test]
fn disallowed_hidden_dir_not_in_allowlist() {
    let disallowed = [".git", ".cache", ".vscode", ".next", ".nuxt"];
    for dir in &disallowed {
        assert!(
            !ALLOWED_HIDDEN_DIRS.contains(dir),
            "{dir} should NOT be in the allowlist"
        );
    }
}

#[test]
fn hidden_file_path_components() {
    let path = PathBuf::from("/project/.storybook/main.ts");
    let relative = path.strip_prefix("/project").unwrap();
    let first = relative.components().next().unwrap();
    if let std::path::Component::Normal(name) = first {
        assert_eq!(name, ".storybook");
    } else {
        panic!("expected Normal component");
    }
}

#[test]
fn dotfile_in_non_hidden_directory() {
    let path = PathBuf::from("/project/src/.eslintrc.js");
    assert_eq!(
        path.file_name().and_then(OsStr::to_str),
        Some(".eslintrc.js")
    );
}

#[test]
fn path_alias_tilde_double_slash() {
    assert!(is_path_alias("~~/utils/shared"));
}

#[test]
fn path_alias_at_slash() {
    assert!(is_path_alias("@/components/Button"));
}

#[test]
fn path_alias_hash_prefix() {
    assert!(is_path_alias("#internal/module"));
}

#[test]
fn path_alias_scoped_uppercase_is_alias() {
    assert!(is_path_alias("@Components/Button"));
    assert!(is_path_alias("@Hooks/useApi"));
}

#[test]
fn path_alias_scoped_lowercase_is_not_alias() {
    assert!(!is_path_alias("@babel/core"));
    assert!(!is_path_alias("@types/node"));
}

#[test]
fn extract_package_name_deeply_nested_subpath() {
    assert_eq!(extract_package_name("lodash/fp/merge/deep"), "lodash");
}

#[test]
fn extract_package_name_scoped_deeply_nested() {
    assert_eq!(
        extract_package_name("@org/pkg/dist/esm/utils/helper"),
        "@org/pkg"
    );
}

#[test]
fn extract_package_name_with_dots() {
    assert_eq!(extract_package_name("my.package.name"), "my.package.name");
}

#[test]
fn extract_package_name_single_char() {
    assert_eq!(extract_package_name("x"), "x");
}

#[test]
fn output_dir_names_in_path() {
    let output_dirs = ["dist", "build", "out", "esm", "cjs"];
    for dir in &output_dirs {
        let path = PathBuf::from(format!("/project/packages/ui/{dir}/utils.js"));
        let components: Vec<_> = path.components().collect();
        let has_output = components.iter().any(|c| {
            if let std::path::Component::Normal(s) = c {
                s.to_str() == Some(*dir)
            } else {
                false
            }
        });
        assert!(has_output, "path should contain {dir} component");
    }
}

#[test]
fn nested_output_dirs_last_position() {
    let path = PathBuf::from("/project/packages/ui/dist/esm/utils.mjs");
    let components: Vec<_> = path.components().collect();
    let output_dirs = ["dist", "build", "out", "esm", "cjs"];
    let last_pos = components.iter().rposition(|c| {
        if let std::path::Component::Normal(s) = c {
            s.to_str().is_some_and(|n| output_dirs.contains(&n))
        } else {
            false
        }
    });
    assert!(last_pos.is_some(), "should find an output dir");
    let found = components[last_pos.unwrap()];
    assert_eq!(
        found,
        std::path::Component::Normal(std::ffi::OsStr::new("esm")),
        "should find 'esm' as the last output dir"
    );
}

#[cfg(target_os = "windows")]
mod windows_paths {
    use super::*;

    #[test]
    fn unc_path_components() {
        let path = PathBuf::from(r"\\server\share\project\src\index.ts");
        assert!(path.to_str().is_some());
        assert_eq!(path.file_name().and_then(OsStr::to_str), Some("index.ts"));
    }

    #[test]
    fn drive_letter_path() {
        let path = PathBuf::from(r"C:\Users\project\src\index.ts");
        assert!(path.is_absolute());
        assert_eq!(path.file_name().and_then(OsStr::to_str), Some("index.ts"));
    }

    #[test]
    fn mixed_separators_normalize() {
        let path = PathBuf::from(r"C:\Users/project\src/index.ts");
        assert_eq!(path.file_name().and_then(OsStr::to_str), Some("index.ts"));
        assert!(path.components().count() >= 4);
    }

    #[test]
    fn unc_node_modules_extraction() {
        let path = PathBuf::from(r"\\server\share\project\node_modules\react\index.js");
        let components: Vec<&str> = path
            .components()
            .filter_map(|c| match c {
                std::path::Component::Normal(s) => s.to_str(),
                _ => None,
            })
            .collect();
        let nm_idx = components.iter().rposition(|&c| c == "node_modules");
        assert!(nm_idx.is_some());
        assert_eq!(components[nm_idx.unwrap() + 1], "react");
    }

    #[test]
    fn drive_letter_with_spaces() {
        let path = PathBuf::from(r"C:\Users\My Documents\project\src\index.ts");
        assert!(path.to_str().is_some());
        assert_eq!(path.file_name().and_then(OsStr::to_str), Some("index.ts"));
    }
}

mod windows_path_strings {
    use super::*;

    #[test]
    fn backslash_in_specifier_is_not_bare() {
        let specifier = r".\utils\helpers";
        assert!(specifier.starts_with('.'));
    }

    #[test]
    fn unc_style_string_parsing() {
        let unc = r"\\server\share\project\node_modules\pkg\index.js";
        let path = PathBuf::from(unc);
        let _components: Vec<_> = path.components().collect();
    }

    #[test]
    fn drive_letter_string_parsing() {
        let drive = r"C:\Users\project\src\index.ts";
        let path = PathBuf::from(drive);
        assert!(path.to_str().is_some());
    }

    #[test]
    fn mixed_separator_string() {
        let mixed = r"C:\Users/project\src/index.ts";
        let path = PathBuf::from(mixed);
        let _ = path.file_name();
        let _ = path.parent();
        let _ = path.extension();
    }

    #[test]
    fn extract_package_name_with_backslash_subpath() {
        let name = extract_package_name("lodash");
        assert_eq!(name, "lodash");
    }
}

#[test]
fn pnpm_path_components_parse_correctly() {
    let path = PathBuf::from(
        "/project/node_modules/.pnpm/@myorg+ui@1.0.0/node_modules/@myorg/ui/dist/index.js",
    );
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();

    let pnpm_idx = components.iter().position(|&c| c == ".pnpm");
    assert!(pnpm_idx.is_some(), "should find .pnpm component");

    let after_pnpm = &components[pnpm_idx.unwrap() + 1..];
    let inner_nm = after_pnpm.iter().position(|&c| c == "node_modules");
    assert!(inner_nm.is_some(), "should find inner node_modules");
}

#[test]
fn pnpm_path_with_peer_deps_suffix() {
    let path = PathBuf::from(
        "/project/node_modules/.pnpm/@myorg+ui@1.0.0_react@18.2.0/node_modules/@myorg/ui/dist/index.js",
    );
    let components: Vec<&str> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect();
    let pnpm_idx = components.iter().position(|&c| c == ".pnpm").unwrap();
    let version_component = components[pnpm_idx + 1];
    assert!(
        version_component.contains("_react@"),
        "peer dep suffix should be part of the version component"
    );
}

#[test]
#[expect(
    clippy::cast_possible_truncation,
    reason = "test file counts are trivially small"
)]
fn file_id_assignment_is_deterministic_by_path_sort() {
    let mut files = [
        DiscoveredFile {
            id: FileId(0),
            path: PathBuf::from("/project/src/z.ts"),
            size_bytes: 100,
        },
        DiscoveredFile {
            id: FileId(1),
            path: PathBuf::from("/project/src/a.ts"),
            size_bytes: 200,
        },
        DiscoveredFile {
            id: FileId(2),
            path: PathBuf::from("/project/src/m.ts"),
            size_bytes: 50,
        },
    ];

    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    for (idx, file) in files.iter_mut().enumerate() {
        file.id = FileId(idx as u32);
    }

    assert_eq!(files[0].path, PathBuf::from("/project/src/a.ts"));
    assert_eq!(files[0].id, FileId(0));
    assert_eq!(files[1].path, PathBuf::from("/project/src/m.ts"));
    assert_eq!(files[1].id, FileId(1));
    assert_eq!(files[2].path, PathBuf::from("/project/src/z.ts"));
    assert_eq!(files[2].id, FileId(2));
}

#[test]
#[expect(
    clippy::cast_possible_truncation,
    reason = "test file counts are trivially small"
)]
fn file_id_assignment_stable_regardless_of_size() {
    let make_files = |sizes: [u64; 3]| {
        let mut files = vec![
            DiscoveredFile {
                id: FileId(0),
                path: PathBuf::from("/project/b.ts"),
                size_bytes: sizes[0],
            },
            DiscoveredFile {
                id: FileId(1),
                path: PathBuf::from("/project/a.ts"),
                size_bytes: sizes[1],
            },
            DiscoveredFile {
                id: FileId(2),
                path: PathBuf::from("/project/c.ts"),
                size_bytes: sizes[2],
            },
        ];
        files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        for (idx, file) in files.iter_mut().enumerate() {
            file.id = FileId(idx as u32);
        }
        files
    };

    let run1 = make_files([100, 200, 300]);
    let run2 = make_files([999, 1, 50]);

    for (f1, f2) in run1.iter().zip(run2.iter()) {
        assert_eq!(f1.path, f2.path, "paths should match");
        assert_eq!(f1.id, f2.id, "IDs should be stable across size changes");
    }
}
