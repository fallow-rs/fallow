#![no_main]
//! Property target for config glob validation.
//!
//! The threat model promises that a config glob with an absolute path or a
//! `..` segment is an error. This target builds patterns from tokens that are
//! near that boundary and checks two things for each pattern the validator
//! accepts:
//!
//! 1. The pattern cannot name a path outside the root under POSIX or Windows
//!    path rules.
//! 2. Expanding the pattern on disk with `expand_disk_glob`, the wrapper
//!    that every on-disk glob expansion uses, returns only paths inside the
//!    root.

use std::path::PathBuf;
use std::sync::OnceLock;

use arbitrary::Arbitrary;
use fallow_config::glob_validation::{
    compile_user_glob, expand_disk_glob, is_exact_workspace_root,
    validate_user_finding_ignore_globs, validate_user_ignore_pattern_globs, validate_user_path,
};
use libfuzzer_sys::fuzz_target;

#[path = "common/glob_invariant.rs"]
mod glob_invariant;

#[derive(Arbitrary, Debug)]
enum Token {
    Slash,
    Backslash,
    Dot,
    DotDot,
    Star,
    DoubleStar,
    Question,
    Bang,
    Tilde,
    Colon,
    DriveLetter(u8),
    EncodedDotDot,
    BraceDotDot,
    ClassDotDot,
    OpenBrace,
    CloseBrace,
    Comma,
    OpenBracket,
    CloseBracket,
    Name(u8),
    Raw(char),
}

impl Token {
    fn push_to(&self, out: &mut String) {
        match self {
            Self::Slash => out.push('/'),
            Self::Backslash => out.push('\\'),
            Self::Dot => out.push('.'),
            Self::DotDot => out.push_str(".."),
            Self::Star => out.push('*'),
            Self::DoubleStar => out.push_str("**"),
            Self::Question => out.push('?'),
            Self::Bang => out.push('!'),
            Self::Tilde => out.push('~'),
            Self::Colon => out.push(':'),
            Self::DriveLetter(b) => {
                out.push(char::from(b'a' + b % 26));
                out.push(':');
            }
            Self::EncodedDotDot => out.push_str("%2e%2e"),
            Self::BraceDotDot => out.push_str("{..,x}"),
            Self::ClassDotDot => out.push_str("[.][.]"),
            Self::OpenBrace => out.push('{'),
            Self::CloseBrace => out.push('}'),
            Self::Comma => out.push(','),
            Self::OpenBracket => out.push('['),
            Self::CloseBracket => out.push(']'),
            Self::Name(b) => out.push_str(NAMES[usize::from(*b) % NAMES.len()]),
            Self::Raw(c) => out.push(*c),
        }
    }
}

/// Names that exist in the fixture tree, so expansion on disk finds matches.
const NAMES: &[&str] = &[
    "root", "inside", "secret", "pkg", "a", "src", "index.ts", ".hidden",
];

const FIELD: &str = "fuzz";

/// A fixed tree: `<base>/secret/secret` lies outside `<base>/root`, which is
/// the project root. `root/.hidden` gives dot-wildcard segments a legitimate
/// match. Each process gets its own base, so `-fork` workers do not share it.
struct Fixture {
    root: PathBuf,
    canonical_root: PathBuf,
}

fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let base = std::env::temp_dir().join(format!("fallow-fuzz-glob-{}", std::process::id()));
        let root = base.join("root");
        for dir in [
            root.join("inside").join("src"),
            root.join("pkg").join("a"),
            root.join(".hidden"),
            base.join("secret"),
        ] {
            std::fs::create_dir_all(&dir).expect("create fixture directory");
        }
        for file in [
            root.join("inside").join("src").join("index.ts"),
            root.join("pkg").join("a").join("index.ts"),
            root.join(".hidden").join("secret"),
            base.join("secret").join("secret"),
            base.join("secret").join("index.ts"),
        ] {
            std::fs::write(&file, "").expect("write fixture file");
        }
        let canonical_root = dunce::canonicalize(&root).expect("canonicalize fixture root");
        Fixture {
            root,
            canonical_root,
        }
    })
}

/// Expansion on disk is bounded so one input cannot walk a large tree.
const MAX_EXPANDED_PATHS: usize = 64;

fn assert_expansion_stays_inside(pattern: &str) {
    let fixture = fixture();
    let Some(paths) = expand_disk_glob(&fixture.root, pattern) else {
        return;
    };
    for path in paths.filter_map(Result::ok).take(MAX_EXPANDED_PATHS) {
        let Ok(canonical) = dunce::canonicalize(&path) else {
            continue;
        };
        assert!(
            canonical.starts_with(&fixture.canonical_root),
            "accepted glob {pattern:?} expanded to {} outside the root",
            canonical.display()
        );
    }
}

fn assert_accepted_is_contained(pattern: &str, via: &str) {
    if let Some(reason) = glob_invariant::escape_reason(pattern) {
        panic!("{via} accepted {pattern:?} ({reason})");
    }
    assert_expansion_stays_inside(pattern);
}

fn check(pattern: &str) {
    let path_ok = validate_user_path(pattern, FIELD).is_ok();
    let glob_result = compile_user_glob(pattern, FIELD);

    // Both entry points share the absolute and traversal checks. Only glob
    // syntax can make `compile_user_glob` reject a pattern that the path check
    // accepts.
    if glob_result.is_ok() {
        assert!(
            path_ok,
            "compile_user_glob accepted {pattern:?}, validate_user_path did not"
        );
    }
    if path_ok {
        assert_accepted_is_contained(pattern, "validate_user_path");
    }
    if is_exact_workspace_root(pattern) {
        assert!(
            path_ok,
            "is_exact_workspace_root accepted {pattern:?}, validate_user_path did not"
        );
    }

    // Ignore fields allow a leading `!` for negation. The rest of the pattern
    // must still satisfy the invariant.
    let list = [pattern.to_owned()];
    let body = pattern.strip_prefix('!').unwrap_or(pattern);
    let mut errors = Vec::new();
    validate_user_ignore_pattern_globs(&list, FIELD, &mut errors);
    if errors.is_empty() {
        assert_accepted_is_contained(body, "validate_user_ignore_pattern_globs");
    }
    let mut errors = Vec::new();
    validate_user_finding_ignore_globs(&list, FIELD, &mut errors);
    if errors.is_empty() {
        assert_accepted_is_contained(body, "validate_user_finding_ignore_globs");
    }
}

/// Bounds the pattern length; real config globs are short.
const MAX_TOKENS: usize = 24;

fuzz_target!(|tokens: Vec<Token>| {
    let mut pattern = String::new();
    for token in tokens.iter().take(MAX_TOKENS) {
        token.push_to(&mut pattern);
    }
    check(&pattern);
});
