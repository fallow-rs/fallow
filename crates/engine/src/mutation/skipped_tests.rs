//! Static detection of skipped and todo tests in the test files that a diff
//! changes. A test that the runner skips checks nothing, so it cannot kill a
//! mutant. The scan reads the syntax tree and runs no test.

use fallow_types::extract::{byte_offset_to_line_col, compute_line_offsets};
use oxc_ast::ast::{
    Argument, CallExpression, Expression, ObjectExpression, ObjectPropertyKind, PropertyKey,
};
use oxc_ast_visit::{Visit, walk};
use serde::Serialize;

use super::layout::Layout;

/// How a test is left out of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SkipKind {
    /// The runner skips the test.
    Skip,
    /// The test is a placeholder that the runner does not run.
    Todo,
}

/// One test that a test file leaves out of the run.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SkippedTest {
    /// The root-relative path of the test file, with forward slashes.
    pub file: String,
    /// The 1-based line of the call.
    pub line: u32,
    /// How the runner leaves the test out.
    pub kind: SkipKind,
    /// The test name, when the first argument is a string literal.
    pub name: Option<String>,
}

/// The callee names of a test, a suite or a group of tests.
const TEST_ROOTS: [&str; 4] = ["it", "test", "describe", "suite"];

/// The names that Vitest allows between the root and `skip` or `todo`, for
/// example `it.concurrent.skip`.
const MODIFIERS: [&str; 3] = ["concurrent", "sequential", "shuffle"];

/// The callee names that skip a test with no member access.
const SKIP_ALIASES: [&str; 3] = ["xit", "xtest", "xdescribe"];

/// The skipped and todo tests in the source of one test file. A file that
/// does not parse has none. A conditional skip such as `it.skipIf(flag)` is
/// not a skipped test, because the condition decides at run time.
pub(super) fn skipped_tests_in(file: &str, source: &str, layout: Layout) -> Vec<SkippedTest> {
    let line_offsets = compute_line_offsets(source);
    let (text, kind) = layout.parse_view(source);
    let mut tests = fallow_extract::parse_bounded_or_default(&text, kind.source_type(), |parsed| {
        let mut scan = Scan {
            file,
            line_offsets: &line_offsets,
            tests: Vec::new(),
        };
        scan.visit_program(&parsed.program);
        scan.tests
    });
    tests.sort();
    tests
}

struct Scan<'a> {
    file: &'a str,
    line_offsets: &'a [u32],
    tests: Vec<SkippedTest>,
}

impl<'a> Visit<'a> for Scan<'_> {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(kind) = skip_kind(call) {
            let (line, _) = byte_offset_to_line_col(self.line_offsets, call.span.start);
            self.tests.push(SkippedTest {
                file: self.file.to_owned(),
                line,
                kind,
                name: test_name(call),
            });
        }
        walk::walk_call_expression(self, call);
    }
}

fn skip_kind(call: &CallExpression<'_>) -> Option<SkipKind> {
    match &call.callee {
        Expression::Identifier(name) if SKIP_ALIASES.contains(&name.name.as_str()) => {
            Some(SkipKind::Skip)
        }
        Expression::Identifier(name) if TEST_ROOTS.contains(&name.name.as_str()) => {
            call.arguments.iter().find_map(|argument| match argument {
                Argument::ObjectExpression(options) => option_kind(options),
                _ => None,
            })
        }
        callee => member_kind(callee),
    }
}

/// `it.skip`, `test.todo`, `describe.concurrent.skip` and the like.
fn member_kind(callee: &Expression<'_>) -> Option<SkipKind> {
    let mut names: Vec<&str> = Vec::new();
    let mut current = callee;
    while let Expression::StaticMemberExpression(member) = current {
        names.push(member.property.name.as_str());
        current = &member.object;
    }
    let Expression::Identifier(root) = current else {
        return None;
    };
    if !TEST_ROOTS.contains(&root.name.as_str()) {
        return None;
    }
    let (outer, inner) = names.split_first()?;
    if !inner.iter().all(|name| MODIFIERS.contains(name)) {
        return None;
    }
    match *outer {
        "skip" => Some(SkipKind::Skip),
        "todo" => Some(SkipKind::Todo),
        _ => None,
    }
}

