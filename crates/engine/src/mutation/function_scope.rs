//! Widens the changed lines of a diff to the whole function (the
//! `function_scope` option of a plan).

/// Adds, for each changed line, the byte span of the innermost function that
/// holds the first non-blank byte of the line. When no function holds it, the
/// line selects the largest function that starts on the line, for example
/// `export function f() {`. A parameter line, a signature line and a body line
/// all select their whole function. A line with no such function, for example
/// a top-level `const`, adds nothing. The result is sorted, and overlapping or
/// adjacent ranges are merged.
///
/// `lines` are the 1-based changed lines, `ranges` are their byte ranges and
/// `functions` are the byte spans of the functions of the file.
pub(super) fn widened_ranges(
    source: &str,
    lines: &[u64],
    mut ranges: Vec<(u32, u32)>,
    functions: &[(u32, u32)],
) -> Vec<(u32, u32)> {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(source.match_indices('\n').map(|(index, _)| index + 1))
        .collect();
    for &line in lines {
        let Some(&start) = usize::try_from(line)
            .ok()
            .and_then(|line| line.checked_sub(1))
            .and_then(|index| starts.get(index))
        else {
            continue;
        };
        let end = starts
            .iter()
            .copied()
            .find(|&next| next > start)
            .unwrap_or(source.len());
        let text = &source[start..end];
        let first = start + text.len() - text.trim_start().len();
        let Ok(position) = u32::try_from(first) else {
            continue;
        };
        if first >= end {
            continue;
        }
        let holding = functions
            .iter()
            .filter(|&&(from, to)| from <= position && position < to)
            .min_by_key(|&&(from, to)| to - from);
        let selected = holding.or_else(|| {
            let line_end = u32::try_from(end).unwrap_or(u32::MAX);
            functions
                .iter()
                .filter(|&&(from, _)| position <= from && from < line_end)
                .max_by_key(|&&(from, to)| to - from)
        });
        if let Some(&span) = selected {
            ranges.push(span);
        }
    }
    merge(ranges)
}

fn merge(mut ranges: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.0 <= last.1 => last.1 = last.1.max(range.1),
            _ => merged.push(range),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "const top = 1;\nfunction f(a, b) {\n  return a + b;\n}\nconst g = () => {\n  const h = () => 2;\n  return h();\n};\n";

    fn spans() -> Vec<(u32, u32)> {
        let find = |needle: &str, end: &str| {
            let from = SOURCE.find(needle).expect("start");
            let to = from + SOURCE[from..].find(end).expect("end") + end.len();
            (
                u32::try_from(from).expect("from"),
                u32::try_from(to).expect("to"),
            )
        };
        vec![
            find("function f", "}\n"),
            find("() => {", "};"),
            find("() => 2", ";"),
        ]
    }

    #[test]
    fn a_signature_line_selects_its_whole_function() {
        let function = spans()[0];
        assert_eq!(
            widened_ranges(SOURCE, &[2], vec![(15, 32)], &spans()),
            vec![(function.0.min(15), function.1.max(32))]
        );
    }

    #[test]
    fn a_body_line_selects_the_innermost_function_that_holds_its_first_byte() {
        let outer = spans()[1];
        let line_start = u32::try_from(SOURCE.find("  const h").expect("line")).expect("offset");
        let line_end = u32::try_from(SOURCE.find("  return h").expect("line")).expect("offset");
        assert_eq!(
            widened_ranges(SOURCE, &[6], vec![(line_start, line_end)], &spans()),
            vec![outer],
            "the line starts in `g`, and the inner arrow starts later on the line"
        );
    }

    #[test]
    fn a_function_that_starts_after_a_modifier_on_the_line_is_selected() {
        let source = "export function f(a) {\n  return a;\n}\n";
        let span = (7, u32::try_from(source.len() - 1).expect("length"));
        assert_eq!(
            widened_ranges(source, &[1], vec![(0, 23)], &[span]),
            vec![(0, span.1)]
        );
    }

    #[test]
    fn a_top_level_line_adds_nothing() {
        assert_eq!(
            widened_ranges(SOURCE, &[1], vec![(0, 15)], &spans()),
            vec![(0, 15)]
        );
    }

    #[test]
    fn overlapping_ranges_merge_and_a_blank_line_adds_nothing() {
        let merged = merge(vec![(10, 20), (0, 5), (5, 8), (18, 30)]);
        assert_eq!(merged, vec![(0, 8), (10, 30)]);
        assert_eq!(
            widened_ranges("a\n\nb\n", &[2], Vec::new(), &[(0, 5)]),
            Vec::new()
        );
    }
}
