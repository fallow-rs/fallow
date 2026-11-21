#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

//! `fallow fix` must keep the meaning of the code that it keeps.
//!
//! The drift guard runs `fix --yes` on a copy of each fixture with a planned
//! source fix, undoes the changes outside the source files, and analyzes the
//! result again. Each fixer is covered when it
//! has one fixture. The guard asserts that each edited file still parses,
//! that the fix adds no `unresolved-import` finding, that each kept enum
//! member keeps its constant value, and that the findings of the applied
//! fixes are gone.
//!
//! The regression tests cover the known shapes: a removed enum member that
//! shifts the value of a kept member (issue #4963), a removed name of an
//! export list whose import stays (issue #4833), and a removed `export` of a
//! declaration that also exports a name in use (issue #4659).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oxc_ast::ast::{
    BinaryOperator, Expression, TSEnumDeclaration, TSEnumMemberName, UnaryOperator,
};
use oxc_ast_visit::Visit;
use oxc_span::SourceType;
use rayon::prelude::*;

use crate::common::{copy_fixture_tree, fixture_path, parse_json, run_fallow_in_root};

/// The fix entry types of the source fixers.
const SOURCE_FIX_TYPES: [&str; 3] = ["remove_export", "remove_enum_member", "remove_class_member"];

/// The extensions of the files that the guard parses.
const SCRIPT_EXTENSIONS: [&str; 8] = ["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// The extensions of the files that the source fixers edit.
const SOURCE_EXTENSIONS: [&str; 11] = [
    "ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs", "vue", "svelte", "astro",
];

/// The bytes of each file under `root` that is not a source file, by path.
fn other_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if entry.file_name() != ".fallow" {
                    pending.push(path);
                }
                continue;
            }
            let source = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension));
            if !source && let Ok(bytes) = std::fs::read(&path) {
                files.insert(path, bytes);
            }
        }
    }
    files
}

/// Undo each change of `fix` outside the source files: the dependency and
/// config fixers have their own tests, and a removed dependency can turn off
/// a framework plugin, which changes the analysis of the source files.
fn restore_other_files(root: &Path, before: &BTreeMap<PathBuf, Vec<u8>>) {
    for path in other_files(root).keys() {
        if !before.contains_key(path) {
            std::fs::remove_file(path).unwrap();
        }
    }
    for (path, bytes) in before {
        std::fs::write(path, bytes).unwrap();
    }
}

/// The text of each script file under `root`, by path relative to `root`.
fn script_files(root: &Path) -> BTreeMap<PathBuf, String> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if name == "node_modules" || name == ".fallow" || name == ".git" {
                continue;
            }
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let script = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension));
            if script && let Ok(text) = std::fs::read_to_string(&path) {
                files.insert(path.strip_prefix(root).unwrap().to_path_buf(), text);
            }
        }
    }
    files
}

/// A constant value of an enum member.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Number(f64),
    Text(String),
}

/// The constant value of each enum member of a text, keyed by the enum
/// name, the position among the enums of that name, and the member name.
/// `None` when the parse fails.
fn enum_values(path: &Path, text: &str) -> Option<BTreeMap<(String, usize, String), Value>> {
    let source_type = SourceType::from_path(path).ok()?;
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, text, source_type).parse();
    if parsed.fatal_error || parsed.diagnostics.has_errors() {
        return None;
    }
    let mut collector = EnumValues::default();
    collector.visit_program(&parsed.program);
    Some(collector.values)
}

#[derive(Default)]
struct EnumValues {
    values: BTreeMap<(String, usize, String), Value>,
    seen: BTreeMap<String, usize>,
}

