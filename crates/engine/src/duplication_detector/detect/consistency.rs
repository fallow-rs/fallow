//! Consistent-identifier refinement of raw clone groups.
//!
//! With `normalization.ignoreIdentifiers`, every identifier hashes to one
//! value, so the matcher pairs `a = b + c` with `x = x + x`. With
//! `normalization.consistentIdentifiers`, this step checks each raw group
//! after the hash match: the names of each instance must map one-to-one to
//! the names of a reference instance.
//!
//! The rule has two parts:
//!
//! 1. Full-length classes. Instances that map one-to-one over the full
//!    length form one class. A one-to-one mapping is an equivalence
//!    relation, so the classes partition the instances. Each class with two
//!    or more instances stays a clone of the full length.
//! 2. Split. When the group has more than one class, the step also scans
//!    all instances together against the first instance. At the first token
//!    where one instance breaks its mapping, the current segment ends and a
//!    new segment starts at that token with empty mappings. Each segment of
//!    `min_tokens` or more tokens becomes a clone of all instances.
//!
//! Every emitted clone is consistent. The split is greedy, so it can miss a
//! longer consistent segment that starts before a break. The later subset
//! removal drops a segment that a full-length class already covers.

use rustc_hash::FxHashMap;

use super::FileData;
use super::extraction::RawGroup;
use crate::duplicates::tokenize::TokenKind;

/// Refine raw groups so that each emitted group has a one-to-one identifier
/// mapping between its instances.
pub(super) fn refine_groups(
    raw_groups: Vec<RawGroup>,
    files: &[FileData],
    min_tokens: usize,
) -> Vec<RawGroup> {
    let mut refined = Vec::with_capacity(raw_groups.len());
    for group in raw_groups {
        refine_group(group, files, min_tokens, &mut refined);
    }
    refined
}

fn refine_group(group: RawGroup, files: &[FileData], min_tokens: usize, out: &mut Vec<RawGroup>) {
    let names: Vec<Vec<Option<&str>>> = group
        .instances
        .iter()
        .map(|&(file_id, offset)| identifier_names(&files[file_id], offset, group.length))
        .collect();

    let classes = full_length_classes(&names);
    if classes.len() == 1 {
        out.push(group);
        return;
    }
    for class in &classes {
        if class.len() >= 2 {
            out.push(RawGroup {
                instances: class.iter().map(|&i| group.instances[i]).collect(),
                length: group.length,
            });
        }
    }
    for (start, end) in consistent_segments(&names, group.length) {
        if end - start >= min_tokens {
            out.push(RawGroup {
                instances: group
                    .instances
                    .iter()
                    .map(|&(file_id, offset)| (file_id, offset + start))
                    .collect(),
                length: end - start,
            });
        }
    }
}

/// The identifier name at each position of one instance, `None` for other
/// tokens.
fn identifier_names(file: &FileData, offset: usize, length: usize) -> Vec<Option<&str>> {
    file.hashed_tokens[offset..offset + length]
        .iter()
        .map(
            |hashed| match &file.file_tokens.tokens[hashed.original_index].kind {
                TokenKind::Identifier(name) => Some(name.as_str()),
                _ => None,
            },
        )
        .collect()
}

/// A one-to-one mapping between the names of two instances.
#[derive(Default)]
struct Bijection<'a> {
    forward: FxHashMap<&'a str, &'a str>,
    backward: FxHashMap<&'a str, &'a str>,
}

impl<'a> Bijection<'a> {
    /// Add the pair `left -> right`. Returns `false` when the pair breaks
    /// the mapping.
    fn accept(&mut self, left: &'a str, right: &'a str) -> bool {
        match (self.forward.get(left), self.backward.get(right)) {
            (Some(&mapped), _) if mapped != right => false,
            (_, Some(&mapped)) if mapped != left => false,
            (Some(_), Some(_)) => true,
            _ => {
                self.forward.insert(left, right);
                self.backward.insert(right, left);
                true
            }
        }
    }

    fn clear(&mut self) {
        self.forward.clear();
        self.backward.clear();
    }
}

fn accept_position<'a>(
    mapping: &mut Bijection<'a>,
    reference: Option<&'a str>,
    other: Option<&'a str>,
) -> bool {
    match (reference, other) {
        (Some(left), Some(right)) => mapping.accept(left, right),
        _ => true,
    }
}

fn maps_one_to_one(reference: &[Option<&str>], other: &[Option<&str>]) -> bool {
    let mut mapping = Bijection::default();
    reference
        .iter()
        .zip(other)
        .all(|(&left, &right)| accept_position(&mut mapping, left, right))
}

/// Partition instance indices into classes that map one-to-one over the full
/// length.
fn full_length_classes(names: &[Vec<Option<&str>>]) -> Vec<Vec<usize>> {
    let mut classes: Vec<Vec<usize>> = Vec::new();
    for index in 0..names.len() {
        match classes
            .iter_mut()
            .find(|class| maps_one_to_one(&names[class[0]], &names[index]))
        {
            Some(class) => class.push(index),
            None => classes.push(vec![index]),
        }
    }
    classes
}

/// Greedy `[start, end)` segments in which every instance maps one-to-one to
/// the first instance.
fn consistent_segments(names: &[Vec<Option<&str>>], length: usize) -> Vec<(usize, usize)> {
    let reference = &names[0];
    let mut mappings: Vec<Bijection<'_>> = std::iter::repeat_with(Bijection::default)
        .take(names.len() - 1)
        .collect();
    let mut segments = Vec::new();
    let mut start = 0;
    for position in 0..length {
        let breaks = names[1..]
            .iter()
            .zip(mappings.iter_mut())
            .any(|(other, mapping)| {
                !accept_position(mapping, reference[position], other[position])
            });
        if !breaks {
            continue;
        }
        segments.push((start, position));
        start = position;
        for (other, mapping) in names[1..].iter().zip(mappings.iter_mut()) {
            mapping.clear();
            accept_position(mapping, reference[position], other[position]);
        }
    }
    segments.push((start, length));
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names<'a>(tokens: &[&'a str]) -> Vec<Option<&'a str>> {
        tokens
            .iter()
            .map(|&token| (token != "+" && token != "=").then_some(token))
            .collect()
    }

    #[test]
    fn bijection_rejects_two_names_for_one() {
        let mut mapping = Bijection::default();
        assert!(mapping.accept("a", "x"));
        assert!(mapping.accept("b", "y"));
        assert!(mapping.accept("a", "x"));
        assert!(!mapping.accept("c", "x"), "x already stands for a");
        assert!(!mapping.accept("a", "z"), "a already maps to x");
    }

    #[test]
    fn consistent_rename_is_one_class() {
        let reference = names(&["a", "=", "b", "+", "c"]);
        let renamed = names(&["x", "=", "y", "+", "z"]);
        assert_eq!(full_length_classes(&[reference, renamed]), vec![vec![0, 1]]);
    }

    #[test]
    fn inconsistent_rename_splits_at_the_conflict() {
        let reference = names(&["a", "=", "b", "+", "c"]);
        let collapsed = names(&["x", "=", "x", "+", "x"]);
        let all = [reference, collapsed];
        assert_eq!(full_length_classes(&all).len(), 2);
        assert_eq!(consistent_segments(&all, 5), vec![(0, 2), (2, 4), (4, 5)]);
    }

    #[test]
    fn segments_cover_the_full_length_without_conflict() {
        let reference = names(&["a", "+", "a"]);
        let renamed = names(&["b", "+", "b"]);
        assert_eq!(consistent_segments(&[reference, renamed], 3), vec![(0, 3)]);
    }
}
