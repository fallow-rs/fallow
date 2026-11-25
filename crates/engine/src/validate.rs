use std::path::{Component, Path, PathBuf};

/// Validate a user-supplied git ref before it reaches a git subprocess:
/// rejects empty refs, option-like refs starting with `-`, and characters
/// outside the ref-syntax allowlist.
///
/// # Errors
///
/// Returns a message describing why the ref was rejected.
pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    crate::changed_files::validate_git_ref(s)
}

/// Canonicalize a project root and require it to be an existing directory.
///
/// The root must not pass through a link that moves it out of its checkout.
/// Without this check, a committed link could point the root at a directory
/// outside the repository, and the analysis would then use that directory
/// as the project. See [`check_root_links`] for the rules.
///
/// When the root is the current directory, or relative to it, the check
/// also covers the spelling of the root from the `PWD` variable. The
/// current directory that the process gets is already resolved, so a shell
/// that changed into a linked directory leaves the link only in `PWD`.
///
/// # Errors
///
/// Returns a message when the path cannot be canonicalized, is not a
/// directory, or passes through such a link.
pub fn validate_root(root: &Path) -> Result<PathBuf, String> {
    let invalid = |err: std::io::Error| format!("invalid root path '{}': {err}", root.display());
    let cwd = std::env::current_dir().map_err(invalid)?;
    let pwd = std::env::var_os("PWD").map(PathBuf::from);
    let home = fallow_process::HostPaths::from_env()
        .home
        .filter(|home| home.is_absolute())
        .and_then(|home| dunce::canonicalize(home).ok());
    let probe = Probe {
        cwd: &cwd,
        ceiling: None,
        home: home.as_deref(),
    };
    check_root_spellings(root, &probe, pwd.as_deref())?;
    let canonical = dunce::canonicalize(root).map_err(invalid)?;
    if !canonical.is_dir() {
        return Err(format!("root path '{}' is not a directory", root.display()));
    }
    Ok(canonical)
}

/// The context of one root check.
struct Probe<'a> {
    /// The current directory. A relative root starts here.
    cwd: &'a Path,
    /// A directory that the search for a checkout does not go above, as
    /// with `GIT_CEILING_DIRECTORIES`. Tests set it, so that a temporary
    /// directory inside a checkout does not change the result.
    ceiling: Option<&'a Path>,
    /// The real path of the home directory. The search for a project stops
    /// below it, so a stray manifest in the home directory does not make the
    /// whole home directory one project.
    home: Option<&'a Path>,
}

impl Probe<'_> {
    /// A git command in `dir` that honors the ceiling.
    fn git(&self, dir: &Path) -> std::process::Command {
        let mut command = crate::git_env::git_command(dir);
        if let Some(ceiling) = self.ceiling {
            command.env("GIT_CEILING_DIRECTORIES", ceiling);
        }
        command
    }

    /// `dir` and its ancestors below the ceiling, nearest first.
    fn ancestors<'p>(&self, dir: &'p Path) -> impl Iterator<Item = &'p Path> {
        let ceiling = self.ceiling;
        dir.ancestors()
            .take_while(move |ancestor| ceiling.is_none_or(|ceiling| *ancestor != ceiling))
    }

    /// The nearest directory, `dir` or an ancestor below the ceiling, that
    /// holds a `.git` entry.
    fn nearest_git_holder(&self, dir: &Path) -> Option<PathBuf> {
        self.ancestors(dir)
            .find(|ancestor| holds_git_entry(ancestor))
            .map(Path::to_path_buf)
    }

    /// The nearest directory, `dir` or an ancestor below the ceiling, that
    /// holds a project manifest. The home directory, each directory that
    /// holds it, and the file system root are never a project.
    fn nearest_project(&self, dir: &Path) -> Option<PathBuf> {
        let home = self.home;
        self.ancestors(dir)
            .filter(|ancestor| home.is_none_or(|home| !home.starts_with(ancestor)))
            .filter(|ancestor| ancestor.parent().is_some())
            .find(|ancestor| holds_project_manifest(ancestor))
            .map(Path::to_path_buf)
    }
}

fn holds_git_entry(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// Check `root`, and its spelling from `pwd` when `pwd` names the current
/// directory and the root is the current directory or relative to it.
fn check_root_spellings(root: &Path, probe: &Probe<'_>, pwd: Option<&Path>) -> Result<(), String> {
    check_root_links(root, probe)?;
    let Some(pwd) = pwd.filter(|pwd| pwd.is_absolute()) else {
        return Ok(());
    };
    let spelled = if root.is_relative() {
        pwd.join(root)
    } else if root == probe.cwd {
        pwd.to_path_buf()
    } else {
        return Ok(());
    };
    // A stale `PWD` names another directory; only the spelling of the same
    // directory is checked.
    match (dunce::canonicalize(pwd), dunce::canonicalize(probe.cwd)) {
        (Ok(pwd), Ok(cwd)) if pwd == cwd => check_root_links(&spelled, probe),
        _ => Ok(()),
    }
}

/// Refuse a root path that passes through a link that leaves its checkout
/// or its project.
///
/// The walk resolves `root` one component at a time, in the same order as
/// the operating system. These rules apply to each link:
///
/// - The project of a link is the nearest directory, from the directory
///   that holds the link upward, with a project manifest (such as
///   `package.json` or a fallow config file).
/// - The checkout of a link is the nearest directory, from the directory
///   that holds the link upward, with a `.git` entry. A link whose target
///   stays in its checkout and in its project is allowed, and fallow runs
///   no git command for it.
/// - For each other link, git names the checkout. Fallow uses the answer
///   only when it is a directory with a `.git` entry that holds the link.
///   A submodule belongs to the work tree of its superproject when the
///   index of the superproject has a submodule entry for it. A repository
///   with no commit at `HEAD` or with an empty index is not a checkout.
/// - In a checkout, a link that git tracks and whose target leaves the
///   checkout is refused. A branch checkout creates only tracked links. An
///   untracked local link (such as a build directory that a cache tool
///   links elsewhere) is allowed.
/// - Outside every checkout, or in a checkout whose index has no entry in
///   the project, a link whose target leaves its project is refused. An
///   example is a project unpacked from an archive. A link with no project
///   (such as `/tmp` on macOS, or a personal link in the home directory) is
///   allowed.
///
/// A relative `root` starts at the current directory.
///
/// # Errors
///
/// Returns a message for a refused link. Also returns a message when a path
/// component or a link target cannot be read. When git cannot read the
/// repository or the index, a link whose target leaves its checkout is
/// refused, and a link whose target stays in its checkout follows the rule
/// for a link outside every checkout. Thus the check never accepts a path
/// that it cannot decide.
fn check_root_links(root: &Path, probe: &Probe<'_>) -> Result<(), String> {
    let invalid = |err: std::io::Error| format!("invalid root path '{}': {err}", root.display());
    let mut current = if root.is_absolute() {
        PathBuf::new()
    } else {
        dunce::canonicalize(probe.cwd).map_err(invalid)?
    };
    for component in root.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            // `current` is a resolved path, so its parent is the real parent.
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(name) => {
                let next = current.join(name);
                let metadata = std::fs::symlink_metadata(&next).map_err(invalid)?;
                if !metadata.file_type().is_symlink() {
                    current = next;
                    continue;
                }
                let target = dunce::canonicalize(&next).map_err(invalid)?;
                let link = Link {
                    root,
                    parent: &current,
                    path: &next,
                    target: &target,
                };
                check_link(&link, probe)?;
                current = target;
            }
        }
    }
    Ok(())
}

