//! The exported variable declarations whose `export` keyword exports more
//! than one name.
//!
//! One `export` keyword exports every name of `export const { a, b } = x;`,
//! `export const [a, b] = x;` and `export const a = 1, b = 2;`. The fixer can
//! remove that keyword only when every one of these names is unused, so it
//! must know all the names of the declaration, also of a nested pattern, a
//! default value or a rest element. A text scan cannot find them safely, so
//! the engine parses the file, and this module maps the result to lines.

use std::path::Path;

use fallow_engine::export_declarations::shared_export_declarations;
use rustc_hash::FxHashSet;

/// One exported variable declaration that binds more than one name or binds
/// its names through a destructuring pattern.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SharedDeclaration {
    /// The line of the `export` keyword.
    pub(super) start_line: usize,
    /// The byte offset of the `export` keyword in its line.
    pub(super) start_col: usize,
    /// The last line of the declaration.
    pub(super) end_line: usize,
    /// Every name that the declaration binds.
    pub(super) names: Vec<String>,
}

impl SharedDeclaration {
    fn holds(&self, line_idx: usize, name: &str) -> bool {
        (self.start_line..=self.end_line).contains(&line_idx)
            && self.names.iter().any(|bound| bound == name)
    }
}

/// The shared declarations of one file, and the lines that no text edit of
/// the `export` keyword may touch.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct SharedDeclarations {
    declarations: Vec<SharedDeclaration>,
    /// Lines with a variable export whose names are not known, because the
    /// file did not parse.
    unsafe_lines: FxHashSet<usize>,
}

impl SharedDeclarations {
    /// Find the shared declarations of `content`.
    ///
    /// When the engine cannot parse the file, a line scan marks each line
    /// with a variable export that can bind more than one name, or that does
    /// not start with its `export` keyword. The fixer plans no edit on such
    /// a line, and the finding stays reported by `fallow dead-code`.
    pub(super) fn scan(path: &Path, content: &str, lines: &[&str]) -> Self {
        if !lines.iter().any(|line| has_variable_export(line)) {
            return Self::default();
        }
        parse_shared_declarations(path, content, lines).unwrap_or_else(|| Self {
            declarations: Vec::new(),
            unsafe_lines: unsafe_variable_export_lines(lines),
        })
    }

    /// The index of the declaration that binds `name` on `line_idx`.
    pub(super) fn find(&self, line_idx: usize, name: &str) -> Option<usize> {
        self.declarations
            .iter()
            .position(|declaration| declaration.holds(line_idx, name))
    }

    pub(super) fn get(&self, index: usize) -> &SharedDeclaration {
        &self.declarations[index]
    }

    /// True when the `export` keyword at the start of `line_idx` may be
    /// removed for a name that no shared declaration binds: the line is not
    /// unsafe, and that keyword does not start a shared declaration.
    pub(super) fn allows_line_start_removal(&self, line_idx: usize, indent: usize) -> bool {
        !self.unsafe_lines.contains(&line_idx)
            && !self.declarations.iter().any(|declaration| {
                declaration.start_line == line_idx && declaration.start_col == indent
            })
    }
}

/// The text of `text` after `export` and an optional `declare`, when it
/// starts a `const`, `let` or `var` declaration.
fn variable_export_body(text: &str) -> Option<&str> {
    let after_export = text.strip_prefix("export")?;
    if !after_export.starts_with(char::is_whitespace) {
        return None;
    }
    let after_export = after_export.trim_start();
    let after_declare = after_export
        .strip_prefix("declare ")
        .map_or(after_export, str::trim_start);
    ["const ", "let ", "var "]
        .iter()
        .find_map(|keyword| after_declare.strip_prefix(keyword))
        .map(str::trim_start)
}

/// The byte offset and the body of each variable export in `line`.
fn variable_exports(line: &str) -> impl Iterator<Item = (usize, &str)> {
    line.match_indices("export")
        .filter_map(|(offset, _)| variable_export_body(&line[offset..]).map(|body| (offset, body)))
}

fn has_variable_export(line: &str) -> bool {
    variable_exports(line).next().is_some()
}

fn unsafe_variable_export_lines(lines: &[&str]) -> FxHashSet<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            let indent = line.len() - line.trim_start().len();
            variable_exports(line).any(|(offset, body)| {
                offset != indent || body.starts_with(['{', '[']) || body.contains(',')
            })
        })
        .map(|(line_idx, _)| line_idx)
        .collect()
}