impl<'a> Visit<'a> for EnumValues {
    fn visit_ts_enum_declaration(&mut self, it: &TSEnumDeclaration<'a>) {
        let name = it.id.name.to_string();
        let position = *self
            .seen
            .entry(name.clone())
            .and_modify(|n| *n += 1)
            .or_insert(0);
        let mut known: BTreeMap<String, Value> = BTreeMap::new();
        let mut next = Some(Value::Number(0.0));
        for member in &it.body.members {
            let key = match &member.id {
                TSEnumMemberName::Identifier(id) => id.name.to_string(),
                TSEnumMemberName::String(text) | TSEnumMemberName::ComputedString(text) => text
                    .value
                    .as_str()
                    .map_or_else(|| text.value.display().to_string(), str::to_owned),
                TSEnumMemberName::ComputedTemplateString(_) => continue,
            };
            let value = match &member.initializer {
                Some(initializer) => constant(initializer, &name, &known),
                None if it.declare && !it.r#const => None,
                None => next.clone(),
            };
            next = match &value {
                Some(Value::Number(number)) => Some(Value::Number(number + 1.0)),
                _ => None,
            };
            if let Some(value) = value {
                known.insert(key.clone(), value.clone());
                self.values.insert((name.clone(), position, key), value);
            }
        }
    }
}

/// The value of a constant enum expression: literals, signs, references to
/// earlier members, and the arithmetic and bitwise operators.
fn constant(
    expression: &Expression<'_>,
    enumeration: &str,
    known: &BTreeMap<String, Value>,
) -> Option<Value> {
    let number = |expression: &Expression<'_>| match constant(expression, enumeration, known)? {
        Value::Number(number) => Some(number),
        Value::Text(_) => None,
    };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "bitwise operators work on 32-bit integers"
    )]
    let int = |value: f64| value as i64 as i32;
    match expression {
        Expression::NumericLiteral(literal) => Some(Value::Number(literal.value)),
        Expression::StringLiteral(literal) => Some(Value::Text(literal.value.as_str()?.to_owned())),
        Expression::ParenthesizedExpression(inner) => {
            constant(&inner.expression, enumeration, known)
        }
        Expression::Identifier(identifier) => known.get(identifier.name.as_str()).cloned(),
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) if object.name == enumeration => {
                known.get(member.property.name.as_str()).cloned()
            }
            _ => None,
        },
        Expression::UnaryExpression(unary) => {
            let value = number(&unary.argument)?;
            match unary.operator {
                UnaryOperator::UnaryNegation => Some(Value::Number(-value)),
                UnaryOperator::UnaryPlus => Some(Value::Number(value)),
                UnaryOperator::BitwiseNot => Some(Value::Number(f64::from(!int(value)))),
                _ => None,
            }
        }
        Expression::BinaryExpression(binary) => {
            if binary.operator == BinaryOperator::Addition
                && let (Some(Value::Text(left)), Some(Value::Text(right))) = (
                    constant(&binary.left, enumeration, known),
                    constant(&binary.right, enumeration, known),
                )
            {
                return Some(Value::Text(format!("{left}{right}")));
            }
            let (left, right) = (number(&binary.left)?, number(&binary.right)?);
            let shift = |value: f64| (int(value) & 31).cast_unsigned();
            let value = match binary.operator {
                BinaryOperator::Addition => left + right,
                BinaryOperator::Subtraction => left - right,
                BinaryOperator::Multiplication => left * right,
                BinaryOperator::Division => left / right,
                BinaryOperator::Remainder => left % right,
                BinaryOperator::Exponential => left.powf(right),
                BinaryOperator::ShiftLeft => f64::from(int(left).wrapping_shl(shift(right))),
                BinaryOperator::ShiftRight => f64::from(int(left).wrapping_shr(shift(right))),
                BinaryOperator::BitwiseOR => f64::from(int(left) | int(right)),
                BinaryOperator::BitwiseAnd => f64::from(int(left) & int(right)),
                BinaryOperator::BitwiseXOR => f64::from(int(left) ^ int(right)),
                _ => return None,
            };
            Some(Value::Number(value))
        }
        _ => None,
    }
}

