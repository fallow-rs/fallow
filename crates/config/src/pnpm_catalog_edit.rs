//! Edit plans for the catalogs of `pnpm-workspace.yaml`.
//!
//! [`plan_catalog_edits`](crate::pnpm_catalog_edit::plan_catalog_edits) finds each key by its path in the text that it
//! edits. It accepts a plan only when the edited text parses to the
//! original YAML without the requested keys.

use std::ops::Range;

use crate::yaml::{self, YamlMapping, YamlNode};
use crate::{CatalogPrecedingCommentPolicy, KeyLocation, PnpmCatalogData, PnpmWorkspaceDoc};

/// One requested removal, named by its key path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogRemoval<'a> {
    /// An entry of a catalog.
    Entry {
        /// The catalog name. `"default"` is the top-level `catalog` map.
        catalog: &'a str,
        /// The package name of the entry.
        entry: &'a str,
    },
    /// An empty named group under `catalogs`.
    Group {
        /// The group name.
        catalog: &'a str,
    },
}

/// An edit of whole lines. Line indexes are 0-based indexes into
/// `source.split('\n')`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineEdit {
    /// The lines that the edit replaces.
    pub lines: Range<usize>,
    /// `None` deletes `lines`. `Some(text)` replaces the one line of
    /// `lines` with `text`, which has no line terminator.
    pub replacement: Option<String>,
}

/// Why the planner refuses a removal or a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogEditRefusal {
    /// The key path is not in the YAML.
    NotFound,
    /// The group has entries.
    NotEmpty,
    /// The key is not a block-style key, or its catalog header has an
    /// inline value.
    UnsupportedStyle,
    /// The source has more than one YAML document.
    MultiDocument,
    /// The source is not valid YAML.
    InvalidYaml,
    /// The edited YAML differs from the source by more than the removals.
    DeltaMismatch,
}

/// The lines of one planned removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCatalogRemoval {
    /// 0-based line of the key.
    pub key_line: usize,
    /// The deleted lines. `lines.start` is before `key_line` when the
    /// comment policy takes a comment block.
    pub lines: Range<usize>,
}

/// The result for one requested removal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogRemovalOutcome {
    /// The plan removes the key.
    Planned(PlannedCatalogRemoval),
    /// The plan does not remove the key.
    Refused(CatalogEditRefusal),
}

/// The edits of one `pnpm-workspace.yaml` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEditPlan {
    /// One outcome for each requested removal, in request order.
    pub outcomes: Vec<CatalogRemovalOutcome>,
    /// Deletions and header rewrites, sorted by `lines.start` in
    /// descending order. No two edits overlap.
    pub edits: Vec<LineEdit>,
}

/// A removal after the lookup of its key path.
enum Resolved {
    Entry {
        catalog: usize,
        range: Range<usize>,
        key_idx: usize,
    },
    Group {
        range: Range<usize>,
        key_idx: usize,
    },
    Refused(CatalogEditRefusal),
    SameAs(usize),
}

