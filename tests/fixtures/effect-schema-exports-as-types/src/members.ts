import * as Schema from "effect/Schema";

const LocalId = Schema.String;

export const MemberOptional = Schema.optional(Schema.String);

export const MemberOptionalImported = Schema.optional(LocalId.annotate({ description: "x" }));

export const MemberOptionalKey = Schema.optionalKey(Schema.String);

export const memberBrandId = Schema.BrandSchemaId;
