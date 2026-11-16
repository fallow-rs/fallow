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
//! 2. Windows. When the group has more than one class, the step compares the
//!    first instance of each class with the first instance of each other
//!    class. For each pair it finds every maximal window that maps
//!    one-to-one: at a token that breaks the mapping, the next window starts
//!    one token after the last earlier occurrence of the conflicting name.
//!    Pairs of classes that share one window merge into one clone, because
//!    one-to-one mappings compose. Each window of `min_tokens` or more tokens
//!    stays a clone.
//!
//! Every emitted clone is consistent. The later subset removal drops a
//! window that a longer clone already covers. Above
//! `MAX_PAIRWISE_CLASSES` classes, the step compares each class only with
//! the first class, so the work stays bounded.

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
        refine_group(&group, files, min_tokens, &mut refined);
    }
    refined
}

fn refine_group(group: &RawGroup, files: &[FileData], min_tokens: usize, out: &mut Vec<RawGroup>) {
    let names: Vec<Vec<Option<&str>>> = group
        .instances
        .iter()
        .map(|&(file_id, offset)| identifier_names(&files[file_id], offset, group.length))
        .collect();
    for planned in plan_groups(&names, group.length, min_tokens) {
        out.push(RawGroup {
            instances: planned
                .instances
                .iter()
                .map(|&index| {
                    let (file_id, offset) = group.instances[index];
                    (file_id, offset + planned.start)
                })
                .collect(),
            length: planned.length,
        });
    }
}

/// One consistent clone: instance indices into the raw group, and a token
/// range relative to the start of each instance.
#[derive(Debug, PartialEq, Eq)]
struct PlannedGroup {
    instances: Vec<usize>,
    start: usize,
    length: usize,
}

