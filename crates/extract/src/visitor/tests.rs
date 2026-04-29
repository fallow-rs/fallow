// Visitor tests invoke Oxc parser which is ~1000x slower under Miri.
#![cfg(all(test, not(miri)))]

use std::path::Path;

use super::*;
use crate::tests::parse_ts as parse;
use crate::{ImportedName, MemberKind};
use fallow_types::discover::FileId;
use helpers::regex_pattern_to_suffix;

// ── into_module_info transfers all fields ────────────────────

#[test]
fn into_module_info_transfers_exports() {
    let info = parse("export const a = 1; export function b() {}");
    assert_eq!(info.exports.len(), 2);
    assert_eq!(info.file_id, FileId(0));
}

#[test]
fn into_module_info_transfers_imports() {
    let info = parse("import { foo } from './bar'; import baz from 'baz';");
    assert_eq!(info.imports.len(), 2);
}

#[test]
fn into_module_info_transfers_re_exports() {
    let info = parse("export { foo } from './bar'; export * from './baz';");
    assert_eq!(info.re_exports.len(), 2);
}

#[test]
fn into_module_info_transfers_dynamic_imports() {
    let info = parse("const m = import('./lazy');");
    assert_eq!(info.dynamic_imports.len(), 1);
}

#[test]
fn into_module_info_transfers_require_calls() {
    let info = parse("const x = require('./util');");
    assert_eq!(info.require_calls.len(), 1);
}

#[test]
fn into_module_info_transfers_whole_object_uses() {
    let info = parse(
        "import { Status } from './types';\nObject.values(Status);\nconst y = { ...Status };",
    );
    // Object.values + spread = 2 whole-object uses
    assert!(info.whole_object_uses.len() >= 2);
}

#[test]
fn into_module_info_transfers_member_accesses() {
    let info = parse("import { Obj } from './x';\nObj.method();");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Obj" && a.member == "method")
    );
}

#[test]
fn into_module_info_transfers_cjs_flag() {
    let info = parse("module.exports = {};");
    assert!(info.has_cjs_exports);
}

// ── merge_into extends (not replaces) ────────────────────────

#[test]
fn merge_into_extends_imports() {
    let mut base = parse("import { a } from './a';");
    let _extra = parse("import { b } from './b';");

    // Build a second extractor from parsing and merge
    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::from_path(Path::new("extra.ts")).unwrap_or_default();
    let parser_return =
        oxc_parser::Parser::new(&allocator, "import { c } from './c';", source_type).parse();
    let mut extractor = ModuleInfoExtractor::new();
    oxc_ast_visit::Visit::visit_program(&mut extractor, &parser_return.program);
    extractor.merge_into(&mut base);

    assert!(
        base.imports.len() >= 2,
        "merge_into should add to existing imports, not replace"
    );
}

#[test]
fn merge_into_ors_cjs_flag() {
    let mut base = parse("export const x = 1;");
    assert!(!base.has_cjs_exports);

    let allocator = oxc_allocator::Allocator::default();
    let source_type = oxc_span::SourceType::from_path(Path::new("cjs.ts")).unwrap_or_default();
    let parser_return =
        oxc_parser::Parser::new(&allocator, "module.exports = {};", source_type).parse();
    let mut extractor = ModuleInfoExtractor::new();
    oxc_ast_visit::Visit::visit_program(&mut extractor, &parser_return.program);
    extractor.merge_into(&mut base);

    assert!(base.has_cjs_exports, "merge_into should OR the cjs flag");
}

// ── Class member extraction ──────────────────────────────────

#[test]
fn extracts_public_class_methods_and_properties() {
    let info = parse(
        r"
            export class MyService {
                name: string;
                getValue() { return 1; }
            }
            ",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "MyService"));
    assert!(class_export.is_some());
    let members = &class_export.unwrap().members;
    assert!(
        members
            .iter()
            .any(|m| m.name == "name" && m.kind == MemberKind::ClassProperty),
        "should extract 'name' property"
    );
    assert!(
        members
            .iter()
            .any(|m| m.name == "getValue" && m.kind == MemberKind::ClassMethod),
        "should extract 'getValue' method"
    );
}

#[test]
fn skips_constructor_in_class_members() {
    let info = parse(
        r"
            export class Foo {
                constructor() {}
                doWork() {}
            }
            ",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "Foo"));
    let members = &class_export.unwrap().members;
    assert!(
        !members.iter().any(|m| m.name == "constructor"),
        "constructor should be skipped"
    );
    assert!(members.iter().any(|m| m.name == "doWork"));
}

#[test]
fn skips_private_and_protected_members() {
    let info = parse(
        r"
            export class Foo {
                private secret: string;
                protected internal(): void {}
                public visible: number;
            }
            ",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "Foo"));
    let members = &class_export.unwrap().members;
    assert!(
        !members.iter().any(|m| m.name == "secret"),
        "private members should be skipped"
    );
    assert!(
        !members.iter().any(|m| m.name == "internal"),
        "protected members should be skipped"
    );
    assert!(
        members.iter().any(|m| m.name == "visible"),
        "public members should be included"
    );
}

#[test]
fn class_member_with_decorator_flagged() {
    let info = parse(
        r"
            function Injectable() { return (target: any) => target; }
            export class Service {
                @Injectable()
                handler() {}
            }
            ",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "Service"));
    let members = &class_export.unwrap().members;
    let handler = members.iter().find(|m| m.name == "handler");
    assert!(handler.is_some());
    assert!(
        handler.unwrap().has_decorator,
        "decorated member should have has_decorator = true"
    );
}

#[test]
fn local_class_export_specifier_keeps_members_and_heritage() {
    let info = parse(
        r"
            interface Authorizable {
                authorize(): boolean;
            }

            class SecureCommand implements Authorizable {
                authorize(): boolean {
                    return true;
                }

                cleanup(): void {}
            }

            export { SecureCommand };
            ",
    );

    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "SecureCommand"))
        .expect("SecureCommand export should exist");
    assert!(
        class_export
            .members
            .iter()
            .any(|m| m.name == "authorize" && m.kind == MemberKind::ClassMethod),
        "export specifier should preserve class methods"
    );
    assert!(
        class_export
            .members
            .iter()
            .any(|m| m.name == "cleanup" && m.kind == MemberKind::ClassMethod),
        "export specifier should preserve all public class methods"
    );

    assert!(
        info.class_heritage.iter().any(|heritage| {
            heritage.export_name == "SecureCommand"
                && heritage.implements == vec!["Authorizable".to_string()]
        }),
        "export specifier should preserve implements metadata"
    );
}

// ── Enum member extraction ───────────────────────────────────

#[test]
fn extracts_enum_members() {
    let info = parse(
        r"
            export enum Direction {
                Up,
                Down,
                Left,
                Right
            }
            ",
    );
    let enum_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "Direction"));
    assert!(enum_export.is_some());
    let members = &enum_export.unwrap().members;
    assert_eq!(members.len(), 4);
    assert!(members.iter().all(|m| m.kind == MemberKind::EnumMember));
    assert!(members.iter().any(|m| m.name == "Up"));
    assert!(members.iter().any(|m| m.name == "Right"));
}

// ── Whole-object use patterns ────────────────────────────────

#[test]
fn object_values_marks_whole_use() {
    let info = parse("import { E } from './e';\nObject.values(E);");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

#[test]
fn object_keys_marks_whole_use() {
    let info = parse("import { E } from './e';\nObject.keys(E);");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

#[test]
fn object_entries_marks_whole_use() {
    let info = parse("import { E } from './e';\nObject.entries(E);");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

#[test]
fn for_in_marks_whole_use() {
    let info = parse("import { E } from './e';\nfor (const k in E) {}");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

#[test]
fn spread_marks_whole_use() {
    let info = parse("import { E } from './e';\nconst x = { ...E };");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

#[test]
fn dynamic_computed_access_marks_whole_use() {
    let info = parse("import { E } from './e';\nconst k = 'x';\nE[k];");
    assert!(info.whole_object_uses.contains(&"E".to_string()));
}

// ── this.member tracking ─────────────────────────────────────

#[test]
fn this_member_access_tracked() {
    let info = parse(
        r"
            export class Foo {
                bar: number;
                baz() { return this.bar; }
            }
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "this" && a.member == "bar"),
        "this.bar should be tracked as a member access"
    );
}

#[test]
fn this_assignment_tracked() {
    let info = parse(
        r"
            export class Foo {
                bar: number;
                init() { this.bar = 42; }
            }
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "this" && a.member == "bar"),
        "this.bar = ... should be tracked as a member access"
    );
}

// ── Instance member access tracking ─────────────────────────

#[test]
fn instance_member_access_mapped_to_class() {
    let info = parse(
        r"
            import { MyService } from './service';
            const svc = new MyService();
            svc.greet();
            ",
    );
    // svc.greet() should produce a MemberAccess for MyService.greet
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "greet"),
        "svc.greet() should be mapped to MyService.greet, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn instance_property_access_mapped_to_class() {
    let info = parse(
        r"
            import { MyClass } from './class';
            const obj = new MyClass();
            console.log(obj.name);
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyClass" && a.member == "name"),
        "obj.name should be mapped to MyClass.name, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn injected_object_member_access_mapped_to_class() {
    let info = parse(
        r"
            import { FooClass } from './foo';

            class MyClass {
                constructor(private deps: { foo: FooClass }) {}

                test() {
                    this.deps.foo.foo();
                }
            }
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "FooClass" && a.member == "foo"),
        "this.deps.foo.foo() should map to FooClass.foo, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn assigned_nested_object_member_access_mapped_to_class() {
    let info = parse(
        r"
            import { FooClass } from './foo';

            class MyClass {
                constructor(deps: { foo: FooClass }) {
                    this.deps = deps;
                }

                test() {
                    this.deps.foo.foo();
                }
            }
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "FooClass" && a.member == "foo"),
        "this.deps = deps assignment should propagate nested bindings so this.deps.foo.foo() maps to FooClass.foo, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn instance_whole_object_use_mapped_to_class() {
    let info = parse(
        r"
            import { MyClass } from './class';
            const obj = new MyClass();
            Object.keys(obj);
            ",
    );
    assert!(
        info.whole_object_uses.contains(&"MyClass".to_string()),
        "Object.keys(obj) should map to whole-object use of MyClass, found: {:?}",
        info.whole_object_uses
    );
}

#[test]
fn non_instance_binding_not_mapped() {
    let info = parse(
        r"
            const obj = { greet() {} };
            obj.greet();
            ",
    );
    // obj is not a `new` binding, so no class mapping should exist.
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| { a.object != "obj" && a.object != "this" && a.object != "console" }),
        "non-instance bindings should not produce class-mapped accesses, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn instance_binding_with_no_access_produces_nothing() {
    let info = parse(
        r"
            import { Foo } from './foo';
            const x = new Foo();
            ",
    );
    // Binding exists but no x.method() calls — no synthetic accesses should be emitted.
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "Foo"),
        "binding with no member access should not produce Foo entries, found: {:?}",
        info.member_accesses
    );
    assert!(
        !info.whole_object_uses.contains(&"Foo".to_string()),
        "binding with no whole-object use should not produce Foo entries, found: {:?}",
        info.whole_object_uses
    );
}