/// One link on the walk of [`check_root_links`].
struct Link<'a> {
    root: &'a Path,
    /// The directory that holds the link.
    parent: &'a Path,
    path: &'a Path,
    /// The real path of the link target.
    target: &'a Path,
}

impl Link<'_> {
    fn refusal(&self, area: &str, area_path: &Path) -> String {
        format!(
            "root path '{}' passes through the link '{}', which resolves to '{}' outside the {area} '{}'; pass the resolved path as the root if this is the project you want",
            self.root.display(),
            self.path.display(),
            self.target.display(),
            area_path.display(),
        )
    }

    fn undecided(&self, err: &str) -> String {
        format!(
            "root path '{}' passes through the link '{}' to '{}', and git cannot tell if a checkout tracks it: {err}",
            self.root.display(),
            self.path.display(),
            self.target.display(),
        )
    }

    fn invalid(&self, err: &std::io::Error) -> String {
        format!("invalid root path '{}': {err}", self.root.display())
    }
}

/// Apply the rules of [`check_root_links`] to one link.
fn check_link(link: &Link<'_>, probe: &Probe<'_>) -> Result<(), String> {
    let invalid = |err: std::io::Error| link.invalid(&err);
    let project = probe.nearest_project(link.parent);
    let leaves_project = match &project {
        Some(project) => !is_within(link.target, project).map_err(invalid)?,
        None => false,
    };
    let project_rule = || match &project {
        Some(project) if leaves_project => Err(link.refusal("project directory", project)),
        _ => Ok(()),
    };
    let Some(holder) = probe.nearest_git_holder(link.parent) else {
        return project_rule();
    };
    let stays_in_holder = is_within(link.target, &holder).map_err(invalid)?;
    if stays_in_holder && !leaves_project {
        return Ok(());
    }
    let checkout = match checkout_of(link.parent, probe) {
        Ok(checkout) => checkout,
        Err(_) if stays_in_holder => None,
        Err(err) => return Err(link.undecided(&err)),
    };
    let Some(checkout) = checkout else {
        return project_rule();
    };
    if let Some(project) = project.as_deref().filter(|_| leaves_project) {
        let tracked = index_has_entry_in(&checkout.top, &checkout.listing, project)
            .map_err(|err| link.undecided(&format!("could not read the project: {err}")))?;
        if !tracked {
            return project_rule();
        }
    }
    if is_within(link.target, &checkout.area).map_err(invalid)? {
        return Ok(());
    }
    let tracked = index_links_match(&checkout.top, &checkout.listing, link.path)
        .map_err(|err| link.undecided(&format!("could not read an index entry on disk: {err}")))?;
    if tracked {
        return Err(link.refusal("checkout", &checkout.area));
    }
    Ok(())
}

/// The files that make a directory a project root.
const PROJECT_MANIFESTS: &[&str] = &["package.json", "tsconfig.json", "deno.json", "deno.jsonc"];

fn holds_project_manifest(dir: &Path) -> bool {
    PROJECT_MANIFESTS
        .iter()
        .chain(fallow_config::CONFIG_FILE_NAMES)
        .any(|name| dir.join(name).is_file())
}

/// The checkout of a directory, as git reports it.
struct Checkout {
    /// The top of the checkout that holds the directory.
    top: PathBuf,
    /// The `ls-files -s -z` output of the whole index of `top`.
    listing: Vec<u8>,
    /// The work tree that the area of a link covers: the outermost
    /// superproject of a submodule, else the checkout itself.
    area: PathBuf,
}

/// The checkout that holds `dir`, or `None` when the repository has no
/// commit at `HEAD`, has an empty index, or names a top that is not a
/// directory with a `.git` entry that holds `dir`.
fn checkout_of(dir: &Path, probe: &Probe<'_>) -> Result<Option<Checkout>, String> {
    let Some(top) = trusted_top(dir, probe)? else {
        return Ok(None);
    };
    let head = probe
        .git(&top)
        .args(["rev-parse", "--verify", "-q", "HEAD"])
        .output()
        .map_err(|err| format!("could not run git: {err}"))?;
    if !head.status.success() {
        return Ok(None);
    }
    let listing = index_listing(&top, probe)?;
    if listing.is_empty() {
        return Ok(None);
    }
    let mut area = top.clone();
    loop {
        let outer = git_output(
            probe,
            &area,
            &["rev-parse", "--show-superproject-working-tree"],
        )?;
        if outer.is_empty() {
            break;
        }
        let outer = PathBuf::from(outer);
        if !is_superproject_of(&outer, &area, probe)? {
            break;
        }
        area = outer;
    }
    Ok(Some(Checkout { top, listing, area }))
}

