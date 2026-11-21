use super::*;

fn apply(content: &str, edit: &SourceEdit) -> String {
    let mut result = content.to_owned();
    result.replace_range(edit.start..edit.end, &edit.replacement);
    result
}

fn apply_all(content: &str, edits: &[SourceEdit]) -> String {
    let mut sorted = edits.to_vec();
    sorted.sort_by_key(|edit| edit.start);
    let mut result = content.to_owned();
    for edit in sorted.iter().rev() {
        result.replace_range(edit.start..edit.end, &edit.replacement);
    }
    result
}

/// The split, change and join method that the fixers used before the
/// snapshot: split on the line ending, replace or remove the elements
/// `first..=last`, join, and keep a last line ending.
fn split_and_join(
    content: &str,
    line_ending: &str,
    first: usize,
    last: usize,
    replacement: Option<&[String]>,
) -> String {
    let mut lines: Vec<String> = content.split(line_ending).map(str::to_owned).collect();
    let new_lines: Vec<String> = replacement.map(<[String]>::to_vec).unwrap_or_default();
    lines.splice(first..=last, new_lines);
    let mut result = lines.join(line_ending);
    if content.ends_with(line_ending) && !result.ends_with(line_ending) {
        result.push_str(line_ending);
    }
    result
}

fn keep_line_ending(content: &str, line_ending: &str, mut result: String) -> String {
    if content.ends_with(line_ending) && !result.ends_with(line_ending) {
        result.push_str(line_ending);
    }
    result
}

fn snapshots_with(path: &Path, content: &str) -> SourceSnapshots {
    let mut snapshots = SourceSnapshots::default();
    snapshots.insert(
        path,
        content.to_owned(),
        EncodingMetadata {
            line_ending: "\n",
            had_bom: false,
        },
    );
    snapshots
}

fn deletion(start: usize, end: usize) -> SourceEdit {
    SourceEdit {
        start,
        end,
        replacement: String::new(),
    }
}

#[test]
fn line_edit_equals_the_split_and_join_method() {
    let texts = [
        ("a\nb\nc\nd\n", "\n"),
        ("a\nb\nc\nd", "\n"),
        ("a\r\nb\r\nc\r\nd\r\n", "\r\n"),
        ("a\r\nb\r\nc\r\nd", "\r\n"),
    ];
    let one = vec!["X".to_owned()];
    let two = vec!["X".to_owned(), "Y".to_owned()];
    let replacements: [Option<&[String]>; 4] = [None, Some(&[]), Some(&one), Some(&two)];
    for (content, line_ending) in texts {
        let count = content.split(line_ending).count();
        let ranges = [
            (0, 0),
            (1, 2),
            (count - 1, count - 1),
            (count - 2, count - 1),
            (0, count - 1),
            (1, 3),
        ];
        for (first, last) in ranges {
            for replacement in replacements {
                let edit = line_edit(content, line_ending, first, last, replacement);
                let actual = keep_line_ending(content, line_ending, apply(content, &edit));
                let expected = split_and_join(content, line_ending, first, last, replacement);
                assert_eq!(
                    actual, expected,
                    "{content:?} lines {first}..={last} with {replacement:?}"
                );
            }
        }
    }
}

#[test]
fn line_deletions_join_neighbour_lines() {
    let content = "a\nb\nc\nd\n";
    let edits = line_deletions(content, "\n", &[3, 4, 3]);
    assert_eq!(
        edits.len(),
        1,
        "the last two lines give one edit: {edits:?}"
    );
    assert_eq!(apply_all(content, &edits), "a\nb\nc");

    let edits = line_deletions(content, "\n", &[0, 2, 3]);
    assert_eq!(edits.len(), 2);
    assert_eq!(apply_all(content, &edits), "b\n");

    assert!(line_deletions(content, "\n", &[]).is_empty());
}

#[test]
fn queue_accepts_separate_edits() {
    let path = Path::new("/project/a.ts");
    let mut snapshots = snapshots_with(path, "export const a = 1;\nexport const b = 2;\n");
    assert_eq!(
        snapshots.queue(path, vec![deletion(0, 7)]),
        QueueOutcome::Queued
    );
    assert_eq!(
        snapshots.queue(path, vec![deletion(20, 27)]),
        QueueOutcome::Queued
    );
    assert_eq!(
        snapshots.queue(path, vec![deletion(7, 8)]),
        QueueOutcome::Queued,
        "an edit that only touches a queued edit does not overlap it"
    );
    let rewrites = snapshots.into_rewrites();
    assert_eq!(rewrites.len(), 1);
    assert_eq!(rewrites[0].2, b"onst a = 1;\nconst b = 2;\n");
}

#[test]
fn queue_reports_a_covered_edit() {
    let path = Path::new("/project/a.ts");
    let mut snapshots = snapshots_with(path, "export enum E {\n  A,\n}\n");
    assert_eq!(
        snapshots.queue(path, vec![deletion(0, 23)]),
        QueueOutcome::Queued
    );
    assert_eq!(
        snapshots.queue(path, vec![deletion(0, 23)]),
        QueueOutcome::Covered
    );
    assert_eq!(
        snapshots.queue(path, vec![deletion(15, 20), deletion(0, 7)]),
        QueueOutcome::Covered
    );
}

