//! Removal of the imports that only a removed export list specifier uses.
//!
//! `import { a } from './a'; export { a, used };` imports `a` only for the
//! list. When the fixer removes `a` from the list, the import stays without
//! a use. The analysis does not count such an import as a consumer, so the
//! next fix run removes the `export` of `a` in `./a`, and the kept import
//! then fails the type check (issue #4833). The fixer therefore removes the
//! import binding too, and the full import declaration when no binding is
//! left. An import without bindings (`import './polyfill';`) never changes.

use std::path::Path;

use fallow_engine::import_bindings::{
    ImportBinding, ImportBindingKind, ImportDeclarationBindings, import_bindings,
};
use rustc_hash::FxHashMap;

/// The removal of some bindings of one import declaration.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct ImportRemoval {
    /// The first line of the declaration.
    pub(super) start_line: usize,
    /// The last line of the declaration.
    pub(super) end_line: usize,
    /// The removed local names, in source order.
    pub(super) names: Vec<String>,
    /// The new lines of the declaration, or `None` to delete each line.
    /// The result never has more lines than the declaration.
    pub(super) replacement: Option<Vec<String>>,
}

/// Plan the import removals of one file.
///
/// `removed_references` holds, for each local name, the number of export
/// list specifiers without a `from` clause that the run removes. An import
/// binding goes only when these specifiers are all of its references in the
/// file. A declaration that shares a line with other code does not change.
pub(super) fn plan_import_removals(
    path: &Path,
    content: &str,
    line_ending: &str,
    removed_references: &FxHashMap<String, usize>,
) -> Vec<ImportRemoval> {
    if removed_references.is_empty() {
        return Vec::new();
    }
    let Some(found) = import_bindings(path, content) else {
        return Vec::new();
    };
    let line_starts = line_starts(content, line_ending);
    found
        .declarations
        .iter()
        .filter_map(|declaration| {
            let removable: Vec<bool> = declaration
                .bindings
                .iter()
                .map(|binding| {
                    removed_references
                        .get(&binding.local)
                        .is_some_and(|removed| {
                            found.reference_counts.get(&binding.local) == Some(removed)
                        })
                })
                .collect();
            if !removable.contains(&true) {
                return None;
            }
            plan_declaration(content, line_ending, &line_starts, declaration, &removable)
        })
        .collect()
}

fn plan_declaration(
    content: &str,
    line_ending: &str,
    line_starts: &[usize],
    declaration: &ImportDeclarationBindings,
    removable: &[bool],
) -> Option<ImportRemoval> {
    let start = declaration.start as usize;
    let end = declaration.end as usize;
    let start_line = line_of(line_starts, start);
    let end_line = line_of(line_starts, end);
    let block_start = line_starts[start_line];
    let block_end = line_starts
        .get(end_line + 1)
        .map_or(content.len(), |next| next - line_ending.len());
    if !content[block_start..start].trim().is_empty() || !content[end..block_end].trim().is_empty()
    {
        return None;
    }
    let names = declaration
        .bindings
        .iter()
        .zip(removable)
        .filter(|(_, remove)| **remove)
        .map(|(binding, _)| binding.local.clone())
        .collect();
    if removable.iter().all(|remove| *remove) {
        return Some(ImportRemoval {
            start_line,
            end_line,
            names,
            replacement: None,
        });
    }
    let text = &content[block_start..block_end];
    let ranges = binding_removal_ranges(text, &declaration.bindings, removable, block_start)?;
    let mut new_text = String::with_capacity(text.len());
    let mut cursor = 0;
    for (range_start, range_end) in ranges {
        new_text.push_str(&text[cursor..range_start]);
        cursor = range_end;
    }
    new_text.push_str(&text[cursor..]);
    Some(ImportRemoval {
        start_line,
        end_line,
        names,
        replacement: Some(new_text.split(line_ending).map(str::to_owned).collect()),
    })
}

/// Byte ranges of `text` to delete, sorted and merged, when some bindings of
/// the declaration stay. `offset` is the file offset of `text`.
///
/// Returns `None` when the text between the bindings is not the expected
/// comma and brace layout, for example because a comment holds a brace.
fn binding_removal_ranges(
    text: &str,
    bindings: &[ImportBinding],
    removable: &[bool],
    offset: usize,
) -> Option<Vec<(usize, usize)>> {
    let span = |binding: &ImportBinding| {
        (
            binding.start as usize - offset,
            binding.end as usize - offset,
        )
    };
    let default = bindings
        .first()
        .filter(|binding| binding.kind == ImportBindingKind::Default);
    let named: Vec<usize> = (0..bindings.len())
        .filter(|idx| bindings[*idx].kind == ImportBindingKind::Named)
        .collect();
    let all_named_removed = !named.is_empty() && named.iter().all(|idx| removable[*idx]);
    let mut ranges = Vec::new();
    for (idx, binding) in bindings.iter().enumerate() {
        if !removable[idx] {
            continue;
        }
        let (start, end) = span(binding);
        let range = match binding.kind {
            ImportBindingKind::Default => {
                let comma = end + skip_space(&text[end..]);
                if !text[comma..].starts_with(',') {
                    return None;
                }
                (start, comma + 1 + skip_space(&text[comma + 1..]))
            }
            ImportBindingKind::Namespace => (span(default?).1, end),
            ImportBindingKind::Named if all_named_removed => {
                let last_end = span(&bindings[*named.last()?]).1;
                let close = last_end
                    + text[last_end..].find(|ch: char| !(ch.is_whitespace() || ch == ','))?;
                if !text[close..].starts_with('}') {
                    return None;
                }
                (span(default?).1, close + 1)
            }
            ImportBindingKind::Named => {
                let position = named.iter().position(|named_idx| *named_idx == idx)?;
                if let Some(next) = named.get(position + 1) {
                    (start, span(&bindings[*next]).0)
                } else {
                    let previous_kept = named[..position]
                        .iter()
                        .rev()
                        .find(|named_idx| !removable[**named_idx])?;
                    (span(&bindings[*previous_kept]).1, end)
                }
            }
        };
        ranges.push(range);
    }
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    Some(merged)
}

