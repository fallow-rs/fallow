//! The read-set oracle of the graph cache.
//!
//! A cold run records each path that import resolution reads (see
//! `fallow_graph::read_trace`). For each recorded path, the oracle changes
//! the path in the way that the read can see, runs a warm analysis, and
//! checks that the graph cache key changed: the run rewrote
//! `graph-cache.bin`.
//!
//! - A content read: the oracle changes the content.
//! - An existence read of a file that exists: the oracle deletes the file.
//! - An existence read of a missing path: the oracle creates a file, or a
//!   directory for a directory check.
//!
//! The oracle skips these paths:
//!
//! - an existing directory, and a missing path below an existing file;
//! - a path below `__fallow_ws_self_resolve__`, the directory name that the
//!   resolver uses for a synthetic importer of a workspace package.
//!
//! The key does not walk `node_modules`. It reads the lockfiles and install
//! markers that a package manager writes on each install. So a path below
//! `<dir>/node_modules` is covered when a change to one of these files in
//! `<dir>` or above it changes the key. When no such file exists, no package
//! manager installed into `<dir>`, and the write is out of the model.

use std::path::{Component, Path, PathBuf};

use fallow_graph::read_trace::{ReadKind, ReadRecorder};
use rustc_hash::{FxHashMap, FxHashSet};

use super::project::Project;
use super::templates::{BUN, DENO, NODE, Step, Template, apply};

/// The files that a package manager rewrites on each install, relative to
/// the directory that holds `node_modules`. This is the model of an install,
/// so it is written out here and not taken from the code under test.
const INSTALL_STATE_FILES: [&str; 11] = [
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "deno.lock",
    "node_modules/.package-lock.json",
    "node_modules/.pnpm/lock.yaml",
    "node_modules/.yarn-state.yml",
    "node_modules/.yarn-integrity",
];

/// The perturbation that the oracle applies to one recorded path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Perturbation {
    AppendContent,
    Delete,
    CreateFile,
    CreateDir,
}

/// A recorded path that the cache key does not cover.
#[derive(Debug)]
struct Gap {
    path: String,
    kinds: Vec<ReadKind>,
    perturbation: Perturbation,
    /// The warm run after the change gave other results than the
    /// cache-free run.
    stale: bool,
}