/// `{ skip: true }` and `{ todo: "reason" }` in the options of a `node:test`
/// call. A value that is not a literal depends on the run, so it does not
/// count.
fn option_kind(options: &ObjectExpression<'_>) -> Option<SkipKind> {
    options.properties.iter().find_map(|property| {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            return None;
        };
        let kind = match key_name(&property.key)? {
            "skip" => SkipKind::Skip,
            "todo" => SkipKind::Todo,
            _ => return None,
        };
        let set = match &property.value {
            Expression::BooleanLiteral(value) => value.value,
            Expression::StringLiteral(value) => !value.value.is_empty(),
            _ => false,
        };
        set.then_some(kind)
    })
}

fn key_name<'a>(key: &'a PropertyKey<'_>) -> Option<&'a str> {
    match key {
        PropertyKey::StaticIdentifier(name) => Some(name.name.as_str()),
        PropertyKey::StringLiteral(name) => name.value.as_str(),
        _ => None,
    }
}

fn test_name(call: &CallExpression<'_>) -> Option<String> {
    match call.arguments.first()? {
        Argument::StringLiteral(name) => Some(name.value.display().to_string()),
        Argument::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .and_then(|part| part.value.cooked)
            .map(|cooked| cooked.display().to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(source: &str) -> Vec<(u32, SkipKind, Option<String>)> {
        let layout = Layout::from_path("src/a.test.ts").expect("a TypeScript path");
        skipped_tests_in("src/a.test.ts", source, layout)
            .into_iter()
            .map(|test| (test.line, test.kind, test.name))
            .collect()
    }

    fn named(line: u32, kind: SkipKind, name: &str) -> (u32, SkipKind, Option<String>) {
        (line, kind, Some(name.to_owned()))
    }

    #[test]
    fn each_recognized_form_is_reported_with_its_line_and_name() {
        let source = "\
import { describe, it, test } from \"vitest\";
it.skip(\"a\", () => {});
test.skip(\"b\", () => {});
describe.skip(\"c\", () => {});
it.todo(\"d\");
test.todo(\"e\");
describe.todo(\"f\");
xit(\"g\", () => {});
xtest(\"h\", () => {});
xdescribe(\"i\", () => {});
it.concurrent.skip(\"j\", () => {});
test(\"k\", { skip: true }, () => {});
test(\"l\", { todo: \"later\" }, () => {});
it(\"m\", { skip: \"no network\" }, () => {});
describe(\"n\", { skip: true }, () => {});
it.skip(`o`, () => {});
";
        assert_eq!(
            scan(source),
            vec![
                named(2, SkipKind::Skip, "a"),
                named(3, SkipKind::Skip, "b"),
                named(4, SkipKind::Skip, "c"),
                named(5, SkipKind::Todo, "d"),
                named(6, SkipKind::Todo, "e"),
                named(7, SkipKind::Todo, "f"),
                named(8, SkipKind::Skip, "g"),
                named(9, SkipKind::Skip, "h"),
                named(10, SkipKind::Skip, "i"),
                named(11, SkipKind::Skip, "j"),
                named(12, SkipKind::Skip, "k"),
                named(13, SkipKind::Todo, "l"),
                named(14, SkipKind::Skip, "m"),
                named(15, SkipKind::Skip, "n"),
                named(16, SkipKind::Skip, "o"),
            ]
        );
    }

    #[test]
    fn a_nested_skip_and_a_skip_with_a_computed_name_are_reported() {
        let source = "\
describe(\"suite\", () => {
  it.skip(name, () => {});
  describe(\"inner\", () => {
    it.todo(\"deep\");
  });
});
";
        assert_eq!(
            scan(source),
            vec![(2, SkipKind::Skip, None), named(4, SkipKind::Todo, "deep"),]
        );
    }

    #[test]
    fn normal_tests_conditional_skips_and_other_calls_are_not_reported() {
        let source = "\
it(\"normal\", () => {});
test(\"also normal\", { timeout: 1000 }, () => {});
test(\"flag off\", { skip: false }, () => {});
test(\"runtime flag\", { skip: process.env.CI }, () => {});
it.skipIf(process.env.CI)(\"conditional\", () => {});
it.only(\"only\", () => {});
it.each([1])(\"each %s\", () => {});
other.skip(\"not a test\");
skip(\"not a test\");
const skip = true;
";
        assert_eq!(scan(source), Vec::new());
    }

    #[test]
    fn a_file_that_does_not_parse_has_no_skipped_test() {
        assert_eq!(scan("it.skip(\"a\", () => {"), Vec::new());
    }
}
