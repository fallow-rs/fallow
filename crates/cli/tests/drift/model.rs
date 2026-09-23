//! Project grammar for the drift harness.
//!
//! A [`ProjectModel`] holds raw generated values. [`ProjectModel::materialize`]
//! interprets them into a base state and a head state, so shrinking works on
//! plain integers and booleans and never produces an invalid project.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use proptest::prelude::*;

/// Upper bound on generated source files, so one case stays near a second per surface.
const MAX_FILES: usize = 5;
const MAX_EXPORTS: usize = 3;
const MAX_IMPORTS: usize = 3;
const MAX_DEPS: usize = 3;
const MAX_CHANGES: usize = 3;
/// Length of the cyclic mask that selects which baseline entries a case keeps.
const BASELINE_MASK_LEN: usize = 6;

/// The workspace package that `--workspace` selects in a workspace layout.
pub const SELECTED_WORKSPACE: &str = "pkg-a";

#[derive(Debug, Clone)]
pub struct ExportSpec {
    pub is_type: bool,
    pub suppressed: bool,
}

#[derive(Debug, Clone)]
pub struct FileSpec {
    /// In a workspace layout: `false` puts the file in `pkg-a`, `true` in `pkg-b`.
    pub second_package: bool,
    pub entry_imported: bool,
    pub suppress_file: bool,
    pub exports: Vec<ExportSpec>,
    /// Raw `(file, export)` indices, reduced modulo the real counts.
    pub imports: Vec<(usize, usize)>,
}

#[derive(Debug, Clone)]
pub struct DepSpec {
    /// Raw index of the file that imports the dependency, if any.
    pub used_by: Option<usize>,
    pub dev: bool,
}

#[derive(Debug, Clone)]
pub struct MarkedBlock {
    pub file: usize,
    pub suppressed: bool,
}

#[derive(Debug, Clone)]
pub enum ChangeSpec {
    /// Append a new export to a file.
    Edit(usize),
    /// Add a new file that nothing imports.
    Add,
    /// `git mv` a file and update every import of it.
    Rename(usize),
    /// Delete a file and every import of it.
    Delete(usize),
}

#[derive(Debug, Clone)]
pub struct ProjectModel {
    pub workspaces: bool,
    pub files: Vec<FileSpec>,
    pub deps: Vec<DepSpec>,
    /// Two raw file indices that receive the same duplicated function.
    pub duplicate: Option<(usize, usize, bool)>,
    /// A file that receives a function above the default complexity thresholds.
    pub complex: Option<MarkedBlock>,
    pub changes: Vec<ChangeSpec>,
    /// Cyclic mask: entry `i` of a saved baseline stays when `mask[i % len]` is set.
    pub baseline_mask: Vec<bool>,
}

fn export_strategy() -> impl Strategy<Value = ExportSpec> {
    (any::<bool>(), prop::bool::weighted(0.3)).prop_map(|(is_type, suppressed)| ExportSpec {
        is_type,
        suppressed,
    })
}

fn file_strategy() -> impl Strategy<Value = FileSpec> {
    (
        any::<bool>(),
        prop::bool::weighted(0.6),
        prop::bool::weighted(0.15),
        prop::collection::vec(export_strategy(), 0..=MAX_EXPORTS),
        prop::collection::vec((0..MAX_FILES, 0..MAX_EXPORTS), 0..=MAX_IMPORTS),
    )
        .prop_map(
            |(second_package, entry_imported, suppress_file, exports, imports)| FileSpec {
                second_package,
                entry_imported,
                suppress_file,
                exports,
                imports,
            },
        )
}

fn dep_strategy() -> impl Strategy<Value = DepSpec> {
    (prop::option::of(0..MAX_FILES), prop::bool::weighted(0.3))
        .prop_map(|(used_by, dev)| DepSpec { used_by, dev })
}

fn marked_block_strategy() -> impl Strategy<Value = MarkedBlock> {
    (0..MAX_FILES, prop::bool::weighted(0.4))
        .prop_map(|(file, suppressed)| MarkedBlock { file, suppressed })
}

fn change_strategy() -> impl Strategy<Value = ChangeSpec> {
    prop_oneof![
        (0..MAX_FILES).prop_map(ChangeSpec::Edit),
        Just(ChangeSpec::Add),
        (0..MAX_FILES).prop_map(ChangeSpec::Rename),
        (0..MAX_FILES).prop_map(ChangeSpec::Delete),
    ]
}

