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
    let _ = owners.owner_of(path);
    let _ = owners.owner_count_of(path);
    let _ = owners.owner_and_rule_of(path);
    let _ = owners.section_of(path);
    let _ = owners.section_and_owners_of(path);
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
