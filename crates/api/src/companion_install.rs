//! How the active Fallow binary was installed, for companion install hints.
//!
//! The npm platform packages keep the native binary inside a `node_modules`
//! tree, and the `fallow` npm launcher passes the companions to it. A Fallow
//! binary outside `node_modules` (Homebrew, `cargo install`, a release
//! archive) has no launcher, so the npm-only install commands do not help it.

use std::path::Path;

/// npm always writes this directory name in lower case, and `canonicalize`
/// returns the spelling on disk, so an exact match is enough.
const NODE_MODULES: &str = "node_modules";

/// Linux reports the executable path with this suffix after an upgrade
/// replaced the file of the running binary.
const DELETED_SUFFIX: &str = " (deleted)";

/// Executables that ship the Fallow CLI, LSP, or MCP server. Any other
/// executable (for example `node`, `bun`, or an editor host that loads the
/// Node-API addon) gets its companion from the npm loader.
const FALLOW_EXECUTABLES: &[&str] = &["fallow", "fallow-lsp", "fallow-mcp", "fallow-multicall"];

/// True when the active executable is a Fallow binary outside `node_modules`.
/// An unknown path keeps the npm hint, which is right for most installs.
pub fn is_standalone_fallow_binary(current_exe: Option<&Path>) -> bool {
    let Some(current_exe) = current_exe else {
        return false;
    };
    let resolved = dunce::canonicalize(current_exe).unwrap_or_else(|_| current_exe.to_path_buf());
    let is_fallow = resolved
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.strip_suffix(DELETED_SUFFIX).unwrap_or(stem))
        .is_some_and(|stem| FALLOW_EXECUTABLES.contains(&stem));
    is_fallow
        && !resolved
            .components()
            .any(|component| component.as_os_str() == NODE_MODULES)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_binary_inside_node_modules_is_an_npm_install() {
        let exe = PathBuf::from("/project/node_modules/@fallow-cli/linux-x64-gnu/fallow");
        assert!(!is_standalone_fallow_binary(Some(&exe)));
    }

    #[test]
    fn a_fallow_binary_outside_node_modules_is_standalone() {
        for exe in [
            "/opt/homebrew/Cellar/fallow/3.43.0/bin/fallow",
            "/home/user/.cargo/bin/fallow-lsp",
            "/home/user/node_modules_tools/fallow-mcp",
            "C:/tools/fallow.exe",
        ] {
            assert!(
                is_standalone_fallow_binary(Some(&PathBuf::from(exe))),
                "{exe}"
            );
        }
    }

    /// The Node-API addon runs inside `node` or another host process; the npm
    /// loader passes its companions.
    #[test]
    fn a_host_process_that_loads_the_addon_keeps_the_npm_hint() {
        for exe in ["/usr/local/bin/node", "/home/user/.bun/bin/bun"] {
            assert!(
                !is_standalone_fallow_binary(Some(&PathBuf::from(exe))),
                "{exe}"
            );
        }
    }

    #[test]
    fn an_unknown_binary_path_keeps_the_npm_hint() {
        assert!(!is_standalone_fallow_binary(None));
    }

    #[test]
    fn a_replaced_running_binary_is_still_standalone() {
        let exe = PathBuf::from("/home/linuxbrew/.linuxbrew/bin/fallow (deleted)");
        assert!(is_standalone_fallow_binary(Some(&exe)));
    }

    /// `node_modules/.bin/fallow` links into the platform package, so the
    /// real path decides.
    #[cfg(unix)]
    #[test]
    fn a_bin_link_into_an_npm_package_stays_an_npm_install() {
        let root = tempfile::tempdir().expect("temporary directory");
        let package = root.path().join("node_modules/@fallow-cli/linux-x64-gnu");
        std::fs::create_dir_all(&package).expect("create package directory");
        let real = package.join("fallow");
        std::fs::write(&real, []).expect("write fallow fixture");
        let links = root.path().join("links");
        std::fs::create_dir_all(&links).expect("create link directory");
        let link = links.join("fallow");
        std::os::unix::fs::symlink(&real, &link).expect("link fallow");
        assert!(!is_standalone_fallow_binary(Some(&link)));
    }

    /// Homebrew links `bin/fallow` to the binary in its Cellar.
    #[cfg(unix)]
    #[test]
    fn a_link_to_a_cellar_binary_is_standalone() {
        let root = tempfile::tempdir().expect("temporary directory");
        let cellar = root.path().join("Cellar/fallow/3.43.0/bin");
        std::fs::create_dir_all(&cellar).expect("create Cellar directory");
        let real = cellar.join("fallow");
        std::fs::write(&real, []).expect("write fallow fixture");
        let bin = root.path().join("node_modules/bin");
        std::fs::create_dir_all(&bin).expect("create link directory");
        let link = bin.join("fallow");
        std::os::unix::fs::symlink(&real, &link).expect("link fallow");
        assert!(is_standalone_fallow_binary(Some(&link)));
    }
}