/// The proptest strategy for one generated project.
pub fn project_strategy() -> impl Strategy<Value = ProjectModel> {
    (
        prop::bool::weighted(0.35),
        prop::collection::vec(file_strategy(), 1..=MAX_FILES),
        prop::collection::vec(dep_strategy(), 0..=MAX_DEPS),
        prop::option::weighted(0.5, (0..MAX_FILES, 0..MAX_FILES, prop::bool::weighted(0.4))),
        prop::option::weighted(0.5, marked_block_strategy()),
        prop::collection::vec(change_strategy(), 0..=MAX_CHANGES),
        prop::collection::vec(any::<bool>(), BASELINE_MASK_LEN),
    )
        .prop_map(
            |(workspaces, files, deps, duplicate, complex, changes, baseline_mask)| ProjectModel {
                workspaces,
                files,
                deps,
                duplicate,
                complex,
                changes,
                baseline_mask,
            },
        )
}

/// One resolved source file of a project state.
#[derive(Debug, Clone)]
struct FileDef {
    id: usize,
    stem: String,
    package: usize,
    alive: bool,
    entry_imported: bool,
    suppress_file: bool,
    exports: Vec<ExportSpec>,
    /// Resolved `(file id, export index)` imports.
    imports: Vec<(usize, usize)>,
    deps: Vec<usize>,
    duplicate: Option<bool>,
    complex: Option<bool>,
    appended: usize,
}

/// A resolved project state: every source file plus the dependency list.
#[derive(Debug, Clone)]
struct State {
    workspaces: bool,
    files: Vec<FileDef>,
    deps: Vec<DepSpec>,
    added: usize,
}

/// Rendered files of both commits and the git operations between them.
#[derive(Debug, Clone)]
pub struct Materialized {
    pub base: BTreeMap<String, String>,
    pub head: BTreeMap<String, String>,
    pub renames: Vec<(String, String)>,
}

impl ProjectModel {
    /// Render the base and head states. `suppressions` selects whether marked
    /// lines carry a suppression comment or a plain comment of the same length
    /// in lines, so both variants keep every finding on the same line.
    pub fn materialize(&self, suppressions: bool) -> Materialized {
        let base = self.base_state();
        let (head, renames) = apply_changes(&base, &self.changes);
        Materialized {
            base: render_state(&base, suppressions),
            head: render_state(&head, suppressions),
            renames,
        }
    }

    fn base_state(&self) -> State {
        let count = self.files.len();
        let package_of =
            |index: usize| usize::from(self.workspaces && self.files[index].second_package);
        let mut files: Vec<FileDef> = self
            .files
            .iter()
            .enumerate()
            .map(|(id, spec)| FileDef {
                id,
                stem: format!("f{id}"),
                package: package_of(id),
                alive: true,
                entry_imported: spec.entry_imported,
                suppress_file: spec.suppress_file,
                exports: spec.exports.clone(),
                imports: Vec::new(),
                deps: Vec::new(),
                duplicate: None,
                complex: None,
                appended: 0,
            })
            .collect();
        for (id, spec) in self.files.iter().enumerate() {
            let mut imports: Vec<(usize, usize)> = spec
                .imports
                .iter()
                .filter_map(|&(raw_file, raw_export)| {
                    let target = raw_file % count;
                    let exports = self.files[target].exports.len();
                    (target != id && exports > 0 && package_of(target) == package_of(id))
                        .then_some((target, raw_export % exports.max(1)))
                })
                .collect();
            imports.sort_unstable();
            imports.dedup();
            files[id].imports = imports;
        }
        for (index, dep) in self.deps.iter().enumerate() {
            if let Some(raw) = dep.used_by {
                files[raw % count].deps.push(index);
            }
        }
        if let Some((first, second, suppressed)) = self.duplicate {
            let (first, second) = (first % count, second % count);
            if first != second {
                files[first].duplicate = Some(suppressed);
                files[second].duplicate = Some(suppressed);
            }
        }
        if let Some(block) = &self.complex {
            files[block.file % count].complex = Some(block.suppressed);
        }
        State {
            workspaces: self.workspaces,
            files,
            deps: self.deps.clone(),
            added: 0,
        }
    }
}

