//! Binaries that package.json scripts or process calls run, but that no
//! installed or listed package provides (`unlisted-binaries`).

use std::path::{Path, PathBuf};

use fallow_config::{PackageJson, ResolvedConfig};
use fallow_graph::graph::ModuleGraph;
use fallow_types::extract::ModuleInfo;
use fallow_types::results::{BinaryUseSite, UnlistedBinary};
use rustc_hash::{FxHashMap, FxHashSet};

use super::{LineOffsetsMap, byte_offset_to_line_col};
use crate::scripts;

/// Tools that the operating system, a system package manager, or a language
/// toolchain installs. No npm package is the usual source of these names, so
/// a project that runs them does not list a package for them.
const SYSTEM_BINARIES: &[&str] = &[
    // POSIX and GNU core utilities.
    "basename",
    "chgrp",
    "chmod",
    "chown",
    "clear",
    "cmp",
    "comm",
    "cut",
    "date",
    "dd",
    "df",
    "diff",
    "dirname",
    "du",
    "env",
    "expr",
    "file",
    "grep",
    "egrep",
    "fgrep",
    "gzip",
    "gunzip",
    "hostname",
    "id",
    "install",
    "ln",
    "logger",
    "md5",
    "md5sum",
    "mkfifo",
    "mktemp",
    "nice",
    "nohup",
    "od",
    "patch",
    "printf",
    "ps",
    "readlink",
    "realpath",
    "rev",
    "sha1sum",
    "sha256sum",
    "shasum",
    "split",
    "stat",
    "strings",
    "sync",
    "tar",
    "time",
    "timeout",
    "tr",
    "tty",
    "uname",
    "unzip",
    "xz",
    "yes",
    "zip",
    "bzip2",
    "tput",
    "stty",
    "column",
    "nl",
    "less",
    "more",
    "vi",
    "vim",
    "nano",
    "killall",
    "pgrep",
    "pkill",
    "lsof",
    "whoami",
    "su",
    "sudo",
    "ulimit",
    "umask",
    "command",
    "type",
    "hash",
    "eval",
    "exec",
    "set",
    "unset",
    "read",
    "printenv",
    "trap",
    "seq",
    "join",
    "paste",
    "fold",
    "fmt",
    "base64",
    "openssl",
    "jq",
    "make",
    "cmake",
    "gcc",
    "g++",
    "cc",
    "clang",
    "ld",
    "strip",
    "nm",
    "objdump",
    // Shells.
    "dash",
    "fish",
    "ksh",
    "tcsh",
    "csh",
    // Network and version control.
    "curl",
    "wget",
    "ssh",
    "scp",
    "sftp",
    "rsync",
    "ssh-keygen",
    "ssh-agent",
    "ssh-add",
    "nc",
    "ping",
    "dig",
    "nslookup",
    "host",
    "git",
    "gh",
    "hg",
    "svn",
    "gpg",
    // Containers and media.
    "docker",
    "docker-compose",
    "podman",
    "ffmpeg",
    "ffprobe",
    "convert",
    "magick",
    // macOS.
    "open",
    "osascript",
    "defaults",
    "security",
    "codesign",
    "xcrun",
    "xcodebuild",
    "xcode-select",
    "plutil",
    "sips",
    "pbcopy",
    "pbpaste",
    "launchctl",
    "ditto",
    "hdiutil",
    "lipo",
    "otool",
    "install_name_tool",
    "productbuild",
    "pkgbuild",
    "notarytool",
    "say",
    "caffeinate",
    "brew",
    "softwareupdate",
    "sw_vers",
    "mdfind",
    "screencapture",
    // Linux.
    "xdg-open",
    "systemctl",
    "journalctl",
    "apt",
    "apt-get",
    "dpkg",
    "yum",
    "dnf",
    "apk",
    "pacman",
    "ldd",
    "notify-send",
    "xclip",
    "xsel",
    "wl-copy",
    "wl-paste",
    "update-desktop-database",
    // Windows.
    "cmd",
    "powershell",
    "pwsh",
    "where",
    "taskkill",
    "tasklist",
    "reg",
    "start",
    "explorer",
    "robocopy",
    "xcopy",
    "icacls",
    "certutil",
    "signtool",
    // Language toolchains outside npm.
    "python",
    "python3",
    "pip",
    "pip3",
    "ruby",
    "gem",
    "bundle",
    "java",
    "javac",
    "go",
    "cargo",
    "rustc",
    "rustup",
    "dotnet",
    "php",
    "composer",
    "perl",
    "swift",
    "kotlin",
    "gradle",
    "mvn",
    "uv",
    "poetry",
    // JavaScript runtimes and package managers.
    "node",
    "nodejs",
    "npm",
    "npx",
    "pnpm",
    "pnpx",
    "yarn",
    "yarnpkg",
    "bun",
    "bunx",
    "deno",
    "corepack",
    "nvm",
    "volta",
    "fnm",
];