fn skip_space(text: &str) -> usize {
    text.len() - text.trim_start().len()
}

fn line_starts(content: &str, line_ending: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(
        content
            .match_indices(line_ending)
            .map(|(idx, _)| idx + line_ending.len()),
    );
    starts
}

fn line_of(line_starts: &[usize], offset: usize) -> usize {
    line_starts.partition_point(|start| *start <= offset) - 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply the planned removals for `removed` to `source`.
    fn fix(source: &str, removed: &[(&str, usize)]) -> String {
        fix_with_ending(source, "\n", removed)
    }

    fn fix_with_ending(source: &str, line_ending: &str, removed: &[(&str, usize)]) -> String {
        let removed: FxHashMap<String, usize> = removed
            .iter()
            .map(|(name, count)| ((*name).to_owned(), *count))
            .collect();
        let removals = plan_import_removals(Path::new("barrel.ts"), source, line_ending, &removed);
        let mut lines: Vec<Option<String>> = source
            .split(line_ending)
            .map(|line| Some(line.to_owned()))
            .collect();
        for removal in removals {
            let kept = removal.replacement.map_or(0, |replacement| {
                let kept = replacement.len();
                for (offset, line) in replacement.into_iter().enumerate() {
                    lines[removal.start_line + offset] = Some(line);
                }
                kept
            });
            for line in &mut lines[removal.start_line + kept..=removal.end_line] {
                *line = None;
            }
        }
        lines
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(line_ending)
    }

    // Each source still holds the export list specifiers that the run
    // removes, because the plan reads the file before any edit.

    #[test]
    fn removes_a_declaration_without_other_bindings() {
        let source =
            "import { a } from './a';\nimport { used } from './used';\n\nexport { a, used };\n";
        assert_eq!(
            fix(source, &[("a", 1)]),
            "import { used } from './used';\n\nexport { a, used };\n"
        );
    }

    #[test]
    fn keeps_a_binding_with_other_references() {
        let source = "import { a } from './a';\nconst b = a;\nexport { a, b };\n";
        assert_eq!(fix(source, &[("a", 1)]), source);
    }

    #[test]
    fn removes_named_bindings_and_keeps_the_layout() {
        let source = "import {\n  a,\n  b,\n  c,\n} from './mod';\nexport { a, b, c };\n";
        assert_eq!(
            fix(source, &[("a", 1), ("c", 1)]),
            "import {\n  b,\n} from './mod';\nexport { a, b, c };\n"
        );
        let source = "import { a, b as c, d } from './mod';\nexport { a, c, d };\n";
        assert_eq!(
            fix(source, &[("a", 1), ("d", 1)]),
            "import { b as c } from './mod';\nexport { a, c, d };\n"
        );
    }

    #[test]
    fn removes_a_default_or_namespace_next_to_kept_bindings() {
        let source = "import a, { b } from './mod';\nexport { a, b };\n";
        assert_eq!(
            fix(source, &[("a", 1)]),
            "import { b } from './mod';\nexport { a, b };\n"
        );
        assert_eq!(
            fix(source, &[("b", 1)]),
            "import a from './mod';\nexport { a, b };\n"
        );
        let source = "import a, * as ns from './mod';\nexport { a, ns };\n";
        assert_eq!(
            fix(source, &[("ns", 1)]),
            "import a from './mod';\nexport { a, ns };\n"
        );
    }

    #[test]
    fn keeps_a_declaration_that_shares_its_line() {
        let source = "import { a } from './a'; const x = 1;\nexport { a, x };\n";
        assert_eq!(fix(source, &[("a", 1)]), source);
    }

    #[test]
    fn keeps_side_effect_imports_and_reads_crlf() {
        let source = "import './polyfill';\r\nimport {\r\n  a,\r\n  b,\r\n} from './mod';\r\nexport { a, b };\r\n";
        assert_eq!(
            fix_with_ending(source, "\r\n", &[("a", 1)]),
            "import './polyfill';\r\nimport {\r\n  b,\r\n} from './mod';\r\nexport { a, b };\r\n"
        );
    }
}
