#![no_main]
//! CODEOWNERS files in GitHub and GitLab syntax, including GitLab sections.
//! An analyzed repository controls this file, and ownership reports read it.

use std::path::Path;

use fallow_engine::codeowners::CodeOwners;
use libfuzzer_sys::fuzz_target;

/// Paths that the lookups test in every parsed file.
const PATHS: &[&str] = &[
    "README.md",
    "src/index.ts",
    "src/components/Button.tsx",
    "docs/guide/intro.md",
    "packages/a/package.json",
    ".github/workflows/ci.yml",
];

fn query(owners: &CodeOwners, path: &Path) {
    let matched = owners.match_path(path);
    assert_eq!(owners.owner_of(path), matched.and_then(|m| m.owner));
    assert_eq!(owners.owner_count_of(path), matched.map(|m| m.owner_count));
    if let Some(m) = matched {
        // A GitLab exclusion has no owner, and an owning rule has at least one.
        assert_eq!(m.owner.is_none(), m.owner_count == 0);
        assert!(m.section.is_some() || m.section_owners.is_empty());
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(owners) = CodeOwners::parse(source) else {
        return;
    };
    for path in PATHS {
        query(&owners, Path::new(path));
    }
    // A path that the file itself names reaches the rules that match it.
    for line in source.lines() {
        if let Some(pattern) = line.split_whitespace().next() {
            query(&owners, Path::new(pattern.trim_start_matches('/')));
        }
    }
});