/// Inputs of the `unlisted-binaries` detector.
#[derive(Clone, Copy)]
pub struct UnlistedBinaryInput<'a> {
    pub graph: &'a ModuleGraph,
    pub pkg: &'a PackageJson,
    pub config: &'a ResolvedConfig,
    pub workspaces: &'a [fallow_config::WorkspaceInfo],
    pub modules: &'a [ModuleInfo],
    pub line_offsets_by_file: &'a LineOffsetsMap<'a>,
}

/// One manifest of the project: its directory and its parsed `package.json`.
struct Manifest<'a> {
    dir: PathBuf,
    pkg: std::borrow::Cow<'a, PackageJson>,
}

/// What the project provides: the declared dependencies, their binaries, and
/// the binaries of the project packages themselves.
struct Provision {
    root: PathBuf,
    bin_map: FxHashMap<String, String>,
    declared: FxHashSet<String>,
    own_bins: FxHashSet<String>,
}

impl Provision {
    fn provides(&self, binary: &str, package_dir: &Path) -> bool {
        self.own_bins.contains(binary)
            || scripts::resolve_known_dependency_binary(
                binary,
                &self.root,
                &self.bin_map,
                &self.declared,
            )
            .is_some()
            || bin_link_exists(binary, package_dir, &self.root)
    }
}

/// Whether `node_modules/.bin` of `package_dir` or of a directory between it
/// and `root` links `binary`.
fn bin_link_exists(binary: &str, package_dir: &Path, root: &Path) -> bool {
    let mut dir = Some(package_dir);
    while let Some(current) = dir {
        let bin_dir = current.join("node_modules").join(".bin");
        if ["", ".cmd", ".exe"]
            .iter()
            .any(|ext| bin_dir.join(format!("{binary}{ext}")).exists())
        {
            return true;
        }
        if current == root {
            break;
        }
        dir = current.parent().filter(|parent| parent.starts_with(root));
    }
    false
}

