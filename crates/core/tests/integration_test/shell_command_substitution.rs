//! Commands inside `$(...)` and backtick substitutions run, so the files that
//! they execute are entry points. Single-quoted text does not run.

use super::common::{create_config, fixture_path};

fn unused_script_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    let mut names: Vec<String> = results
        .unused_files
        .iter()
        .filter_map(|f| {
            let path = f.file.path.to_string_lossy().replace('\\', "/");
            path.rsplit_once("/scripts/")
                .map(|(_, name)| name.to_string())
        })
        .collect();
    names.sort();
    names
}

#[test]
fn files_run_inside_command_substitutions_are_entry_points() {
    let root = fixture_path("shell-command-substitution");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert_eq!(
        unused_script_names(&results),
        vec!["none.ts".to_string()],
        "package.json scripts and CI steps run these files through `$(...)` or backticks. \
         Only the single-quoted reference stays unused."
    );
}
