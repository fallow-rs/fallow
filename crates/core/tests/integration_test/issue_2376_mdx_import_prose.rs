use crate::common::{create_config, fixture_path};

/// Run the issue #2376 fixture (Astro plugin enabled so `src/pages/**` is an
/// entry surface) and return the reported unused files and unused exports.
/// File names are unique across the fixture, so no path separators take part
/// in the comparison.
fn reported() -> (Vec<String>, Vec<(String, String)>) {
    let root = fixture_path("issue-2376-mdx-import-prose");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let files = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    let exports = results
        .unused_exports
        .iter()
        .map(|e| {
            let file = e
                .export
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            (file, e.export.export_name.clone())
        })
        .collect();
    (files, exports)
}

/// Issue #2376: an MDX prose sentence that opens with the word "import"
/// (`import the thing and render <NS.Moon /> here.`) is prose, not a
/// statement, so the namespace import of `notes.mdx` survives, its target is
/// not an unused file, and the members rendered in the body are credited while
/// the genuinely unused sibling still reports.
#[test]
fn mdx_import_prose_line_keeps_the_files_imports() {
    let (files, exports) = reported();

    assert!(
        !files.iter().any(|file| file == "ns.ts"),
        "the namespace target of an MDX document must not be an unused file: {files:?}"
    );
    for member in ["Star", "Moon"] {
        assert!(
            !exports
                .iter()
                .any(|(file, name)| file == "ns.ts" && name == member),
            "ns.ts:{member} is rendered in the MDX body and must stay credited: {exports:?}"
        );
    }
    assert!(
        exports
            .iter()
            .any(|(file, name)| file == "ns.ts" && name == "Unused"),
        "the unrendered sibling must still report: {exports:?}"
    );
}

/// Issue #2376 fallback: `rejected.mdx` carries a line the parser rejects
/// (`import data from the API using RJ before rendering.`). The block is
/// demoted to prose instead of dropping every import of the file, so its
/// target is used, and the demoted line still counts as a mention of `RJ`, so
/// the namespace keeps its mark-all crediting (issue #2355) and the sibling
/// that is never rendered is not reported.
#[test]
fn rejected_statement_block_is_demoted_and_keeps_mark_all() {
    let (files, exports) = reported();

    assert!(
        !files.iter().any(|file| file == "rejected.ts"),
        "the import target behind a rejected statement line must not be an unused file: {files:?}"
    );
    for member in ["Kept", "KeptSibling"] {
        assert!(
            !exports
                .iter()
                .any(|(file, name)| file == "rejected.ts" && name == member),
            "rejected.ts:{member}: a mention on the demoted line keeps the namespace on the mark-all path: {exports:?}"
        );
    }
}
