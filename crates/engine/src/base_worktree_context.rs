//! Host context that a detached base snapshot shares with the checkout.
//!
//! The snapshot links the dependency and generated directories of the root and
//! of each workspace package. It copies the `tsconfig.json` of `.svelte-kit`
//! instead, because the relative paths of that file must resolve in the base.

use std::fs;
use std::io::Write as _;
use std::path::Path;

/// Host directories of the root and of each workspace package that detached
/// audit base views share. pnpm links workspace dependencies into the
/// `node_modules` of each package, not into the root `node_modules`. The base
/// view links each directory, except `.svelte-kit`, of which it gets a copy
/// of the `tsconfig.json`.
pub const AUDIT_MATERIALIZED_CONTEXT_DIRS: &[&str] =
    &["node_modules", ".nuxt", ".astro", SVELTE_KIT_DIR];
pub const SVELTE_KIT_DIR: &str = ".svelte-kit";
const TSCONFIG_FILE: &str = "tsconfig.json";
/// Marks a directory of a base view that holds files copied from the checkout.
const GENERATED_COPY_MARKER: &str = ".fallow-base-copy";
/// New content of a copy before the rename that puts it in place.
const STAGING_FILE: &str = ".fallow-base-copy.tmp";

const NODE_MODULES_MARKERS: &[&str] = &[".package-lock.json", ".modules.yaml", ".yarn-state.yml"];
const NUXT_MARKERS: &[&str] = &[
    "tsconfig.json",
    "tsconfig.app.json",
    "imports.d.ts",
    "components.d.ts",
    "types/nitro-routes.d.ts",
    "types/nitro-imports.d.ts",
];
const ASTRO_MARKERS: &[&str] = &["types.d.ts", "content.d.ts", "env.d.ts"];
const SVELTE_KIT_MARKERS: &[&str] = &["tsconfig.json", "ambient.d.ts", "non-ambient.d.ts"];
pub const AUDIT_CONTEXT_FILE_MAX_BYTES: u64 = 16 * 1024 * 1024;

pub fn context_markers(name: &str) -> &'static [&'static str] {
    match name {
        "node_modules" => NODE_MODULES_MARKERS,
        ".nuxt" => NUXT_MARKERS,
        ".astro" => ASTRO_MARKERS,
        SVELTE_KIT_DIR => SVELTE_KIT_MARKERS,
        _ => &[],
    }
}

/// Copy the generated `tsconfig.json` of a `.svelte-kit` directory into a real
/// directory of the base view.
///
/// The `paths` of that file are relative to its own directory. Through a link,
/// they resolve to the source files of the checkout, outside the base root,
/// so the base view must get a copy. A marker file shows that the directory
/// holds a copy of an earlier run. A directory that the base commit tracks has
/// no marker and stays as it is.
#[expect(
    clippy::filetype_is_file,
    reason = "the copy reads and trusts only regular files, never a link, a pipe or a device"
)]
pub fn copy_generated_tsconfig(source_dir: &Path, destination_dir: &Path) -> std::io::Result<()> {
    let marker = destination_dir.join(GENERATED_COPY_MARKER);
    match fs::symlink_metadata(destination_dir) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            if !fs::symlink_metadata(&marker).is_ok_and(|metadata| metadata.file_type().is_file()) {
                return Ok(());
            }
        }
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::remove_file(destination_dir)?;
            fs::create_dir(destination_dir)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(destination_dir)?;
        }
        Ok(_) => return Ok(()),
        Err(error) => return Err(error),
    }
    replace_without_following(destination_dir, &marker, b"")?;

    let source = source_dir.join(TSCONFIG_FILE);
    let destination = destination_dir.join(TSCONFIG_FILE);
    let is_small_regular_file = fs::symlink_metadata(&source).is_ok_and(|metadata| {
        metadata.file_type().is_file() && metadata.len() <= AUDIT_CONTEXT_FILE_MAX_BYTES
    });
    if !is_small_regular_file {
        return match fs::remove_file(&destination) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
    }
    replace_without_following(destination_dir, &destination, &fs::read(&source)?)
}

/// Write `contents` to `destination` through a new file in the same directory
/// and a rename. The base commit controls the files in the directory, so
/// `destination` can be a link to a file outside the base view. A rename
/// replaces that link and never writes through it.
fn replace_without_following(
    directory: &Path,
    destination: &Path,
    contents: &[u8],
) -> std::io::Result<()> {
    let staging = directory.join(STAGING_FILE);
    match fs::remove_file(&staging) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging)?;
    file.write_all(contents)?;
    drop(file);
    fs::rename(&staging, destination)
}

/// A workspace `node_modules` or `.svelte-kit` gets its context only when the
/// base has that package directory. A package that exists only in the
/// checkout has no base files that could use it.
pub fn base_has_slot_parent(worktree_path: &Path, kind: &str, relative: &Path) -> bool {
    if kind != "node_modules" && kind != SVELTE_KIT_DIR {
        return true;
    }
    match relative.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            fs::symlink_metadata(worktree_path.join(parent))
                .is_ok_and(|metadata| metadata.file_type().is_dir())
        }
        _ => true,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn copy_replaces_a_tracked_link_and_leaves_its_target_unchanged() {
        let root =
            std::env::temp_dir().join(format!("fallow-base-context-link-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let source = root.join("checkout");
        let destination = root.join("base");
        let outside = root.join("outside.txt");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&destination).unwrap();
        fs::write(source.join(TSCONFIG_FILE), b"{\"compilerOptions\":{}}").unwrap();
        fs::write(&outside, b"keep").unwrap();
        // A base commit can track both the marker and a link in place of the
        // generated file.
        fs::write(destination.join(GENERATED_COPY_MARKER), b"").unwrap();
        std::os::unix::fs::symlink(&outside, destination.join(TSCONFIG_FILE)).unwrap();

        copy_generated_tsconfig(&source, &destination).unwrap();

        assert_eq!(fs::read(&outside).unwrap(), b"keep");
        let copied = destination.join(TSCONFIG_FILE);
        assert!(
            !fs::symlink_metadata(&copied)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&copied).unwrap(), b"{\"compilerOptions\":{}}");
        assert!(!destination.join(STAGING_FILE).exists());
        fs::remove_dir_all(&root).unwrap();
    }
}
