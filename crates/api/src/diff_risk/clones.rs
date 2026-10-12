//! `diverged-clone-sibling`: the change edited one copy of a base clone group,
//! that copy no longer matches, and no sibling changed.

use rustc_hash::{FxHashMap, FxHashSet};

/// One clone instance location, root-relative with forward slashes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CloneSpan {
    /// Root-relative path.
    pub path: String,
    /// 1-based first line.
    pub start_line: u32,
    /// 1-based last line, inclusive.
    pub end_line: u32,
}

/// The instances of one base clone group. The paths are base paths: the
/// rename remap of the audit does not move them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BaseCloneGroup {
    /// Instance locations in the base tree.
    pub instances: Vec<CloneSpan>,
}

/// One edited clone copy whose siblings did not change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DivergedClone {
    /// Head path of the edited copy.
    pub path: String,
    /// 1-based head line where the edited copy starts, when a line of it
    /// survives.
    pub line: Option<u32>,
    /// The unchanged siblings, at their head paths.
    pub siblings: Vec<CloneSpan>,
}

/// Text access for the detector. `base` takes a base path, `head` a head path.
pub struct CloneTexts<'a> {
    /// Base text of a file, `None` when it cannot be read.
    pub base: &'a mut dyn FnMut(&str) -> Option<String>,
    /// Head text of a file, `None` when the file is gone or unreadable.
    pub head: &'a mut dyn FnMut(&str) -> Option<String>,
}

/// Find the base clone groups where exactly one copy was edited, the edit
/// broke the clone, and every sibling kept its text.
///
/// `changed` holds the head paths of the changed files. `renames` maps a base
/// path to its head path. `head_groups` holds the instances of every head
/// clone group, at head paths.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn diverged_clone_siblings(
    base_groups: &[BaseCloneGroup],
    head_groups: &[Vec<CloneSpan>],
    changed: &FxHashSet<String>,
    renames: &FxHashMap<String, String>,
    texts: &mut CloneTexts<'_>,
) -> Vec<DivergedClone> {
    let mut base_cache: FxHashMap<String, Option<String>> = FxHashMap::default();
    let mut head_cache: FxHashMap<String, Option<String>> = FxHashMap::default();
    let mut found = Vec::new();
    for group in base_groups {
        if group.instances.len() < 2 {
            continue;
        }
        let mut edited: Vec<(CloneSpan, Option<u32>)> = Vec::new();
        let mut siblings: Vec<CloneSpan> = Vec::new();
        let mut skip_group = false;
        for instance in &group.instances {
            let head_path = renames
                .get(&instance.path)
                .cloned()
                .unwrap_or_else(|| instance.path.clone());
            let head_span = CloneSpan {
                path: head_path.clone(),
                ..instance.clone()
            };
            if !changed.contains(&head_path) {
                siblings.push(head_span);
                continue;
            }
            let base_text = base_cache
                .entry(instance.path.clone())
                .or_insert_with(|| (texts.base)(&instance.path));
            let Some(fragment) = base_text
                .as_deref()
                .map(|text| fragment_lines(text, instance))
            else {
                skip_group = true;
                break;
            };
            let head_text = head_cache
                .entry(head_path.clone())
                .or_insert_with(|| (texts.head)(&head_path));
            let Some(head_text) = head_text.as_deref() else {
                // A deleted copy is a removal, not a divergence.
                skip_group = true;
                break;
            };
            match classify_copy(&fragment, head_text) {
                CopyState::Unchanged(lines) => siblings.push(match lines {
                    Some((start_line, end_line)) => CloneSpan {
                        start_line,
                        end_line,
                        ..head_span
                    },
                    None => head_span,
                }),
                CopyState::Edited(line) => edited.push((head_span, line)),
                CopyState::Removed => {
                    skip_group = true;
                    break;
                }
            }
        }
        if skip_group || edited.len() != 1 || siblings.is_empty() {
            continue;
        }
        let (copy, line) = edited.remove(0);
        if still_clones(head_groups, &copy.path, &siblings) {
            continue;
        }
        found.push(DivergedClone {
            path: copy.path,
            line,
            siblings,
        });
    }
    merge_same_copy(found)
}

