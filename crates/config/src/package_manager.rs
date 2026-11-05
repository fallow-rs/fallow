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
    /// Detect the package manager of `root`.
    ///
    /// The search starts at `root` and goes up through its parent directories,
    /// so a workspace package finds the lockfile of its monorepo root. The
    /// nearest directory with a signal decides. In that directory, the
    /// `packageManager` field of `package.json` wins over a lockfile, and
    /// lockfiles count in the order Bun, pnpm, Yarn, npm. The search stops
    /// after a directory that contains `.git`, so it does not read files
    /// outside the repository. Returns `None` when no signal exists.
    #[must_use]
    pub fn detect(root: &Path) -> Option<Self> {
        for dir in root.ancestors() {
            let found = Self::from_package_json_field(dir).or_else(|| Self::from_lockfile(dir));
            if found.is_some() {
                return found;
            }
            if dir.join(".git").exists() {
                break;
            }
        }
        None
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

    /// The command that adds `package` as a development dependency.
    #[must_use]
    pub fn add_dev_command(self, package: &str) -> String {
        match self {
            Self::Npm => format!("npm install --save-dev {package}"),
            Self::Pnpm => format!("pnpm add -D {package}"),
            Self::Yarn => format!("yarn add -D {package}"),
            Self::Bun => format!("bun add -d {package}"),
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
        std::fs::create_dir(root.path().join(".git")).expect("create .git");
        assert_eq!(PackageManager::detect(root.path()), None);
    }

    #[test]
    fn workspace_package_uses_the_monorepo_root_signal() {
        let root = tempfile::tempdir().expect("temp root");
        let package = root.path().join("packages").join("app");
        std::fs::create_dir_all(&package).expect("create package");
        std::fs::write(package.join("package.json"), r#"{"name":"app"}"#)
            .expect("write package.json");
        std::fs::write(root.path().join("pnpm-lock.yaml"), "").expect("write lockfile");
        assert_eq!(PackageManager::detect(&package), Some(PackageManager::Pnpm));
    }

    #[test]
    fn nearest_signal_wins_over_a_parent_signal() {
        let root = tempfile::tempdir().expect("temp root");
        let nested = root.path().join("tools");
        std::fs::create_dir(&nested).expect("create nested");
        std::fs::write(root.path().join("pnpm-lock.yaml"), "").expect("write root lockfile");
        std::fs::write(nested.join("yarn.lock"), "").expect("write nested lockfile");
        assert_eq!(PackageManager::detect(&nested), Some(PackageManager::Yarn));
    }

    #[test]
    fn search_stops_at_the_repository_boundary() {
        let outer = tempfile::tempdir().expect("temp root");
        let repo = outer.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).expect("create repo");
        std::fs::write(outer.path().join("yarn.lock"), "").expect("write outer lockfile");
        assert_eq!(PackageManager::detect(&repo), None);
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
        assert_eq!(PackageManager::Yarn.install_command(), "yarn install");
        assert_eq!(
            PackageManager::Npm.add_dev_command("x"),
            "npm install --save-dev x"
        );
        assert_eq!(PackageManager::Bun.add_dev_command("x"), "bun add -d x");
    }
}
