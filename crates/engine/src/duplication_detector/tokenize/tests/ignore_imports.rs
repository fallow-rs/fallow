use super::*;

fn tokenize_skip_imports(code: &str) -> Vec<SourceToken> {
    let path = PathBuf::from("test.ts");
    tokenize_file(&path, code, true).tokens
}

fn tokenize_no_skip(code: &str) -> Vec<SourceToken> {
    let path = PathBuf::from("test.ts");
    tokenize_file(&path, code, false).tokens
}

fn has_keyword(tokens: &[SourceToken], keyword: KeywordType) -> bool {
    tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(found) if found == keyword))
}

#[test]
fn skip_imports_removes_value_import() {
    let tokens = tokenize_skip_imports("import { useState } from 'react';");
    assert!(
        !has_keyword(&tokens, KeywordType::Import),
        "Value import should be stripped when skip_imports is true"
    );
    assert!(
        tokens.is_empty(),
        "File with only an import should produce no tokens"
    );
}

#[test]
fn skip_imports_removes_default_import() {
    let tokens = tokenize_skip_imports("import React from 'react';");
    assert!(tokens.is_empty(), "Default import should be fully stripped");
}

#[test]
fn skip_imports_removes_namespace_import() {
    let tokens = tokenize_skip_imports("import * as React from 'react';");
    assert!(
        tokens.is_empty(),
        "Namespace import should be fully stripped"
    );
}

#[test]
fn skip_imports_removes_side_effect_import() {
    let tokens = tokenize_skip_imports("import './polyfill';");
    assert!(
        tokens.is_empty(),
        "Side-effect import should be fully stripped"
    );
}

#[test]
fn skip_imports_removes_type_import() {
    let tokens = tokenize_skip_imports("import type { Foo } from './foo';");
    assert!(
        tokens.is_empty(),
        "Type import should be stripped when skip_imports is true"
    );
}

#[test]
fn skip_imports_preserves_runtime_code() {
    let tokens = tokenize_skip_imports("import { useState } from 'react';\nconst x = useState(0);");
    assert!(
        has_keyword(&tokens, KeywordType::Const),
        "Runtime code after import should be preserved"
    );
    assert!(
        !has_keyword(&tokens, KeywordType::Import),
        "Import keyword should be stripped"
    );
}

#[test]
fn skip_imports_preserves_export_declaration() {
    let tokens = tokenize_skip_imports("export function foo() { return 1; }");
    assert!(
        has_keyword(&tokens, KeywordType::Export),
        "Local export declaration should be preserved"
    );
}

#[test]
fn skip_imports_preserves_export_default() {
    let tokens = tokenize_skip_imports("export default class Foo {}");
    assert!(
        has_keyword(&tokens, KeywordType::Export),
        "Export default should be preserved"
    );
}

#[test]
fn skip_imports_preserves_local_named_export() {
    let tokens = tokenize_skip_imports("const foo = 1;\nexport { foo };");
    assert!(
        has_keyword(&tokens, KeywordType::Export),
        "Local named export should be preserved"
    );
}

#[test]
fn skip_imports_removes_reexport() {
    let tokens = tokenize_skip_imports("export { foo } from './foo';");
    assert!(tokens.is_empty(), "Named re-export should be stripped");
}

#[test]
fn skip_imports_removes_default_alias_reexport() {
    let tokens = tokenize_skip_imports("export { default as Button } from './button';");
    assert!(
        tokens.is_empty(),
        "Default alias re-export should be stripped"
    );
}

#[test]
fn skip_imports_removes_namespace_reexport() {
    let tokens = tokenize_skip_imports("export * as ns from './mod';");
    assert!(tokens.is_empty(), "Namespace re-export should be stripped");
}

#[test]
fn skip_imports_removes_export_all() {
    let tokens = tokenize_skip_imports("export * from './mod';");
    assert!(tokens.is_empty(), "Export * should be stripped");
}

#[test]
fn skip_imports_removes_type_reexport() {
    let tokens = tokenize_skip_imports("export type { Foo } from './foo';");
    assert!(tokens.is_empty(), "Type re-export should be stripped");
}

#[test]
fn skip_imports_removes_top_level_require_binding() {
    let tokens = tokenize_skip_imports("const x = require('foo');");
    assert!(
        tokens.is_empty(),
        "Top-level require binding should be stripped"
    );
}