/// The top of the checkout that holds `dir`, when git names a directory
/// that holds `dir` and has a `.git` entry for the same repository. A
/// `core.worktree` setting can name another top. Such an answer gives
/// `None`.
fn trusted_top(dir: &Path, probe: &Probe<'_>) -> Result<Option<PathBuf>, String> {
    let answer = git_output(
        probe,
        dir,
        &["rev-parse", "--show-toplevel", "--absolute-git-dir"],
    )?;
    let mut lines = answer.lines();
    let (Some(top), Some(git_dir)) = (lines.next(), lines.next()) else {
        return Ok(None);
    };
    let top = PathBuf::from(top);
    let io = |err: std::io::Error| err.to_string();
    if !holds_git_entry(&top) || !is_within(dir, &top).map_err(io)? {
        return Ok(None);
    }
    let own_git_dir = git_output(probe, &top, &["rev-parse", "--absolute-git-dir"])?;
    let same =
        file_id(Path::new(git_dir)).map_err(io)? == file_id(Path::new(&own_git_dir)).map_err(io)?;
    Ok(same.then_some(top))
}

/// Whether `outer` is the trusted top of its own checkout, holds `inner`,
/// and has a submodule entry for `inner` in its index.
fn is_superproject_of(outer: &Path, inner: &Path, probe: &Probe<'_>) -> Result<bool, String> {
    let io = |err: std::io::Error| err.to_string();
    let Some(top) = trusted_top(outer, probe)? else {
        return Ok(false);
    };
    if file_id(&top).map_err(io)? != file_id(outer).map_err(io)?
        || !is_within(inner, outer).map_err(io)?
    {
        return Ok(false);
    }
    let listing = index_listing(outer, probe)?;
    index_gitlink_match(outer, &listing, inner)
        .map_err(|err| format!("could not read an index entry on disk: {err}"))
}

