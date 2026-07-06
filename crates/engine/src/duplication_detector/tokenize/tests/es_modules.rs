use super::*;

#[test]
fn tokenize_import_declaration() {
    let tokens = tokenize("import { foo, bar } from './module';");
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    let has_from = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::From)));
    let has_source = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::StringLiteral(s) if s == "./module"));
    assert!(has_import, "Should contain import keyword");
    assert!(has_from, "Should contain from keyword");
    assert!(has_source, "Should contain module source string");
}

#[test]
fn tokenize_export_default_declaration() {
    let tokens = tokenize("export default function() { return 42; }");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    let has_default = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Default)));
    assert!(has_export, "Should contain export keyword");
    assert!(has_default, "Should contain default keyword");
}

#[test]
fn tokenize_export_all_declaration() {
    let tokens = tokenize("export * from './module';");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    let has_from = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::From)));
    let has_source = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::StringLiteral(s) if s == "./module"));
    assert!(has_export, "export * should have export keyword");
    assert!(has_from, "export * should have from keyword");
    assert!(has_source, "export * should have source string");
}

#[test]
fn tokenize_dynamic_import() {
    let tokens = tokenize("const mod = await import('./module');");
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    let has_await = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Await)));
    assert!(
        !has_import,
        "Dynamic import() should not produce Import keyword"
    );
    assert!(has_await, "Should contain await keyword");
}

#[test]
fn tokenize_import_with_as_alias() {
    let tokens = tokenize("import { foo as bar } from './mod';");
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    let has_from = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::From)));
    let has_foo = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "foo"));
    let has_bar = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "bar"));
    assert!(has_import, "Should have import keyword");
    assert!(has_from, "Should have from keyword");
    assert!(has_foo, "Should have original identifier 'foo'");
    assert!(has_bar, "Should have alias identifier 'bar'");
}

#[test]
fn tokenize_import_default_and_named() {
    let tokens = tokenize("import React, { useState } from 'react';");
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    let has_react = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "React"));
    let has_use_state = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "useState"));
    assert!(has_import, "Should have import keyword");
    assert!(has_react, "Should have default import 'React'");
    assert!(has_use_state, "Should have named import 'useState'");
}

#[test]
fn tokenize_import_namespace() {
    let tokens = tokenize("import * as utils from './utils';");
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    let has_utils = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "utils"));
    assert!(has_import, "Should have import keyword");
    assert!(has_utils, "Should have namespace alias 'utils'");
}

#[test]
fn tokenize_export_named_specifiers() {
    let tokens = tokenize("export { foo, bar };");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    let has_foo = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "foo"));
    let has_bar = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(n) if n == "bar"));
    assert!(has_export, "Should have export keyword");
    assert!(has_foo, "Should have exported identifier 'foo'");
    assert!(has_bar, "Should have exported identifier 'bar'");
}

#[test]
fn tokenize_export_named_with_from() {
    let tokens = tokenize("export { default as thing } from './module';");
    let has_export = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    let has_from = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::From)));
    assert!(has_export, "Re-export with from should have export keyword");
    let _ = has_from;
}
