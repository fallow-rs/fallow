//! `Schema` export names from the Effect typings, used by the shape rules in
//! `effect_schema.rs`.
//!
//! `scripts/effect-schema-members.mjs` classifies each value export of
//! `effect/Schema` from its declared type. Its snapshots for effect 3.22.5
//! and effect 4.1.5 are in `crates/extract/tests/effect_schema_members/`, and
//! the test `effect_schema_member_snapshots` checks these lists against them.
//! A member that gives a schema in one of the two versions is in the lists
//! (`optional` gives a property signature in effect 3 and a schema in
//! effect 4).

/// `Schema` functions with a lowercase name whose call gives a schema. Every
/// other lowercase function (`decodeUnknownSync`, `is`, `isGreaterThan`)
/// gives a decoder, a guard or a filter.
pub(super) const LOWERCASE_SCHEMA_CONSTRUCTORS: &[&str] = &[
    "annotations",
    "asSchema",
    "attachPropertySignature",
    "compose",
    "declare",
    "encodedBoundSchema",
    "encodedSchema",
    "exitSchema",
    "extend",
    "failureSchema",
    "filterEffect",
    "flip",
    "fromFormData",
    "fromJsonString",
    "fromURLSearchParams",
    "getNumberIndexedAccess",
    "head",
    "headNonEmpty",
    "headOrElse",
    "instanceOf",
    "keyof",
    "make",
    "mutable",
    "mutableKey",
    "optional",
    "optionalKey",
    "parseJson",
    "parseNumber",
    "partial",
    "partialWith",
    "pluck",
    "readonlyKey",
    "rename",
    "required",
    "requiredKey",
    "revealBottom",
    "revealCodec",
    "serializableSchema",
    "split",
    "standardSchemaV1",
    "successSchema",
    "suspend",
    "tag",
    "tagDefaultOmit",
    "toCodecArrayFromSingle",
    "toCodecGoJson",
    "toCodecIso",
    "toCodecJson",
    "toCodecStringTree",
    "toEncoded",
    "toStandardJSONSchemaV1",
    "toStandardSchemaV1",
    "toType",
    "transform",
    "transformLiteral",
    "transformLiterals",
    "transformOrFail",
    "typeSchema",
];

/// Dual `Schema` functions of effect 3 with the fewest arguments of the
/// data-first form. A call with fewer arguments is the data-last form and
/// gives a function (`Schema.annotations({ title: "x" })`), so it is not a
/// schema. A call with a spread argument counts as a schema.
pub(super) const DATA_FIRST_ARITY: &[(&str, usize)] = &[
    ("annotations", 2),
    ("attachPropertySignature", 3),
    ("compose", 2),
    ("extend", 2),
    ("filterEffect", 2),
    ("headOrElse", 1),
    ("partialWith", 2),
    ("pluck", 2),
    ("rename", 2),
    ("transform", 3),
    ("transformOrFail", 3),
];

/// PascalCase `Schema` values that are not a schema in effect 3.22.5 and
/// effect 4.1.5: annotation ids such as `BrandSchemaId`, `TypeId`, `Config`
/// and `SchemaError`. The class factories (`Class`, `TaggedError`) are not
/// here, because a call chain of a factory gives a schema class.
pub(super) const NON_SCHEMA_VALUE_NAMES: &[&str] = &[
    "BetweenBigDecimalSchemaId",
    "BetweenBigIntSchemaId",
    "BetweenDateSchemaId",
    "BetweenDurationSchemaId",
    "BetweenSchemaId",
    "BrandSchemaId",
    "CapitalizedSchemaId",
    "Config",
    "DateFromSelfSchemaId",
    "EndsWithSchemaId",
    "FiniteSchemaId",
    "FromPropertySignature",
    "GreaterThanBigDecimalSchemaId",
    "GreaterThanBigIntSchemaId",
    "GreaterThanDateSchemaId",
    "GreaterThanDurationSchemaId",
    "GreaterThanOrEqualToBigDecimalSchemaId",
    "GreaterThanOrEqualToBigIntSchemaId",
    "GreaterThanOrEqualToDateSchemaId",
    "GreaterThanOrEqualToDurationSchemaId",
    "GreaterThanOrEqualToSchemaId",
    "GreaterThanSchemaId",
    "IncludesSchemaId",
    "InstanceOfSchemaId",
    "IntSchemaId",
    "ItemsCountSchemaId",
    "JsonNumberSchemaId",
    "LengthSchemaId",
    "LessThanBigDecimalSchemaId",
    "LessThanBigIntSchemaId",
    "LessThanDateSchemaId",
    "LessThanDurationSchemaId",
    "LessThanOrEqualToBigDecimalSchemaId",
    "LessThanOrEqualToBigIntSchemaId",
    "LessThanOrEqualToDateSchemaId",
    "LessThanOrEqualToDurationSchemaId",
    "LessThanOrEqualToSchemaId",
    "LessThanSchemaId",
    "LowercasedSchemaId",
    "MaxItemsSchemaId",
    "MaxLengthSchemaId",
    "MinItemsSchemaId",
    "MinLengthSchemaId",
    "MultipleOfSchemaId",
    "NegativeBigDecimalSchemaId",
    "NonNaNSchemaId",
    "NonNegativeBigDecimalSchemaId",
    "NonPositiveBigDecimalSchemaId",
    "PatternSchemaId",
    "PositiveBigDecimalSchemaId",
    "PropertySignatureDeclaration",
    "PropertySignatureTransformation",
    "PropertySignatureTypeId",
    "RefineSchemaId",
    "SchemaError",
    "StartsWithSchemaId",
    "ToPropertySignature",
    "TrimmedSchemaId",
    "TypeId",
    "ULIDSchemaId",
    "UUIDSchemaId",
    "UncapitalizedSchemaId",
    "UppercasedSchemaId",
    "ValidDateSchemaId",
];

