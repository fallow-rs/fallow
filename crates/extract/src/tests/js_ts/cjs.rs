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

/// The binding lives in both the type and the value namespace, exactly like an
/// `import * as X` namespace import. The graph reads these two vectors to
/// decide which namespaces a member access credits, so a binding used in a type
/// annotation has to land in `type_referenced_import_bindings`.
#[test]
fn import_equals_require_classifies_type_and_value_usage() {
    let info = parse_source(
        "import T = require('./t');\nconsole.log(T.val);\nexport const f = (v: T.Shape): number => \
         v.n;",
    );
    assert!(
        info.type_referenced_import_bindings
            .iter()
            .any(|binding| binding == "T"),
        "`T.Shape` in an annotation is a type reference: {:?}",
        info.type_referenced_import_bindings
    );
    assert!(
        info.value_referenced_import_bindings
            .iter()
            .any(|binding| binding == "T"),
        "`T.val` in an expression is a value reference: {:?}",
        info.value_referenced_import_bindings
    );
}

/// A binding used only in type position credits type space alone, so the value
/// exports of the target stay narrowable.
#[test]
fn import_equals_require_used_only_in_type_position_stays_out_of_value_space() {
    let info =
        parse_source("import T = require('./t');\nexport const f = (v: T.Shape): number => v.n;");
    assert!(
        info.type_referenced_import_bindings
            .iter()
            .any(|binding| binding == "T"),
        "type usage: {:?}",
        info.type_referenced_import_bindings
    );
    assert!(
        !info
            .value_referenced_import_bindings
            .iter()
            .any(|binding| binding == "T"),
        "no expression reads the binding: {:?}",
        info.value_referenced_import_bindings
    );
}

/// An unreferenced import-equals binding is never reported as an unused import
/// binding: the require path has never done that, and the exported form
/// legitimately has no local reference.
#[test]
fn import_equals_require_never_reports_an_unused_import_binding() {
    let info = parse_source("import Unused = require('./unused');");
    assert!(
        !info
            .unused_import_bindings
            .iter()
            .any(|binding| binding == "Unused"),
        "unused import bindings: {:?}",
        info.unused_import_bindings
    );
}

/// `export import X = require('./x')` hands the module object to consumers the
/// graph cannot enumerate, so the binding is recorded as a whole-object use and
/// every export of the target keeps its credit. Both the file-level form and
/// the exported-namespace-body form record it.
#[test]
fn export_import_equals_require_records_a_whole_object_use() {
    let file_level = parse_source("export import Users = require('./users');");
    assert!(
        file_level
            .whole_object_uses
            .iter()
            .any(|name| name == "Users"),
        "file-level whole-object uses: {:?}",
        file_level.whole_object_uses
    );

    let in_namespace =
        parse_source("export namespace Outer {\n  export import Inner = require('./inner');\n}");
    assert!(
        in_namespace
            .whole_object_uses
            .iter()
            .any(|name| name == "Inner"),
        "namespace-body whole-object uses: {:?}",
        in_namespace.whole_object_uses
    );
}

/// Deliberate negative controls for the whole-object mark: an unexported
/// binding is narrowable through its member accesses, and the entity-name form
/// is a local alias with no module object behind it.
#[test]
fn import_equals_without_export_records_no_whole_object_use() {
    let plain = parse_source("import Users = require('./users');\nconsole.log(Users.alpha);");
    assert!(
        plain.whole_object_uses.is_empty(),
        "an unexported binding stays narrowable: {:?}",
        plain.whole_object_uses
    );

    let local_namespace = parse_source(
        "namespace Local {\n  export import Inner = require('./inner');\n}\nconsole.log(Local);",
    );
    assert!(
        !local_namespace
            .whole_object_uses
            .iter()
            .any(|name| name == "Inner"),
        "a binding inside an unexported namespace is not public API: {:?}",
        local_namespace.whole_object_uses
    );

    let entity_name = parse_source(
        "namespace Shapes {\n  export const box = 1;\n}\nexport import Alias = Shapes;",
    );
    assert!(
        entity_name.whole_object_uses.is_empty(),
        "an entity-name alias has no module object: {:?}",
        entity_name.whole_object_uses
    );
}
