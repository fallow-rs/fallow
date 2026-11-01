//! A `require` function based on the file itself acts like the CommonJS
//! `require`, in any spelling. The base is `import.meta.url`,
//! `import.meta.filename`, `__filename` or `fileURLToPath(import.meta.url)`.
//! The function is an inline
//! `createRequire(...)` call or a binding with any name. Another base resolves
//! from another directory, so a call on it records nothing.

use crate::tests::parse_ts as parse_source;

fn loads(source: &str) -> Vec<String> {
    parse_source(source)
        .require_calls
        .iter()
        .map(|call| call.source.clone())
        .collect()
}

fn package_references(source: &str) -> Vec<String> {
    parse_source(source).package_path_references.to_vec()
}

fn relative_references(source: &str) -> Vec<String> {
    parse_source(source)
        .dynamic_imports
        .iter()
        .map(|import| import.source.clone())
        .collect()
}

fn with_import(body: &str) -> String {
    format!("import {{ createRequire }} from 'node:module';\n{body}\n")
}

#[test]
fn inline_call_loads_each_this_file_base() {
    for base in [
        "import.meta.url",
        "import.meta.filename",
        "__filename",
        "fileURLToPath(import.meta.url)",
        "url.fileURLToPath(import.meta.url)",
    ] {
        let source = with_import(&format!("createRequire({base})('sax');"));
        assert_eq!(loads(&source), ["sax"], "base {base}");
    }
}

#[test]
fn inline_call_in_a_declaration_records_the_binding() {
    let info = parse_source(&with_import(
        "const sax = createRequire(import.meta.url)('sax');",
    ));
    let [call] = info.require_calls.as_slice() else {
        panic!("expected one call, got {:?}", info.require_calls);
    };
    assert_eq!(call.source, "sax");
    assert_eq!(call.local_name.as_deref(), Some("sax"));
}

#[test]
fn inline_call_in_a_destructure_records_the_names() {
    let info = parse_source(&with_import(
        "const { parser } = createRequire(import.meta.url)('sax');",
    ));
    let [call] = info.require_calls.as_slice() else {
        panic!("expected one call, got {:?}", info.require_calls);
    };
    assert_eq!(call.destructured_names, ["parser"]);
}

#[test]
fn inline_call_loads_through_a_chain_and_with_a_subpath() {
    let source = with_import(
        "export const a = createRequire(import.meta.url)('sax').parser(true);\n\
         export const b = createRequire(import.meta.url)('sax/lib/sax.js');",
    );
    assert_eq!(loads(&source), ["sax", "sax/lib/sax.js"]);
}

#[test]
fn member_callee_loads() {
    let source = "import * as mod from 'node:module';\n\
         mod.createRequire(import.meta.url)('sax');\n";
    assert_eq!(loads(source), ["sax"]);
}

#[test]
fn parenthesized_and_cast_callees_load() {
    let source = with_import(
        "(createRequire(import.meta.url))('a');\n\
         (createRequire(import.meta.url) as NodeRequire)('b');\n\
         const req = createRequire(import.meta.url);\n\
         (req as NodeRequire)('c');",
    );
    assert_eq!(loads(&source), ["a", "b", "c"]);
}

#[test]
fn inline_resolve_references_the_package_and_the_file() {
    let source = with_import(
        "createRequire(import.meta.url).resolve('some-pkg');\n\
         createRequire(import.meta.url).resolve('./shim.js');",
    );
    assert_eq!(package_references(&source), ["some-pkg"]);
    assert_eq!(relative_references(&source), ["./shim.js"]);
}

#[test]
fn inline_resolve_ignores_a_shadow_of_the_name_require() {
    let source = with_import(
        "export function f(require) {\n\
           return createRequire(import.meta.url).resolve('some-pkg');\n\
         }",
    );
    assert_eq!(package_references(&source), ["some-pkg"]);
}

#[test]
fn renamed_binding_loads_and_resolves() {
    let source = with_import(
        "const req = createRequire(import.meta.url);\n\
         req('sax');\n\
         req.resolve('some-pkg');\n\
         req.resolve('./shim.js');",
    );
    assert_eq!(loads(&source), ["sax"]);
    assert_eq!(package_references(&source), ["some-pkg"]);
    assert_eq!(relative_references(&source), ["./shim.js"]);
}

#[test]
fn renamed_binding_is_known_in_a_function_above_its_declaration() {
    let source = with_import(
        "export function load() { return req('sax'); }\n\
         export function find() { return req.resolve('some-pkg'); }\n\
         const req = createRequire(import.meta.url);",
    );
    assert_eq!(loads(&source), ["sax"]);
    assert_eq!(package_references(&source), ["some-pkg"]);
}

#[test]
fn exported_renamed_binding_loads() {
    let source = with_import("export const req = createRequire(import.meta.url);\nreq('sax');");
    assert_eq!(loads(&source), ["sax"]);
}

#[test]
fn nested_renamed_binding_loads_in_its_scope_only() {
    let source = with_import(
        "export function f() {\n\
           const req = createRequire(import.meta.url);\n\
           return req('in-scope');\n\
         }\n\
         export function g() {\n\
           return req('out-of-scope');\n\
         }",
    );
    assert_eq!(loads(&source), ["in-scope"]);
}

#[test]
fn nested_renamed_binding_is_known_above_its_declaration() {
    let source = with_import(
        "export function f() {\n\
           function inner() { return req('sax'); }\n\
           const req = createRequire(import.meta.url);\n\
           return inner();\n\
         }",
    );
    assert_eq!(loads(&source), ["sax"]);
}

