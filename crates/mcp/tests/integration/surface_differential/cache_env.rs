//! `FALLOW_CACHE_DIR` and `FALLOW_CACHE_MAX_SIZE` have the same effect on
//! each surface (see `docs/environment-variables.md`):
//!
//! - the parse cache goes into the named directory, and a relative path
//!   resolves from the project root;
//! - a size limit removes old entries, so the cache file stays under it;
//! - the run writes nothing else into the project.
//!
//! When the directory is outside the project root, the language server keeps
//! one subdirectory per project root inside it, because one editor process
//! can serve more than one root. The other surfaces, and the language server
//! with a directory inside the root, write into the directory itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::project::{Checkout, materialize};
use super::surfaces::{self, Env, Mcp};
use super::templates;

const CACHE_FILE: &str = "cache.bin";
const MEGABYTE: u64 = 1024 * 1024;
/// A cache file under this size holds almost no entry, so a limit that
/// removes every entry cannot pass.
const MIN_LIMITED_CACHE: u64 = 256 * 1024;
const SIZE_LIMIT_MB: &str = "1";
const RELATIVE_CACHE_DIR: &str = ".cache/fallow";
/// The environment variable that makes [`api_child`] run.
const API_CHILD_ROOT_ENV: &str = "FALLOW_SURFACE_API_CHILD_ROOT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    Cli,
    Mcp,
    Lsp,
    Api,
    Node,
}

