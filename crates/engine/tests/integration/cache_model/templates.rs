//! Project templates and the edit steps that apply to them.

use super::project::Project;

/// A start project and the steps that make sense for it.
pub struct Template {
    pub name: &'static str,
    pub files: &'static [(&'static str, &'static str)],
    pub steps: &'static [Step],
}

/// One edit of the project, or of the analysis options.
///
/// Each step is a toggle between two states, so every step applies to every
/// state of its template and a generated sequence never needs a guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Swap `alpha` and `bravo` in the entry file. Both spellings have the
    /// same length. With `keep_mtime`, the modification time stays the same,
    /// so only the content shows the change.
    SwapUtilUse { keep_mtime: bool },
    /// Give a source file a new modification time, with the same content.
    TouchSource,
    /// Break or restore the clone in `src/dupe-b.ts`, with the same length.
    EditClone { keep_mtime: bool },
    /// Add or delete an unused source file that imports `bravo`.
    ToggleExtraFile,
    /// Move the orphan file to another directory, or back.
    RenameOrphan,
    /// Change the lockfile, with no change to `node_modules`.
    EditLockfile,
    /// Install or remove `plain-pkg`, with the lockfile and the install
    /// marker, as `npm install` does.
    ToggleNpmInstall,
    /// Install or remove `plain-pkg` in `node_modules` only. The Bun
    /// lockfile already lists the package, as after a pull of a lockfile
    /// change, and Bun writes no install marker.
    ToggleBunInstall,
    /// Write a tool cache file and a package readme in `node_modules`.
    NodeModulesNoise,
    /// Swap the two targets of the conditional `#db` entry of `package.json`
    /// `imports`.
    SwapPackageImports,
    /// Swap the names of two workspace packages.
    SwapWorkspaceNames,
    /// Add or remove a conditional `exports` map in a workspace package.
    ToggleLibExports,
    /// Point the alias in the extended `tsconfig` base to the other file.
    SwapExtendedTsconfig,
    /// Point the alias in a nested `tsconfig.json` to the other file.
    SwapNestedTsconfig,
    /// Point the Deno import map to the other file.
    SwapDenoImportMap,
    /// Turn production mode on or off.
    ToggleProduction,
    /// Add or remove an ignore pattern for the orphan file.
    ToggleIgnoreOrphan,
    /// Add or remove the `react-server` resolve condition. The condition
    /// selects the other target of the `#db` entry of `package.json`
    /// `imports`.
    ToggleCondition,
    /// Point the `resolve.alias` entry of `vite.config.ts` to the other
    /// directory. With `keep_mtime`, the length and the modification time
    /// stay the same.
    SwapViteAlias { keep_mtime: bool },
    /// Point the Vitest `setupFiles` entry to the other setup file.
    SwapVitestSetup,
    /// Point the Vitest `include` glob to the other test file suffix.
    SwapVitestInclude,
    /// Point the Sass `includePaths` entry of `angular.json` to the other
    /// theme directory.
    SwapAngularIncludePath,
    /// Point the `main` entry of `angular.json` to the other file.
    SwapAngularMain,
}

const SOURCE_STEPS: [Step; 7] = [
    Step::SwapUtilUse { keep_mtime: false },
    Step::SwapUtilUse { keep_mtime: true },
    Step::TouchSource,
    Step::EditClone { keep_mtime: false },
    Step::EditClone { keep_mtime: true },
    Step::ToggleExtraFile,
    Step::RenameOrphan,
];

const CLONE_A: &str = "export const sumA = (items: number[]): number => {\n  let total = 0;\n  for (const item of items) {\n    total += item * 2;\n    total -= 1;\n  }\n  return total;\n};\n";
const CLONE_B: &str = "export const sumB = (items: number[]): number => {\n  let total = 0;\n  for (const item of items) {\n    total += item * 2;\n    total -= 1;\n  }\n  return total;\n};\n";
const CLONE_B_BROKEN: &str = "export const sumB = (items: number[]): number => {\n  let total = 0;\n  while (items.length>0) {\n    total = items.pop() ?? 10;\n    break;\n  }\n  return total;\n};\n";

const ENTRY_ALPHA: &str = "import { db } from '#db';\nimport { alias } from '@alias';\nimport { alpha } from './util';\nimport { value } from '@demo/lib/src/util';\nimport { nested } from '../nested/entry';\nimport asset from 'plain-pkg/build/missing.mjs?url';\n\nexport const run = (): unknown[] => [db, alias, alpha(), value, nested, asset];\n";
const BUN_ENTRY_ALPHA: &str = "import { alpha } from './util';\nimport asset from 'plain-pkg/build/missing.mjs?url';\n\nexport const run = (): unknown[] => [alpha(), asset];\n";
const DENO_ENTRY_ALPHA: &str = "import { db } from 'db';\nimport { alpha } from './src/util.ts';\n\nexport const run = (): unknown[] => [db, alpha()];\n";