/// Map the shared export declarations that the engine parses to lines.
/// Returns `None` when the engine cannot parse the file, or when a parsed
/// offset does not hold an `export` keyword in `lines`.
fn parse_shared_declarations(
    path: &Path,
    content: &str,
    lines: &[&str],
) -> Option<SharedDeclarations> {
    let parsed = shared_export_declarations(path, content)?;
    let line_starts = line_starts(content);
    let position_of = |offset: u32| {
        let offset = offset as usize;
        let line = line_starts
            .partition_point(|start| *start <= offset)
            .saturating_sub(1);
        (line, offset - line_starts[line])
    };
    let mut declarations = Vec::with_capacity(parsed.len());
    for declaration in parsed {
        let (start_line, start_col) = position_of(declaration.start);
        let (end_line, _) = position_of(declaration.end.saturating_sub(1));
        let holds_keyword = lines
            .get(start_line)
            .and_then(|line| line.get(start_col..))
            .is_some_and(|text| variable_export_body(text).is_some());
        if !holds_keyword {
            return None;
        }
        declarations.push(SharedDeclaration {
            start_line,
            start_col,
            end_line,
            names: declaration.names,
        });
    }
    Some(SharedDeclarations {
        declarations,
        unsafe_lines: FxHashSet::default(),
    })
}

/// The byte offset of the start of each line. A `\r\n` line ending also
/// ends with `\n`, so the line numbers match a split on either ending.
fn line_starts(content: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(content.match_indices('\n').map(|(offset, _)| offset + 1))
        .collect()
}

/// Remove the `export` keyword at byte `col` of `line`, with the white space
/// after it.
pub(super) fn remove_export_keyword_at(line: &str, col: usize) -> String {
    let rest = &line[col..];
    let after = rest.strip_prefix("export").unwrap_or(rest);
    format!("{}{}", &line[..col], after.trim_start_matches([' ', '\t']))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(path: &str, content: &str) -> SharedDeclarations {
        let lines: Vec<&str> = content.split('\n').collect();
        SharedDeclarations::scan(Path::new(path), content, &lines)
    }

    fn declaration(
        (start_line, start_col): (usize, usize),
        end_line: usize,
        names: &[&str],
    ) -> SharedDeclaration {
        SharedDeclaration {
            start_line,
            start_col,
            end_line,
            names: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn finds_every_name_of_each_pattern_form() {
        let content = "export const { a = 1, b: { c }, ...rest } = x;\nexport let [d, , [e], ...f] = y;\nexport var g = 1,\n  h = 2;\nexport const single = 1;\n";
        assert_eq!(
            scan("store.ts", content).declarations,
            vec![
                declaration((0, 0), 0, &["a", "c", "rest"]),
                declaration((1, 0), 1, &["d", "e", "f"]),
                declaration((2, 0), 3, &["g", "h"]),
            ]
        );
    }

    #[test]
    fn finds_a_declaration_that_does_not_start_its_line() {
        let content = "const y = 1; export const { a, b } = o;\n";
        let found = scan("store.ts", content);
        assert_eq!(
            found.declarations,
            vec![declaration((0, 13), 0, &["a", "b"])]
        );
        assert!(found.allows_line_start_removal(0, 0));
    }

    #[test]
    fn counts_lines_of_a_crlf_file() {
        let content = "const x = {};\r\n  export const {\r\n  a,\r\n  b,\r\n} = x;\r\n";
        let lines: Vec<&str> = content.split("\r\n").collect();
        let found = SharedDeclarations::scan(Path::new("store.ts"), content, &lines);
        assert_eq!(
            found.declarations,
            vec![declaration((1, 2), 4, &["a", "b"])]
        );
        assert_eq!(found.find(3, "b"), Some(0));
        assert_eq!(found.find(5, "a"), None);
        assert_eq!(found.find(3, "x"), None);
        assert!(!found.allows_line_start_removal(1, 2));
    }

    #[test]
    fn maps_the_offsets_of_a_vue_script_block() {
        let content = "<template><div /></template>\n<script lang=\"ts\">\nexport const c = 1, d = 2;\n</script>\n";
        let found = scan("store.vue", content);
        assert_eq!(
            found.declarations,
            vec![declaration((2, 0), 2, &["c", "d"])]
        );
    }

    #[test]
    fn marks_each_risky_line_when_the_file_does_not_parse() {
        let content = "export const [a, b] = x;\nexport const c = 1, d = 2;\nconst e = 1; export const f = 1;\nexport const g = 1;\nexport const { h } = x;\nexport const = ;\n";
        let found = scan("store.ts", content);
        assert_eq!(found.declarations, Vec::new());
        let mut lines: Vec<usize> = found.unsafe_lines.iter().copied().collect();
        lines.sort_unstable();
        assert_eq!(lines, vec![0, 1, 2, 4]);
        assert!(found.allows_line_start_removal(3, 0));
    }

    #[test]
    fn skips_the_parse_without_a_variable_export() {
        assert_eq!(
            scan("store.ts", "export function a() {}\n"),
            SharedDeclarations::default()
        );
    }

    #[test]
    fn removes_the_keyword_at_its_column() {
        assert_eq!(
            remove_export_keyword_at("export const z = 1; export const { a } = o;", 20),
            "export const z = 1; const { a } = o;"
        );
        assert_eq!(
            remove_export_keyword_at("  export\tlet [a] = o;", 2),
            "  let [a] = o;"
        );
    }
}