/// The `(path, specifier)` pair of each unresolved import of a report.
fn unresolved_imports(report: &serde_json::Value) -> Vec<(String, String)> {
    let mut imports: Vec<(String, String)> = report["unresolved_imports"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item["path"].as_str().unwrap_or_default().replace('\\', "/"),
                        item["specifier"].as_str().unwrap_or_default().to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    imports.sort();
    imports
}

/// True when the report still has the finding of the applied fix entry.
fn finding_remains(report: &serde_json::Value, fix: &serde_json::Value) -> bool {
    let same = |item: &serde_json::Value, keys: &[(&str, &str)]| {
        item["path"]
            .as_str()
            .map(|path| path.replace('\\', "/"))
            .as_deref()
            == fix["path"].as_str()
            && keys
                .iter()
                .all(|(item_key, fix_key)| item[*item_key] == fix[*fix_key])
    };
    let any = |list: &str, keys: &[(&str, &str)]| {
        report[list]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| same(item, keys)))
    };
    match fix["type"].as_str() {
        Some("remove_export") => any("unused_exports", &[("export_name", "name")]),
        Some("remove_enum_member") => any(
            "unused_enum_members",
            &[("parent_name", "parent"), ("member_name", "name")],
        ),
        Some("remove_class_member") => any(
            "unused_class_members",
            &[("parent_name", "parent"), ("member_name", "name")],
        ),
        _ => false,
    }
}

fn dead_code(root: &Path) -> serde_json::Value {
    let output = run_fallow_in_root("dead-code", root, &["--format", "json", "--quiet"]);
    parse_json(&output)
}

/// The edited files that no longer parse, and the kept enum members whose
/// constant value changed.
fn enum_problems(
    name: &str,
    files_before: &BTreeMap<PathBuf, String>,
    files_after: &BTreeMap<PathBuf, String>,
) -> Vec<String> {
    let mut problems = Vec::new();
    for (path, before) in files_before {
        let Some(after) = files_after.get(path) else {
            continue;
        };
        if after == before {
            continue;
        }
        let Some(values_before) = enum_values(path, before) else {
            continue;
        };
        let Some(values_after) = enum_values(path, after) else {
            problems.push(format!(
                "{name}: {} does not parse after the fix",
                path.display()
            ));
            continue;
        };
        for (key, value) in &values_after {
            if let Some(old) = values_before.get(key)
                && old != value
            {
                problems.push(format!(
                    "{name}: {} changes {}.{} from {old:?} to {value:?}",
                    path.display(),
                    key.0,
                    key.2
                ));
            }
        }
    }
    problems
}

/// The problems that `fix --yes` gives on a copy of the fixture at `source`.
/// `None` when the fixture has no planned source fix. Also gives the type of
/// each applied fix entry.
fn check_fixture(source: &Path) -> Option<(Vec<String>, Vec<String>)> {
    let name = source.file_name()?.to_string_lossy().to_string();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    copy_fixture_tree(source, root);

    let preview = run_fallow_in_root("fix", root, &["--dry-run", "--format", "json", "--quiet"]);
    let Ok(preview) = serde_json::from_str::<serde_json::Value>(&preview.stdout) else {
        return None;
    };
    let planned = preview["fixes"].as_array().is_some_and(|fixes| {
        fixes.iter().any(|fix| {
            fix["type"]
                .as_str()
                .is_some_and(|kind| SOURCE_FIX_TYPES.contains(&kind))
                && fix["skipped"] != true
        })
    });
    if !planned {
        return None;
    }

    let files_before = script_files(root);
    let others_before = other_files(root);
    let report_before = dead_code(root);
    let applied = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    restore_other_files(root, &others_before);
    let Ok(applied) = serde_json::from_str::<serde_json::Value>(&applied.stdout) else {
        return Some((
            vec![format!(
                "{name}: `fix --yes` gave no JSON: {}",
                applied.stderr
            )],
            Vec::new(),
        ));
    };
    let files_after = script_files(root);
    let report_after = dead_code(root);

    let mut problems = enum_problems(&name, &files_before, &files_after);
    let unresolved_before = unresolved_imports(&report_before);
    for import in unresolved_imports(&report_after) {
        if !unresolved_before.contains(&import) {
            problems.push(format!("{name}: new unresolved import {import:?}"));
        }
    }
    let mut kinds = Vec::new();
    for fix in applied["fixes"].as_array().into_iter().flatten() {
        if fix["applied"] != true {
            continue;
        }
        kinds.push(fix["type"].as_str().unwrap_or_default().to_owned());
        if finding_remains(&report_after, fix) {
            problems.push(format!("{name}: the finding of {fix} remains"));
        }
    }
    Some((problems, kinds))
}

