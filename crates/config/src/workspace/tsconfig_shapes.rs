//! The `extends` shapes that every merged-tsconfig consumer must handle.
//!
//! Each consumer of [`super::MergedTsconfig`] runs its own field through every
//! [`TsconfigExtendsShape`], so a consumer that reads a tsconfig without the
//! merged reader fails on the first shape that its own reader gets wrong.
//!
//! Each shape writes `tsconfig.json` in a project folder. The settings that
//! the consumer reads come only from a parent config, and they point back to
//! the project folder:
//!
//! | Option | Value | Resolves to |
//! |---|---|---|
//! | `jsx` | `react-jsx` | |
//! | `baseUrl` | the project folder | |
//! | `paths` | `@app/*` to `src/*` | `<project>/src/*` |
//! | `typeRoots` | `types` | `<project>/types` |
//! | `rootDir`, `outDir` | `src`, `dist` | `<project>/src`, `<project>/dist` |
//! | `include` | `src` | `<project>/src` |
//!
//! A shape with an override or a broken entry also writes a config whose
//! values point to `<project>/wrong`. A consumer that reads that config, or
//! no parent at all, gets a wrong value.
#![allow(
    clippy::expect_used,
    reason = "a test fixture writer stops the test when a file cannot be written"
)]

use std::path::Path;

/// One shape of an `extends` chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TsconfigExtendsShape {
    /// `"extends": "./configs/base.json"`.
    Relative,
    /// `"extends": "@acme/tsconfig"`, resolved through `node_modules`.
    Package,
    /// `"extends": "@acme/tsconfig/react"`, resolved through the package
    /// `exports`.
    PackageSubpath,
    /// `"extends": ["./configs/wrong.json", "./configs/base.json"]`: the
    /// later entry overrides the earlier one.
    Array,
    /// The parent extends the leaf again.
    Cycle,
    /// An array whose first entry does not exist.
    MissingParent,
    /// An array whose last entry does not parse.
    MalformedParent,
    /// `"extends": "@acme/tsconfig"`, whose path values all start with
    /// `${configDir}`, the usual form of a shared config.
    ConfigDirPackage,
}

impl TsconfigExtendsShape {
    /// Every shape, in a fixed order.
    pub const ALL: [Self; 8] = [
        Self::Relative,
        Self::Package,
        Self::PackageSubpath,
        Self::Array,
        Self::Cycle,
        Self::MissingParent,
        Self::MalformedParent,
        Self::ConfigDirPackage,
    ];

