//! Work bounds for the statement probes of the MDX scan.

use std::fmt::Write as _;

use super::{MAX_BOUNDARY_PROBES, MAX_STATEMENT_PROBES, PROBE_BYTES, accepted_mdx_source};

fn probe_bytes(source: &str) -> (String, usize) {
    PROBE_BYTES.with(|bytes| bytes.set(0));
    let body = accepted_mdx_source(source).1.body;
    (body, PROBE_BYTES.with(std::cell::Cell::get))
}

#[test]
fn unfinished_statement_probes_stay_linear_in_the_page_size() {
    let mut source = String::from("export const a = <div>\n");
    for _ in 0..2_000 {
        source.push_str("line\n");
    }

    let (body, bytes) = probe_bytes(&source);

    assert!(!body.contains("export const a"));
    // A probe per line parses the whole block again, so the bytes grow with
    // the square of the line count.
    assert!(
        bytes <= (MAX_STATEMENT_PROBES + 2) * source.len(),
        "probe bytes: {bytes}"
    );
}

#[test]
fn a_long_statement_below_the_probe_limit_ends_at_its_last_line() {
    let mut source = String::from("export const Hero = () => (\n  <div>\n");
    for line in 0..100 {
        writeln!(source, "    <p>row {line}</p>").unwrap();
    }
    source.push_str("  </div>\n)\nProse right after the statement.\n");

    let (body, _) = probe_bytes(&source);

    assert!(body.contains("export const Hero"));
    assert!(body.contains("<p>row 99</p>"));
    assert!(!body.contains("Prose right after"));
}

/// `export const Table` with `rows` JSX rows, then a blank line and prose.
fn long_table_export(rows: usize, row_separator: &str) -> String {
    let mut source = String::from("export const Table = () => (\n  <table>\n");
    for row in 0..rows {
        write!(source, "    <tr><td>{row}</td></tr>\n{row_separator}").unwrap();
    }
    source.push_str("  </table>\n)\n\n# Heading\n\nProse after the statement.\n");
    source
}

#[test]
fn a_statement_past_the_probe_limit_ends_at_the_next_blank_line() {
    let source = long_table_export(MAX_STATEMENT_PROBES + 44, "");

    let (body, _) = probe_bytes(&source);

    assert!(body.contains("export const Table"));
    assert!(body.contains("</table>"));
    assert!(!body.contains("# Heading"));
}

#[test]
fn blank_line_probes_stay_linear_in_the_page_size() {
    let mut source = String::from("export const a = <div>\n");
    for _ in 0..2_000 {
        source.push_str("line\n\n");
    }

    let (body, bytes) = probe_bytes(&source);

    assert!(!body.contains("export const a"));
    assert!(
        bytes <= (MAX_STATEMENT_PROBES + MAX_BOUNDARY_PROBES + 2) * source.len(),
        "probe bytes: {bytes}"
    );
}
