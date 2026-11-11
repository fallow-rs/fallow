//! Detection of the package manager that a project uses.

use std::path::Path;

/// A JavaScript package manager that Fallow can name in a suggested command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    /// npm.
    Npm,
    /// pnpm.
    Pnpm,
    /// Yarn.
    Yarn,
    /// Bun.
    Bun,
}

impl PackageManager {
    /// Detect the package manager from the files in `root` only.
    ///
    /// The `packageManager` field of `package.json` wins over a lockfile, and
    /// lockfiles count in the order Bun, pnpm, Yarn, npm. Returns `None` when
    /// neither signal exists. Use this variant when the result selects a
    /// command that Fallow runs: files outside `root` must not choose it.
    #[must_use]
    pub fn detect(root: &Path) -> Option<Self> {
        Self::from_package_json_field(root).or_else(|| Self::from_lockfile(root))
    }

    /// Detect the package manager for a command that Fallow only suggests.
    ///
    /// When `root` is inside a git repository, the search goes up from `root`
    /// to the repository root, so a workspace package finds the signal of its
    /// monorepo root. The nearest directory with a signal decides, by the
    /// rules of [`Self::detect`]. Outside a git repository, only `root` counts.
    /// Never use the result to select a command that Fallow runs.
    #[must_use]
    pub fn detect_nearest(root: &Path) -> Option<Self> {
        let Some(repo_depth) = root.ancestors().position(|dir| dir.join(".git").exists()) else {
            return Self::detect(root);
        };
        root.ancestors().take(repo_depth + 1).find_map(Self::detect)
    }

    /// The command that a hint shows to add `package` as a development
    /// dependency of the project in `project_root`.
    ///
    /// The package manager comes from [`Self::detect_nearest`]. npm is the
    /// default when no signal names one or when `project_root` is `None`.
    /// Never run the result: files above `project_root` can select it.
    #[must_use]
    pub fn suggested_add_dev_command(project_root: Option<&Path>, package: &str) -> String {
        let Some(root) = project_root else {
            return Self::Npm.add_dev_command(Path::new(""), package);
        };
        Self::detect_nearest(root)
            .unwrap_or(Self::Npm)
            .add_dev_command(root, package)
    }

    /// The executable name, which is also the `packageManager` field prefix.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }

    /// The command that installs the declared dependencies.
    #[must_use]
    pub fn install_command(self) -> String {
        format!("{} install", self.name())
    }

    /// The command that adds `package` as a development dependency when the
    /// user runs it in `dir`.
    ///
    /// At the root of a Yarn 1 workspace project, the command has `-W`.
    /// Without it, Yarn 1 refuses to add a dependency to the workspace root.
    #[must_use]
    pub fn add_dev_command(self, dir: &Path, package: &str) -> String {
        match self {
            Self::Npm => format!("npm install --save-dev {package}"),
            Self::Pnpm => format!("pnpm add -D {package}"),
            Self::Yarn if self.needs_workspace_root_flag(dir) => {
                format!("yarn add -D -W {package}")
            }
            Self::Yarn => format!("yarn add -D {package}"),
            Self::Bun => format!("bun add -d {package}"),
        }
    }