#[test]
fn queue_refuses_an_overlap_with_a_queued_edit() {
    let path = Path::new("/project/a.ts");
    let content = "export const a = 1;\n";
    let mut snapshots = snapshots_with(path, content);
    let replacement = SourceEdit {
        start: 0,
        end: 19,
        replacement: "const a = 2;".to_owned(),
    };
    assert_eq!(
        snapshots.queue(path, vec![replacement]),
        QueueOutcome::Queued
    );
    assert_eq!(
        snapshots.queue(path, vec![deletion(19, 20), deletion(0, 7)]),
        QueueOutcome::Conflict,
        "an edit inside a queued replacement is a conflict"
    );
    assert_eq!(
        snapshots.queue(Path::new("/project/other.ts"), vec![deletion(0, 1)]),
        QueueOutcome::Conflict,
        "a path with no snapshot"
    );
    assert_eq!(
        snapshots.queue(path, vec![deletion(19, 19)]),
        QueueOutcome::Conflict,
        "an empty range is not valid"
    );
    let rewrites = snapshots.into_rewrites();
    assert_eq!(
        rewrites[0].2, b"const a = 2;\n",
        "the queue keeps only the first edit"
    );
}

#[test]
fn queue_refuses_an_overlap_inside_one_group() {
    let path = Path::new("/project/a.ts");
    let content = "export { a, b };\n";
    let mut snapshots = snapshots_with(path, content);
    let group = vec![
        deletion(0, 7),
        SourceEdit {
            start: 0,
            end: 16,
            replacement: "export { b };".to_owned(),
        },
    ];
    assert_eq!(snapshots.queue(path, group), QueueOutcome::Conflict);
    assert!(snapshots.into_rewrites().is_empty());

    let mut snapshots = snapshots_with(path, content);
    assert_eq!(
        snapshots.queue(path, vec![deletion(0, 7), deletion(0, 7)]),
        QueueOutcome::Queued
    );
    let rewrites = snapshots.into_rewrites();
    assert_eq!(
        rewrites[0].2, b"{ a, b };\n",
        "the equal edit applies one time"
    );
}

#[test]
fn into_rewrites_keeps_the_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("bom.ts");
    std::fs::write(
        &file,
        "\u{FEFF}export const a = 1;\r\nexport const b = 2;\r\n",
    )
    .unwrap();
    let mut plan = FixPlan::for_root(dir.path()).unwrap();
    let (content, meta) =
        read_source_snapshot(dir.path(), &file, &CapturedHashes::default(), &mut plan).unwrap();
    assert!(meta.had_bom);
    assert_eq!(meta.line_ending, "\r\n");
    let edits = line_deletions(&content, meta.line_ending, &[1]);
    assert_eq!(plan.sources_mut().queue(&file, edits), QueueOutcome::Queued);
    let outcome = plan.commit();
    assert!(outcome.failed.is_empty());
    assert_eq!(
        std::fs::read(&file).unwrap(),
        "\u{FEFF}export const a = 1;\r\n".as_bytes()
    );

    let all = dir.path().join("all.ts");
    std::fs::write(&all, "export enum E {\n  A,\n}\n").unwrap();
    let mut plan = FixPlan::for_root(dir.path()).unwrap();
    let (content, meta) =
        read_source_snapshot(dir.path(), &all, &CapturedHashes::default(), &mut plan).unwrap();
    let edits = line_deletions(&content, meta.line_ending, &[0, 1, 2]);
    assert_eq!(plan.sources_mut().queue(&all, edits), QueueOutcome::Queued);
    assert!(plan.commit().failed.is_empty());
    assert_eq!(std::fs::read_to_string(&all).unwrap(), "\n");
}

#[test]
fn read_source_snapshot_returns_the_analysed_text() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sample.ts");
    let source = "export const x = 1;\n";
    std::fs::write(&file, source).unwrap();
    let mut hashes = CapturedHashes::default();
    hashes.insert(file.clone(), xxhash_rust::xxh3::xxh3_64(source.as_bytes()));

    let mut plan = FixPlan::for_root(dir.path()).unwrap();
    let (content, _) = read_source_snapshot(dir.path(), &file, &hashes, &mut plan).unwrap();
    assert_eq!(
        plan.sources_mut().queue(&file, vec![deletion(0, 7)]),
        QueueOutcome::Queued
    );
    let (again, _) = read_source_snapshot(dir.path(), &file, &hashes, &mut plan).unwrap();
    assert_eq!(again, content, "a later read returns the analysed text");
    assert!(plan.skipped().is_empty());

    let mut stale = CapturedHashes::default();
    stale.insert(file.clone(), 0xDEAD_BEEF);
    let mut plan = FixPlan::for_root(dir.path()).unwrap();
    assert!(read_source_snapshot(dir.path(), &file, &stale, &mut plan).is_none());
    assert_eq!(plan.skipped().len(), 1);
    assert_eq!(plan.skipped()[0].reason, SkipReason::ContentChanged);
}

#[test]
fn queue_joins_deletions_that_share_a_line_ending() {
    let path = Path::new("/project/a.ts");
    let content = "enum A {\n  a,\n}\nenum B {\n  b,\n}";
    let mut snapshots = snapshots_with(path, content);
    let first = line_deletions(content, "\n", &[0, 1, 2]);
    let second = line_deletions(content, "\n", &[3, 4, 5]);
    assert_eq!(snapshots.queue(path, first), QueueOutcome::Queued);
    assert_eq!(snapshots.queue(path, second), QueueOutcome::Queued);
    let rewrites = snapshots.into_rewrites();
    assert_eq!(rewrites[0].2, b"");

    let mut snapshots = snapshots_with(path, content);
    assert_eq!(
        snapshots.queue(path, vec![deletion(0, 12), deletion(10, 20)]),
        QueueOutcome::Conflict,
        "deletions that share more than a line ending still conflict"
    );
}
