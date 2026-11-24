//! Positive controls of the clone-group rules of I4: fixed projects whose
//! expected split holds a clone group of a known attribution, so a gap in the
//! harness rule fails without a random case.

use std::fmt::Write as _;

use crate::model::{Materialized, ProjectModel};
use crate::{
    Project, audit_control_model, audit_keys, cli_audit, expected_audit_split, invariants, keys,
};

/// Positive control of the clone-group rules of I4. A clone that grows with
/// one added line in each instance is a new clone group, but the added line
/// alone is shorter than a clone, so audit grades it `inherited-edited` and
/// reports it as inherited (#4968). A clone that changes shape only because a
/// deleted file took an import away holds no added line, so audit demotes it
/// to inherited (#2164). A clone whose copies the head edits in one line
/// after a delete is a small edit of the base clone too. Without this control, an identity that
/// ignores the clone size, or an expected split without the demotion or the
/// edit rule, passes I4 until a random case finds the gap.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn audit_controls_see_clone_group_changes() {
    use crate::model::{ChangeSpec, ExportSpec, FileSpec};
    let file = |exports: usize, imports: Vec<(usize, usize)>| FileSpec {
        second_package: false,
        entry_imported: false,
        suppress_file: false,
        exports: vec![
            ExportSpec {
                is_type: false,
                suppressed: false,
            };
            exports
        ],
        imports,
    };
    let model = |files: Vec<FileSpec>, duplicate: (usize, usize), changes| ProjectModel {
        workspaces: false,
        files,
        deps: Vec::new(),
        duplicate: Some((duplicate.0, duplicate.1, false)),
        complex: None,
        changes,
        baseline_mask: vec![false],
    };
    let grown = model(
        vec![file(0, Vec::new()), file(0, Vec::new())],
        (0, 1),
        vec![ChangeSpec::Edit(0), ChangeSpec::Edit(1)],
    );
    let reshaped = model(
        vec![
            file(1, Vec::new()),
            file(1, Vec::new()),
            file(0, vec![(0, 0)]),
        ],
        (1, 2),
        vec![ChangeSpec::Delete(0)],
    );
    // Both clone files import the deleted file, so the head edits the
    // `console.log` line that starts the surviving clone in both. Its text
    // changes, so the key is new, but the edit is one line.
    let edited = model(
        vec![
            file(1, Vec::new()),
            file(1, Vec::new()),
            file(0, vec![(0, 0), (1, 0)]),
            file(0, vec![(0, 0), (1, 0)]),
        ],
        (2, 3),
        vec![ChangeSpec::Delete(1)],
    );
    for (name, model, split) in [
        ("grown", grown, "inherited"),
        ("reshaped", reshaped, "inherited"),
        ("edited", edited, "inherited"),
    ] {
        let project = Project::new(&model, true);
        let expected = expected_audit_split(&project);
        let keys = if split == "introduced" {
            &expected.introduced
        } else {
            &expected.inherited
        };
        assert!(
            keys.iter().any(|key| key.kind == keys::DUPLICATION_KIND),
            "the {name} control has no {split} clone group\n{}",
            keys::render(keys)
        );
        let cli = audit_keys(&cli_audit(&project.root));
        project
            .explain(invariants::i4_audit_attribution(&expected, &cli))
            .unwrap_or_else(|err| panic!("{name} control: {err}"));
    }
}

/// A function of `count` assignment lines, `3 + count` lines long. `edits`
/// replaces the multiplier of an assignment, by its 1-based index.
fn clone_control_function(name: &str, count: usize, edits: &[(usize, usize)]) -> String {
    let mut text = format!("export function {name}(input, offset) {{\n");
    for index in 1..=count {
        let factor = edits
            .iter()
            .find(|(at, _)| *at == index)
            .map_or(index, |(_, factor)| *factor);
        let _ = writeln!(
            text,
            "  const v{index} = input.v{index} ?? fallback(\"v{index}\", {index}) + offset * {factor};"
        );
    }
    text.push_str("  return v1;\n}\n");
    text
}

/// The [`audit_control_model`] base files, with `clones` on the base and
/// `edited` on the head, and no other change.
fn clone_control_project(clones: &[(&str, String)], edited: &[(&str, String)]) -> Project {
    let base = audit_control_model(false).materialize(true).base;
    let mut files = Materialized {
        head: base.clone(),
        base,
        renames: Vec::new(),
    };
    for (path, text) in clones {
        files.base.insert((*path).to_string(), text.clone());
        files.head.insert((*path).to_string(), text.clone());
    }
    for (path, text) in edited {
        files.head.insert((*path).to_string(), text.clone());
    }
    Project::from_files(files)
}

/// Positive controls of the edit rule of I4, where a rule on paths and copy
/// counts disagrees with the audit.
///
/// - Two old functions at lines 30-40 differ in one line, next to an old
///   clone at lines 1-10. The change makes them equal: a new clone, so
///   introduced.
/// - A 3-copy clone. The change makes the same edit in two copies and
///   another edit in the third: the group of the two copies is an edit of
///   the base clone, so inherited.
#[test]
#[ignore = "needs the fallow-mcp binary; run with: cargo build -p fallow-mcp && cargo test -p fallow-cli --test drift -- --include-ignored"]
fn audit_controls_see_clone_edit_rules() {
    let with_filler = |file: &str, twin_factor: usize| -> String {
        let mut filler = String::new();
        for line in 11..=29 {
            let _ = writeln!(
                filler,
                "export const filler_{file}_{line} = {line} + {};",
                line * 7
            );
        }
        [
            clone_control_function("shared", 7, &[]),
            filler,
            clone_control_function("twin", 8, &[(5, twin_factor)]),
        ]
        .concat()
    };
    let made_equal = clone_control_project(
        &[
            ("src/a.js", with_filler("a", 5)),
            ("src/b.js", with_filler("b", 50)),
        ],
        &[("src/b.js", with_filler("b", 5))],
    );
    let original = clone_control_function("triple", 12, &[]);
    let edited = clone_control_function("triple", 12, &[(5, 70)]);
    let two_of_three = clone_control_project(
        &[
            ("src/a.js", original.clone()),
            ("src/b.js", original.clone()),
            ("src/c.js", original),
        ],
        &[
            ("src/a.js", edited.clone()),
            ("src/b.js", edited),
            ("src/c.js", clone_control_function("triple", 12, &[(8, 90)])),
        ],
    );
    for (name, project, split) in [
        ("made equal", made_equal, "introduced"),
        ("two of three", two_of_three, "inherited"),
    ] {
        let expected = expected_audit_split(&project);
        let (keys, other) = if split == "introduced" {
            (&expected.introduced, &expected.inherited)
        } else {
            (&expected.inherited, &expected.introduced)
        };
        assert!(
            keys.iter().any(|key| key.kind == keys::DUPLICATION_KIND),
            "the {name} control has no {split} clone group\n{}",
            keys::render(keys)
        );
        if split == "inherited" {
            assert!(
                !other.iter().any(|key| key.kind == keys::DUPLICATION_KIND),
                "the {name} control has an introduced clone group\n{}",
                keys::render(other)
            );
        }
        let cli = audit_keys(&cli_audit(&project.root));
        project
            .explain(invariants::i4_audit_attribution(&expected, &cli))
            .unwrap_or_else(|err| panic!("{name} control: {err}"));
    }
}