#[test]
fn builtin_constructor_not_tracked() {
    let info = parse(
        r"
            const url = new URL('https://example.com');
            url.href;
            const m = new Map();
            m.get('key');
            ",
    );
    // Built-in constructors should not create instance bindings
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "URL"),
        "new URL() should not create instance binding, found: {:?}",
        info.member_accesses
    );
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "Map"),
        "new Map() should not create instance binding, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn multiple_instances_same_class() {
    let info = parse(
        r"
            import { Svc } from './svc';
            const a = new Svc();
            const b = new Svc();
            a.foo();
            b.bar();
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Svc" && a.member == "foo"),
        "a.foo() should map to Svc.foo, found: {:?}",
        info.member_accesses
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Svc" && a.member == "bar"),
        "b.bar() should map to Svc.bar, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn exported_instance_binding_is_recorded() {
    let info = parse(
        r"
            import { Box } from './box';
            export const box = new Box();
            ",
    );

    assert!(
        info.member_accesses.iter().any(|a| {
            a.object == format!("{}box", crate::INSTANCE_EXPORT_SENTINEL) && a.member == "Box"
        }),
        "exported instance binding should be recorded, found: {:?}",
        info.member_accesses
    );
}

// ── Factory initializer instance tracking ──────────────────

#[test]
fn array_destructured_factory_arrow_expression_body() {
    let info = parse(
        r"
            import { MyService } from './service';
            const [svc] = useState(() => new MyService());
            svc.process();
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "process"),
        "svc.process() should be mapped to MyService.process, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn array_destructured_factory_arrow_block_body() {
    let info = parse(
        r"
            import { MyService } from './service';
            const [svc, setSvc] = useState(() => { return new MyService(); });
            svc.greet();
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "greet"),
        "svc.greet() should be mapped to MyService.greet, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn array_destructured_factory_function_expression() {
    let info = parse(
        r"
            import { MyService } from './service';
            const [svc] = useMemo(function() { return new MyService(); }, []);
            svc.run();
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "run"),
        "svc.run() should be mapped to MyService.run, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn array_destructured_factory_builtin_not_tracked() {
    let info = parse(
        r"
            const [m] = useState(() => new Map());
            m.get('key');
            ",
    );
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "Map"),
        "new Map() through factory should not create instance binding, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn array_destructured_factory_whole_object_use() {
    let info = parse(
        r"
            import { Config } from './config';
            const [cfg] = useState(() => new Config());
            Object.keys(cfg);
            ",
    );
    assert!(
        info.whole_object_uses.contains(&"Config".to_string()),
        "Object.keys(cfg) should map to whole-object use of Config, found: {:?}",
        info.whole_object_uses
    );
}

#[test]
fn non_array_destructured_call_not_tracked() {
    let info = parse(
        r"
            import { Foo } from './foo';
            const result = someFunc(() => new Foo());
            result.bar();
            ",
    );
    // Without array destructuring, the factory pattern should not create a binding
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Foo" && a.member == "bar"),
        "non-array-destructured call should not create instance binding, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn array_destructured_no_factory_not_tracked() {
    let info = parse(
        r"
            import { Foo } from './foo';
            const [x] = someFunc(42);
            x.bar();
            ",
    );
    // No factory argument — should not create instance binding
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Foo" && a.member == "bar"),
        "array destructuring without factory should not map to Foo, found: {:?}",
        info.member_accesses
    );
}

// ── Typed-binding nullable unions ─────

#[test]
fn type_annotation_nullable_union_undefined_binds_class() {
    let info = parse(
        r"
            import { Aggregate } from './aggregate';
            let x: Aggregate | undefined;
            x = loadAggregate();
            x.someMutation();
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Aggregate" && a.member == "someMutation"),
        "x.someMutation() should map to Aggregate.someMutation through `Aggregate | undefined`, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_annotation_nullable_union_null_binds_class() {
    let info = parse(
        r"
            import { Aggregate } from './aggregate';
            let x: Aggregate | null;
            x.someMutation();
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Aggregate" && a.member == "someMutation"),
        "x.someMutation() should map through `Aggregate | null`, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_annotation_three_way_nullable_union_binds_class() {
    let info = parse(
        r"
            import { Aggregate } from './aggregate';
            let x: Aggregate | null | undefined;
            x.someMutation();
            ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Aggregate" && a.member == "someMutation"),
        "x.someMutation() should map through `Aggregate | null | undefined`, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_annotation_promise_not_unwrapped() {
    let info = parse(
        r"
            import { Aggregate } from './aggregate';
            const result: Promise<Aggregate> = repo.findById(id);
            result.someMutation();
            ",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Aggregate" && a.member == "someMutation"),
        "Promise<Aggregate> should not bind Promise object members to Aggregate, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_annotation_promise_nullable_union_not_unwrapped() {
    let info = parse(
        r"
            import { Aggregate } from './aggregate';
            const result: Promise<Aggregate | undefined> = repo.findById(id);
            result.someMutation();
            ",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Aggregate" && a.member == "someMutation"),
        "Promise<Aggregate | undefined> should not bind Promise object members to Aggregate, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_annotation_multi_class_union_not_bound() {
    let info = parse(
        r"
            import { Aggregate, Other } from './aggregate';
            let x: Aggregate | Other;
            x.someMutation();
            ",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| (a.object == "Aggregate" || a.object == "Other") && a.member == "someMutation"),
        "ambiguous `Aggregate | Other` union should not pick a class, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_annotation_array_generic_not_unwrapped() {
    let info = parse(
        r"
            import { Aggregate } from './aggregate';
            let xs: Array<Aggregate>;
            xs.someMutation();
            ",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Aggregate" && a.member == "someMutation"),
        "Array<Aggregate> binds the array, not its element type, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_annotation_qualified_promise_not_unwrapped() {
    // `Foo.Promise<Aggregate>` is not the global Promise; binding should fall back
    // to the bare reference name (`Foo.Promise`), not unwrap to `Aggregate`.
    let info = parse(
        r"
            import { Aggregate, Foo } from './foo';
            let x: Foo.Promise<Aggregate>;
            x.someMutation();
            ",
    );
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Aggregate" && a.member == "someMutation"),
        "qualified `Foo.Promise<Aggregate>` should not unwrap to Aggregate, found: {:?}",
        info.member_accesses
    );
}

// ── this.field chained member access ────────────────────────

#[test]
fn this_field_new_assignment_enables_chained_access() {
    let info = parse(
        r"
            import { MyService } from './service';
            class App {
                constructor() {
                    this.service = new MyService();
                }
                run() {
                    this.service.doWork();
                }
            }
            ",
    );
    // this.service.doWork() should be mapped to MyService.doWork
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "doWork"),
        "this.service.doWork() should be mapped to MyService.doWork, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn this_field_chained_access_without_new_not_mapped() {
    let info = parse(
        r"
            class App {
                run() {
                    this.config.getValue();
                }
            }
            ",
    );
    // No `this.config = new Config()` assignment, so no class mapping.
    // The raw `this.config.getValue` access should exist but not be resolved to a class.
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "this.config" && a.member == "getValue"),
        "raw this.config.getValue access should be recorded, found: {:?}",
        info.member_accesses
    );
    // No class-level mapping should exist
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "Config" && a.member == "getValue"),
        "without assignment, no class mapping should exist, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn typed_variable_binding_maps_member_access() {
    let info = parse(
        r"
            import { VirtualScrollStrategy } from './strategy';
            const strategy: VirtualScrollStrategy = createStrategy();
            strategy.attach();
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "VirtualScrollStrategy" && a.member == "attach"),
        "typed variable binding should map strategy.attach() to VirtualScrollStrategy.attach, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn type_only_alias_binding_maps_member_access() {
    let info = parse(
        r"
            import type { VirtualScrollStrategy as Strategy } from './strategy';
            const strategy: Strategy = createStrategy();
            strategy.attach();
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Strategy" && a.member == "attach"),
        "type-only aliased binding should map strategy.attach() to Strategy.attach, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn this_field_assignment_from_typed_parameter_maps_member_access() {
    let info = parse(
        r"
            import { VirtualScrollStrategy } from './strategy';
            class ScrollViewport {
                private strategy: VirtualScrollStrategy;

                constructor(strategy: VirtualScrollStrategy) {
                    this.strategy = strategy;
                }

                initialize() {
                    this.strategy.attach();
                }
            }
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "VirtualScrollStrategy" && a.member == "attach"),
        "typed field assignment should map this.strategy.attach() to VirtualScrollStrategy.attach, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn parameter_property_maps_this_field_member_access() {
    let info = parse(
        r"
            import { VirtualScrollStrategy } from './strategy';
            class ScrollViewport {
                constructor(private strategy: VirtualScrollStrategy) {}

                initialize() {
                    this.strategy.attach();
                }
            }
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "VirtualScrollStrategy" && a.member == "attach"),
        "parameter property should map this.strategy.attach() to VirtualScrollStrategy.attach, found: {:?}",
        info.member_accesses
    );
}

#[test]
fn this_field_builtin_constructor_not_tracked() {
    let info = parse(
        r"
            class App {
                constructor() {
                    this.cache = new Map();
                }
                run() {
                    this.cache.get('key');
                }
            }
            ",
    );
    // Built-in constructors should not create this.field bindings
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "Map"),
        "new Map() should not create this.field instance binding, found: {:?}",
        info.member_accesses
    );
}