    /// Whether `add` in `dir` needs the `-W` flag of Yarn 1.
    ///
    /// This is true only for Yarn 1 in a directory whose `package.json` has a
    /// `workspaces` field. A `packageManager` field that names Yarn decides
    /// the version: `yarn@1.` is Yarn 1, and each other version is not. With
    /// no such field, a `yarn.lock` with the Yarn 1 header marks Yarn 1.
    /// Yarn Berry rejects `-W`, so an unknown Yarn version gets no flag.
    #[must_use]
    pub fn needs_workspace_root_flag(self, dir: &Path) -> bool {
        if self != Self::Yarn {
            return false;
        }
        let manifest = std::fs::read_to_string(dir.join("package.json"))
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());
        let Some(manifest) = manifest else {
            return false;
        };
        if manifest
            .get("workspaces")
            .is_none_or(|workspaces| workspaces.is_null() || workspaces == false)
        {
            return false;
        }
        match manifest
            .get("packageManager")
            .and_then(serde_json::Value::as_str)
        {
            Some(field) if field.starts_with("yarn@") => field.starts_with("yarn@1."),
            _ => has_yarn_classic_lockfile(dir),
        }
    }

    fn from_package_json_field(root: &Path) -> Option<Self> {
        let content = std::fs::read_to_string(root.join("package.json")).ok()?;
        let value: serde_json::Value = serde_json::from_str(&content).ok()?;
        let field = value.get("packageManager")?.as_str()?;
        match field.split('@').next().unwrap_or(field) {
            "npm" => Some(Self::Npm),
            "pnpm" => Some(Self::Pnpm),
            "yarn" => Some(Self::Yarn),
            "bun" => Some(Self::Bun),
            _ => None,
        }
    }

    fn from_lockfile(root: &Path) -> Option<Self> {
        const LOCKFILES: [(&str, PackageManager); 6] = [
            ("bun.lockb", PackageManager::Bun),
            ("bun.lock", PackageManager::Bun),
            ("pnpm-lock.yaml", PackageManager::Pnpm),
            ("yarn.lock", PackageManager::Yarn),
            ("package-lock.json", PackageManager::Npm),
            ("npm-shrinkwrap.json", PackageManager::Npm),
        ];
        LOCKFILES
            .iter()
            .find(|(lockfile, _)| root.join(lockfile).exists())
            .map(|&(_, manager)| manager)
    }
}

/// Yarn 1 writes this header near the top of `yarn.lock`. A Yarn Berry
/// lockfile is YAML with a `__metadata` entry instead.
const YARN_CLASSIC_LOCKFILE_HEADER: &str = "# yarn lockfile v1";
const LOCKFILE_HEADER_BYTES: u64 = 512;

fn has_yarn_classic_lockfile(dir: &Path) -> bool {
    use std::io::Read as _;
    let lockfile = dir.join("yarn.lock");
    // A FIFO named `yarn.lock` would block the read.
    if !lockfile.is_file() {
        return false;
    }
    let Ok(file) = std::fs::File::open(lockfile) else {
        return false;
    };
    let mut head = Vec::new();
    if file
        .take(LOCKFILE_HEADER_BYTES)
        .read_to_end(&mut head)
        .is_err()
    {
        return false;
    }
    String::from_utf8_lossy(&head)
        .lines()
        .any(|line| line.trim_end() == YARN_CLASSIC_LOCKFILE_HEADER)
}

#[cfg(test)]
mod tests {
    use super::PackageManager;

