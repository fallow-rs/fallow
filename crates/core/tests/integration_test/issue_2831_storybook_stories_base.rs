//! Storybook reads `stories` globs relative to the `.storybook/` directory
//! (issue #2831).

use super::common::{create_config, fixture_path};

fn rel(path: &std::path::Path, root: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_files() -> Vec<String> {
    let root = fixture_path("issue-2831-storybook-stories-base");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_files
        .iter()
        .map(|finding| rel(&finding.file.path, &config.root))
        .collect()
}

#[test]
fn root_storybook_stories_resolve_against_the_config_directory() {
    let unused = unused_files();
    assert!(
        !unused.contains(&"src/button.story.tsx".to_string()),
        "a story that `../src/**/*.story.tsx` matches must be credited, found {unused:?}"
    );
    assert!(
        unused.contains(&"src/orphan.ts".to_string()),
        "a file outside the stories globs must still be reported, found {unused:?}"
    );
}

#[test]
fn workspace_storybook_stories_resolve_against_the_config_directory() {
    let unused = unused_files();
    assert!(
        !unused.contains(&"packages/ui/src/a.docs.tsx".to_string()),
        "a story that `../src/**/*.docs.tsx` matches must be credited, found {unused:?}"
    );
    assert!(
        unused.contains(&"packages/ui/src/orphan.ts".to_string()),
        "a file outside the stories globs must still be reported, found {unused:?}"
    );
}

#[test]
fn storybook_extglob_group_credits_a_nested_story() {
    let unused = unused_files();
    assert!(
        !unused.contains(&"src/components/button/button.case.tsx".to_string()),
        "a story that `../src/**/*.case.@(ts|tsx)` matches must be credited, found {unused:?}"
    );
}