// ── CJS export patterns ──────────────────────────────────────

#[test]
fn module_exports_object_extracts_keys() {
    let info = parse("module.exports = { foo: 1, bar: 2 };");
    assert!(info.has_cjs_exports);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "foo"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "bar"))
    );
}

#[test]
fn exports_dot_property() {
    let info = parse("exports.myFunc = function() {};");
    assert!(info.has_cjs_exports);
    assert!(
        info.exports
            .iter()
            .any(|e| { matches!(&e.name, ExportName::Named(n) if n == "myFunc") })
    );
}

// ── Destructured require/import ──────────────────────────────

#[test]
fn destructured_require_captures_names() {
    let info = parse("const { readFile, writeFile } = require('fs');");
    assert_eq!(info.require_calls.len(), 1);
    let call = &info.require_calls[0];
    assert_eq!(call.source, "fs");
    assert!(call.destructured_names.contains(&"readFile".to_string()));
    assert!(call.destructured_names.contains(&"writeFile".to_string()));
}

#[test]
fn namespace_require_has_local_name() {
    let info = parse("const fs = require('fs');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].local_name, Some("fs".to_string()));
    assert!(info.require_calls[0].destructured_names.is_empty());
}

#[test]
fn destructured_await_import_captures_names() {
    let info = parse("const { foo, bar } = await import('./mod');");
    assert_eq!(info.dynamic_imports.len(), 1);
    let imp = &info.dynamic_imports[0];
    assert_eq!(imp.source, "./mod");
    assert!(imp.destructured_names.contains(&"foo".to_string()));
    assert!(imp.destructured_names.contains(&"bar".to_string()));
}

#[test]
fn namespace_await_import_has_local_name() {
    let info = parse("const mod = await import('./mod');");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].local_name, Some("mod".to_string()));
}

// ── new URL pattern ──────────────────────────────────────────

#[test]
fn new_url_with_import_meta_url_tracked() {
    let info = parse("const w = new URL('./worker.js', import.meta.url);");
    assert!(
        info.dynamic_imports
            .iter()
            .any(|d| d.source == "./worker.js"),
        "new URL('./worker.js', import.meta.url) should be tracked as dynamic import"
    );
}

// ── import.meta.glob ─────────────────────────────────────────

#[test]
fn import_meta_glob_string_pattern() {
    let info = parse("const mods = import.meta.glob('./modules/*.ts');");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./modules/*.ts");
}

#[test]
fn import_meta_glob_array_patterns() {
    let info = parse("const mods = import.meta.glob(['./a/*.ts', './b/*.ts']);");
    assert_eq!(info.dynamic_import_patterns.len(), 2);
}

// ── require.context ──────────────────────────────────────────

#[test]
fn require_context_non_recursive() {
    let info = parse("const ctx = require.context('./components', false);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./components/");
}

#[test]
fn require_context_recursive() {
    let info = parse("const ctx = require.context('./components', true);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./components/**/");
}

#[test]
fn require_context_regex_simple_extension() {
    let info = parse("const ctx = require.context('./components', true, /\\.vue$/);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./components/**/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".vue".to_string())
    );
}

#[test]
fn require_context_regex_optional_char() {
    let info = parse("const ctx = require.context('./src', true, /\\.tsx?$/);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".{ts,tsx}".to_string())
    );
}

#[test]
fn require_context_regex_alternation() {
    let info = parse("const ctx = require.context('./src', false, /\\.(js|ts)$/);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./src/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".{js,ts}".to_string())
    );
}

#[test]
fn require_context_no_regex_has_no_suffix() {
    let info = parse("const ctx = require.context('./icons', true);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert!(info.dynamic_import_patterns[0].suffix.is_none());
}

// ── regex_pattern_to_suffix unit tests ──────────────────────

#[test]
fn regex_suffix_simple_ext() {
    assert_eq!(regex_pattern_to_suffix(r"\.vue$"), Some(".vue".to_string()));
    assert_eq!(
        regex_pattern_to_suffix(r"\.json$"),
        Some(".json".to_string())
    );
    assert_eq!(regex_pattern_to_suffix(r"\.css$"), Some(".css".to_string()));
}

#[test]
fn regex_suffix_optional_char() {
    assert_eq!(
        regex_pattern_to_suffix(r"\.tsx?$"),
        Some(".{ts,tsx}".to_string())
    );
    assert_eq!(
        regex_pattern_to_suffix(r"\.jsx?$"),
        Some(".{js,jsx}".to_string())
    );
}

#[test]
fn regex_suffix_alternation() {
    assert_eq!(
        regex_pattern_to_suffix(r"\.(js|ts)$"),
        Some(".{js,ts}".to_string())
    );
    assert_eq!(
        regex_pattern_to_suffix(r"\.(js|jsx|ts|tsx)$"),
        Some(".{js,jsx,ts,tsx}".to_string())
    );
}

#[test]
fn regex_suffix_complex_returns_none() {
    // Patterns too complex to convert
    assert_eq!(regex_pattern_to_suffix(r"\..*$"), None);
    assert_eq!(regex_pattern_to_suffix(r"\.[^.]+$"), None);
    assert_eq!(regex_pattern_to_suffix(r"test"), None);
}

// ── Whole-object-use edge cases ─────────────────────────────

#[test]
fn for_in_loop_marks_enum_as_whole_use() {
    let info =
        parse("import { MyEnum } from './types';\nfor (const key in MyEnum) { console.log(key); }");
    assert!(
        info.whole_object_uses.contains(&"MyEnum".to_string()),
        "for...in should mark MyEnum as whole-object-use"
    );
}

#[test]
fn spread_in_object_marks_whole_use() {
    let info = parse("import { obj } from './data';\nconst copy = { ...obj };");
    assert!(
        info.whole_object_uses.contains(&"obj".to_string()),
        "spread in object literal should mark obj as whole-object-use"
    );
}

#[test]
fn object_get_own_property_names_marks_whole_use() {
    let info = parse("import { MyEnum } from './types';\nObject.getOwnPropertyNames(MyEnum);");
    assert!(
        info.whole_object_uses.contains(&"MyEnum".to_string()),
        "Object.getOwnPropertyNames should mark MyEnum as whole-object-use"
    );
}

#[test]
fn nested_member_access_only_tracks_object() {
    let info = parse("import { obj } from './data';\nconst val = obj.nested.prop;");
    // obj should be tracked as a member access, not as whole-object-use
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "obj" && a.member == "nested"),
        "obj.nested should be tracked as a member access"
    );
    // obj should NOT be in whole_object_uses (it's a specific member access)
    assert!(
        !info.whole_object_uses.contains(&"obj".to_string()),
        "nested member access should not mark obj as whole-object-use"
    );
}

// ── Export extraction ────────────────────────────────────────

#[test]
fn export_default_class_declaration() {
    let info = parse("export default class Foo { bar() {} }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_default_anonymous_class() {
    let info = parse("export default class {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_default_expression() {
    let info = parse("export default 42;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_default_arrow_function() {
    let info = parse("export default () => {};");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Default);
}

#[test]
fn export_const_multiple_declarators() {
    let info = parse("export const a = 1, b = 2, c = 3;");
    assert_eq!(info.exports.len(), 3);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "a"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "b"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "c"))
    );
}

#[test]
fn export_let_declaration() {
    let info = parse("export let mutable = 'hello';");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("mutable".to_string())
    );
    assert!(!info.exports[0].is_type_only);
}

#[test]
fn export_destructured_object() {
    let info = parse("export const { a, b } = { a: 1, b: 2 };");
    assert_eq!(info.exports.len(), 2);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "a"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "b"))
    );
}

#[test]
fn export_destructured_with_default_value() {
    let info = parse("export const { x = 10, y } = obj;");
    assert_eq!(info.exports.len(), 2);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "x"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "y"))
    );
}

#[test]
fn export_destructured_array() {
    let info = parse("export const [first, , third] = [1, 2, 3];");
    assert_eq!(info.exports.len(), 2);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "first"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "third"))
    );
}

#[test]
fn export_specifier_with_alias() {
    let info = parse("const x = 1;\nexport { x as myAlias };");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("myAlias".to_string())
    );
    assert_eq!(info.exports[0].local_name, Some("x".to_string()));
}

#[test]
fn export_specifier_list_multiple() {
    let info = parse("const a = 1; const b = 2; const c = 3;\nexport { a, b, c };");
    assert_eq!(info.exports.len(), 3);
}

#[test]
fn export_async_function() {
    let info = parse("export async function fetchData() {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("fetchData".to_string())
    );
}

#[test]
fn export_generator_function() {
    let info = parse("export function* gen() { yield 1; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("gen".to_string()));
}

// ── Type exports ─────────────────────────────────────────────

#[test]
fn export_type_alias() {
    let info = parse("export type ID = string | number;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("ID".to_string()));
    assert!(info.exports[0].is_type_only);
}

#[test]
fn export_interface() {
    let info = parse("export interface Props { name: string; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Props".to_string()));
    assert!(info.exports[0].is_type_only);
}

#[test]
fn export_type_specifier_on_individual_spec() {
    let info = parse("const a = 1; type B = string;\nexport { a, type B };");
    assert_eq!(info.exports.len(), 2);
    let a_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "a"))
        .unwrap();
    let b_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "B"))
        .unwrap();
    assert!(!a_export.is_type_only);
    assert!(b_export.is_type_only);
}

#[test]
fn export_declare_module() {
    let info = parse("export declare module 'my-module' {}");
    assert_eq!(info.exports.len(), 1);
    assert!(info.exports[0].is_type_only);
}

#[test]
fn export_declare_namespace() {
    let info = parse("export declare namespace MyNS {}");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("MyNS".to_string()));
    assert!(info.exports[0].is_type_only);
}

// ── Re-export extraction ─────────────────────────────────────

#[test]
fn re_export_named() {
    let info = parse("export { foo } from './bar';");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.re_exports[0].imported_name, "foo");
    assert_eq!(info.re_exports[0].exported_name, "foo");
    assert_eq!(info.re_exports[0].source, "./bar");
}

#[test]
fn re_export_with_rename() {
    let info = parse("export { foo as bar } from './baz';");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.re_exports[0].imported_name, "foo");
    assert_eq!(info.re_exports[0].exported_name, "bar");
}

