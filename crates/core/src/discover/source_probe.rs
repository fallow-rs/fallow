//! The on-disk probes that map a file reference to the source file it names.

use std::path::{Path, PathBuf};

use super::walk::SOURCE_EXTENSIONS;

/// The extensions of a module that a script runtime such as `node` or `tsx`
/// runs.
const RUNNABLE_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// The source file that a package entry (`main`, `bin`, `exports`) names:
/// the exact file, or the path with its extension replaced by a source
/// extension (`lib/index.js` to `lib/index.ts`).
pub fn probe_source_file(resolved: &Path) -> Option<PathBuf> {
    if resolved.is_file() {
        return Some(resolved.to_path_buf());
    }
    SOURCE_EXTENSIONS
        .iter()
        .map(|ext| resolved.with_extension(ext))
        .find(|candidate| candidate.is_file())
}

/// The module that a file argument of a script, task or CI command runs, as
/// Node and the TypeScript runners resolve it. The probe tries the exact file
/// first. Then:
///
/// - A path with a runnable extension gets each runnable extension in its
///   place (`scripts/gen.js` to `scripts/gen.ts`).
/// - Any other path gets each runnable extension added (`scripts/build` to
///   `scripts/build.js`, `scripts/build.prod` to `scripts/build.prod.js`).
///   Another extension is never replaced, so `scripts/deploy.sh` does not
///   name `scripts/deploy.ts`.
///
/// A directory does not resolve to its index file. A command can pass a
/// directory as a filter (`vitest run src/lib/`), and then the index file is
/// not a module that the command runs.
pub fn probe_runnable_file(resolved: &Path) -> Option<PathBuf> {
    if resolved.is_file() {
        return Some(resolved.to_path_buf());
    }
    let has_runnable_extension = resolved
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| RUNNABLE_EXTENSIONS.contains(&ext));
    RUNNABLE_EXTENSIONS
        .iter()
        .map(|ext| {
            if has_runnable_extension {
                return resolved.with_extension(ext);
            }
            let mut name = resolved.as_os_str().to_os_string();
            name.push(".");
            name.push(ext);
            PathBuf::from(name)
        })
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_entries_replace_any_extension_and_commands_only_runnable_ones() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let lib = dir.path().join("lib");
        std::fs::create_dir_all(&lib).expect("lib");
        for file in ["index.ts", "theme.css"] {
            std::fs::write(lib.join(file), "").expect("write");
        }

        assert_eq!(
            probe_source_file(&lib.join("index.json")),
            Some(lib.join("index.ts"))
        );
        assert_eq!(probe_runnable_file(&lib.join("index.json")), None);
        assert_eq!(probe_runnable_file(&lib.join("theme")), None);
        assert_eq!(
            probe_runnable_file(&lib.join("index")),
            Some(lib.join("index.ts"))
        );
    }
}
