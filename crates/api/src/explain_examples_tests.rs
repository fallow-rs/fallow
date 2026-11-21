use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::explain::all_rules;

/// The rules with no example project, each with the data that a small
/// project cannot give.
const RULES_WITHOUT_EXAMPLE: &[(&str, &str)] = &[
    (
        "unlisted-binary",
        "the rule runs only when the project has a `node_modules` directory, which git ignores",
    ),
    ("runtime-safe-to-delete", "needs production runtime data"),
    ("runtime-review-required", "needs production runtime data"),
    ("runtime-low-traffic", "needs production runtime data"),
    (
        "runtime-coverage-unavailable",
        "needs production runtime data",
    ),
    ("runtime-coverage", "needs production runtime data"),
    (
        "coverage-intelligence-risky-change",
        "needs production runtime data",
    ),
    (
        "coverage-intelligence-delete",
        "needs production runtime data",
    ),
    (
        "coverage-intelligence-review",
        "needs production runtime data",
    ),
    (
        "coverage-intelligence-refactor",
        "needs production runtime data",
    ),
    ("mutation-survived", "needs a run of the project's tests"),
    (
        "tainted-sink",
        "each finding of the family reports under the id of its category",
    ),
];

const UPDATE_ENV: &str = "FALLOW_UPDATE_RULE_EXAMPLES";

fn crate_root() -> PathBuf {
    PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
}

fn examples_root() -> PathBuf {
    crate_root().join("rule-examples")
}

fn table_path() -> PathBuf {
    crate_root().join("src/explain_examples_table.rs")
}

fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .map(|entry| entry.expect("directory entry").path())
        .collect();
    entries.sort();
    entries
}

/// The files below `dir`, as paths from `dir` with `/` separators, sorted.
fn project_files(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, found: &mut Vec<String>) {
        for path in sorted_entries(dir) {
            if path.is_dir() {
                walk(root, &path, found);
            } else {
                let relative = path.strip_prefix(root).expect("path below the root");
                let parts: Vec<String> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                found.push(parts.join("/"));
            }
        }
    }
    let mut found = Vec::new();
    if dir.is_dir() {
        walk(dir, dir, &mut found);
    }
    found.sort();
    found
}

struct Meta {
    args: Vec<String>,
    hidden: BTreeSet<String>,
}

fn read_meta(example: &Path) -> Meta {
    let path = example.join("example.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Meta {
            args: Vec::new(),
            hidden: BTreeSet::new(),
        };
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not JSON: {error}", path.display()));
    let object = value.as_object().expect("example.json holds an object");
    for key in object.keys() {
        assert!(
            key == "args" || key == "hidden",
            "{} has the unknown key `{key}`",
            path.display()
        );
    }
    let strings = |key: &str| -> Vec<String> {
        object
            .get(key)
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().expect("a string").to_string())
                    .collect()
            })
            .unwrap_or_default()
    };
    Meta {
        args: strings("args"),
        hidden: strings("hidden").into_iter().collect(),
    }
}

fn render_files(out: &mut String, rule: &str, part: &str, files: &[String], meta: &Meta) {
    if files.is_empty() {
        writeln!(out, "        {part}: &[],").expect("write to a string");
        return;
    }
    writeln!(out, "        {part}: &[").expect("write to a string");
    for file in files {
        let hidden = meta.hidden.contains(file);
        writeln!(
            out,
            "            file!({rule:?}, {part:?}, {file:?}, {hidden}),"
        )
        .expect("write to a string");
    }
    writeln!(out, "        ],").expect("write to a string");
}

/// The text of `explain_examples_table.rs` for the examples on disk.
fn render_table() -> String {
    let mut out = String::from(
        "//! The example projects of `crates/api/rule-examples/`, embedded.\n\
         //!\n\
         //! Generated from that directory. Do not edit by hand. To write the file again, run\n\
         //! `FALLOW_UPDATE_RULE_EXAMPLES=1 cargo test -p fallow-api --lib explain_examples`.\n\
         \n\
         use super::{ExampleFile, RuleExample};\n\
         \n\
         macro_rules! file {\n\
         \x20   ($rule:literal, $part:literal, $path:literal, $hidden:literal) => {\n\
         \x20       ExampleFile {\n\
         \x20           path: $path,\n\
         \x20           code: include_str!(concat!(\"../rule-examples/\", $rule, \"/\", $part, \"/\", $path)),\n\
         \x20           hidden: $hidden,\n\
         \x20       }\n\
         \x20   };\n\
         }\n\
         \n\
         #[rustfmt::skip]\n\
         pub(super) const RULE_EXAMPLES: &[RuleExample] = &[\n",
    );
    for example in sorted_entries(&examples_root()) {
        if !example.is_dir() {
            continue;
        }
        let rule = example
            .file_name()
            .expect("a directory name")
            .to_string_lossy()
            .into_owned();
        let meta = read_meta(&example);
        let files = project_files(&example.join("files"));
        let base = project_files(&example.join("base"));
        assert!(!files.is_empty(), "{rule} has no file in `files/`");
        for hidden in &meta.hidden {
            assert!(
                files.contains(hidden) || base.contains(hidden),
                "{rule}: the hidden file `{hidden}` does not exist"
            );
        }
        writeln!(out, "    RuleExample {{").expect("write to a string");
        writeln!(out, "        rule: {rule:?},").expect("write to a string");
        writeln!(out, "        args: &{:?},", meta.args).expect("write to a string");
        render_files(&mut out, &rule, "base", &base, &meta);
        render_files(&mut out, &rule, "files", &files, &meta);
        writeln!(out, "    }},").expect("write to a string");
    }
    out.push_str("];\n");
    out
}