/// The guard must check enough fixtures to cover each source fixer. A lower
/// count means that the fixtures or the preview changed, and the guard no
/// longer checks what it claims to check.
const MIN_CHECKED_FIXTURES: usize = 20;

#[test]
fn fix_output_keeps_program_meaning() {
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(fixture_path(""))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    fixtures.sort();
    let checked: Vec<(Vec<String>, Vec<String>)> = fixtures
        .par_iter()
        .filter_map(|fixture| check_fixture(fixture))
        .collect();
    assert!(
        checked.len() >= MIN_CHECKED_FIXTURES,
        "only {} fixtures have a planned source fix",
        checked.len()
    );
    for kind in ["remove_export", "remove_enum_member"] {
        assert!(
            checked
                .iter()
                .any(|(_, kinds)| kinds.iter().any(|seen| seen == kind)),
            "no fixture applies a `{kind}` fix"
        );
    }
    let problems: Vec<String> = checked
        .into_iter()
        .flat_map(|(problems, _)| problems)
        .collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn write_files(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

fn fix_yes(root: &Path) {
    let output = run_fallow_in_root("fix", root, &["--yes", "--format", "json", "--quiet"]);
    assert!(output.code == 0 || output.code == 1, "{}", output.stderr);
}

const PACKAGE: &str =
    r#"{ "name": "meaning", "version": "1.0.0", "type": "module", "main": "src/index.ts" }"#;

#[test]
fn issue_4963_fix_keeps_the_value_of_each_kept_enum_member() {
    let dir = tempfile::tempdir().unwrap();
    let level = "export enum Level {\n  Low,\n  Medium,\n  High,\n}\n";
    write_files(
        dir.path(),
        &[
            ("package.json", PACKAGE),
            ("src/level.ts", level),
            (
                "src/index.ts",
                "import { Level } from './level';\nconsole.log(Level.Medium, Level.High);\n",
            ),
        ],
    );
    fix_yes(dir.path());
    let path = Path::new("level.ts");
    let before = enum_values(path, level).unwrap();
    let after_text = std::fs::read_to_string(dir.path().join("src/level.ts")).unwrap();
    let after = enum_values(path, &after_text).unwrap();
    for member in ["Medium", "High"] {
        let key = ("Level".to_owned(), 0, member.to_owned());
        assert_eq!(after.get(&key), before.get(&key), "{after_text}");
    }
}

#[test]
fn issue_4833_fix_never_leaves_the_import_of_a_removed_export_list_name() {
    let dir = tempfile::tempdir().unwrap();
    write_files(
        dir.path(),
        &[
            ("package.json", PACKAGE),
            (
                "src/index.ts",
                "import { used } from './barrel';\n\nused();\n",
            ),
            (
                "src/barrel.ts",
                "import { a } from './a';\nimport { used } from './used';\n\nexport { a, used };\n",
            ),
            ("src/a.ts", "export function a() {}\n"),
            ("src/used.ts", "export function used() {}\n"),
        ],
    );
    fix_yes(dir.path());
    let barrel = std::fs::read_to_string(dir.path().join("src/barrel.ts")).unwrap();
    let imports_a = barrel.contains("import { a }");
    let exports_a = barrel.contains("export { a");
    assert_eq!(imports_a, exports_a, "{barrel}");
}

#[test]
fn issue_4659_fix_keeps_the_export_of_a_destructured_name_in_use() {
    let dir = tempfile::tempdir().unwrap();
    write_files(
        dir.path(),
        &[
            ("package.json", PACKAGE),
            (
                "src/store.ts",
                "const source = { a: 1, b: 2 };\nexport const { a, b } = source;\n",
            ),
            (
                "src/index.ts",
                "import { b } from './store';\nconsole.log(b);\n",
            ),
        ],
    );
    fix_yes(dir.path());
    let store = std::fs::read_to_string(dir.path().join("src/store.ts")).unwrap();
    assert!(store.contains("export const { a, b }"), "{store}");
}