/// Plans the removal of catalog entries and empty catalog groups from
/// `source`.
///
/// # Errors
///
/// Returns a refusal for the whole file when `source` is not one valid
/// YAML document, or when the edited text does not parse to the original
/// YAML without the removed keys.
pub fn plan_catalog_edits(
    source: &str,
    removals: &[CatalogRemoval<'_>],
    comments: CatalogPrecedingCommentPolicy,
) -> Result<CatalogEditPlan, CatalogEditRefusal> {
    let lines: Vec<&str> = source
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    let normalized = lines.join("\n");
    if is_multi_document_yaml(&normalized) {
        return Err(CatalogEditRefusal::MultiDocument);
    }
    let doc = PnpmWorkspaceDoc::parse(&normalized).map_err(|_| CatalogEditRefusal::InvalidYaml)?;
    let before = yaml::parse(&normalized).map_err(|_| CatalogEditRefusal::InvalidYaml)?;
    let data = doc.catalogs();

    let mut resolved: Vec<Resolved> = removals
        .iter()
        .enumerate()
        .map(|(index, removal)| {
            if let Some(first) = removals[..index].iter().position(|other| other == removal) {
                return Resolved::SameAs(first);
            }
            resolve(removal, data, &lines, comments)
        })
        .collect();

    let mut edits = header_rewrites(&mut resolved, data, &lines);
    for item in &resolved {
        if let Resolved::Entry { range, .. } | Resolved::Group { range, .. } = item {
            edits.push(LineEdit {
                lines: range.clone(),
                replacement: None,
            });
        }
    }
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.lines.start));
    if edits
        .windows(2)
        .any(|pair| pair[1].lines.end > pair[0].lines.start)
    {
        return Err(CatalogEditRefusal::DeltaMismatch);
    }

    let outcomes = outcomes_of(&resolved);
    let mut removed_entries: Vec<(&str, &str)> = Vec::new();
    let mut removed_groups: Vec<&str> = Vec::new();
    for (removal, item) in removals.iter().zip(&resolved) {
        match (removal, item) {
            (CatalogRemoval::Entry { catalog, entry }, Resolved::Entry { .. }) => {
                removed_entries.push((catalog, entry));
            }
            (CatalogRemoval::Group { catalog }, Resolved::Group { .. }) => {
                removed_groups.push(catalog);
            }
            _ => {}
        }
    }
    if removed_entries.is_empty() && removed_groups.is_empty() {
        return Ok(CatalogEditPlan {
            outcomes,
            edits: Vec::new(),
        });
    }

    let mut edited: Vec<String> = lines.iter().map(ToString::to_string).collect();
    apply_line_edits(&mut edited, &edits);
    let after = yaml::parse(&edited.join("\n")).map_err(|_| CatalogEditRefusal::DeltaMismatch)?;
    if !delta_matches(
        before.root(),
        after.root(),
        &removed_entries,
        &removed_groups,
    ) {
        return Err(CatalogEditRefusal::DeltaMismatch);
    }
    Ok(CatalogEditPlan { outcomes, edits })
}

/// Applies `edits` to `lines`. `edits` must be in the order of
/// [`CatalogEditPlan::edits`].
pub fn apply_line_edits(lines: &mut Vec<String>, edits: &[LineEdit]) {
    for edit in edits {
        match &edit.replacement {
            Some(text) => lines[edit.lines.start].clone_from(text),
            None => {
                lines.drain(edit.lines.clone());
            }
        }
    }
}

/// Whether `content` has a `---` YAML document separator on its own line.
#[must_use]
pub fn is_multi_document_yaml(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.trim_end() == "---" || line.trim_end().starts_with("--- "))
}

fn resolve(
    removal: &CatalogRemoval<'_>,
    data: &PnpmCatalogData,
    lines: &[&str],
    comments: CatalogPrecedingCommentPolicy,
) -> Resolved {
    match *removal {
        CatalogRemoval::Entry { catalog, entry } => {
            let mut matches = data
                .catalogs
                .iter()
                .enumerate()
                .filter(|(_, candidate)| candidate.name == catalog);
            let (Some((index, found)), None) = (matches.next(), matches.next()) else {
                return Resolved::Refused(CatalogEditRefusal::NotFound);
            };
            let Some(found) = found
                .entries
                .iter()
                .find(|candidate| candidate.package_name == entry)
            else {
                return Resolved::Refused(CatalogEditRefusal::NotFound);
            };
            match block_lines(found.location, lines.len()) {
                Ok((key_idx, end)) => Resolved::Entry {
                    catalog: index,
                    range: comment_block_start(lines, key_idx, comments).unwrap_or(key_idx)..end,
                    key_idx,
                },
                Err(refusal) => Resolved::Refused(refusal),
            }
        }
        CatalogRemoval::Group { catalog } => {
            if data
                .catalogs
                .iter()
                .any(|candidate| candidate.name == catalog)
            {
                return Resolved::Refused(CatalogEditRefusal::NotEmpty);
            }
            let Some(group) = data
                .empty_named_catalog_groups
                .iter()
                .find(|candidate| candidate.name == catalog)
            else {
                return Resolved::Refused(CatalogEditRefusal::NotFound);
            };
            match block_lines(group.location, lines.len()) {
                Ok((key_idx, end)) => Resolved::Group {
                    range: key_idx..end,
                    key_idx,
                },
                Err(refusal) => Resolved::Refused(refusal),
            }
        }
    }
}