fn plan_groups(names: &[Vec<Option<&str>>], length: usize, min_tokens: usize) -> Vec<PlannedGroup> {
    let classes = full_length_classes(names);
    if classes.len() == 1 {
        return vec![PlannedGroup {
            instances: (0..names.len()).collect(),
            start: 0,
            length,
        }];
    }
    let mut planned = Vec::new();
    for class in &classes {
        if class.len() >= 2 {
            planned.push(PlannedGroup {
                instances: class.clone(),
                start: 0,
                length,
            });
        }
    }
    planned.extend(window_groups(names, &classes, min_tokens));
    planned
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

/// Above this number of classes, each class is compared only with the first
/// class instead of with every other class.
const MAX_PAIRWISE_CLASSES: usize = 32;

/// Clones of the windows that two or more classes share.
fn window_groups(
    names: &[Vec<Option<&str>>],
    classes: &[Vec<usize>],
    min_tokens: usize,
) -> Vec<PlannedGroup> {
    let mut pairs_by_window: FxHashMap<(usize, usize), Vec<(usize, usize)>> = FxHashMap::default();
    let firsts = if classes.len() > MAX_PAIRWISE_CLASSES {
        1
    } else {
        classes.len()
    };
    for left in 0..firsts {
        for right in left + 1..classes.len() {
            let windows = maximal_windows(&names[classes[left][0]], &names[classes[right][0]]);
            for window in windows {
                if window.1 - window.0 >= min_tokens {
                    pairs_by_window
                        .entry(window)
                        .or_default()
                        .push((left, right));
                }
            }
        }
    }
    let mut windows: Vec<_> = pairs_by_window.into_iter().collect();
    windows.sort_unstable_by_key(|&(window, _)| window);
    let mut planned = Vec::new();
    for ((start, end), pairs) in windows {
        for component in connected_classes(classes.len(), &pairs) {
            let mut instances: Vec<usize> = component
                .iter()
                .flat_map(|&class| classes[class].iter().copied())
                .collect();
            instances.sort_unstable();
            planned.push(PlannedGroup {
                instances,
                start,
                length: end - start,
            });
        }
    }
    planned
}

/// Connected components of the classes that the pairs link, in order of
/// their smallest class. Classes without a pair are left out.
fn connected_classes(class_count: usize, pairs: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut parent: Vec<usize> = (0..class_count).collect();
    let find = |parent: &mut Vec<usize>, mut class: usize| {
        while parent[class] != class {
            parent[class] = parent[parent[class]];
            class = parent[class];
        }
        class
    };
    for &(left, right) in pairs {
        let (left, right) = (find(&mut parent, left), find(&mut parent, right));
        parent[left.max(right)] = left.min(right);
    }
    let mut linked = vec![false; class_count];
    for &(left, right) in pairs {
        linked[left] = true;
        linked[right] = true;
    }
    let mut components: Vec<Vec<usize>> = Vec::new();
    let mut component_of_root: FxHashMap<usize, usize> = FxHashMap::default();
    for class in (0..class_count).filter(|&class| linked[class]) {
        let root = find(&mut parent, class);
        let index = *component_of_root.entry(root).or_insert_with(|| {
            components.push(Vec::new());
            components.len() - 1
        });
        components[index].push(class);
    }
    components
}

/// Every maximal `[start, end)` window in which the names of two instances
/// map one-to-one.
fn maximal_windows(left: &[Option<&str>], right: &[Option<&str>]) -> Vec<(usize, usize)> {
    let mut windows = Vec::new();
    let mut mapping = Bijection::default();
    let mut last_left: FxHashMap<&str, usize> = FxHashMap::default();
    let mut last_right: FxHashMap<&str, usize> = FxHashMap::default();
    let mut start = 0;
    for position in 0..left.len() {
        let (Some(left_name), Some(right_name)) = (left[position], right[position]) else {
            continue;
        };
        if mapping.accept(left_name, right_name) {
            last_left.insert(left_name, position);
            last_right.insert(right_name, position);
            continue;
        }
        windows.push((start, position));
        let left_conflict = mapping
            .forward
            .get(left_name)
            .is_some_and(|&mapped| mapped != right_name);
        let right_conflict = mapping
            .backward
            .get(right_name)
            .is_some_and(|&mapped| mapped != left_name);
        if left_conflict {
            start = start.max(last_left[left_name] + 1);
        }
        if right_conflict {
            start = start.max(last_right[right_name] + 1);
        }
        mapping.clear();
        last_left.clear();
        last_right.clear();
        for earlier in start..=position {
            if let (Some(left_name), Some(right_name)) = (left[earlier], right[earlier]) {
                mapping.accept(left_name, right_name);
                last_left.insert(left_name, earlier);
                last_right.insert(right_name, earlier);
            }
        }
    }
    windows.push((start, left.len()));
    windows
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

    fn repeated(
        head: &'static str,
        tail: &'static str,
        body: &[&'static str],
    ) -> Vec<Option<&'static str>> {
        let mut tokens = vec![Some(head)];
        tokens.extend((0..30).map(|index| Some(body[index % body.len()])));
        tokens.push(Some(tail));
        tokens.extend((0..30).map(|index| Some(body[(index + 1) % body.len()])));
        tokens
    }

    fn group(instances: &[usize], start: usize, length: usize) -> PlannedGroup {
        PlannedGroup {
            instances: instances.to_vec(),
            start,
            length,
        }
    }

    #[test]
    fn window_after_a_conflict_starts_after_the_conflicting_name() {
        // `a` maps to `x` at 0 and to `z` at 31. `[1, 62)` maps one-to-one.
        let all = [repeated("a", "a", &["b"]), repeated("x", "z", &["y"])];
        let planned = plan_groups(&all, 62, 50);
        assert_eq!(planned, vec![group(&[0, 1], 1, 61)]);
    }

    #[test]
    fn pair_keeps_its_window_when_a_third_instance_breaks_often() {
        let all = [
            repeated("a", "a", &["b"]),
            repeated("x", "z", &["y"]),
            repeated("p", "p", &["q", "r", "q", "s"]),
        ];
        let planned = plan_groups(&all, 62, 50);
        assert!(planned.contains(&group(&[0, 1], 1, 61)), "{planned:?}");
        assert!(
            planned
                .iter()
                .all(|planned| !planned.instances.contains(&2)),
            "{planned:?}"
        );
    }

    #[test]
    fn three_instances_share_one_window() {
        let all = [
            repeated("a", "a", &["b"]),
            repeated("x", "z", &["y"]),
            repeated("p", "r", &["q"]),
        ];
        // The last two instances map one-to-one over the full length.
        assert_eq!(
            plan_groups(&all, 62, 50),
            vec![group(&[1, 2], 0, 62), group(&[0, 1, 2], 1, 61)]
        );
    }

    #[test]
    fn consistent_pair_of_three_instances_keeps_the_full_length() {
        let all = [
            repeated("a", "c", &["b"]),
            repeated("x", "w", &["y"]),
            repeated("p", "p", &["q"]),
        ];
        let planned = plan_groups(&all, 62, 50);
        assert!(planned.contains(&group(&[0, 1], 0, 62)), "{planned:?}");
    }

    #[test]
    fn short_windows_of_three_instances_are_dropped() {
        let all = [
            repeated("a", "a", &["b", "c"]),
            repeated("x", "x", &["y", "y", "z"]),
            repeated("p", "p", &["q", "r", "q", "s"]),
        ];
        assert_eq!(plan_groups(&all, 62, 10), vec![]);
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
    fn inconsistent_rename_gives_maximal_windows() {
        let reference = names(&["a", "=", "b", "+", "c"]);
        let collapsed = names(&["x", "=", "x", "+", "x"]);
        assert_eq!(
            full_length_classes(&[reference.clone(), collapsed.clone()]).len(),
            2
        );
        assert_eq!(
            maximal_windows(&reference, &collapsed),
            vec![(0, 2), (1, 4), (3, 5)]
        );
    }

    #[test]
    fn window_covers_the_full_length_without_conflict() {
        let reference = names(&["a", "+", "a"]);
        let renamed = names(&["b", "+", "b"]);
        assert_eq!(maximal_windows(&reference, &renamed), vec![(0, 3)]);
    }
}