/// `Schema` functions that `pipe` can call to get a schema: the data-last
/// form returns a schema (filters, `brand`, `extend`, `check`, `decodeTo`).
/// The list leaves out `decode` and `encode`: `pipe` takes their bare read
/// too, and a bare read gives a decoder in both versions. It leaves out the
/// class factory `Opaque` for the same reason.
pub(super) const PIPE_TRANSFORMS: &[&str] = &[
    "annotate",
    "annotateEncoded",
    "annotateKey",
    "annotations",
    "attachPropertySignature",
    "between",
    "betweenBigDecimal",
    "betweenBigInt",
    "betweenDate",
    "betweenDuration",
    "brand",
    "capitalized",
    "catchDecoding",
    "catchDecodingWithContext",
    "catchEncoding",
    "catchEncodingWithContext",
    "check",
    "clamp",
    "clampBigDecimal",
    "clampBigInt",
    "clampDuration",
    "compose",
    "decodeTo",
    "encodeKeys",
    "encodeTo",
    "endsWith",
    "extend",
    "extendTo",
    "fieldsAssign",
    "filter",
    "filterEffect",
    "finite",
    "fromBrand",
    "greaterThan",
    "greaterThanBigDecimal",
    "greaterThanBigInt",
    "greaterThanDate",
    "greaterThanDuration",
    "greaterThanOrEqualTo",
    "greaterThanOrEqualToBigDecimal",
    "greaterThanOrEqualToBigInt",
    "greaterThanOrEqualToDate",
    "greaterThanOrEqualToDuration",
    "headOrElse",
    "includes",
    "int",
    "itemsCount",
    "length",
    "lessThan",
    "lessThanBigDecimal",
    "lessThanBigInt",
    "lessThanDate",
    "lessThanDuration",
    "lessThanOrEqualTo",
    "lessThanOrEqualToBigDecimal",
    "lessThanOrEqualToBigInt",
    "lessThanOrEqualToDate",
    "lessThanOrEqualToDuration",
    "lowercased",
    "maxItems",
    "maxLength",
    "middlewareDecoding",
    "middlewareEncoding",
    "minItems",
    "minLength",
    "multipleOf",
    "mutable",
    "mutableKey",
    "negative",
    "negativeBigDecimal",
    "negativeBigInt",
    "nonEmptyString",
    "nonNaN",
    "nonNegative",
    "nonNegativeBigDecimal",
    "nonNegativeBigInt",
    "nonPositive",
    "nonPositiveBigDecimal",
    "nonPositiveBigInt",
    "omit",
    "optional",
    "optionalKey",
    "overrideToCodecIso",
    "overrideToEquivalence",
    "overrideToFormatter",
    "partialWith",
    "pattern",
    "pick",
    "pickLiteral",
    "pluck",
    "positive",
    "positiveBigDecimal",
    "positiveBigInt",
    "readonlyKey",
    "refine",
    "rename",
    "required",
    "requiredKey",
    "startsWith",
    "toEncoded",
    "toTaggedUnion",
    "toType",
    "transform",
    "transformOrFail",
    "trimmed",
    "uncapitalized",
    "uppercased",
    "validDate",
    "withConstructorDefault",
    "withDecodingDefault",
    "withDecodingDefaultKey",
    "withDecodingDefaultType",
    "withDecodingDefaultTypeKey",
];

