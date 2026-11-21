//! One row per change shape: the grade of each region and whether it fails
//! the `introduced-clone-group` gate. A new shape is one new row.

use std::fmt::Write as _;
use std::path::PathBuf;

use super::*;

fn root() -> PathBuf {
    PathBuf::from("/project")
}

/// An instance whose text is `text`. Instances with equal text give a moved
/// group the same fragment hash as its base group.
fn instance_with(path: &str, start_line: usize, end_line: usize, text: &str) -> CloneInstance {
    CloneInstance {
        is_symlink: false,
        file: root().join(path),
        start_line,
        end_line,
        start_col: 0,
        end_col: 0,
        fragment: text.to_owned(),
    }
}

fn instance(path: &str, start_line: usize, end_line: usize) -> CloneInstance {
    instance_with(path, start_line, end_line, &format!("{path}:{start_line}"))
}

fn group(instances: Vec<CloneInstance>) -> CloneGroup {
    CloneGroup {
        instances,
        token_count: 40,
        line_count: 6,
        similarity: None,
    }
}

fn span(path: &str, start_line: u32, end_line: u32) -> CloneSpan {
    CloneSpan {
        path: path.to_owned(),
        start_line,
        end_line,
    }
}

fn base_group(instances: Vec<CloneSpan>) -> BaseCloneGroup {
    BaseCloneGroup { instances }
}

/// The default `duplicates.minLines`.
const MIN_LINES: usize = 5;

/// The base clone of most rows: `src/a.js:1-16` and `src/b.js:1-16`.
fn ab_base() -> Vec<BaseCloneGroup> {
    vec![base_group(vec![
        span("src/a.js", 1, 16),
        span("src/b.js", 1, 16),
    ])]
}

const ONE_LINE_EDIT: &str = "diff --git a/src/a.js b/src/a.js\n--- a/src/a.js\n+++ b/src/a.js\n@@ -7 +7 @@\n-  old\n+  new\n";

const EDIT_EVERY_COPY: &str = "diff --git a/src/a.js b/src/a.js\n--- a/src/a.js\n+++ b/src/a.js\n@@ -7,0 +8,1 @@\n+  added\ndiff --git a/src/b.js b/src/b.js\n--- a/src/b.js\n+++ b/src/b.js\n@@ -7,0 +8,1 @@\n+  added\n";

/// A diff that adds `path` as a new file with `lines` lines.
fn new_file(path: &str, lines: usize) -> String {
    format!(
        "diff --git a/{path} b/{path}\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{lines} @@\n{}",
        "+x\n".repeat(lines)
    )
}

/// A diff that replaces `old_len` lines of `path` at `start` with `new_len`
/// new lines.
fn replace(path: &str, start: usize, old_len: usize, new_len: usize) -> String {
    format!(
        "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -{start},{old_len} +{start},{new_len} @@\n{}{}",
        "-old\n".repeat(old_len),
        "+new\n".repeat(new_len)
    )
}

/// The head and base of one row.
#[derive(Default)]
struct Case {
    groups: Vec<CloneGroup>,
    ignored: Vec<IgnoredCloneGroup>,
    diff: Option<String>,
    base_keys: Vec<String>,
    base_groups: Vec<BaseCloneGroup>,
    accepted: Vec<AcceptedCloneGroup>,
    renames: Vec<RenamedFile>,
    changed: Vec<&'static str>,
}

/// The expected grade, gate result and accepting entries of one region.
struct Expect {
    grade: CloneRegionGrade,
    gates: bool,
    accepted_by: &'static [&'static str],
    needs_new_ignore_keys: bool,
}

const fn expect(grade: CloneRegionGrade, gates: bool) -> Expect {
    Expect {
        grade,
        gates,
        accepted_by: &[],
        needs_new_ignore_keys: false,
    }
}

fn grade(case: &Case) -> CloneRegions {
    let groups: Vec<&CloneGroup> = case.groups.iter().collect();
    let diff = case.diff.as_deref().map(DiffIndex::from_unified_diff);
    let keys: FxHashSet<String> = case.base_keys.iter().cloned().collect();
    let changed = ChangedPaths {
        changed: case.changed.iter().map(|path| (*path).to_owned()).collect(),
        deleted: FxHashSet::default(),
    };
    grade_clone_regions(
        &CloneRegionHead {
            groups: &groups,
            ignored: &case.ignored,
            root: &root(),
            min_lines: MIN_LINES,
        },
        &CloneRegionBase {
            keys: &keys,
            groups: &case.base_groups,
            accepted: &case.accepted,
            renames: &case.renames,
            changed: &changed,
        },
        diff.as_ref(),
    )
}