#[test]
fn re_export_multiple() {
    let info = parse("export { a, b, c } from './mod';");
    assert_eq!(info.re_exports.len(), 3);
}

#[test]
fn re_export_star() {
    let info = parse("export * from './all';");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.re_exports[0].imported_name, "*");
    assert_eq!(info.re_exports[0].exported_name, "*");
    assert!(!info.re_exports[0].is_type_only);
}

#[test]
fn re_export_star_as_namespace() {
    let info = parse("export * as ns from './all';");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.re_exports[0].imported_name, "*");
    assert_eq!(info.re_exports[0].exported_name, "ns");
}

#[test]
fn re_export_type_only() {
    let info = parse("export type { Foo, Bar } from './types';");
    assert_eq!(info.re_exports.len(), 2);
    assert!(info.re_exports[0].is_type_only);
    assert!(info.re_exports[1].is_type_only);
}

#[test]
fn re_export_type_on_individual_specifier() {
    let info = parse("export { type Foo, bar } from './mod';");
    assert_eq!(info.re_exports.len(), 2);
    let foo_re = info
        .re_exports
        .iter()
        .find(|r| r.exported_name == "Foo")
        .unwrap();
    let bar_re = info
        .re_exports
        .iter()
        .find(|r| r.exported_name == "bar")
        .unwrap();
    assert!(foo_re.is_type_only);
    assert!(!bar_re.is_type_only);
}

#[test]
fn re_export_star_type_only() {
    let info = parse("export type * from './types';");
    assert_eq!(info.re_exports.len(), 1);
    assert!(info.re_exports[0].is_type_only);
    assert_eq!(info.re_exports[0].imported_name, "*");
}

// ── Import extraction ────────────────────────────────────────

#[test]
fn import_named_single() {
    let info = parse("import { foo } from './bar';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(
        info.imports[0].imported_name,
        ImportedName::Named("foo".to_string())
    );
    assert_eq!(info.imports[0].local_name, "foo");
    assert_eq!(info.imports[0].source, "./bar");
}

#[test]
fn import_named_multiple() {
    let info = parse("import { a, b, c } from './mod';");
    assert_eq!(info.imports.len(), 3);
}

#[test]
fn import_default() {
    let info = parse("import React from 'react';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::Default);
    assert_eq!(info.imports[0].local_name, "React");
}

#[test]
fn import_namespace() {
    let info = parse("import * as utils from './utils';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::Namespace);
    assert_eq!(info.imports[0].local_name, "utils");
}

#[test]
fn import_side_effect() {
    let info = parse("import './styles.css';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.imports[0].imported_name, ImportedName::SideEffect);
    assert!(info.imports[0].local_name.is_empty());
}

#[test]
fn import_with_alias() {
    let info = parse("import { foo as bar } from './mod';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(
        info.imports[0].imported_name,
        ImportedName::Named("foo".to_string())
    );
    assert_eq!(info.imports[0].local_name, "bar");
}

#[test]
fn import_default_and_named() {
    let info = parse("import React, { useState, useEffect } from 'react';");
    assert_eq!(info.imports.len(), 3);
    assert_eq!(info.imports[0].imported_name, ImportedName::Default);
    assert_eq!(
        info.imports[1].imported_name,
        ImportedName::Named("useState".to_string())
    );
    assert_eq!(
        info.imports[2].imported_name,
        ImportedName::Named("useEffect".to_string())
    );
}

#[test]
fn import_default_and_namespace() {
    let info = parse("import def, * as ns from './mod';");
    assert_eq!(info.imports.len(), 2);
    assert_eq!(info.imports[0].imported_name, ImportedName::Default);
    assert_eq!(info.imports[1].imported_name, ImportedName::Namespace);
}

#[test]
fn import_type_only_declaration() {
    let info = parse("import type { Foo } from './types';");
    assert_eq!(info.imports.len(), 1);
    assert!(info.imports[0].is_type_only);
    assert_eq!(
        info.imports[0].imported_name,
        ImportedName::Named("Foo".to_string())
    );
}

#[test]
fn import_type_on_individual_specifier() {
    let info = parse("import { type Foo, Bar } from './types';");
    assert_eq!(info.imports.len(), 2);
    let foo_imp = info.imports.iter().find(|i| i.local_name == "Foo").unwrap();
    let bar_imp = info.imports.iter().find(|i| i.local_name == "Bar").unwrap();
    assert!(foo_imp.is_type_only);
    assert!(!bar_imp.is_type_only);
}

#[test]
fn import_type_namespace() {
    let info = parse("import type * as Types from './types';");
    assert_eq!(info.imports.len(), 1);
    assert!(info.imports[0].is_type_only);
    assert_eq!(info.imports[0].imported_name, ImportedName::Namespace);
}

#[test]
fn import_type_default() {
    let info = parse("import type React from 'react';");
    assert_eq!(info.imports.len(), 1);
    assert!(info.imports[0].is_type_only);
    assert_eq!(info.imports[0].imported_name, ImportedName::Default);
}

#[test]
fn import_source_span_populated() {
    let info = parse("import { foo } from './bar';");
    assert_eq!(info.imports.len(), 1);
    // source_span should cover the string literal './bar'
    assert!(info.imports[0].source_span.start < info.imports[0].source_span.end);
}

// ── Dynamic import extraction ────────────────────────────────

#[test]
fn dynamic_import_string_literal() {
    let info = parse("import('./lazy');");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./lazy");
    assert!(info.dynamic_imports[0].local_name.is_none());
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
}

#[test]
fn dynamic_import_assigned_to_variable() {
    let info = parse("const mod = import('./lazy');");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./lazy");
    assert_eq!(info.dynamic_imports[0].local_name, Some("mod".to_string()));
}

#[test]
fn dynamic_import_await() {
    let info = parse("async function f() { const mod = await import('./lazy'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./lazy");
    assert_eq!(info.dynamic_imports[0].local_name, Some("mod".to_string()));
}

#[test]
fn dynamic_import_destructured() {
    let info = parse("async function f() { const { a, b } = await import('./mod'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert!(info.dynamic_imports[0].local_name.is_none());
    assert_eq!(info.dynamic_imports[0].destructured_names, vec!["a", "b"]);
}

#[test]
fn dynamic_import_destructured_with_rest_clears_names() {
    let info = parse("async function f() { const { a, ...rest } = await import('./mod'); }");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert!(info.dynamic_imports[0].destructured_names.is_empty());
}

#[test]
fn dynamic_import_variable_source_ignored() {
    let info = parse("import(variable);");
    assert!(info.dynamic_imports.is_empty());
    assert!(info.dynamic_import_patterns.is_empty());
}

#[test]
fn dynamic_import_template_literal_exact() {
    let info = parse("import(`./exact`);");
    assert_eq!(info.dynamic_imports.len(), 1);
    assert_eq!(info.dynamic_imports[0].source, "./exact");
}

#[test]
fn dynamic_import_template_literal_with_expression() {
    let info = parse("import(`./locales/${lang}.json`);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./locales/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".json".to_string())
    );
}

#[test]
fn dynamic_import_template_multi_expression_globstar() {
    let info = parse("import(`./plugins/${cat}/${name}.js`);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./plugins/**/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".js".to_string())
    );
}

#[test]
fn dynamic_import_concat_prefix_only() {
    let info = parse("import('./pages/' + name);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./pages/");
    assert!(info.dynamic_import_patterns[0].suffix.is_none());
}

#[test]
fn dynamic_import_concat_with_suffix() {
    let info = parse("import('./pages/' + name + '.tsx');");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./pages/");
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".tsx".to_string())
    );
}

#[test]
fn dynamic_import_non_relative_template_ignored() {
    let info = parse("import(`lodash/${fn}`);");
    assert!(info.dynamic_import_patterns.is_empty());
}

#[test]
fn dynamic_import_non_relative_concat_ignored() {
    let info = parse("import('lodash/' + fn);");
    assert!(info.dynamic_import_patterns.is_empty());
}

#[test]
fn dynamic_import_no_duplicate_when_assigned() {
    // When assigned to a variable, the import should appear exactly once
    let info = parse("async function f() { const m = await import('./svc'); }");
    assert_eq!(
        info.dynamic_imports.len(),
        1,
        "assigned dynamic import should not produce duplicate entries"
    );
}

// ── Require call extraction ──────────────────────────────────

#[test]
fn require_call_simple() {
    let info = parse("const fs = require('fs');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "fs");
    assert_eq!(info.require_calls[0].local_name, Some("fs".to_string()));
}

#[test]
fn require_call_destructured() {
    let info = parse("const { readFile, writeFile } = require('fs');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "fs");
    assert!(info.require_calls[0].local_name.is_none());
    assert_eq!(
        info.require_calls[0].destructured_names,
        vec!["readFile", "writeFile"]
    );
}

#[test]
fn require_call_bare_in_expression() {
    let info = parse("doSomething(require('foo'));");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].source, "foo");
    assert!(info.require_calls[0].local_name.is_none());
}

#[test]
fn require_call_variable_arg_ignored() {
    let info = parse("const x = require(someVar);");
    assert!(info.require_calls.is_empty());
}

#[test]
fn require_call_template_literal_arg_ignored() {
    let info = parse("const x = require(`./mod`);");
    assert!(info.require_calls.is_empty());
}

#[test]
fn require_multiple_calls() {
    let info = parse("const a = require('a'); const b = require('b');");
    assert_eq!(info.require_calls.len(), 2);
}

#[test]
fn require_destructured_with_alias() {
    let info = parse("const { foo: localFoo } = require('./mod');");
    assert_eq!(info.require_calls.len(), 1);
    assert_eq!(info.require_calls[0].destructured_names, vec!["foo"]);
}

#[test]
fn require_destructured_with_rest_returns_empty() {
    let info = parse("const { a, ...rest } = require('./mod');");
    assert_eq!(info.require_calls.len(), 1);
    assert!(info.require_calls[0].destructured_names.is_empty());
}

// ── Member access extraction ─────────────────────────────────

#[test]
fn member_access_static() {
    let info = parse("import { Status } from './types';\nStatus.Active;");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Status" && a.member == "Active"),
        "should track Status.Active"
    );
}

#[test]
fn member_access_method_call() {
    let info = parse("import { MyClass } from './mod';\nMyClass.create();");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyClass" && a.member == "create"),
        "should track MyClass.create"
    );
}

