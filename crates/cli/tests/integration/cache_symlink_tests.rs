//! A cloned repository can commit a symlink at `.fallow` or inside it. The
//! analysis caches must not write through such a link to a path outside the
//! project.

#![cfg(unix)]

use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use crate::common::run_fallow_in_root;

struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    outside: PathBuf,
}

fn project() -> Project {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("project");
    let outside = dir.path().join("outside");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"p","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"duplicates":{"minCorpusSizeForTokenCache":1}}"#,
    )
    .expect("config");
    let body = "export function f(a: number) {\n  const b = a * 2;\n  return b + 1;\n}\n";
    std::fs::write(root.join("src/index.ts"), format!("import './a';\n{body}")).expect("index");
    std::fs::write(root.join("src/a.ts"), body).expect("a");
    for args in [
        &["init", "-q"][..],
        &["add", "-A"][..],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-qm",
            "init",
        ][..],
    ] {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&root)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?}");
    }
    Project {
        _dir: dir,
        root,
        outside,
    }
}

/// Run the commands that write caches. Returns whether `health
/// --save-snapshot` refused the snapshot path with exit code 2.
fn run_cache_writers(root: &Path) -> bool {
    let mut snapshot_refused = false;
    for (command, args) in [
        ("check", &["--format", "json", "--quiet"][..]),
        ("dupes", &["--format", "json", "--quiet"][..]),
        ("health", &["--format", "json", "--quiet", "--hotspots"][..]),
        (
            "health",
            &["--format", "json", "--quiet", "--save-snapshot"][..],
        ),
    ] {
        let output = run_fallow_in_root(command, root, args);
        if command == "health" && output.code == 2 {
            let text = format!("{}{}", output.stdout, output.stderr);
            assert!(text.contains("symlink"), "health failed: {text}");
            snapshot_refused = true;
            continue;
        }
        assert!(
            output.code == 0 || output.code == 1,
            "{command} failed: {}{}",
            output.stdout,
            output.stderr
        );
    }
    snapshot_refused
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn the_caches_are_written_in_a_plain_cache_dir() {
    let p = project();
    assert!(!run_cache_writers(&p.root));
    let cache = entries(&p.root.join(".fallow"));
    assert!(cache.contains(&"cache.bin".to_owned()), "{cache:?}");
    assert!(cache.contains(&".gitignore".to_owned()), "{cache:?}");
    assert!(cache.contains(&"snapshots".to_owned()), "{cache:?}");
    assert!(cache.contains(&"churn.bin".to_owned()), "{cache:?}");
}

#[test]
fn a_linked_cache_dir_gets_no_cache_write() {
    let p = project();
    symlink(&p.outside, p.root.join(".fallow")).expect("link");
    assert!(run_cache_writers(&p.root));
    assert_eq!(entries(&p.outside), Vec::<String>::new());
}

#[test]
fn a_dangling_gitignore_link_in_the_cache_dir_is_not_followed() {
    let p = project();
    std::fs::create_dir_all(p.root.join(".fallow")).expect("cache dir");
    let victim = p.outside.join("victim");
    symlink(&victim, p.root.join(".fallow/.gitignore")).expect("link");
    assert!(!run_cache_writers(&p.root));
    assert!(
        !victim.exists(),
        "a cache write followed the .gitignore link"
    );
}

#[test]
fn a_linked_cache_subdir_gets_no_cache_write() {
    let p = project();
    std::fs::create_dir_all(p.root.join(".fallow")).expect("cache dir");
    symlink(&p.outside, p.root.join(".fallow/cache")).expect("link");
    symlink(&p.outside, p.root.join(".fallow/snapshots")).expect("link");
    assert!(run_cache_writers(&p.root));
    assert_eq!(entries(&p.outside), Vec::<String>::new());
}
