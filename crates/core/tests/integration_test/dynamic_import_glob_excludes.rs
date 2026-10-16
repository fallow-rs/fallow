//! Negative `import.meta.glob` entries exclude files, and the literal text of
//! a template literal or a string concatenation matches only itself.

use super::common::{create_config, fixture_path};

fn rel(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_files() -> Vec<String> {
    let root = fixture_path("dynamic-import-glob-excludes");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| rel(&finding.file.path, &config.root))
        .collect();
    unused.sort();
    unused
}

#[test]
fn negative_import_meta_glob_entries_exclude_files() {
    let unused = unused_files();
    assert!(
        !unused.contains(&"src/pages/home.js".to_string()),
        "`./pages/*.js` loads home.js, found {unused:?}"
    );
    for excluded in ["src/pages/skip.js", "src/pages/draft.js"] {
        assert!(
            unused.contains(&excluded.to_string()),
            "a negative entry excludes {excluded}, found {unused:?}"
        );
    }
}

#[test]
fn double_star_exclude_matches_from_the_common_base_of_the_positive_globs() {
    // Vite matches `**/pages/kept.js` relative to `src/pages`, the common base
    // of `./pages/*.js`. No path there ends in `pages/kept.js`, so Vite loads
    // the file.
    let unused = unused_files();
    assert!(
        !unused.contains(&"src/pages/kept.js".to_string()),
        "`!**/pages/kept.js` excludes nothing below `src/pages`, found {unused:?}"
    );
}

#[test]
fn literal_brackets_and_braces_match_only_themselves() {
    let unused = unused_files();
    for used in ["src/[id]/view.js", "src/{group}/item.js"] {
        assert!(
            !unused.contains(&used.to_string()),
            "the dynamic import loads {used}, found {unused:?}"
        );
    }
    for decoy in ["src/i/view.js", "src/group/item.js"] {
        assert!(
            unused.contains(&decoy.to_string()),
            "no dynamic import loads {decoy}, found {unused:?}"
        );
    }
}