/// A change that pastes a block into a new file.
fn paste() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 16),
            instance("src/c.js", 1, 16),
        ])],
        diff: Some(new_file("src/c.js", 16)),
        ..Case::default()
    }
}

/// Issue #4967: one paste gives a long group and a short group inside it
/// with one more copy that the base already had.
fn nested_paste() -> Case {
    Case {
        groups: vec![
            group(vec![
                instance("src/a.js", 1, 11),
                instance("src/d.js", 1, 11),
            ]),
            group(vec![
                instance("src/a.js", 6, 11),
                instance("src/c.js", 3, 8),
                instance("src/d.js", 6, 11),
            ]),
        ],
        diff: Some(new_file("src/d.js", 11)),
        base_groups: vec![base_group(vec![
            span("src/a.js", 6, 11),
            span("src/c.js", 3, 8),
        ])],
        ..Case::default()
    }
}

/// Issue #4967: the config accepts the long group of the nested paste, so
/// the engine hides it, and the short group only adds an old copy.
fn nested_paste_with_accepted_outer_group() -> Case {
    let mut case = nested_paste();
    // The base pass reports only the groups in the changed files, so it
    // holds no group for the old copies in `src/a.js` and `src/c.js`.
    case.base_groups.clear();
    let outer = case.groups.remove(0);
    case.ignored = vec![IgnoredCloneGroup {
        key: "dup:0000aaaa:2".to_owned(),
        group: outer,
    }];
    case
}

/// Issue #4968: the same one-line edit in every copy of a base clone. The
/// added line alone is shorter than `minLines`, so it cannot be a clone.
fn edit_in_every_copy() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 17),
            instance("src/b.js", 1, 17),
        ])],
        diff: Some(EDIT_EVERY_COPY.to_owned()),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// Issue #4968: the config accepts the base clone, and the change edits
/// every copy. The entry follows the edited clone.
fn edit_in_every_copy_of_an_accepted_clone() -> Case {
    Case {
        base_groups: Vec::new(),
        accepted: vec![AcceptedCloneGroup {
            key: "dup:0bbbe8b1:2".to_owned(),
            instances: ab_base().remove(0).instances,
        }],
        ..edit_in_every_copy()
    }
}

/// Issue #4564: an edit inside one copy splits the base group into two
/// smaller groups with new keys.
fn edit_in_one_copy() -> Case {
    Case {
        groups: vec![
            group(vec![instance("src/a.js", 1, 7), instance("src/b.js", 1, 7)]),
            group(vec![
                instance("src/a.js", 7, 16),
                instance("src/b.js", 7, 16),
            ]),
        ],
        diff: Some(ONE_LINE_EDIT.to_owned()),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// Issue #4580: the change renames one copy and rewrites the lines above the
/// other copy, so the clone starts on a rewritten line.
fn move_with_rewritten_lines_above() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/b.js", 2, 15),
            instance("src/r.js", 1, 14),
        ])],
        diff: Some(
            "diff --git a/src/a.js b/src/r.js\nsimilarity index 100%\nrename from src/a.js\nrename to src/r.js\ndiff --git a/src/b.js b/src/b.js\n--- a/src/b.js\n+++ b/src/b.js\n@@ -1,3 +1,2 @@\n-import { a0 } from './a.js';\n-import { c0 } from './c.js';\n-export const b0 = a0 + c0;\n+import { a0 } from './r.js';\n+export const b0 = a0;\n"
                .to_owned(),
        ),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 14),
            span("src/b.js", 3, 16),
        ])],
        renames: vec![RenamedFile {
            from: root().join("src/a.js"),
            to: root().join("src/r.js"),
        }],
        ..Case::default()
    }
}

