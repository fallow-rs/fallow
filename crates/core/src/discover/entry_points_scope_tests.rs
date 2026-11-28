//! Tests of the scope check that runs before the probe of a package entry.

use std::path::Path;

use fallow_types::discover::EntryPointSource;
use rustc_hash::FxHashMap;

use super::{resolve_entry_path, resolve_entry_path_with_tracking};

/// Resolve each entry with a skip tracker and return the tracker.
fn skipped_for(base: &Path, entries: &[&str]) -> FxHashMap<String, usize> {
    let canonical = dunce::canonicalize(base).unwrap();
    let mut skipped_entries = FxHashMap::default();
    for entry in entries {
        let result = resolve_entry_path_with_tracking(
            base,
            entry,
            &canonical,
            EntryPointSource::PackageJsonMain,
            Some(&mut skipped_entries),
            None,
        );
        assert!(result.is_none(), "{entry} must not resolve");
    }
    skipped_entries
}

#[test]
fn absolute_entry_out_of_the_project_is_skipped_whether_or_not_it_exists() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let root = sandbox.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(sandbox.path().join("other")).unwrap();
    std::fs::write(sandbox.path().join("other/present.ts"), "").unwrap();
    let present = sandbox.path().join("other/present.ts");
    let present = present.to_str().unwrap();
    let missing = sandbox.path().join("other/missing.ts");
    let missing = missing.to_str().unwrap();
    let without_extension = sandbox.path().join("other/present");
    let without_extension = without_extension.to_str().unwrap();

    let skipped = skipped_for(&root, &[present, missing, without_extension]);

    for entry in [present, missing, without_extension] {
        assert_eq!(skipped.get(entry), Some(&1), "{entry}: {skipped:?}");
    }
}

#[test]
fn absolute_entry_inside_the_project_still_resolves() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/index.ts"), "").unwrap();
    let entry = dunce::canonicalize(dir.path())
        .unwrap()
        .join("src/index.ts");

    let resolved = resolve_entry_path(
        dir.path(),
        entry.to_str().unwrap(),
        &dunce::canonicalize(dir.path()).unwrap(),
        EntryPointSource::PackageJsonMain,
    );
    assert_eq!(resolved.map(|entry_point| entry_point.path), Some(entry));
}

#[cfg(unix)]
#[test]
fn entry_behind_a_link_out_of_the_project_is_skipped_whether_or_not_it_exists() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let root = sandbox.path().join("project");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(sandbox.path().join("other")).unwrap();
    std::fs::write(sandbox.path().join("other/present.ts"), "").unwrap();
    std::os::unix::fs::symlink(sandbox.path().join("other"), root.join("linked")).unwrap();
    std::os::unix::fs::symlink(
        sandbox.path().join("other/present.ts"),
        root.join("present.ts"),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        sandbox.path().join("other/missing.ts"),
        root.join("missing.ts"),
    )
    .unwrap();
    let entries = [
        "linked/present.ts",
        "linked/missing.ts",
        "linked/present",
        "linked/missing",
        "present.ts",
        "missing.ts",
    ];

    let skipped = skipped_for(&root, &entries);

    for entry in entries {
        assert_eq!(skipped.get(entry), Some(&1), "{entry}: {skipped:?}");
    }
}

/// A link out of the project that only an extension or index probe
/// reaches counts as a missing file, whether its target exists or not.
#[cfg(unix)]
#[test]
fn probed_link_out_of_the_project_counts_as_missing() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let root = sandbox.path().join("project");
    std::fs::create_dir_all(root.join("dir")).unwrap();
    std::fs::create_dir_all(sandbox.path().join("other")).unwrap();
    std::fs::write(sandbox.path().join("other/present.ts"), "").unwrap();
    for (link, target) in [
        ("fallback.ts", "present.ts"),
        ("gone.ts", "missing.ts"),
        ("dir/index.ts", "present.ts"),
    ] {
        std::os::unix::fs::symlink(sandbox.path().join("other").join(target), root.join(link))
            .unwrap();
    }
    let entries = ["fallback", "gone", "dir", "fallback.js", "gone.js"];

    let skipped = skipped_for(&root, &entries);

    assert!(skipped.is_empty(), "{skipped:?}");
}

/// An output entry whose same-stem source file is a link out of the project
/// maps to no source file, whether the link target exists or not.
#[cfg(unix)]
#[test]
fn output_entry_source_link_out_of_the_project_counts_as_missing() {
    let sandbox = tempfile::tempdir().expect("create sandbox");
    let root = sandbox.path().join("project");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(sandbox.path().join("other")).unwrap();
    std::fs::write(sandbox.path().join("other/present.ts"), "").unwrap();
    for (link, target) in [("src/x.ts", "present.ts"), ("src/y.ts", "missing.ts")] {
        std::os::unix::fs::symlink(sandbox.path().join("other").join(target), root.join(link))
            .unwrap();
    }
    let entries = ["dist/x.js", "dist/y.js", "./lib/x.js", "./lib/y.js"];

    let skipped = skipped_for(&root, &entries);

    assert!(skipped.is_empty(), "{skipped:?}");
}
