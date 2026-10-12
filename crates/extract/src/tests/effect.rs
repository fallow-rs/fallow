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