/// `HttpApiSchema` functions whose call gives a function from a schema to a
/// schema, for `pipe`. The list comes from the typings of `@effect/platform`
/// 0.97.2 (`asEmpty`, `withEncoding`) and effect 4.1.3. It leaves out
/// `annotations` of `@effect/platform`, which returns an annotations object.
pub(super) const HTTP_API_PIPE_TRANSFORMS: &[&str] = &[
    "asEmpty",
    "asFormUrlEncoded",
    "asJson",
    "asMultipart",
    "asMultipartStream",
    "asNoContent",
    "asText",
    "asUint8Array",
    "status",
    "withEncoding",
];

/// PascalCase `Schema` names that are only a type or an interface in
/// effect 3.22.4 and effect 4.0.5, such as `Schema.Schema` and `Schema.Top`.
pub(super) const TYPE_ONLY_NAMES: &[&str] = &[
    "Annotable",
    "AnnotableClass",
    "AnnotableDeclare",
    "Annotations",
    "Bottom",
    "BottomLazy",
    "BottomLazyWithoutNew",
    "BottomWithoutNew",
    "BrandSchema",
    "CauseEncoded",
    "CauseIso",
    "CauseReasonIso",
    "ChunkIso",
    "Codec",
    "Constraint",
    "ConstraintCodec",
    "ConstraintDecoder",
    "ConstraintEncoder",
    "ConstraintRebuildable",
    "ConstructorDefault",
    "Decoder",
    "DecodingDefaultOptions",
    "DurationEncoded",
    "EitherEncoded",
    "Element",
    "EncodedGraph",
    "Encoder",
    "EnumsDefinition",
    "ErrorOptions",
    "ExitEncoded",
    "ExitIso",
    "FiberIdEncoded",
    "FilterIssue",
    "FilterOutput",
    "GraphIso",
    "HashMapIso",
    "HashSetIso",
    "IndexSignature",
    "JsonArray",
    "LeftEncoded",
    "MakeOptions",
    "Mutability",
    "MutableJsonArray",
    "MutableJsonObject",
    "Optic",
    "OptionEncoded",
    "OptionIso",
    "OptionalOptions",
    "Optionality",
    "ParseJsonOptions",
    "PropertySignature",
    "ReadonlyMapIso",
    "ReadonlySetIso",
    "ResultIso",
    "RightEncoded",
    "Schema",
    "SchemaClass",
    "Serializable",
    "SerializableWithResult",
    "Simplify",
    "SimplifyMutable",
    "StringTree",
    "TaggedErrorClass",
    "TaggedRequestClass",
    "ToJsonSchemaOptions",
    "Top",
    "TreeRecord",
    "Tuple2",
    "TupleType",
    "TypeLiteral",
    "WithResult",
    "WithoutConstructorDefault",
];

#[cfg(test)]
mod effect_schema_member_snapshots {
    use rustc_hash::FxHashMap;

    use super::super::{CLASS_FACTORIES, PIPE_CONSTRUCTORS};
    use super::{
        DATA_FIRST_ARITY, LOWERCASE_SCHEMA_CONSTRUCTORS, NON_SCHEMA_VALUE_NAMES, PIPE_TRANSFORMS,
        TYPE_ONLY_NAMES,
    };

    const SNAPSHOTS: &[&str] = &[
        include_str!("../tests/effect_schema_members/effect-3.22.5.json"),
        include_str!("../tests/effect_schema_members/effect-4.1.5.json"),
    ];

    /// Transforms that `PIPE_TRANSFORMS` leaves out on purpose: `pipe` takes a
    /// bare read of each list member, and a bare read of these gives a
    /// decoder or a class factory.
    const PIPE_TRANSFORM_EXCLUSIONS: &[&str] = &["Opaque", "decode", "encode"];

    /// Lines of a snapshot around its member lines: the opening brace, the
    /// `effect`, `typescript` and `members` keys, and two closing braces.
    const FRAME_LINES: usize = 6;

    /// The kinds of each member of one snapshot. Each line between the frame
    /// lines must be a member, so a change of the format fails the test.
    fn snapshot_members(snapshot: &'static str) -> Vec<(&'static str, Vec<&'static str>)> {
        let lines: Vec<&str> = snapshot.lines().collect();
        assert!(lines.len() > FRAME_LINES, "a snapshot has member lines");
        assert_eq!(lines[3].trim(), "\"members\": {", "the members key");
        let members: Vec<_> = lines[4..lines.len() - 2]
            .iter()
            .map(|line| {
                let (name, kinds) = line
                    .trim()
                    .strip_prefix('"')
                    .and_then(|line| line.split_once("\": ["))
                    .unwrap_or_else(|| panic!("not a member line: {line}"));
                let kinds = kinds.trim_end_matches(',').strip_suffix(']');
                let kinds = kinds.unwrap_or_else(|| panic!("not a member line: {line}"));
                (
                    name,
                    kinds
                        .split(", ")
                        .map(|kind| kind.trim_matches('"'))
                        .collect(),
                )
            })
            .collect();
        assert!(members.len() > 300, "a snapshot lists each member");
        members
    }

