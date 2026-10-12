import { Data, Schema } from "effect"

export class NotFound extends Data.TaggedError("NotFound")<{ readonly id: string }> {}

export class Forbidden extends Schema.TaggedError<Forbidden>()("Forbidden", {
  reason: Schema.String,
}) {}

export class NeverThrown extends Data.TaggedError("NeverThrown")<{}> {}

export class DecodedOnly extends Schema.TaggedError<DecodedOnly>()("DecodedOnly", {}) {}

export const ApiErrors = Schema.Union(DecodedOnly)

export class ViaNamespace extends Data.TaggedError("ViaNamespace")<{}> {}

// fallow-ignore-next-line unused-tagged-error
export class Silenced extends Data.TaggedError("Silenced")<{}> {}

export class PublicError extends Data.TaggedError("PublicError")<{}> {}

export type PublicFailure = PublicError