const UTIL: &str = "export const alpha = (): number => 1;\nexport const bravo = (): number => 2;\n";

const NPM_LOCK: &str = r#"{"name":"app","lockfileVersion":3,"packages":{"node_modules/other-pkg":{"version":"1.0.0"}}}"#;
const NPM_LOCK_INSTALLED: &str = r#"{"name":"app","lockfileVersion":3,"packages":{"node_modules/other-pkg":{"version":"1.0.0"},"node_modules/plain-pkg":{"version":"1.0.0"}}}"#;

/// A workspace project with npm install state, package imports, extended
/// and nested `tsconfig` aliases, and a Vite asset query import.
pub const NODE: Template = Template {
    name: "node",
    files: &[
        (
            "package.json",
            r##"{"name":"app","private":true,"workspaces":["packages/*"],"main":"src/index.ts","imports":{"#db":{"react-server":"./src/db-b.ts","default":"./src/db-a.ts"}},"dependencies":{"plain-pkg":"1.0.0","other-pkg":"1.0.0","@demo/lib":"*"},"devDependencies":{"vite":"*"}}"##,
        ),
        ("package-lock.json", NPM_LOCK),
        ("node_modules/.package-lock.json", NPM_LOCK),
        (
            "node_modules/other-pkg/package.json",
            r#"{"name":"other-pkg","version":"1.0.0","main":"index.js"}"#,
        ),
        ("node_modules/other-pkg/index.js", "module.exports = {};\n"),
        (
            "tsconfig.json",
            r#"{"extends":"./config/tsconfig.base.json"}"#,
        ),
        (
            "config/tsconfig.base.json",
            r#"{"compilerOptions":{"baseUrl":"..","paths":{"@alias":["./src/alias-a.ts"]}}}"#,
        ),
        ("src/index.ts", ENTRY_ALPHA),
        ("src/util.ts", UTIL),
        ("src/db-a.ts", "export const db = 'a';\n"),
        ("src/db-b.ts", "export const db = 'b';\n"),
        ("src/alias-a.ts", "export const alias = 'a';\n"),
        ("src/alias-b.ts", "export const alias = 'b';\n"),
        ("src/orphan.ts", "export const orphan = 1;\n"),
        ("src/dupe-a.ts", CLONE_A),
        ("src/dupe-b.ts", CLONE_B),
        (
            "nested/tsconfig.json",
            r#"{"compilerOptions":{"paths":{"@n":["./n-a.ts"]}}}"#,
        ),
        (
            "nested/entry.ts",
            "import { n } from '@n';\nexport const nested = n;\n",
        ),
        ("nested/n-a.ts", "export const n = 'a';\n"),
        ("nested/n-b.ts", "export const n = 'b';\n"),
        ("packages/lib/package.json", r#"{"name":"@demo/lib"}"#),
        ("packages/lib/src/util.ts", "export const value = 'lib';\n"),
        ("packages/lib/src/alt.ts", "export const value = 'alt';\n"),
        ("packages/other/package.json", r#"{"name":"@demo/other"}"#),
        (
            "packages/other/src/util.ts",
            "export const value = 'other';\n",
        ),
    ],
    steps: &[
        SOURCE_STEPS[0],
        SOURCE_STEPS[1],
        SOURCE_STEPS[2],
        SOURCE_STEPS[3],
        SOURCE_STEPS[4],
        SOURCE_STEPS[5],
        SOURCE_STEPS[6],
        Step::EditLockfile,
        Step::ToggleNpmInstall,
        Step::NodeModulesNoise,
        Step::SwapPackageImports,
        Step::SwapWorkspaceNames,
        Step::ToggleLibExports,
        Step::SwapExtendedTsconfig,
        Step::SwapNestedTsconfig,
        Step::ToggleProduction,
        Step::ToggleIgnoreOrphan,
        Step::ToggleCondition,
    ],
};

/// A Bun project whose lockfile already lists the package that a step
/// installs.
pub const BUN: Template = Template {
    name: "bun",
    files: &[
        (
            "package.json",
            r#"{"name":"bun-app","private":true,"main":"src/index.ts","dependencies":{"plain-pkg":"1.0.0","other-pkg":"1.0.0"},"devDependencies":{"vite":"*"}}"#,
        ),
        ("bun.lock", "other-pkg@1.0.0\nplain-pkg@1.0.0\n"),
        (
            "node_modules/other-pkg/package.json",
            r#"{"name":"other-pkg","version":"1.0.0","main":"index.js"}"#,
        ),
        ("node_modules/other-pkg/index.js", "module.exports = {};\n"),
        ("src/index.ts", BUN_ENTRY_ALPHA),
        ("src/util.ts", UTIL),
        ("src/orphan.ts", "export const orphan = 1;\n"),
        ("src/dupe-a.ts", CLONE_A),
        ("src/dupe-b.ts", CLONE_B),
    ],
    steps: &[
        SOURCE_STEPS[0],
        SOURCE_STEPS[1],
        SOURCE_STEPS[2],
        SOURCE_STEPS[3],
        SOURCE_STEPS[4],
        SOURCE_STEPS[5],
        SOURCE_STEPS[6],
        Step::ToggleBunInstall,
        Step::NodeModulesNoise,
        Step::ToggleProduction,
        Step::ToggleIgnoreOrphan,
    ],
};

/// A Deno project with an import map in `deno.json`.
pub const DENO: Template = Template {
    name: "deno",
    files: &[
        (
            "deno.json",
            r#"{"name":"deno-app","exports":"./main.ts","imports":{"db":"./src/db-a.ts"}}"#,
        ),
        ("main.ts", DENO_ENTRY_ALPHA),
        ("src/util.ts", UTIL),
        ("src/db-a.ts", "export const db = 'a';\n"),
        ("src/db-b.ts", "export const db = 'b';\n"),
        ("src/orphan.ts", "export const orphan = 1;\n"),
        ("src/dupe-a.ts", CLONE_A),
        ("src/dupe-b.ts", CLONE_B),
    ],
    steps: &[
        SOURCE_STEPS[0],
        SOURCE_STEPS[1],
        SOURCE_STEPS[2],
        SOURCE_STEPS[3],
        SOURCE_STEPS[4],
        SOURCE_STEPS[5],
        SOURCE_STEPS[6],
        Step::SwapDenoImportMap,
        Step::ToggleProduction,
        Step::ToggleIgnoreOrphan,
    ],
};

const VITE_ENTRY_ALPHA: &str = "import { alpha } from './util';\nimport { value } from '@v/value';\n\nexport const run = (): unknown[] => [alpha(), value];\n";

/// A Vite and Vitest project. The graph depends on a `resolve.alias` entry,
/// a `setupFiles` entry and an `include` glob, which only the plugin stage
/// reads. Both configs are source files, so the content fingerprint of the
/// manifest also sees each edit.
pub const VITE: Template = Template {
    name: "vite",
    files: &[
        (
            "package.json",
            r#"{"name":"vite-app","private":true,"main":"src/index.ts","devDependencies":{"vite":"*","vitest":"*"}}"#,
        ),
        (
            "vite.config.ts",
            "import { defineConfig } from 'vite';\n\nexport default defineConfig({ resolve: { alias: { '@v': './src/a' } } });\n",
        ),
        (
            "vitest.config.ts",
            "import { defineConfig } from 'vitest/config';\n\nexport default defineConfig({ test: { include: ['test/**/*.spec.ts'], setupFiles: ['./test/setup-a.ts'] } });\n",
        ),
        ("src/index.ts", VITE_ENTRY_ALPHA),
        ("src/util.ts", UTIL),
        ("src/a/value.ts", "export const value = 'a';\n"),
        ("src/b/value.ts", "export const value = 'b';\n"),
        ("src/orphan.ts", "export const orphan = 1;\n"),
        ("src/dupe-a.ts", CLONE_A),
        ("src/dupe-b.ts", CLONE_B),
        (
            "test/setup-a.ts",
            "import { alpha } from '../src/util';\nexport const setupA = alpha;\n",
        ),
        (
            "test/setup-b.ts",
            "import { bravo } from '../src/util';\nexport const setupB = bravo;\n",
        ),
        (
            "test/one.spec.ts",
            "import { orphan } from '../src/orphan';\nexport const one = orphan;\n",
        ),
        (
            "test/two.case.ts",
            "import { sumA } from '../src/dupe-a';\nexport const two = sumA;\n",
        ),
    ],
    steps: &[
        SOURCE_STEPS[0],
        SOURCE_STEPS[1],
        SOURCE_STEPS[2],
        SOURCE_STEPS[3],
        SOURCE_STEPS[4],
        SOURCE_STEPS[5],
        SOURCE_STEPS[6],
        Step::SwapViteAlias { keep_mtime: false },
        Step::SwapViteAlias { keep_mtime: true },
        Step::SwapVitestSetup,
        Step::SwapVitestInclude,
        Step::ToggleProduction,
        Step::ToggleIgnoreOrphan,
    ],
};

/// An Angular project. `angular.json` is not a source file, so the manifest
/// does not fingerprint it. An edit of its Sass include path reaches the
/// graph cache key only through the plugin results, and an edit of its
/// `main` entry only through the entry points.
pub const ANGULAR: Template = Template {
    name: "angular",
    files: &[
        (
            "package.json",
            r#"{"name":"ng-app","private":true,"dependencies":{"@angular/core":"*"}}"#,
        ),
        (
            "angular.json",
            r#"{"projects":{"app":{"architect":{"build":{"options":{"main":"src/index.ts","styles":["src/styles.scss"],"stylePreprocessorOptions":{"includePaths":["src/theme-a"]}}}}}}}"#,
        ),
        ("src/index.ts", ENTRY_ALPHA_PLAIN),
        (
            "src/other.ts",
            "import { bravo } from './util';\nexport const other = bravo();\n",
        ),
        ("src/util.ts", UTIL),
        ("src/styles.scss", "@use 'tokens';\n"),
        ("src/theme-a/_tokens.scss", "$color: red;\n"),
        ("src/theme-b/_tokens.scss", "$color: blue;\n"),
        ("src/orphan.ts", "export const orphan = 1;\n"),
        ("src/dupe-a.ts", CLONE_A),
        ("src/dupe-b.ts", CLONE_B),
    ],
    steps: &[
        SOURCE_STEPS[0],
        SOURCE_STEPS[1],
        SOURCE_STEPS[2],
        SOURCE_STEPS[3],
        SOURCE_STEPS[4],
        SOURCE_STEPS[5],
        SOURCE_STEPS[6],
        Step::SwapAngularIncludePath,
        Step::SwapAngularMain,
        Step::ToggleProduction,
        Step::ToggleIgnoreOrphan,
    ],
};

const ENTRY_ALPHA_PLAIN: &str =
    "import { alpha } from './util';\n\nexport const run = (): unknown[] => [alpha()];\n";

pub const TEMPLATES: [&Template; 5] = [&NODE, &BUN, &DENO, &VITE, &ANGULAR];

/// The file that the template uses as its entry.
fn entry_file(project: &Project) -> &'static str {
    if project.exists("main.ts") {
        "main.ts"
    } else {
        "src/index.ts"
    }
}

/// Swap each `from` with `to` in a file, and each `to` with `from`. The two
/// spellings must have the same length when `keep_mtime` is set, so the
/// metadata cannot show the edit.
fn swap_in_file(project: &mut Project, file: &str, from: &str, to: &str, keep_mtime: bool) {
    let content = project.read(file).expect("swap target exists");
    const MARK: &str = "\u{0}";
    let next = content
        .replace(from, MARK)
        .replace(to, from)
        .replace(MARK, to);
    assert_ne!(next, content, "{file}: the swap must change the file");
    if keep_mtime {
        assert_eq!(
            next.len(),
            content.len(),
            "{file}: a kept-mtime edit keeps the length"
        );
        project.write_keep_mtime(file, &next);
    } else {
        project.write(file, &next);
    }
}

fn toggle_option(values: &mut Vec<String>, value: &str) {
    if let Some(index) = values.iter().position(|existing| existing == value) {
        values.remove(index);
    } else {
        values.push(value.to_string());
    }
}

fn toggle_package(project: &mut Project, installed_files: &[(&str, &str)]) -> bool {
    let installed = project.exists("node_modules/plain-pkg/package.json");
    if installed {
        std::fs::remove_dir_all(project.path("node_modules/plain-pkg")).expect("uninstall");
    } else {
        for (file, content) in installed_files {
            project.write(file, content);
        }
    }
    !installed
}

const PLAIN_PKG: [(&str, &str); 2] = [
    (
        "node_modules/plain-pkg/package.json",
        r#"{"name":"plain-pkg","version":"1.0.0","main":"index.js"}"#,
    ),
    ("node_modules/plain-pkg/index.js", "module.exports = {};\n"),
];

/// Apply `step` to `project`.
pub fn apply(project: &mut Project, step: Step) {
    match step {
        Step::SwapUtilUse { keep_mtime } => {
            let entry = entry_file(project);
            swap_in_file(project, entry, "alpha", "bravo", keep_mtime);
        }
        Step::TouchSource => project.touch("src/util.ts"),
        Step::EditClone { keep_mtime } => {
            assert_eq!(CLONE_B.len(), CLONE_B_BROKEN.len());
            swap_in_file(
                project,
                "src/dupe-b.ts",
                CLONE_B,
                CLONE_B_BROKEN,
                keep_mtime,
            );
        }
        Step::ToggleExtraFile => {
            if project.exists("src/extra.ts") {
                project.remove("src/extra.ts");
            } else {
                project.write(
                    "src/extra.ts",
                    "import { bravo } from './util';\nexport const extra = bravo();\n",
                );
            }
        }
        Step::RenameOrphan => {
            if project.exists("src/orphan.ts") {
                project.rename("src/orphan.ts", "src/moved/orphan.ts");
            } else {
                project.rename("src/moved/orphan.ts", "src/orphan.ts");
            }
        }
        Step::ToggleProduction => project.options.production = !project.options.production,
        Step::ToggleIgnoreOrphan => {
            toggle_option(&mut project.options.ignore_patterns, "src/orphan.ts");
        }
        Step::ToggleCondition => {
            toggle_option(&mut project.options.conditions, "react-server");
        }
        Step::SwapViteAlias { keep_mtime } => {
            swap_in_file(
                project,
                "vite.config.ts",
                "./src/a'",
                "./src/b'",
                keep_mtime,
            );
        }
        Step::SwapVitestSetup => {
            swap_in_file(project, "vitest.config.ts", "setup-a", "setup-b", false);
        }
        Step::SwapVitestInclude => {
            swap_in_file(project, "vitest.config.ts", "*.spec.ts", "*.case.ts", false);
        }
        Step::SwapAngularIncludePath => {
            swap_in_file(project, "angular.json", "theme-a", "theme-b", false);
        }
        Step::SwapAngularMain => {
            swap_in_file(
                project,
                "angular.json",
                "src/index.ts",
                "src/other.ts",
                false,
            );
        }
        manifest => apply_manifest_step(project, manifest),
    }
}

/// Apply a step that edits a manifest, a lockfile or `node_modules`.
fn apply_manifest_step(project: &mut Project, step: Step) {
    match step {
        Step::EditLockfile => {
            swap_in_file(
                project,
                "package-lock.json",
                "\"name\":\"app\"",
                "\"name\":\"ap2\"",
                false,
            );
        }
        Step::ToggleNpmInstall => {
            let installed = toggle_package(project, &PLAIN_PKG);
            let lock = if installed {
                NPM_LOCK_INSTALLED
            } else {
                NPM_LOCK
            };
            project.write("package-lock.json", lock);
            project.write("node_modules/.package-lock.json", lock);
        }
        Step::ToggleBunInstall => {
            toggle_package(project, &PLAIN_PKG);
        }
        Step::NodeModulesNoise => {
            let count = project
                .read("node_modules/.cache/tool/data")
                .map_or(0, |data| data.len());
            project.write("node_modules/.cache/tool/data", &"x".repeat(count + 1));
            project.write(
                "node_modules/other-pkg/README.md",
                &"# readme\n".repeat(count + 1),
            );
        }
        Step::SwapPackageImports => {
            swap_in_file(
                project,
                "package.json",
                "./src/db-a.ts",
                "./src/db-b.ts",
                false,
            );
        }
        Step::SwapWorkspaceNames => {
            let lib = project
                .read("packages/lib/package.json")
                .expect("lib manifest");
            let other = project
                .read("packages/other/package.json")
                .expect("other manifest");
            project.write("packages/lib/package.json", &other);
            project.write("packages/other/package.json", &lib);
        }
        Step::ToggleLibExports => {
            let lib = project
                .read("packages/lib/package.json")
                .expect("lib manifest");
            let name = if lib.contains("@demo/lib") {
                "@demo/lib"
            } else {
                "@demo/other"
            };
            let next = if lib.contains("exports") {
                format!(r#"{{"name":"{name}"}}"#)
            } else {
                format!(
                    r#"{{"name":"{name}","exports":{{"./src/util":{{"react-server":"./src/util.ts","default":"./src/alt.ts"}}}}}}"#
                )
            };
            project.write("packages/lib/package.json", &next);
        }
        Step::SwapExtendedTsconfig => {
            swap_in_file(
                project,
                "config/tsconfig.base.json",
                "alias-a",
                "alias-b",
                false,
            );
        }
        Step::SwapNestedTsconfig => {
            swap_in_file(project, "nested/tsconfig.json", "n-a.ts", "n-b.ts", false);
        }
        Step::SwapDenoImportMap => {
            swap_in_file(
                project,
                "deno.json",
                "./src/db-a.ts",
                "./src/db-b.ts",
                false,
            );
        }
        other => unreachable!("{other:?} is not a manifest step"),
    }
}