    /// The kinds of each member, over all snapshots.
    fn members() -> FxHashMap<&'static str, Vec<&'static str>> {
        let mut members: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
        for snapshot in SNAPSHOTS {
            for (name, kinds) in snapshot_members(snapshot) {
                members.entry(name).or_default().extend(kinds);
            }
        }
        members
    }

    #[test]
    fn data_first_arity_matches_the_dual_members() {
        // A member needs an arity when each snapshot that lets its call give a
        // schema marks the member dual.
        let mut arities: FxHashMap<&str, Option<usize>> = FxHashMap::default();
        for snapshot in SNAPSHOTS {
            for (name, kinds) in snapshot_members(snapshot) {
                if !kinds.contains(&"call") {
                    continue;
                }
                let arity = kinds
                    .iter()
                    .find_map(|kind| kind.strip_prefix("arity:"))
                    .map(|arity| arity.parse::<usize>().expect("a number"));
                let entry = arities.entry(name).or_insert(arity);
                if arity.is_none() {
                    *entry = None;
                }
            }
        }
        let mut expected: Vec<(&str, usize)> = arities
            .into_iter()
            .filter(|(name, _)| name.starts_with(|c: char| c.is_ascii_lowercase()))
            .filter_map(|(name, arity)| arity.map(|arity| (name, arity)))
            .collect();
        expected.sort_unstable();
        assert_eq!(DATA_FIRST_ARITY, expected.as_slice());
    }

    fn gives_schema(kinds: &[&str]) -> bool {
        kinds.iter().any(|kind| matches!(*kind, "call" | "read"))
    }

    fn sorted(mut names: Vec<&str>) -> Vec<&str> {
        names.sort_unstable();
        names
    }

    #[test]
    fn lowercase_constructors_match_the_members_whose_call_gives_a_schema() {
        let members = members();
        let missing = sorted(
            members
                .iter()
                .filter(|(name, kinds)| {
                    name.starts_with(|c: char| c.is_ascii_lowercase())
                        && gives_schema(kinds)
                        && !LOWERCASE_SCHEMA_CONSTRUCTORS.contains(name)
                })
                .map(|(name, _)| *name)
                .collect(),
        );
        assert!(missing.is_empty(), "missing constructors: {missing:?}");
        let extra = sorted(
            LOWERCASE_SCHEMA_CONSTRUCTORS
                .iter()
                .copied()
                .filter(|name| !members.get(name).is_some_and(|kinds| gives_schema(kinds)))
                .collect(),
        );
        assert!(
            extra.is_empty(),
            "constructors that give no schema: {extra:?}"
        );
    }

    #[test]
    fn pascal_case_values_that_give_no_schema_are_listed() {
        let members = members();
        let accepted_non_schemas = sorted(
            members
                .iter()
                .filter(|(name, kinds)| {
                    name.starts_with(|c: char| c.is_ascii_uppercase())
                        && !gives_schema(kinds)
                        && !CLASS_FACTORIES.contains(name)
                        && !NON_SCHEMA_VALUE_NAMES.contains(name)
                })
                .map(|(name, _)| *name)
                .collect(),
        );
        assert!(
            accepted_non_schemas.is_empty(),
            "values that give no schema: {accepted_non_schemas:?}"
        );
        let rejected_schemas = sorted(
            NON_SCHEMA_VALUE_NAMES
                .iter()
                .chain(TYPE_ONLY_NAMES)
                .copied()
                .filter(|name| members.get(name).is_some_and(|kinds| gives_schema(kinds)))
                .collect(),
        );
        assert!(
            rejected_schemas.is_empty(),
            "listed names that give a schema: {rejected_schemas:?}"
        );
    }

    #[test]
    fn pipe_lists_match_the_members_that_map_a_schema() {
        let members = members();
        let missing = sorted(
            members
                .iter()
                .filter(|(name, kinds)| {
                    kinds.contains(&"transform")
                        && !PIPE_TRANSFORMS.contains(name)
                        && !PIPE_TRANSFORM_EXCLUSIONS.contains(name)
                })
                .map(|(name, _)| *name)
                .collect(),
        );
        assert!(missing.is_empty(), "missing transforms: {missing:?}");
        let extra = sorted(
            PIPE_TRANSFORMS
                .iter()
                .chain(PIPE_CONSTRUCTORS)
                .copied()
                .filter(|name| {
                    !members.get(name).is_some_and(|kinds| {
                        kinds
                            .iter()
                            .any(|kind| matches!(*kind, "call" | "transform"))
                    })
                })
                .collect(),
        );
        assert!(extra.is_empty(), "pipe names that map no schema: {extra:?}");
    }
}