fn apply_changes(base: &State, changes: &[ChangeSpec]) -> (State, Vec<(String, String)>) {
    let mut head = base.clone();
    let mut renames = Vec::new();
    let count = base.files.len();
    for change in changes {
        match *change {
            ChangeSpec::Edit(raw) => {
                let file = &mut head.files[raw % count];
                if file.alive {
                    file.appended += 1;
                }
            }
            ChangeSpec::Add => head.added += 1,
            ChangeSpec::Rename(raw) => {
                let index = raw % count;
                if head.files[index].alive && !head.files[index].stem.starts_with('r') {
                    let old = file_path(&head, &head.files[index]);
                    head.files[index].stem = format!("r{index}");
                    renames.push((old, file_path(&head, &head.files[index])));
                }
            }
            ChangeSpec::Delete(raw) => {
                let index = raw % count;
                if head.files[index].alive {
                    head.files[index].alive = false;
                    renames.retain(|(_, new)| *new != file_path(&head, &head.files[index]));
                    for file in &mut head.files {
                        file.imports.retain(|&(target, _)| target != index);
                    }
                }
            }
        }
    }
    (head, renames)
}

fn package_dir(workspaces: bool, package: usize) -> &'static str {
    match (workspaces, package) {
        (false, _) => "",
        (true, 0) => "packages/a/",
        (true, _) => "packages/b/",
    }
}

fn file_path(state: &State, file: &FileDef) -> String {
    format!(
        "{}src/{}.ts",
        package_dir(state.workspaces, file.package),
        file.stem
    )
}

fn export_name(file: usize, export: usize, spec: &ExportSpec) -> String {
    if spec.is_type {
        format!("T{file}x{export}")
    } else {
        format!("e{file}x{export}")
    }
}

fn marker(suppressions: bool, suppressed: bool, directive: &str) -> String {
    if suppressions && suppressed {
        format!("// {directive}\n")
    } else {
        "// drift fixture line\n".to_string()
    }
}

fn render_state(state: &State, suppressions: bool) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let packages = if state.workspaces { 2 } else { 1 };
    if state.workspaces {
        out.insert(
            "package.json".to_string(),
            "{\n  \"name\": \"drift-root\",\n  \"private\": true,\n  \"workspaces\": [\"packages/*\"]\n}\n"
                .to_string(),
        );
    }
    for package in 0..packages {
        let dir = package_dir(state.workspaces, package);
        out.insert(
            format!("{dir}package.json"),
            render_manifest(state, package),
        );
        out.insert(format!("{dir}src/index.ts"), render_entry(state, package));
    }
    for file in state.files.iter().filter(|file| file.alive) {
        out.insert(
            file_path(state, file),
            render_file(state, file, suppressions),
        );
    }
    for added in 0..state.added {
        out.insert(
            format!("{}src/n{added}.ts", package_dir(state.workspaces, 0)),
            format!("export const fresh{added} = {added};\n"),
        );
    }
    out
}

fn render_manifest(state: &State, package: usize) -> String {
    let name = match (state.workspaces, package) {
        (false, _) => "drift-fixture",
        (true, 0) => SELECTED_WORKSPACE,
        (true, _) => "pkg-b",
    };
    let mut deps = Vec::new();
    let mut dev_deps = Vec::new();
    for (index, dep) in state.deps.iter().enumerate() {
        let owner = dep
            .used_by
            .map_or(0, |raw| state.files[raw % state.files.len()].package);
        if owner != package {
            continue;
        }
        let entry = format!("\"dep-{index}\": \"1.0.0\"");
        if dep.dev {
            dev_deps.push(entry);
        } else {
            deps.push(entry);
        }
    }
    format!(
        "{{\n  \"name\": \"{name}\",\n  \"private\": true,\n  \"type\": \"module\",\n  \"main\": \"src/index.ts\",\n  \"dependencies\": {{{}}},\n  \"devDependencies\": {{{}}}\n}}\n",
        deps.join(", "),
        dev_deps.join(", ")
    )
}

fn render_entry(state: &State, package: usize) -> String {
    let mut out = String::from("// entry\n");
    for file in state
        .files
        .iter()
        .filter(|file| file.alive && file.entry_imported && file.package == package)
    {
        let _ = writeln!(out, "import \"./{}\";", file.stem);
    }
    out.push_str("export {};\n");
    out
}

