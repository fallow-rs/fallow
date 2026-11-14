//! New-only duplication attribution for a head clone group that lies inside a
//! base clone group: an edit inside an existing clone is inherited, and a new
//! copy stays introduced.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use crate::common::{commit_all, git, parse_json, run_fallow_raw};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

const SUBSPAN_CLONE: &str = "export function NAME(items, opts) {\n  const out = [];\n  for (const item of items) {\n    if (item.kind === 'alpha' && opts.includeAlpha) {\n      out.push({ id: item.id, label: item.name.toUpperCase(), score: item.score * 2 });\n    } else if (item.kind === 'beta' && opts.includeBeta) {\n      out.push({ id: item.id, label: item.name.toLowerCase(), score: item.score + 10 });\n    } else if (item.kind === 'gamma') {\n      out.push({ id: item.id, label: item.name.trim(), score: Math.max(0, item.score - 5) });\n    } else {\n      out.push({ id: item.id, label: String(item.name), score: 0 });\n    }\n  }\n  const total = out.reduce((sum, row) => sum + row.score, 0);\n  const sorted = out.sort((x, y) => y.score - x.score);\n  return { total, rows: sorted, count: sorted.length, empty: sorted.length === 0 };\n}\n";

/// Repo with one committed clone group across `src/a.js` and `src/b.js`. The
/// `edit` callback changes the working tree, and the function returns the
/// JSON of a new-only audit against the commit.
fn audit_after_clone_edit(edit: impl FnOnce(&Path)) -> serde_json::Value {
    audit_after_clone_edit_in("", &["a", "b"], edit)
}

/// Repo with one committed clone group: one copy of the clone per name in
/// `copies`, at `<project>/src/<name>.js`. The project is the repo
/// subdirectory `project_dir` (empty for the repo root). The `edit` callback
/// gets the project directory. The function returns the
/// JSON of a new-only audit of the project against the commit.
fn audit_after_clone_edit_in(
    project_dir: &str,
    copies: &[&str],
    edit: impl FnOnce(&Path),
) -> serde_json::Value {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let repo = tmp.path();
    let project = repo.join(project_dir);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::write(
        project.join("package.json"),
        r#"{"name":"audit-clone-subspan","type":"module","main":"src/index.js"}"#,
    )
    .unwrap();
    write_index(&project, copies.iter().map(|name| (*name, *name)));
    for name in copies {
        fs::write(
            project.join(format!("src/{name}.js")),
            SUBSPAN_CLONE.replace("NAME", name),
        )
        .unwrap();
    }
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "core.autocrlf", "false"]);
    commit_all(repo, "initial");
    edit(&project);

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        project.to_str().unwrap(),
        "--base",
        "HEAD",
        "--gate",
        "new-only",
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    assert_eq!(
        output.code, 0,
        "audit should run. stdout: {}\nstderr: {}",
        output.stdout, output.stderr
    );
    parse_json(&output)
}

/// Writes `src/index.js`, which imports each `(name, module)` pair as
/// `import { name } from './module.js'`.
fn write_index<'a>(project: &Path, imports: impl IntoIterator<Item = (&'a str, &'a str)>) {
    let (names, lines): (Vec<&str>, Vec<String>) = imports
        .into_iter()
        .map(|(name, module)| (name, format!("import {{ {name} }} from './{module}.js';\n")))
        .unzip();
    let text = format!("{}console.log({});\n", lines.concat(), names.join(", "));
    fs::write(project.join("src/index.js"), text).unwrap();
}

fn edit_file(dir: &Path, rel: &str, from: &str, to: &str) {
    let path = dir.join(rel);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(from), "{rel} must contain {from}");
    fs::write(&path, text.replace(from, to)).unwrap();
}

fn assert_all_clone_groups_inherited(json: &serde_json::Value) {
    let groups = json["duplication"]["clone_groups"]
        .as_array()
        .expect("clone groups array");
    assert!(!groups.is_empty(), "expected a clone group: {json}");
    assert!(
        groups.iter().all(|group| group["introduced"] == false),
        "every clone group inside a base clone group must be inherited: {json}"
    );
    assert_eq!(json["attribution"]["duplication_introduced"], 0);
    assert_eq!(json["attribution"]["duplication_demoted"], 0);
}

/// A one-line edit inside one copy of a base clone splits the group into
/// smaller groups. Each smaller group lies inside the base group, so the
/// change did not write the duplication, even where a group holds the edited
/// line.
#[test]
fn audit_one_line_edit_inside_base_clone_is_inherited() {
    let json = audit_after_clone_edit(|dir| {
        edit_file(dir, "src/a.js", "item.score + 10", "item.score + 11");
    });
    assert_all_clone_groups_inherited(&json);
}

/// A parameter rename in one copy of a base clone shrinks the group to the
/// lines that do not name the parameter. The smaller group is inherited.
#[test]
fn audit_parameter_rename_inside_base_clone_is_inherited() {
    let json = audit_after_clone_edit(|dir| {
        let path = dir.join("src/a.js");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replace("opts", "options")).unwrap();
    });
    assert_all_clone_groups_inherited(&json);
}

