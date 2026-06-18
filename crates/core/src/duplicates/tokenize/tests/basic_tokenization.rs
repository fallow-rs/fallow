use super::*;

#[test]
fn tokenize_variable_declaration() {
    let tokens = tokenize("const x = 42;");
    assert!(!tokens.is_empty());
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::Const)
    ));
}

#[test]
fn tokenize_function_declaration() {
    let tokens = tokenize("function foo() { return 1; }");
    assert!(!tokens.is_empty());
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::Function)
    ));
}

#[test]
fn tokenize_arrow_function() {
    let tokens = tokenize("const f = (a, b) => a + b;");
    assert!(!tokens.is_empty());
    let has_arrow = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Operator(OperatorType::Arrow)));
    assert!(has_arrow, "Should contain arrow operator");
}

#[test]
fn tokenize_if_else() {
    let tokens = tokenize("if (x) { y; } else { z; }");
    assert!(!tokens.is_empty());
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::If)
    ));
    let has_else = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Else)));
    assert!(has_else, "Should contain else keyword");
}

#[test]
fn tokenize_class() {
    let tokens = tokenize("class Foo extends Bar { }");
    assert!(!tokens.is_empty());
    assert!(matches!(
        tokens[0].kind,
        TokenKind::Keyword(KeywordType::Class)
    ));
    let has_extends = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Extends)));
    assert!(has_extends, "Should contain extends keyword");
}

#[test]
fn tokenize_string_literal() {
    let tokens = tokenize("const s = \"hello\";");
    let has_string = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::StringLiteral(s) if s == "hello"));
    assert!(has_string, "Should contain string literal");
}

#[test]
fn tokenize_boolean_literal() {
    let tokens = tokenize("const b = true;");
    let has_bool = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::BooleanLiteral(true)));
    assert!(has_bool, "Should contain boolean literal");
}

#[test]
fn tokenize_null_literal() {
    let tokens = tokenize("const n = null;");
    let has_null = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::NullLiteral));
    assert!(has_null, "Should contain null literal");
}

#[test]
fn tokenize_empty_file() {
    let tokens = tokenize("");
    assert!(tokens.is_empty());
}

#[test]
fn tokenize_ts_interface() {
    let tokens = tokenize("interface Foo { bar: string; baz: number; }");
    let has_interface = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Interface)));
    assert!(has_interface, "Should contain interface keyword");
    let has_bar = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(name) if name == "bar"));
    assert!(has_bar, "Should contain property name 'bar'");
    let has_string = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(name) if name == "string"));
    assert!(has_string, "Should contain type 'string'");
    assert!(
        tokens.len() >= 10,
        "Interface should produce sufficient tokens, got {}",
        tokens.len()
    );
}

#[test]
fn tokenize_ts_type_alias() {
    let tokens = tokenize("type Result = { ok: boolean; error: string; }");
    let has_type = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Type)));
    assert!(has_type, "Should contain type keyword");
}

#[test]
fn tokenize_ts_enum() {
    let tokens = tokenize("enum Color { Red, Green, Blue }");
    let has_enum = tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Enum)));
    assert!(has_enum, "Should contain enum keyword");
    let has_red = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(name) if name == "Red"));
    assert!(has_red, "Should contain enum member 'Red'");
}

#[test]
fn tokenize_jsx_element() {
    let tokens =
        tokenize_tsx("const x = <div className=\"foo\"><Button onClick={handler} /></div>;");
    let has_div = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(name) if name == "div"));
    assert!(has_div, "Should contain JSX element name 'div'");
    let has_classname = tokens
        .iter()
        .any(|t| matches!(&t.kind, TokenKind::Identifier(name) if name == "className"));
    assert!(has_classname, "Should contain JSX attribute 'className'");
    let brackets = tokens
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                TokenKind::Punctuation(
                    PunctuationType::OpenBracket | PunctuationType::CloseBracket,
                )
            )
        })
        .count();
    assert!(
        brackets >= 4,
        "Should contain JSX angle brackets, got {brackets}"
    );
}