fn render_file(state: &State, file: &FileDef, suppressions: bool) -> String {
    let mut out = marker(
        suppressions,
        file.suppress_file,
        "fallow-ignore-file unused-file",
    );
    let mut values = Vec::new();
    for &(target, export) in &file.imports {
        let target_file = &state.files[target];
        let spec = &target_file.exports[export];
        let name = export_name(target, export, spec);
        if spec.is_type {
            let _ = writeln!(
                out,
                "import type {{ {name} }} from \"./{}\";",
                target_file.stem
            );
            let _ = writeln!(out, "const use{name}: {name} | null = null;");
            values.push(format!("use{name}"));
        } else {
            let _ = writeln!(out, "import {{ {name} }} from \"./{}\";", target_file.stem);
            values.push(name);
        }
    }
    for &dep in &file.deps {
        let _ = writeln!(out, "import dep{dep} from \"dep-{dep}\";");
        values.push(format!("dep{dep}"));
    }
    if !values.is_empty() {
        let _ = writeln!(out, "console.log({});", values.join(", "));
    }
    for (index, spec) in file.exports.iter().enumerate() {
        let name = export_name(file.id, index, spec);
        let directive = if spec.is_type {
            "fallow-ignore-next-line unused-type"
        } else {
            "fallow-ignore-next-line unused-export"
        };
        out.push_str(&marker(suppressions, spec.suppressed, directive));
        if spec.is_type {
            let _ = writeln!(out, "export type {name} = {{ value{index}: number }};");
        } else {
            let _ = writeln!(out, "export const {name} = {};", file.id * 10 + index);
        }
    }
    if let Some(suppressed) = file.duplicate {
        out.push_str(&marker(
            suppressions,
            suppressed,
            "fallow-ignore-next-line code-duplication",
        ));
        out.push_str(DUPLICATED_FUNCTION);
        out.push_str("console.log(compute);\n");
    }
    if let Some(suppressed) = file.complex {
        out.push_str(&marker(
            suppressions,
            suppressed,
            "fallow-ignore-next-line complexity",
        ));
        out.push_str(&complex_function(file.id));
        let _ = writeln!(out, "console.log(branchy{});", file.id);
    }
    for appended in 0..file.appended {
        let _ = writeln!(
            out,
            "export const added{}x{appended} = {appended};",
            file.id
        );
    }
    out
}

const DUPLICATED_FUNCTION: &str = "function compute(values: number[]): number {
  let total = 0;
  for (const value of values) {
    if (value > 10) {
      total += value * 2;
    } else if (value > 5) {
      total += value + 3;
    } else {
      total -= value;
    }
  }
  const result = total / values.length;
  return Math.round(result * 100) / 100;
}
";

fn complex_function(id: usize) -> String {
    let mut out =
        format!("function branchy{id}(a: number, b: number, c: number): number {{\n  let r = 0;\n");
    for step in 0..12 {
        let _ = writeln!(
            out,
            "  if (a > {step} && b < {step}) {{ if (c === {step}) {{ r += {step}; }} else {{ r -= {step}; }} }}"
        );
    }
    out.push_str("  return r;\n}\n");
    out
}

/// Write both commits of `materialized` into `root` as a fresh git repository.
///
/// # Panics
///
/// Panics when a file write or a git command fails.
pub fn write_repository(root: &Path, materialized: &Materialized) {
    git(root, &["init", "-q", "-b", "main"]);
    write_files(root, &materialized.base);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "base"]);
    for (old, new) in &materialized.renames {
        if let Some(parent) = root.join(new).parent() {
            std::fs::create_dir_all(parent).expect("create rename target dir");
        }
        git(root, &["mv", old, new]);
    }
    for path in materialized.base.keys() {
        let renamed = materialized.renames.iter().any(|(old, _)| old == path);
        if !renamed && !materialized.head.contains_key(path) {
            git(root, &["rm", "-q", path]);
        }
    }
    write_files(root, &materialized.head);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", "head"]);
}

fn write_files(root: &Path, files: &BTreeMap<String, String>) {
    for (path, content) in files {
        let target = root.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("create fixture dir");
        }
        std::fs::write(&target, content).expect("write fixture file");
    }
}

/// Run git in `root` with an isolated identity and no signing, and without
/// the `GIT_*` variables a hook environment can leak into the test process.
///
/// # Panics
///
/// Panics when git cannot start or exits with a failure.
pub fn git(root: &Path, args: &[&str]) -> String {
    // An empty global config next to the project keeps user hooks, signing and
    // aliases out of the fixture on every platform.
    let global = root.with_extension("gitconfig");
    if !global.is_file() {
        std::fs::write(&global, "").expect("write empty git config");
    }
    let output = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=drift",
            "-c",
            "user.email=drift@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Describe the files of the head commit, for a failure report.
pub fn describe(materialized: &Materialized) -> String {
    let mut out = String::new();
    for (path, content) in &materialized.head {
        let _ = writeln!(out, "--- {path}\n{content}");
    }
    if !materialized.renames.is_empty() {
        let _ = writeln!(out, "renames: {:?}", materialized.renames);
    }
    out
}