/// Record the reads of a cold run, then check each read against the key.
fn assert_key_covers_read_set(name: &str, template: &Template, setup: &[Step]) {
    let mut project = Project::new(template.files);
    for step in setup {
        apply(&mut project, *step);
    }
    let recorder = ReadRecorder::start(project.scope());
    run_analysis(&project);
    let events = recorder.finish();
    assert!(
        project.graph_cache_bytes().is_some(),
        "{name}: the cold run must persist graph-cache.bin"
    );

    let reads = group_reads(&project, &events);
    assert!(
        reads
            .keys()
            .any(|path| path.ends_with("src/util.ts") || path.ends_with("main.ts")),
        "{name}: the recorder must see the resolver read a source file, got {:?}",
        reads.keys().collect::<Vec<_>>()
    );

    let baseline = project.cache_snapshot();
    let unchanged = project.graph_cache_bytes();
    run_analysis(&project);
    assert_eq!(
        project.graph_cache_bytes(),
        unchanged,
        "{name}: a warm run with no change must keep graph-cache.bin, or a rewrite proves nothing"
    );
    let mut proxies: FxHashMap<PathBuf, bool> = FxHashMap::default();
    let mut gaps = Vec::new();
    let mut checked = 0usize;
    for (path, kinds) in &reads {
        let Some(perturbation) = perturbation_for(path, kinds) else {
            continue;
        };
        checked += 1;
        if key_changes_after(&project, &baseline, path, perturbation) {
            continue;
        }
        if let Some(owner) = node_modules_owner(path) {
            let covered = *proxies
                .entry(owner.clone())
                .or_insert_with(|| install_state_keys_cache(&project, &baseline, &owner));
            if covered {
                continue;
            }
        }
        gaps.push(Gap {
            path: display_path(&project, path),
            kinds: sorted(kinds),
            perturbation,
            stale: stale_after(&project, &baseline, path, perturbation),
        });
    }
    assert!(checked > 0, "{name}: the oracle checked no recorded read");
    assert!(
        gaps.is_empty(),
        "{name}: the graph cache key does not cover these resolver reads:\n{}",
        gaps.iter()
            .map(|gap| format!(
                "  {} {:?} after {:?}, stale result: {}",
                gap.path, gap.kinds, gap.perturbation, gap.stale
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn sorted(kinds: &FxHashSet<ReadKind>) -> Vec<ReadKind> {
    let mut kinds: Vec<ReadKind> = kinds.iter().copied().collect();
    kinds.sort_unstable();
    kinds
}

#[expect(
    deprecated,
    reason = "the graph cache lives behind fallow_core::analyze, the path that the cache oracle must drive"
)]
fn run_analysis(project: &Project) {
    fallow_core::analyze(&project.config(false)).expect("analysis succeeds");
}

/// Group the events by path, spelled below the non-canonical scope, and drop
/// the reads of the cache directories.
fn group_reads(
    project: &Project,
    events: &[fallow_graph::read_trace::ReadEvent],
) -> std::collections::BTreeMap<PathBuf, FxHashSet<ReadKind>> {
    let scope = project.scope();
    let canonical_scope = dunce::canonicalize(scope).unwrap_or_else(|_| scope.to_path_buf());
    let mut reads: std::collections::BTreeMap<PathBuf, FxHashSet<ReadKind>> =
        std::collections::BTreeMap::new();
    for event in events {
        let Some(relative) = event
            .path
            .strip_prefix(scope)
            .or_else(|_| event.path.strip_prefix(&canonical_scope))
            .ok()
        else {
            continue;
        };
        if relative.starts_with("cache") || relative.starts_with("cold-cache") {
            continue;
        }
        reads
            .entry(scope.join(relative))
            .or_default()
            .insert(event.kind);
    }
    reads
}

fn perturbation_for(path: &Path, kinds: &FxHashSet<ReadKind>) -> Option<Perturbation> {
    let synthetic = path
        .components()
        .any(|component| component == Component::Normal(SELF_RESOLVE_DIR.as_ref()));
    if synthetic || path.is_dir() {
        return None;
    }
    if path.is_file() {
        return Some(if kinds.contains(&ReadKind::Content) {
            Perturbation::AppendContent
        } else {
            Perturbation::Delete
        });
    }
    if path.ancestors().skip(1).any(Path::is_file) {
        return None;
    }
    Some(if kinds.contains(&ReadKind::Dir) {
        Perturbation::CreateDir
    } else {
        Perturbation::CreateFile
    })
}

/// The directory name of the synthetic importer for a workspace package.
const SELF_RESOLVE_DIR: &str = "__fallow_ws_self_resolve__";

/// Apply `perturbation` again and compare a warm run with a cache-free run.
fn stale_after(
    project: &Project,
    baseline: &[(PathBuf, Vec<u8>)],
    path: &Path,
    perturbation: Perturbation,
) -> bool {
    let undo = perturb(path, perturbation);
    let stale = project.observe_warm() != project.observe_cold();
    undo.restore();
    project.restore_cache(baseline);
    stale
}

/// Apply `perturbation` to `path`, run a warm analysis, and undo both the
/// change and the cache write. Return whether the run rewrote the graph
/// cache, which it does only when the key changed.
fn key_changes_after(
    project: &Project,
    baseline: &[(PathBuf, Vec<u8>)],
    path: &Path,
    perturbation: Perturbation,
) -> bool {
    let undo = perturb(path, perturbation);
    let before = project.graph_cache_bytes();
    run_analysis(project);
    let changed = before != project.graph_cache_bytes();
    undo.restore();
    project.restore_cache(baseline);
    changed
}

/// The state to put back after one perturbation.
enum Undo {
    Content(PathBuf, Vec<u8>, std::time::SystemTime),
    Remove(PathBuf),
}

impl Undo {
    fn restore(self) {
        match self {
            Self::Content(path, bytes, modified) => {
                std::fs::write(&path, bytes).expect("restore file");
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|file| {
                        file.set_times(
                            std::fs::FileTimes::new()
                                .set_accessed(modified)
                                .set_modified(modified),
                        )
                    })
                    .expect("restore file times");
            }
            Self::Remove(path) => {
                if path.is_dir() {
                    std::fs::remove_dir_all(&path).expect("remove created dir");
                } else {
                    std::fs::remove_file(&path).expect("remove created file");
                }
            }
        }
    }
}

fn perturb(path: &Path, perturbation: Perturbation) -> Undo {
    match perturbation {
        Perturbation::AppendContent | Perturbation::Delete => {
            let bytes = std::fs::read(path).expect("read recorded file");
            let modified = std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .expect("recorded file time");
            if perturbation == Perturbation::Delete {
                std::fs::remove_file(path).expect("delete recorded file");
            } else {
                let mut next = bytes.clone();
                next.extend_from_slice(b"\n ");
                std::fs::write(path, next).expect("change recorded file");
            }
            Undo::Content(path.to_path_buf(), bytes, modified)
        }
        Perturbation::CreateFile | Perturbation::CreateDir => {
            let top = first_missing_ancestor(path);
            if perturbation == Perturbation::CreateDir {
                std::fs::create_dir_all(path).expect("create probed dir");
            } else {
                std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
                std::fs::write(path, probe_content(path)).expect("create probed file");
            }
            Undo::Remove(top)
        }
    }
}

/// The outermost directory or file that a creation of `path` adds.
fn first_missing_ancestor(path: &Path) -> PathBuf {
    let mut top = path.to_path_buf();
    for ancestor in path.ancestors().skip(1) {
        if ancestor.exists() {
            break;
        }
        top = ancestor.to_path_buf();
    }
    top
}

fn probe_content(path: &Path) -> &'static str {
    let is_json = path
        .extension()
        .is_some_and(|extension| extension == "json" || extension == "jsonc");
    if is_json {
        "{}\n"
    } else {
        "export const probe = 1;\n"
    }
}