    #[test]
    fn field_wins_over_lockfile() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(
            root.path().join("package.json"),
            r#"{"packageManager":"pnpm@10.0.0"}"#,
        )
        .expect("write package.json");
        std::fs::write(root.path().join("package-lock.json"), "").expect("write lockfile");
        assert_eq!(
            PackageManager::detect(root.path()),
            Some(PackageManager::Pnpm)
        );
    }

    #[test]
    fn unknown_field_falls_back_to_lockfile() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(
            root.path().join("package.json"),
            r#"{"packageManager":"deno@2.0.0"}"#,
        )
        .expect("write package.json");
        std::fs::write(root.path().join("yarn.lock"), "").expect("write lockfile");
        assert_eq!(
            PackageManager::detect(root.path()),
            Some(PackageManager::Yarn)
        );
    }

    #[test]
    fn lockfile_order_prefers_bun_then_pnpm_then_yarn() {
        let root = tempfile::tempdir().expect("temp root");
        for lockfile in ["yarn.lock", "pnpm-lock.yaml", "bun.lockb"] {
            std::fs::write(root.path().join(lockfile), "").expect("write lockfile");
        }
        assert_eq!(
            PackageManager::detect(root.path()),
            Some(PackageManager::Bun)
        );
    }

    #[test]
    fn no_signal_is_none() {
        let root = tempfile::tempdir().expect("temp root");
        assert_eq!(PackageManager::detect(root.path()), None);
    }

    #[test]
    fn workspace_package_uses_the_monorepo_root_signal() {
        let root = tempfile::tempdir().expect("temp root");
        let package = root.path().join("packages").join("app");
        std::fs::create_dir_all(&package).expect("create package");
        std::fs::write(package.join("package.json"), r#"{"name":"app"}"#)
            .expect("write package.json");
        std::fs::create_dir(root.path().join(".git")).expect("create .git");
        std::fs::write(root.path().join("pnpm-lock.yaml"), "").expect("write lockfile");
        assert_eq!(
            PackageManager::detect_nearest(&package),
            Some(PackageManager::Pnpm)
        );
    }

    /// Security review finding: a file above the project root must not select
    /// a command that Fallow runs.
    #[test]
    fn detect_ignores_parent_directories() {
        let root = tempfile::tempdir().expect("temp root");
        let package = root.path().join("packages").join("app");
        std::fs::create_dir_all(&package).expect("create package");
        std::fs::create_dir(root.path().join(".git")).expect("create .git");
        std::fs::write(root.path().join("pnpm-lock.yaml"), "").expect("write lockfile");
        assert_eq!(PackageManager::detect(&package), None);
    }

    #[test]
    fn detect_nearest_outside_a_repository_reads_only_the_root() {
        let outer = tempfile::tempdir().expect("temp root");
        let project = outer.path().join("project");
        std::fs::create_dir(&project).expect("create project");
        std::fs::write(outer.path().join("yarn.lock"), "").expect("write outer lockfile");
        assert_eq!(PackageManager::detect_nearest(&project), None);
    }

    #[test]
    fn nearest_signal_wins_over_a_parent_signal() {
        let root = tempfile::tempdir().expect("temp root");
        let nested = root.path().join("tools");
        std::fs::create_dir(&nested).expect("create nested");
        std::fs::write(root.path().join("pnpm-lock.yaml"), "").expect("write root lockfile");
        std::fs::create_dir(root.path().join(".git")).expect("create .git");
        std::fs::write(nested.join("yarn.lock"), "").expect("write nested lockfile");
        assert_eq!(
            PackageManager::detect_nearest(&nested),
            Some(PackageManager::Yarn)
        );
    }

    #[test]
    fn search_stops_at_the_repository_boundary() {
        let outer = tempfile::tempdir().expect("temp root");
        let repo = outer.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("create repo");
        std::fs::write(outer.path().join("yarn.lock"), "").expect("write outer lockfile");
        assert_eq!(PackageManager::detect_nearest(&repo), None);
    }

    #[test]
    fn shrinkwrap_alone_is_npm() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(root.path().join("npm-shrinkwrap.json"), "").expect("write lockfile");
        assert_eq!(
            PackageManager::detect(root.path()),
            Some(PackageManager::Npm)
        );
    }

    #[test]
    fn malformed_package_json_falls_back_to_lockfile() {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(root.path().join("package.json"), "{ not json").expect("write package.json");
        std::fs::write(root.path().join("bun.lock"), "").expect("write lockfile");
        assert_eq!(
            PackageManager::detect(root.path()),
            Some(PackageManager::Bun)
        );
    }

    #[test]
    fn commands_match_each_manager() {
        let root = tempfile::tempdir().expect("temp root");
        assert_eq!(PackageManager::Yarn.install_command(), "yarn install");
        assert_eq!(
            PackageManager::Npm.add_dev_command(root.path(), "x"),
            "npm install --save-dev x"
        );
        assert_eq!(
            PackageManager::Bun.add_dev_command(root.path(), "x"),
            "bun add -d x"
        );
        assert_eq!(
            PackageManager::Yarn.add_dev_command(root.path(), "x"),
            "yarn add -D x"
        );
    }

    const YARN_CLASSIC_LOCKFILE: &str = "# THIS IS AN AUTOGENERATED FILE. DO NOT EDIT THIS FILE DIRECTLY.\n# yarn lockfile v1\n\n\n";
    const YARN_BERRY_LOCKFILE: &str = "# This file is generated by running \"yarn install\" inside your project.\n\n__metadata:\n  version: 8\n";

    fn yarn_root(manifest: &str, lockfile: Option<&str>) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("temp root");
        std::fs::write(root.path().join("package.json"), manifest).expect("write package.json");
        if let Some(lockfile) = lockfile {
            std::fs::write(root.path().join("yarn.lock"), lockfile).expect("write lockfile");
        }
        root
    }

    /// Yarn 1 refuses `yarn add -D` at a workspace root without `-W`
    /// (fallow-rs/fallow#4519).
    #[test]
    fn yarn_classic_workspace_root_adds_the_root_flag() {
        let root = yarn_root(
            r#"{"private":true,"workspaces":["packages/*"]}"#,
            Some(YARN_CLASSIC_LOCKFILE),
        );
        assert_eq!(
            PackageManager::Yarn.add_dev_command(root.path(), "x"),
            "yarn add -D -W x"
        );
    }

    #[test]
    fn yarn_classic_package_manager_field_marks_the_root() {
        let root = yarn_root(
            r#"{"packageManager":"yarn@1.22.22","workspaces":{"packages":["packages/*"]}}"#,
            None,
        );
        assert!(PackageManager::Yarn.needs_workspace_root_flag(root.path()));
    }

    #[test]
    fn yarn_berry_workspace_root_has_no_root_flag() {
        let by_lockfile = yarn_root(
            r#"{"workspaces":["packages/*"]}"#,
            Some(YARN_BERRY_LOCKFILE),
        );
        assert!(!PackageManager::Yarn.needs_workspace_root_flag(by_lockfile.path()));
        let by_field = yarn_root(
            r#"{"packageManager":"yarn@4.5.0","workspaces":["packages/*"]}"#,
            Some(YARN_CLASSIC_LOCKFILE),
        );
        assert!(!PackageManager::Yarn.needs_workspace_root_flag(by_field.path()));
    }

    #[test]
    fn yarn_classic_without_workspaces_has_no_root_flag() {
        let root = yarn_root(r#"{"name":"app"}"#, Some(YARN_CLASSIC_LOCKFILE));
        assert!(!PackageManager::Yarn.needs_workspace_root_flag(root.path()));
    }

    #[test]
    fn unknown_yarn_version_has_no_root_flag() {
        let root = yarn_root(r#"{"workspaces":["packages/*"]}"#, None);
        assert!(!PackageManager::Yarn.needs_workspace_root_flag(root.path()));
    }

    #[test]
    fn package_manager_field_with_a_hash_marks_yarn_classic() {
        let root = yarn_root(
            r#"{"packageManager":"yarn@1.22.22+sha512.abc","workspaces":["packages/*"]}"#,
            None,
        );
        assert!(PackageManager::Yarn.needs_workspace_root_flag(root.path()));
    }

    #[test]
    fn crlf_lockfile_marks_yarn_classic() {
        let root = yarn_root(
            r#"{"workspaces":["packages/*"]}"#,
            Some("# THIS IS AN AUTOGENERATED FILE.\r\n# yarn lockfile v1\r\n"),
        );
        assert!(PackageManager::Yarn.needs_workspace_root_flag(root.path()));
    }

    #[test]
    fn null_or_false_workspaces_is_not_a_workspace_root() {
        for workspaces in ["null", "false"] {
            let root = yarn_root(
                &format!(r#"{{"workspaces":{workspaces}}}"#),
                Some(YARN_CLASSIC_LOCKFILE),
            );
            assert!(
                !PackageManager::Yarn.needs_workspace_root_flag(root.path()),
                "{workspaces}"
            );
        }
    }

    /// Yarn 1 needs `-W` only at the workspace root. A workspace package of
    /// the same project gets the plain command.
    #[test]
    fn suggested_command_in_a_yarn_classic_workspace_package_has_no_root_flag() {
        let root = yarn_root(
            r#"{"private":true,"workspaces":["packages/*"]}"#,
            Some(YARN_CLASSIC_LOCKFILE),
        );
        std::fs::create_dir(root.path().join(".git")).expect("create .git");
        let package = root.path().join("packages").join("app");
        std::fs::create_dir_all(&package).expect("create package");
        std::fs::write(package.join("package.json"), r#"{"name":"app"}"#)
            .expect("write package.json");
        assert_eq!(
            PackageManager::suggested_add_dev_command(Some(&package), "x"),
            "yarn add -D x"
        );
        assert_eq!(
            PackageManager::suggested_add_dev_command(Some(root.path()), "x"),
            "yarn add -D -W x"
        );
    }

    #[test]
    fn suggested_command_without_a_root_is_npm() {
        assert_eq!(
            PackageManager::suggested_add_dev_command(None, "x"),
            "npm install --save-dev x"
        );
    }

    #[test]
    fn only_yarn_takes_the_root_flag() {
        let root = yarn_root(
            r#"{"workspaces":["packages/*"]}"#,
            Some(YARN_CLASSIC_LOCKFILE),
        );
        assert_eq!(
            PackageManager::Pnpm.add_dev_command(root.path(), "x"),
            "pnpm add -D x"
        );
    }
}
