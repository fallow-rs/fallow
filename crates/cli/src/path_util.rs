//! Cross-platform path classification helpers and the `--file` input
//! resolution that several commands share.

use std::path::{Path, PathBuf};

pub use fallow_types::path_util::is_absolute_path_any_platform;

/// Resolves each `--file` input against `root`, in input order. An absolute
/// input stays as it is. Unless `quiet` is set, the function writes one
/// warning to stderr for each input whose resolved path does not exist.
pub fn resolve_file_inputs(root: &Path, files: &[PathBuf], quiet: bool) -> Vec<PathBuf> {
    let resolved: Vec<PathBuf> = files
        .iter()
        .map(|path| {
            if is_absolute_path_any_platform(path) {
                path.clone()
            } else {
                root.join(path)
            }
        })
        .collect();
    if !quiet {
        for (original, path) in missing_file_inputs(files, &resolved) {
            eprintln!(
                "Warning: --file '{}' (resolved to '{}') was not found in the project",
                original.display(),
                path.display()
            );
        }
    }
    resolved
}

/// The `(input, resolved path)` pairs whose resolved path does not exist, in
/// input order. `resolved` must hold the resolved path of each input at the
/// same index.
fn missing_file_inputs<'a>(
    files: &'a [PathBuf],
    resolved: &'a [PathBuf],
) -> Vec<(&'a Path, &'a Path)> {
    files
        .iter()
        .zip(resolved)
        .filter(|(_, path)| !path.exists())
        .map(|(original, path)| (original.as_path(), path.as_path()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_inputs_keep_their_own_resolved_path_in_input_order() {
        let dir = tempfile::tempdir().expect("temp dir");
        let mut files = Vec::new();
        for index in 0..6 {
            let name = PathBuf::from(format!("present{index}.ts"));
            std::fs::write(dir.path().join(&name), "").expect("write fixture");
            files.push(name);
            files.push(PathBuf::from(format!("missing{index}.ts")));
        }

        let resolved = resolve_file_inputs(dir.path(), &files, true);
        let missing = missing_file_inputs(&files, &resolved);

        let expected: Vec<(PathBuf, PathBuf)> = (0..6)
            .map(|index| {
                let name = PathBuf::from(format!("missing{index}.ts"));
                let path = dir.path().join(&name);
                (name, path)
            })
            .collect();
        let actual: Vec<(PathBuf, PathBuf)> = missing
            .into_iter()
            .map(|(original, path)| (original.to_path_buf(), path.to_path_buf()))
            .collect();
        assert_eq!(actual, expected);
    }

    #[test]
    fn an_absolute_input_stays_as_it_is() {
        let dir = tempfile::tempdir().expect("temp dir");
        let absolute = dir.path().join("elsewhere.ts");
        let files = vec![absolute.clone(), PathBuf::from("src/a.ts")];

        let resolved = resolve_file_inputs(Path::new("/project"), &files, true);

        assert_eq!(
            resolved,
            vec![absolute, Path::new("/project").join("src/a.ts")]
        );
    }
}
