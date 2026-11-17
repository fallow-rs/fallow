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

/// One exported variable declaration that binds more than one name or binds
/// its names through a destructuring pattern.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct SharedDeclaration {
    /// The line of the `export` keyword.
    pub(super) start_line: usize,
    /// The last line of the declaration.
    pub(super) end_line: usize,
    /// Every name that the declaration binds. Empty when the file did not
    /// parse, so the names are not known.
    pub(super) names: Vec<String>,
}

impl SharedDeclaration {
    /// True when the declaration binds `name` on `line_idx`, or, for a file
    /// that did not parse, when the declaration starts on `line_idx`.
    fn holds(&self, line_idx: usize, name: &str) -> bool {
        if self.names.is_empty() {
            return self.start_line == line_idx;
        }
        (self.start_line..=self.end_line).contains(&line_idx)
            && self.names.iter().any(|bound| bound == name)
    }
}

/// The shared declarations of one file.
pub(super) struct SharedDeclarations(Vec<SharedDeclaration>);

impl SharedDeclarations {
    /// Find the shared declarations of `content`.
    ///
    /// A file that is not JavaScript or TypeScript (for example a Vue or
    /// Svelte file), or that does not parse, falls back to a line scan. That
    /// scan finds only a destructuring pattern on the `export` line and
    /// leaves its names unknown, so no removal of such a declaration happens.
    pub(super) fn scan(path: &Path, content: &str, lines: &[&str]) -> Self {
        if !lines.iter().any(|line| starts_variable_export(line)) {
            return Self(Vec::new());
        }
        parse_shared_declarations(path, content)
            .map_or_else(|| Self(scan_destructuring_lines(lines)), Self)
    }

    /// The index of the declaration that binds `name` on `line_idx`.
    pub(super) fn find(&self, line_idx: usize, name: &str) -> Option<usize> {
        self.0
            .iter()
            .position(|declaration| declaration.holds(line_idx, name))
    }

    pub(super) fn get(&self, index: usize) -> &SharedDeclaration {
        &self.0[index]
    }
}

/// The text of `line` after `export` and an optional `declare`, when it is
/// a `const`, `let` or `var` declaration.
fn variable_export_body(line: &str) -> Option<&str> {
    let after_export = line.trim_start().strip_prefix("export ")?.trim_start();
    let after_declare = after_export
        .strip_prefix("declare ")
        .map_or(after_export, str::trim_start);
    ["const ", "let ", "var "]
        .iter()
        .find_map(|keyword| after_declare.strip_prefix(keyword))
        .map(str::trim_start)
}

fn starts_variable_export(line: &str) -> bool {
    variable_export_body(line).is_some()
}

fn scan_destructuring_lines(lines: &[&str]) -> Vec<SharedDeclaration> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| {
            variable_export_body(line).is_some_and(|body| body.starts_with(['{', '[']))
        })
        .map(|(line_idx, _)| SharedDeclaration {
            start_line: line_idx,
            end_line: line_idx,
            names: Vec::new(),
        })
        .collect()
}

/// Map the shared export declarations that the engine parses to lines.
fn parse_shared_declarations(path: &Path, content: &str) -> Option<Vec<SharedDeclaration>> {
    let declarations = shared_export_declarations(path, content)?;
    let line_starts = line_starts(content);
    let line_of = |offset: u32| {
        line_starts
            .partition_point(|start| *start <= offset as usize)
            .saturating_sub(1)
    };
    Some(
        declarations
            .into_iter()
            .map(|declaration| SharedDeclaration {
                start_line: line_of(declaration.start),
                end_line: line_of(declaration.end.saturating_sub(1)),
                names: declaration.names,
            })
            .collect(),
    )
}

/// The byte offset of the start of each line. A `\r\n` line ending also
/// ends with `\n`, so the line numbers match a split on either ending.
fn line_starts(content: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(content.match_indices('\n').map(|(offset, _)| offset + 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(path: &str, content: &str) -> Vec<SharedDeclaration> {
        let lines: Vec<&str> = content.split('\n').collect();
        SharedDeclarations::scan(Path::new(path), content, &lines).0
    }

    fn declaration(start_line: usize, end_line: usize, names: &[&str]) -> SharedDeclaration {
        SharedDeclaration {
            start_line,
            end_line,
            names: names.iter().map(|name| (*name).to_owned()).collect(),
        }
    }

    #[test]
    fn finds_every_name_of_each_pattern_form() {
        let content = "export const { a = 1, b: { c }, ...rest } = x;\nexport let [d, , [e], ...f] = y;\nexport var g = 1,\n  h = 2;\nexport const single = 1;\n";
        assert_eq!(
            scan("store.ts", content),
            vec![
                declaration(0, 0, &["a", "c", "rest"]),
                declaration(1, 1, &["d", "e", "f"]),
                declaration(2, 3, &["g", "h"]),
            ]
        );
    }

    #[test]
    fn counts_lines_of_a_crlf_file() {
        let content = "const x = {};\r\nexport const {\r\n  a,\r\n  b,\r\n} = x;\r\n";
        let lines: Vec<&str> = content.split("\r\n").collect();
        let found = SharedDeclarations::scan(Path::new("store.ts"), content, &lines);
        assert_eq!(found.0, vec![declaration(1, 4, &["a", "b"])]);
        assert_eq!(found.find(3, "b"), Some(0));
        assert_eq!(found.find(5, "a"), None);
        assert_eq!(found.find(3, "x"), None);
    }

    #[test]
    fn falls_back_to_the_export_line_when_the_file_is_not_script() {
        let content =
            "<script>\nexport const { a, b } = x;\nexport const c = 1, d = 2;\n</script>\n";
        let found = scan("store.vue", content);
        assert_eq!(found, vec![declaration(1, 1, &[])]);
    }

    #[test]
    fn falls_back_when_the_file_does_not_parse() {
        let content = "export const [a, b] = x;\nexport const = ;\n";
        assert_eq!(scan("store.ts", content), vec![declaration(0, 0, &[])]);
    }

    #[test]
    fn skips_the_parse_without_a_variable_export() {
        assert_eq!(scan("store.ts", "export function a() {}\n"), Vec::new());
    }
}
