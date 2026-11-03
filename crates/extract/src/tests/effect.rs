//! Effect facts: tagged error classes, caught tags, literal tags and the
//! layer graph that one module contributes.

use fallow_types::extract::EffectFacts;

use super::parse_ts;

fn facts(source: &str) -> EffectFacts {
    *parse_ts(source)
        .effect_facts
        .expect("an Effect module should record facts")
}

#[test]
fn module_without_an_effect_import_records_nothing() {
    let info = parse_ts(
        "class NotFound extends Data.TaggedError(\"NotFound\") {}\nexport const x = Layer.succeed(1)",
    );
    assert!(info.effect_facts.is_none());
}

#[test]
fn records_every_tagged_error_constructor_shape() {
    let facts = facts(
        r#"
import { Data, Schema } from "effect"
export class A extends Data.TaggedError("A")<{ readonly id: string }> {}
export class B extends Schema.TaggedError<B>()("B", { reason: Schema.String }) {}
export class C extends Schema.TaggedError<C>("app/C")("C", {}) {}
export class D extends Schema.TaggedErrorClass<D>()("D", {}) {}
const tag = "E"
export class E extends Data.TaggedError(tag) {}
"#,
    );
    let errors: Vec<(&str, &str)> = facts
        .tagged_errors
        .iter()
        .map(|error| (error.name.as_str(), error.tag.as_str()))
        .collect();
    assert_eq!(errors, vec![("A", "A"), ("B", "B"), ("C", "C"), ("D", "D")]);
}

#[test]
fn separates_value_and_type_references_and_ignores_the_own_declaration() {
    let facts = facts(
        r#"
import { Effect, Schema } from "effect"
export class Typed extends Schema.TaggedError<Typed>()("Typed", {}) {}
export class Built extends Schema.TaggedError<Built>()("Built", {}) {}
export const run = (): Effect.Effect<void, Typed | Built> => Effect.fail(new Built())
"#,
    );
    let typed = &facts.tagged_errors[0];
    assert!(typed.local_type_reference);
    assert!(!typed.local_value_reference);
    let built = &facts.tagged_errors[1];
    assert!(built.local_value_reference);
}

#[test]
fn records_catch_tag_forms_and_literal_tags() {
    let facts = facts(
        r#"
import { Effect, Schema, Data } from "effect"
export const a = (self: any) => self.pipe(
  Effect.catchTag("One", () => Effect.void),
  Effect.catchTag(["Two", "Three"], () => Effect.void),
  Effect.catchTags({ Four: () => Effect.void, "Five": () => Effect.void }),
)
export const b = (self: any) => Effect.catchTag(self, "Six", () => Effect.void)
export const literal = { _tag: "Seven" }
export const Shape = Schema.TaggedStruct("Eight", {})
export const tagged = Data.tagged("Nine")
"#,
    );
    let caught: Vec<&str> = facts
        .caught_tags
        .iter()
        .map(|caught| caught.tag.as_str())
        .collect();
    assert_eq!(caught, vec!["One", "Two", "Three", "Four", "Five", "Six"]);
    assert_eq!(facts.literal_tags, vec!["Eight", "Nine", "Seven"]);
}