#[test]
fn member_access_computed_string_literal() {
    let info = parse("import { Status } from './types';\nStatus['Active'];");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Status" && a.member == "Active"),
        "computed access with string literal should resolve to member"
    );
}

#[test]
fn member_access_computed_dynamic_marks_whole() {
    let info = parse("import { Status } from './types';\nconst k = 'x';\nStatus[k];");
    assert!(
        info.whole_object_uses.contains(&"Status".to_string()),
        "dynamic computed access should mark as whole-object use"
    );
}

#[test]
fn member_access_this_read() {
    let info = parse(
        r"
        export class Foo {
            x: number;
            getX() { return this.x; }
        }
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "this" && a.member == "x"),
        "this.x read should be tracked"
    );
}

#[test]
fn member_access_this_write() {
    let info = parse(
        r"
        export class Foo {
            x: number;
            setX() { this.x = 5; }
        }
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "this" && a.member == "x"),
        "this.x = ... should be tracked"
    );
}

#[test]
fn member_access_chained() {
    let info = parse("import { obj } from './data';\nobj.a.b.c;");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "obj" && a.member == "a"),
        "first level of chained access should be tracked"
    );
}

// ── Whole-object use patterns ────────────────────────────────

#[test]
fn whole_object_object_values() {
    let info = parse("Object.values(myObj);");
    assert!(info.whole_object_uses.contains(&"myObj".to_string()));
}

#[test]
fn whole_object_object_keys() {
    let info = parse("Object.keys(myObj);");
    assert!(info.whole_object_uses.contains(&"myObj".to_string()));
}

#[test]
fn whole_object_object_entries() {
    let info = parse("Object.entries(myObj);");
    assert!(info.whole_object_uses.contains(&"myObj".to_string()));
}

#[test]
fn whole_object_get_own_property_names() {
    let info = parse("Object.getOwnPropertyNames(myObj);");
    assert!(info.whole_object_uses.contains(&"myObj".to_string()));
}

#[test]
fn whole_object_spread() {
    let info = parse("const copy = { ...myObj };");
    assert!(info.whole_object_uses.contains(&"myObj".to_string()));
}

#[test]
fn whole_object_for_in() {
    let info = parse("for (const k in myObj) {}");
    assert!(info.whole_object_uses.contains(&"myObj".to_string()));
}

#[test]
fn whole_object_spread_in_array() {
    let info = parse("const arr = [...myArr];");
    assert!(info.whole_object_uses.contains(&"myArr".to_string()));
}

#[test]
fn whole_object_spread_in_call_args() {
    let info = parse("fn(...myArr);");
    assert!(info.whole_object_uses.contains(&"myArr".to_string()));
}

// ── Type-level member access ────────────────────────────────

#[test]
fn type_qualified_name_tracks_member_access() {
    let info = parse("import { Status } from './types';\ntype X = Status.Active;");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Status" && a.member == "Active"),
        "Enum.Member in type position should be tracked as member access"
    );
}

#[test]
fn mapped_type_constraint_marks_whole_object_use() {
    let info = parse(
        "import { BreakpointString } from './types';\ntype X = { [K in BreakpointString]: string };",
    );
    assert!(
        info.whole_object_uses
            .contains(&"BreakpointString".to_string()),
        "enum used as mapped type constraint should be marked as whole-object use"
    );
}

#[test]
fn mapped_type_with_optional_marks_whole_object_use() {
    let info = parse("import { Dir } from './types';\ntype X = { [K in Dir]?: number };");
    assert!(
        info.whole_object_uses.contains(&"Dir".to_string()),
        "enum in optional mapped type should be whole-object use"
    );
}

#[test]
fn mapped_type_keyof_typeof_marks_whole_object_use() {
    let info =
        parse("import { Dir } from './types';\ntype X = { [K in keyof typeof Dir]: string };");
    assert!(
        info.whole_object_uses.contains(&"Dir".to_string()),
        "keyof typeof in mapped type constraint should be whole-object use"
    );
}

#[test]
fn record_utility_type_marks_whole_object_use() {
    let info = parse("import { Status } from './types';\ntype X = Record<Status, string>;");
    assert!(
        info.whole_object_uses.contains(&"Status".to_string()),
        "Record<Enum, T> should mark enum as whole-object use"
    );
}

#[test]
fn partial_record_marks_whole_object_use() {
    let info =
        parse("import { Status } from './types';\ntype X = Partial<Record<Status, number>>;");
    assert!(
        info.whole_object_uses.contains(&"Status".to_string()),
        "Partial<Record<Enum, T>> should mark enum as whole-object use (nested walk)"
    );
}

#[test]
fn record_with_aliased_import_marks_whole_object_use() {
    let info = parse("import { Status as S } from './types';\ntype X = Record<S, string>;");
    assert!(
        info.whole_object_uses.contains(&"S".to_string()),
        "Record<AliasedEnum, T> should emit the local alias name"
    );
}

#[test]
fn record_with_non_identifier_key_no_whole_object_use() {
    let info = parse("type X = Record<string, number>;");
    assert!(
        info.whole_object_uses.is_empty(),
        "Record<string, T> should not produce whole-object use"
    );
}

// ── CommonJS exports ─────────────────────────────────────────

#[test]
fn cjs_module_exports_object_keys() {
    let info = parse("module.exports = { foo: 1, bar: 2, baz: 3 };");
    assert!(info.has_cjs_exports);
    assert_eq!(info.exports.len(), 3);
}

#[test]
fn cjs_exports_dot_property() {
    let info = parse("exports.myFunc = function() {};");
    assert!(info.has_cjs_exports);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "myFunc"))
    );
}

#[test]
fn cjs_module_exports_non_object() {
    let info = parse("module.exports = someValue;");
    assert!(info.has_cjs_exports);
    // Non-object RHS doesn't produce named exports
    assert!(info.exports.is_empty());
}

#[test]
fn cjs_both_patterns() {
    let info = parse("module.exports = { a: 1 };\nexports.b = 2;");
    assert!(info.has_cjs_exports);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "a"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "b"))
    );
}

#[test]
fn cjs_module_exports_dot_property() {
    let info = parse(
        "module.exports.foo = function() {};\nmodule.exports.bar = 42;\nmodule.exports.baz = class {};",
    );
    assert!(info.has_cjs_exports);
    assert_eq!(info.exports.len(), 3);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "foo"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "bar"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "baz"))
    );
}

// ── TypeScript enum extraction ───────────────────────────────

#[test]
fn ts_enum_members_extracted() {
    let info = parse("export enum Color { Red, Green, Blue }");
    assert_eq!(info.exports.len(), 1);
    let members = &info.exports[0].members;
    assert_eq!(members.len(), 3);
    assert!(members.iter().all(|m| m.kind == MemberKind::EnumMember));
    assert!(members.iter().any(|m| m.name == "Red"));
    assert!(members.iter().any(|m| m.name == "Green"));
    assert!(members.iter().any(|m| m.name == "Blue"));
}

#[test]
fn ts_enum_with_string_values() {
    let info = parse(r#"export enum Status { Active = "active", Inactive = "inactive" }"#);
    assert_eq!(info.exports.len(), 1);
    let members = &info.exports[0].members;
    assert_eq!(members.len(), 2);
    assert!(members.iter().any(|m| m.name == "Active"));
    assert!(members.iter().any(|m| m.name == "Inactive"));
}

#[test]
fn ts_enum_with_numeric_values() {
    let info = parse("export enum Dir { Up = 0, Down = 1, Left = 2, Right = 3 }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].members.len(), 4);
}

#[test]
fn ts_const_enum() {
    let info = parse("export const enum Flags { A, B, C }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].members.len(), 3);
}

#[test]
fn ts_enum_string_member_name() {
    let info = parse(r#"export enum E { "some-key" = 1 }"#);
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].members.len(), 1);
    assert_eq!(info.exports[0].members[0].name, "some-key");
}

// ── Class member extraction ──────────────────────────────────

#[test]
fn class_public_methods_and_properties() {
    let info = parse(
        r"
        export class Svc {
            name: string;
            greet() {}
            static create() {}
        }
        ",
    );
    let class_export = info
        .exports
        .iter()
        .find(|e| matches!(&e.name, ExportName::Named(n) if n == "Svc"))
        .unwrap();
    assert!(
        class_export
            .members
            .iter()
            .any(|m| m.name == "name" && m.kind == MemberKind::ClassProperty)
    );
    assert!(
        class_export
            .members
            .iter()
            .any(|m| m.name == "greet" && m.kind == MemberKind::ClassMethod)
    );
    assert!(
        class_export
            .members
            .iter()
            .any(|m| m.name == "create" && m.kind == MemberKind::ClassMethod)
    );
}

#[test]
fn class_skips_constructor() {
    let info = parse("export class Foo { constructor() {} }");
    let members = &info.exports[0].members;
    assert!(!members.iter().any(|m| m.name == "constructor"));
}

#[test]
fn class_skips_private_members() {
    let info = parse(
        r"
        export class Foo {
            private secret: string;
            public visible: number;
        }
        ",
    );
    let members = &info.exports[0].members;
    assert!(!members.iter().any(|m| m.name == "secret"));
    assert!(members.iter().any(|m| m.name == "visible"));
}

#[test]
fn class_skips_protected_members() {
    let info = parse(
        r"
        export class Foo {
            protected internal(): void {}
            open(): void {}
        }
        ",
    );
    let members = &info.exports[0].members;
    assert!(!members.iter().any(|m| m.name == "internal"));
    assert!(members.iter().any(|m| m.name == "open"));
}

#[test]
fn class_member_decorator_tracked() {
    let info = parse(
        r"
        function Dec() { return (t: any) => t; }
        export class Svc {
            @Dec()
            handler() {}
            plain() {}
        }
        ",
    );
    let members = &info.exports[0].members;
    let handler = members.iter().find(|m| m.name == "handler").unwrap();
    let plain = members.iter().find(|m| m.name == "plain").unwrap();
    assert!(handler.has_decorator);
    assert!(!plain.has_decorator);
}

// ── Instance member access mapping ───────────────────────────

#[test]
fn instance_method_call_mapped() {
    let info = parse(
        r"
        import { MyService } from './svc';
        const svc = new MyService();
        svc.hello();
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "MyService" && a.member == "hello")
    );
}

#[test]
fn instance_property_mapped() {
    let info = parse(
        r"
        import { Config } from './config';
        const cfg = new Config();
        console.log(cfg.port);
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Config" && a.member == "port")
    );
}

#[test]
fn builtin_constructor_instance_not_mapped() {
    let info = parse(
        r"
        const m = new Map();
        m.set('key', 'value');
        ",
    );
    assert!(
        !info.member_accesses.iter().any(|a| a.object == "Map"),
        "built-in Map should not produce instance mapping"
    );
}

#[test]
fn instance_whole_object_mapped() {
    let info = parse(
        r"
        import { MyClass } from './cls';
        const obj = new MyClass();
        Object.keys(obj);
        ",
    );
    assert!(info.whole_object_uses.contains(&"MyClass".to_string()));
}

#[test]
fn multiple_instances_same_class_mapped() {
    let info = parse(
        r"
        import { Svc } from './svc';
        const a = new Svc();
        const b = new Svc();
        a.foo();
        b.bar();
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Svc" && a.member == "foo")
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "Svc" && a.member == "bar")
    );
}