/// A renamed copy with an edit inside it maps to its base path through the
/// renames of the diff.
fn rename_with_edit() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/lib/a.js", 1, 7),
            instance("src/b.js", 1, 7),
        ])],
        diff: Some(
            "diff --git a/src/a.js b/src/lib/a.js\nrename from src/a.js\nrename to src/lib/a.js\n--- a/src/a.js\n+++ b/src/lib/a.js\n@@ -7 +7 @@\n-  old\n+  new\n"
                .to_owned(),
        ),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// Issue #4670: the change splits one file in two, and the clone moves with
/// its text into the new files.
fn file_split() -> Case {
    let base = group(vec![
        instance_with("src/old.ts", 1, 10, "shared text"),
        instance_with("src/old.ts", 20, 29, "shared text"),
    ]);
    Case {
        groups: vec![group(vec![
            instance_with("src/one.ts", 1, 10, "shared text"),
            instance_with("src/two.ts", 1, 10, "shared text"),
        ])],
        diff: Some([new_file("src/one.ts", 10), new_file("src/two.ts", 10)].concat()),
        base_keys: vec![dupe_group_key(&base, &root())],
        changed: vec!["src/old.ts", "src/one.ts", "src/two.ts"],
        ..Case::default()
    }
}

/// A new copy of a base clone in a new file.
fn new_copy_of_an_old_clone() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 16),
            instance("src/b.js", 1, 16),
            instance("src/c.js", 1, 16),
        ])],
        diff: Some(new_file("src/c.js", 16)),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// The change edits one of two different old functions, so the two become
/// equal. Each instance has base lines, but no base group holds both.
fn clone_from_two_old_functions() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 16),
            instance("src/e.js", 1, 16),
        ])],
        diff: Some(
            "diff --git a/src/e.js b/src/e.js\n--- a/src/e.js\n+++ b/src/e.js\n@@ -9 +9 @@\n-  old\n+  new\n"
                .to_owned(),
        ),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// Issue #2164: the change removes code in another place, and two old
/// functions become one clone group. The change wrote no line of it.
fn clone_from_a_removal_elsewhere() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 7),
            instance("src/e.js", 1, 7),
        ])],
        diff: Some(
            "diff --git a/src/c.js b/src/c.js\n--- a/src/c.js\n+++ b/src/c.js\n@@ -3,2 +2,0 @@\n-  gone\n-  gone\n"
                .to_owned(),
        ),
        ..Case::default()
    }
}

/// A base group under the same key.
fn unchanged_clone() -> Case {
    let head = group(vec![
        instance("src/a.js", 1, 16),
        instance("src/b.js", 1, 16),
    ]);
    Case {
        base_keys: vec![dupe_group_key(&head, &root())],
        groups: vec![head],
        diff: Some(ONE_LINE_EDIT.to_owned()),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// Two groups in other places of the code are two regions.
fn two_separate_pastes() -> Case {
    Case {
        groups: vec![
            group(vec![instance("src/a.js", 1, 8), instance("src/c.js", 1, 8)]),
            group(vec![
                instance("src/a.js", 9, 16),
                instance("src/c.js", 9, 16),
            ]),
        ],
        diff: Some(new_file("src/c.js", 16)),
        ..Case::default()
    }
}

/// Without a diff, a new key is introduced.
fn edit_in_every_copy_without_a_diff() -> Case {
    Case {
        diff: None,
        ..edit_in_every_copy()
    }
}

/// A copy of a clone with itself inside one base copy.
fn self_clone_inside_one_base_copy() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 7),
            instance("src/a.js", 9, 15),
        ])],
        diff: Some(ONE_LINE_EDIT.to_owned()),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// A group that grows past the base instance, and a group with a copy in a
/// file that the base clone does not hold.
fn growth_past_the_base_clone() -> Case {
    Case {
        groups: vec![
            group(vec![
                instance("src/a.js", 1, 20),
                instance("src/b.js", 1, 20),
            ]),
            group(vec![instance("src/a.js", 1, 7), instance("src/d.js", 1, 7)]),
        ],
        diff: Some(ONE_LINE_EDIT.to_owned()),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// An old clone group, and a new group that overlaps one of its copies and
/// has a copy in a new file. The old group keeps its own attribution.
fn old_group_next_to_a_new_group() -> Case {
    let old = group(vec![
        instance("src/a.js", 1, 20),
        instance("src/b.js", 1, 20),
    ]);
    Case {
        base_keys: vec![dupe_group_key(&old, &root())],
        groups: vec![
            old,
            group(vec![
                instance("src/a.js", 15, 30),
                instance("src/new.js", 1, 16),
            ]),
        ],
        diff: Some(new_file("src/new.js", 16)),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 20),
            span("src/b.js", 1, 20),
        ])],
        ..Case::default()
    }
}