/// Whether `binary` is a command name that this check can judge. Paths,
/// variables, globs and file names are not binaries that a package provides.
fn is_checkable_binary(binary: &str) -> bool {
    !binary.is_empty()
        && !binary.starts_with(['-', '.', '$', '%', '@'])
        && !binary.contains(['/', '\\', '$', '*', '?', '[', '{', '`', '=', '"', '\''])
        && !scripts::looks_like_script_file(binary)
        && !binary
            .rsplit_once('.')
            .is_some_and(|(_, ext)| FILE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Extensions that mark a file argument, not a command on `PATH`.
const FILE_EXTENSIONS: &[&str] = &[
    "sh", "bash", "js", "mjs", "cjs", "ts", "mts", "cts", "py", "rb", "pl", "exe", "cmd", "bat",
    "ps1",
];

fn is_skipped_binary(binary: &str, config: &ResolvedConfig) -> bool {
    !is_checkable_binary(binary)
        || scripts::is_shell_builtin(binary)
        || SYSTEM_BINARIES.contains(&binary)
        || config.ignore_binaries.is_ignored(binary)
}

/// Find the binaries that the project runs but does not provide.
///
/// The check needs an installed dependency tree to tell a missing binary from
/// one that an install would link, so a project without a root
/// `node_modules` reports nothing.
pub fn find_unlisted_binaries(input: UnlistedBinaryInput<'_>) -> Vec<UnlistedBinary> {
    let root = &input.config.root;
    if !root.join("node_modules").is_dir() {
        return Vec::new();
    }
    let manifests = collect_manifests(&input);
    let provision = build_provision(&input, &manifests);

    let mut found: FxHashMap<String, Vec<BinaryUseSite>> = FxHashMap::default();
    // One binary often runs from many sites in one package, and each check
    // reads the file system.
    let mut provided: FxHashMap<(String, PathBuf), bool> = FxHashMap::default();
    let mut record = |binary: &str, package_dir: &Path, site: BinaryUseSite| {
        if is_skipped_binary(binary, input.config) {
            return;
        }
        let is_provided = *provided
            .entry((binary.to_string(), package_dir.to_path_buf()))
            .or_insert_with(|| provision.provides(binary, package_dir));
        if !is_provided {
            found.entry(binary.to_string()).or_default().push(site);
        }
    };

    for manifest in &manifests {
        collect_script_uses(input.config, manifest, &mut record);
    }
    collect_source_uses(&input, &manifests, &mut record);

    let mut binaries: Vec<UnlistedBinary> = found
        .into_iter()
        .map(|(binary_name, mut used_in)| {
            used_in.sort_by(|a, b| {
                a.path
                    .cmp(&b.path)
                    .then(a.line.cmp(&b.line))
                    .then(a.col.cmp(&b.col))
                    .then(a.script_name.cmp(&b.script_name))
            });
            used_in.dedup_by(|a, b| {
                a.path == b.path
                    && a.line == b.line
                    && a.col == b.col
                    && a.script_name == b.script_name
            });
            UnlistedBinary {
                binary_name,
                used_in,
            }
        })
        .collect();
    binaries.sort_by(|a, b| a.binary_name.cmp(&b.binary_name));
    binaries
}

fn collect_manifests<'a>(input: &UnlistedBinaryInput<'a>) -> Vec<Manifest<'a>> {
    let mut manifests = vec![Manifest {
        dir: input.config.root.clone(),
        pkg: std::borrow::Cow::Borrowed(input.pkg),
    }];
    for ws in input.workspaces {
        if ws.root == input.config.root {
            continue;
        }
        if let Some(pkg) = fallow_config::load_dir_package_json(&ws.root) {
            manifests.push(Manifest {
                dir: ws.root.clone(),
                pkg: std::borrow::Cow::Owned(pkg),
            });
        }
    }
    manifests
}

fn build_provision(input: &UnlistedBinaryInput<'_>, manifests: &[Manifest<'_>]) -> Provision {
    let manifest_refs: Vec<&PackageJson> = manifests.iter().map(|m| m.pkg.as_ref()).collect();
    let dep_names = scripts::all_dependency_names(&manifest_refs);
    let declared: FxHashSet<String> = dep_names.iter().cloned().collect();
    let nm_roots: Vec<&Path> = manifests
        .iter()
        .map(|m| m.dir.as_path())
        .filter(|dir| dir.join("node_modules").is_dir())
        .collect();
    let bin_map = scripts::project_bin_map(
        &nm_roots,
        &dep_names,
        manifest_refs.iter().copied(),
        &declared,
    );
    let own_bins = manifest_refs
        .iter()
        .flat_map(|pkg| scripts::package_bin_names(pkg))
        .collect();
    Provision {
        root: input.config.root.clone(),
        bin_map,
        declared,
        own_bins,
    }
}

fn collect_script_uses(
    config: &ResolvedConfig,
    manifest: &Manifest<'_>,
    record: &mut impl FnMut(&str, &Path, BinaryUseSite),
) {
    let Some(all_scripts) = manifest.pkg.scripts.as_ref() else {
        return;
    };
    let scripts_to_check = if config.production {
        scripts::filter_production_scripts(all_scripts)
    } else {
        all_scripts.clone()
    };
    if scripts_to_check.is_empty() {
        return;
    }
    let path = manifest.dir.join("package.json");
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut names: Vec<&String> = scripts_to_check.keys().collect();
    names.sort_unstable();
    for name in names {
        let body = &scripts_to_check[name];
        let line = find_script_line(&content, name);
        for binary in scripts::script_binaries(body) {
            record(
                &binary,
                &manifest.dir,
                BinaryUseSite {
                    path: path.clone(),
                    line,
                    col: 0,
                    script_name: Some(name.clone()),
                },
            );
        }
    }
}

