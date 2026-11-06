//! A GitHub Actions step of a known action runs a command from a `with:`
//! input, such as the `version` and `publish` inputs of the changesets
//! action. The files that such a command runs are entry points. The inputs of
//! other actions hold free text and add no entry points.

use super::common::{create_config, fixture_path};

#[test]
fn command_inputs_of_a_known_action_make_entry_points() {
    let root = fixture_path("ci-action-command-inputs");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused_files.sort();
    assert_eq!(unused_files, vec![".github/not-a-command.js"]);
}
