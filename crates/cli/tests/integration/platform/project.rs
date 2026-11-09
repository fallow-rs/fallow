//! A generated project, and its files in a chosen line-ending form.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

/// A small seeded generator (SplitMix64), so one seed gives one project.
pub struct Seeded(u64);

impl Seeded {
    pub const fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        value ^ (value >> 31)
    }

    /// A number in `low..=high`.
    pub fn between(&mut self, low: usize, high: usize) -> usize {
        let span = u64::try_from(high - low + 1).expect("small span");
        low + usize::try_from(self.next_u64() % span).expect("small number")
    }

    pub fn percent(&mut self, chance: u64) -> bool {
        self.next_u64() % 100 < chance
    }
}

/// The line endings of a materialized project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndings {
    /// Each file keeps `\n`. This is the reference form.
    Lf,
    /// Each file uses `\r\n`, as a Windows checkout with `core.autocrlf`.
    Crlf,
    /// Each line break is `\r\n` or `\n`, chosen per line by the seed, so
    /// one file can hold both forms.
    Mixed(u64),
}

/// The files of a generated project: a base commit and a change on top of it.
#[derive(Debug, Default)]
pub struct Project {
    pub base: Vec<(String, String)>,
    pub change: Vec<(String, String)>,
}

/// Build the project for `seed`. Each project has unused files, unused
/// exports, an unused dependency, a block that repeats in several files, and
/// a complex function. The seed changes the counts, the indentation, and the
/// blank lines, because a CRLF bug often depends on the position of a line
/// break in a fragment.
pub fn generate(seed: u64) -> Project {
    let mut rng = Seeded::new(seed);
    let mut project = Project::default();
    let unused_dependency = if rng.percent(60) {
        r#","left-pad":"^1.3.0""#
    } else {
        ""
    };
    project.base.push((
        "package.json".to_owned(),
        format!(
            r#"{{"name":"platform-demo","private":true,"type":"module","main":"src/index.ts","dependencies":{{"zod":"^3.23.0"{unused_dependency}}}}}"#
        ),
    ));
    let indent = if rng.percent(50) { "  " } else { "    " };
    let copies = rng.between(2, 4);
    let mut index = String::from("import { z } from 'zod';\nimport { used } from './util';\n");
    for copy in 0..copies {
        if rng.percent(50) {
            let _ = writeln!(index, "import {{ copy{copy} }} from './copy-{copy}';");
        }
        project.base.push((
            format!("src/copy-{copy}.ts"),
            repeated_block(copy, indent, rng.percent(50)),
        ));
    }
    index.push_str("\nexport const schema = z.string();\nexport const run = () => used();\n");
    project.base.push(("src/index.ts".to_owned(), index));

    let mut util = String::from("export const used = (): number => 1;\n");
    for extra in 0..rng.between(1, 3) {
        if rng.percent(40) {
            util.push('\n');
        }
        let _ = writeln!(util, "export const unused{extra} = {extra};");
    }
    let _ = writeln!(util, "export type UnusedShape = {{ id: string }};");
    project.base.push(("src/util.ts".to_owned(), util));
    for orphan in 0..rng.between(1, 2) {
        project.base.push((
            format!("src/orphan-{orphan}.ts"),
            format!("// An orphan file.\nexport const orphan{orphan} = true;\n"),
        ));
    }
    let mut complex = complex_function("branches", rng.between(12, 18), indent);
    if rng.percent(50) {
        // A file without a final line break.
        complex.pop();
    }
    project.base.push(("src/complex.ts".to_owned(), complex));

    project.change.push((
        "src/util.ts".to_owned(),
        format!(
            "{}export const addedUnused = 2;\n\n{}",
            project
                .base
                .iter()
                .find(|(path, _)| path == "src/util.ts")
                .expect("util")
                .1,
            complex_function("addedBranches", rng.between(14, 20), indent)
        ),
    ));
    project.change.push((
        "src/added.ts".to_owned(),
        repeated_block(copies, indent, true),
    ));
    project
}