#[test]
fn skip_imports_removes_top_level_destructured_require_binding() {
    let tokens = tokenize_skip_imports("const { readFile, writeFile } = require('node:fs');");
    assert!(
        tokens.is_empty(),
        "Top-level destructured require binding should be stripped"
    );
}

#[test]
fn skip_imports_removes_top_level_multi_require_binding() {
    let tokens = tokenize_skip_imports("const fs = require('fs'), path = require('path');");
    assert!(
        tokens.is_empty(),
        "Top-level declaration with only require bindings should be stripped"
    );
}

#[test]
fn skip_imports_preserves_function_local_require_binding() {
    let tokens = tokenize_skip_imports("function load() { const x = require('foo'); return x; }");
    assert!(
        has_keyword(&tokens, KeywordType::Const),
        "Function-local require binding should stay tokenized"
    );
}

#[test]
fn skip_imports_preserves_require_call_argument() {
    let tokens = tokenize_skip_imports("doSomething(require('foo'));");
    assert!(
        !tokens.is_empty(),
        "Require used as an executable call argument should stay tokenized"
    );
}

#[test]
fn skip_imports_preserves_conditional_require_binding() {
    let tokens = tokenize_skip_imports("const x = condition ? require('a') : require('b');");
    assert!(
        has_keyword(&tokens, KeywordType::Const),
        "Conditional require binding should stay tokenized"
    );
}

#[test]
fn skip_imports_preserves_mixed_require_declaration() {
    let tokens = tokenize_skip_imports("const fs = require('fs'), mode = process.env.NODE_ENV;");
    assert!(
        has_keyword(&tokens, KeywordType::Const),
        "Mixed require and runtime declaration should stay tokenized"
    );
}

#[test]
fn skip_imports_preserves_non_string_require_binding() {
    let tokens = tokenize_skip_imports("const plugin = require(pluginName);");
    assert!(
        has_keyword(&tokens, KeywordType::Const),
        "Dynamic require binding should stay tokenized"
    );
}

#[test]
fn skip_imports_preserves_multi_arg_require_binding() {
    let tokens = tokenize_skip_imports("const plugin = require('plugin', options);");
    assert!(
        has_keyword(&tokens, KeywordType::Const),
        "Require binding with extra arguments should stay tokenized"
    );
}

#[test]
fn skip_imports_preserves_side_effect_require() {
    let tokens = tokenize_skip_imports("require('dotenv/config');");
    assert!(
        !tokens.is_empty(),
        "Side-effect require call should stay tokenized"
    );
}

#[test]
fn skip_imports_reduces_token_count() {
    let code = "import { a } from 'a';\nimport { b } from 'b';\nconst x = a + b;";
    let with_imports = tokenize_no_skip(code);
    let without_imports = tokenize_skip_imports(code);
    assert!(
        without_imports.len() < with_imports.len(),
        "Skipping imports should produce fewer tokens: with={}, without={}",
        with_imports.len(),
        without_imports.len()
    );
}

#[test]
fn skip_imports_disabled_preserves_imports() {
    let code = "import { useState } from 'react';";
    let tokens = tokenize_no_skip(code);
    let has_import = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(
        has_import,
        "With skip_imports=false, imports should be tokenized"
    );
}

#[test]
fn skip_imports_removes_sorted_import_block() {
    let code = r"import { A } from './a';
import { B } from './b';
import { C } from './c';
import { D } from './d';
import { E } from './e';

export function process() {
    return A + B + C + D + E;
}";
    let tokens = tokenize_skip_imports(code);
    let import_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)))
        .count();
    assert_eq!(import_count, 0, "All import declarations should be removed");
    assert!(
        has_keyword(&tokens, KeywordType::Export),
        "Export function should be preserved"
    );
}

#[test]
fn skip_imports_does_not_filter_dynamic_import() {
    let tokens = tokenize_skip_imports("const mod = import('./module');");
    assert!(
        !tokens.is_empty(),
        "Dynamic import() expression should NOT be filtered (it's a CallExpression)"
    );
}

#[test]
fn skip_imports_with_cross_language() {
    let path = PathBuf::from("test.ts");
    let code =
        "import type { Foo } from './foo';\nimport { bar } from './bar';\nconst x: Foo = bar();";
    let tokens = tokenize_file_cross_language(&path, code, true, true).tokens;
    let import_count = tokens
        .iter()
        .filter(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)))
        .count();
    assert_eq!(
        import_count, 0,
        "Both type and value imports should be removed when both flags are active"
    );
    let has_const = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Runtime code should be preserved");
}