#[test]
fn builds_layer_nodes_dependencies_and_roots() {
    let facts = facts(
        r#"
import { Context, Effect, Layer } from "effect"
import { Remote as RemoteAlias } from "./remote"
export class Db extends Context.Service<Db, {}>()("Db") {
  static readonly layer = Layer.succeed(this, {})
  static readonly Live = this.layer.pipe(Layer.provide(RemoteAlias.layer))
}
const DbLive = Db.layer.pipe(Layer.provide(RemoteAlias.layer))
export const AppLive = Layer.mergeAll(DbLive)
const main = Layer.launch(AppLive)
Effect.runFork(main)
"#,
    );
    let layers: Vec<(&str, Vec<&str>)> = facts
        .layers
        .iter()
        .map(|layer| {
            (
                layer.key.as_str(),
                layer.dependencies.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        layers,
        vec![
            ("Db.layer", vec![]),
            ("Db.Live", vec!["Db.layer", "Remote.layer"]),
            ("DbLive", vec!["Db.layer", "Remote.layer"]),
            ("AppLive", vec!["DbLive"]),
        ],
    );
    assert!(
        facts.layer_root_keys.iter().any(|key| key == "AppLive"),
        "a Layer.launch argument outside a layer is a root: {:?}",
        facts.layer_root_keys
    );
    assert!(
        !facts.layer_root_keys.iter().any(|key| key == "DbLive"),
        "a reference inside a layer is an edge, not a root"
    );
}

#[test]
fn literal_tags_unwrap_type_wrappers_and_read_class_fields() {
    let facts = facts(
        r#"
import { Effect } from "effect"
export const a = { _tag: "AsConst" as const }
export const b = { _tag: "Satisfied" satisfies string }
export class Manual {
  readonly _tag = "ClassField"
}
export const handled = (self: any) => self.pipe(Effect.catchTag("AsConst", () => Effect.void))
"#,
    );
    assert_eq!(
        facts.literal_tags,
        vec!["AsConst", "ClassField", "Satisfied"]
    );
}

#[test]
fn default_exported_error_is_no_candidate_and_default_layer_is_a_root() {
    let error_facts = facts(
        r#"
import { Data, Effect } from "effect"
class Hidden extends Data.TaggedError("Hidden")<{}> {}
export const handled = (self: any) => self.pipe(Effect.catchTag("Hidden", () => Effect.void))
export default Hidden
"#,
    );
    assert!(error_facts.tagged_errors.is_empty());
    let layer_facts = super::parse_ts(
        r#"
import { Layer } from "effect"
const AppLive = Layer.empty
const RootLive = Layer.mergeAll(AppLive)
export default RootLive
"#,
    )
    .effect_facts
    .expect("an Effect module should record facts");
    assert!(
        layer_facts
            .layer_root_keys
            .iter()
            .any(|key| key == "RootLive")
    );
}

#[test]
fn caught_tag_anchor_points_at_the_tag_text() {
    let source = "import { Effect } from \"effect\"\nexport const x = Effect.catchTag(\"Gone\", () => Effect.void)\nexport const y = Effect.catchTags({ \"Quoted\": () => Effect.void, Plain: () => Effect.void })\n";
    let facts = facts(source);
    assert_eq!(facts.caught_tags.len(), 3);
    for caught in &facts.caught_tags {
        let start = caught.span_start as usize;
        assert_eq!(&source[start..start + caught.tag.len()], caught.tag);
    }
}

fn schema_exports(source: &str) -> Vec<String> {
    super::parse_ts(source)
        .effect_facts
        .map(|facts| facts.schema_exports)
        .unwrap_or_default()
}

#[test]
fn records_schema_class_exports_for_every_class_constructor() {
    let names = schema_exports(
        r#"
import { Schema } from "effect"
export class V3Class extends Schema.Class<V3Class>("V3Class")({ id: Schema.String }) {
  get label() { return this.id }
}
export class V3Tagged extends Schema.TaggedClass<V3Tagged>()("V3Tagged", {}) {}
export class V3Error extends Schema.TaggedError<V3Error>()("V3Error", {}) {}
export class V3Request extends Schema.TaggedRequest<V3Request>()("V3Request", {
  failure: Schema.Never,
  success: Schema.Void,
  payload: {},
}) {}
export class V4Error extends Schema.Error<V4Error>("V4Error")({}) {}
export class V4Opaque extends Schema.Opaque<V4Opaque>()(Schema.Struct({ a: Schema.String })) {}
export class Extended extends V3Class.extend<Extended>("Extended")({ more: Schema.Number }) {}
class Plain {}
export class NotSchema extends Plain {}
"#,
    );
    assert_eq!(
        names,
        vec![
            "Extended",
            "V3Class",
            "V3Error",
            "V3Request",
            "V3Tagged",
            "V4Error",
            "V4Opaque",
        ]
    );
}

#[test]
fn records_schema_const_exports_and_pipe_forms() {
    let names = schema_exports(
        r#"
import { Schema } from "effect"
export const User = Schema.Struct({ id: Schema.String })
export const UserId = Schema.String.pipe(Schema.brand("UserId"))
export const Name = Schema.String
export const Annotated = Schema.Struct({}).annotations({ title: "A" })
export const Tree = Schema.suspend(() => Name)
export const Admin = User.pipe(Schema.extend(Schema.Struct({ admin: Schema.Boolean })))
const Hidden = Schema.Number
export { Hidden as Renamed }
const DefaultSchema = Schema.Literal("a")
export default DefaultSchema
"#,
    );
    assert_eq!(
        names,
        vec![
            "Admin",
            "Annotated",
            "Name",
            "Renamed",
            "Tree",
            "User",
            "UserId",
            "default",
        ]
    );
}

#[test]
fn ignores_values_that_a_schema_function_returns() {
    let names = schema_exports(
        r#"
import { Schema } from "effect"
const User = Schema.Struct({ id: Schema.String })
export const decodeUser = Schema.decodeUnknownSync(User)
export const isUser = Schema.is(User)
export const sample = User.make({ id: "1" })
export const fields = User.fields
export const positive = Schema.isGreaterThan(0)
export const plain = 1
export const SchemaAlias = Schema
"#,
    );
    assert!(names.is_empty(), "no export is a schema: {names:?}");
}

#[test]
fn accepts_every_effect_schema_import_form() {
    let named_alias =
        schema_exports("import { Schema as S } from \"effect\"\nexport const A = S.Struct({})\n");
    assert_eq!(named_alias, vec!["A"]);
    let v4_namespace = schema_exports(
        "import * as Schema from \"effect/Schema\"\nexport const B = Schema.String\n",
    );
    assert_eq!(v4_namespace, vec!["B"]);
    let legacy_named = schema_exports(
        "import { Schema } from \"@effect/schema\"\nexport const C = Schema.String\n",
    );
    assert_eq!(legacy_named, vec!["C"]);
    let legacy_namespace = schema_exports(
        "import * as S from \"@effect/schema/Schema\"\nexport class D extends S.Class<D>(\"D\")({}) {}\n",
    );
    assert_eq!(legacy_namespace, vec!["D"]);
}

#[test]
fn schema_name_that_does_not_bind_to_effect_is_no_schema() {
    let local_object = schema_exports(
        r#"
import { Effect } from "effect"
const Schema = { Struct: (fields: object) => fields, String: "string" }
export const NotEffect = Schema.Struct({})
export const NotEffectMember = Schema.String
export const run = Effect.void
"#,
    );
    assert!(local_object.is_empty(), "{local_object:?}");
    let other_package = schema_exports(
        "import { Effect } from \"effect\"\nimport { Schema } from \"other-schema\"\nexport const Other = Schema.Struct({})\nexport const run = Effect.void\n",
    );
    assert!(other_package.is_empty(), "{other_package:?}");
    let shadowed = schema_exports(
        "import { Schema } from \"effect\"\nexport const make = (Schema: any) => Schema.Struct({})\nexport const Real = Schema.String\n",
    );
    assert_eq!(
        shadowed,
        vec!["Real"],
        "a parameter named Schema is no import"
    );
}

#[test]
fn records_default_exported_schema_classes() {
    let anonymous = schema_exports(
        "import { Schema } from \"effect\"\nexport default class extends Schema.Class<any>(\"A\")({}) {}\n",
    );
    assert_eq!(anonymous, vec!["default"]);
    let named = schema_exports(
        "import { Schema } from \"effect\"\nexport default class Named extends Schema.TaggedClass<Named>()(\"Named\", {}) {}\n",
    );
    assert_eq!(named, vec!["default"]);
    let plain = schema_exports(
        "import { Effect } from \"effect\"\nexport default class extends Error {}\nexport const run = Effect.void\n",
    );
    assert!(plain.is_empty(), "{plain:?}");
}

#[test]
fn pipe_arguments_must_be_schema_transforms() {
    let names = schema_exports(
        r#"
import { Schema } from "effect"
const User = Schema.Struct({ id: Schema.String })
export const DecodeSync = Schema.Struct({}).pipe(Schema.decodeUnknownSync)
export const DecodeEffect = Schema.Struct({}).pipe(Schema.decodeUnknown)
export const Arrow = Schema.Struct({}).pipe((s) => Schema.decodeUnknownSync(s))
export const ClassFactory = Schema.Struct({}).pipe(Schema.Class<any>("x"))
export const LocalDecode = User.pipe(Schema.decodeUnknownSync)
export const MixedArguments = Schema.String.pipe(Schema.minLength(1), Schema.decodeUnknownSync)
export const Branded = Schema.String.pipe(Schema.minLength(1), Schema.brand("Name"))
export const Nullable = Schema.String.pipe(Schema.NullOr)
export const Checked = Schema.String.pipe(Schema.check(Schema.isMinLength(1)))
export const Decoded = Schema.String.pipe(Schema.decodeTo(Schema.Number))
export const Admin = User.pipe(Schema.extend(Schema.Struct({ admin: Schema.Boolean })))
"#,
    );
    assert_eq!(
        names,
        vec!["Admin", "Branded", "Checked", "Decoded", "Nullable"],
        "a pipe is a schema only when every argument is a schema transform"
    );
}

#[test]
fn type_only_schema_names_are_no_constructor() {
    let names = schema_exports(
        r#"
import { Schema } from "effect"
export const SchemaInterface = Schema.Schema
export const TopInterface = Schema.Top
export const Real = Schema.String
"#,
    );
    assert_eq!(names, vec!["Real"]);
}

#[test]
fn records_a_default_exported_schema_expression() {
    let schema = schema_exports(
        "import { Schema } from \"effect\"\nexport default Schema.Struct({ id: Schema.String })\n",
    );
    assert_eq!(schema, vec!["default"]);
    let decoder = schema_exports(
        "import { Schema } from \"effect\"\nconst User = Schema.Struct({})\nexport default Schema.decodeUnknownSync(User)\n",
    );
    assert!(decoder.is_empty(), "{decoder:?}");
}

#[test]
fn records_chains_that_start_at_an_import_as_derived_schemas() {
    let facts = facts(
        r#"
import { Schema } from "effect"
import { Rule, helper } from "./contracts"
import type { TypeOnly } from "./contracts"
export const Derived = Rule.pipe(Schema.decodeTo(Schema.String))
const Step = Rule.pipe(Schema.brand("Step"))
export const DerivedTwice = Step.annotations({})
export class DerivedClass extends Rule.extend<DerivedClass>("DerivedClass")({}) {}
export const NotSchemaPipe = helper.pipe((value: unknown) => value)
export const Made = Rule.make({})
export const FromType = TypeOnly
export const Own = Schema.String
"#,
    );
    let derived: Vec<(&str, &str)> = facts
        .derived_schema_exports
        .iter()
        .map(|derived| (derived.export_name.as_str(), derived.import_local.as_str()))
        .collect();
    assert_eq!(
        derived,
        vec![
            ("Derived", "Rule"),
            ("DerivedTwice", "Rule"),
            ("DerivedClass", "Rule"),
        ]
    );
    assert_eq!(facts.schema_exports, vec!["Own"]);
}