#[test]
fn parameter_shadows_a_renamed_binding() {
    let source = with_import(
        "const req = createRequire(import.meta.url);\n\
         export function f(req) { return [req('sax'), req.resolve('some-pkg')]; }\n\
         export const kept = req('kept');",
    );
    assert_eq!(loads(&source), ["kept"]);
    assert!(package_references(&source).is_empty());
}

#[test]
fn arrow_and_method_parameters_shadow_a_renamed_binding() {
    let source = with_import(
        "const req = createRequire(import.meta.url);\n\
         export const arrow = (req) => req('arrow-param');\n\
         export class Loader { load(req) { return req('method-param'); } }\n\
         export const kept = () => req('kept');",
    );
    assert_eq!(loads(&source), ["kept"]);
}

#[test]
fn nested_declaration_shadows_a_renamed_binding() {
    let source = with_import(
        "const req = createRequire(import.meta.url);\n\
         export function f(load) {\n\
           const req = load;\n\
           return req('sax');\n\
         }",
    );
    assert!(loads(&source).is_empty());
}

#[test]
fn nested_renamed_binding_keeps_shadowing_an_import_of_the_same_name() {
    let source = "import { createRequire } from 'node:module';\n\
         import helper from './helper.js';\n\
         export function f() {\n\
           const helper = createRequire(import.meta.url);\n\
           return helper.member;\n\
         }\n\
         export const outer = helper.outer;\n";
    let info = parse_source(source);
    let members: Vec<&str> = info
        .member_accesses
        .iter()
        .filter(|access| access.object == "helper")
        .map(|access| access.member.as_str())
        .collect();
    assert_eq!(members, ["outer"]);
}

#[test]
fn aliased_import_loads_inline_and_through_a_binding() {
    for module in ["node:module", "module"] {
        let source = format!(
            "import {{ createRequire as cr }} from '{module}';\n\
             cr(import.meta.url)('inline');\n\
             cr(import.meta.url).resolve('resolved');\n\
             const req = cr(import.meta.url);\n\
             req('named');\n"
        );
        assert_eq!(loads(&source), ["inline", "named"], "module {module}");
        assert_eq!(package_references(&source), ["resolved"], "module {module}");
    }
}

#[test]
fn aliased_require_destructure_loads() {
    let source = "const { createRequire: cr } = require('node:module');\n\
         cr(__filename)('sax');\n";
    assert_eq!(loads(source), ["node:module", "sax"]);
}

#[test]
fn alias_from_another_module_does_not_load() {
    let source = "import { createRequire as cr } from './my-loader.js';\n\
         cr(import.meta.url)('sax');\n";
    assert!(loads(source).is_empty());
}

#[test]
fn type_only_alias_does_not_load() {
    let source = "import { type createRequire as cr } from 'node:module';\n\
         cr(import.meta.url)('sax');\n";
    assert!(loads(source).is_empty());
}

#[test]
fn parameter_shadows_an_aliased_import() {
    let source = "import { createRequire as cr } from 'node:module';\n\
         export function f(cr) { return cr(import.meta.url)('sax'); }\n";
    assert!(loads(source).is_empty());
}

#[test]
fn another_base_records_nothing() {
    for body in [
        "createRequire('/other/base')('sax');",
        "createRequire(process.cwd())('sax');",
        "createRequire('/other/base').resolve('sax');",
        "const req = createRequire('/other/base');\nreq('sax');\nreq.resolve('sax');",
        "createRequire(import.meta.url, 'extra')('sax');",
        "createRequire(fileURLToPath(other))('sax');",
    ] {
        let source = with_import(body);
        assert!(loads(&source).is_empty(), "load in {body}");
        assert!(package_references(&source).is_empty(), "resolve in {body}");
    }
}

#[test]
fn local_function_with_another_base_records_nothing() {
    let source = "const createRequire = (u) => (n) => n;\ncreateRequire('x')('sax');\n";
    assert!(loads(source).is_empty());
}

#[test]
fn module_level_require_with_another_base_keeps_its_behavior() {
    let source = with_import("const require = createRequire('/other/base');\nrequire('sax');");
    assert_eq!(loads(&source), ["sax"]);
}

#[test]
fn nested_require_with_another_base_still_shadows_resolve() {
    let source = with_import(
        "export function f(dir) {\n\
           const require = createRequire(join(dir, 'x.json'));\n\
           return require.resolve('some-pkg');\n\
         }",
    );
    assert!(package_references(&source).is_empty());
}

#[test]
fn helper_parameter_credits_through_each_form() {
    for (declaration, call) in [
        ("", "createRequire(import.meta.url).resolve(name)"),
        (
            "const req = createRequire(import.meta.url);\n",
            "req.resolve(name)",
        ),
    ] {
        let source = with_import(&format!(
            "{declaration}function find(name) {{ return {call}; }}\nfind('some-pkg');"
        ));
        assert_eq!(package_references(&source), ["some-pkg"], "form {call}");
    }
}

#[test]
fn helper_with_a_local_renamed_binding_credits_its_parameter() {
    let source = with_import(
        "function find(name) {\n\
           const req = createRequire(import.meta.url);\n\
           return req.resolve(name);\n\
         }\n\
         find('some-pkg');",
    );
    assert_eq!(package_references(&source), ["some-pkg"]);
}

#[test]
fn helper_parameter_shadows_a_renamed_binding() {
    let source = with_import(
        "const req = createRequire(import.meta.url);\n\
         function find(req, name) { return req.resolve(name); }\n\
         function nested(name) {\n\
           const inner = (req) => req.resolve(name);\n\
           return inner;\n\
         }\n\
         find(other, 'some-pkg');\n\
         nested('other-pkg');",
    );
    assert!(package_references(&source).is_empty());
}