#[test]
fn table_matches_the_examples_on_disk() {
    let expected = render_table();
    if std::env::var_os(UPDATE_ENV).is_some() {
        fs::write(table_path(), &expected).expect("write the table");
        return;
    }
    let actual = fs::read_to_string(table_path()).expect("read the table");
    assert!(
        actual.replace("\r\n", "\n") == expected,
        "explain_examples_table.rs does not match crates/api/rule-examples/. \
         Run `{UPDATE_ENV}=1 cargo test -p fallow-api --lib explain_examples`."
    );
}

/// Whether `reason` has the form "only <command> runs ...", where the command
/// is one word or a code span such as `` `fallow audit` ``.
fn names_other_command(reason: &str) -> bool {
    reason.match_indices("only ").any(|(start, matched)| {
        let rest = &reason[start + matched.len()..];
        let after_command = match rest.strip_prefix('`') {
            Some(code) => code.split_once('`').map(|(_, after)| after),
            None => rest.split_once(' ').map(|(_, after)| after),
        };
        after_command.is_some_and(|after| after.trim_start().starts_with("runs"))
    })
}

#[test]
fn names_other_command_matches_the_command_forms() {
    assert!(names_other_command(
        "only `fallow audit` runs the stylesheet pass that reports it"
    ));
    assert!(names_other_command("only audit runs the pass"));
    assert!(!names_other_command("needs production runtime data"));
    assert!(!names_other_command(
        "the rule runs only when the project has a `node_modules` directory, which git ignores"
    ));
}

/// A rule that only another command reports is a bug of the registry or of
/// the command, not a reason to skip the example. The example test runs each
/// rule with its registered command, so it finds that disagreement.
#[test]
fn no_rule_skips_its_example_because_another_command_reports_it() {
    for (rule, reason) in RULES_WITHOUT_EXAMPLE {
        assert!(
            !names_other_command(reason),
            "{rule}: the reason \"{reason}\" says that another command reports the rule. \
             Make the registered command report it, or register the rule under the other \
             command, and add an example project."
        );
    }
}

#[test]
fn each_rule_has_an_example_or_a_reason() {
    let without: BTreeSet<&str> = RULES_WITHOUT_EXAMPLE
        .iter()
        .map(|(rule, _)| *rule)
        .collect();
    assert_eq!(
        without.len(),
        RULES_WITHOUT_EXAMPLE.len(),
        "a rule is listed twice"
    );
    for (rule, reason) in RULES_WITHOUT_EXAMPLE {
        assert!(!reason.is_empty(), "{rule} has no reason");
    }
    let mut known = BTreeSet::new();
    for rule in all_rules() {
        let bare = bare_rule_id(rule);
        known.insert(bare);
        let has_example = rule_example(rule).is_some();
        assert!(
            has_example != without.contains(bare),
            "{bare}: add an example project in crates/api/rule-examples/{bare}/, or list the \
             rule in RULES_WITHOUT_EXAMPLE with the reason. A rule cannot have both."
        );
    }
    for example in rule_examples() {
        assert!(
            known.contains(example.rule),
            "the example `{}` is for no registered rule",
            example.rule
        );
    }
    for rule in &without {
        assert!(known.contains(rule), "`{rule}` is no registered rule");
    }
}

#[test]
fn each_example_shows_at_least_one_file() {
    for example in rule_examples() {
        assert!(
            example.files.iter().any(|file| !file.hidden),
            "{}: every file is hidden, so the explain output shows no code",
            example.rule
        );
        let is_change = !example.base.is_empty();
        let is_audit_rule = all_rules()
            .find(|rule| bare_rule_id(rule) == example.rule)
            .is_some_and(|rule| crate::explain::rule_command(rule) == "audit");
        assert_eq!(
            is_change, is_audit_rule,
            "{}: only a rule of the `audit` command has a `base/` directory, and each one needs it",
            example.rule
        );
    }
}

#[test]
fn explain_output_carries_the_visible_files_only() {
    let output = crate::explain::explain_issue_type("unused-file").expect("a known rule");
    let paths: Vec<&str> = output
        .example_files
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    assert!(paths.contains(&"src/old-widget.ts"));
    assert!(!paths.contains(&"package.json"), "package.json is hidden");
    assert!(output.example_base_files.is_empty());

    let change = crate::explain::explain_issue_type("weakened-tests").expect("a known rule");
    assert_eq!(change.example_base_files.len(), 1);
    assert_eq!(change.example_base_files[0].path, "test/tax.test.ts");
    assert!(change.example_files[0].code.contains("it.skip"));

    let none = crate::explain::explain_issue_type("mutation-survived").expect("a known rule");
    assert!(none.example_files.is_empty());
}