// ── Namespace destructuring ──────────────────────────────────

#[test]
fn namespace_import_destructuring() {
    let info = parse("import * as ns from './mod';\nconst { a, b } = ns;");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "ns" && a.member == "a")
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "ns" && a.member == "b")
    );
}

#[test]
fn namespace_import_destructuring_with_rest_marks_whole() {
    let info = parse("import * as ns from './mod';\nconst { a, ...rest } = ns;");
    assert!(info.whole_object_uses.contains(&"ns".to_string()));
}

#[test]
fn require_namespace_destructuring() {
    let info = parse("const mod = require('./mod');\nconst { x, y } = mod;");
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "mod" && a.member == "x")
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "mod" && a.member == "y")
    );
}

#[test]
fn dynamic_import_namespace_destructuring() {
    let info = parse(
        r"
        async function f() {
            const mod = await import('./mod');
            const { foo, bar } = mod;
        }
        ",
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "mod" && a.member == "foo")
    );
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "mod" && a.member == "bar")
    );
}

#[test]
fn non_namespace_destructuring_not_tracked() {
    let info = parse("const obj = { a: 1 };\nconst { a } = obj;");
    assert!(
        !info
            .member_accesses
            .iter()
            .any(|a| a.object == "obj" && a.member == "a"),
        "destructuring of non-namespace vars should not produce member accesses"
    );
}

// ── new URL pattern ──────────────────────────────────────────

#[test]
fn new_url_import_meta_url_tracked() {
    let info = parse("new URL('./worker.js', import.meta.url);");
    assert!(
        info.dynamic_imports
            .iter()
            .any(|d| d.source == "./worker.js")
    );
}

#[test]
fn new_url_non_relative_not_tracked() {
    let info = parse("new URL('https://example.com', import.meta.url);");
    assert!(info.dynamic_imports.is_empty());
}

#[test]
fn new_url_without_import_meta_url_not_tracked() {
    let info = parse("new URL('./worker.js', baseUrl);");
    assert!(info.dynamic_imports.is_empty());
}

// ── import.meta.glob ─────────────────────────────────────────

#[test]
fn import_meta_glob_string() {
    let info = parse("import.meta.glob('./components/*.tsx');");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./components/*.tsx");
}

#[test]
fn import_meta_glob_array() {
    let info = parse("import.meta.glob(['./a/*.ts', './b/*.ts']);");
    assert_eq!(info.dynamic_import_patterns.len(), 2);
}

#[test]
fn import_meta_glob_non_relative_ignored() {
    let info = parse("import.meta.glob('node_modules/**/*.js');");
    assert!(info.dynamic_import_patterns.is_empty());
}

// ── require.context ──────────────────────────────────────────

#[test]
fn require_context_non_recursive_prefix() {
    let info = parse("require.context('./icons', false);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./icons/");
}

#[test]
fn require_context_recursive_prefix() {
    let info = parse("require.context('./icons', true);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(info.dynamic_import_patterns[0].prefix, "./icons/**/");
}

#[test]
fn require_context_with_regex_suffix() {
    let info = parse(r"require.context('./src', true, /\.vue$/);");
    assert_eq!(info.dynamic_import_patterns.len(), 1);
    assert_eq!(
        info.dynamic_import_patterns[0].suffix,
        Some(".vue".to_string())
    );
}

#[test]
fn require_context_non_relative_ignored() {
    let info = parse("require.context('node_modules', false);");
    assert!(info.dynamic_import_patterns.is_empty());
}

// ── Function overload deduplication ──────────────────────────

#[test]
fn function_overloads_produce_single_export() {
    let info = parse(
        r"
        export function parse(): void;
        export function parse(input: string): void;
        export function parse(input?: string): void {}
        ",
    );
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("parse".to_string()));
}

// ── Edge cases ───────────────────────────────────────────────

#[test]
fn empty_source_produces_no_results() {
    let info = parse("");
    assert!(info.exports.is_empty());
    assert!(info.imports.is_empty());
    assert!(info.re_exports.is_empty());
    assert!(info.dynamic_imports.is_empty());
    assert!(info.require_calls.is_empty());
    assert!(!info.has_cjs_exports);
}

#[test]
fn no_module_syntax_produces_no_results() {
    let info = parse("const x = 1;\nconsole.log(x);");
    assert!(info.exports.is_empty());
    assert!(info.imports.is_empty());
    assert!(info.re_exports.is_empty());
    assert!(!info.has_cjs_exports);
}

#[test]
fn namespace_import_adds_to_namespace_bindings() {
    let info = parse("import * as ns from './mod';\nns.foo();");
    // ns should be tracked as a namespace binding and ns.foo as a member access
    assert!(
        info.member_accesses
            .iter()
            .any(|a| a.object == "ns" && a.member == "foo")
    );
}

#[test]
fn export_abstract_class() {
    let info = parse("export abstract class Base { abstract doWork(): void; }");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("Base".to_string()));
}

#[test]
fn export_enum_not_type_only() {
    let info = parse("export enum Dir { Up, Down }");
    assert_eq!(info.exports.len(), 1);
    assert!(!info.exports[0].is_type_only);
}

#[test]
fn mixed_esm_and_cjs_in_same_file() {
    let info =
        parse("import { foo } from './bar';\nexport const x = 1;\nmodule.exports = { y: 2 };");
    assert_eq!(info.imports.len(), 1);
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "x"))
    );
    assert!(
        info.exports
            .iter()
            .any(|e| matches!(&e.name, ExportName::Named(n) if n == "y"))
    );
    assert!(info.has_cjs_exports);
}

#[test]
fn export_with_satisfies() {
    let info = parse("export const config = { port: 3000 } satisfies Config;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("config".to_string())
    );
}

#[test]
fn export_with_as_const() {
    let info = parse("export const COLORS = ['red', 'blue'] as const;");
    assert_eq!(info.exports.len(), 1);
    assert_eq!(
        info.exports[0].name,
        ExportName::Named("COLORS".to_string())
    );
}

#[test]
fn import_and_re_export_same_source() {
    let info = parse("import { foo } from './mod';\nexport { bar } from './mod';");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.imports[0].source, "./mod");
    assert_eq!(info.re_exports[0].source, "./mod");
}

// ── "Import then re-export" pattern detection ────────────────
// Pattern: `import { X } from './a'; export { X };` is semantically
// equivalent to `export { X } from './a';` and must be treated as a
// re-export, not a local export. Without this, duplicate-export
// detection fires and the re-export chain breaks.

#[test]
fn import_then_export_same_name_is_re_export() {
    let info = parse("import { Foo } from './types';\nexport { Foo };");
    assert_eq!(info.imports.len(), 1);
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.exports.len(), 0);
    assert_eq!(info.re_exports[0].source, "./types");
    assert_eq!(info.re_exports[0].imported_name, "Foo");
    assert_eq!(info.re_exports[0].exported_name, "Foo");
    assert!(!info.re_exports[0].is_type_only);
}

#[test]
fn import_type_then_export_type_is_type_only_re_export() {
    let info = parse("import type { Foo } from './types';\nexport type { Foo };");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.exports.len(), 0);
    assert!(info.re_exports[0].is_type_only);
}

#[test]
fn value_import_then_type_export_is_type_only_re_export() {
    // `import { MyEnum } from './a'; export type { MyEnum };`
    // The `type` keyword on the export side erases the value reference,
    // making this a type-only re-export even though the import is a value.
    let info = parse("import { MyEnum } from './a';\nexport type { MyEnum };");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.exports.len(), 0);
    assert!(info.re_exports[0].is_type_only);
}

#[test]
fn import_with_rename_then_export_is_re_export_with_original_name() {
    // `import { X as Foo } from './a'; export { Foo };`
    // The re-export must reference the ORIGINAL name on the remote side
    // (`X`) even though the local binding is `Foo`.
    let info = parse("import { X as Foo } from './a';\nexport { Foo };");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.exports.len(), 0);
    assert_eq!(info.re_exports[0].imported_name, "X");
    assert_eq!(info.re_exports[0].exported_name, "Foo");
}

#[test]
fn import_then_export_with_rename_is_re_export_with_alias() {
    // `import { X } from './a'; export { X as Y };`
    // Re-export should carry both the original name and the alias.
    let info = parse("import { X } from './a';\nexport { X as Y };");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.exports.len(), 0);
    assert_eq!(info.re_exports[0].imported_name, "X");
    assert_eq!(info.re_exports[0].exported_name, "Y");
}

#[test]
fn default_import_then_export_is_re_export_of_default() {
    // `import D from './a'; export { D };` re-exports the remote default.
    let info = parse("import D from './a';\nexport { D };");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.exports.len(), 0);
    assert_eq!(info.re_exports[0].source, "./a");
    assert_eq!(info.re_exports[0].imported_name, "default");
    assert_eq!(info.re_exports[0].exported_name, "D");
}