/// One new block replaces lines at two different places of a base clone.
/// Each head instance maps inside a base instance, but not at the same
/// place, so the block is a new clone.
fn new_block_at_two_places_of_a_base_clone() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 20, 34),
            instance("src/b.js", 150, 164),
        ])],
        diff: Some(
            [
                replace("src/a.js", 20, 3, 15),
                replace("src/b.js", 150, 3, 15),
            ]
            .concat(),
        ),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 200),
            span("src/b.js", 1, 200),
        ])],
        ..Case::default()
    }
}

/// A diff that inserts `lines` new lines after line `after` of `a.js` and of
/// `b.js`.
fn insert_in_both(after: usize, lines: usize) -> String {
    let body = "+new\n".repeat(lines);
    ["src/a.js", "src/b.js"]
        .map(|path| {
            format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -{after},0 +{},{lines} @@\n{body}",
                after + 1,
            )
        })
        .concat()
}

/// The same 40-line block inserted inside every copy of a base clone. The
/// block alone is a clone, so the change wrote new duplication.
fn large_block_inserted_into_every_copy() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 56),
            instance("src/b.js", 1, 56),
        ])],
        diff: Some(insert_in_both(11, 40)),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// The same 40-line block appended at the end of every copy of a base clone.
fn large_block_appended_to_every_copy() -> Case {
    Case {
        diff: Some(insert_in_both(16, 40)),
        ..large_block_inserted_into_every_copy()
    }
}

/// Every copy grows by `minLines - 1` lines: a small edit.
fn small_block_inserted_into_every_copy() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 20),
            instance("src/b.js", 1, 20),
        ])],
        diff: Some(insert_in_both(11, MIN_LINES - 1)),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// Every copy grows by `minLines` lines: the added lines can be a clone.
fn growth_of_min_lines_in_every_copy() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 21),
            instance("src/b.js", 1, 21),
        ])],
        diff: Some(insert_in_both(11, MIN_LINES)),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// The base clone `src/a.js:1-20` and `src/b.js:1-20`.
fn ab_base_of_20() -> Vec<BaseCloneGroup> {
    vec![base_group(vec![
        span("src/a.js", 1, 20),
        span("src/b.js", 1, 20),
    ])]
}

/// The same 4 new lines after each of the first 19 old lines of both copies.
/// No run reaches `minLines`, but each copy grows by 76 lines.
fn split_additions_in_every_copy() -> Case {
    let hunks = |path: &str| -> String {
        let mut text = format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n");
        for line in 1..20 {
            let start = line + 4 * (line - 1) + 1;
            let _ = writeln!(text, "@@ -{line},0 +{start},4 @@");
            text.push_str(&"+new\n".repeat(4));
        }
        text
    };
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 96),
            instance("src/b.js", 1, 96),
        ])],
        diff: Some([hunks("src/a.js"), hunks("src/b.js")].concat()),
        base_groups: ab_base_of_20(),
        ..Case::default()
    }
}

/// A rewrite of `minLines` lines in every copy, for example a renamed
/// variable, keeps the line count, so the copies do not grow.
fn rewrite_without_growth_in_every_copy() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 16),
            instance("src/b.js", 1, 16),
        ])],
        diff: Some(
            [
                replace("src/a.js", 5, MIN_LINES + 1, MIN_LINES + 1),
                replace("src/b.js", 5, MIN_LINES + 1, MIN_LINES + 1),
            ]
            .concat(),
        ),
        base_groups: ab_base(),
        ..Case::default()
    }
}

/// Base copies of 30 lines each. The head parts start one line apart, the
/// tolerance of a copy that does not grow.
fn offset_at_the_tolerance() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 6, 12),
            instance("src/b.js", 7, 13),
        ])],
        diff: Some([replace("src/a.js", 6, 1, 1), replace("src/b.js", 7, 1, 1)].concat()),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 30),
            span("src/b.js", 1, 30),
        ])],
        ..Case::default()
    }
}