/// The 0-based key line and the exclusive end line of a block-style key.
fn block_lines(
    location: KeyLocation,
    line_count: usize,
) -> Result<(usize, usize), CatalogEditRefusal> {
    let KeyLocation::Block { line, end_line } = location else {
        return Err(CatalogEditRefusal::UnsupportedStyle);
    };
    let end = end_line as usize;
    if line == 0 || end_line < line || end > line_count {
        return Err(CatalogEditRefusal::UnsupportedStyle);
    }
    Ok((line as usize - 1, end))
}

/// The `{}` rewrites of the headers of catalogs that the plan empties.
/// A catalog whose header the planner cannot rewrite gets its entries
/// refused.
fn header_rewrites(
    resolved: &mut [Resolved],
    data: &PnpmCatalogData,
    lines: &[&str],
) -> Vec<LineEdit> {
    let mut edits = Vec::new();
    for (index, catalog) in data.catalogs.iter().enumerate() {
        let key_lines: Vec<usize> = resolved
            .iter()
            .filter_map(|item| match item {
                Resolved::Entry {
                    catalog, key_idx, ..
                } if *catalog == index => Some(*key_idx),
                _ => None,
            })
            .collect();
        let Some(&first_key_idx) = key_lines.iter().min() else {
            continue;
        };
        if key_lines.len() != catalog.entries.len() {
            continue;
        }
        let default_keys = ["catalog", "default"];
        let named_key = [catalog.name.as_str()];
        let expected: &[&str] = if catalog.name == "default" {
            &default_keys
        } else {
            &named_key
        };
        match header_rewrite(lines, first_key_idx, expected) {
            Some(edit) => edits.push(edit),
            None => {
                for item in resolved.iter_mut() {
                    if matches!(item, Resolved::Entry { catalog, .. } if *catalog == index) {
                        *item = Resolved::Refused(CatalogEditRefusal::UnsupportedStyle);
                    }
                }
            }
        }
    }
    edits
}

/// The `{}` rewrite of the header above `first_key_idx`. The default
/// catalog is the top-level `catalog` key or the `default` group under
/// `catalogs`, so its header can have either key.
fn header_rewrite(lines: &[&str], first_key_idx: usize, expected: &[&str]) -> Option<LineEdit> {
    let idx = find_parent_header(lines, first_key_idx)?;
    let (key, colon_end) = split_header(lines[idx])?;
    let rest = &lines[idx][colon_end..];
    let after = rest.trim_start();
    if !expected.contains(&key) || !(after.is_empty() || after.starts_with('#')) {
        return None;
    }
    Some(LineEdit {
        lines: idx..idx + 1,
        replacement: Some(format!("{} {{}}{rest}", &lines[idx][..colon_end])),
    })
}

fn outcomes_of(resolved: &[Resolved]) -> Vec<CatalogRemovalOutcome> {
    let mut outcomes: Vec<CatalogRemovalOutcome> = Vec::with_capacity(resolved.len());
    for item in resolved {
        let outcome = match item {
            Resolved::Entry { range, key_idx, .. } | Resolved::Group { range, key_idx } => {
                CatalogRemovalOutcome::Planned(PlannedCatalogRemoval {
                    key_line: *key_idx,
                    lines: range.clone(),
                })
            }
            Resolved::Refused(refusal) => CatalogRemovalOutcome::Refused(*refusal),
            Resolved::SameAs(first) => outcomes[*first].clone(),
        };
        outcomes.push(outcome);
    }
    outcomes
}

/// Whether `after` equals `before` without the removed entries and groups.
fn delta_matches(
    before: YamlNode<'_>,
    after: YamlNode<'_>,
    removed_entries: &[(&str, &str)],
    removed_groups: &[&str],
) -> bool {
    let (Some(before), Some(after)) = (before.as_mapping(), after.as_mapping()) else {
        return false;
    };
    if before.len() != after.len() {
        return false;
    }
    before
        .iter()
        .zip(after.iter())
        .all(|((key, value), (after_key, after_value))| {
            if key != after_key {
                return false;
            }
            match key.as_str() {
                Some("catalog") => {
                    catalog_matches(value, after_value, &removed_in(removed_entries, "default"))
                }
                Some("catalogs") => {
                    catalogs_match(value, after_value, removed_entries, removed_groups)
                }
                _ => value == after_value,
            }
        })
}

fn removed_in<'a>(removed_entries: &[(&str, &'a str)], catalog: &str) -> Vec<&'a str> {
    removed_entries
        .iter()
        .filter(|(name, _)| *name == catalog)
        .map(|(_, entry)| *entry)
        .collect()
}

