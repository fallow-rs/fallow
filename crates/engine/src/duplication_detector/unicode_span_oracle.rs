//! Unicode oracle for clone locations.
//!
//! The oracle puts comments with astral characters, combining marks and
//! Unicode whitespace before the first and the last token of a clone. The
//! `start_col` and `end_col` of each clone instance must be byte columns, so
//! that the LSP and the other consumers can slice the line at them (issue
//! #4378).
//!
//! The case count and the seed come from `PROPTEST_CASES` and
//! `PROPTEST_RNG_SEED`. Without `PROPTEST_RNG_SEED` the seed is fixed, so
//! each pull request run checks the same cases.

use proptest::prelude::*;
use proptest::test_runner::RngSeed;

use super::{DuplicatesConfig, find_duplicates};
use fallow_types::discover::{DiscoveredFile, FileId};

const DEFAULT_SEED: u64 = 0x0001_f600;

/// Comment text: ASCII, two-byte, three-byte and astral characters, a
/// combining mark, a zero-width joiner sequence and Unicode whitespace.
const TEXT: &[&str] = &[
    "a",
    " ",
    "\u{e9}",
    "e\u{301}",
    "\u{65e5}",
    "\u{1f600}",
    "\u{1f469}\u{200d}\u{1f4bb}",
    "\u{1d49c}",
    "\u{a0}",
    "\u{3000}",
    "\u{feff}",
    "\u{2028}",
];

const START: &str = "export function shared(value: number): number {";
const END: &str = "return tripled * 4";

fn config() -> ProptestConfig {
    let mut config = ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    if matches!(config.rng_seed, RngSeed::Random) {
        config.rng_seed = RngSeed::Fixed(DEFAULT_SEED);
    }
    config
}

fn text() -> BoxedStrategy<String> {
    prop::collection::vec(prop::sample::select(TEXT), 0..6)
        .prop_map(|parts| parts.concat())
        .boxed()
}

fn source(first: &str, last: &str) -> String {
    format!(
        "/* {first} */ {START}\n  const doubled = value * 2;\n  const shifted = doubled + 3;\n  const tripled = shifted * 3;\n  /* {last} */ {END};\n}}\n"
    )
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn clone_columns_are_byte_columns(first in text(), last in text()) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let mut files = Vec::new();
        for (index, (name, content)) in [("a.ts", source(&first, &last)), ("b.ts", source("", ""))]
            .into_iter()
            .enumerate()
        {
            let path = dir.path().join(name);
            std::fs::write(&path, &content).expect("write clone");
            files.push((
                DiscoveredFile {
                    id: FileId(index as u32),
                    path,
                    size_bytes: content.len() as u64,
                },
                content,
            ));
        }
        let discovered: Vec<DiscoveredFile> = files.iter().map(|(file, _)| file.clone()).collect();
        let config = DuplicatesConfig {
            min_tokens: 5,
            min_lines: 2,
            ..DuplicatesConfig::default()
        };

        let report = find_duplicates(dir.path(), &discovered, &config);

        let group = report.clone_groups.first().expect("clone group");
        prop_assert_eq!(group.instances.len(), 2);
        for instance in &group.instances {
            let (_, content) = files
                .iter()
                .find(|(file, _)| file.path == instance.file)
                .expect("instance file");
            let lines: Vec<&str> = content.split('\n').collect();
            let start_line = lines[instance.start_line - 1];
            let end_line = lines[instance.end_line - 1];
            prop_assert!(
                start_line.get(instance.start_col..).is_some_and(|rest| rest.starts_with(START)),
                "start_col {} is not the byte column of the clone start in {:?}",
                instance.start_col,
                start_line,
            );
            prop_assert!(
                end_line.get(..instance.end_col).is_some_and(|head| head.ends_with(END)),
                "end_col {} is not the byte column of the clone end in {:?}",
                instance.end_col,
                end_line,
            );
        }
    }
}