/// The same base copies. The head parts start two lines apart.
fn offset_past_the_tolerance() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 6, 12),
            instance("src/b.js", 8, 14),
        ])],
        diff: Some([replace("src/a.js", 6, 1, 1), replace("src/b.js", 8, 1, 1)].concat()),
        ..offset_at_the_tolerance()
    }
}

/// An old clone `1-10`, and two other functions at `30-40` that differ in one
/// line. The change makes them equal: a new clone outside the old one.
fn two_old_functions_made_equal_next_to_an_old_clone() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 30, 40),
            instance("src/b.js", 30, 40),
        ])],
        diff: Some(replace("src/b.js", 35, 1, 1)),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 10),
            span("src/b.js", 1, 10),
        ])],
        ..Case::default()
    }
}

/// A 3-copy base clone. The change makes the same edit in `a` and `b`, and
/// another edit in `c`. The group of `a` and `b` is an edit of the base clone.
fn same_edit_in_two_of_three_copies() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 1, 16),
            instance("src/b.js", 1, 16),
        ])],
        diff: Some(
            [
                replace("src/a.js", 7, 1, 1),
                replace("src/b.js", 7, 1, 1),
                replace("src/c.js", 9, 1, 1),
            ]
            .concat(),
        ),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 16),
            span("src/b.js", 1, 16),
            span("src/c.js", 1, 16),
        ])],
        ..Case::default()
    }
}

/// The base copies hold the same tokens on 30 and on 60 lines. A small new
/// block replaces lines at the start of one copy and at the middle of the
/// other, so it is not at the same place of the clone.
fn new_block_at_other_places_of_differently_wrapped_copies() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 6, 12),
            instance("src/b.js", 31, 37),
        ])],
        diff: Some([replace("src/a.js", 6, 2, 2), replace("src/b.js", 31, 2, 2)].concat()),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 30),
            span("src/b.js", 1, 60),
        ])],
        ..Case::default()
    }
}

/// The same base copies, and an edit in one copy. The split part starts at
/// the same relative place of both copies.
fn edit_in_one_of_differently_wrapped_copies() -> Case {
    Case {
        groups: vec![group(vec![
            instance("src/a.js", 7, 30),
            instance("src/b.js", 13, 60),
        ])],
        diff: Some(replace("src/a.js", 7, 1, 1)),
        base_groups: vec![base_group(vec![
            span("src/a.js", 1, 30),
            span("src/b.js", 1, 60),
        ])],
        ..Case::default()
    }
}

type Row = (&'static str, fn() -> Case, &'static [Expect]);

use CloneRegionGrade::{Inherited, InheritedEdited, Introduced};