#[test]
fn tokenize_vue_sfc_extracts_script_block() {
    let vue_source = r#"<template><div>Hello</div></template>
<script lang="ts">
import { ref } from 'vue';
const count = ref(0);
</script>"#;
    let path = PathBuf::from("Component.vue");
    let result = tokenize_file(&path, vue_source, false);
    assert!(!result.tokens.is_empty(), "Vue SFC should produce tokens");
    let has_import = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(has_import, "Should tokenize import in <script> block");
    let has_const = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Const)));
    assert!(has_const, "Should tokenize const in <script> block");
    assert!(
        has_identifier(&result.tokens, "template") && has_identifier(&result.tokens, "div"),
        "Should tokenize template markup"
    );
}

#[test]
fn tokenize_svelte_sfc_extracts_script_block() {
    let svelte_source = r"<script>
let count = 0;
function increment() { count += 1; }
</script>
<button on:click={increment}>{count}</button>";
    let path = PathBuf::from("Component.svelte");
    let result = tokenize_file(&path, svelte_source, false);
    assert!(
        !result.tokens.is_empty(),
        "Svelte SFC should produce tokens"
    );
    let has_let = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Let)));
    assert!(has_let, "Should tokenize let in <script> block");
    let has_function = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Function)));
    assert!(has_function, "Should tokenize function in <script> block");
    assert!(
        has_identifier(&result.tokens, "button") && has_identifier(&result.tokens, "on"),
        "Should tokenize Svelte markup"
    );
}

#[test]
fn tokenize_vue_sfc_adjusts_span_offsets() {
    let vue_source = "<template><div/></template>\n<script>\nconst x = 1;\n</script>";
    let path = PathBuf::from("Test.vue");
    let result = tokenize_file(&path, vue_source, false);
    for token in &result.tokens {
        if matches!(token.kind, TokenKind::Boundary(_)) {
            continue;
        }
        assert!(
            token.span.end as usize <= vue_source.len(),
            "Token span end ({}) should stay inside the full SFC source",
            token.span.end
        );
        let text = &vue_source[token.span.start as usize..token.span.end as usize];
        assert!(
            !text.is_empty(),
            "Token span should recover non-empty text from full SFC source"
        );
    }
}

#[test]
fn tokenize_astro_extracts_frontmatter() {
    let astro_source = "---\nimport { Layout } from '../layouts/Layout.astro';\nconst title = 'Home';\n---\n<Layout title={title}><h1>Hello</h1></Layout>";
    let path = PathBuf::from("page.astro");
    let result = tokenize_file(&path, astro_source, false);
    assert!(
        !result.tokens.is_empty(),
        "Astro frontmatter should produce tokens"
    );
    let has_import = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(has_import, "Should tokenize import in frontmatter");
    assert!(
        has_identifier(&result.tokens, "layout") && has_identifier(&result.tokens, "h1"),
        "Should tokenize Astro template markup"
    );
}

#[test]
fn tokenize_astro_without_frontmatter_tokenizes_template() {
    let astro_source = "<html><body>Hello</body></html>";
    let path = PathBuf::from("page.astro");
    let result = tokenize_file(&path, astro_source, false);
    assert!(
        has_identifier(&result.tokens, "html") && has_identifier(&result.tokens, "body"),
        "Astro without frontmatter should tokenize template markup"
    );
}

#[test]
fn tokenize_astro_adjusts_span_offsets() {
    let astro_source = "---\nconst x = 1;\n---\n<div/>";
    let path = PathBuf::from("page.astro");
    let result = tokenize_file(&path, astro_source, false);
    assert!(!result.tokens.is_empty());
    for token in &result.tokens {
        assert!(
            token.span.start >= 4,
            "Token span start ({}) should be offset into the full astro source",
            token.span.start
        );
    }
}

#[test]
fn tokenize_mdx_extracts_imports_and_exports() {
    let mdx_source = "import { Button } from './Button';\nexport const meta = { title: 'Hello' };\n\n# Hello World\n\n<Button>Click me</Button>";
    let path = PathBuf::from("page.mdx");
    let result = tokenize_file(&path, mdx_source, false);
    assert!(
        !result.tokens.is_empty(),
        "MDX should produce tokens from imports/exports"
    );
    let has_import = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Import)));
    assert!(has_import, "Should tokenize import in MDX");
    let has_export = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Keyword(KeywordType::Export)));
    assert!(has_export, "Should tokenize export in MDX");
}