/// One finding for each edited copy: two base groups that share the copy
/// (nested or overlapping clones) merge, and their siblings join.
fn merge_same_copy(found: Vec<DivergedClone>) -> Vec<DivergedClone> {
    let mut merged: Vec<DivergedClone> = Vec::new();
    for diverged in found {
        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing.path == diverged.path && existing.line == diverged.line)
        {
            existing.siblings.extend(diverged.siblings);
        } else {
            merged.push(diverged);
        }
    }
    for diverged in &mut merged {
        diverged
            .siblings
            .sort_by(|a, b| a.path.cmp(&b.path).then(a.start_line.cmp(&b.start_line)));
        diverged
            .siblings
            .dedup_by(|a, b| a.path == b.path && a.start_line == b.start_line);
    }
    merged.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    merged
}

enum CopyState {
    /// The fragment is still in the head file, word for word. Carries the
    /// 1-based head lines of its first and last non-blank line, so a copy
    /// that moved inside its file keeps its head location.
    Unchanged(Option<(u32, u32)>),
    /// The fragment changed, and most of its lines survive. Carries the head
    /// line of the first surviving line.
    Edited(Option<u32>),
    /// Most of the fragment is gone: a removal or a rewrite, not an edit.
    Removed,
}

/// The trimmed, non-blank lines of a clone instance in its file.
fn fragment_lines(text: &str, span: &CloneSpan) -> Vec<String> {
    let start = usize::try_from(span.start_line.saturating_sub(1)).unwrap_or(usize::MAX);
    let count = usize::try_from(span.end_line.saturating_sub(span.start_line) + 1).unwrap_or(0);
    text.lines()
        .skip(start)
        .take(count)
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn classify_copy(fragment: &[String], head_text: &str) -> CopyState {
    if fragment.is_empty() {
        return CopyState::Unchanged(None);
    }
    // Each non-blank head line with its 1-based line number.
    let numbered: Vec<(u32, &str)> = head_text
        .lines()
        .enumerate()
        .map(|(index, line)| (index, line.trim()))
        .filter(|(_, line)| !line.is_empty())
        .filter_map(|(index, line)| u32::try_from(index + 1).ok().map(|number| (number, line)))
        .collect();
    let head_lines: Vec<&str> = numbered.iter().map(|(_, line)| *line).collect();
    let verbatim = head_lines.windows(fragment.len()).position(|window| {
        window
            .iter()
            .zip(fragment)
            .all(|(head, base)| *head == base)
    });
    if let Some(start) = verbatim {
        let first = numbered[start].0;
        let last = numbered[start + fragment.len() - 1].0;
        return CopyState::Unchanged(Some((first, last)));
    }
    let head_set: FxHashSet<&str> = head_lines.iter().copied().collect();
    let surviving = fragment
        .iter()
        .filter(|line| head_set.contains(line.as_str()))
        .count();
    if surviving * 2 < fragment.len() {
        return CopyState::Removed;
    }
    let first_line = fragment
        .iter()
        .find(|line| head_set.contains(line.as_str()))
        .and_then(|line| {
            head_text
                .lines()
                .position(|head| head.trim() == line)
                .and_then(|index| u32::try_from(index + 1).ok())
        });
    CopyState::Edited(first_line)
}

/// Whether a head clone group still joins the edited copy with the whole of
/// a sibling. A group that covers only part of the sibling means the clone
/// shrank: the edit broke the match from that point on.
fn still_clones(head_groups: &[Vec<CloneSpan>], copy_path: &str, siblings: &[CloneSpan]) -> bool {
    head_groups.iter().any(|group| {
        group.iter().any(|instance| instance.path == copy_path)
            && group.iter().any(|instance| {
                siblings.iter().any(|sibling| {
                    sibling.path == instance.path
                        && instance.start_line <= sibling.start_line
                        && sibling.end_line <= instance.end_line
                })
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const COPY: &str = "export function total(items) {\n  let sum = 0;\n  for (const item of items) {\n    sum += item.price * item.quantity;\n  }\n  return sum;\n}\n";
    const EDITED: &str = "export function total(items) {\n  let sum = 0;\n  for (const item of items) {\n    sum += item.price * item.quantity * (1 - item.discount);\n  }\n  return sum;\n}\n";

    fn span(path: &str) -> CloneSpan {
        CloneSpan {
            path: path.to_owned(),
            start_line: 1,
            end_line: 7,
        }
    }

    fn group() -> Vec<BaseCloneGroup> {
        vec![BaseCloneGroup {
            instances: vec![span("src/a.ts"), span("src/b.ts")],
        }]
    }

    fn run(
        changed: &[&str],
        head_a: Option<&str>,
        head_b: &str,
        head_groups: &[Vec<CloneSpan>],
    ) -> Vec<DivergedClone> {
        let changed: FxHashSet<String> = changed.iter().map(|path| (*path).to_owned()).collect();
        let head_a = head_a.map(str::to_owned);
        let head_b = head_b.to_owned();
        let mut base = |_: &str| Some(COPY.to_owned());
        let mut head = move |path: &str| match path {
            "src/a.ts" => head_a.clone(),
            _ => Some(head_b.clone()),
        };
        diverged_clone_siblings(
            &group(),
            head_groups,
            &changed,
            &FxHashMap::default(),
            &mut CloneTexts {
                base: &mut base,
                head: &mut head,
            },
        )
    }

    #[test]
    fn edited_copy_with_unchanged_sibling_is_reported() {
        let found = run(&["src/a.ts"], Some(EDITED), COPY, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, "src/a.ts");
        assert_eq!(found[0].line, Some(1));
        assert_eq!(found[0].siblings, vec![span("src/b.ts")]);
    }

    #[test]
    fn both_copies_edited_is_not_reported() {
        assert!(run(&["src/a.ts", "src/b.ts"], Some(EDITED), EDITED, &[]).is_empty());
    }

    #[test]
    fn edit_that_keeps_the_clone_is_not_reported() {
        let head_groups = vec![vec![span("src/a.ts"), span("src/b.ts")]];
        assert!(run(&["src/a.ts"], Some(EDITED), COPY, &head_groups).is_empty());
    }

    #[test]
    fn edit_that_shrinks_the_clone_to_a_prefix_is_reported() {
        let prefix = |path: &str| CloneSpan {
            path: path.to_owned(),
            start_line: 1,
            end_line: 3,
        };
        let head_groups = vec![vec![prefix("src/a.ts"), prefix("src/b.ts")]];
        assert_eq!(
            run(&["src/a.ts"], Some(EDITED), COPY, &head_groups).len(),
            1
        );
    }

    #[test]
    fn two_groups_on_one_edited_copy_report_once_with_every_sibling() {
        let short = |path: &str| CloneSpan {
            path: path.to_owned(),
            start_line: 1,
            end_line: 6,
        };
        let groups = vec![
            BaseCloneGroup {
                instances: vec![span("src/a.ts"), span("src/b.ts")],
            },
            BaseCloneGroup {
                instances: vec![short("src/a.ts"), short("src/c.ts")],
            },
        ];
        let changed: FxHashSet<String> = std::iter::once("src/a.ts".to_owned()).collect();
        let mut base = |_: &str| Some(COPY.to_owned());
        let mut head = |path: &str| Some(if path == "src/a.ts" { EDITED } else { COPY }.to_owned());
        let found = diverged_clone_siblings(
            &groups,
            &[],
            &changed,
            &FxHashMap::default(),
            &mut CloneTexts {
                base: &mut base,
                head: &mut head,
            },
        );
        assert_eq!(found.len(), 1);
        let paths: Vec<&str> = found[0].siblings.iter().map(|s| s.path.as_str()).collect();
        assert_eq!(paths, vec!["src/b.ts", "src/c.ts"]);
    }

    const SHIFTED: &str = "import { round } from './round';\n";

    fn shifted(path: &str) -> CloneSpan {
        CloneSpan {
            path: path.to_owned(),
            start_line: 2,
            end_line: 8,
        }
    }

    #[test]
    fn sibling_shifted_in_a_changed_file_that_still_clones_is_not_reported() {
        let head_b = format!("{SHIFTED}{COPY}");
        let head_groups = vec![vec![span("src/a.ts"), shifted("src/b.ts")]];
        assert!(
            run(
                &["src/a.ts", "src/b.ts"],
                Some(EDITED),
                &head_b,
                &head_groups
            )
            .is_empty()
        );
    }

    #[test]
    fn sibling_shifted_in_a_changed_file_reports_its_head_lines() {
        let head_b = format!("{SHIFTED}{COPY}");
        let found = run(&["src/a.ts", "src/b.ts"], Some(EDITED), &head_b, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].siblings, vec![shifted("src/b.ts")]);
    }

    #[test]
    fn change_elsewhere_in_the_file_is_not_reported() {
        let head = format!("{COPY}export const other = 1;\n");
        assert!(run(&["src/a.ts"], Some(&head), COPY, &[]).is_empty());
    }

    #[test]
    fn removed_or_deleted_copy_is_not_reported() {
        assert!(run(&["src/a.ts"], Some("export const x = 1;\n"), COPY, &[]).is_empty());
        assert!(run(&["src/a.ts"], None, COPY, &[]).is_empty());
    }
}