/// Each file of the project outside `.git`, with its size.
fn tree(root: &Path) -> BTreeMap<PathBuf, u64> {
    fn walk(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, u64>) {
        for entry in std::fs::read_dir(dir).expect("read project dir") {
            let entry = entry.expect("project entry");
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            let metadata = entry.metadata().expect("entry metadata");
            if metadata.is_dir() {
                walk(root, &path, files);
            } else {
                let relative = path.strip_prefix(root).expect("under root").to_path_buf();
                files.insert(relative, metadata.len());
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files);
    files
}

/// Each cache file under `dir`.
fn cache_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.file_name().is_some_and(|name| name == CACHE_FILE) {
                found.push(path);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, &mut found);
    found.sort();
    found
}

/// One analysis with the cache on, through `surface`, in a process with
/// `env`. Each process runs outside the project, so only the project root
/// can anchor a relative cache directory.
fn analyze(surface: Surface, root: &Path, cwd: &Path, env: Env<'_>) {
    match surface {
        Surface::Cli => {
            let output = surfaces::command(&crate::fallow_binary(), env)
                .args(["dead-code", "--format", "json", "--quiet", "--root"])
                .arg(root)
                .current_dir(cwd)
                .output()
                .expect("run fallow");
            assert!(
                matches!(output.status.code(), Some(0 | 1)),
                "fallow failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Surface::Mcp => {
            Mcp::start(cwd, env).call("analyze", root, &json!({}));
        }
        Surface::Lsp => {
            surfaces::lsp(root, "src/index.ts", cwd, env);
        }
        Surface::Api => {
            let test_binary = std::env::current_exe().expect("test binary");
            let output = surfaces::command(&test_binary, env)
                .env(API_CHILD_ROOT_ENV, root)
                .args([
                    "surface_differential::cache_env::api_child",
                    "--exact",
                    "--ignored",
                    "--test-threads=1",
                ])
                .current_dir(cwd)
                .output()
                .expect("run the fallow_api child");
            assert!(
                output.status.success(),
                "the fallow_api child failed: {}",
                String::from_utf8_lossy(&output.stdout)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "the fallow_api child ran no test"
            );
        }
        Surface::Node => {
            let addon = surfaces::node_addon().expect("Node bindings");
            surfaces::node(&addon, "detectDeadCode", &json!({ "root": root }), cwd, env);
        }
    }
}

/// The child process of the `fallow_api` surface: one dead-code run with the
/// cache on, as the Node bindings run it, in a process with the environment
/// of the parent.
#[test]
#[ignore = "child process of cache_controls_have_the_same_effect_on_every_surface"]
fn api_child() {
    let Some(root) = std::env::var_os(API_CHILD_ROOT_ENV) else {
        return;
    };
    let options = fallow_api::DeadCodeOptions {
        analysis: fallow_api::AnalysisOptions {
            root: Some(PathBuf::from(root)),
            ..fallow_api::AnalysisOptions::default()
        },
        ..fallow_api::DeadCodeOptions::default()
    };
    if let Err(error) = fallow_api::run_dead_code(&options) {
        panic!("run_dead_code failed: {}", error.message);
    }
}

/// The cache file of one run, which must be the only one under `dir`. With
/// a directory outside the root, the language server puts it into a
/// subdirectory named after the root folder.
fn the_cache_file(surface: Surface, dir: &Path, root: &Path) -> PathBuf {
    let found = cache_files(dir);
    assert_eq!(
        found.len(),
        1,
        "{surface:?} writes one {CACHE_FILE} under {}: {found:?}",
        dir.display()
    );
    let file = found[0].clone();
    let parent = file.parent().expect("cache file parent");
    if surface == Surface::Lsp && !dir.starts_with(root) {
        let folder = root
            .file_name()
            .and_then(|name| name.to_str())
            .expect("root name");
        assert_eq!(
            parent.parent(),
            Some(dir),
            "{surface:?} cache depth: {}",
            file.display()
        );
        let sub = parent
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        assert!(
            sub.starts_with(&format!("{folder}-")),
            "{surface:?} names the cache subdirectory after the root: {}",
            file.display()
        );
    } else {
        assert_eq!(
            parent, dir,
            "{surface:?} writes the cache into the directory"
        );
    }
    file
}

fn surfaces_under_test() -> Vec<Surface> {
    let mut list = vec![Surface::Cli, Surface::Mcp, Surface::Lsp, Surface::Api];
    if surfaces::node_addon().is_some() {
        list.push(Surface::Node);
    }
    list
}

#[test]
fn cache_controls_have_the_same_effect_on_every_surface() {
    let project = templates::large_cache();
    let outside = tempfile::tempdir().expect("process dir");

    // Without a limit the cache is larger than the limit of the check, so a
    // small cache file shows that the limit removed entries.
    let unlimited = materialize(&project);
    let unlimited_dir = outside.path().join("unlimited");
    let dir_value = unlimited_dir.to_str().expect("UTF-8 path").to_owned();
    analyze(
        Surface::Cli,
        &unlimited.root,
        outside.path(),
        &[("FALLOW_CACHE_DIR", &dir_value)],
    );
    let size = std::fs::metadata(unlimited_dir.join(CACHE_FILE))
        .expect("unlimited cache file")
        .len();
    assert!(
        size > MEGABYTE,
        "the project must give a parse cache over {MEGABYTE} bytes, got {size}"
    );

    for surface in surfaces_under_test() {
        let absolute = materialize(&project);
        let cache_dir = outside.path().join(format!("{surface:?}-cache"));
        let dir_value = cache_dir.to_str().expect("UTF-8 path").to_owned();
        check_run(surface, &absolute, outside.path(), &cache_dir, &dir_value);

        let relative = materialize(&project);
        check_run(
            surface,
            &relative,
            outside.path(),
            &relative.root.join(RELATIVE_CACHE_DIR),
            RELATIVE_CACHE_DIR,
        );
    }
}

/// One run with `FALLOW_CACHE_DIR=<dir_value>` and a size limit. The cache
/// file is in `cache_dir`, it holds entries but stays under the limit, and
/// neither the project nor the process directory `cwd` has another new or
/// changed file.
fn check_run(surface: Surface, checkout: &Checkout, cwd: &Path, cache_dir: &Path, dir_value: &str) {
    let before = (tree(&checkout.root), tree(cwd));
    analyze(
        surface,
        &checkout.root,
        cwd,
        &[
            ("FALLOW_CACHE_DIR", dir_value),
            ("FALLOW_CACHE_MAX_SIZE", SIZE_LIMIT_MB),
        ],
    );
    let file = the_cache_file(surface, cache_dir, &checkout.root);
    let size = std::fs::metadata(&file).expect("cache file").len();
    assert!(
        (MIN_LIMITED_CACHE..=MEGABYTE).contains(&size),
        "{surface:?} with FALLOW_CACHE_DIR={dir_value}: the cache file has {size} bytes, \
         not between {MIN_LIMITED_CACHE} and the limit of {MEGABYTE}"
    );

    let outside_cache = |files: BTreeMap<PathBuf, u64>, base: &Path| {
        let prefix = cache_dir.strip_prefix(base).ok().map(Path::to_path_buf);
        files
            .into_iter()
            .filter(|(path, _)| {
                prefix
                    .as_ref()
                    .is_none_or(|prefix| !path.starts_with(prefix))
            })
            .collect::<BTreeMap<_, _>>()
    };
    let after = (
        outside_cache(tree(&checkout.root), &checkout.root),
        outside_cache(tree(cwd), cwd),
    );
    let before = (
        outside_cache(before.0, &checkout.root),
        outside_cache(before.1, cwd),
    );
    assert_eq!(
        before.0, after.0,
        "{surface:?} with FALLOW_CACHE_DIR={dir_value} writes into the project outside the cache directory"
    );
    assert_eq!(
        before.1, after.1,
        "{surface:?} with FALLOW_CACHE_DIR={dir_value} writes into its process directory outside the cache directory"
    );
}