/// The same function body in each copy, so `dupes` reports one clone group.
fn repeated_block(copy: usize, indent: &str, blank_line: bool) -> String {
    let gap = if blank_line { "\n" } else { "" };
    format!(
        "export function copy{copy}(items: number[]): number {{\n{indent}let sum = 0;\n{gap}{indent}for (const item of items) {{\n{indent}{indent}if (item % 2 === 0) {{\n{indent}{indent}{indent}sum += item * 3;\n{indent}{indent}}} else {{\n{indent}{indent}{indent}sum -= item + 7;\n{indent}{indent}}}\n{indent}{indent}if (sum > 1000) {{\n{indent}{indent}{indent}sum = sum % 997;\n{indent}{indent}}}\n{indent}}}\n{gap}{indent}const scaled = sum * 11 + items.length;\n{indent}return scaled > 0 ? scaled : -scaled;\n}}\n"
    )
}

/// A function with `count` branches, above the default complexity limits.
fn complex_function(name: &str, count: usize, indent: &str) -> String {
    let mut body = format!(
        "export function {name}(a: number, b: boolean): number {{\n{indent}let total = 0;\n"
    );
    for step in 0..count {
        let _ = writeln!(
            body,
            "{indent}if (a > {step} && b) {{ total += {step}; }} else if (a < -{step}) {{ total -= 1; }}"
        );
    }
    let _ = write!(body, "{indent}return total;\n}}\n");
    body
}

/// Convert the line breaks of `text` from `\n` to the chosen form.
fn convert(text: &str, endings: LineEndings, mixed: &mut Seeded) -> String {
    match endings {
        LineEndings::Lf => text.to_owned(),
        LineEndings::Crlf => text.replace('\n', "\r\n"),
        LineEndings::Mixed(_) => text
            .split_inclusive('\n')
            .map(|line| match line.strip_suffix('\n') {
                Some(body) if mixed.percent(50) => format!("{body}\r\n"),
                _ => line.to_owned(),
            })
            .collect(),
    }
}

fn write_files(root: &Path, files: &[(String, String)], endings: LineEndings) {
    let mut mixed = Seeded::new(match endings {
        LineEndings::Mixed(seed) => seed,
        LineEndings::Lf | LineEndings::Crlf => 0,
    });
    for (path, contents) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(path, convert(contents, endings, &mut mixed)).expect("write project file");
    }
}

/// Keep git apart from the configuration of the machine: a global
/// `core.autocrlf=true` (the default of git for Windows) would rewrite the
/// line endings of the base worktree that `fallow audit` checks out.
pub fn isolate_git(command: &mut Command) -> &mut Command {
    command
        .env("GIT_CONFIG_GLOBAL", NULL_CONFIG)
        .env("GIT_CONFIG_SYSTEM", NULL_CONFIG)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
}

/// Git for Windows also reads `/dev/null` as an empty file.
const NULL_CONFIG: &str = "/dev/null";

/// Run git in `root` with fixed identities and dates, and without line-ending
/// conversion, so the commit keeps the bytes of the chosen form.
fn git(root: &Path, args: &[&str]) -> String {
    let output = isolate_git(&mut Command::new("git"))
        .current_dir(root)
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=Platform"])
        .args(["-c", "user.email=platform@example.com"])
        .args(args)
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} fails: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Write `project` in the form `endings` as two commits, and return the SHAs
/// of the base commit and the head commit.
pub fn materialize(root: &Path, project: &Project, endings: LineEndings) -> (String, String) {
    write_files(root, &project.base, endings);
    git(root, &["init", "-q", "-b", "main"]);
    // The repository config keeps the bytes of the chosen form in each later
    // git call, also in the git calls of `fallow audit`.
    git(root, &["config", "core.autocrlf", "false"]);
    git(root, &["config", "core.safecrlf", "false"]);
    git(root, &["config", "gc.auto", "0"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    write_files(root, &project.change, endings);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "change"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    (base, head)
}