#[test]
fn tokenize_mdx_without_statements_returns_empty() {
    let mdx_source = "# Just Markdown\n\nNo imports or exports here.";
    let path = PathBuf::from("page.mdx");
    let result = tokenize_file(&path, mdx_source, false);
    assert!(
        result.tokens.is_empty(),
        "MDX without imports/exports should produce no tokens"
    );
}

#[test]
fn tokenize_css_returns_tokens() {
    let css_source = ".foo { color: red; }\n.bar { font-size: 16px; }";
    let path = PathBuf::from("styles.css");
    let result = tokenize_file(&path, css_source, false);
    assert!(
        has_identifier(&result.tokens, "foo")
            && has_identifier(&result.tokens, "color")
            && has_identifier(&result.tokens, "font-size"),
        "CSS files should produce selector and declaration tokens"
    );
    assert!(result.line_count >= 1);
}

#[test]
fn tokenize_scss_returns_tokens() {
    let scss_source = "$color: red;\n.foo { color: $color; }";
    let path = PathBuf::from("styles.scss");
    let result = tokenize_file(&path, scss_source, false);
    assert!(
        has_identifier(&result.tokens, "$color") && has_identifier(&result.tokens, "foo"),
        "SCSS files should produce variable and selector tokens"
    );
}

#[test]
fn tokenize_less_and_sass_return_tokens() {
    let less = tokenize_file(
        &PathBuf::from("styles.less"),
        "@color: red;\n.foo { color: @color; }",
        false,
    );
    assert!(has_identifier(&less.tokens, "@color"));

    let sass = tokenize_file(
        &PathBuf::from("styles.sass"),
        "$color: red\n.foo\n  color: $color",
        false,
    );
    assert!(has_identifier(&sass.tokens, "$color"));
}

#[test]
fn tokenize_sfc_ignores_commented_out_template_blocks() {
    let source =
        "<!-- <template><IgnoredThing /></template> -->\n<template><UsedThing /></template>";
    let result = tokenize_file(&PathBuf::from("Component.vue"), source, false);
    assert!(!has_identifier(&result.tokens, "ignoredthing"));
    assert!(has_identifier(&result.tokens, "usedthing"));
}

#[test]
fn tokenize_sfc_style_src_does_not_tokenize_external_reference_as_style_body() {
    let source = r#"<template><div /></template><style src="./theme.css"></style>"#;
    let result = tokenize_file(&PathBuf::from("Component.vue"), source, false);
    assert!(!has_identifier(&result.tokens, "theme"));
}

fn has_identifier(tokens: &[SourceToken], name: &str) -> bool {
    tokens
        .iter()
        .any(|token| matches!(&token.kind, TokenKind::Identifier(value) if value == name))
}

#[test]
fn file_tokens_line_count_matches_source() {
    let source = "const x = 1;\nconst y = 2;\nconst z = 3;";
    let path = PathBuf::from("test.ts");
    let result = tokenize_file(&path, source, false);
    assert_eq!(result.line_count, 3);
    assert_eq!(result.source, source);
}

#[test]
fn file_tokens_line_count_minimum_is_one() {
    let path = PathBuf::from("test.ts");
    let result = tokenize_file(&path, "", false);
    assert_eq!(result.line_count, 1, "Empty file should have line_count 1");
}

#[test]
fn js_file_with_jsx_retries_as_jsx() {
    let jsx_code = r#"
function App() {
return (
    <div className="app">
        <h1>Hello World</h1>
        <p>Welcome to the app</p>
    </div>
);
}
"#;
    let path = PathBuf::from("app.js");
    let result = tokenize_file(&path, jsx_code, false);
    let has_brackets = result
        .tokens
        .iter()
        .any(|t| matches!(t.kind, TokenKind::Punctuation(PunctuationType::OpenBracket)));
    assert!(
        has_brackets,
        "JSX fallback retry should produce JSX tokens from .js file"
    );
}

#[test]
fn tokenize_no_extension_uses_default_source_type() {
    let path = PathBuf::from("Makefile");
    let result = tokenize_file(&path, "const x = 1;", false);
    assert!(result.line_count >= 1);
}
