use crate::tests::parse_ts as parse_source;

#[test]
fn require_destructured_empty_object() {
    let info = parse_source("const {} = require('./mod');");
    assert_eq!(info.require_calls.len(), 1);
    assert!(info.require_calls[0].destructured_names.is_empty());
    assert!(info.require_calls[0].local_name.is_none());
}

#[test]
fn require_destructured_multiple_properties() {
    let info = parse_source("const { a, b, c } = require('./mod');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(
        info.require_calls[0].destructured_names,
        vec!["a", "b", "c"]
    );
}

#[test]
fn require_destructured_with_rest_returns_empty() {
    let info = parse_source("const { a, ...rest } = require('./mod');");
    assert_eq!(info.require_calls.len(), 1);
    assert!(
        info.require_calls[0].destructured_names.is_empty(),
        "Rest element should cause extract_destructured_names to return empty vec"
    );
}

#[test]
fn require_destructured_computed_property_skipped() {
    let info = parse_source("const key = 'x';\nconst { [key]: val, b } = require('./mod');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(
        info.require_calls[0].destructured_names,
        vec!["b"],
        "Computed property should be skipped, only 'b' captured"
    );
}

#[test]
fn require_destructured_aliased_properties() {
    let info = parse_source("const { foo: localFoo, bar: localBar } = require('./mod');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(
        info.require_calls[0].destructured_names,
        vec!["foo", "bar"],
        "Aliased destructured names should use the key (imported) name, not the local alias"
    );
}

#[test]
fn dynamic_import_destructured_empty_object() {
    let info = parse_source("async function f() { const {} = await import('./mod'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
    assert!(info.dynamic_imports[0].local_name.is_none());
}

#[test]
fn dynamic_import_destructured_computed_property_skipped() {
    let info =
        parse_source("async function f() { const { [key]: val, b } = await import('./mod'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(
        info.dynamic_imports[0].destructured_names,
        vec!["b"],
        "Computed property should be skipped in dynamic import destructuring"
    );
}

#[test]
fn dynamic_import_destructured_aliased_properties() {
    let info =
        parse_source("async function f() { const { foo: f1, bar: b1 } = await import('./mod'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(
        info.dynamic_imports[0].destructured_names,
        vec!["foo", "bar"],
        "Aliased destructured names should use the key name"
    );
}

#[test]
fn require_with_variable_arg_not_captured() {
    let info = parse_source("const x = require(someVariable);");
    assert!(
        info.require_calls.is_empty(),
        "require() with a variable argument should not be captured"
    );
}

#[test]
fn require_with_template_literal_arg_not_captured() {
    let info = parse_source("const x = require(`./module`);");
    assert!(
        info.require_calls.is_empty(),
        "require() with a template literal should not be captured as a static require"
    );
}

#[test]
fn nested_require_inside_function_not_captured_as_declarator() {
    let info = parse_source("doSomething(require('foo'));");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "foo");
    assert!(info.require_calls[0].local_name.is_none());
    assert!(info.require_calls[0].destructured_names.is_empty());
}

#[test]
fn require_with_non_require_callee_not_captured() {
    let info = parse_source("const x = notRequire('foo');");
    assert!(
        info.require_calls.is_empty(),
        "Only functions named 'require' should be captured"
    );
}

/// Issue #2365: `import X = require('./y')` is the TypeScript spelling of a
/// CommonJS require binding and records the same require call as
/// `const X = require('./y')`.
#[test]
fn import_equals_require_records_a_non_destructured_require_call() {
    let info = parse_source("import Assigned = require('./assigned');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "./assigned");
    assert_eq!(
        info.require_calls[0].local_name.as_deref(),
        Some("Assigned")
    );
    assert!(info.require_calls[0].destructured_names.is_empty());
}

/// The specifier span anchors the `unresolved-import` squiggly under `'./y'`
/// rather than under the whole declaration.
#[test]
fn import_equals_require_anchors_the_specifier_span() {
    let source = "import Assigned = require('./assigned');";
    let info = parse_source(source);
    assert_eq!(info.require_calls.len(), 1);
    let call = &info.require_calls[0];
    assert_eq!(
        &source[call.source_span.start as usize..call.source_span.end as usize],
        "'./assigned'"
    );
    assert_eq!(
        &source[call.span.start as usize..call.span.end as usize],
        "require('./assigned')"
    );
}

/// `X.member` narrows the target's exports the way a namespace import does, so
/// the member access and the binding name both have to be recorded.
#[test]
fn import_equals_require_records_member_accesses_through_the_binding() {
    let info = parse_source(
        "import Assigned = require('./assigned');\nconsole.log(Assigned.viaAssignment);",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|access| access.object == "Assigned" && access.member == "viaAssignment"),
        "member accesses: {:?}",
        info.member_accesses
    );
}

/// `export import X = require('./y')` inside a namespace body is still an
/// import of `./y`.
#[test]
fn export_import_equals_require_inside_a_namespace_records_the_require_call() {
    let info =
        parse_source("export namespace Outer {\n  export import Inner = require('./inner');\n}");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "./inner");
    assert_eq!(info.require_calls[0].local_name.as_deref(), Some("Inner"));
}

/// Deliberate negative control: `import X = Some.Namespace` names an entity
/// declared in this file, not a module, so it stays a local alias and records
/// no edge.
#[test]
fn import_equals_entity_name_records_no_require_call() {
    let info = parse_source(
        "namespace Some {\n  export namespace Nested {\n    export const value = 1;\n  }\n}\nimport \
         Alias = Some;\nimport Deep = Some.Nested;",
    );
    assert!(
        info.require_calls.is_empty(),
        "an entity-name import-equals is a local alias: {:?}",
        info.require_calls
    );
    assert!(
        info.imports.is_empty(),
        "an entity-name import-equals records no import: {:?}",
        info.imports
    );
}