fn catalog_matches(before: YamlNode<'_>, after: YamlNode<'_>, removed: &[&str]) -> bool {
    match before.as_mapping() {
        Some(mapping) => pairs_match(mapping, after, removed),
        None => before == after,
    }
}

fn catalogs_match(
    before: YamlNode<'_>,
    after: YamlNode<'_>,
    removed_entries: &[(&str, &str)],
    removed_groups: &[&str],
) -> bool {
    let Some(groups) = before.as_mapping() else {
        return before == after;
    };
    let kept: Vec<(YamlNode<'_>, YamlNode<'_>)> = groups
        .iter()
        .filter(|(key, _)| {
            !key.as_str()
                .is_some_and(|name| removed_groups.contains(&name))
        })
        .collect();
    if kept.is_empty() {
        return after.is_null() || after.as_mapping().is_some_and(YamlMapping::is_empty);
    }
    let Some(after) = after.as_mapping() else {
        return false;
    };
    if after.len() != kept.len() {
        return false;
    }
    kept.iter()
        .zip(after.iter())
        .all(|((key, value), (after_key, after_value))| {
            if *key != after_key {
                return false;
            }
            let removed = key
                .as_str()
                .map(|name| removed_in(removed_entries, name))
                .unwrap_or_default();
            catalog_matches(*value, after_value, &removed)
        })
}

fn pairs_match(before: YamlMapping<'_>, after: YamlNode<'_>, removed: &[&str]) -> bool {
    let Some(after) = after.as_mapping() else {
        return false;
    };
    let kept: Vec<(YamlNode<'_>, YamlNode<'_>)> = before
        .iter()
        .filter(|(key, _)| !key.as_str().is_some_and(|name| removed.contains(&name)))
        .collect();
    kept.len() == after.len() && kept.into_iter().eq(after.iter())
}

/// Splits a block mapping line into its key, without quotes, and the
/// byte index after the `:` of the key.
fn split_header(line: &str) -> Option<(&str, usize)> {
    let indent = line.len() - line.trim_start().len();
    let text = &line[indent..];
    for quote in ['"', '\''] {
        if let Some(rest) = text.strip_prefix(quote) {
            let close = rest.find(quote)?;
            let after = &rest[close + 1..];
            let colon = after.find(':')?;
            if !after[..colon].trim().is_empty() {
                return None;
            }
            return Some((&rest[..close], indent + close + colon + 3));
        }
    }
    let colon = text.find(':')?;
    Some((text[..colon].trim_end(), indent + colon + 1))
}

/// The nearest line above `key_idx` that is not blank, is not a comment
/// and has a smaller indent than the line `key_idx`.
fn find_parent_header(lines: &[&str], key_idx: usize) -> Option<usize> {
    let key_indent = leading_spaces(lines[key_idx]);
    (0..key_idx).rev().find(|&idx| {
        let content = lines[idx].trim();
        !content.is_empty() && !content.starts_with('#') && leading_spaces(lines[idx]) < key_indent
    })
}

/// The first line of the comment block above `key_idx` that the policy
/// deletes with the entry.
fn comment_block_start(
    lines: &[&str],
    key_idx: usize,
    policy: CatalogPrecedingCommentPolicy,
) -> Option<usize> {
    if matches!(policy, CatalogPrecedingCommentPolicy::Never) || key_idx == 0 {
        return None;
    }
    let indent = leading_spaces(lines[key_idx]);
    let mut start = key_idx;
    while start > 0 && is_entry_comment(lines[start - 1], indent) {
        start -= 1;
    }
    if start == key_idx {
        return None;
    }
    let block = &lines[start..key_idx];
    if block.iter().any(|line| line.contains("fallow-keep")) {
        return None;
    }
    match policy {
        CatalogPrecedingCommentPolicy::Always => Some(start),
        CatalogPrecedingCommentPolicy::Never => None,
        CatalogPrecedingCommentPolicy::Auto => {
            if block.iter().any(|line| is_section_banner_line(line)) {
                return None;
            }
            let before = start.checked_sub(1)?;
            (lines[before].trim().is_empty() || find_parent_header(lines, key_idx) == Some(before))
                .then_some(start)
        }
    }
}

fn is_entry_comment(line: &str, entry_indent: usize) -> bool {
    leading_spaces(line) == entry_indent && line.trim_start().starts_with('#')
}