/// A new copy of a base clone in a new file has no containing base instance,
/// so its clone group stays introduced.
#[test]
fn audit_new_copy_of_base_clone_stays_introduced() {
    let json = audit_after_clone_edit(|dir| {
        fs::write(dir.join("src/c.js"), SUBSPAN_CLONE.replace("NAME", "c")).unwrap();
        fs::write(
            dir.join("src/index.js"),
            "import { a } from './a.js';\nimport { b } from './b.js';\nimport { c } from './c.js';\nconsole.log(a, b, c);\n",
        )
        .unwrap();
    });
    let groups = json["duplication"]["clone_groups"]
        .as_array()
        .expect("clone groups array");
    assert!(
        groups.iter().any(|group| group["introduced"] == true),
        "a new copy must keep an introduced clone group: {json}"
    );
}

/// A one-line edit in one copy of a base clone group with three instances
/// splits the group. Each smaller group lies inside the base group, so each
/// is inherited.
#[test]
fn audit_one_line_edit_inside_three_instance_base_clone_is_inherited() {
    let json = audit_after_clone_edit_in("", &["a", "b", "c"], |project| {
        edit_file(project, "src/a.js", "item.score + 10", "item.score + 11");
    });
    assert_all_clone_groups_inherited(&json);
}

/// A copy that the change renames with `git mv` and also edits still lies
/// inside its base instance, because the check maps the path through the
/// rename.
#[test]
fn audit_renamed_and_edited_copy_inside_base_clone_is_inherited() {
    let json = audit_after_clone_edit(|dir| {
        git(dir, &["mv", "src/a.js", "src/renamed.js"]);
        edit_file(dir, "src/renamed.js", "item.score + 10", "item.score + 11");
        write_index(dir, [("a", "renamed"), ("b", "b")]);
    });
    assert_all_clone_groups_inherited(&json);
}

/// The change renames one copy and rewrites the lines above the other copy.
/// Two import lines become one, so that copy moves up by one line, and its
/// clone starts on a rewritten line. The rewritten line replaces several base
/// lines, and the clone text on it matches only the last of them. The head
/// group thus lies inside the base group (issue #4580).
#[test]
fn audit_renamed_copy_and_rewritten_lines_above_other_copy_is_inherited() {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let repo = tmp.path();
    fs::create_dir_all(repo.join("src")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"name":"audit-clone-rename-shift","type":"module","main":"src/index.js"}"#,
    )
    .unwrap();
    let clone = SUBSPAN_CLONE.replace("NAME", "compute");
    fs::write(
        repo.join("src/a.js"),
        format!("export const a0 = 1;\n{clone}"),
    )
    .unwrap();
    fs::write(repo.join("src/c.js"), "export const c0 = 2;\n").unwrap();
    fs::write(
        repo.join("src/b.js"),
        format!(
            "import {{ a0 }} from './a.js';\nimport {{ c0 }} from './c.js';\nexport const b0 = a0 + c0;\n{clone}"
        ),
    )
    .unwrap();
    fs::write(
        repo.join("src/index.js"),
        "import { b0 } from './b.js';\nimport { compute } from './a.js';\nconsole.log(b0, compute);\n",
    )
    .unwrap();
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "core.autocrlf", "false"]);
    commit_all(repo, "initial");

    git(repo, &["mv", "src/a.js", "src/r.js"]);
    git(repo, &["rm", "-q", "src/c.js"]);
    fs::write(
        repo.join("src/b.js"),
        format!("import {{ a0 }} from './r.js';\nexport const b0 = a0;\n{clone}"),
    )
    .unwrap();
    edit_file(repo, "src/index.js", "'./a.js'", "'./r.js'");
    commit_all(repo, "rename and rewrite");

    let output = run_fallow_raw(&[
        "audit",
        "--root",
        repo.to_str().unwrap(),
        "--base",
        "HEAD~1",
        "--gate",
        "new-only",
        "--no-cache",
        "--format",
        "json",
        "--quiet",
    ]);
    let json = parse_json(&output);
    assert_all_clone_groups_inherited(&json);
    assert_eq!(json["attribution"]["duplication_inherited"], 1);
}

/// A project in a subdirectory of the repo gets the same result. The added
/// lines move the head clone below the end of its base instance. The head
/// span maps back inside the base instance only when the diff paths are
/// relative to the project directory, not to the repo root.
#[test]
fn audit_shifted_edit_inside_base_clone_in_subdirectory_root_is_inherited() {
    let json = audit_after_clone_edit_in("packages/app", &["a", "b"], |project| {
        let path = project.join("src/a.js");
        let text = fs::read_to_string(&path).unwrap();
        let shift = "// Scores the items.\n".repeat(SUBSPAN_CLONE.lines().count());
        fs::write(&path, format!("{shift}{text}")).unwrap();
        edit_file(project, "src/a.js", "item.score + 10", "item.score + 11");
    });
    assert_all_clone_groups_inherited(&json);
}
