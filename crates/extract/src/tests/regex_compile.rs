//! Compiles every static regex pattern in the crate.
//!
//! Each static pattern goes through `crate::static_regex` inside a `LazyLock`.
//! The pattern compiles only when a code path uses it, so a parse test finds an
//! invalid pattern only when its input reaches that branch. This test reads the
//! crate source, takes the string literal of each `static_regex` call, and
//! compiles it with the same `regex` crate features as the production code.

use std::path::{Path, PathBuf};

/// The call text to look for. It is split so that this file does not match it.
const CALL: &str = concat!("static_", "regex(");

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read source directory");
    for entry in entries {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Decode a raw string literal (`r"..."`, `r#"..."#`) at the start of `text`.
fn raw_literal(text: &str) -> Option<String> {
    let rest = text.strip_prefix('r')?;
    let hashes = rest.len() - rest.trim_start_matches('#').len();
    let body = rest[hashes..].strip_prefix('"')?;
    let terminator = format!("\"{}", "#".repeat(hashes));
    let end = body.find(&terminator)?;
    Some(body[..end].to_string())
}

/// Decode a normal string literal (`"..."`) at the start of `text`.
fn escaped_literal(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '0' => out.push('\0'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '\n' => {
                    let rest = chars.as_str().trim_start();
                    chars = rest.chars();
                }
                other => panic!("unsupported escape `\\{other}` in a static regex literal"),
            },
            _ => out.push(c),
        }
    }
    None
}

#[test]
fn every_static_regex_pattern_compiles() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);

    let mut compiled = 0;
    for file in files {
        let source = std::fs::read_to_string(&file).expect("read source file");
        for (idx, _) in source.match_indices(CALL) {
            if source[..idx].ends_with("fn ") {
                continue;
            }
            let line = source[..idx].matches('\n').count() + 1;
            let argument = source[idx + CALL.len()..].trim_start();
            let pattern = raw_literal(argument)
                .or_else(|| escaped_literal(argument))
                .unwrap_or_else(|| {
                    panic!(
                        "{}:{line}: give `static_regex` a string literal so this test can compile it",
                        file.display()
                    )
                });
            if let Err(error) = regex::Regex::new(&pattern) {
                panic!("{}:{line}: invalid static regex: {error}", file.display());
            }
            compiled += 1;
        }
    }
    assert!(
        compiled > 0,
        "found no static regex patterns under {}",
        src.display()
    );
}