#[test]
fn mixed_export_splits_into_local_and_re_export() {
    // `import { X } from './a'; const Y = 1; export { X, Y };`
    // X matches an import → re-export. Y is local → regular export.
    let info = parse("import { X } from './a';\nconst Y = 1;\nexport { X, Y };");
    assert_eq!(info.re_exports.len(), 1);
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.re_exports[0].imported_name, "X");
    assert_eq!(info.exports[0].name, ExportName::Named("Y".to_string()));
}

#[test]
fn local_export_without_matching_import_stays_local() {
    // Regression guard: unrelated local exports must not be converted.
    let info = parse("const X = 1;\nexport { X };");
    assert_eq!(info.re_exports.len(), 0);
    assert_eq!(info.exports.len(), 1);
    assert_eq!(info.exports[0].name, ExportName::Named("X".to_string()));
}

#[test]
fn namespace_import_then_export_stays_local() {
    // `import * as ns from './a'; export { ns };` — namespace re-exports
    // are an edge case and not converted by this heuristic.
    let info = parse("import * as ns from './a';\nexport { ns };");
    assert_eq!(info.re_exports.len(), 0);
    assert_eq!(info.exports.len(), 1);
}

mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arb_valid_js_source() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("const x = 1;".to_string()),
            Just("export const a = 42;".to_string()),
            Just("import { foo } from './bar';".to_string()),
            Just("export default function() {}".to_string()),
            Just("export { x } from './mod';".to_string()),
            Just("const y = require('./util');".to_string()),
            Just("export class Foo {}".to_string()),
            Just("export type T = string;".to_string()),
            Just("export interface I { x: number; }".to_string()),
            Just("import * as ns from './all';".to_string()),
            Just("export * from './barrel';".to_string()),
            "[a-zA-Z_][a-zA-Z0-9_]{0,20}".prop_map(|id| format!("export const {id} = 1;")),
            "[a-zA-Z_][a-zA-Z0-9_]{0,20}".prop_map(|id| format!("import {{ {id} }} from './mod';")),
        ]
    }

    proptest! {
        /// Parsing any valid JS/TS source never panics.
        #[test]
        fn parse_never_panics(source in "[a-zA-Z0-9 (){};=+\\-*/'\",.<>:\\n!?@#$%^&|~`_]{0,200}") {
            let _ = parse(&source);
        }

        /// Star re-export sources should go into re_exports, not exports.
        #[test]
        fn star_reexport_does_not_pollute_exports(
            mod_name in "[a-z]{1,10}",
        ) {
            let source = format!("export * from './{mod_name}';");
            let info = parse(&source);
            // Star re-exports should be in re_exports, not exports
            prop_assert!(
                !info.re_exports.is_empty(),
                "Star re-export should produce a re_export entry"
            );
            // The export list should not contain the re-exported names
            for exp in &info.exports {
                if let ExportName::Named(name) = &exp.name {
                    prop_assert_ne!(name, "*", "Star re-export should not appear in exports");
                }
            }
        }

        /// All export names should be non-empty strings (for Named variants).
        #[test]
        fn export_names_are_non_empty(source in arb_valid_js_source()) {
            let info = parse(&source);
            for export in &info.exports {
                if let ExportName::Named(name) = &export.name {
                    prop_assert!(!name.is_empty(), "Named export should have non-empty name");
                }
            }
        }

        /// All import sources should be non-empty strings.
        #[test]
        fn import_sources_are_non_empty(source in arb_valid_js_source()) {
            let info = parse(&source);
            for import in &info.imports {
                prop_assert!(!import.source.is_empty(), "Import source should be non-empty");
            }
            for re_export in &info.re_exports {
                prop_assert!(!re_export.source.is_empty(), "Re-export source should be non-empty");
            }
        }
    }
}

// ── Angular @Component decorator detection ───────────────────

#[test]
fn angular_component_template_url_emits_side_effect_import() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            templateUrl: './app.html',
        })
        export class App {}
        ",
    );
    let side_effect_count = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .count();
    assert_eq!(side_effect_count, 1);
    assert_eq!(
        info.imports
            .iter()
            .find(|i| matches!(i.imported_name, ImportedName::SideEffect))
            .unwrap()
            .source,
        "./app.html"
    );
}

#[test]
fn angular_component_style_url_emits_side_effect_import() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            templateUrl: './app.html',
            styleUrl: './app.scss',
        })
        export class App {}
        ",
    );
    let side_effect_count = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .count();
    assert_eq!(side_effect_count, 2);
    let has_html = info
        .imports
        .iter()
        .any(|i| i.source == "./app.html" && matches!(i.imported_name, ImportedName::SideEffect));
    let has_scss = info
        .imports
        .iter()
        .any(|i| i.source == "./app.scss" && matches!(i.imported_name, ImportedName::SideEffect));
    assert!(has_html);
    assert!(has_scss);
}

#[test]
fn angular_component_style_urls_array_emits_multiple_imports() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            templateUrl: './app.html',
            styleUrls: ['./app.scss', './theme.scss'],
        })
        export class App {}
        ",
    );
    let side_effect_count = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .count();
    assert_eq!(side_effect_count, 3);
}

#[test]
fn angular_component_template_url_without_dot_slash_normalized() {
    // Angular resolves `'app.component.html'` the same as `'./app.component.html'`.
    // Fallow must normalize the bare form so downstream resolution doesn't
    // misclassify it as an unlisted npm package. Regression test for #99.
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            templateUrl: 'app.component.html',
        })
        export class AppComponent {}
        ",
    );
    let template_import = info
        .imports
        .iter()
        .find(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .unwrap();
    assert_eq!(template_import.source, "./app.component.html");
}

#[test]
fn angular_component_style_url_without_dot_slash_normalized() {
    // Regression test for #99: bare `styleUrl` values must be normalized.
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            templateUrl: 'app.component.html',
            styleUrl: 'app.component.scss',
        })
        export class AppComponent {}
        ",
    );
    let sources: Vec<&str> = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .map(|i| i.source.as_str())
        .collect();
    assert!(sources.contains(&"./app.component.html"));
    assert!(sources.contains(&"./app.component.scss"));
}

#[test]
fn angular_component_style_urls_array_without_dot_slash_normalized() {
    // Regression test for #99: bare entries in a `styleUrls` array must be
    // normalized individually, and a mixed array must preserve relative entries.
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            templateUrl: 'app.component.html',
            styleUrls: ['app.component.scss', './theme.scss'],
        })
        export class AppComponent {}
        ",
    );
    let sources: Vec<&str> = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .map(|i| i.source.as_str())
        .collect();
    assert!(sources.contains(&"./app.component.html"));
    assert!(sources.contains(&"./app.component.scss"));
    assert!(sources.contains(&"./theme.scss"));
    // The already-relative entry must not be double-prefixed.
    assert!(!sources.contains(&".//theme.scss"));
    assert!(!sources.contains(&"././theme.scss"));
}

#[test]
fn angular_component_without_template_url_no_side_effect() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            template: '<h1>Inline</h1>',
        })
        export class App {}
        ",
    );
    let side_effect_count = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .count();
    assert_eq!(side_effect_count, 0);
}

#[test]
fn non_component_decorator_ignored() {
    let info = parse(
        r"
        import { Injectable } from '@angular/core';

        @Injectable()
        export class MyService {}
        ",
    );
    let side_effect_count = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .count();
    assert_eq!(side_effect_count, 0);
}

// ── Angular inline template scanning ──────────────────────────

#[test]
fn angular_inline_template_emits_sentinel_member_accesses() {
    let info = parse(
        r#"
        import { Component, signal } from '@angular/core';

        @Component({
            selector: 'app-inline',
            template: '<p>{{ message() }}</p><button (click)="onClick()">Go</button>',
        })
        export class InlineComponent {
            readonly message = signal('Hello');
            onClick(): void {}
        }
        "#,
    );
    let sentinel = crate::sfc_template::angular::ANGULAR_TPL_SENTINEL;
    let sentinel_refs: Vec<&str> = info
        .member_accesses
        .iter()
        .filter(|a| a.object == sentinel)
        .map(|a| a.member.as_str())
        .collect();
    assert!(sentinel_refs.contains(&"message"));
    assert!(sentinel_refs.contains(&"onClick"));
}

#[test]
fn angular_inline_template_backtick_scanned() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            template: `<h1>{{ title }}</h1>`,
        })
        export class App {
            title = 'Hello';
        }
        ",
    );
    let sentinel = crate::sfc_template::angular::ANGULAR_TPL_SENTINEL;
    let has_title = info
        .member_accesses
        .iter()
        .any(|a| a.object == sentinel && a.member == "title");
    assert!(has_title);
}

#[test]
fn angular_inline_template_no_side_effect_imports() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            template: '<p>{{ value }}</p>',
        })
        export class App {
            value = 42;
        }
        ",
    );
    let side_effects = info
        .imports
        .iter()
        .filter(|i| matches!(i.imported_name, ImportedName::SideEffect))
        .count();
    assert_eq!(side_effects, 0);
}

#[test]
fn angular_inline_template_complexity_anchored_at_decorator() {
    // Inline `template: \`...\`` literals with non-trivial control-flow density
    // emit a synthetic `<template>` finding on the host `.ts` file. The
    // finding's line/col anchors at the `@Component` decorator so existing
    // line-level suppression and jump-to-source land usefully.
    let source = "import { Component } from '@angular/core';\n\
@Component({\n\
  selector: 'host-game',\n\
  template: `\n\
    @if (game(); as g) {\n\
      @if (g.state === 'lobby') {\n\
        <host-lobby [code]=\"g.code\" />\n\
      } @else if (g.state === 'question') {\n\
        @for (player of g.players; track player.id) {\n\
          <player-tile [player]=\"player\" [score]=\"g.scores[player.id] ?? 0\" />\n\
        }\n\
      }\n\
    }\n\
  `,\n\
})\n\
export class HostGameComponent {}\n";
    let info = crate::tests::parse_ts_with_complexity(source);
    let template = info
        .complexity
        .iter()
        .find(|fc| fc.name == "<template>")
        .expect("inline template emits a synthetic <template> finding");
    assert!(
        template.cyclomatic >= 4,
        "control-flow blocks contribute to cyclomatic: {template:?}"
    );
    assert!(
        template.cognitive >= 4,
        "nested control-flow contributes to cognitive: {template:?}"
    );
    // The decorator starts on line 2 (after the import on line 1).
    assert_eq!(template.line, 2, "anchored at @Component line");
    assert_eq!(template.col, 0, "anchored at @ column");
}

