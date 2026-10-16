//! `import.meta.glob` patterns with extglob groups match files the same way
//! Vite does (issue #3489).

use super::common::{create_config, fixture_path};

fn rel(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_files() -> Vec<String> {
    let root = fixture_path("issue-3489-import-meta-glob-extglob");
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
fn negated_extglob_credits_every_file_except_the_negated_ones() {
    let unused = unused_files();
    for used in ["src/modules/alpha.js", "src/modules/nested/beta.js"] {
        assert!(
            !unused.contains(&used.to_string()),
            "`./modules/**/!(*.spec).js` loads {used}, found {unused:?}"
        );
    }
    assert!(
        unused.contains(&"src/modules/alpha.spec.js".to_string()),
        "`!(*.spec)` excludes the spec file, found {unused:?}"
    );
}

#[test]
fn exactly_one_extglob_credits_only_the_listed_alternatives() {
    let unused = unused_files();
    for used in ["src/locales/en.js", "src/locales/nl.js"] {
        assert!(
            !unused.contains(&used.to_string()),
            "`./locales/@(en|nl).js` loads {used}, found {unused:?}"
        );
    }
    assert!(
        unused.contains(&"src/locales/de.js".to_string()),
        "`@(en|nl)` does not match de.js, found {unused:?}"
    );
}