/// The directory that holds the outermost `node_modules` of `path`.
fn node_modules_owner(path: &Path) -> Option<PathBuf> {
    let mut owner = PathBuf::new();
    for component in path.components() {
        if component == Component::Normal("node_modules".as_ref()) {
            return Some(owner);
        }
        owner.push(component);
    }
    None
}

/// True when no package manager installed into `owner`, or when a change to
/// one of its install state files changes the key.
fn install_state_keys_cache(
    project: &Project,
    baseline: &[(PathBuf, Vec<u8>)],
    owner: &Path,
) -> bool {
    let scope = project.scope();
    let state_files: Vec<PathBuf> = owner
        .ancestors()
        .take_while(|dir| dir.starts_with(scope))
        .flat_map(|dir| INSTALL_STATE_FILES.iter().map(move |name| dir.join(name)))
        .filter(|file| file.is_file())
        .collect();
    state_files.is_empty()
        || state_files
            .iter()
            .any(|file| key_changes_after(project, baseline, file, Perturbation::AppendContent))
}

fn display_path(project: &Project, path: &Path) -> String {
    path.strip_prefix(project.scope())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn graph_cache_key_covers_node_workspace_reads() {
    assert_key_covers_read_set("node workspace", &NODE, &[]);
}

#[test]
fn graph_cache_key_covers_reads_after_an_npm_install() {
    assert_key_covers_read_set(
        "node workspace after npm install",
        &NODE,
        &[Step::ToggleNpmInstall, Step::ToggleLibExports],
    );
}

#[test]
fn graph_cache_key_covers_bun_reads() {
    assert_key_covers_read_set("bun project", &BUN, &[Step::ToggleBunInstall]);
}

#[test]
fn graph_cache_key_covers_deno_reads() {
    assert_key_covers_read_set("deno project", &DENO, &[Step::ToggleDenoInstall]);
}

#[test]
fn graph_cache_key_covers_reads_of_files_that_discovery_does_not_list() {
    assert_key_covers_read_set(
        "node workspace with files at missed candidate paths",
        &NODE,
        &[
            Step::ToggleExtensionlessUtil,
            Step::ToggleDataJson,
            Step::ToggleBuildOutput,
        ],
    );
}