const ROWS: &[Row] = &[
    ("paste", paste, &[expect(Introduced, true)]),
    (
        "nested paste (#4967)",
        nested_paste,
        &[expect(Introduced, true)],
    ),
    (
        "nested paste, outer group accepted (#4967)",
        nested_paste_with_accepted_outer_group,
        &[Expect {
            grade: Introduced,
            gates: false,
            accepted_by: &["dup:0000aaaa:2"],
            needs_new_ignore_keys: false,
        }],
    ),
    (
        "edit in every copy (#4968)",
        edit_in_every_copy,
        &[expect(InheritedEdited, false)],
    ),
    (
        "edit in every copy of an accepted clone (#4968)",
        edit_in_every_copy_of_an_accepted_clone,
        &[Expect {
            grade: InheritedEdited,
            gates: false,
            accepted_by: &["dup:0bbbe8b1:2"],
            needs_new_ignore_keys: true,
        }],
    ),
    (
        "edit in one copy (#4564)",
        edit_in_one_copy,
        &[expect(InheritedEdited, false)],
    ),
    (
        "move with rewritten lines above (#4580)",
        move_with_rewritten_lines_above,
        &[expect(InheritedEdited, false)],
    ),
    (
        "rename with an edit",
        rename_with_edit,
        &[expect(InheritedEdited, false)],
    ),
    (
        "file split (#4670)",
        file_split,
        &[expect(Inherited, false)],
    ),
    (
        "new copy of an old clone",
        new_copy_of_an_old_clone,
        &[expect(Introduced, true)],
    ),
    (
        "clone from two old functions",
        clone_from_two_old_functions,
        &[expect(Introduced, true)],
    ),
    (
        "clone from a removal elsewhere (#2164)",
        clone_from_a_removal_elsewhere,
        &[expect(Inherited, false)],
    ),
    (
        "unchanged clone",
        unchanged_clone,
        &[expect(Inherited, false)],
    ),
    (
        "two separate pastes",
        two_separate_pastes,
        &[expect(Introduced, true), expect(Introduced, true)],
    ),
    (
        "edit in every copy without a diff",
        edit_in_every_copy_without_a_diff,
        &[expect(Introduced, true)],
    ),
    (
        "self clone inside one base copy",
        self_clone_inside_one_base_copy,
        &[expect(Introduced, true)],
    ),
    (
        "growth past the base clone",
        growth_past_the_base_clone,
        &[expect(Introduced, true)],
    ),
    (
        "old group next to a new group",
        old_group_next_to_a_new_group,
        &[expect(Introduced, true)],
    ),
    (
        "new block at two places of a base clone",
        new_block_at_two_places_of_a_base_clone,
        &[expect(Introduced, true)],
    ),
    (
        "large block inserted into every copy",
        large_block_inserted_into_every_copy,
        &[expect(Introduced, true)],
    ),
    (
        "large block appended to every copy",
        large_block_appended_to_every_copy,
        &[expect(Introduced, true)],
    ),
    (
        "growth of minLines - 1 in every copy",
        small_block_inserted_into_every_copy,
        &[expect(InheritedEdited, false)],
    ),
    (
        "growth of minLines in every copy",
        growth_of_min_lines_in_every_copy,
        &[expect(Introduced, true)],
    ),
    (
        "split additions in every copy",
        split_additions_in_every_copy,
        &[expect(Introduced, true)],
    ),
    (
        "rewrite without growth in every copy",
        rewrite_without_growth_in_every_copy,
        &[expect(InheritedEdited, false)],
    ),
    (
        "offset at the tolerance",
        offset_at_the_tolerance,
        &[expect(InheritedEdited, false)],
    ),
    (
        "offset past the tolerance",
        offset_past_the_tolerance,
        &[expect(Introduced, true)],
    ),
    (
        "two old functions made equal next to an old clone",
        two_old_functions_made_equal_next_to_an_old_clone,
        &[expect(Introduced, true)],
    ),
    (
        "same edit in two of three copies",
        same_edit_in_two_of_three_copies,
        &[expect(InheritedEdited, false)],
    ),
    (
        "new block at other places of differently wrapped copies",
        new_block_at_other_places_of_differently_wrapped_copies,
        &[expect(Introduced, true)],
    ),
    (
        "edit in one of differently wrapped copies",
        edit_in_one_of_differently_wrapped_copies,
        &[expect(InheritedEdited, false)],
    ),
];

/// A large block in every copy of an accepted clone is new duplication, so
/// the region fails the gate and offers no `ignoredClones` key.
#[test]
fn large_block_in_an_accepted_clone_gets_no_ignore_hint() {
    let mut case = large_block_inserted_into_every_copy();
    case.accepted = vec![AcceptedCloneGroup {
        key: "dup:0bbbe8b1:2".to_owned(),
        instances: case.base_groups.remove(0).instances,
    }];
    let regions = grade(&case);
    let region = &regions.regions()[0];
    assert!(region.gates);
    assert!(region.accepted_by.is_empty());
    assert!(region.ignore_hint.is_empty());
}

/// The gate fails on the region, but only the new group is introduced. A
/// rule override for the file of the old group then cannot fail the gate
/// through the old group.
#[test]
fn old_group_in_an_introduced_region_stays_not_introduced() {
    let regions = grade(&old_group_next_to_a_new_group());
    assert!(!regions.group_introduced(0));
    assert!(regions.group_introduced(1));
    assert_eq!(regions.introduced_count(), 1);
}

/// A demoted group keeps its demotion in a region that fails the gate.
#[test]
fn demoted_group_in_an_introduced_region_stays_demoted() {
    let mut case = clone_from_a_removal_elsewhere();
    let removal = case.diff.take().unwrap_or_default();
    case.groups.push(group(vec![
        instance("src/a.js", 5, 12),
        instance("src/c2.js", 1, 8),
    ]));
    case.diff = Some([removal, new_file("src/c2.js", 8)].concat());
    let regions = grade(&case);
    assert_eq!(regions.introduced_count(), 1);
    assert!(regions.group_demoted(0));
    assert!(!regions.group_introduced(0));
}