/// Run git in `dir` and return its trimmed standard output.
fn git_output(probe: &Probe<'_>, dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = probe
        .git(dir)
        .args(args)
        .output()
        .map_err(|err| format!("could not run git: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} failed: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The `ls-files -s -z` output for every index entry of the checkout at
/// `top`, with each name relative to `top`.
fn index_listing(top: &Path, probe: &Probe<'_>) -> Result<Vec<u8>, String> {
    let output = probe
        .git(top)
        .args(["ls-files", "-s", "-z", "--full-name", "--", ":(top)"])
        .output()
        .map_err(|err| format!("could not run git: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git ls-files failed: {}", stderr.trim()));
    }
    Ok(output.stdout)
}

/// The index mode of a link.
const LINK_MODE: &str = "120000";

/// The index mode of a submodule.
const GITLINK_MODE: &str = "160000";

/// The entries of the `ls-files -s -z` output `listing`, as (mode and
/// object, name) pairs.
fn index_entries(listing: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    listing.split(|byte| *byte == 0).filter_map(|record| {
        let tab = record.iter().position(|byte| *byte == b'\t')?;
        Some((&record[..tab], &record[tab + 1..]))
    })
}

/// Whether one of the link entries in the `listing` of the checkout at
/// `top` is the same file on disk as `link`. The comparison by file
/// identity finds the entry also when the index spells the link or a
/// parent in another case or Unicode form.
fn index_links_match(top: &Path, listing: &[u8], link: &Path) -> std::io::Result<bool> {
    let link_id = link_id_of(link)?;
    for (meta, name) in index_entries(listing) {
        if !meta.starts_with(LINK_MODE.as_bytes()) {
            continue;
        }
        match link_id_of(&top.join(path_from_bytes(name))) {
            Ok(id) if id == link_id => return Ok(true),
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(false)
}

/// Whether one of the submodule entries in the `listing` of the checkout at
/// `top` is the same directory on disk as `inner`.
fn index_gitlink_match(top: &Path, listing: &[u8], inner: &Path) -> std::io::Result<bool> {
    let inner_id = file_id(inner)?;
    for (meta, name) in index_entries(listing) {
        if !meta.starts_with(GITLINK_MODE.as_bytes()) {
            continue;
        }
        match file_id(&top.join(path_from_bytes(name))) {
            Ok(id) if id == inner_id => return Ok(true),
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(false)
}

/// Whether the `listing` of the checkout at `top` has an entry at or below
/// `project`. Only the distinct name prefixes with the depth of `project`
/// below `top` are compared by file identity. A prefix that cannot be read
/// does not count, so an error never adds an entry.
fn index_has_entry_in(top: &Path, listing: &[u8], project: &Path) -> std::io::Result<bool> {
    if is_within(top, project)? {
        return Ok(!listing.is_empty());
    }
    let project_id = file_id(project)?;
    let top_id = file_id(top)?;
    let mut depth = None;
    for (steps, ancestor) in project.ancestors().enumerate() {
        if file_id(ancestor)? == top_id {
            depth = Some(steps);
            break;
        }
    }
    let Some(depth) = depth else {
        return Ok(false);
    };
    let mut prefixes = rustc_hash::FxHashSet::default();
    for (_, name) in index_entries(listing) {
        let mut ends = name
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b'/')
            .map(|(index, _)| index)
            .chain(std::iter::once(name.len()));
        if let Some(end) = ends.nth(depth - 1) {
            prefixes.insert(&name[..end]);
        }
    }
    Ok(prefixes
        .into_iter()
        .any(|prefix| file_id(&top.join(path_from_bytes(prefix))).is_ok_and(|id| id == project_id)))
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Whether `path` is `dir` or below it. The test compares the file
/// identity of `dir` with each ancestor of the real path of `path`, so a
/// difference in case or in the spelling of a parent link does not matter.
fn is_within(path: &Path, dir: &Path) -> std::io::Result<bool> {
    let resolved = dunce::canonicalize(path)?;
    let dir_id = file_id(&dunce::canonicalize(dir)?)?;
    for ancestor in resolved.ancestors() {
        if file_id(ancestor)? == dir_id {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(unix)]
type FileId = (u64, u64);

/// Without a portable file identity, the real path itself is the identity.
#[cfg(not(unix))]
type FileId = PathBuf;

#[cfg(unix)]
fn file_id(path: &Path) -> std::io::Result<FileId> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn file_id(path: &Path) -> std::io::Result<FileId> {
    Ok(path_identity(&dunce::canonicalize(path)?, cfg!(windows)))
}

/// The identity of the link itself, not of its target.
#[cfg(unix)]
fn link_id_of(path: &Path) -> std::io::Result<FileId> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::symlink_metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

/// The identity of the link itself: the real path of its parent and the
/// name of the link.
#[cfg(not(unix))]
fn link_id_of(path: &Path) -> std::io::Result<FileId> {
    std::fs::symlink_metadata(path)?;
    let no_name = || std::io::Error::new(std::io::ErrorKind::InvalidInput, "the link has no name");
    let name = path.file_name().ok_or_else(no_name)?;
    let parent = path.parent().ok_or_else(no_name)?;
    Ok(link_identity(
        &dunce::canonicalize(parent)?,
        name,
        cfg!(windows),
    ))
}

/// The identity of a real path on a platform with no file identity. On a
/// file system that ignores case, the identity is in lower case.
#[cfg(any(not(unix), test))]
fn path_identity(real: &Path, ignore_case: bool) -> PathBuf {
    if ignore_case {
        PathBuf::from(real.to_string_lossy().to_lowercase())
    } else {
        real.to_path_buf()
    }
}

/// The identity of the link `name` in the directory with the real path
/// `real_parent`, on a platform with no file identity.
#[cfg(any(not(unix), test))]
fn link_identity(real_parent: &Path, name: &std::ffi::OsStr, ignore_case: bool) -> PathBuf {
    path_identity(&real_parent.join(name), ignore_case)
}

/// Reject strings containing control characters (bytes < 0x20) except
/// newline (0x0A) and tab (0x09). This prevents agents from accidentally
/// passing invisible characters in CLI arguments.
pub fn validate_no_control_chars(s: &str, arg_name: &str) -> Result<(), String> {
    for (i, byte) in s.bytes().enumerate() {
        if byte < 0x20 && byte != b'\n' && byte != b'\t' {
            return Err(format!(
                "{arg_name} contains control character (byte 0x{byte:02x}) at position {i}"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_chars_rejects_bytes_below_space_except_newline_and_tab() {
        for input in [
            "test\x07ref",
            "\x1b[31mred",
            "main\rinjected",
            "abc\x0cdef",
            "abc\x08def",
        ] {
            assert!(
                validate_no_control_chars(input, "--arg").is_err(),
                "{input:?} must be rejected"
            );
        }
    }

    #[test]
    fn control_chars_allows_printable_text_newline_and_tab() {
        for input in [
            "main",
            "line1\nline2",
            "col1\tcol2",
            "",
            "my-package-日本語",
            "./path/to/config.toml",
            "hello world",
        ] {
            assert_eq!(
                validate_no_control_chars(input, "--arg"),
                Ok(()),
                "{input:?} must be accepted"
            );
        }
    }

    #[test]
    fn git_ref_rejects_shell_metacharacters_and_option_like_refs() {
        for input in [
            "main;rm -rf /",
            "main`whoami`",
            "main$HOME",
            "main|cat /etc/passwd",
            "main&&echo pwned",
            "$(whoami)",
            "--upload-pack=evil",
            "-flag",
        ] {
            assert!(
                validate_git_ref(input).is_err(),
                "{input:?} must be rejected"
            );
        }
    }

    #[test]
    fn git_ref_allows_ref_syntax_and_reflog_selectors() {
        for input in [
            "main",
            "feature/my-branch",
            "HEAD~3",
            "HEAD^2",
            "abc123def456",
            "v1.2.3",
            "feature_branch",
            "HEAD@{0}~3",
            "origin/main@{0}",
            "HEAD@{2025-01-01}",
            "HEAD@{1 week ago}",
            "HEAD@{3 days ago}",
        ] {
            assert_eq!(
                validate_git_ref(input),
                Ok(input),
                "{input:?} must be accepted"
            );
        }
    }

    #[test]
    fn control_chars_rejects_null_byte() {
        let result = validate_no_control_chars("main\x00branch", "--changed-since");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("0x00"));
        assert!(err.contains("--changed-since"));
    }

    #[test]
    fn git_ref_rejects_unclosed_brace() {
        let result = validate_git_ref("HEAD@{");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("unclosed"),
            "Error should mention unclosed brace, got: {err}"
        );
    }

    #[test]
    fn git_ref_rejects_colon_outside_braces() {
        let result = validate_git_ref("HEAD:file.txt");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("disallowed character"),
            "Error should mention disallowed character, got: {err}"
        );
        assert!(
            err.contains(':'),
            "Error should mention the colon, got: {err}"
        );
    }

    #[test]
    fn git_ref_rejects_space_outside_braces() {
        let result = validate_git_ref("some ref");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("disallowed character"),
            "Error should mention disallowed character, got: {err}"
        );
    }

    #[test]
    fn git_ref_rejects_empty() {
        let result = validate_git_ref("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty"));
    }

    #[test]
    fn git_ref_rejects_leading_dash() {
        let result = validate_git_ref("--evil-flag");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("start with '-'"));
    }

    #[test]
    fn validate_root_nonexistent_path() {
        let result = validate_root(std::path::Path::new(
            "/nonexistent/path/that/does/not/exist",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn validate_root_valid_dir() {
        let temp = std::env::temp_dir();
        let result = validate_root(&temp);
        assert!(result.is_ok());
    }

    use crate::git_fixture::fixture_git as git;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    /// A temporary directory that is also the ceiling of the checkout
    /// search, so a temporary directory inside a checkout does not change
    /// the result. `base` is its real path.
    struct Fixture {
        _temp: tempfile::TempDir,
        base: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("temp dir");
            let base = dunce::canonicalize(temp.path()).expect("temp path");
            Self { _temp: temp, base }
        }

        fn dir(&self, relative: &str) -> PathBuf {
            let dir = self.base.join(relative);
            std::fs::create_dir_all(&dir).expect("create dir");
            dir
        }

        /// A git repository at `relative` with one committed file.
        fn repo(&self, relative: &str) -> PathBuf {
            let repo = self.dir(relative);
            git(&repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join("README.md"), "repo\n").expect("readme");
            commit(&repo);
            repo
        }

        /// Check `root` from the current directory `cwd`.
        fn check(&self, root: &Path, cwd: &Path) -> Result<(), String> {
            self.check_with_pwd(root, cwd, None)
        }

        fn check_with_pwd(
            &self,
            root: &Path,
            cwd: &Path,
            pwd: Option<&Path>,
        ) -> Result<(), String> {
            self.check_with_home(root, cwd, pwd, None)
        }

        fn check_with_home(
            &self,
            root: &Path,
            cwd: &Path,
            pwd: Option<&Path>,
            home: Option<&Path>,
        ) -> Result<(), String> {
            let probe = Probe {
                cwd,
                ceiling: Some(&self.base),
                home,
            };
            check_root_spellings(root, &probe, pwd)
        }
    }

    fn commit(repo: &Path) {
        git(repo, &["add", "-A"]);
        git(repo, &["commit", "-q", "-m", "commit"]);
    }

    /// A committed link `repo/packages` to `outside`, which has an `app`
    /// directory.
    #[cfg(unix)]
    fn committed_outside_link(fixture: &Fixture) -> (PathBuf, PathBuf) {
        let repo = fixture.repo("repo");
        let outside = fixture.dir("outside/app").parent().unwrap().to_path_buf();
        symlink(&outside, repo.join("packages")).expect("link");
        commit(&repo);
        (repo, outside)
    }

    /// Whether the file system of `dir` ignores case.
    #[cfg(unix)]
    fn ignores_case(dir: &Path) -> bool {
        let probe = dir.join("case-probe");
        std::fs::create_dir(&probe).expect("probe dir");
        let ignores = dir.join("CASE-PROBE").exists();
        std::fs::remove_dir(&probe).expect("remove probe");
        ignores
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_committed_link_that_resolves_outside_the_checkout() {
        let fixture = Fixture::new();
        let (repo, _outside) = committed_outside_link(&fixture);
        let elsewhere = fixture.dir("elsewhere");

        for root in [repo.join("packages").join("app"), repo.join("packages")] {
            let err = fixture.check(&root, &elsewhere).unwrap_err();
            assert!(err.contains("outside the checkout"), "got: {err}");
        }
        let err = fixture
            .check(Path::new("packages/app/../app"), &repo)
            .unwrap_err();
        assert!(err.contains("outside the checkout"), "got: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn allows_an_untracked_local_link_that_leaves_the_checkout() {
        let fixture = Fixture::new();
        let repo = fixture.repo("repo");
        let outside = fixture.dir("outside/app").parent().unwrap().to_path_buf();
        symlink(&outside, repo.join("target")).expect("link");

        assert_eq!(
            fixture.check(&repo.join("target").join("app"), &repo),
            Ok(())
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_leaving_link_when_git_cannot_read_the_checkout() {
        let fixture = Fixture::new();
        let repo = fixture.dir("repo");
        // A `.git` file that names no repository fails in git itself.
        std::fs::write(repo.join(".git"), "gitdir: /nonexistent-fallow-gitdir\n")
            .expect("broken git file");
        let outside = fixture.dir("outside");
        symlink(&outside, repo.join("packages")).expect("link");

        let err = fixture.check(&repo.join("packages"), &repo).unwrap_err();
        assert!(err.contains("git cannot tell"), "got: {err}");
    }

    /// A link that stays below the directory with the nearest `.git` entry
    /// needs no answer from git, so a repository that git cannot read does
    /// not refuse it.
    #[cfg(unix)]
    #[test]
    fn allows_a_link_inside_the_checkout_when_git_cannot_read_it() {
        let fixture = Fixture::new();
        let repo = fixture.dir("repo");
        std::fs::write(repo.join(".git"), "gitdir: /nonexistent-fallow-gitdir\n")
            .expect("broken git file");
        std::fs::write(repo.join("package.json"), "{}").expect("manifest");
        std::fs::create_dir_all(repo.join("shared").join("app")).expect("shared dir");
        symlink("shared", repo.join("packages")).expect("link");

        assert_eq!(
            fixture.check(&repo.join("packages").join("app"), &repo),
            Ok(())
        );
    }

    /// A `core.worktree` setting can name a work tree that does not hold the
    /// `.git` entry. Such an answer is not trusted, so the rule for a
    /// directory without a checkout applies.
    #[cfg(unix)]
    #[test]
    fn ignores_a_checkout_top_that_does_not_hold_the_git_entry() {
        let fixture = Fixture::new();
        let home = fixture.repo("home");
        let repo = fixture.repo("home/repo");
        std::fs::write(repo.join("package.json"), "{}").expect("manifest");
        let outside = fixture.dir("outside/app").parent().unwrap().to_path_buf();
        symlink(&outside, repo.join("packages")).expect("link");
        commit(&repo);
        let elsewhere = fixture.dir("elsewhere");
        let root = repo.join("packages").join("app");
        let probe = Probe {
            cwd: &elsewhere,
            ceiling: Some(&fixture.base),
            home: None,
        };
        assert_eq!(trusted_top(&repo, &probe), Ok(Some(repo.clone())));

        // A top with no `.git` entry, and the top of another repository.
        for worktree in [&fixture.base, &home] {
            git(
                &repo,
                &["config", "core.worktree", worktree.to_str().unwrap()],
            );
            assert_eq!(trusted_top(&repo, &probe), Ok(None), "{worktree:?}");
            let err = fixture.check(&root, &elsewhere).unwrap_err();
            assert!(err.contains("outside the project directory"), "got: {err}");
        }
    }

    /// A step to a superproject counts only when its index has a submodule
    /// entry that is the same directory on disk as the inner checkout.
    #[cfg(unix)]
    #[test]
    fn index_gitlink_match_compares_the_file_identity_of_each_submodule_entry() {
        let fixture = Fixture::new();
        let outer = fixture.dir("outer");
        let inner = fixture.dir("outer/vendor/lib");
        fixture.dir("outer/vendor/other");
        symlink("vendor", outer.join("Vendor-spelled")).expect("second name");
        let entry = |mode: &str, name: &str| format!("{mode} 0123abcd 0\t{name}\0");
        let matches = |entries: &[(&str, &str)]| {
            let listing: String = entries
                .iter()
                .map(|(mode, name)| entry(mode, name))
                .collect();
            index_gitlink_match(&outer, listing.as_bytes(), &inner).unwrap()
        };

        assert!(matches(&[("160000", "Vendor-spelled/lib")]));
        assert!(!matches(&[
            ("160000", "vendor/other"),
            ("100644", "README.md")
        ]));
        assert!(!matches(&[("100644", "vendor/lib")]));
    }

    #[cfg(unix)]
    #[test]
    fn allows_a_link_that_stays_in_the_checkout() {
        let fixture = Fixture::new();
        let repo = fixture.repo("repo");
        std::fs::create_dir_all(repo.join("shared").join("app")).expect("shared dir");
        symlink("shared", repo.join("packages")).expect("link");
        commit(&repo);

        assert_eq!(
            fixture.check(&repo.join("packages").join("app"), &repo),
            Ok(())
        );
    }

    #[cfg(unix)]
    #[test]
    fn allows_a_link_outside_every_checkout() {
        let fixture = Fixture::new();
        let repo = fixture.repo("repo");
        let linked = fixture.base.join("linked");
        symlink(&repo, &linked).expect("link");
        let elsewhere = fixture.dir("elsewhere");

        assert_eq!(fixture.check(&linked, &elsewhere), Ok(()));
    }

    /// Without a file identity, the identity is the real path, in lower case
    /// when the file system ignores case. A link keeps its own name below
    /// the real path of its parent.
    #[test]
    fn path_identity_compares_real_paths_without_case_when_the_platform_ignores_case() {
        let upper = Path::new("/r/Apps").join("Packages");
        let lower = Path::new("/r/apps").join("packages");
        assert_eq!(path_identity(&upper, true), path_identity(&lower, true));
        assert_ne!(path_identity(&upper, false), path_identity(&lower, false));
        assert_eq!(
            link_identity(Path::new("/r/Apps"), std::ffi::OsStr::new("Packages"), true),
            path_identity(&lower, true)
        );
        assert_ne!(
            link_identity(Path::new("/r/apps"), std::ffi::OsStr::new("other"), true),
            path_identity(&lower, true)
        );
    }

    /// The index entries of a directory are matched with a link by file
    /// identity. A second name for the same link stands for a spelling in
    /// another case or Unicode form, so this test runs on every platform.
    #[cfg(unix)]
    #[test]
    fn index_links_match_compares_the_file_identity_of_each_link_entry() {
        let fixture = Fixture::new();
        let dir = fixture.dir("dir");
        symlink("/nonexistent-target", dir.join("packages")).expect("link");
        std::fs::hard_link(dir.join("packages"), dir.join("Packages-spelled"))
            .expect("second name of the link");
        symlink("/nonexistent-target", dir.join("other")).expect("other link");
        let entry = |mode: &str, name: &str| format!("{mode} 0123abcd 0\t{name}\0");
        let link = dir.join("packages");
        let matches = |entries: &[(&str, &str)]| {
            let listing: String = entries
                .iter()
                .map(|(mode, name)| entry(mode, name))
                .collect();
            index_links_match(&dir, listing.as_bytes(), &link).unwrap()
        };

        assert!(matches(&[
            ("120000", "Packages-spelled"),
            ("100644", "README.md")
        ]));
        assert!(!matches(&[("120000", "other"), ("120000", "deleted")]));
        assert!(!matches(&[("100644", "packages")]));
    }

    /// The listing holds the whole index, so a link entry below a parent
    /// that the index spells in another case or Unicode form still matches.
    /// A second name for the parent directory stands for that spelling.
    #[cfg(unix)]
    #[test]
    fn index_links_match_finds_a_link_entry_below_a_parent_with_another_spelling() {
        let fixture = Fixture::new();
        let top = fixture.dir("top");
        let apps = fixture.dir("top/apps");
        symlink("/nonexistent-target", apps.join("packages")).expect("link");
        symlink("apps", top.join("Apps-spelled")).expect("second name");
        let listing = "120000 0123abcd 0\tApps-spelled/packages\0";

        assert!(index_links_match(&top, listing.as_bytes(), &apps.join("packages")).unwrap());
    }

    /// The index spells the parent `Apps` and the disk spells it `apps`. The
    /// test needs a file system that ignores case.
    #[cfg(unix)]
    #[test]
    fn refuses_a_committed_link_below_a_parent_spelled_in_another_case() {
        let fixture = Fixture::new();
        if !ignores_case(&fixture.base) {
            return;
        }
        let repo = fixture.repo("repo");
        let outside = fixture.dir("outside/app").parent().unwrap().to_path_buf();
        std::fs::create_dir(repo.join("Apps")).expect("apps dir");
        symlink(&outside, repo.join("Apps").join("packages")).expect("link");
        commit(&repo);
        std::fs::rename(repo.join("Apps"), repo.join("apps-moved")).expect("rename");
        std::fs::rename(repo.join("apps-moved"), repo.join("apps")).expect("rename");

        let err = fixture
            .check(&repo.join("apps").join("packages").join("app"), &repo)
            .unwrap_err();
        assert!(err.contains("outside the checkout"), "got: {err}");
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_committed_link_spelled_in_another_case_or_unicode_form() {
        let fixture = Fixture::new();
        if !ignores_case(&fixture.base) {
            return;
        }
        let repo = fixture.repo("repo");
        let outside = fixture.dir("outside/app").parent().unwrap().to_path_buf();
        symlink(&outside, repo.join("Packages")).expect("link");
        symlink(&outside, repo.join("\u{c4}pp")).expect("link");
        commit(&repo);

        for spelled in ["packages", "\u{e4}pp", "a\u{308}pp"] {
            let err = fixture
                .check(&repo.join(spelled).join("app"), &repo)
                .unwrap_err();
            assert!(err.contains("outside the checkout"), "{spelled}: {err}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn allows_a_link_inside_a_checkout_spelled_in_another_case() {
        let fixture = Fixture::new();
        if !ignores_case(&fixture.base) {
            return;
        }
        let repo = fixture.repo("repo");
        std::fs::create_dir_all(repo.join("shared").join("app")).expect("shared dir");
        symlink(repo.join("shared"), repo.join("packages")).expect("link");
        commit(&repo);
        let upper = fixture.base.join("REPO");

        assert_eq!(
            fixture.check(&upper.join("packages").join("app"), &repo),
            Ok(())
        );
    }

    #[cfg(unix)]
    #[test]
    fn allows_a_link_from_a_submodule_into_its_superproject() {
        let fixture = Fixture::new();
        let library = fixture.repo("library");
        let repo = fixture.repo("repo");
        git(
            &repo,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                library.to_str().unwrap(),
                "vendor/lib",
            ],
        );
        commit(&repo);
        std::fs::create_dir_all(repo.join("shared")).expect("shared dir");
        let submodule = repo.join("vendor").join("lib");
        symlink("../../shared", submodule.join("src")).expect("link");
        commit(&submodule);

        assert_eq!(fixture.check(&submodule.join("src"), &repo), Ok(()));
    }

    /// A linked worktree has a `.git` file like a submodule. A repository in
    /// a directory above it, such as a repository of the home directory, is
    /// not its superproject.
    #[cfg(unix)]
    #[test]
    fn refuses_a_link_from_a_linked_worktree_into_an_unrelated_outer_repository() {
        let fixture = Fixture::new();
        let home = fixture.repo("home");
        let notes = fixture.dir("home/notes");
        let repo = fixture.repo("repo");
        symlink(&notes, repo.join("packages")).expect("link");
        commit(&repo);
        let worktree = home.join("work").join("wt");
        git(
            &repo,
            &["worktree", "add", "-q", worktree.to_str().unwrap()],
        );

        let err = fixture
            .check(&worktree.join("packages"), &worktree)
            .unwrap_err();
        assert!(err.contains("outside the checkout"), "got: {err}");
    }

    /// A `.git` entry that git does not accept as a repository does not end
    /// the search: git names the checkout that holds the link.
    #[cfg(unix)]
    #[test]
    fn refuses_a_committed_link_below_a_directory_with_an_invalid_git_entry() {
        let fixture = Fixture::new();
        let repo = fixture.repo("repo");
        let inner = fixture.dir("repo/inner");
        let outside = fixture.dir("outside");
        symlink(&outside, inner.join("packages")).expect("link");
        commit(&repo);
        std::fs::create_dir(inner.join(".git")).expect("empty git dir");

        let err = fixture.check(&inner.join("packages"), &repo).unwrap_err();
        assert!(err.contains("outside the checkout"), "got: {err}");
    }

    /// A repository with no commit, or with an empty index, is not a
    /// checkout, so the rule for a directory without a checkout applies.
    #[cfg(unix)]
    #[test]
    fn treats_a_repository_with_no_commit_or_an_empty_index_as_no_checkout() {
        for empty_index in [false, true] {
            let fixture = Fixture::new();
            let project = fixture.dir("project");
            git(&project, &["init", "-q", "-b", "main"]);
            let outside = fixture.dir("outside/app").parent().unwrap().to_path_buf();
            symlink(&outside, project.join("packages")).expect("link");
            git(&project, &["add", "-A"]);
            if empty_index {
                git(&project, &["commit", "-q", "-m", "commit"]);
                git(&project, &["rm", "-q", "-r", "--cached", "."]);
            }
            let root = project.join("packages").join("app");

            assert_eq!(fixture.check(&root, &project), Ok(()), "no manifest");
            std::fs::write(project.join("package.json"), "{}").expect("manifest");
            let err = fixture.check(&root, &project).unwrap_err();
            assert!(
                err.contains("outside the project directory"),
                "empty index {empty_index}: {err}"
            );
        }
    }

    /// A project unpacked from an archive has no `.git`. A link in the
    /// project directory that resolves outside it is refused.
    #[cfg(unix)]
    #[test]
    fn refuses_a_link_out_of_a_project_directory_without_a_checkout() {
        let fixture = Fixture::new();
        let project = fixture.dir("project");
        std::fs::write(project.join("package.json"), "{}").expect("manifest");
        let outside = fixture.dir("outside/app").parent().unwrap().to_path_buf();
        symlink(&outside, project.join("packages")).expect("link");

        let err = fixture
            .check(Path::new("packages/app"), &project)
            .unwrap_err();
        assert!(err.contains("outside the project directory"), "got: {err}");
        let absolute = project.join("packages").join("app");
        let err = fixture.check(&absolute, &project).unwrap_err();
        assert!(err.contains("outside the project directory"), "got: {err}");
        // The rule follows the project that holds the link, not the current
        // directory.
        let elsewhere = fixture.dir("elsewhere");
        let err = fixture.check(&absolute, &elsewhere).unwrap_err();
        assert!(err.contains("outside the project directory"), "got: {err}");
        // A link in a directory below the project counts for that project.
        let nested = fixture.dir("project/src");
        symlink(&outside, nested.join("lib")).expect("nested link");
        let err = fixture
            .check(&nested.join("lib").join("app"), &elsewhere)
            .unwrap_err();
        assert!(err.contains("outside the project directory"), "got: {err}");
    }

    /// A project that the index of an outer checkout does not track, such as
    /// a project in a home directory that is a repository, follows the rule
    /// for a project without a checkout.
    #[cfg(unix)]
    #[test]
    fn refuses_a_link_out_of_an_untracked_project_inside_a_checkout() {
        let fixture = Fixture::new();
        let home = fixture.repo("home");
        let notes = fixture.dir("home/notes");
        let project = fixture.dir("home/downloads/project");
        std::fs::write(project.join("package.json"), "{}").expect("manifest");
        symlink(&notes, project.join("packages")).expect("link");
        let elsewhere = fixture.dir("elsewhere");

        let err = fixture
            .check(&project.join("packages"), &elsewhere)
            .unwrap_err();
        assert!(err.contains("outside the project directory"), "got: {err}");
        assert_eq!(
            fixture.check(&home.join("notes"), &elsewhere),
            Ok(()),
            "a plain directory of the outer checkout stays allowed"
        );
    }

    /// In a monorepo, a tracked link from one package to another leaves the
    /// package but stays in the checkout.
    #[cfg(unix)]
    #[test]
    fn allows_a_link_out_of_a_tracked_project_that_stays_in_the_checkout() {
        let fixture = Fixture::new();
        let repo = fixture.repo("repo");
        let package = fixture.dir("repo/packages/a");
        std::fs::write(package.join("package.json"), "{}").expect("manifest");
        std::fs::create_dir_all(repo.join("packages").join("shared").join("src"))
            .expect("shared dir");
        std::fs::write(repo.join("packages/shared/src/index.ts"), "").expect("source");
        symlink("../shared", package.join("lib")).expect("link");
        commit(&repo);

        assert_eq!(
            fixture.check(&package.join("lib").join("src"), &repo),
            Ok(())
        );
    }

    /// A manifest in the home directory does not make the home directory a
    /// project, so a personal link under it that leaves it stays allowed. A
    /// manifest in a project below the home directory still counts.
    #[cfg(unix)]
    #[test]
    fn allows_a_link_under_a_home_directory_that_holds_a_manifest() {
        let fixture = Fixture::new();
        let home = fixture.dir("home");
        std::fs::write(home.join("package.json"), "{}").expect("home manifest");
        let volume = fixture
            .dir("volume/dev/app")
            .parent()
            .unwrap()
            .to_path_buf();
        let notes = fixture.dir("home/notes");
        symlink(&volume, notes.join("dev")).expect("link");
        let root = notes.join("dev").join("app");
        let check = |cwd: &Path| fixture.check_with_home(&root, cwd, None, Some(&home));

        assert_eq!(check(&home), Ok(()));
        std::fs::write(notes.join("package.json"), "{}").expect("project manifest");
        let err = check(&home).unwrap_err();
        assert!(err.contains("outside the project directory"), "got: {err}");
    }

    /// A directory that holds the home directory is never a project, also for
    /// a link outside the home directory, such as in the home directory of
    /// another user.
    #[cfg(unix)]
    #[test]
    fn ignores_a_manifest_in_a_directory_that_holds_the_home_directory() {
        let fixture = Fixture::new();
        let users = fixture.dir("users");
        let home = fixture.dir("users/me");
        std::fs::write(users.join("package.json"), "{}").expect("manifest");
        let volume = fixture.dir("volume/app").parent().unwrap().to_path_buf();
        let notes = fixture.dir("users/other/notes");
        symlink(&volume, notes.join("dev")).expect("link");
        let root = notes.join("dev").join("app");

        assert_eq!(
            fixture.check_with_home(&root, &home, None, Some(&home)),
            Ok(())
        );
    }

    /// A personal link in a directory with no project manifest, such as a
    /// link in the home directory to another volume, stays allowed.
    #[cfg(unix)]
    #[test]
    fn allows_a_link_out_of_a_current_directory_with_no_project_manifest() {
        let fixture = Fixture::new();
        let home = fixture.dir("home");
        let volume = fixture
            .dir("volume/dev/app")
            .parent()
            .unwrap()
            .to_path_buf();
        symlink(&volume, home.join("dev")).expect("link");

        assert_eq!(fixture.check(&home.join("dev").join("app"), &home), Ok(()));
        assert_eq!(fixture.check(Path::new("dev/app"), &home), Ok(()));
    }

    /// A shell that changed into `repo/packages/app` through a committed
    /// link reports the resolved current directory, and keeps the link in
    /// `PWD`.
    #[cfg(unix)]
    #[test]
    fn refuses_a_current_directory_reached_through_a_link() {
        let fixture = Fixture::new();
        let (repo, outside) = committed_outside_link(&fixture);
        let cwd = outside.join("app");
        let pwd = repo.join("packages").join("app");

        let err = fixture.check_with_pwd(&cwd, &cwd, Some(&pwd)).unwrap_err();
        assert!(err.contains("outside the checkout"), "got: {err}");
        let err = fixture
            .check_with_pwd(Path::new("."), &cwd, Some(&pwd))
            .unwrap_err();
        assert!(err.contains("outside the checkout"), "got: {err}");
        // A `PWD` that names another directory is not a spelling of the root.
        assert_eq!(fixture.check_with_pwd(&cwd, &cwd, Some(&repo)), Ok(()));
        assert_eq!(fixture.check_with_pwd(&cwd, &cwd, None), Ok(()));
    }

    #[test]
    fn validate_root_allows_a_plain_checkout_subdirectory() {
        let fixture = Fixture::new();
        let repo = fixture.repo("repo");
        let app = fixture.dir("repo/packages/app");

        assert_eq!(fixture.check(&app, &repo), Ok(()));
        assert!(validate_root(&app).is_ok());
    }

    #[test]
    fn control_chars_error_includes_position() {
        let result = validate_no_control_chars("ab\x01cd", "--test");
        let err = result.unwrap_err();
        assert!(err.contains("position 2"), "got: {err}");
        assert!(err.contains("--test"), "got: {err}");
    }
}
