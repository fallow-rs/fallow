import { Schema } from "effect";

export class ForwardedError extends Schema.TaggedError<ForwardedError>()("ForwardedError", {
  message: Schema.String,
}) {}

export const ForwardedUnion = Schema.Union(Schema.String, Schema.Number);

export const BaseRule = Schema.Struct({ key: Schema.String });

export const decodeBaseRule = Schema.decodeUnknownSync(BaseRule);

export const plainValue = { pipe: (...fns: unknown[]): unknown => fns };
