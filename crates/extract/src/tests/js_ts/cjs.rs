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

/// A `declare module '...'` body is the only context outside file scope where
/// TypeScript accepts `export import X = require('...')`. The edge is recorded
/// there too, so the package is not a false unused dependency.
#[test]
fn export_import_equals_require_inside_an_ambient_module_records_the_require_call() {
    let info = parse_source(
        "declare module 'virtual:api' {\n  export import Inner = require('inner-pkg');\n}",
    );
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "inner-pkg");
    assert_eq!(info.require_calls[0].local_name.as_deref(), Some("Inner"));
}

/// `import type X = require('pkg')` is the one require spelling TypeScript
/// erases: the emitted JavaScript holds no `require` call, so the edge is
/// type-only and dependency classification must not read it as runtime usage.
/// The unerased spelling next to it stays a runtime require.
#[test]
fn import_type_equals_require_records_a_type_only_require_call() {
    let erased = parse_source("import type T = require('pkg');\nexport type Alias = T.Shape;");
    assert_eq!(erased.require_calls.len(), 1);
    assert_eq!(erased.require_calls[0].source, "pkg");
    assert!(
        erased.require_calls[0].is_type_only,
        "`import type X = require(...)` emits no require call at runtime"
    );

    let runtime = parse_source("import T = require('pkg');\nconsole.log(T.value);");
    assert_eq!(runtime.require_calls.len(), 1);
    assert!(
        !runtime.require_calls[0].is_type_only,
        "`import X = require(...)` does emit a require call"
    );

    let variable = parse_source("const T = require('pkg');\nconsole.log(T.value);");
    assert_eq!(variable.require_calls.len(), 1);
    assert!(
        !variable.require_calls[0].is_type_only,
        "`const X = require(...)` has no type-only spelling"
    );
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

#[test]
fn ordinary_require_classifies_qualified_type_usage() {
    let info = parse_source(
        "const T = require('./t');\nexport const f = (v: T.Shape): T.Result => v.value;",
    );
    assert!(
        info.type_referenced_import_bindings
            .iter()
            .any(|binding| binding == "T"),
        "`T.Shape` and `T.Result` are type references: {:?}",
        info.type_referenced_import_bindings
    );
    assert!(
        !info
            .value_referenced_import_bindings
            .iter()
            .any(|binding| binding == "T"),
        "the require binding is not read as a runtime value: {:?}",
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

/// An unreferenced import-equals binding is an unused import binding, exactly
/// as the `import * as X` spelling of the same binding is. TypeScript elides
/// both, so neither may buy the target a whole-object credit: crediting one
/// deleted every unused-export and unused-type row on the target.
#[test]
fn unreferenced_import_equals_reports_an_unused_import_binding() {
    let info = parse_source("import Unused = require('./unused');\nexport const other = 1;");
    assert!(
        info.unused_import_bindings
            .iter()
            .any(|binding| binding == "Unused"),
        "unused import bindings: {:?}",
        info.unused_import_bindings
    );

    let esm_twin = parse_source("import * as Unused from './unused';\nexport const other = 1;");
    assert_eq!(
        info.unused_import_bindings, esm_twin.unused_import_bindings,
        "the namespace-import twin reports the same way"
    );

    let ordinary = parse_source("const Unused = require('./unused');\nexport const other = 1;");
    assert_eq!(
        ordinary.unused_import_bindings, esm_twin.unused_import_bindings,
        "an ordinary require binding follows the same unused rule"
    );

    // The erased spelling is elided just as completely, and its twin is
    // `import type * as X`.
    let type_only =
        parse_source("import type Unused = require('./unused');\nexport const other = 1;");
    let type_only_esm_twin =
        parse_source("import type * as Unused from './unused';\nexport const other = 1;");
    assert_eq!(
        type_only.unused_import_bindings, type_only_esm_twin.unused_import_bindings,
        "the type-only namespace-import twin reports the same way"
    );
    assert!(
        type_only
            .unused_import_bindings
            .iter()
            .any(|binding| binding == "Unused"),
        "unused import bindings: {:?}",
        type_only.unused_import_bindings
    );

    // Positive control: a binding anything references is not reported, so the
    // assertion above is not passing because every binding lands there.
    let referenced = parse_source("import Used = require('./used');\nconsole.log(Used.alpha);");
    assert!(
        referenced.unused_import_bindings.is_empty(),
        "a referenced binding is not unused: {:?}",
        referenced.unused_import_bindings
    );
}

/// `export import X = require('./x')` is exempt: the binding is the file's
/// public API and has no local reference by construction, so reporting it
/// unused would withdraw the whole-object credit issue #2373 gives its
/// `import * as X; export { X }` twin.
#[test]
fn exported_import_equals_is_not_an_unused_import_binding() {
    let info = parse_source("export import Users = require('./users');");
    assert!(
        info.unused_import_bindings.is_empty(),
        "unused import bindings: {:?}",
        info.unused_import_bindings
    );

    let esm_twin = parse_source("import * as Users from './users';\nexport { Users };");
    assert_eq!(
        info.unused_import_bindings, esm_twin.unused_import_bindings,
        "the re-exported namespace-import twin reports the same way"
    );
}

/// `export import X = require('./x')` at file level hands the module object to
/// consumers the graph cannot enumerate, so the binding is recorded as a
/// whole-object use and every export of the target keeps its credit.
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
}

/// Lenient-parse pin, deliberately non-compiling input: TypeScript rejects
/// `export import X = require('...')` inside a namespace body with TS1147, so
/// no compiling project reaches this arm. fallow parses leniently and still
/// has to behave, which is what this pins: the edge is recorded and the
/// binding is credited, the same conservative direction the file-level form
/// takes. The valid spellings are covered by the file-level and
/// ambient-module tests above.
#[test]
fn export_import_equals_inside_a_namespace_body_is_a_lenient_parse_pin() {
    let info =
        parse_source("export namespace Outer {\n  export import Inner = require('./inner');\n}");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "./inner");
    assert_eq!(info.require_calls[0].local_name.as_deref(), Some("Inner"));
    assert!(
        info.whole_object_uses.iter().any(|name| name == "Inner"),
        "namespace-body whole-object uses: {:?}",
        info.whole_object_uses
    );
}

/// Deliberate negative controls for the whole-object mark: an unexported
/// binding is narrowable through its member accesses, a binding inside a
/// `declare module` body augments that module rather than this file's public
/// API, and the entity-name form is a local alias with no module object.
#[test]
fn import_equals_without_export_records_no_whole_object_use() {
    let plain = parse_source("import Users = require('./users');\nconsole.log(Users.alpha);");
    assert!(
        plain.whole_object_uses.is_empty(),
        "an unexported binding stays narrowable: {:?}",
        plain.whole_object_uses
    );

    let ambient = parse_source(
        "declare module 'virtual:api' {\n  export import Inner = require('inner-pkg');\n}",
    );
    assert!(
        ambient.whole_object_uses.is_empty(),
        "an ambient-module member is not this file's public API: {:?}",
        ambient.whole_object_uses
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

/// The mark is granted whatever else the file binds under that name. A
/// same-named local in a nested scope is not a reason to withhold it: the
/// `import * as X; export { X }` twin has no such condition, and withholding
/// reported every export of the real target as an auto-fixable unused export.
///
/// The twin carries no whole-object mark of its own here, so the two extraction
/// records differ; the target credit does not, because `export { X }` puts the
/// twin's binding on mark-all through the graph's re-export test instead
/// (issue #2373). `a_shadowed_export_import_equals_keeps_the_targets_credit` in
/// the integration suite pins that the two report identically end to end.
#[test]
fn export_import_equals_whole_object_use_survives_a_shadowed_name() {
    let shadowed = parse_source(
        "export import Session = require('./beta');\nexport const run = (): void => {\n  const \
         Session = makeSession();\n  Session.start();\n};\n",
    );
    assert!(
        shadowed
            .whole_object_uses
            .iter()
            .any(|name| name == "Session"),
        "a shadowed name keeps the mark the exported form earns: {:?}",
        shadowed.whole_object_uses
    );

    let unshadowed = parse_source(
        "export import Session = require('./beta');\nexport const run = (): void => {\n  const \
         local = makeSession();\n  local.start();\n};\n",
    );
    assert!(
        unshadowed
            .whole_object_uses
            .iter()
            .any(|name| name == "Session"),
        "an unshadowed name keeps its mark: {:?}",
        unshadowed.whole_object_uses
    );

    // One name yields one entry whether or not it is shadowed, so the grant
    // never stacks a second copy on the genuine one (issue #2377).
    assert_eq!(
        shadowed
            .whole_object_uses
            .iter()
            .filter(|name| *name == "Session")
            .count(),
        1,
        "whole-object uses: {:?}",
        shadowed.whole_object_uses
    );
}

/// A whole-object use the file genuinely wrote survives a shadowed name, and
/// the record matches the `import * as X; export { X }` twin exactly: both
/// hold the one `Object.values(X)` entry. An earlier revision withdrew the
/// exported form's mark by name, which deleted the genuine entry with it and
/// turned every export of the target into a false `unused-export` row.
#[test]
fn a_genuine_whole_object_use_survives_a_shadowed_export_import_equals() {
    let source = "export import Config = require('./config');\nexport const read = (): number => \
                  Object.values(Config).length;\nexport const parse = (Config: { n: number }): \
                  number => Config.n;\n";
    let shadowed = parse_source(source);
    assert!(
        shadowed.whole_object_uses.iter().any(|n| n == "Config"),
        "`Object.values(Config)` is a whole-object use the file wrote: {:?}",
        shadowed.whole_object_uses
    );

    let esm_twin = parse_source(
        "import * as Config from './config';\nexport { Config };\nexport const read = (): number \
         => Object.values(Config).length;\nexport const parse = (Config: { n: number }): number \
         => Config.n;\n",
    );
    assert_eq!(
        shadowed.whole_object_uses.to_vec(),
        esm_twin.whole_object_uses.to_vec(),
        "the namespace-import twin records the same whole-object uses"
    );
}

/// A bare reference that hands the module object on (a call argument, an alias,
/// a return value) credits every export the receiver can reach, exactly as the
/// `import * as X` spelling does since issue #2377. Without it a file with one
/// dotted access plus one handover narrows to that member and reports every
/// sibling the receiver still uses.
#[test]
fn a_bare_import_equals_reference_is_a_whole_object_use() {
    let handed = parse_source(
        "import Icons = require('./icons');\nconsole.log(Icons.Star);\nregister(Icons);\n",
    );
    assert!(
        handed.whole_object_uses.iter().any(|name| name == "Icons"),
        "a call argument hands the module object on: {:?}",
        handed.whole_object_uses
    );

    let esm_twin = parse_source(
        "import * as Icons from './icons';\nconsole.log(Icons.Star);\nregister(Icons);\n",
    );
    assert_eq!(
        handed.whole_object_uses.to_vec(),
        esm_twin.whole_object_uses.to_vec(),
        "the namespace-import twin records the same whole-object uses"
    );

    // Negative control: a dotted-only binding keeps narrowing, so the rule does
    // not credit every import-equals binding wholesale.
    let dotted = parse_source("import Icons = require('./icons');\nconsole.log(Icons.Star);\n");
    assert!(
        dotted.whole_object_uses.is_empty(),
        "a resolved member access is not a handover: {:?}",
        dotted.whole_object_uses
    );
}

/// One name yields one unused-import-binding row. A file that declares the
/// same name at root and inside a namespace body pushes two entries into the
/// candidate list, both resolving to the one root binding, and the graph reads
/// membership rather than a count.
#[test]
fn a_repeated_import_equals_name_reports_one_unused_import_binding() {
    let info = parse_source(
        "import Dup = require('./dup');\nexport namespace Outer {\n  import Dup = \
         require('./dup-inner');\n}\nexport const other = 1;",
    );
    assert_eq!(
        info.unused_import_bindings
            .iter()
            .filter(|binding| *binding == "Dup")
            .count(),
        1,
        "unused import bindings: {:?}",
        info.unused_import_bindings
    );
}
