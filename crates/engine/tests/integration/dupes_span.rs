//! A clone instance must cover only the lines of its matched tokens.
//!
//! A token such as `const`, `if` or `{` belongs to an AST node that can span
//! many lines. When the token carries the span of that node, a clone that ends
//! (or starts) on the token reaches to the end (or start) of the node. The
//! reported line range and the duplicated line count are then much larger than
//! the matched code.

#![allow(
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use fallow_engine::discover::{DiscoveredFile, FileId};

const SHARED_HEADER: &str = "interface Args {\n\
  alpha: string\n\
  beta: number\n\
  gamma: boolean\n\
  delta: string\n\
  epsilon: number\n\
}\n\
\n\
export const ";

/// Number of lines that the shared header covers, up to and including the
/// `export const` line.
const SHARED_HEADER_LINES: usize = 9;

const BODY_LINES: usize = 40;

fn source_with_body(name: &str, statement: &str) -> String {
    let mut source =
        format!("{SHARED_HEADER}{name} = async ({{ alpha, beta }}: Args): Promise<void> => {{\n");
    for index in 0..BODY_LINES {
        source.push_str(&statement.replace('N', &index.to_string()));
        source.push('\n');
    }
    source.push_str("}\n");
    source
}

fn discovered(id: usize, path: std::path::PathBuf, source: &str) -> DiscoveredFile {
    DiscoveredFile {
        id: FileId(u32::try_from(id).expect("small test id")),
        path,
        size_bytes: source.len() as u64,
    }
}

#[test]
fn clone_ending_on_a_declaration_keyword_does_not_cover_the_declaration() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("create src");

    let first = source_with_body("first", "  if (alpha === 'kN') { return }");
    let second = source_with_body("second", "  await beta.call(N)");
    std::fs::write(src.join("first.ts"), &first).expect("write first");
    std::fs::write(src.join("second.ts"), &second).expect("write second");

    let files = vec![
        discovered(0, src.join("first.ts"), &first),
        discovered(1, src.join("second.ts"), &second),
    ];
    let config = fallow_config::DuplicatesConfig {
        min_tokens: 10,
        min_lines: 3,
        ..Default::default()
    };

    let report = fallow_engine::duplicates::find_duplicates(dir.path(), &files, &config);

    assert_eq!(
        report.clone_groups.len(),
        1,
        "the shared header is one clone group: {:#?}",
        report.clone_groups
    );
    for instance in &report.clone_groups[0].instances {
        assert!(
            instance.end_line <= SHARED_HEADER_LINES,
            "clone in {} ends on line {}, but the matched code ends on line {}",
            instance.file.display(),
            instance.end_line,
            SHARED_HEADER_LINES
        );
        assert!(
            instance.fragment.ends_with("export const"),
            "the fragment ends with the last matched keyword: {:?}",
            instance.fragment
        );
    }
    assert!(
        report.stats.duplicated_lines <= 2 * SHARED_HEADER_LINES,
        "duplicated_lines is {}, but only the header lines match",
        report.stats.duplicated_lines
    );
}