fn collect_source_uses(
    input: &UnlistedBinaryInput<'_>,
    manifests: &[Manifest<'_>],
    record: &mut impl FnMut(&str, &Path, BinaryUseSite),
) {
    let mut package_dirs: Vec<&Path> = manifests.iter().map(|m| m.dir.as_path()).collect();
    // The deepest directory that contains a file owns it.
    package_dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for module in input.modules {
        if module.command_invocations.is_empty() {
            continue;
        }
        let Some(node) = input.graph.modules.get(module.file_id.0 as usize) else {
            continue;
        };
        let package_dir = package_dirs
            .iter()
            .copied()
            .find(|dir| node.path.starts_with(dir))
            .unwrap_or(input.config.root.as_path());
        for invocation in &module.command_invocations {
            let (line, col) = byte_offset_to_line_col(
                input.line_offsets_by_file,
                module.file_id,
                invocation.span_start,
            );
            let binaries = if invocation.shell {
                scripts::script_binaries(&invocation.command)
            } else {
                vec![invocation.command.trim().to_string()]
            };
            for binary in binaries {
                record(
                    &binary,
                    package_dir,
                    BinaryUseSite {
                        path: node.path.clone(),
                        line,
                        col,
                        script_name: None,
                    },
                );
            }
        }
    }
}

/// The 1-based line of the script `name` inside the `scripts` object of a
/// `package.json`. Falls back to the line of the `scripts` key, then to 1.
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count in package.json is bounded by file size"
)]
fn find_script_line(content: &str, name: &str) -> u32 {
    let needle = format!("\"{name}\"");
    let mut scripts: Option<(usize, usize)> = None;
    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let Some((scripts_index, scripts_indent)) = scripts else {
            if trimmed
                .strip_prefix("\"scripts\"")
                .is_some_and(|rest| rest.trim_start().starts_with(':'))
            {
                scripts = Some((index, indent));
            }
            continue;
        };
        if trimmed.starts_with('}') && indent <= scripts_indent {
            return (scripts_index + 1) as u32;
        }
        if let Some(rest) = trimmed.strip_prefix(needle.as_str())
            && rest.trim_start().starts_with(':')
        {
            return (index + 1) as u32;
        }
    }
    scripts.map_or(1, |(index, _)| (index + 1) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_line_is_found_inside_the_scripts_object() {
        let content = "{\n  \"name\": \"a\",\n  \"build\": {},\n  \"scripts\": {\n    \"build\": \"tsc\",\n    \"pack\": \"gnome-extensions pack\"\n  }\n}\n";
        assert_eq!(find_script_line(content, "pack"), 6);
        assert_eq!(find_script_line(content, "build"), 5);
        assert_eq!(find_script_line(content, "missing"), 4);
    }

    #[test]
    fn script_line_ignores_keys_outside_the_scripts_object() {
        let content = "{\n  \"files\": [\n    \"scripts\"\n  ],\n  \"scripts\": {\n    \"pack\": \"tool\"\n  },\n  \"lint\": {}\n}\n";
        assert_eq!(find_script_line(content, "pack"), 6);
        assert_eq!(find_script_line(content, "lint"), 5);
    }

    #[test]
    fn paths_variables_and_files_are_not_checkable_binaries() {
        for token in [
            "./scripts/x.sh",
            "bin/foo",
            "$BIN",
            "${TOOL}",
            "*.js",
            "run.sh",
            "tool.exe",
            "-v",
            "",
        ] {
            assert!(!is_checkable_binary(token), "{token}");
        }
        assert!(is_checkable_binary("gnome-extensions"));
        assert!(is_checkable_binary("pkg-config"));
    }
}
