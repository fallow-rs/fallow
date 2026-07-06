mod basic_tokenization;
mod css_canonicalization;
mod edge_cases;
mod es_modules;
mod expressions;
mod ignore_imports;
mod jsx;
mod operators;
mod proptests;
mod statements;
mod token_ordering;
mod token_types;
mod type_stripping;
mod typescript;

use super::*;
use crate::duplicates::token_types::point_span;
use std::path::PathBuf;

fn tokenize(code: &str) -> Vec<SourceToken> {
    let path = PathBuf::from("test.ts");
    tokenize_file(&path, code, false).tokens
}

fn tokenize_tsx(code: &str) -> Vec<SourceToken> {
    let path = PathBuf::from("test.tsx");
    tokenize_file(&path, code, false).tokens
}

fn tokenize_cross_language(code: &str) -> Vec<SourceToken> {
    let path = PathBuf::from("test.ts");
    tokenize_file_cross_language(&path, code, true, false).tokens
}