    /// A short name for test messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Relative => "relative",
            Self::Package => "package",
            Self::PackageSubpath => "package subpath",
            Self::Array => "array",
            Self::Cycle => "cycle",
            Self::MissingParent => "missing parent",
            Self::MalformedParent => "malformed parent",
            Self::ConfigDirPackage => "configDir package",
        }
    }

    /// True when every config of the chain resolves and parses.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        matches!(
            self,
            Self::Relative
                | Self::Package
                | Self::PackageSubpath
                | Self::Array
                | Self::ConfigDirPackage
        )
    }

    /// Write the shape below `project`: `project/tsconfig.json` and its
    /// parents.
    ///
    /// # Panics
    ///
    /// Panics when a file cannot be written.
    pub fn write(self, project: &Path) {
        match self {
            Self::Relative => {
                write_settings(project, "configs/base.json", "../", "", None);
                write_leaf(project, r#""./configs/base.json""#);
            }
            Self::Package => {
                write_file(
                    project,
                    "node_modules/@acme/tsconfig/package.json",
                    r#"{"name":"@acme/tsconfig","version":"1.0.0"}"#,
                );
                write_settings(
                    project,
                    "node_modules/@acme/tsconfig/tsconfig.json",
                    "../../../",
                    "",
                    None,
                );
                write_leaf(project, r#""@acme/tsconfig""#);
            }
            Self::PackageSubpath => {
                write_file(
                    project,
                    "node_modules/@acme/tsconfig/package.json",
                    r#"{"name":"@acme/tsconfig","version":"1.0.0","exports":{"./react":"./presets/react.json"}}"#,
                );
                write_settings(
                    project,
                    "node_modules/@acme/tsconfig/presets/react.json",
                    "../../../../",
                    "",
                    None,
                );
                write_leaf(project, r#""@acme/tsconfig/react""#);
            }
            Self::Array => {
                write_settings(
                    project,
                    "configs/wrong.json",
                    "../",
                    "wrong/",
                    Some("preserve"),
                );
                write_settings(project, "configs/base.json", "../", "", None);
                write_leaf(project, r#"["./configs/wrong.json","./configs/base.json"]"#);
            }
            Self::Cycle => {
                write_settings_with_extends(
                    project,
                    "configs/base.json",
                    "../",
                    Some(r#""../tsconfig.json""#),
                );
                write_leaf(project, r#""./configs/base.json""#);
            }
            Self::MissingParent => {
                write_settings(project, "configs/base.json", "../", "", None);
                write_leaf(
                    project,
                    r#"["./configs/missing.json","./configs/base.json"]"#,
                );
            }
            Self::ConfigDirPackage => {
                write_file(
                    project,
                    "node_modules/@acme/tsconfig/package.json",
                    r#"{"name":"@acme/tsconfig","version":"1.0.0"}"#,
                );
                write_settings(
                    project,
                    "node_modules/@acme/tsconfig/tsconfig.json",
                    "${configDir}/",
                    "",
                    None,
                );
                write_leaf(project, r#""@acme/tsconfig""#);
            }
            Self::MalformedParent => {
                write_settings(project, "configs/base.json", "../", "", None);
                write_file(project, "configs/broken.json", "{ \"compilerOptions\": ");
                write_leaf(
                    project,
                    r#"["./configs/base.json","./configs/broken.json"]"#,
                );
            }
        }
    }
}

fn write_leaf(project: &Path, extends: &str) {
    write_file(
        project,
        "tsconfig.json",
        &format!(r#"{{"extends":{extends}}}"#),
    );
}

/// A config with every setting of the table. `up` climbs from the config
/// folder to the project folder, and `prefix` puts each target below a
/// subfolder, such as `wrong/`.
fn write_settings(project: &Path, file: &str, up: &str, prefix: &str, jsx: Option<&str>) {
    let jsx = jsx.unwrap_or("react-jsx");
    let target = |name: &str| {
        if prefix.is_empty() {
            format!("{up}{name}")
        } else {
            format!("{up}{}", prefix.trim_end_matches('/'))
        }
    };
    let paths_target = if prefix.is_empty() {
        "src/*"
    } else {
        "wrong/*"
    };
    let content = format!(
        r#"{{
  "compilerOptions": {{
    "jsx": "{jsx}",
    "baseUrl": "{base}",
    "paths": {{ "@app/*": ["{paths_target}"] }},
    "typeRoots": ["{types}"],
    "rootDir": "{root_dir}",
    "outDir": "{out_dir}"
  }},
  "include": ["{include}"]
}}"#,
        base = if up.is_empty() { "." } else { up },
        types = target("types"),
        root_dir = target("src"),
        out_dir = target("dist"),
        include = target("src"),
    );
    write_file(project, file, &content);
}

fn write_settings_with_extends(project: &Path, file: &str, up: &str, extends: Option<&str>) {
    write_settings(project, file, up, "", None);
    if let Some(extends) = extends {
        let path = project.join(file);
        let content = std::fs::read_to_string(&path).expect("read settings");
        let content = content.replacen('{', &format!("{{\n  \"extends\": {extends},"), 1);
        std::fs::write(path, content).expect("write settings");
    }
}

fn write_file(project: &Path, file: &str, content: &str) {
    let path = project.join(file);
    std::fs::create_dir_all(path.parent().expect("parent folder")).expect("create folder");
    std::fs::write(path, content).expect("write file");
}