/// The public check that the drift harness calls gives the edit result of
/// the region grade for each row with one group: `inherited-edited` exactly
/// when the check holds.
#[test]
fn edits_base_clone_agrees_with_the_grade() {
    for (name, case, _) in ROWS {
        let case = case();
        let (Some(diff), [group]) = (case.diff.as_deref(), case.groups.as_slice()) else {
            continue;
        };
        if !case.accepted.is_empty() || !case.base_keys.is_empty() {
            continue;
        }
        let diff = DiffIndex::from_unified_diff(diff);
        let spans = head_spans(group, &root());
        let edits = edits_base_clone(
            &spans,
            &CloneChange {
                root: &root(),
                diff: &diff,
                renames: &case.renames,
            },
            &case.base_groups,
            MIN_LINES,
        );
        let grade = grade(&case).regions()[0].grade;
        assert_eq!(edits, grade == InheritedEdited, "row: {name}");
    }
}

#[test]
fn each_change_shape_gets_its_grade_and_gate() {
    for (name, case, expected) in ROWS {
        let case = case();
        let regions = grade(&case);
        let actual: Vec<_> = regions
            .regions()
            .iter()
            .map(|region| {
                (
                    region.grade,
                    region.gates,
                    region.accepted_by.clone(),
                    region.follows_edited_accept(),
                )
            })
            .collect();
        let wanted: Vec<_> = expected
            .iter()
            .map(|row| {
                (
                    row.grade,
                    row.gates,
                    row.accepted_by
                        .iter()
                        .map(|key| (*key).to_owned())
                        .collect::<Vec<_>>(),
                    row.needs_new_ignore_keys,
                )
            })
            .collect();
        assert_eq!(actual, wanted, "row: {name}");
        assert_eq!(
            regions.introduced_count(),
            expected.iter().filter(|row| row.gates).count(),
            "row: {name}"
        );
        assert_eq!(
            regions.group_regions().len(),
            case.groups.len(),
            "row: {name}: each group belongs to one region"
        );
    }
}

#[test]
fn nested_paste_region_holds_both_groups_and_the_union_of_their_lines() {
    let regions = grade(&nested_paste());
    let region = &regions.regions()[0];
    assert_eq!(region.groups, vec![0, 1]);
    assert_eq!(
        region.spans,
        vec![
            span("src/a.js", 1, 11),
            span("src/c.js", 3, 8),
            span("src/d.js", 1, 11),
        ]
    );
    assert!(regions.group_introduced(0) && regions.group_introduced(1));
}

#[test]
fn removal_elsewhere_marks_the_group_demoted() {
    let regions = grade(&clone_from_a_removal_elsewhere());
    assert!(regions.group_demoted(0));
    assert_eq!(regions.demoted_count(), 1);
    let findings = regions.findings();
    assert_eq!(
        findings[0].demotion_reason,
        Some(CloneDemotionReason::NoAddedLines)
    );
}

#[test]
fn accepted_edited_clone_prints_the_new_keys_of_its_groups() {
    let case = edit_in_every_copy_of_an_accepted_clone();
    let regions = grade(&case);
    let findings = regions.findings();
    let expected =
        CloneFingerprintSet::from_groups(&case.groups).ignored_clone_key_for_group(&case.groups[0]);
    assert_eq!(findings[0].accepted_by, vec!["dup:0bbbe8b1:2".to_owned()]);
    assert_eq!(findings[0].ignore_hint, vec![expected]);
    assert!(!findings[0].introduced);
}

/// The config accepts the long group, but the change also edited the old
/// copy in `src/c.js`, so the short group can hold new duplication.
#[test]
fn nested_accept_needs_old_copies_without_an_edit() {
    let mut case = nested_paste_with_accepted_outer_group();
    case.diff = Some(
        [
            new_file("src/d.js", 11).as_str(),
            "diff --git a/src/c.js b/src/c.js\n--- a/src/c.js\n+++ b/src/c.js\n@@ -5 +5 @@\n-  old\n+  new\n",
        ]
        .concat(),
    );
    let regions = grade(&case);
    assert_eq!(regions.introduced_count(), 1);
    assert!(regions.regions()[0].accepted_by.is_empty());
}
