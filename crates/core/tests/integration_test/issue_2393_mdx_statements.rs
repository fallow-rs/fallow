use crate::common::{create_config, fixture_path};

#[test]
fn mdx_statement_recovery_preserves_every_valid_module_edge() {
    let root = fixture_path("issue-2393-mdx-statements");
    let results = fallow_core::analyze(&create_config(root)).expect("analysis should succeed");
    let unused_files: Vec<_> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    for target in ["types.ts", "commented.ts", "lazy.ts", "values.ts"] {
        assert!(
            !unused_files.iter().any(|path| path.ends_with(target)),
            "the valid MDX statement must preserve its edge to {target}: {unused_files:?}"
        );
    }
}