/// Recognize banner-shaped comment lines like `# ====`, `# ----`, `# ====
/// React 18 pins ====`. Returns true when the comment body (after `#` and
/// optional leading whitespace) starts with 3+ repeats of `=`, `-`, `*`,
/// `_`, `~`, `+`, or `#`. Used by the Auto policy to preserve section
/// dividers above the next catalog entry.
fn is_section_banner_line(line: &str) -> bool {
    let Some(after_hash) = line.trim_start().strip_prefix('#') else {
        return false;
    };
    let body = after_hash.trim_start();
    let Some(first) = body.chars().next() else {
        return false;
    };
    if !matches!(first, '=' | '-' | '*' | '_' | '~' | '+' | '#') {
        return false;
    }
    body.chars().take(3).all(|c| c == first)
}

fn leading_spaces(line: &str) -> usize {
    line.bytes().take_while(|&b| b == b' ').count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CatalogPrecedingCommentPolicy::{Auto, Never};

    fn entry<'a>(catalog: &'a str, entry: &'a str) -> CatalogRemoval<'a> {
        CatalogRemoval::Entry { catalog, entry }
    }

    fn apply(source: &str, plan: &CatalogEditPlan) -> String {
        let mut lines: Vec<String> = source.split('\n').map(str::to_string).collect();
        apply_line_edits(&mut lines, &plan.edits);
        lines.join("\n")
    }

    fn planned(plan: &CatalogEditPlan, index: usize) -> &PlannedCatalogRemoval {
        match &plan.outcomes[index] {
            CatalogRemovalOutcome::Planned(planned) => planned,
            CatalogRemovalOutcome::Refused(refusal) => panic!("refused: {refusal:?}"),
        }
    }

    fn refused(source: &str, removal: CatalogRemoval<'_>) -> (CatalogEditRefusal, usize) {
        let plan = plan_catalog_edits(source, &[removal], Never).unwrap();
        match plan.outcomes[0] {
            CatalogRemovalOutcome::Refused(refusal) => (refusal, plan.edits.len()),
            CatalogRemovalOutcome::Planned(_) => panic!("planned"),
        }
    }

    #[test]
    fn entry_and_group_use_one_snapshot() {
        let source =
            "catalog:\n  used: ^1\n  unused: ^1\ncatalogs:\n  old:\nlinkWorkspacePackages: true\n";
        let removals = [
            entry("default", "unused"),
            CatalogRemoval::Group { catalog: "old" },
        ];
        let plan = plan_catalog_edits(source, &removals, Never).unwrap();
        assert_eq!(
            apply(source, &plan),
            "catalog:\n  used: ^1\ncatalogs:\nlinkWorkspacePackages: true\n"
        );
    }

    #[test]
    fn empty_map_goes_before_inline_comment() {
        let source = "catalogs:\n  ui: # pinned versions\n    react: ^18\n";
        let plan = plan_catalog_edits(source, &[entry("ui", "react")], Never).unwrap();
        let edited = apply(source, &plan);
        assert_eq!(edited, "catalogs:\n  ui: {} # pinned versions\n");
        let document = yaml::parse(&edited).unwrap();
        let ui = document.root().get("catalogs").and_then(|c| c.get("ui"));
        assert!(
            ui.and_then(YamlNode::as_mapping)
                .is_some_and(YamlMapping::is_empty)
        );
    }

    #[test]
    fn object_entry_with_blank_line_is_one_block() {
        let source = "catalog:\n  react:\n\n    specifier: ^18\n  vue:\n    specifier: ^3\n";
        let plan = plan_catalog_edits(source, &[entry("default", "react")], Never).unwrap();
        assert_eq!(planned(&plan, 0).lines, 1..4);
        assert_eq!(
            apply(source, &plan),
            "catalog:\n  vue:\n    specifier: ^3\n"
        );
    }

    #[test]
    fn scalar_entry_keeps_following_blank_line() {
        let source = "catalog:\n  is-even: ^1.0.0\n\n  is-odd: ^1.0.0\n";
        let plan = plan_catalog_edits(source, &[entry("default", "is-even")], Never).unwrap();
        assert_eq!(planned(&plan, 0).lines, 1..2);
    }

    #[test]
    fn object_entry_spans_until_indent_drops() {
        let source = "catalog:\n  react:\n    specifier: ^18.2.0\n    publishConfig: {}\n  is-even: ^1.0.0\n";
        let plan = plan_catalog_edits(source, &[entry("default", "react")], Never).unwrap();
        assert_eq!(planned(&plan, 0).lines, 1..4);
    }

    #[test]
    fn auto_policy_takes_comment_under_the_header() {
        let source = "catalog:\n  # note\n  is-even: ^1.0.0\n  is-odd: ^1.0.0\n";
        let plan = plan_catalog_edits(source, &[entry("default", "is-even")], Auto).unwrap();
        let removal = planned(&plan, 0);
        assert_eq!(removal.key_line, 2);
        assert_eq!(removal.lines, 1..3);
    }

    #[test]
    fn last_entry_rewrites_the_header() {
        let source = "catalogs:\n  \"日本\":\n    react: ^17.0.2\n";
        let plan = plan_catalog_edits(source, &[entry("日本", "react")], Never).unwrap();
        assert_eq!(apply(source, &plan), "catalogs:\n  \"日本\": {}\n");
    }

    #[test]
    fn default_group_under_catalogs_rewrites_its_header() {
        let source = "catalogs:\n  default:\n    react: ^18\n";
        let plan = plan_catalog_edits(source, &[entry("default", "react")], Never).unwrap();
        assert_eq!(apply(source, &plan), "catalogs:\n  default: {}\n");
    }

    #[test]
    fn group_with_children_is_refused() {
        let source = "catalogs:\n  legacy:\n    react: ^18\n";
        let refusal = refused(source, CatalogRemoval::Group { catalog: "legacy" });
        assert_eq!(refusal, (CatalogEditRefusal::NotEmpty, 0));
    }

    #[test]
    fn entry_of_other_catalog_is_not_found() {
        let source = "catalogs:\n  b:\n    react: ^18\n";
        let refusal = refused(source, entry("a", "react"));
        assert_eq!(refusal.0, CatalogEditRefusal::NotFound);
    }

    #[test]
    fn flow_style_entry_is_refused() {
        let source = "catalogs:\n  ui: { react: \"18\" }\n";
        let refusal = refused(source, entry("ui", "react"));
        assert_eq!(refusal, (CatalogEditRefusal::UnsupportedStyle, 0));
    }

    #[test]
    fn empty_group_forms_are_removed() {
        let source = "catalogs:\n  react17: {}\n  legacy:\n  vue3:\n    vue: ^3\n";
        let removals = [
            CatalogRemoval::Group { catalog: "react17" },
            CatalogRemoval::Group { catalog: "legacy" },
        ];
        let plan = plan_catalog_edits(source, &removals, Never).unwrap();
        assert_eq!(apply(source, &plan), "catalogs:\n  vue3:\n    vue: ^3\n");
    }

    #[test]
    fn multi_document_is_refused() {
        let source = "catalog:\n  a: 1\n---\nfoo: bar\n";
        let result = plan_catalog_edits(source, &[entry("default", "a")], Never);
        assert_eq!(result, Err(CatalogEditRefusal::MultiDocument));
    }

    #[test]
    fn invalid_yaml_is_refused() {
        let source = "catalog:\n  a: [1\n";
        let result = plan_catalog_edits(source, &[entry("default", "a")], Never);
        assert_eq!(result, Err(CatalogEditRefusal::InvalidYaml));
    }

    #[test]
    fn crlf_source_gives_the_same_line_indexes() {
        let source = "catalog:\r\n  is-odd: ^1\r\n  is-even: ^1\r\n";
        let plan = plan_catalog_edits(source, &[entry("default", "is-even")], Never).unwrap();
        assert_eq!(planned(&plan, 0).lines, 2..3);
        assert!(
            plan.edits
                .iter()
                .filter_map(|edit| edit.replacement.as_deref())
                .all(|text| !text.contains('\r'))
        );
    }

    #[test]
    fn removal_of_an_anchor_that_an_alias_uses_is_refused() {
        let source = "catalog:\n  react: &v ^18\n  vue: *v\n";
        let result = plan_catalog_edits(source, &[entry("default", "react")], Never);
        assert_eq!(result, Err(CatalogEditRefusal::DeltaMismatch));
    }

    /// A catalog map with an anchor can be the value of another key through
    /// an alias or a merge key. A removal from the map then also removes the
    /// key from that other value, and the edited file still parses. The
    /// delta check sees the changed alias target and refuses the edit.
    #[test]
    fn removal_from_a_catalog_map_that_another_key_aliases_is_refused() {
        let cases: [(&str, CatalogRemoval<'_>); 3] = [
            (
                "catalogs:\n  pins: &pins\n    minimist: 1.2.8\n    tar: 6.2.1\n\noverrides: *pins\n",
                entry("pins", "tar"),
            ),
            (
                "catalogs:\n  pins: &pins\n    minimist: 1.2.8\n    tar: 6.2.1\noverrides:\n  <<: *pins\n  lodash: 4.17.21\n",
                entry("pins", "tar"),
            ),
            (
                "catalog: &c\n  react: ^18\n  tar: 6.2.1\noverrides: *c\n",
                entry("default", "tar"),
            ),
        ];
        for (source, removal) in cases {
            let result = plan_catalog_edits(source, &[removal], Never);
            let refused = match &result {
                Err(_) => true,
                Ok(plan) => {
                    plan.edits.is_empty()
                        && matches!(plan.outcomes[0], CatalogRemovalOutcome::Refused(_))
                }
            };
            assert!(refused, "{source:?} gave {result:?}");
        }
    }

    /// The delta check refuses a removal that changes a merged map, and it
    /// keeps a removal that changes only the removed entry. With a merge key,
    /// the removal of `tar` from `app` would give `app` the `tar` version of
    /// `base`, so that removal is refused.
    #[test]
    fn removal_from_a_catalog_map_with_an_anchor_or_merge_key_follows_the_delta_check() {
        let shadowing =
            "catalogs:\n  base: &base\n    tar: 6.0.0\n  app:\n    <<: *base\n    tar: 6.2.1\n";
        assert_eq!(
            plan_catalog_edits(shadowing, &[entry("app", "tar")], Never),
            Err(CatalogEditRefusal::DeltaMismatch)
        );
        for (source, removal, expected) in [
            (
                "catalog: &c\n  react: ^18\n  tar: 6.2.1\n",
                entry("default", "tar"),
                "catalog: &c\n  react: ^18\n",
            ),
            (
                "catalogs:\n  base: &base\n    react: ^18\n  app:\n    <<: *base\n    react: ^18\n    tar: 6.2.1\n",
                entry("app", "tar"),
                "catalogs:\n  base: &base\n    react: ^18\n  app:\n    <<: *base\n    react: ^18\n",
            ),
        ] {
            let plan = plan_catalog_edits(source, &[removal], Never).unwrap();
            assert_eq!(apply(source, &plan), expected, "{source:?}");
        }
    }

    #[test]
    fn delta_check_rejects_a_changed_tree() {
        let check = |before: &str, after: &str, removed: (&str, &str)| {
            let before = yaml::parse(before).unwrap();
            let after = yaml::parse(after).unwrap();
            delta_matches(before.root(), after.root(), &[removed], &[])
        };
        let source = "catalog:\n  a: 1\n  b: 2\nother: true\n";
        assert!(!check(source, "catalog:\n  b: 2\n", ("default", "a")));
        assert!(check(
            source,
            "catalog:\n  b: 2\nother: true\n",
            ("default", "a")
        ));
        assert!(!check(
            "catalogs:\n  ui:\n    react: 1\n",
            "catalogs:\n  ui:\n",
            ("ui", "react")
        ));
    }

    #[test]
    fn text_helpers_match_the_cli_rules() {
        assert!(is_multi_document_yaml("foo: bar\n---\nbaz: qux\n"));
        assert!(is_multi_document_yaml("---\nfoo: bar\n"));
        assert!(!is_multi_document_yaml("catalog:\n  is-even: ^1.0.0\n"));
        assert!(!is_multi_document_yaml("catalog:\n  foo: \"---\"\n"));
        assert!(is_section_banner_line("# === banner ==="));
        assert!(is_section_banner_line("  # ----"));
        assert!(is_section_banner_line("# ***"));
        assert!(is_section_banner_line("# ___"));
        assert!(is_section_banner_line("#==="));
        assert!(!is_section_banner_line("# mention is-even"));
        assert!(!is_section_banner_line("# = single sep"));
        assert!(!is_section_banner_line("# -- two seps only"));
        assert!(!is_section_banner_line("not a comment"));
    }
}