#[test]
fn angular_inline_template_with_simple_template_emits_no_finding() {
    // Trivial templates without control-flow should not produce a finding,
    // matching the existing external-template behaviour.
    let info = crate::tests::parse_ts_with_complexity(
        "import { Component } from '@angular/core';\n\
@Component({ selector: 'a', template: '<p>hi</p>' })\n\
export class A {}\n",
    );
    assert!(
        !info.complexity.iter().any(|fc| fc.name == "<template>"),
        "trivial template emits nothing"
    );
}

#[test]
fn angular_template_with_interpolation_expressions_is_skipped() {
    // Template literals with `${...}` expressions are skipped
    // (variable-substituted templates are out of scope for the first cut).
    let info = crate::tests::parse_ts_with_complexity(
        "import { Component } from '@angular/core';\n\
const HEADER = 'h1';\n\
@Component({ selector: 'a', template: `<${HEADER}>x</${HEADER}>` })\n\
export class A {}\n",
    );
    assert!(
        !info.complexity.iter().any(|fc| fc.name == "<template>"),
        "interpolated templates are skipped"
    );
}

// ── Angular host binding detection ────────────────────────────

#[test]
fn angular_host_bindings_emit_sentinel_accesses() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            template: '<p>test</p>',
            host: {
                '[class]': 'hostClass()',
                '[class.is-active]': 'isActive',
                '(click)': 'onHostClick($event)',
                '[style.--custom-color]': 'customColor()',
            },
        })
        export class App {
            hostClass(): string { return 'app'; }
            isActive = true;
            onHostClick(_event: Event): void {}
            customColor(): string { return '#007bff'; }
        }
        ",
    );
    let sentinel = crate::sfc_template::angular::ANGULAR_TPL_SENTINEL;
    let host_refs: Vec<&str> = info
        .member_accesses
        .iter()
        .filter(|a| a.object == sentinel)
        .map(|a| a.member.as_str())
        .collect();
    assert!(host_refs.contains(&"hostClass"));
    assert!(host_refs.contains(&"isActive"));
    assert!(host_refs.contains(&"onHostClick"));
    assert!(host_refs.contains(&"customColor"));
}

#[test]
fn angular_host_binding_skips_keywords() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            template: '',
            host: {
                '[hidden]': 'true',
                '(click)': 'undefined',
            },
        })
        export class App {}
        ",
    );
    let sentinel = crate::sfc_template::angular::ANGULAR_TPL_SENTINEL;
    // "true" and "undefined" are keywords and should be skipped
    assert!(
        info.member_accesses
            .iter()
            .filter(|a| a.object == sentinel)
            .map(|a| a.member.as_str())
            .next()
            .is_none()
    );
}

// ── Angular inputs/outputs metadata arrays ────────────────────

#[test]
fn angular_inputs_outputs_metadata_emit_sentinel_accesses() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({
            selector: 'app-root',
            template: '<p>test</p>',
            inputs: ['bankName', 'id: account-id'],
            outputs: ['clicked'],
        })
        export class App {
            bankName = '';
            id = '';
            clicked = null;
        }
        ",
    );
    let sentinel = crate::sfc_template::angular::ANGULAR_TPL_SENTINEL;
    let refs: Vec<&str> = info
        .member_accesses
        .iter()
        .filter(|a| a.object == sentinel)
        .map(|a| a.member.as_str())
        .collect();
    // 'bankName' from inputs, 'id' from 'id: account-id', 'clicked' from outputs
    assert!(refs.contains(&"bankName"));
    assert!(refs.contains(&"id"));
    assert!(refs.contains(&"clicked"));
}

#[test]
fn angular_queries_metadata_emit_sentinel_accesses() {
    let info = parse(
        r"
        import { Component, ViewChild, ContentChild, ElementRef } from '@angular/core';

        @Component({
            selector: 'app-root',
            template: '<p>test</p>',
            queries: {
                header: null,
                footer: null,
            },
        })
        export class App {
            header: ElementRef;
            footer: ElementRef;
        }
        ",
    );
    let sentinel = crate::sfc_template::angular::ANGULAR_TPL_SENTINEL;
    let refs: Vec<&str> = info
        .member_accesses
        .iter()
        .filter(|a| a.object == sentinel)
        .map(|a| a.member.as_str())
        .collect();
    assert!(refs.contains(&"header"));
    assert!(refs.contains(&"footer"));
}

// ── Angular signal API detection ──────────────────────────────

#[test]
fn angular_signal_input_marks_member_as_decorated() {
    let info = parse(
        r"
        import { Component, input } from '@angular/core';

        @Component({ selector: 'app', template: '' })
        export class App {
            readonly label = input<string>('default');
            readonly required = input.required<number>();
        }
        ",
    );
    let app_export = info
        .exports
        .iter()
        .find(|e| e.name.to_string() == "App")
        .unwrap();
    let label = app_export
        .members
        .iter()
        .find(|m| m.name == "label")
        .unwrap();
    let required = app_export
        .members
        .iter()
        .find(|m| m.name == "required")
        .unwrap();
    assert!(label.has_decorator, "input() should set has_decorator");
    assert!(
        required.has_decorator,
        "input.required() should set has_decorator"
    );
}

#[test]
fn angular_signal_output_model_viewchild_marks_as_decorated() {
    let info = parse(
        r"
        import { Component, output, model, viewChild, contentChild, viewChildren, contentChildren, ElementRef } from '@angular/core';

        @Component({ selector: 'app', template: '' })
        export class App {
            readonly saved = output<void>();
            readonly count = model(0);
            readonly myButton = viewChild<ElementRef>('btn');
            readonly icon = contentChild<ElementRef>('icon');
            readonly items = viewChildren<ElementRef>('item');
            readonly tabs = contentChildren<ElementRef>('tab');
        }
        ",
    );
    let app_export = info
        .exports
        .iter()
        .find(|e| e.name.to_string() == "App")
        .unwrap();
    for member_name in &["saved", "count", "myButton", "icon", "items", "tabs"] {
        let member = app_export
            .members
            .iter()
            .find(|m| m.name == *member_name)
            .unwrap_or_else(|| panic!("member {member_name} not found"));
        assert!(
            member.has_decorator,
            "{member_name} should have has_decorator=true"
        );
    }
}

#[test]
fn angular_signal_apis_not_marked_on_non_angular_class() {
    let info = parse(
        r"
        export class PlainClass {
            readonly label = input<string>('default');
        }
        ",
    );
    let export = info
        .exports
        .iter()
        .find(|e| e.name.to_string() == "PlainClass")
        .unwrap();
    let label = export.members.iter().find(|m| m.name == "label").unwrap();
    assert!(
        !label.has_decorator,
        "signal APIs on non-Angular class should not set has_decorator"
    );
}

#[test]
fn angular_regular_property_not_marked_as_decorated() {
    let info = parse(
        r"
        import { Component } from '@angular/core';

        @Component({ selector: 'app', template: '' })
        export class App {
            regularProp = 'hello';
            anotherProp = 42;
        }
        ",
    );
    let app_export = info
        .exports
        .iter()
        .find(|e| e.name.to_string() == "App")
        .unwrap();
    let regular = app_export
        .members
        .iter()
        .find(|m| m.name == "regularProp")
        .unwrap();
    let another = app_export
        .members
        .iter()
        .find(|m| m.name == "anotherProp")
        .unwrap();
    assert!(
        !regular.has_decorator,
        "regular property should not have has_decorator"
    );
    assert!(
        !another.has_decorator,
        "regular property should not have has_decorator"
    );
}

#[test]
fn angular_output_from_observable_marks_as_decorated() {
    let info = parse(
        r"
        import { Component } from '@angular/core';
        import { outputFromObservable } from '@angular/core/rxjs-interop';
        import { Subject } from 'rxjs';

        @Component({ selector: 'app', template: '' })
        export class App {
            private readonly save$ = new Subject<void>();
            readonly saved = outputFromObservable(this.save$);
        }
        ",
    );
    let app_export = info
        .exports
        .iter()
        .find(|e| e.name.to_string() == "App")
        .unwrap();
    let saved = app_export
        .members
        .iter()
        .find(|m| m.name == "saved")
        .unwrap();
    assert!(
        saved.has_decorator,
        "outputFromObservable() should set has_decorator"
    );
}

// ── Angular combined metadata extraction ──────────────────────

#[test]
fn angular_component_all_metadata_combined() {
    let info = parse(
        r"
        import { Component, input, output } from '@angular/core';

        @Component({
            selector: 'app-root',
            templateUrl: './app.html',
            styleUrl: './app.scss',
            template: '<p>{{ greeting() }}</p>',
            host: {
                '(click)': 'handleClick()',
            },
            inputs: ['externalInput'],
            outputs: ['externalOutput'],
        })
        export class App {
            readonly name = input<string>();
            readonly saved = output<void>();
            greeting(): string { return 'hi'; }
            handleClick(): void {}
            externalInput = '';
            externalOutput = null;
        }
        ",
    );
    // templateUrl creates side-effect import
    let has_html_import = info
        .imports
        .iter()
        .any(|i| i.source == "./app.html" && matches!(i.imported_name, ImportedName::SideEffect));
    assert!(has_html_import);

    // Sentinel accesses from inline template + host + inputs/outputs
    let sentinel = crate::sfc_template::angular::ANGULAR_TPL_SENTINEL;
    let refs: Vec<&str> = info
        .member_accesses
        .iter()
        .filter(|a| a.object == sentinel)
        .map(|a| a.member.as_str())
        .collect();
    assert!(refs.contains(&"greeting")); // from inline template
    assert!(refs.contains(&"handleClick")); // from host
    assert!(refs.contains(&"externalInput")); // from inputs
    assert!(refs.contains(&"externalOutput")); // from outputs

    // Signal APIs mark members as decorated
    let app_export = info
        .exports
        .iter()
        .find(|e| e.name.to_string() == "App")
        .unwrap();
    let name_member = app_export
        .members
        .iter()
        .find(|m| m.name == "name")
        .unwrap();
    let saved_member = app_export
        .members
        .iter()
        .find(|m| m.name == "saved")
        .unwrap();
    assert!(name_member.has_decorator);
    assert!(saved_member.has_decorator);
}
