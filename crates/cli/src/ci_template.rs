use crate::exit_codes::exit_invalid_input;
use crate::setup_hooks::symlink_guard::{refusal_message, symlinked_component};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

struct VendoredFile {
    path: &'static str,
    content: &'static str,
    executable: bool,
}

const GITLAB_TEMPLATE: &str = include_str!("../templates/ci/gitlab-ci.yml");

const GITLAB_FILES: &[VendoredFile] = &[
    VendoredFile {
        path: "ci/gitlab-ci.yml",
        content: GITLAB_TEMPLATE,
        executable: false,
    },
    VendoredFile {
        path: "ci/scripts/comment.sh",
        content: include_str!("../templates/ci/scripts/comment.sh"),
        executable: true,
    },
    VendoredFile {
        path: "ci/scripts/review.sh",
        content: include_str!("../templates/ci/scripts/review.sh"),
        executable: true,
    },
    VendoredFile {
        path: "ci/scripts/gitlab_common.sh",
        content: include_str!("../templates/ci/scripts/gitlab_common.sh"),
        executable: true,
    },
    VendoredFile {
        path: "ci/scripts/mutation.sh",
        content: include_str!("../templates/ci/scripts/mutation.sh"),
        executable: true,
    },
    VendoredFile {
        path: "ci/scripts/mutation_common.sh",
        content: include_str!("../templates/ci/scripts/mutation_common.sh"),
        executable: true,
    },
];

pub struct GitlabTemplateOptions {
    pub vendor_dir: Option<PathBuf>,
    pub force: bool,
}

pub fn run_gitlab_template(opts: &GitlabTemplateOptions) -> ExitCode {
    if let Some(dir) = &opts.vendor_dir {
        return vendor_gitlab_files(dir, opts.force);
    }

    crate::report::sink::write_stdout_str(GITLAB_TEMPLATE);
    ExitCode::SUCCESS
}

fn vendor_gitlab_files(root: &Path, force: bool) -> ExitCode {
    if let Some((path, link)) = outside_link(root) {
        eprintln!(
            "Error: {}",
            refusal_message(&display_path(root, &path), &display_path(root, &link))
        );
        return exit_invalid_input();
    }
    for file in GITLAB_FILES {
        let path = root.join(file.path);
        if let Err(err) = write_vendored_file(&path, file.content, file.executable, force) {
            eprintln!("Error: failed to write {}: {err}", path.display());
            return exit_invalid_input();
        }
    }

    crate::report::sink::stdoutln!(
        "Vendored GitLab CI integration to {} ({} files)",
        root.display(),
        GITLAB_FILES.len()
    );
    ExitCode::SUCCESS
}

/// The first vendored file whose path goes through a symlink that resolves
/// outside `root`, with that symlink. A vendor directory that does not exist
/// yet has no entries below it, so it has no such link.
fn outside_link(root: &Path) -> Option<(PathBuf, PathBuf)> {
    root.symlink_metadata().ok()?;
    GITLAB_FILES.iter().find_map(|file| {
        let path = root.join(file.path);
        symlinked_component(root, &path).map(|link| (path, link))
    })
}

fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}

