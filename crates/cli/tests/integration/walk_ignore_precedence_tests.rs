//! The precedence of ignore rules from the global excludes file of Git and
//! from the directories between the analysis root and the repository top
//! level.

#![cfg(unix)]

use std::fs;
use std::path::Path;

use crate::common::{fallow_command, parse_json};

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
    fs::write(path, contents).expect("write file");
}

fn listed_files(base: &Path, root: &Path) -> Vec<String> {
    let home = base.join("home");
    let output = fallow_command()
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .args([
            "list",
            "--files",
            "--format",
            "json",
            "--quiet",
            "--no-cache",
            "--root",
        ])
        .arg(root)
        .output()
        .expect("run fallow");
    let output = crate::common::CommandOutput::from_process(output);
    let json = parse_json(&output);
    let mut files: Vec<String> = json["files"]
        .as_array()
        .expect("files")
        .iter()
        .filter_map(|file| file.as_str().or_else(|| file["path"].as_str()))
        .map(|file| file.replace('\\', "/"))
        .collect();
    files.sort();
    files
}

/// The global excludes file ignores `*.gen.ts`, and the `.gitignore` of the
/// repository top level re-includes `keep.gen.ts`. A `.gitignore` match
/// wins over a global match, also from a directory above the root, and also
/// when an `.ignore` file is above the top level.
#[test]
fn a_parent_gitignore_re_include_wins_over_the_global_excludes_file() {
    let temp = tempfile::tempdir().expect("temp dir");
    let base = dunce::canonicalize(temp.path()).expect("canonical temp dir");
    let home = base.join("home");
    write(&home.join("global-ignore"), "*.gen.ts\n");
    write(
        &home.join(".gitconfig"),
        &format!(
            "[core]\n\texcludesFile = {}\n",
            home.join("global-ignore").display()
        ),
    );
    let repository = base.join("repo");
    fs::create_dir_all(repository.join(".git")).expect("git directory");
    write(&repository.join(".gitignore"), "!keep.gen.ts\n");
    let root = repository.join("pkg");
    for file in ["src/index.ts", "src/keep.gen.ts", "src/other.gen.ts"] {
        write(&root.join(file), "export const value = 1;\n");
    }
    let expected = vec!["src/index.ts".to_string(), "src/keep.gen.ts".to_string()];

    assert_eq!(listed_files(&base, &root), expected);

    write(&base.join(".ignore"), "unrelated.ts\n");
    assert_eq!(listed_files(&base, &root), expected);
}