fn write_vendored_file(
    path: &Path,
    content: &str,
    executable: bool,
    force: bool,
) -> std::io::Result<()> {
    if path.exists() {
        let current = std::fs::read_to_string(path)?;
        if current == content {
            set_executable(path, executable)?;
            return Ok(());
        }
        if !force {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "file exists with different content; pass --force to overwrite",
            ));
        }
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::File::create(path)?;
    file.write_all(content.as_bytes())?;
    set_executable(path, executable)
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = if executable { 0o755 } else { 0o644 };
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(mode);
    std::fs::set_permissions(path, perms)
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "Windows stub must match the Unix signature so the unconditional caller can `?`-propagate without #[cfg] branching"
)]
fn set_executable(_path: &Path, _executable: bool) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendored_gitlab_files_include_template_and_scripts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let code = vendor_gitlab_files(dir.path(), false);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(dir.path().join("ci/gitlab-ci.yml").is_file());
        assert!(dir.path().join("ci/scripts/comment.sh").is_file());
        assert!(dir.path().join("ci/scripts/review.sh").is_file());
        // comment.sh and review.sh both `source` gitlab_common.sh, so a vendored
        // pipeline that omits it breaks at runtime.
        assert!(dir.path().join("ci/scripts/gitlab_common.sh").is_file());
        // FALLOW_MUTATION runs mutation.sh, and mutation.sh and comment.sh
        // source mutation_common.sh.
        assert!(dir.path().join("ci/scripts/mutation.sh").is_file());
        assert!(dir.path().join("ci/scripts/mutation_common.sh").is_file());
    }

    #[test]
    fn vendoring_refuses_to_overwrite_user_edits_without_force() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ci/gitlab-ci.yml");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(&path, "custom").expect("write custom");

        let code = vendor_gitlab_files(dir.path(), false);

        assert_eq!(code, exit_invalid_input());
        assert_eq!(std::fs::read_to_string(path).expect("read"), "custom");
    }

    #[cfg(unix)]
    #[test]
    fn vendoring_does_not_create_a_file_through_a_dangling_link() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        let planted = outside.path().join("pipeline.yml");
        std::fs::create_dir(dir.path().join("ci")).expect("mkdir");
        std::os::unix::fs::symlink(&planted, dir.path().join("ci/gitlab-ci.yml")).expect("symlink");

        let code = vendor_gitlab_files(dir.path(), false);

        assert_eq!(code, exit_invalid_input());
        assert!(!planted.exists());
    }

    #[cfg(unix)]
    #[test]
    fn forced_vendoring_does_not_overwrite_a_linked_file_outside_the_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        let target = outside.path().join("script");
        std::fs::write(&target, "keep\n").expect("write");
        std::fs::create_dir_all(dir.path().join("ci/scripts")).expect("mkdir");
        std::os::unix::fs::symlink(&target, dir.path().join("ci/scripts/comment.sh"))
            .expect("symlink");

        let code = vendor_gitlab_files(dir.path(), true);

        assert_eq!(code, exit_invalid_input());
        assert_eq!(std::fs::read_to_string(&target).expect("read"), "keep\n");
    }

    #[cfg(unix)]
    #[test]
    fn vendoring_does_not_write_into_a_linked_directory_outside_the_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outside = tempfile::tempdir().expect("tempdir");
        std::os::unix::fs::symlink(outside.path(), dir.path().join("ci")).expect("symlink");

        let code = vendor_gitlab_files(dir.path(), false);

        assert_eq!(code, exit_invalid_input());
        assert!(!outside.path().join("gitlab-ci.yml").exists());
    }

    #[test]
    fn vendoring_creates_a_missing_vendor_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("new/vendor");

        let code = vendor_gitlab_files(&target, false);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(target.join("ci/gitlab-ci.yml").is_file());
    }

    #[cfg(unix)]
    #[test]
    fn vendoring_writes_through_a_link_inside_the_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(dir.path().join("shared")).expect("mkdir");
        std::os::unix::fs::symlink("shared", dir.path().join("ci")).expect("symlink");

        let code = vendor_gitlab_files(dir.path(), false);

        assert_eq!(code, ExitCode::SUCCESS);
        assert!(dir.path().join("shared/gitlab-ci.yml").is_file());
    }

    #[test]
    fn gitlab_ci_template_for_loops_match_vendored_files() {
        let prefixes = ["ci/scripts/"];
        let lines: Vec<&str> = GITLAB_TEMPLATE.lines().collect();
        let mut referenced: Vec<String> = Vec::new();
        for (idx, line) in lines.iter().enumerate() {
            let Some(rest) = line.trim_start().strip_prefix("for f in ") else {
                continue;
            };
            let Some(spec) = rest.split(';').next() else {
                continue;
            };
            let filenames: Vec<&str> = spec.split_whitespace().collect();
            let prefix = lines
                .iter()
                .skip(idx + 1)
                .take(8)
                .find_map(|next| prefixes.iter().find(|p| next.contains(*p)).copied());
            if let Some(p) = prefix {
                for f in filenames {
                    referenced.push(format!("{p}{f}"));
                }
            }
        }
        assert!(
            !referenced.is_empty(),
            "did not parse any cp loops out of gitlab-ci.yml; the loop format may have changed"
        );

        let bundled: std::collections::BTreeSet<String> =
            GITLAB_FILES.iter().map(|f| f.path.to_string()).collect();
        let missing: Vec<&String> = referenced
            .iter()
            .filter(|p| !bundled.contains(*p))
            .collect();
        assert!(
            missing.is_empty(),
            "gitlab-ci.yml references files via for-in loops that GITLAB_FILES does not bundle: \
             {missing:?}. Either add them to GITLAB_FILES or drop the references from \
             ci/gitlab-ci.yml so vendored pipelines stay in sync with remote-fetch ones."
        );
    }
}
